//! The rules members choose their password under, at signup or later. Concierge is
//! the authority: signup pages get [`PageRules`] as JSON for their live
//! checklist, and the server checks every rule again.

use std::{collections::HashSet, fmt, sync::LazyLock};

use ergo_api::Passphrase;
use serde::Serialize;

pub const MIN_LENGTH: usize = 12;
/// Ergo's limit.
pub const MAX_BYTES: usize = 300;

static COMMON: LazyLock<HashSet<String>> = LazyLock::new(|| {
    include_str!("common-passwords.txt")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_lowercase)
        .collect()
});

/// One line of the checklist new-password forms show, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    Length,
    NoWhitespace,
    NoLeadingColon,
    NotCommon,
    Matches,
}

impl Rule {
    pub const ALL: [Self; 5] = [
        Self::Length,
        Self::NoWhitespace,
        Self::NoLeadingColon,
        Self::NotCommon,
        Self::Matches,
    ];

    /// What the page's script knows the rule by.
    pub fn id(self) -> &'static str {
        match self {
            Self::Length => "length",
            Self::NoWhitespace => "no-whitespace",
            Self::NoLeadingColon => "no-leading-colon",
            Self::NotCommon => "not-common",
            Self::Matches => "matches",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Length => "12+ characters",
            Self::NoWhitespace => "no spaces",
            Self::NoLeadingColon => "no leading :",
            Self::NotCommon => "not a common password",
            Self::Matches => "passwords match",
        }
    }
}

/// The parameters the page's script checks [`Rule::ALL`] with.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRules {
    pub min_length: usize,
    pub max_bytes: usize,
    /// Lowercased: passwords are compared in lowercase.
    pub common: Vec<&'static str>,
}

pub fn page_rules() -> PageRules {
    let mut common: Vec<_> = COMMON.iter().map(String::as_str).collect();
    common.sort_unstable();
    PageRules {
        min_length: MIN_LENGTH,
        max_bytes: MAX_BYTES,
        common,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    TooShort,
    TooLong,
    Whitespace,
    LeadingColon,
    Common,
    Mismatch,
}

impl Refusal {
    pub fn rule(self) -> Rule {
        match self {
            Self::TooShort | Self::TooLong => Rule::Length,
            Self::Whitespace => Rule::NoWhitespace,
            Self::LeadingColon => Rule::NoLeadingColon,
            Self::Common => Rule::NotCommon,
            Self::Mismatch => Rule::Matches,
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "Passwords have at least {MIN_LENGTH} characters."),
            Self::TooLong => write!(f, "Passwords have at most {MAX_BYTES} bytes."),
            Self::Whitespace => f.write_str("Passwords can't contain spaces."),
            Self::LeadingColon => f.write_str("Passwords can't start with a colon."),
            Self::Common => f.write_str("That password is too common."),
            Self::Mismatch => f.write_str("The passwords don't match."),
        }
    }
}

impl std::error::Error for Refusal {}

/// What a form with a new password shows under each of its two fields.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FieldErrors {
    pub password: Option<Refusal>,
    pub confirmation: Option<Refusal>,
}

impl From<Refusal> for FieldErrors {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::Mismatch => Self {
                confirmation: Some(refusal),
                ..Self::default()
            },
            _ => Self {
                password: Some(refusal),
                ..Self::default()
            },
        }
    }
}

