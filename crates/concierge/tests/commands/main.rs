//! Concierge's commands, run against a temporary state directory and
//! credentials and a fake Ergo.

mod bot;
mod invite;
mod mail;
mod mailer;
mod member;
mod migrate;
mod remind;
mod status;

use std::{fs, path::Path};

use clap::Parser;
use concierge::{
    admin::{self, Command},
    config::{Config, ERGO_API_TOKEN_CREDENTIAL, IrcServer, Presence, SESSION_KEY_CREDENTIAL},
    db::{Db, NewMember, NewSession},
};
use ergo_api::{Passphrase, Registration, fake::FakeErgo};
use outbox::Outbox;
use tempfile::TempDir;

/// The password [`Fixture::register`] gives accounts.
const OLD_PASSWORD: &str = "correct-horse-battery";

struct Fixture {
    config: Config,
    ergo: FakeErgo,
    _state: TempDir,
    _outbox: TempDir,
    _credentials: TempDir,
}

impl Fixture {
    async fn start() -> Self {
        let ergo = FakeErgo::start().await;
        let [state, outbox, credentials] = [(); 3].map(|()| tempfile::tempdir().unwrap());
        let credential =
            |name, value: &str| fs::write(credentials.path().join(name), value).unwrap();
        credential(SESSION_KEY_CREDENTIAL, &"k".repeat(64));
        credential(ERGO_API_TOKEN_CREDENTIAL, &format!("{}\n", ergo.token()));
        let config = Config {
            listen: "127.0.0.1:8080".parse().unwrap(),
            public_url: "https://members.weblounge.localhost:10443".into(),
            invite_links: "https://join.weblounge.localhost:10443/#".into(),
            webchat_url: "https://irc.weblounge.localhost:10443".into(),
            irc_server: IrcServer {
                host: "irc.weblounge.localhost".into(),
                port: 16697,
            },
            ergo_api: ergo.url(),
            assets_dir: "/nix/store/asqgxh40xn390yzqhq97lkbqf5sslbyr-weblounge-web/members".into(),
            state_dir: state.path().into(),
            outbox_path: outbox.path().join("outbox.db"),
            credentials_dir: credentials.path().into(),
            presence_defaults: Presence {
                always_on: true,
                auto_away: true,
            },
        };
        Db::migrate(&config.database_path()).await.unwrap();
        Outbox::migrate(&config.outbox_path).await.unwrap();
        Self {
            config,
            ergo,
            _state: state,
            _outbox: outbox,
            _credentials: credentials,
        }
    }

    /// Runs `concierge <args>` and answers with what it prints.
    async fn run(&self, args: &[&str]) -> anyhow::Result<String> {
        let Command::Admin(command) = Command::try_parse_from(["concierge"].iter().chain(args))?
        else {
            panic!("{args:?} is not an admin command");
        };
        admin::run(command, &self.config).await
    }

    async fn db(&self) -> Db {
        Db::open(&self.config.database_path()).await.unwrap()
    }

    async fn outbox(&self) -> Outbox {
        Outbox::open(&self.config.outbox_path).await.unwrap()
    }

    fn state_dir(&self) -> &Path {
        &self.config.state_dir
    }

    async fn register(&self, account: &str) {
        let passphrase: Passphrase = OLD_PASSWORD.parse().unwrap();
        let registration = self.ergo.client().register(account, &passphrase).await;
        assert_eq!(registration.unwrap(), Registration::Created);
    }

    async fn add_member(&self, account: &str, invited_by: Option<&str>) {
        self.register(account).await;
        self.insert_member(account, invited_by).await;
    }

    async fn insert_member(&self, account: &str, invited_by: Option<&str>) {
        let member = NewMember {
            account,
            email: "someone@example.org",
            news_opt_in: false,
            invited_by,
            joined_at: 1_760_000_000,
            is_admin: false,
        };
        self.db().await.insert_member(&member).await.unwrap();
    }

    async fn sign_in(&self, account: &str, token_hash: &[u8]) {
        let session = NewSession {
            token_hash,
            account,
            created_at: 1_760_000_000,
            expires_at: 1_762_592_000,
        };
        self.db().await.insert_session(&session).await.unwrap();
    }

    async fn signs_in_to_ergo(&self, account: &str, password: &str) -> bool {
        let authenticated = self.ergo.client().check_auth(account, password).await;
        authenticated.unwrap().is_some()
    }
}
