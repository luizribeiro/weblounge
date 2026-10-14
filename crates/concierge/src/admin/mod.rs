//! The commands admins run on the server through concierge-admin. Each
//! prints plain lines a script can read.

mod invite;

use clap::{Parser, Subcommand};

use crate::{config::Config, db::Db, status};

/// The members' site of weblounge.org, and the admin commands behind it.
#[derive(Parser)]
pub enum Command {
    /// Serve the site on CONCIERGE_LISTEN.
    Serve,
    #[command(flatten)]
    Admin(AdminCommand),
}

#[derive(Subcommand)]
pub enum AdminCommand {
    /// Show the configuration, state, secrets, database and Ergo this process sees.
    Status,
    /// Mint invites.
    #[command(subcommand)]
    Invite(invite::InviteCommand),
}

/// Runs `command` and answers with what it prints.
pub async fn run(command: AdminCommand, config: &Config) -> anyhow::Result<String> {
    match command {
        AdminCommand::Status => status::report(config).await,
        AdminCommand::Invite(command) => invite::run(command, &Admin::connect(config).await?).await,
    }
}

/// What the commands work with.
struct Admin {
    config: Config,
    db: Db,
}

impl Admin {
    async fn connect(config: &Config) -> anyhow::Result<Self> {
        Ok(Self {
            config: config.clone(),
            db: Db::open(&config.database_path()).await?,
        })
    }
}
