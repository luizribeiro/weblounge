//! What concierge and the scrubber share about uploads: the images the
//! network takes, how big they may be, and the names they're stored and
//! linked under.

pub mod handoff;

use std::fmt;

use common::random;

/// The largest image anyone may upload, in bytes.
pub const MAX_BYTES: usize = 10_000_000;

/// The images the network takes, whatever a client calls what it sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageType {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageType {
    pub const ALL: [Self; 4] = [Self::Png, Self::Jpeg, Self::Gif, Self::Webp];

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }

    /// What its files end in, which nginx serves them under the content
    /// type of.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }

    pub fn from_extension(extension: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.extension() == extension)
    }
}

/// The random name an upload is stored and linked under, which is all
/// that keeps others from finding it. Its `Debug` output leaves it out.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct UploadId(String);

impl UploadId {
    /// 132 bits in base64url, as many as an invite code carries.
    pub const LENGTH: usize = 22;

    pub fn generate() -> Self {
        Self(random::string(random::URL_SAFE, Self::LENGTH))
    }

    pub fn parse(id: &str) -> Option<Self> {
        let valid = id.len() == Self::LENGTH && id.bytes().all(|c| random::URL_SAFE.contains(&c));
        valid.then(|| Self(id.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The name of its file, with `kind`'s extension.
    pub fn file_name(&self, kind: ImageType) -> String {
        format!("{}.{}", self.0, kind.extension())
    }
}

impl fmt::Debug for UploadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UploadId(..)")
    }
}

common::sqlite_newtype!(UploadId, 0: String, UploadId);

/// The id and type of the upload whose file is `name`, if it's one.
pub fn parse_file_name(name: &str) -> Option<(UploadId, ImageType)> {
    let (id, extension) = name.split_once('.')?;
    Some((UploadId::parse(id)?, ImageType::from_extension(extension)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_carry_at_least_128_bits_in_url_safe_characters() {
        assert!(random::entropy_bits(random::URL_SAFE, UploadId::LENGTH) >= 128.0);
        let id = UploadId::generate();
        assert_eq!(UploadId::parse(id.as_str()), Some(id.clone()));
        assert_ne!(UploadId::generate(), id);
    }

    #[test]
    fn only_ids_of_the_right_length_and_alphabet_parse() {
        assert!(UploadId::parse("q7Hx2mZp9vLcT4wRfBa-_w").is_some());
        for id in [
            "",
            "q7Hx2mZp9vLcT4wRfBa-_",
            "q7Hx2mZp9vLcT4wRfBa-_wx",
            "q7Hx2mZp9vLcT4wRfBa/_w",
            "../../../../etc/passwd",
            "q7Hx2mZp9vLcT4wRfBa.png",
        ] {
            assert_eq!(UploadId::parse(id), None, "{id}");
        }
    }

    #[test]
    fn debug_output_leaves_the_id_out() {
        let id = UploadId::parse("q7Hx2mZp9vLcT4wRfBa-_w").unwrap();
        assert_eq!(format!("{id:?}"), "UploadId(..)");
    }

    #[test]
    fn files_are_named_by_id_and_the_types_extension() {
        let id = UploadId::parse("q7Hx2mZp9vLcT4wRfBa-_w").unwrap();
        for kind in ImageType::ALL {
            let name = id.file_name(kind);
            assert_eq!(parse_file_name(&name), Some((id.clone(), kind)), "{name}");
        }
        assert_eq!(id.file_name(ImageType::Jpeg), "q7Hx2mZp9vLcT4wRfBa-_w.jpg");
    }

    #[test]
    fn other_files_have_no_upload() {
        for name in [
            "q7Hx2mZp9vLcT4wRfBa-_w",
            "q7Hx2mZp9vLcT4wRfBa-_w.svg",
            "q7Hx2mZp9vLcT4wRfBa-_w.jpeg",
            "q7Hx2mZp9vLcT4wRfBa-_w.png.html",
            ".tmpAbC123",
        ] {
            assert_eq!(parse_file_name(name), None, "{name}");
        }
    }

    #[test]
    fn each_type_has_its_own_content_type_and_extension() {
        let types: Vec<_> = ImageType::ALL.map(ImageType::content_type).into();
        assert_eq!(
            types,
            ["image/png", "image/jpeg", "image/gif", "image/webp"]
        );
        for kind in ImageType::ALL {
            assert_eq!(ImageType::from_extension(kind.extension()), Some(kind));
        }
    }
}
