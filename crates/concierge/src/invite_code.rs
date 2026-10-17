//! Invite codes: the secret part of an invite link.

pub use crate::link_code::{HASH_LENGTH, hash};

use crate::link_code;

const LENGTH: usize = 22;
// The database tells hashed codes from ones kept before hashing by length.
const _: () = assert!(LENGTH != HASH_LENGTH);

pub fn generate() -> String {
    link_code::generate(LENGTH)
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
    use crate::random;

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
        assert!(random::entropy_bits(link_code::ALPHABET, LENGTH) >= 128.0);
    }

    #[test]
    fn has_its_own_length() {
        assert_eq!(generate().len(), LENGTH);
    }
}
