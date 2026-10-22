//! The credentials in an `Authorization` header, as uploads and token
//! introspection take them.

use base64::{Engine, engine::general_purpose::STANDARD};

pub enum Credentials<'a> {
    /// The web client's OAuth access token, as it was sent.
    Bearer(&'a str),
    /// A nick and password, as IRC clients log in with.
    Basic { user: String, password: String },
}

impl<'a> Credentials<'a> {
    /// The credentials in the header `value`, if it holds a scheme we take,
    /// in any case, and Basic ones decode to a `user:password`.
    pub fn parse(value: &'a str) -> Option<Self> {
        let (scheme, credentials) = value.split_once(' ')?;
        let credentials = credentials.trim();
        if scheme.eq_ignore_ascii_case("bearer") {
            Some(Self::Bearer(credentials))
        } else if scheme.eq_ignore_ascii_case("basic") {
            let decoded = String::from_utf8(STANDARD.decode(credentials).ok()?).ok()?;
            let (user, password) = decoded.split_once(':')?;
            Some(Self::Basic {
                user: user.to_owned(),
                password: password.to_owned(),
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basic(credentials: &str) -> String {
        format!("Basic {}", STANDARD.encode(credentials))
    }

    #[test]
    fn decodes_basic_credentials_up_to_the_first_colon() {
        let header = basic("alice:hunter:2");
        for header in [header.clone(), header.replacen("Basic", "BASIC", 1)] {
            let Some(Credentials::Basic { user, password }) = Credentials::parse(&header) else {
                panic!("{header}");
            };
            assert_eq!((user.as_str(), password.as_str()), ("alice", "hunter:2"));
        }
    }

    #[test]
    fn takes_a_bearer_token_as_it_is() {
        let parsed = Credentials::parse("bearer  s3cret ");
        assert!(matches!(parsed, Some(Credentials::Bearer("s3cret"))));
    }

    #[test]
    fn refuses_anything_else() {
        for header in [
            "",
            "Basic",
            "Basic not-base64!",
            &basic("no colon"),
            &format!("Basic {}", STANDARD.encode(b"alice:\xff")),
            "Digest username=alice",
        ] {
            assert!(Credentials::parse(header).is_none(), "{header:?}");
        }
    }
}
