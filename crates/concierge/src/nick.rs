//! Which nicks new members may choose. A nick is also the member's Ergo
//! account name, so it never changes.

use std::fmt;

pub const MAX_LENGTH: usize = 32;
const SPECIALS: [char; 2] = ['-', '_'];
/// The services Ergo runs, and the ones it keeps free for networks that run
/// their own; Ergo refuses accounts under these names.
const RESERVED: [&str; 8] = [
    "NickServ", "ChanServ", "HostServ", "HistServ", "MemoServ", "BotServ", "OperServ", "Global",
];

/// The rule in words, for forms to show next to the field.
pub const RULE: &str = "Up to 32 letters, digits, hyphens and underscores, starting with a letter.";

/// The rule as an HTML `pattern` attribute, so forms check it before
/// submitting; only the reserved names are left to the server. Browsers
/// compile patterns with the `v` flag, which wants `-` escaped in a class.
pub fn html_pattern() -> String {
    let specials: String = SPECIALS
        .iter()
        .map(|&c| match c {
            '-' => r"\-".to_owned(),
            c => c.to_string(),
        })
        .collect();
    format!("[A-Za-z][A-Za-z0-9{specials}]{{0,{}}}", MAX_LENGTH - 1)
}

/// A nick that follows [`RULE`], which is stricter than what Ergo accepts:
/// it fits in URLs, shells and every IRC client without escaping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nick(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidNick {
    Empty,
    TooLong,
    StartsWithoutALetter,
    Forbidden(char),
    Reserved,
}

impl fmt::Display for InvalidNick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("Choose a nick."),
            Self::TooLong => write!(f, "Nicks have at most {MAX_LENGTH} characters."),
            Self::StartsWithoutALetter => f.write_str("Nicks start with a letter."),
            Self::Forbidden(c) => write!(f, "Nicks can't contain {c:?}. {RULE}"),
            Self::Reserved => f.write_str("That nick is reserved for the network's services."),
        }
    }
}

impl std::error::Error for InvalidNick {}

impl Nick {
    pub fn parse(nick: &str) -> Result<Self, InvalidNick> {
        let first = nick.chars().next().ok_or(InvalidNick::Empty)?;
        if let Some(c) = nick.chars().find(|&c| !allowed(c)) {
            return Err(InvalidNick::Forbidden(c));
        }
        if nick.len() > MAX_LENGTH {
            return Err(InvalidNick::TooLong);
        }
        if !first.is_ascii_alphabetic() {
            return Err(InvalidNick::StartsWithoutALetter);
        }
        if RESERVED.iter().any(|name| name.eq_ignore_ascii_case(nick)) {
            return Err(InvalidNick::Reserved);
        }
        Ok(Self(nick.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn allowed(c: char) -> bool {
    c.is_ascii_alphanumeric() || SPECIALS.contains(&c)
}

impl fmt::Display for Nick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<Nick> for ergo_api::AccountName {
    fn from(nick: Nick) -> Self {
        nick.0.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_nicks_as_typed() {
        for nick in [
            "alice",
            "Bob",
            "c",
            "dave_2",
            "e-mc2",
            "Zed__",
            &"x".repeat(MAX_LENGTH),
        ] {
            assert_eq!(Nick::parse(nick).unwrap().as_str(), nick);
        }
    }

    #[test]
    fn rejects_empty_and_overlong_nicks() {
        assert_eq!(Nick::parse(""), Err(InvalidNick::Empty));
        assert_eq!(
            Nick::parse(&"x".repeat(MAX_LENGTH + 1)),
            Err(InvalidNick::TooLong)
        );
    }

    #[test]
    fn nicks_start_with_a_letter() {
        for nick in ["2pac", "_alice", "-bob"] {
            assert_eq!(
                Nick::parse(nick),
                Err(InvalidNick::StartsWithoutALetter),
                "{nick}"
            );
        }
    }

    #[test]
    fn rejects_what_irc_or_ergo_would_choke_on() {
        for (nick, c) in [
            ("al ice", ' '),
            ("alice!", '!'),
            ("al@ce", '@'),
            ("a,b", ','),
            ("a.b", '.'),
            ("a:b", ':'),
            ("a*", '*'),
            ("ali?e", '?'),
            ("#lounge", '#'),
        ] {
            assert_eq!(Nick::parse(nick), Err(InvalidNick::Forbidden(c)), "{nick}");
        }
    }

    #[test]
    fn rejects_specials_some_clients_or_urls_mangle() {
        for (nick, c) in [
            ("a|away", '|'),
            ("[m]", '['),
            ("a\\b", '\\'),
            ("a`", '`'),
            ("a^", '^'),
            ("a{b}", '{'),
        ] {
            assert_eq!(Nick::parse(nick), Err(InvalidNick::Forbidden(c)), "{nick}");
        }
    }

    #[test]
    fn rejects_anything_beyond_ascii() {
        assert_eq!(Nick::parse("zoë"), Err(InvalidNick::Forbidden('ë')));
        assert_eq!(Nick::parse("аlice"), Err(InvalidNick::Forbidden('а')));
    }

    #[test]
    fn rejects_the_services_names_in_any_case() {
        for nick in ["NickServ", "nickserv", "CHANSERV", "global"] {
            assert_eq!(Nick::parse(nick), Err(InvalidNick::Reserved), "{nick}");
        }
        assert!(Nick::parse("NickServer").is_ok());
    }

    #[test]
    fn the_html_pattern_accepts_exactly_what_parse_accepts_but_reserved_names() {
        let pattern = regex::Regex::new(&format!("^(?:{})$", html_pattern())).unwrap();
        let lengths = [MAX_LENGTH, MAX_LENGTH + 1].map(|n| "x".repeat(n));
        let samples = [
            "alice",
            "Bob",
            "c",
            "dave_2",
            "e-mc2",
            "Zed__",
            "",
            "2pac",
            "_alice",
            "-bob",
            "al ice",
            "alice!",
            "a.b",
            "a|away",
            "[m]",
            r"a\b",
            "zoë",
            "#lounge",
            &lengths[0],
            &lengths[1],
        ];
        for nick in samples {
            assert_eq!(
                pattern.is_match(nick),
                Nick::parse(nick).is_ok(),
                "{nick:?}"
            );
        }
    }

    #[test]
    fn the_html_pattern_escapes_hyphens_for_the_v_flag() {
        assert_eq!(html_pattern(), r"[A-Za-z][A-Za-z0-9\-_]{0,31}");
    }

    #[test]
    fn explains_each_refusal() {
        assert_eq!(
            InvalidNick::Forbidden('!').to_string(),
            format!("Nicks can't contain '!'. {RULE}")
        );
        assert_eq!(
            InvalidNick::TooLong.to_string(),
            "Nicks have at most 32 characters."
        );
        assert!(RULE.contains(&MAX_LENGTH.to_string()));
    }
}
