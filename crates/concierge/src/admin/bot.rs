use anyhow::Context;
use clap::Subcommand;
use ergo_api::{Passphrase, Preference, Registration, SettingsChanges};

use super::{Admin, erase_reminder, print_once};
use crate::{
    db::{self, Bot, NewBot},
    nick::Nick,
    password, tree,
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
    /// List the bots, of one member or of everyone: name, owner, creation
    /// date and note.
    List { owner: Option<String> },
    /// Give a bot a new generated password, and print it once. Turns its
    /// always-on and auto-away off again too.
    ResetPassword { name: String },
    /// Lock a bot out with a password nobody knows and mark it deleted,
    /// keeping its name taken.
    Delete { name: String },
}

pub(super) async fn run(command: BotCommand, admin: &Admin) -> anyhow::Result<String> {
    match command {
        BotCommand::Create { owner, name, note } => {
            create(admin, &owner, &name, note.as_deref()).await
        }
        BotCommand::List { owner } => list(admin, owner.as_deref()).await,
        BotCommand::ResetPassword { name } => reset_password(admin, &name).await,
        BotCommand::Delete { name } => delete(admin, &name).await,
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

async fn list(admin: &Admin, owner: Option<&str>) -> anyhow::Result<String> {
    if let Some(owner) = owner {
        admin
            .db
            .member(owner)
            .await?
            .with_context(|| format!("{owner} is not a member"))?;
    }
    let bots = admin.db.bots(owner).await?;
    Ok(bots
        .into_iter()
        .map(|bot| {
            let fields = [bot.account, bot.owner, tree::date(bot.created_at)];
            let fields: Vec<_> = fields.into_iter().chain(bot.note).collect();
            fields.join(" ") + "\n"
        })
        .collect())
}

async fn reset_password(admin: &Admin, name: &str) -> anyhow::Result<String> {
    let bot = live_bot(admin, name).await?;
    stay_offline_when_stopped(admin, &bot.account).await?;
    let password = admin.set_generated_password(&bot.account).await?;
    Ok(print_once(&password))
}

async fn delete(admin: &Admin, name: &str) -> anyhow::Result<String> {
    let bot = live_bot(admin, name).await?;
    remove(admin, &bot.account).await
}

/// Locks the live bot `account` out and marks it deleted, answering what's
/// left for an oper to do.
pub(super) async fn remove(admin: &Admin, account: &str) -> anyhow::Result<String> {
    admin.set_generated_password(account).await?;
    admin.db.remove_bot(account, db::now()).await?;
    Ok(format!(
        "removed bot {account}\n{}",
        erase_reminder(account)
    ))
}

/// The bot named `name`, unless there's none or it was deleted.
async fn live_bot(admin: &Admin, name: &str) -> anyhow::Result<Bot> {
    let bot = admin
        .db
        .bot(name)
        .await?
        .with_context(|| format!("{name} is not a bot"))?;
    anyhow::ensure!(bot.removed_at.is_none(), "{} was deleted", bot.account);
    Ok(bot)
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
    stay_offline_when_stopped(admin, nick.as_str())
        .await
        .with_context(|| {
            format!("{nick} was created, but not set up; bot reset-password {nick} tries again")
        })?;
    Ok(nick)
}

/// Turns the bot's always-on and auto-away off, so a stopped bot leaves the
/// channels instead of lingering in them.
async fn stay_offline_when_stopped(admin: &Admin, account: &str) -> anyhow::Result<()> {
    let changes = SettingsChanges {
        always_on: Some(Preference::Off),
        auto_away: Some(Preference::Off),
        ..Default::default()
    };
    admin
        .ergo
        .update_settings(account, changes)
        .await
        .with_context(|| format!("Ergo didn't turn {account}'s always-on and auto-away off"))?;
    Ok(())
}
