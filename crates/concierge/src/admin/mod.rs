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
    house,
    name::{HouseName, MemberName},
    status,
    vhost::Vhost,
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
    /// Delete the uploads in CONCIERGE_UPLOADS past their days, and files
    /// there no upload stands for.
    ExpireUploads,
    /// Create a house account, one of the network's own, with the password
    /// in its <name>-password credential, or set its password to it, and
    /// give it its vhost and always-on. Meant to run before what connects
    /// as it starts, every time.
    EnsureHouseAccount {
        #[arg(value_parser = |name: &str| house::parse_name(name))]
        name: HouseName,
        #[arg(value_parser = |vhost: &str| Vhost::parse(vhost))]
        vhost: Vhost,
    },
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
}

/// A generated password as the commands print it: alone on its line, the
/// only time anyone sees it.
fn print_once(password: &Passphrase) -> String {
    format!("{}\n", password.expose())
}

/// What's left to do once concierge let go of `account`.
fn erase_reminder(account: &MemberName) -> String {
    format!("An oper must still run /NS ERASE {account}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ensure_house_account(args: &[&str]) -> Result<(HouseName, Vhost), clap::Error> {
        let args = ["concierge", "ensure-house-account"].iter().chain(args);
        match Command::try_parse_from(args)? {
            Command::EnsureHouseAccount { name, vhost } => Ok((name, vhost)),
            _ => unreachable!(),
        }
    }

    #[test]
    fn ensures_a_house_account_by_its_name_and_vhost() {
        let (name, vhost) = ensure_house_account(&["bender", "weblounge/bender"]).unwrap();
        assert_eq!(name.as_str(), "bender");
        assert_eq!(vhost.as_str(), "weblounge/bender");
    }

    #[test]
    fn refuses_names_and_vhosts_ergo_wouldnt_take() {
        for args in [["2pac", "weblounge/2pac"], ["bender", "weblounge bender"]] {
            assert!(ensure_house_account(&args).is_err(), "{args:?}");
        }
    }
}
