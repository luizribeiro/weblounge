//! The passwords concierge generates for members' and bots' IRC accounts.
//!
//! Lowercase letters and digits, minus the ones that look alike (0 and o;
//! 1, i and l), so a password reads back unambiguously and types without
//! switching keyboards on a phone. No separators: a double-click selects the
//! whole password. Long enough for 128 bits.

use anyhow::Context;
use common::random;
use ergo_api::{Client, Passphrase};

use crate::name::Name;

const ALPHABET: &[u8] = b"23456789abcdefghjkmnpqrstuvwxyz";
const LENGTH: usize = 26;

pub fn generate() -> Passphrase {
    Passphrase::new(random::string(ALPHABET, LENGTH))
        .expect("the alphabet has neither whitespace nor colons")
}

/// Replaces `account`'s password in Ergo.
pub async fn set<K>(ergo: &Client, account: &Name<K>, password: &Passphrase) -> anyhow::Result<()> {
    ergo.set_password(account, password)
        .await
        .with_context(|| format!("Ergo didn't take {account}'s new password"))
}

/// Gives `account` a new generated password in Ergo, and answers it.
pub async fn set_generated<K>(ergo: &Client, account: &Name<K>) -> anyhow::Result<Passphrase> {
    let password = generate();
    set(ergo, account, &password).await?;
    Ok(password)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_long_and_drawn_from_the_alphabet() {
        let password = generate();
        assert_eq!(password.expose().len(), LENGTH);
        assert!(password.expose().bytes().all(|c| ALPHABET.contains(&c)));
    }

    #[test]
    fn carries_at_least_128_bits() {
        assert!(random::entropy_bits(ALPHABET, LENGTH) >= 128.0);
    }

    #[test]
    fn leaves_out_characters_that_look_alike() {
        assert!(!ALPHABET.iter().any(|c| b"0o1il".contains(c)));
        assert!(!ALPHABET.iter().any(u8::is_ascii_uppercase));
    }

    #[test]
    fn differs_every_time() {
        assert_ne!(generate().expose(), generate().expose());
    }
}
