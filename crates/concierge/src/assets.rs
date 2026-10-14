use std::{collections::HashMap, fs, path::Path};

use anyhow::Context;
use axum::{
    body::Bytes,
    http::header,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

pub const URL_PREFIX: &str = "/assets/";
/// esbuild's metafile, written next to the files it describes.
const MANIFEST: &str = "manifest.json";
/// Every file carries its content hash in its name.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// The stylesheets and images esbuild built for concierge, held in memory.
pub struct Assets {
    urls: HashMap<String, String>,
    files: HashMap<String, File>,
}

struct File {
    content_type: &'static str,
    body: Bytes,
}

#[derive(Deserialize)]
struct Metafile {
    outputs: HashMap<String, Output>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    entry_point: Option<String>,
}

impl Assets {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let manifest = dir.join(MANIFEST);
        let json =
            fs::read(&manifest).with_context(|| format!("can't read {}", manifest.display()))?;
        let metafile: Metafile = serde_json::from_slice(&json)
            .with_context(|| format!("can't parse {}", manifest.display()))?;

        let mut assets = Self {
            urls: HashMap::new(),
            files: HashMap::new(),
        };
        for (output, Output { entry_point }) in metafile.outputs {
            let name = file_name(&output)
                .with_context(|| format!("{} lists {output:?}", manifest.display()))?;
            let path = dir.join(name);
            let body = fs::read(&path).with_context(|| format!("can't read {}", path.display()))?;
            if let Some(entry_point) = entry_point {
                let url = format!("{URL_PREFIX}{name}");
                assets.urls.insert(logical_name(&entry_point, name), url);
            }
            let content_type = content_type(name);
            let body = body.into();
            assets
                .files
                .insert(name.to_owned(), File { content_type, body });
        }
        Ok(assets)
    }

    /// Where the file built from an entry point is served, e.g.
    /// `members.css` → `/assets/members-N4AQSMXV.css`.
    pub fn url(&self, name: &str) -> askama::Result<&str> {
        self.urls
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| askama::Error::custom(format!("no asset was built as {name}")))
    }

    /// The text of the file built from an entry point, for drawings pages
    /// inline so their scripts can animate them, such as `chair.svg`.
    pub fn inline(&self, name: &str) -> askama::Result<&str> {
        let url = self.url(name)?;
        let file = &self.files[&url[URL_PREFIX.len()..]];
        std::str::from_utf8(&file.body).map_err(askama::Error::custom)
    }

    pub fn response(&self, file_name: &str) -> Option<Response> {
        self.files.get(file_name).map(|file| {
            let headers = [
                (header::CONTENT_TYPE, file.content_type),
                (header::CACHE_CONTROL, IMMUTABLE),
            ];
            (headers, file.body.clone()).into_response()
        })
    }
}

fn file_name(output: &str) -> Option<&str> {
    Path::new(output).file_name()?.to_str()
}

/// The entry point's name with the output's extension: `styles/members.css`
/// is `members.css`, and a `scripts/members.ts` would be `members.js`.
fn logical_name(entry_point: &str, output: &str) -> String {
    let stem = Path::new(entry_point).file_stem().unwrap_or_default();
    let extension = Path::new(output).extension().unwrap_or_default();
    format!("{}.{}", stem.to_string_lossy(), extension.to_string_lossy())
}

fn content_type(file_name: &str) -> &'static str {
    match Path::new(file_name).extension().and_then(|e| e.to_str()) {
        Some("css") => "text/css",
        Some("js") => "text/javascript",
        Some("svg") => "image/svg+xml",
        Some("map") => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/assets")
    }

    fn fixture() -> Assets {
        Assets::load(&fixture_dir()).unwrap()
    }

    #[test]
    fn maps_entry_points_to_their_hashed_urls() {
        let assets = fixture();
        assert_eq!(
            assets.url("members.css").unwrap(),
            "/assets/members-N4AQSMXV.css"
        );
        assert_eq!(
            assets.url("favicon.svg").unwrap(),
            "/assets/favicon-E6HOAGDL.svg"
        );
        assert_eq!(
            assets.url("members.js").unwrap(),
            "/assets/members-7QK2WZ3D.js"
        );
    }

    #[test]
    fn inlines_a_built_file_by_its_entry_point() {
        let assets = fixture();
        let chair = assets.inline("chair.svg").unwrap();
        assert!(chair.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg" id="chair""#));
        let error = assets.inline("sofa.svg").unwrap_err();
        assert_eq!(error.to_string(), "no asset was built as sofa.svg");
    }

    #[test]
    fn names_a_missing_entry_point_in_the_error() {
        let error = fixture().url("invite.js").unwrap_err();
        assert_eq!(error.to_string(), "no asset was built as invite.js");
    }

    #[test]
    fn names_scripts_by_their_output_extension() {
        assert_eq!(
            logical_name("scripts/members.ts", "members-ABCD1234.js"),
            "members.js"
        );
    }

    #[test]
    fn serves_every_output_including_source_maps_but_not_the_manifest() {
        let assets = fixture();
        let mut served: Vec<_> = assets.files.keys().map(String::as_str).collect();
        served.sort();
        assert_eq!(
            served,
            [
                "chair-K3J5NB2Q.svg",
                "favicon-E6HOAGDL.svg",
                "members-7QK2WZ3D.js",
                "members-N4AQSMXV.css",
                "members-N4AQSMXV.css.map",
                "wordmark-R5TY7UQ8.svg",
            ]
        );
        assert!(assets.response(MANIFEST).is_none());
    }

    #[test]
    fn fails_to_load_without_a_manifest() {
        let error = Assets::load(Path::new("/nonexistent")).err().unwrap();
        assert!(
            error.to_string().contains("/nonexistent/manifest.json"),
            "{error}"
        );
    }

    #[test]
    fn fails_to_load_when_a_listed_file_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        fs::copy(fixture_dir().join(MANIFEST), dir.path().join(MANIFEST)).unwrap();
        let error = Assets::load(dir.path()).err().unwrap();
        assert!(error.to_string().starts_with("can't read"), "{error}");
    }
}