/// A password that follows every [`Rule`], ready for Ergo.
pub fn check(password: &str, confirmation: &str) -> Result<Passphrase, Refusal> {
    if password.chars().count() < MIN_LENGTH {
        return Err(Refusal::TooShort);
    }
    if password.len() > MAX_BYTES {
        return Err(Refusal::TooLong);
    }
    if password.chars().any(char::is_whitespace) {
        return Err(Refusal::Whitespace);
    }
    if password.starts_with(':') {
        return Err(Refusal::LeadingColon);
    }
    if COMMON.contains(&password.to_lowercase()) {
        return Err(Refusal::Common);
    }
    if password != confirmation {
        return Err(Refusal::Mismatch);
    }
    Ok(Passphrase::new(password).expect("the rules are stricter than Ergo's"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(password: &str) -> Option<Refusal> {
        check(password, password).err()
    }

    #[test]
    fn accepts_long_unusual_passwords() {
        for password in [
            "q7Hx-2mZp_9vLcT4",
            "correct-horse-battery",
            "pässwörd-über-alles",
            "a:colon:inside",
            &"x".repeat(MAX_BYTES),
        ] {
            assert_eq!(
                check(password, password).unwrap().as_str(),
                password,
                "{password}"
            );
        }
    }

    #[test]
    fn counts_characters_for_the_minimum_and_bytes_for_the_maximum() {
        assert_eq!(refusal("elevenchars"), Some(Refusal::TooShort));
        assert_eq!(refusal("ëëëëëëëëëëëë"), None);
        assert_eq!(refusal(&"x".repeat(MAX_BYTES + 1)), Some(Refusal::TooLong));
        assert_eq!(
            refusal(&"é".repeat(MAX_BYTES / 2 + 1)),
            Some(Refusal::TooLong)
        );
    }

    #[test]
    fn refuses_what_ergo_would() {
        for (password, why) in [
            ("correct horse battery", Refusal::Whitespace),
            ("tabs\tare-whitespace", Refusal::Whitespace),
            ("no\u{a0}break-spaces-either", Refusal::Whitespace),
            (":starts-with-a-colon", Refusal::LeadingColon),
        ] {
            assert_eq!(refusal(password), Some(why), "{password:?}");
        }
    }

    #[test]
    fn refuses_common_passwords_in_any_case() {
        for password in ["qwerty123456", "QWERTY123456", "1q2w3e4r5t6y"] {
            assert_eq!(refusal(password), Some(Refusal::Common), "{password}");
        }
    }

    #[test]
    fn the_list_is_long_and_holds_only_passwords_the_other_rules_allow() {
        assert!(COMMON.len() > 1000, "{}", COMMON.len());
        for password in COMMON.iter() {
            assert_eq!(refusal(password), Some(Refusal::Common), "{password}");
        }
    }

    #[test]
    fn the_confirmation_must_match() {
        let refused = check("correct-horse-battery", "correct-horse-batter");
        assert_eq!(refused.unwrap_err(), Refusal::Mismatch);
    }

    #[test]
    fn every_refusal_points_at_a_checklist_line() {
        for refusal in [
            Refusal::TooShort,
            Refusal::TooLong,
            Refusal::Whitespace,
            Refusal::LeadingColon,
            Refusal::Common,
            Refusal::Mismatch,
        ] {
            assert!(Rule::ALL.contains(&refusal.rule()), "{refusal:?}");
        }
        assert_eq!(
            Refusal::TooShort.to_string(),
            "Passwords have at least 12 characters."
        );
    }

    #[test]
    fn a_mismatch_shows_under_the_confirmation_and_the_rest_under_the_password() {
        assert_eq!(
            FieldErrors::from(Refusal::Mismatch),
            FieldErrors {
                password: None,
                confirmation: Some(Refusal::Mismatch)
            }
        );
        assert_eq!(
            FieldErrors::from(Refusal::Common),
            FieldErrors {
                password: Some(Refusal::Common),
                confirmation: None
            }
        );
    }

    #[test]
    fn the_page_gets_the_servers_parameters_and_list() {
        let rules = page_rules();
        assert_eq!((rules.min_length, rules.max_bytes), (MIN_LENGTH, MAX_BYTES));
        assert_eq!(rules.common.len(), COMMON.len());
        assert!(rules.common.is_sorted());
        assert!(rules.common.iter().all(|p| *p == p.to_lowercase()));
        let json = serde_json::to_value(&rules).unwrap();
        assert_eq!(json["minLength"], 12);
        assert_eq!(json["maxBytes"], 300);
        assert!(
            json["common"]
                .as_array()
                .unwrap()
                .contains(&"qwerty123456".into())
        );
    }
}
