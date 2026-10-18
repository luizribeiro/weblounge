use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::Context;
use clap::Subcommand;
use ergo_api::{Passphrase, Preference, Registration, SettingsChanges};

use super::{Admin, erase_reminder, print_once};
use crate::{
    db::{self, Bot, Member, NewBot},
    nick::{self, Nick},
    password, password_rules, tree,
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
    /// always-on and auto-away off and sets its vhost again too.
    ResetPassword { name: String },
    /// Lock a bot out with a password nobody knows and mark it deleted,
    /// keeping its name taken.
    Delete { name: String },
    /// Create a bot whose password is in a file, or set an existing bot's
    /// password to the file's. Meant to run before the bot starts, every
    /// time.
    Ensure {
        owner: String,
        name: String,
        /// The file holding the password, or - for standard input.
        #[arg(long)]
        password_file: PathBuf,
    },
}

pub(super) async fn run(command: BotCommand, admin: &Admin) -> anyhow::Result<String> {
    match command {
        BotCommand::Create { owner, name, note } => {
            create(admin, &owner, &name, note.as_deref()).await
        }
        BotCommand::List { owner } => list(admin, owner.as_deref()).await,
        BotCommand::ResetPassword { name } => reset_password(admin, &name).await,
        BotCommand::Delete { name } => delete(admin, &name).await,
        BotCommand::Ensure {
            owner,
            name,
            password_file,
        } => ensure(admin, &owner, &name, &password_file).await,
    }
}

async fn create(
    admin: &Admin,
    owner: &str,
    name: &str,
    note: Option<&str>,
) -> anyhow::Result<String> {
    let owner = admin.current_member(owner).await?;
    let password = password::generate();
    register(admin, &owner, name, note, &password).await?;
    Ok(print_once(&password))
}

async fn ensure(
    admin: &Admin,
    owner: &str,
    name: &str,
    password_file: &Path,
) -> anyhow::Result<String> {
    let password = read_password(password_file)?;
    let owner = admin.current_member(owner).await?;
    let Some(bot) = admin.db.bot(name).await? else {
        let nick = register(admin, &owner, name, None, &password).await?;
        return Ok(format!("created {nick}\n"));
    };
    ensure_live(&bot)?;
    anyhow::ensure!(
        bot.owner == owner.account,
        "{} belongs to {}",
        bot.account,
        bot.owner
    );
    set_up(admin, &bot.account, &bot.owner).await?;
    admin.set_password(&bot.account, &password).await?;
    Ok(format!("updated {}\n", bot.account))
}

/// The password in `path`, or on standard input for `-`, without the
/// newline that may end it.
fn read_password(path: &Path) -> anyhow::Result<Passphrase> {
    let contents = if path == Path::new("-") {
        io::read_to_string(io::stdin()).context("can't read standard input")?
    } else {
        fs::read_to_string(path).with_context(|| format!("can't read {}", path.display()))?
    };
    let password = contents.strip_suffix('\n').unwrap_or(&contents);
    password_rules::check(password, password)
        .with_context(|| format!("{} doesn't hold a usable password", path.display()))
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
    set_up(admin, &bot.account, &bot.owner).await?;
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
    ensure_live(&bot)?;
    Ok(bot)
}

fn ensure_live(bot: &Bot) -> anyhow::Result<()> {
    anyhow::ensure!(bot.removed_at.is_none(), "{} was deleted", bot.account);
    Ok(())
}

/// Records `name` as a bot of `owner` and registers its Ergo account with
/// `password`, answering its name.
async fn register(
    admin: &Admin,
    owner: &Member,
    name: &str,
    note: Option<&str>,
    password: &Passphrase,
) -> anyhow::Result<Nick> {
    let nick = Nick::parse(name).with_context(|| format!("{name} can't be a bot's name"))?;
    let taken = || anyhow::anyhow!("{nick} is taken");
    if admin.db.name_held(nick.as_str()).await? {
        return Err(taken());
    }
    // Recorded first, so an account Ergo creates always has its row; the
    // row goes again if Ergo creates nothing.
    admin
        .db
        .insert_bot(&NewBot {
            account: nick.as_str(),
            owner: &owner.account,
            note,
            created_at: db::now(),
        })
        .await?;
    let registration = admin.ergo.register(nick.clone(), password).await;
    if !matches!(registration, Ok(Registration::Created)) {
        admin.db.forget_bot(nick.as_str()).await?;
    }
    match registration.with_context(|| format!("Ergo didn't create {nick}"))? {
        Registration::Created => {}
        Registration::NameTaken => return Err(taken()),
    }
    set_up(admin, nick.as_str(), &owner.account)
        .await
        .with_context(|| {
            format!("{nick} was created, but not set up; bot reset-password {nick} tries again")
        })?;
    Ok(nick)
}

/// The host everyone sees `owner`'s bots connect from, so WHOIS and joins
/// say whose bot it is, and one ban keeps every bot out of a channel.
fn vhost(owner: &str) -> String {
    format!("{VHOST_PREFIX}{owner}")
}

const VHOST_PREFIX: &str = "weblounge/bot/";

/// Ergo's default `accounts.vhosts.max-length`, which owners' names stay
/// within.
const ERGO_VHOST_MAX_LENGTH: usize = 64;
const _: () = assert!(VHOST_PREFIX.len() + nick::MAX_LENGTH <= ERGO_VHOST_MAX_LENGTH);

async fn set_up(admin: &Admin, account: &str, owner: &str) -> anyhow::Result<()> {
    stay_offline_when_stopped(admin, account).await?;
    set_vhost(admin, account, owner).await
}

async fn set_vhost(admin: &Admin, account: &str, owner: &str) -> anyhow::Result<()> {
    admin
        .ergo
        .set_vhost(account, vhost(owner))
        .await
        .with_context(|| format!("Ergo didn't give {account} its vhost"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_owner_under_the_networks_bot_namespace() {
        assert_eq!(vhost("luiz"), "weblounge/bot/luiz");
        assert_eq!(vhost("Zoe"), "weblounge/bot/Zoe");
    }
}
