//! Invite codes: the secret part of an invite link.

use crate::random;

const ALPHABET: &[u8] = random::URL_SAFE;
const LENGTH: usize = 22;

pub fn generate() -> String {
    random::string(ALPHABET, LENGTH)
}

/// Whether `text` uses only the characters codes are made of, so it can
/// go into a URL as it is.
pub fn is_url_safe(text: &str) -> bool {
    text.bytes().all(|byte| ALPHABET.contains(&byte))
}

/// The code in what someone typed into the invite code field: the code
/// itself, or the whole link pasted in.
pub fn from_input(input: &str) -> &str {
    let input = input.trim();
    input.rsplit(['#', '/']).next().unwrap_or(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_code_alone_or_out_of_a_pasted_link() {
        let code = "q7Hx2mZp9vLcT4wRfBa-_w";
        for input in [
            code,
            " q7Hx2mZp9vLcT4wRfBa-_w\n",
            "https://members.weblounge.org/invite#q7Hx2mZp9vLcT4wRfBa-_w",
            "https://members.weblounge.org/invite/q7Hx2mZp9vLcT4wRfBa-_w",
        ] {
            assert_eq!(from_input(input), code, "{input:?}");
        }
        assert_eq!(from_input(""), "");
    }

    #[test]
    fn carries_at_least_128_bits() {
        assert!(random::entropy_bits(ALPHABET, LENGTH) >= 128.0);
    }

    #[test]
    fn needs_no_escaping_in_a_url() {
        let code = generate();
        assert_eq!(code.len(), LENGTH);
        assert!(
            code.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
            "{code}"
        );
    }

    #[test]
    fn tells_what_can_go_into_a_url_as_it_is() {
        assert!(is_url_safe(&generate()));
        for unsafe_text in ["a b", "a#b", "a/b", "ä", "a\r\nLocation: x"] {
            assert!(!is_url_safe(unsafe_text), "{unsafe_text:?}");
        }
    }

    #[test]
    fn differs_every_time() {
        assert_ne!(generate(), generate());
    }
}
