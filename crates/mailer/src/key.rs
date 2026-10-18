//! The key that opens sealed mail. Each mailer generates its own as it
//! starts and keeps it in memory alone, so what was sealed to it can't be
//! opened from the disk, or once that mailer is gone.

use age::x25519::Identity;
use outbox::PublicKey;

/// Zeroed as it's dropped.
pub struct Key(Identity);

impl Key {
    pub fn generate() -> Self {
        Self(Identity::generate())
    }

    pub fn public(&self) -> PublicKey {
        self.0.to_public()
    }

    /// What `ciphertext` says, if it was sealed to this key.
    pub fn open(&self, ciphertext: &[u8]) -> Option<String> {
        String::from_utf8(age::decrypt(&self.0, ciphertext).ok()?).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_new() {
        let [first, second] = [(); 2].map(|()| Key::generate().public());
        assert_ne!(first, second);
        assert!(first.to_string().starts_with("age1"), "{first}");
    }

    #[test]
    fn opens_only_what_was_sealed_to_it() {
        let (key, other) = (Key::generate(), Key::generate());
        let sealed = age::encrypt(&key.public(), "Hi Zoë".as_bytes()).unwrap();

        assert_eq!(key.open(&sealed).as_deref(), Some("Hi Zoë"));
        assert_eq!(other.open(&sealed), None);
        assert_eq!(key.open(b"Hi Zoe"), None);
    }
}
