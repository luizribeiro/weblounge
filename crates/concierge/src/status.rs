use std::{fs, os::unix::fs::MetadataExt, path::Path};

use anyhow::Context;
use ergo_api::Client;

use crate::{config::Config, db::Db};

const PROBE: &str = ".concierge-status-probe";

/// What an admin needs to see that this process runs with the service's
/// configuration, state and secrets, and reaches its database and Ergo.
pub async fn report(config: &Config) -> anyhow::Result<String> {
    let uid = writer_uid(&config.state_dir)?;
    let key = config.read_session_key()?;
    let ergo = config.ergo_client()?;
    let database = config.database_path();
    let db = Db::open(&database).await?;
    let migration = db
        .migration_version()
        .await?
        .map_or_else(|| "none".to_owned(), |version| version.to_string());
    let members = db.member_count().await?;
    Ok(format!(
        "public url: {}\n\
         state directory: {} (writable by uid {uid})\n\
         session key: {} bytes\n\
         database: {} (migration {migration}, {members} members)\n\
         ergo: {} {}\n",
        config.public_url,
        config.state_dir.display(),
        key.len(),
        database.display(),
        config.ergo_api,
        ergo_health(&ergo).await,
    ))
}

async fn ergo_health(ergo: &Client) -> String {
    match ergo.status().await {
        Ok(status) => format!("answers, version {}", status.version),
        Err(error) => format!("does not answer: {:#}", anyhow::Error::new(error)),
    }
}

/// The owner of a file this process writes, which tells an admin whose
/// identity it runs under.
fn writer_uid(dir: &Path) -> anyhow::Result<u32> {
    let probe = dir.join(PROBE);
    let written = fs::write(&probe, b"").and_then(|()| fs::metadata(&probe));
    let removed = fs::remove_file(&probe);
    written
        .and_then(|metadata| removed.map(|()| metadata.uid()))
        .with_context(|| format!("state directory {} is not writable", dir.display()))
}
