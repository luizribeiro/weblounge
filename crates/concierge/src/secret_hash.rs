//! What the database keeps of the secrets concierge hands out: their
//! hashes, with a type for each kind of secret, so one kind's hash can't
//! be looked up, stored or deleted as another's.

use std::marker::PhantomData;

use sha2::{Digest, Sha256};

use crate::sqlite_newtype::sqlite_newtype;

/// The hash of a secret of the kind `K`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretHash<K> {
    bytes: Vec<u8>,
    kind: PhantomData<K>,
}

/// A session's token, which its cookie holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Session {}

/// A code the web client trades for an access token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizationCode {}

/// A token the web client signs in to IRC with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessToken {}

/// The code of an invite link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InviteCode {}

/// The code of a link that confirms a member's email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationCode {}

/// The code of a link that resets a member's password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResetCode {}

pub type SessionHash = SecretHash<Session>;
pub type AuthorizationCodeHash = SecretHash<AuthorizationCode>;
pub type AccessTokenHash = SecretHash<AccessToken>;
pub type InviteCodeHash = SecretHash<InviteCode>;
pub type VerificationCodeHash = SecretHash<VerificationCode>;
pub type ResetCodeHash = SecretHash<ResetCode>;

/// The secrets hashed with the machine's session key, as
/// [`crate::session::Hasher`] does: tokens of the session kind.
pub trait Keyed {}

impl Keyed for Session {}
impl Keyed for AuthorizationCode {}
impl Keyed for AccessToken {}

/// The codes in links, which a plain hash is enough for: they're random
/// and long, so nobody can search for one that hashes the same.
pub trait LinkCode {}

impl LinkCode for InviteCode {}
impl LinkCode for VerificationCode {}
impl LinkCode for ResetCode {}

impl<K> SecretHash<K> {
    /// The hash stored as `bytes`.
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
            kind: PhantomData,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl<K: LinkCode> SecretHash<K> {
    /// What the database keeps of `code`.
    pub fn of(code: &str) -> Self {
        Self::from_bytes(Sha256::digest(code.as_bytes()).to_vec())
    }
}

sqlite_newtype!(SecretHash, bytes: Vec<u8>, SecretHash::from_bytes);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_link_codes_with_sha256() {
        let hex: String = InviteCodeHash::of("q7Hx2mZp9vLcT4wRfBa-_w")
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(
            hex,
            "22cdf3661de9f286e70ff43127a6d9e94aadeb9a076cfcdaa771265fcaf85f70"
        );
        assert_eq!(
            InviteCodeHash::of("x").as_bytes().len(),
            crate::link_code::HASH_LENGTH
        );
    }

    #[tokio::test]
    async fn travels_to_and_from_sqlite_as_a_blob() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        let stored = SessionHash::from_bytes(b"\xa1\x00".to_vec());
        let (hash, kind): (SessionHash, String) = sqlx::query_as("SELECT ?1, typeof(?1)")
            .bind(&stored)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((hash, kind.as_str()), (stored, "blob"));
    }
}
