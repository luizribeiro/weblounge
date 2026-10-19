//! What the database keeps of the secrets concierge hands out: their
//! hashes, with a type for each kind of secret, so one kind's hash can't
//! be looked up, stored or deleted as another's.

use std::marker::PhantomData;

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

pub type SessionHash = SecretHash<Session>;
pub type AuthorizationCodeHash = SecretHash<AuthorizationCode>;
pub type AccessTokenHash = SecretHash<AccessToken>;

/// The secrets hashed with the machine's session key, as
/// [`crate::session::Hasher`] does: tokens of the session kind.
pub trait Keyed {}

impl Keyed for Session {}
impl Keyed for AuthorizationCode {}
impl Keyed for AccessToken {}

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

sqlite_newtype!(SecretHash, bytes: Vec<u8>, SecretHash::from_bytes);

#[cfg(test)]
mod tests {
    use super::*;

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
