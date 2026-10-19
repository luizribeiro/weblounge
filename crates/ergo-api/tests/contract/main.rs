//! One suite, run against the real `ergo` binary and against [`FakeErgo`],
//! so the fake can't drift from what it stands in for.

mod real_ergo;

use ergo_api::{
    AccountSummary, Client, Erasure, Error, ErrorCode, Passphrase, Preference, Registration,
    ReplayJoins, Settings, SettingsChanges, Url, fake::FakeErgo,
};

/// The longest vhost Ergo's default `accounts.vhosts.max-length` allows.
const VHOST_MAX_LENGTH: usize = 64;
use real_ergo::RealErgo;

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
            describes_and_lists_accounts,
            settings_round_trip,
            sets_and_clears_vhosts,
            erases_accounts_and_frees_their_names,
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

mod real {
    with_every_case!(run_against!(super::RealErgo));
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

async fn describes_and_lists_accounts(ergo: impl Backend) {
    let client = registered(
        &ergo,
        &[("alice", "correct-horse"), ("Bob", "battery-staple")],
    )
    .await;
    let email = SettingsChanges {
        email: Some("alice@example.org".into()),
        ..Default::default()
    };
    client.update_settings("alice", email).await.unwrap();

    let alice = client.info("alice").await.unwrap().unwrap();
    assert_eq!(alice.email.as_deref(), Some("alice@example.org"));
    assert!(alice.registered_at.is_some(), "{alice:?}");
    assert_eq!(client.info("carol").await.unwrap(), None);
    let error = client.info("").await.unwrap_err();
    assert_eq!(error.code(), Some(&ErrorCode::InvalidRequest));

    let mut accounts = client.accounts().await.unwrap();
    accounts.sort_by(|a, b| a.account_name.cmp(&b.account_name));
    assert_eq!(
        accounts,
        [
            AccountSummary {
                account_name: "Bob".into(),
                email: None,
            },
            AccountSummary {
                account_name: "alice".into(),
                email: Some("alice@example.org".into()),
            },
        ]
    );
}

async fn settings_round_trip(ergo: impl Backend) {
    let client = registered(&ergo, &[("alice", "correct-horse")]).await;
    let defaults = Settings {
        always_on: Preference::Default,
        auto_away: Preference::Default,
        email: None,
        replay_joins: ReplayJoins::CommandsOnly,
    };
    assert_eq!(client.settings("alice").await.unwrap(), defaults);

    let changes = SettingsChanges {
        always_on: Some(Preference::Off),
        replay_joins: Some(ReplayJoins::On),
        email: Some("alice@example.org".into()),
        ..Default::default()
    };
    let changed = Settings {
        always_on: Preference::Off,
        replay_joins: ReplayJoins::On,
        email: Some("alice@example.org".into()),
        ..defaults
    };
    assert_eq!(
        client.update_settings("alice", changes).await.unwrap(),
        changed
    );
    assert_eq!(client.settings("ALICE").await.unwrap(), changed);

    let clear_email = SettingsChanges {
        email: Some(String::new()),
        ..Default::default()
    };
    let cleared = client.update_settings("alice", clear_email).await.unwrap();
    assert_eq!(cleared.email, None);

    for (name, code) in [
        ("", ErrorCode::InvalidRequest),
        ("bob", ErrorCode::AccountDoesNotExist),
    ] {
        let error = client.settings(name).await.unwrap_err();
        assert_eq!(error.code(), Some(&code), "saget {name:?}");
        let error = client
            .update_settings(name, SettingsChanges::default())
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(&code), "saset {name:?}");
    }
}

async fn sets_and_clears_vhosts(ergo: impl Backend) {
    let client = registered(&ergo, &[("louro", "correct-horse")]).await;
    let longest = format!("weblounge/{}", "x".repeat(VHOST_MAX_LENGTH - 10));
    for vhost in ["weblounge/bot/luiz", "Bot-1.example_org", &longest, ""] {
        client.set_vhost("LOURO", vhost).await.unwrap();
    }

    let too_long = format!("{longest}x");
    for (name, vhost, code) in [
        ("louro", "bot host", ErrorCode::InvalidVhost),
        ("louro", "weblounge/bot/luiz!", ErrorCode::InvalidVhost),
        ("louro", too_long.as_str(), ErrorCode::InvalidVhost),
        (
            "carol",
            "weblounge/bot/luiz",
            ErrorCode::AccountDoesNotExist,
        ),
        ("carol", "", ErrorCode::AccountDoesNotExist),
        ("", "weblounge/bot/luiz", ErrorCode::InvalidRequest),
    ] {
        let error = client.set_vhost(name, vhost).await.unwrap_err();
        assert_eq!(error.code(), Some(&code), "{name:?} {vhost:?}");
    }
}

async fn erases_accounts_and_frees_their_names(ergo: impl Backend) {
    let client = registered(
        &ergo,
        &[("Bender", "correct-horse"), ("louro", "battery-staple")],
    )
    .await;

    assert_eq!(client.erase("BENDER").await.unwrap(), Erasure::Erased);

    assert_eq!(
        client.check_auth("bender", "correct-horse").await.unwrap(),
        None
    );
    assert_eq!(client.info("bender").await.unwrap(), None);
    let names: Vec<_> = client
        .accounts()
        .await
        .unwrap()
        .into_iter()
        .map(|account| account.account_name)
        .collect();
    assert_eq!(names, ["louro".into()]);
    for name in ["bender", "carol", "a b"] {
        let erasure = client.erase(name).await.unwrap();
        assert_eq!(erasure, Erasure::NoSuchAccount, "{name}");
    }
    let error = client.erase("").await.unwrap_err();
    assert_eq!(error.code(), Some(&ErrorCode::InvalidRequest));
    let again = client
        .register("bender", &passphrase("correct-horse"))
        .await;
    assert_eq!(again.unwrap(), Registration::Created);
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
