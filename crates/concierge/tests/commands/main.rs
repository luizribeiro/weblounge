//! Concierge's commands, run against a temporary state directory and
//! credentials and a fake Ergo.

mod invite;
mod member;
mod status;

use std::{fs, path::Path};

use clap::Parser;
use concierge::{
    admin::{self, Command},
    config::{Config, ERGO_API_TOKEN_CREDENTIAL, IrcServer, SESSION_KEY_CREDENTIAL},
    db::Db,
};
use ergo_api::fake::FakeErgo;
use tempfile::TempDir;

struct Fixture {
    config: Config,
    ergo: FakeErgo,
    _state: TempDir,
    _credentials: TempDir,
}

impl Fixture {
    async fn start() -> Self {
        let ergo = FakeErgo::start().await;
        let (state, credentials) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let credential =
            |name, value: &str| fs::write(credentials.path().join(name), value).unwrap();
        credential(SESSION_KEY_CREDENTIAL, &"k".repeat(64));
        credential(ERGO_API_TOKEN_CREDENTIAL, &format!("{}\n", ergo.token()));
        let config = Config {
            listen: "127.0.0.1:8080".parse().unwrap(),
            public_url: "https://members.weblounge.localhost:10443".into(),
            irc_server: IrcServer {
                host: "irc.weblounge.localhost".into(),
                port: 16697,
            },
            ergo_api: ergo.url(),
            assets_dir: "/nix/store/asqgxh40xn390yzqhq97lkbqf5sslbyr-weblounge-web/members".into(),
            state_dir: state.path().into(),
            credentials_dir: credentials.path().into(),
        };
        Self {
            config,
            ergo,
            _state: state,
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

    fn state_dir(&self) -> &Path {
        &self.config.state_dir
    }
}
