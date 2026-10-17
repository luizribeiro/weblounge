//! The secret codes in links concierge shows or mails once, for invites and
//! email verification. Only their hashes are kept: nothing needs a code
//! back.

use common::random;
use sha2::{Digest, Sha256};

pub const ALPHABET: &[u8] = random::URL_SAFE;
pub const HASH_LENGTH: usize = 32;

/// A new code of `length` characters, none of which a URL needs escaped.
pub fn generate(length: usize) -> String {
    random::string(ALPHABET, length)
}

/// The code in what someone typed into a code field: the code itself, or
/// the whole link pasted in.
pub fn from_input(input: &str) -> &str {
    let input = input.trim();
    input.rsplit(['#', '/']).next().unwrap_or(input)
}

/// The code of `length` characters in `input`, as [`from_input`] reads
/// it, if that's what it holds.
pub fn parse(input: &str, length: usize) -> Option<&str> {
    let code = from_input(input);
    let well_formed = code.len() == length && code.bytes().all(|c| ALPHABET.contains(&c));
    well_formed.then_some(code)
}

/// What [`parse`] accepts, as a form field's `pattern`. The escapes keep
/// it valid in the `v` mode browsers compile patterns in.
pub fn html_pattern(length: usize) -> String {
    format!(r"\s*(?:.*[#\/])?[A-Za-z0-9_\-]{{{length}}}\s*")
}

/// What the database keeps of `code`. A plain hash is enough: codes are
/// random and long, so nobody can search for one that hashes the same.
pub fn hash(code: &str) -> Vec<u8> {
    Sha256::digest(code.as_bytes()).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_no_escaping_in_a_url() {
        let code = generate(43);
        assert_eq!(code.len(), 43);
        assert!(
            code.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
            "{code}"
        );
    }

    #[test]
    fn takes_the_code_alone_or_out_of_a_pasted_link() {
        let code = "q7Hx2mZp9vLcT4wRfBa-_w";
        for input in [
            code,
            " q7Hx2mZp9vLcT4wRfBa-_w\n",
            "https://join.weblounge.org/#q7Hx2mZp9vLcT4wRfBa-_w",
            "https://members.weblounge.org/verify#q7Hx2mZp9vLcT4wRfBa-_w",
            "https://members.weblounge.org/invite/q7Hx2mZp9vLcT4wRfBa-_w",
        ] {
            assert_eq!(from_input(input), code, "{input:?}");
        }
        assert_eq!(from_input(""), "");
    }

    #[test]
    fn parses_only_codes_of_the_length_and_alphabet() {
        let code = "q7Hx2mZp9vLcT4wRfBa-_w";
        let link = format!("https://members.weblounge.org/verify#{code}");
        assert_eq!(parse(&link, 22), Some(code));
        assert_eq!(parse(&format!(" {code}\n"), 22), Some(code));
        assert_eq!(parse(code, 21), None);
        assert_eq!(parse(code, 23), None);
        for bad in [
            "q7Hx2mZp9vLcT4wRfBa+_w",
            "q7Hx2mZp9vLcT4wRfBa=_w",
            "q7Hx2mZp9vLcT4wRfBaé_",
        ] {
            assert_eq!(parse(bad, 22), None, "{bad}");
        }
    }

    #[test]
    fn the_html_pattern_accepts_exactly_what_parse_accepts() {
        let pattern = regex::Regex::new(&format!("^(?:{})$", html_pattern(4))).unwrap();
        let mut inputs: Vec<String> = (0..=127u8)
            .map(|c| format!("ab{}d", char::from(c)))
            .collect();
        inputs.extend(
            [
                "abcd",
                " abcd\t",
                "https://members.weblounge.org/verify#abcd",
                "see https://join.weblounge.org/#ab-_ ",
                "x/abcd",
                "abc",
                "abcde",
                "abcd#",
                "#abcd/",
                "ab cd",
                "abçd",
                "",
            ]
            .map(String::from),
        );
        for input in inputs {
            assert_eq!(
                pattern.is_match(&input),
                parse(&input, 4).is_some(),
                "{input:?}"
            );
        }
    }

    #[test]
    fn hashes_with_sha256() {
        let hex: String = hash("q7Hx2mZp9vLcT4wRfBa-_w")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(
            hex,
            "22cdf3661de9f286e70ff43127a6d9e94aadeb9a076cfcdaa771265fcaf85f70"
        );
        assert_eq!(hash("x").len(), HASH_LENGTH);
    }

    #[test]
    fn differs_every_time() {
        assert_ne!(generate(22), generate(22));
    }
}
