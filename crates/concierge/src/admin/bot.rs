use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::Context;
use clap::Subcommand;
use ergo_api::Passphrase;

use super::{Admin, print_once};
use crate::{
    bots::{Bots, Refusal},
    db::{Bot, Member},
    name::BotName,
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
    /// Erase a bot's account, disconnecting it, and mark it deleted. Its
    /// owner may reuse its name at once; others once the chat history has
    /// expired for it.
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
    /// A migration: give every live bot its owner's vhost and print each
    /// one's name. Bots from before vhosts need it once; create, ensure
    /// and reset-password set it since.
    ApplyVhosts,
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
        BotCommand::ApplyVhosts => apply_vhosts(admin).await,
    }
}

impl Admin {
    fn bots(&self) -> Bots<'_> {
        Bots {
            db: &self.db,
            ergo: &self.ergo,
            history_expiry: self.config.history_expiry,
        }
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
    admin.bots().set_up(&bot.account, &bot.owner).await?;
    password::set(&admin.ergo, &bot.account, &password).await?;
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
    let owner = match owner {
        Some(owner) => {
            let member = admin.db.member(owner).await?;
            Some(member.with_context(|| format!("{owner} is not a member"))?)
        }
        None => None,
    };
    let bots = admin
        .db
        .bots(owner.as_ref().map(|owner| &owner.account))
        .await?;
    Ok(bots
        .into_iter()
        .map(|bot| {
            let fields = [
                bot.account.to_string(),
                bot.owner.to_string(),
                tree::date(bot.created_at),
            ];
            let fields: Vec<_> = fields.into_iter().chain(bot.note).collect();
            fields.join(" ") + "\n"
        })
        .collect())
}

/// Deleted bots are left alone: Ergo erased them, or, for bots deleted
/// before it could, nobody can sign in to them.
async fn apply_vhosts(admin: &Admin) -> anyhow::Result<String> {
    let bots = admin.db.bots(None).await?;
    let mut failures = Vec::new();
    for bot in &bots {
        if let Err(error) = admin.bots().set_vhost(&bot.account, &bot.owner).await {
            failures.push(format!("{error:#}"));
        }
    }
    anyhow::ensure!(
        failures.is_empty(),
        "could not set the vhosts of {} of {} bots:\n{}",
        failures.len(),
        bots.len(),
        failures.join("\n")
    );
    Ok(bots
        .iter()
        .map(|bot| format!("{}\n", bot.account))
        .collect())
}

async fn reset_password(admin: &Admin, name: &str) -> anyhow::Result<String> {
    let bot = live_bot(admin, name).await?;
    let password = admin
        .bots()
        .reset_password(&bot.account, &bot.owner)
        .await?;
    Ok(print_once(&password))
}

async fn delete(admin: &Admin, name: &str) -> anyhow::Result<String> {
    let bot = live_bot(admin, name).await?;
    remove(admin, &bot.account).await
}

/// Erases the live bot `account` and marks it deleted, answering what it
/// did.
pub(super) async fn remove(admin: &Admin, account: &BotName) -> anyhow::Result<String> {
    admin.bots().remove(account).await?;
    Ok(format!("removed bot {account}\n"))
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

/// Creates `name` as a bot of `owner` with `password`, answering its name,
/// with the refusals put the way the commands print them. Admins aren't
/// held to the members' cap.
async fn register(
    admin: &Admin,
    owner: &Member,
    name: &str,
    note: Option<&str>,
    password: &Passphrase,
) -> anyhow::Result<BotName> {
    match admin
        .bots()
        .create(&owner.account, name, note, password, None)
        .await?
    {
        Ok(bot) => Ok(bot),
        Err(Refusal::InvalidName(invalid)) => {
            Err(anyhow::Error::new(invalid).context(format!("{name} can't be a bot's name")))
        }
        Err(Refusal::Taken) => anyhow::bail!("{name} is taken"),
        Err(Refusal::TooMany) => unreachable!("admins make as many bots as they like"),
    }
}
