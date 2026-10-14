use std::{
    env,
    fs::{self, OpenOptions},
    net::TcpListener,
    process::{Child, Command},
    thread,
    time::Duration,
};

use ergo_api::{Client, Error, Url};
use serde_json::json;
use tempfile::TempDir;

use crate::Backend;

const TOKEN: &str = "contract-test-token";
const STARTUP_ATTEMPTS: u32 = 100;
const STARTUP_INTERVAL: Duration = Duration::from_millis(50);
const LOG: &str = "ergo.log";

/// The `ergo` binary at `$ERGO_BIN`, running on `$ERGO_CONFIG` with its own
/// datastore and IRC socket in a temporary directory, and its API on a free
/// loopback port.
pub struct RealErgo {
    process: Child,
    url: Url,
    dir: TempDir,
}

impl Backend for RealErgo {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let api = free_loopback_address();
        let initdb = ergo(&dir, &api, "initdb").status().unwrap();
        assert!(initdb.success(), "ergo initdb failed\n{}", log(&dir));

        let mut ergo = Self {
            process: ergo(&dir, &api, "run").spawn().unwrap(),
            url: format!("http://{api}").parse().unwrap(),
            dir,
        };
        ergo.wait_until_ready().await;
        ergo
    }

    fn url(&self) -> Url {
        self.url.clone()
    }

    fn token(&self) -> &str {
        TOKEN
    }

    async fn stop(self) {
        drop(self)
    }
}

impl RealErgo {
    async fn wait_until_ready(&mut self) {
        let client = Client::new(self.url(), TOKEN);
        for _ in 0..STARTUP_ATTEMPTS {
            if let Some(status) = self.process.try_wait().unwrap() {
                panic!("ergo exited with {status}\n{}", log(&self.dir));
            }
            match client.status().await {
                Ok(_) => return,
                Err(Error::Transport(_)) => tokio::time::sleep(STARTUP_INTERVAL).await,
                Err(error) => panic!("ergo answered {error}\n{}", log(&self.dir)),
            }
        }
        panic!("ergo's API never came up\n{}", log(&self.dir));
    }
}

impl Drop for RealErgo {
    fn drop(&mut self) {
        self.process.kill().ok();
        self.process.wait().ok();
        if thread::panicking() {
            eprintln!("ergo's log:\n{}", log(&self.dir));
        }
    }
}

fn ergo(dir: &TempDir, api: &str, subcommand: &str) -> Command {
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.path().join(LOG))
        .unwrap();
    let mut command = Command::new(required_env("ERGO_BIN"));
    command
        .args([
            subcommand,
            "--conf",
            &required_env("ERGO_CONFIG"),
            "--quiet",
        ])
        .current_dir(dir.path())
        .envs(overrides(dir, api))
        .stdout(log.try_clone().unwrap())
        .stderr(log);
    command
}

fn log(dir: &TempDir) -> String {
    fs::read_to_string(dir.path().join(LOG)).unwrap_or_default()
}

fn path(dir: &TempDir, name: &str) -> String {
    dir.path().join(name).display().to_string()
}

fn required_env(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| {
        panic!("{name} is unset; the dev shell and crane's test derivation set it")
    })
}

fn free_loopback_address() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().to_string()
}

/// The base config has no listeners, and overrides merge into maps, so the
/// socket is the only one Ergo gets.
fn overrides(dir: &TempDir, api: &str) -> [(&'static str, String); 5] {
    [
        (
            "ERGO__SERVER__LISTENERS",
            json!({ path(dir, "irc.sock"): {} }).to_string(),
        ),
        ("ERGO__DATASTORE__PATH", path(dir, "ircd.db")),
        ("ERGO__API__ENABLED", "true".into()),
        ("ERGO__API__LISTENER", api.into()),
        ("ERGO__API__BEARER_TOKENS", json!([TOKEN]).to_string()),
    ]
}
