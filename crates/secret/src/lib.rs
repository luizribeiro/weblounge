//! Secrets in memory: passwords, tokens, codes and keys, wiped as they're
//! dropped and kept out of `Debug` output, so out of logs and panics.
//!
//! A [`Secret`] has no `Display` and doesn't clone, and its value is read
//! with [`Secret::expose`] alone, so every place a secret leaves its
//! wrapper is one a search for `expose` finds.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
pub use zeroize::Zeroizing;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// A secret `T`, overwritten with zeros as it's dropped.
///
/// It isn't `Serialize`, so none is written out by accident:
///
/// ```compile_fail
/// #[derive(serde::Serialize)]
/// struct Request {
///     passphrase: secret::SecretString,
/// }
/// ```
pub struct Secret<T: Zeroize>(Zeroizing<T>);

/// A password, token or code.
pub type SecretString = Secret<String>;

impl<T: Zeroize> Secret<T> {
    pub fn new(value: T) -> Self {
        Self(Zeroizing::new(value))
    }

    /// The secret itself. Whatever copies it out, a library that wants a
    /// plain `String` say, keeps a copy this doesn't wipe.
    pub fn expose(&self) -> &T {
        &self.0
    }
}

impl<T: Zeroize> From<T> for Secret<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self::new(value.to_owned())
    }
}

impl<T: Zeroize + Default> Default for Secret<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Zeroize> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

// The Zeroizing inside does the wiping; this only says so.
impl<T: Zeroize> ZeroizeOnDrop for Secret<T> {}

/// Deserializing needs no opt-in: what arrives in a form or a body is
/// wrapped before anything else sees it.
impl<'de, T: Zeroize + Deserialize<'de>> Deserialize<'de> for Secret<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self::new)
    }
}

/// Serializes the secret itself, for a field that must carry it, such as
/// a request to the server that checks it:
/// `#[serde(serialize_with = "secret::serialize_exposed")]`.
pub fn serialize_exposed<T, S>(secret: &Secret<T>, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Zeroize + Serialize,
    S: Serializer,
{
    secret.expose().serialize(serializer)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use serde::{Deserialize, Serialize};

    use super::*;

    /// A secret whose bytes outlive it, to see what dropping it left.
    struct Probe(Rc<RefCell<Vec<u8>>>);

    impl Zeroize for Probe {
        fn zeroize(&mut self) {
            self.0.borrow_mut().as_mut_slice().zeroize();
        }
    }

    #[test]
    fn dropping_a_secret_overwrites_it_with_zeros() {
        let bytes = Rc::new(RefCell::new(b"hunter2".to_vec()));
        let secret = Secret::new(Probe(bytes.clone()));
        assert_eq!(*secret.expose().0.borrow(), b"hunter2");
        drop(secret);
        assert_eq!(*bytes.borrow(), [0; 7]);
    }

    #[test]
    fn debug_output_never_shows_the_secret() {
        #[derive(Debug)]
        #[allow(dead_code)]
        struct Login {
            nick: &'static str,
            password: SecretString,
        }
        let login = Login {
            nick: "alice",
            password: "hunter2".into(),
        };
        assert_eq!(
            format!("{login:?}"),
            r#"Login { nick: "alice", password: <redacted> }"#
        );
        assert_eq!(format!("{:#?}", login.password), "<redacted>");
    }

    #[test]
    fn deserializes_as_the_value_it_holds() {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Form {
            password: SecretString,
            key: Secret<Vec<u8>>,
        }
        let form: Form = serde_json::from_str(r#"{"key": [1, 2]}"#).unwrap();
        assert_eq!(form.password.expose(), "");
        assert_eq!(form.key.expose(), &[1, 2]);
        let form: Form = serde_json::from_str(r#"{"password": "hunter2"}"#).unwrap();
        assert_eq!(form.password.expose(), "hunter2");
    }

    #[test]
    fn serializes_only_where_a_field_says_so() {
        #[derive(Serialize)]
        struct Request {
            #[serde(serialize_with = "serialize_exposed")]
            passphrase: SecretString,
        }
        let request = Request {
            passphrase: "hunter2".into(),
        };
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"passphrase":"hunter2"}"#
        );
    }
}
