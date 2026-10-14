//! Invite codes: the secret part of an invite link.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

use crate::random;

const ALPHABET: &[u8] = random::URL_SAFE;
const LENGTH: usize = 22;
/// 16 base64url characters: plenty to tell one member's invites apart.
const PUBLIC_ID_BYTES: usize = 12;

pub fn generate() -> String {
    random::string(ALPHABET, LENGTH)
}

/// What URLs and pages call the invite `code` without giving it away: the
/// start of its SHA-256, which nobody can turn back into the code.
pub fn public_id(code: &str) -> String {
    URL_SAFE_NO_PAD.encode(&Sha256::digest(code.as_bytes())[..PUBLIC_ID_BYTES])
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
    fn the_public_id_is_stable_short_and_not_the_code() {
        let code = "q7Hx2mZp9vLcT4wRfBa-_w";
        let id = public_id(code);
        assert_eq!(id, public_id(code));
        assert_eq!(id.len(), 16);
        assert!(!code.contains(&id) && !id.contains(&code[..8]), "{id}");
        assert_ne!(id, public_id("q7Hx2mZp9vLcT4wRfBa-_x"));
    }

    #[test]
    fn differs_every_time() {
        assert_ne!(generate(), generate());
    }
}
