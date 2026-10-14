use std::{
    fs::{self, OpenOptions},
    net::{SocketAddr, TcpListener},
    process::{Child, Command},
    thread,
    time::Duration,
};

use ergo_api::{Error, Url, local::LocalErgo};
use tempfile::TempDir;

use crate::Backend;

const TOKEN: &str = "contract-test-token";
const STARTUP_ATTEMPTS: u32 = 100;
const STARTUP_INTERVAL: Duration = Duration::from_millis(50);
const LOG: &str = "ergo.log";

/// A [`LocalErgo`] in a temporary directory, with its API on a free
/// loopback port and its output in a log shown when a test fails.
pub struct RealErgo {
    ergo: LocalErgo,
    process: Child,
    dir: TempDir,
}

impl Backend for RealErgo {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let ergo = LocalErgo::from_env(dir.path(), free_loopback_address(), TOKEN).unwrap();
        let initdb = logged(&dir, ergo.command("initdb")).status().unwrap();
        assert!(initdb.success(), "ergo initdb failed\n{}", log(&dir));

        let mut ergo = Self {
            process: logged(&dir, ergo.command("run")).spawn().unwrap(),
            ergo,
            dir,
        };
        ergo.wait_until_ready().await;
        ergo
    }

    fn url(&self) -> Url {
        self.ergo.url()
    }

    fn token(&self) -> &str {
        self.ergo.token()
    }

    async fn stop(self) {
        drop(self)
    }
}

impl RealErgo {
    async fn wait_until_ready(&mut self) {
        let client = self.ergo.client();
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

fn logged(dir: &TempDir, mut command: Command) -> Command {
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.path().join(LOG))
        .unwrap();
    command.stdout(log.try_clone().unwrap()).stderr(log);
    command
}

fn log(dir: &TempDir) -> String {
    fs::read_to_string(dir.path().join(LOG)).unwrap_or_default()
}

fn free_loopback_address() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}
