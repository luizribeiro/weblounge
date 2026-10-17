//! The secret codes in links concierge shows or mails once, for invites and
//! email verification. Only their hashes are kept: nothing needs a code
//! back.

use sha2::{Digest, Sha256};

use crate::random;

pub const ALPHABET: &[u8] = random::URL_SAFE;
pub const HASH_LENGTH: usize = 32;

/// A new code of `length` characters, none of which a URL needs escaped.
pub fn generate(length: usize) -> String {
    random::string(ALPHABET, length)
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
