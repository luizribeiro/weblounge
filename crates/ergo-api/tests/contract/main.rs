//! One suite for every backend that answers like Ergo's API.

use ergo_api::{Client, Error, ErrorCode, Passphrase, Registration, Url, fake::FakeErgo};

trait Backend: Sized {
    async fn start() -> Self;
    fn url(&self) -> Url;
    fn token(&self) -> &str;
    async fn stop(self);

    fn client(&self) -> Client {
        Client::new(self.url(), self.token())
    }
}

impl Backend for FakeErgo {
    async fn start() -> Self {
        FakeErgo::start().await
    }

    fn url(&self) -> Url {
        FakeErgo::url(self)
    }

    fn token(&self) -> &str {
        FakeErgo::token(self)
    }

    async fn stop(self) {
        FakeErgo::stop(self).await
    }
}

/// Calls `$then!($($args)*; case, case, …)` with every case in the suite.
macro_rules! with_every_case {
    ($then:ident!($($args:tt)*)) => {
        $then!($($args)*;
            reports_its_status,
            registers_and_signs_in,
            names_clash_whatever_their_case,
            refuses_names_irc_cant_carry,
            wrong_credentials_sign_nobody_in,
            a_new_password_replaces_the_old,
            rejects_a_wrong_token,
            fails_to_reach_a_stopped_server,
        );
    };
}

macro_rules! run_against {
    ($backend:ty; $($case:ident),* $(,)?) => {
        $(
            #[tokio::test]
            async fn $case() {
                super::$case(<$backend as super::Backend>::start().await).await;
            }
        )*
    };
}

mod fake {
    with_every_case!(run_against!(ergo_api::fake::FakeErgo));
}

fn passphrase(passphrase: &str) -> Passphrase {
    passphrase.parse().unwrap()
}

async fn registered(ergo: &impl Backend, accounts: &[(&str, &str)]) -> Client {
    let client = ergo.client();
    for (name, password) in accounts {
        let registration = client.register(*name, &passphrase(password)).await.unwrap();
        assert_eq!(registration, Registration::Created, "{name}");
    }
    client
}

async fn reports_its_status(ergo: impl Backend) {
    let status = ergo.client().status().await.unwrap();
    assert!(status.version.starts_with("2.19.1"), "{status:?}");
}

async fn registers_and_signs_in(ergo: impl Backend) {
    let client = registered(&ergo, &[("alice", "correct-horse")]).await;
    let signed_in = client.check_auth("alice", "correct-horse").await.unwrap();
    assert_eq!(signed_in, Some("alice".into()));
}

async fn names_clash_whatever_their_case(ergo: impl Backend) {
    let client = registered(&ergo, &[("Alice", "correct-horse")]).await;

    let again = client
        .register("alice", &passphrase("battery-staple"))
        .await;
    assert_eq!(again.unwrap(), Registration::NameTaken);

    let signed_in = client.check_auth("ALICE", "correct-horse").await.unwrap();
    assert_eq!(signed_in, Some("Alice".into()));
    let account = client.info("aLiCe").await.unwrap().unwrap();
    assert_eq!(account.account_name, "Alice".into());
}

async fn refuses_names_irc_cant_carry(ergo: impl Backend) {
    let client = ergo.client();
    for name in ["a b", "#lounge", "al!ce"] {
        let error = client
            .register(name, &passphrase("correct-horse"))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(&ErrorCode::UnknownError), "{name}");
    }
}

async fn wrong_credentials_sign_nobody_in(ergo: impl Backend) {
    let client = registered(&ergo, &[("alice", "correct-horse")]).await;
    for (name, password) in [
        ("alice", "battery-staple"),
        ("alice", ""),
        ("bob", "correct-horse"),
        ("", "correct-horse"),
    ] {
        let signed_in = client.check_auth(name, password).await.unwrap();
        assert_eq!(signed_in, None, "{name}/{password}");
    }
}

async fn a_new_password_replaces_the_old(ergo: impl Backend) {
    let client = registered(&ergo, &[("alice", "correct-horse")]).await;

    client
        .set_password("alice", &passphrase("battery-staple"))
        .await
        .unwrap();

    assert_eq!(
        client.check_auth("alice", "correct-horse").await.unwrap(),
        None
    );
    assert_eq!(
        client.check_auth("alice", "battery-staple").await.unwrap(),
        Some("alice".into())
    );
    let error = client
        .set_password("bob", &passphrase("battery-staple"))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(&ErrorCode::AccountDoesNotExist));
}

async fn rejects_a_wrong_token(ergo: impl Backend) {
    let client = Client::new(ergo.url(), "not-the-token");
    let error = client.status().await.unwrap_err();
    assert!(
        matches!(&error, Error::Unauthorized(body) if body == "Unauthorized"),
        "{error:?}"
    );
}

async fn fails_to_reach_a_stopped_server(ergo: impl Backend) {
    let client = ergo.client();
    ergo.stop().await;
    let error = client.status().await.unwrap_err();
    assert!(matches!(error, Error::Transport(_)), "{error:?}");
}
