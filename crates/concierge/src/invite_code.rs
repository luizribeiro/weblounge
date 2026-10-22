//! Invite codes: the secret part of an invite link.

pub use crate::link_code::{HASH_LENGTH, from_input};

use secret::SecretString;

use crate::link_code;

const LENGTH: usize = 22;
// The database tells hashed codes from ones kept before hashing by length.
const _: () = assert!(LENGTH != HASH_LENGTH);

pub fn generate() -> SecretString {
    link_code::generate(LENGTH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::random;

    #[test]
    fn carries_at_least_128_bits() {
        assert!(random::entropy_bits(link_code::ALPHABET, LENGTH) >= 128.0);
    }

    #[test]
    fn has_its_own_length() {
        assert_eq!(generate().expose().len(), LENGTH);
    }
}
