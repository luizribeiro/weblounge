//! The commands admins run on the server through concierge-admin. Each
//! prints plain lines a script can read.

mod bot;
mod invite;
mod mail;
mod member;

use anyhow::Context;
use clap::{Parser, Subcommand};
use ergo_api::{Client, Passphrase};
use outbox::Outbox;

use crate::{
    config::Config,
    db::{Db, Member},
    password, status,
};

/// The members' site of weblounge.org, and the admin commands behind it.
#[derive(Parser)]
pub enum Command {
    /// Bring the database in STATE_DIRECTORY, and the outbox at
    /// CONCIERGE_OUTBOX, up to this build's schema.
    Migrate,
    /// Serve the site on CONCIERGE_LISTEN.
    Serve,
    /// Mail a fresh link to confirm their email, leading to
    /// CONCIERGE_PUBLIC_URL, to every member whose one reminder is due, a
    /// batch at a time.
    Remind,
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
    /// Manage members.
    #[command(subcommand)]
    Member(member::MemberCommand),
    /// Manage members' bots.
    #[command(subcommand)]
    Bot(bot::BotCommand),
    /// Check that mail goes out.
    #[command(subcommand)]
    Mail(mail::MailCommand),
}

/// Runs `command` and answers with what it prints.
pub async fn run(command: AdminCommand, config: &Config) -> anyhow::Result<String> {
    match command {
        AdminCommand::Status => status::report(config).await,
        AdminCommand::Invite(command) => invite::run(command, &Admin::connect(config).await?).await,
        AdminCommand::Member(command) => member::run(command, &Admin::connect(config).await?).await,
        AdminCommand::Bot(command) => bot::run(command, &Admin::connect(config).await?).await,
        AdminCommand::Mail(command) => mail::run(command, &Admin::connect(config).await?).await,
    }
}

/// What the commands work with.
struct Admin {
    config: Config,
    db: Db,
    outbox: Outbox,
    ergo: Client,
}

impl Admin {
    async fn connect(config: &Config) -> anyhow::Result<Self> {
        Ok(Self {
            config: config.clone(),
            db: Db::open(&config.database_path()).await?,
            outbox: Outbox::open(&config.outbox_path).await?,
            ergo: config.ergo_client()?,
        })
    }

    /// The member named `name`, unless there's none or they were removed.
    async fn current_member(&self, name: &str) -> anyhow::Result<Member> {
        let member = self
            .db
            .member(name)
            .await?
            .with_context(|| format!("{name} is not a member"))?;
        anyhow::ensure!(
            member.removed_at.is_none(),
            "{} was removed",
            member.account
        );
        Ok(member)
    }

    /// Replaces `account`'s password in Ergo.
    async fn set_password(&self, account: &str, password: &Passphrase) -> anyhow::Result<()> {
        self.ergo
            .set_password(account, password)
            .await
            .with_context(|| format!("Ergo didn't take {account}'s new password"))
    }

    /// Gives `account` a new generated password in Ergo, and answers it.
    async fn set_generated_password(&self, account: &str) -> anyhow::Result<Passphrase> {
        let password = password::generate();
        self.set_password(account, &password).await?;
        Ok(password)
    }
}

/// A generated password as the commands print it: alone on its line, the
/// only time anyone sees it.
fn print_once(password: &Passphrase) -> String {
    format!("{}\n", password.as_str())
}

/// What's left to do once concierge let go of `account`.
fn erase_reminder(account: &str) -> String {
    format!("Ergo's API can't delete accounts: an oper must run /NS ERASE {account}\n")
}
