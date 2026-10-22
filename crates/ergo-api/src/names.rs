use std::{fmt, str::FromStr};

use secret::SecretString;
use serde::{Deserialize, Serialize};

/// The name of an Ergo account, which is also the nick it reserves.
///
/// Ergo compares names case-insensitively and answers with the spelling the
/// account was registered under. This type doesn't validate: deciding which
/// nicks to allow is up to the caller.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccountName(String);

impl AccountName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for AccountName {
    fn from(name: &str) -> Self {
        Self(name.to_owned())
    }
}

impl From<String> for AccountName {
    fn from(name: String) -> Self {
        Self(name)
    }
}

impl fmt::Display for AccountName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

const MAX_PASSPHRASE_BYTES: usize = 300;

/// A passphrase that passes Ergo's own checks.
///
/// Ergo refuses malformed passphrases, but takes an empty one to mean "remove
/// the account's passphrase"; a `Passphrase` is neither. It's a [`secret::Secret`]:
/// wiped as it's dropped, refused or not, and redacted in `Debug` output.
#[derive(Debug, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct Passphrase(#[serde(serialize_with = "secret::serialize_exposed")] SecretString);

/// Why a string can't be a [`Passphrase`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidPassphrase {
    #[error("the passphrase is empty")]
    Empty,
    #[error("the passphrase is longer than {MAX_PASSPHRASE_BYTES} bytes")]
    TooLong,
    #[error("the passphrase contains whitespace")]
    Whitespace,
    #[error("the passphrase starts with a colon")]
    LeadingColon,
    #[error("the passphrase is `*`, which Ergo reserves")]
    Asterisk,
}

impl Passphrase {
    pub fn new(passphrase: impl Into<String>) -> Result<Self, InvalidPassphrase> {
        let passphrase = SecretString::new(passphrase.into());
        match passphrase.expose().as_str() {
            "" => Err(InvalidPassphrase::Empty),
            "*" => Err(InvalidPassphrase::Asterisk),
            p if p.len() > MAX_PASSPHRASE_BYTES => Err(InvalidPassphrase::TooLong),
            p if p.starts_with(':') => Err(InvalidPassphrase::LeadingColon),
            p if p.chars().any(char::is_whitespace) => Err(InvalidPassphrase::Whitespace),
            _ => Ok(Self(passphrase)),
        }
    }

    pub fn expose(&self) -> &str {
        self.0.expose()
    }
}

/// Ergo's request types own their passphrase, so sending one copies it,
/// into another secret.
impl Clone for Passphrase {
    fn clone(&self) -> Self {
        Self(self.expose().into())
    }
}

impl TryFrom<String> for Passphrase {
    type Error = InvalidPassphrase;

    fn try_from(passphrase: String) -> Result<Self, Self::Error> {
        Self::new(passphrase)
    }
}

impl FromStr for Passphrase {
    type Err = InvalidPassphrase;

    fn from_str(passphrase: &str) -> Result<Self, Self::Err> {
        Self::new(passphrase)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_what_ergo_accepts() {
        for passphrase in [
            "q7Hx-2mZp_9vLcT4wRfB",
            "pässwörd",
            "a:b",
            "**",
            &"x".repeat(MAX_PASSPHRASE_BYTES),
        ] {
            assert_eq!(Passphrase::new(passphrase).unwrap().expose(), passphrase);
        }
    }

    #[test]
    fn rejects_what_ergo_rejects() {
        for (passphrase, reason) in [
            ("", InvalidPassphrase::Empty),
            ("*", InvalidPassphrase::Asterisk),
            (":hunter2", InvalidPassphrase::LeadingColon),
            ("correct horse", InvalidPassphrase::Whitespace),
            ("tab\there", InvalidPassphrase::Whitespace),
            ("no\u{a0}break", InvalidPassphrase::Whitespace),
            (
                &"x".repeat(MAX_PASSPHRASE_BYTES + 1),
                InvalidPassphrase::TooLong,
            ),
            (
                &"é".repeat(MAX_PASSPHRASE_BYTES / 2 + 1),
                InvalidPassphrase::TooLong,
            ),
        ] {
            assert_eq!(
                Passphrase::new(passphrase).unwrap_err(),
                reason,
                "{passphrase:?}"
            );
        }
    }

    #[test]
    fn is_sent_as_a_plain_string_and_validated_when_received() {
        let passphrase: Passphrase = "hunter2".parse().unwrap();
        assert_eq!(serde_json::to_string(&passphrase).unwrap(), r#""hunter2""#);
        assert!(serde_json::from_str::<Passphrase>(r#""hunter 2""#).is_err());
    }

    #[test]
    fn debug_output_hides_the_passphrase() {
        let passphrase: Passphrase = "hunter2".parse().unwrap();
        assert_eq!(format!("{passphrase:?}"), "Passphrase(<redacted>)");
    }

    #[test]
    fn account_names_travel_as_plain_strings() {
        let name = AccountName::from("Alice");
        assert_eq!(serde_json::to_string(&name).unwrap(), r#""Alice""#);
        assert_eq!(name.to_string(), "Alice");
    }
}
