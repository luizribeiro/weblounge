//! The names of the accounts concierge knows the kind of: members' and
//! bots'. A [`Nick`] is a name someone asked for, checked against the
//! rules for new names; a [`MemberName`] or a [`BotName`] is the name of
//! an account known to be of that kind, as the database or Ergo spells
//! it, so one kind's can't be handed to what acts on the other's. Both
//! are Ergo accounts, and turn into its [`AccountName`] for its API.

use std::{
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
};

use ergo_api::AccountName;

use crate::nick::Nick;

/// The name of an account of the kind `K`, in the spelling it was
/// registered under. Names compare and hash in any ASCII case, as Ergo
/// and the database's `COLLATE NOCASE` columns compare them.
#[derive(Debug, Clone)]
pub struct Name<K> {
    name: String,
    kind: PhantomData<K>,
}

/// A member's account, removed members' placeholders included.
#[derive(Debug, Clone)]
pub enum Member {}

/// A bot's account, deleted bots' rows included.
#[derive(Debug, Clone)]
pub enum Bot {}

pub type MemberName = Name<Member>;
pub type BotName = Name<Bot>;

impl<K> Name<K> {
    /// `name`, known to be the name of an account of the kind `K`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: PhantomData,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.name
    }
}

/// A nick becomes the name of the account recorded under it.
impl<K> From<Nick> for Name<K> {
    fn from(nick: Nick) -> Self {
        Self::new(nick.as_str())
    }
}

impl<K> PartialEq for Name<K> {
    fn eq(&self, other: &Self) -> bool {
        self.name.eq_ignore_ascii_case(&other.name)
    }
}

impl<K> Eq for Name<K> {}

impl<K> Hash for Name<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.to_ascii_lowercase().hash(state);
    }
}

impl<K> fmt::Display for Name<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

impl<K> From<&Name<K>> for AccountName {
    fn from(name: &Name<K>) -> Self {
        name.as_str().into()
    }
}

impl<K> From<Name<K>> for AccountName {
    fn from(name: Name<K>) -> Self {
        name.name.into()
    }
}

common::sqlite_newtype!(Name<K>, name: String, Name::new);

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn names_are_equal_in_any_case_and_keep_their_spelling() {
        let name = MemberName::new("Alice");
        assert_eq!(name, MemberName::new("alice"));
        assert_eq!(name, MemberName::new("ALICE"));
        assert_ne!(name, MemberName::new("Alicia"));
        assert_eq!(name.as_str(), "Alice");
        assert_eq!(name.to_string(), "Alice");
    }

    #[test]
    fn names_equal_in_any_case_hash_alike() {
        let bots = HashSet::from([BotName::new("Bender")]);
        assert!(bots.contains(&BotName::new("bender")));
        assert!(bots.contains(&BotName::new("BENDER")));
        assert!(!bots.contains(&BotName::new("bender2")));
    }

    #[test]
    fn folds_ascii_alone_as_sqlite_nocase_does() {
        assert_ne!(MemberName::new("zoë"), MemberName::new("ZOË"));
        assert_eq!(MemberName::new("zoë"), MemberName::new("ZOë"));
    }

    #[test]
    fn a_nick_becomes_a_name_under_its_spelling() {
        let name = BotName::from(Nick::parse("Bender").unwrap());
        assert_eq!(name.as_str(), "Bender");
    }

    #[test]
    fn goes_to_ergo_under_its_spelling() {
        let name = MemberName::new("Alice");
        assert_eq!(AccountName::from(&name).as_str(), "Alice");
        assert_eq!(AccountName::from(name).as_str(), "Alice");
    }

    #[tokio::test]
    async fn travels_to_and_from_sqlite_as_text() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        let (name, kind): (MemberName, String) = sqlx::query_as("SELECT ?1, typeof(?1)")
            .bind(MemberName::new("Alice"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((name.as_str(), kind.as_str()), ("Alice", "text"));
    }
}
