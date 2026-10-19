//! Which email addresses members may give. This only catches typos: the
//! link mailed to an address is what proves it works.

use std::fmt;

use sqlx::{
    Decode, Encode, Sqlite, Type,
    encode::IsNull,
    error::BoxDynError,
    sqlite::{SqliteArgumentValue, SqliteTypeInfo, SqliteValueRef},
};

/// The longest address SMTP carries.
pub const MAX_LENGTH: usize = 254;

/// The rule as an HTML `pattern` attribute, so forms check it before
/// submitting: something, an `@`, and a domain with a dot inside.
pub const HTML_PATTERN: &str = r"[^\s@]+@[^\s@]+\.[^\s@]+";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidEmail {
    Empty,
    TooLong,
    Malformed,
}

impl fmt::Display for InvalidEmail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("Enter an email address."),
            Self::TooLong => write!(f, "Addresses have at most {MAX_LENGTH} characters."),
            Self::Malformed => f.write_str("That doesn't look like an email address."),
        }
    }
}

impl std::error::Error for InvalidEmail {}

impl Email {
    /// Follows [`HTML_PATTERN`] exactly, after trimming the spaces phones
    /// like to append.
    pub fn parse(email: &str) -> Result<Self, InvalidEmail> {
        let email = email.trim();
        if email.is_empty() {
            return Err(InvalidEmail::Empty);
        }
        if email.chars().count() > MAX_LENGTH {
            return Err(InvalidEmail::TooLong);
        }
        let well_formed = email
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && has_inner_dot(domain))
            && email.matches('@').count() == 1
            && !email.chars().any(char::is_whitespace);
        if !well_formed {
            return Err(InvalidEmail::Malformed);
        }
        Ok(Self(email.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Type<Sqlite> for Email {
    fn type_info() -> SqliteTypeInfo {
        <String as Type<Sqlite>>::type_info()
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        <String as Type<Sqlite>>::compatible(ty)
    }
}

impl<'q> Encode<'q, Sqlite> for Email {
    fn encode_by_ref(
        &self,
        args: &mut Vec<SqliteArgumentValue<'q>>,
    ) -> Result<IsNull, BoxDynError> {
        self.0.encode_by_ref(args)
    }
}

/// Read back as stored, without parsing again: only addresses that parsed
/// are ever stored, and a rule tightened since mustn't lock their members
/// out of reading their own rows.
impl<'r> Decode<'r, Sqlite> for Email {
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        String::decode(value).map(Self)
    }
}

/// A dot with something on either side.
fn has_inner_dot(domain: &str) -> bool {
    let mut chars = domain.char_indices();
    chars.next();
    chars.next_back();
    chars.any(|(_, c)| c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_addresses_trimmed() {
        for (input, stored) in [
            ("alice@example.org", "alice@example.org"),
            ("a.b+irc@mail.example.co.uk", "a.b+irc@mail.example.co.uk"),
            (" bob@example.org ", "bob@example.org"),
            ("zoë@exämple.org", "zoë@exämple.org"),
        ] {
            assert_eq!(Email::parse(input).unwrap().as_str(), stored);
        }
    }

    #[test]
    fn explains_what_is_wrong() {
        assert_eq!(Email::parse("  "), Err(InvalidEmail::Empty));
        let long = format!("{}@example.org", "a".repeat(MAX_LENGTH));
        assert_eq!(Email::parse(&long), Err(InvalidEmail::TooLong));
        for email in [
            "alice",
            "alice@",
            "@example.org",
            "alice@example",
            "a@b@c.org",
            "al ice@example.org",
            "alice@.org",
            "alice@example.",
        ] {
            assert_eq!(Email::parse(email), Err(InvalidEmail::Malformed), "{email}");
        }
    }

    #[test]
    fn the_html_pattern_accepts_exactly_what_parse_accepts() {
        let pattern = regex::Regex::new(&format!("^(?:{HTML_PATTERN})$")).unwrap();
        for email in [
            "alice@example.org",
            "a.b+irc@mail.example.co.uk",
            "zoë@exämple.org",
            "alice",
            "alice@",
            "@example.org",
            "alice@example",
            "a@b@c.org",
            "al ice@example.org",
            "alice@.org",
            "alice@example.",
            "alice@a.b.",
            "alice@.a.b",
        ] {
            assert_eq!(
                pattern.is_match(email),
                Email::parse(email).is_ok(),
                "{email}"
            );
        }
    }
}
