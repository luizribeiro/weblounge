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
}
