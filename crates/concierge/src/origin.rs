//! The web origins concierge answers to: the members' site, whose own
//! pages post to it, and the web client's, which posts to its token
//! endpoint alone. Each has its own type, so where a request is checked
//! against one, the other can't stand in.

use std::{fmt, marker::PhantomData};

use axum::http::Uri;

/// The http(s) origin of the site `S`, without a trailing slash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin<S> {
    url: String,
    site: PhantomData<S>,
}

/// The members' site, concierge's own pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Members {}

/// The web client, gamja.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Webchat {}

pub type SiteOrigin = Origin<Members>;
pub type WebchatOrigin = Origin<Webchat>;

impl<S> Origin<S> {
    /// `url` as an origin, if it's an http(s) one without a path.
    pub fn parse(url: &str) -> Option<Self> {
        normalize(url).map(|url| Self {
            url,
            site: PhantomData,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.url
    }
}

impl<S> fmt::Display for Origin<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.url)
    }
}

/// `url` without its trailing slash, if it's an http(s) origin without a
/// path.
pub fn normalize(url: &str) -> Option<String> {
    let uri: Uri = url.parse().ok()?;
    let has_path = !matches!(uri.path(), "" | "/") || uri.query().is_some();
    let web_scheme = matches!(uri.scheme_str(), Some("http" | "https"));
    (web_scheme && uri.authority().is_some() && !has_path)
        .then(|| url.trim_end_matches('/').to_owned())
}
