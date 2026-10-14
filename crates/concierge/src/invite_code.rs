//! Invite codes: the secret part of an invite link.

use crate::random;

/// base64url's alphabet: nothing in it needs escaping in a URL.
const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const LENGTH: usize = 22;

pub fn generate() -> String {
    random::string(ALPHABET, LENGTH)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn differs_every_time() {
        assert_ne!(generate(), generate());
    }
}
