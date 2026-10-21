//! Which platform a visitor is on, guessed from their User-Agent, so the
//! welcome page and the clients page can lead with apps that run there.

use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Windows,
    Linux,
    Ios,
    Android,
}

impl Platform {
    /// `None` when the User-Agent names none of them, as for ChromeOS.
    /// iPads asking for desktop sites claim to be Macs, and are taken at
    /// their word.
    pub fn from_user_agent(user_agent: &str) -> Option<Self> {
        let says = |words: &[&str]| words.iter().any(|word| user_agent.contains(word));
        if says(&["iPhone", "iPad", "iPod"]) {
            Some(Self::Ios)
        } else if says(&["Android"]) {
            Some(Self::Android)
        } else if says(&["CrOS"]) {
            None
        } else if says(&["Windows"]) {
            Some(Self::Windows)
        } else if says(&["Macintosh", "Mac OS X"]) {
            Some(Self::MacOs)
        } else if says(&["Linux", "X11"]) {
            Some(Self::Linux)
        } else {
            None
        }
    }

    /// What to call the visitor's device: "Chat on your Mac".
    pub fn device(self) -> &'static str {
        match self {
            Self::MacOs => "Mac",
            Self::Windows => "PC",
            Self::Linux => "computer",
            Self::Ios => "iPhone or iPad",
            Self::Android => "Android phone or tablet",
        }
    }
}

/// The visitor's platform, if their User-Agent gives it away.
pub struct VisitorPlatform(pub Option<Platform>);

impl<S: Send + Sync> FromRequestParts<S> for VisitorPlatform {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let user_agent = parts
            .headers
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok());
        Ok(Self(user_agent.and_then(Platform::from_user_agent)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_browsers_people_redeem_invites_in() {
        for (user_agent, platform) in [
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.6 Safari/605.1.15",
                Platform::MacOs,
            ),
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:143.0) Gecko/20100101 Firefox/143.0",
                Platform::MacOs,
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0",
                Platform::Windows,
            ),
            (
                "Mozilla/5.0 (X11; Linux x86_64; rv:143.0) Gecko/20100101 Firefox/143.0",
                Platform::Linux,
            ),
            (
                "Mozilla/5.0 (X11; Ubuntu; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
                Platform::Linux,
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.6 Mobile/15E148 Safari/604.1",
                Platform::Ios,
            ),
            (
                "Mozilla/5.0 (iPad; CPU OS 17_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/141.0.7390.41 Mobile/15E148 Safari/604.1",
                Platform::Ios,
            ),
            (
                "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Mobile Safari/537.36",
                Platform::Android,
            ),
            (
                "Mozilla/5.0 (Android 15; Mobile; rv:143.0) Gecko/143.0 Firefox/143.0",
                Platform::Android,
            ),
        ] {
            assert_eq!(
                Platform::from_user_agent(user_agent),
                Some(platform),
                "{user_agent}"
            );
        }
    }

    #[test]
    fn knows_nothing_of_the_rest() {
        for user_agent in [
            "",
            "curl/8.16.0",
            "Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
            "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)",
        ] {
            assert_eq!(Platform::from_user_agent(user_agent), None, "{user_agent}");
        }
    }
}
