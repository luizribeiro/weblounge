//! The hosts Ergo shows accounts connecting from, in place of the
//! network's cloak, which concierge sets through its API.

use std::fmt;

/// Ergo's default `accounts.vhosts.max-length`.
pub const MAX_LENGTH: usize = 64;

/// A vhost Ergo accepts: at most [`MAX_LENGTH`] of the characters its
/// default `accounts.vhosts.valid-regexp`, `^[0-9A-Za-z.\-_/]+$`, allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vhost(String);

impl Vhost {
    pub fn parse(vhost: &str) -> Result<Self, String> {
        if vhost.is_empty() || vhost.len() > MAX_LENGTH {
            return Err(format!("vhosts have 1 to {MAX_LENGTH} characters"));
        }
        if let Some(c) = vhost.chars().find(|&c| !allowed(c)) {
            return Err(format!(
                "vhosts can't contain {c:?}, only letters, digits and . - _ /"
            ));
        }
        Ok(Self(vhost.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn allowed(c: char) -> bool {
    c.is_ascii_alphanumeric() || ".-_/".contains(c)
}

impl fmt::Display for Vhost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_what_ergo_accepts() {
        let longest = "w".repeat(MAX_LENGTH);
        for vhost in [
            "weblounge/concierge",
            "weblounge/bot/Zoe_2",
            "a.b-c",
            &longest,
        ] {
            assert_eq!(Vhost::parse(vhost).unwrap().as_str(), vhost);
        }
    }

    #[test]
    fn refuses_empty_and_overlong_vhosts() {
        for vhost in [String::new(), "w".repeat(MAX_LENGTH + 1)] {
            let error = Vhost::parse(&vhost).unwrap_err();
            assert_eq!(error, "vhosts have 1 to 64 characters", "{vhost}");
        }
    }

    #[test]
    fn refuses_characters_outside_ergos_pattern() {
        for (vhost, c) in [
            ("weblounge bender", ' '),
            ("bender@weblounge", '@'),
            ("weblounge/bénder", 'é'),
            ("a:b", ':'),
        ] {
            let error = Vhost::parse(vhost).unwrap_err();
            assert!(error.contains(&format!("{c:?}")), "{vhost}: {error}");
        }
    }
}
