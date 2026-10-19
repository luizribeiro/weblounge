//! How members who forgot their password get a new one: a link to their
//! confirmed email, found by their nick or that address, which works once
//! within the hour.
//!
//! Times are Unix seconds.

use std::fmt;

use crate::{email, nick};

/// How long a reset link lasts. The mailer gives up on the email that
/// carries it once it stops working.
pub const LIFETIME: i64 = 60 * 60;
/// The most characters the "Nick or email" field takes: an address is
/// longer than any nick.
pub const MAX_LENGTH: usize = email::MAX_LENGTH;
const _: () = assert!(MAX_LENGTH >= nick::MAX_LENGTH);

pub fn expires_at(created_at: i64) -> i64 {
    created_at + LIFETIME
}

/// Whose password to reset, as someone who forgot it typed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    Nick(String),
    /// Matched in any case, as verified addresses are unique.
    Email(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidLookup {
    Empty,
    TooLong,
}

impl fmt::Display for InvalidLookup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("Enter your nick or email."),
            Self::TooLong => write!(f, "Nicks and emails have at most {MAX_LENGTH} characters."),
        }
    }
}

impl std::error::Error for InvalidLookup {}

impl Lookup {
    /// Nicks never hold an `@`, and addresses always do.
    pub fn parse(input: &str) -> Result<Self, InvalidLookup> {
        let input = input.trim();
        if input.is_empty() {
            return Err(InvalidLookup::Empty);
        }
        if input.chars().count() > MAX_LENGTH {
            return Err(InvalidLookup::TooLong);
        }
        Ok(if input.contains('@') {
            Self::Email(input.to_owned())
        } else {
            Self::Nick(input.to_owned())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_lasts_an_hour() {
        assert_eq!(expires_at(1_760_000_000), 1_760_003_600);
    }

    #[test]
    fn an_at_sign_makes_it_an_email_and_anything_else_a_nick() {
        for (input, lookup) in [
            ("alice", Lookup::Nick("alice".into())),
            (" Alice\n", Lookup::Nick("Alice".into())),
            (
                "alice@example.org",
                Lookup::Email("alice@example.org".into()),
            ),
            (
                " Alice@Example.org ",
                Lookup::Email("Alice@Example.org".into()),
            ),
            ("@", Lookup::Email("@".into())),
        ] {
            assert_eq!(Lookup::parse(input), Ok(lookup), "{input:?}");
        }
    }

    #[test]
    fn takes_the_longest_address_and_nothing_longer_or_empty() {
        let longest = format!("{}@example.org", "a".repeat(MAX_LENGTH - 12));
        assert!(Lookup::parse(&longest).is_ok());
        assert_eq!(
            Lookup::parse(&format!("a{longest}")),
            Err(InvalidLookup::TooLong)
        );
        for empty in ["", " ", "\t\n"] {
            assert_eq!(Lookup::parse(empty), Err(InvalidLookup::Empty));
        }
        assert_eq!(
            InvalidLookup::TooLong.to_string(),
            "Nicks and emails have at most 254 characters."
        );
    }
}
