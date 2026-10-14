//! Concierge's commands, run against a temporary state directory and
//! credentials and a fake Ergo.

mod status;

use std::{fs, path::Path};

use concierge::config::{Config, ERGO_API_TOKEN_CREDENTIAL, SESSION_KEY_CREDENTIAL};
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

    fn state_dir(&self) -> &Path {
        &self.config.state_dir
    }
}
