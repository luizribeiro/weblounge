use anyhow::Context;
use clap::Subcommand;
use ergo_api::{Passphrase, Preference, Registration, SettingsChanges};

use super::{Admin, print_once};
use crate::{
    db::{self, NewBot},
    nick::Nick,
    password,
};

#[derive(Subcommand)]
pub enum BotCommand {
    /// Register a bot account for a member, and print its generated password
    /// once.
    Create {
        owner: String,
        name: String,
        /// What the bot is for.
        #[arg(long)]
        note: Option<String>,
    },
}

pub(super) async fn run(command: BotCommand, admin: &Admin) -> anyhow::Result<String> {
    match command {
        BotCommand::Create { owner, name, note } => {
            create(admin, &owner, &name, note.as_deref()).await
        }
    }
}

async fn create(
    admin: &Admin,
    owner: &str,
    name: &str,
    note: Option<&str>,
) -> anyhow::Result<String> {
    let password = password::generate();
    register(admin, owner, name, note, &password).await?;
    Ok(print_once(&password))
}

/// Registers the Ergo account `name` with `password` and records it as a
/// bot of `owner`, answering its name.
async fn register(
    admin: &Admin,
    owner: &str,
    name: &str,
    note: Option<&str>,
    password: &Passphrase,
) -> anyhow::Result<Nick> {
    let owner = admin.current_member(owner).await?;
    let nick = Nick::parse(name).with_context(|| format!("{name} can't be a bot's name"))?;
    let taken = || anyhow::anyhow!("{nick} is taken");
    if admin.db.name_held(nick.as_str()).await? {
        return Err(taken());
    }
    let registration = admin
        .ergo
        .register(nick.clone(), password)
        .await
        .with_context(|| format!("Ergo didn't create {nick}"))?;
    if registration == Registration::NameTaken {
        return Err(taken());
    }
    admin
        .db
        .insert_bot(&NewBot {
            account: nick.as_str(),
            owner: &owner.account,
            note,
            created_at: db::now(),
        })
        .await
        .with_context(|| format!("Ergo created {nick}, but recording it as a bot failed"))?;
    stay_offline_when_stopped(admin, &nick)
        .await
        .with_context(|| {
            format!("{nick} was created, but not set up; bot reset-password {nick} tries again")
        })?;
    Ok(nick)
}

/// Turns the bot's always-on and auto-away off, so a stopped bot leaves the
/// channels instead of lingering in them.
async fn stay_offline_when_stopped(admin: &Admin, nick: &Nick) -> anyhow::Result<()> {
    let changes = SettingsChanges {
        always_on: Some(Preference::Off),
        auto_away: Some(Preference::Off),
        ..Default::default()
    };
    admin
        .ergo
        .update_settings(nick.clone(), changes)
        .await
        .with_context(|| format!("Ergo didn't turn {nick}'s always-on and auto-away off"))?;
    Ok(())
}
