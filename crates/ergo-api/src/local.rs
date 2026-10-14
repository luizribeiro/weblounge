//! The real `ergo` binary on loopback, for contract tests and concierge's
//! dev loop: `$ERGO_BIN` on the base config at `$ERGO_CONFIG`, which the
//! dev shell and crane's test derivation set.

use std::{env, io, net::SocketAddr, path::PathBuf, process::Command};

use reqwest::Url;
use serde_json::json;

use crate::Client;

/// An Ergo whose datastore and IRC socket live in `dir` and whose API
/// listens on `api` and accepts `token`.
pub struct LocalErgo {
    bin: String,
    config: String,
    dir: PathBuf,
    api: SocketAddr,
    token: String,
}

impl LocalErgo {
    pub fn from_env(
        dir: impl Into<PathBuf>,
        api: SocketAddr,
        token: impl Into<String>,
    ) -> io::Result<Self> {
        Ok(Self {
            bin: required_env("ERGO_BIN")?,
            config: required_env("ERGO_CONFIG")?,
            dir: dir.into(),
            api,
            token: token.into(),
        })
    }

    pub fn url(&self) -> Url {
        format!("http://{}", self.api)
            .parse()
            .expect("a socket address makes a valid URL")
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn client(&self) -> Client {
        Client::new(self.url(), &self.token)
    }

    /// `ergo <subcommand>` on this instance's settings.
    pub fn command(&self, subcommand: &str) -> Command {
        let mut command = Command::new(&self.bin);
        command
            .args([subcommand, "--conf", &self.config, "--quiet"])
            .current_dir(&self.dir)
            .envs(self.overrides());
        command
    }

    /// The base config has no listeners, and overrides merge into maps, so
    /// the socket is the only one Ergo gets.
    fn overrides(&self) -> [(&'static str, String); 5] {
        let path = |name| self.dir.join(name).display().to_string();
        [
            (
                "ERGO__SERVER__LISTENERS",
                json!({ path("irc.sock"): {} }).to_string(),
            ),
            ("ERGO__DATASTORE__PATH", path("ircd.db")),
            ("ERGO__API__ENABLED", "true".into()),
            ("ERGO__API__LISTENER", self.api.to_string()),
            ("ERGO__API__BEARER_TOKENS", json!([self.token]).to_string()),
        ]
    }
}

fn required_env(name: &str) -> io::Result<String> {
    env::var(name).map_err(|_| {
        io::Error::other(format!(
            "{name} is unset; the dev shell and crane's test derivation set it"
        ))
    })
}
