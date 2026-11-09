//! Members' bots: Ergo accounts concierge records as theirs. The admin
//! commands and the Bots tab both go through these.

use anyhow::Context;
use ergo_api::{Client, Erasure, Passphrase, Registration};

use crate::{
    bot_rules,
    db::{self, Bot, BotInsertion, Db, NewBot},
    house::HouseAccounts,
    name::{BotName, MemberName},
    nick::{self, InvalidNick, Nick},
    password,
    registration::{self, LOST_REPLY_CHECKS},
    uploads::Store,
    vhost,
};

/// What bot changes work with.
pub struct Bots<'a> {
    pub db: &'a Db,
    pub ergo: &'a Client,
    /// How long Ergo keeps chat history, in seconds, which deleted bots'
    /// names wait out before others may take them.
    pub history_expiry: i64,
    /// Where bots' uploads are, which go with them.
    pub uploads: &'a Store,
    /// The names nobody may make a bot under.
    pub house_accounts: &'a HouseAccounts,
}

/// Why a bot wasn't created, in terms its owner can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    InvalidName(InvalidNick),
    /// A member holds the name or once did, or a bot does or did lately.
    Taken,
    /// The owner has as many live bots as they may.
    TooMany,
}

/// Whether someone may take a name, as far as concierge knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameClaim {
    Free,
    /// A deleted bot's name, free to the claimant; a new bot takes its row.
    Reuses(Bot),
    Held,
}

/// Whether `claimant`, a member making a bot, or None for someone signing
/// up, may take `name`, as far as concierge knows: members' names stay
/// theirs, and deleted bots' follow [`bot_rules::deleted_name_free`].
pub async fn name_claim(
    db: &Db,
    name: &str,
    claimant: Option<&MemberName>,
    history_expiry: i64,
) -> sqlx::Result<NameClaim> {
    if db.held_by_member(name).await? {
        return Ok(NameClaim::Held);
    }
    let Some(bot) = db.bot(name).await? else {
        return Ok(NameClaim::Free);
    };
    Ok(match bot.removed_at {
        Some(removed_at)
            if bot_rules::deleted_name_free(
                &bot.owner,
                removed_at,
                claimant,
                db::now(),
                history_expiry,
            ) =>
        {
            NameClaim::Reuses(bot)
        }
        _ => NameClaim::Held,
    })
}

impl Bots<'_> {
    /// Records `name` as a bot of `owner` and registers its Ergo account
    /// with `password`, answering its name, or why it can't be created.
    /// With `max_live`, it's created only while `owner` has fewer live bots.
    pub async fn create(
        &self,
        owner: &MemberName,
        name: &str,
        note: Option<&str>,
        password: &Passphrase,
        max_live: Option<i64>,
    ) -> anyhow::Result<Result<BotName, Refusal>> {
        let nick = match Nick::parse(name, self.house_accounts) {
            Ok(nick) => nick,
            Err(invalid) => return Ok(Err(Refusal::InvalidName(invalid))),
        };
        let replacing =
            match name_claim(self.db, nick.as_str(), Some(owner), self.history_expiry).await? {
                NameClaim::Free => None,
                NameClaim::Reuses(deleted) => Some(deleted),
                NameClaim::Held => return Ok(Err(Refusal::Taken)),
            };
        let account = BotName::from(nick);
        // Recorded first, so an account Ergo creates always has its row; the
        // row is taken back if Ergo creates nothing.
        let bot = NewBot {
            account: &account,
            owner,
            note,
            created_at: db::now(),
        };
        match self
            .db
            .insert_bot(&bot, replacing.as_ref(), max_live)
            .await?
        {
            BotInsertion::Inserted => {}
            BotInsertion::NameTaken => return Ok(Err(Refusal::Taken)),
            BotInsertion::TooMany => return Ok(Err(Refusal::TooMany)),
        }
        let registered = self.register(&account, password).await;
        if !matches!(registered, Ok(true)) {
            self.db.take_back_bot(&account, replacing.as_ref()).await?;
        }
        if !registered.with_context(|| format!("Ergo didn't create {account}"))? {
            return Ok(Err(Refusal::Taken));
        }
        self.set_vhost(&account, owner).await.with_context(|| {
            format!(
                "{account} was created, but not set up; bot reset-password {account} tries again"
            )
        })?;
        Ok(Ok(account))
    }

    /// Registers `account` with `password`, answering whether the account
    /// is the bot's: Ergo created it, or `password` opens the account Ergo
    /// has under the name, as when Ergo didn't answer, or an earlier try
    /// whose answer was lost made it. The bot's row holds the name by
    /// now, so concierge knows of no member or other bot under it; that
    /// the password, generated or the bot's own secret, opens it is what
    /// proves the account Ergo has is the bot's.
    async fn register(&self, account: &BotName, password: &Passphrase) -> ergo_api::Result<bool> {
        match self.ergo.register(account, password).await {
            Ok(Registration::Created) => Ok(true),
            // TODO: bot ensure adopts any account Ergo has that concierge
            // has no record of, such as the house account concierge, when
            // the password opens it. Revisit once registration moves to the
            // papo server.
            Ok(Registration::NameTaken) => self.opens(account, password, 1).await,
            Err(error) if error.unanswered() => {
                eprintln!("Ergo didn't answer creating {account}: {error}");
                match self.opens(account, password, LOST_REPLY_CHECKS).await {
                    Ok(true) => Ok(true),
                    Ok(false) => Err(error),
                    Err(check) => {
                        eprintln!("Checking whether Ergo made {account} failed: {check}");
                        Err(error)
                    }
                }
            }
            Err(error) => Err(error),
        }
    }

    async fn opens(
        &self,
        account: &BotName,
        password: &Passphrase,
        checks: usize,
    ) -> ergo_api::Result<bool> {
        let opened = registration::opened(self.ergo, account, password, checks).await?;
        Ok(opened.is_some())
    }

    /// Gives the live bot `account` of `owner` its vhost again and a new
    /// generated password, answering it.
    pub async fn reset_password(
        &self,
        account: &BotName,
        owner: &MemberName,
    ) -> anyhow::Result<Passphrase> {
        self.set_vhost(account, owner).await?;
        password::set_generated(self.ergo, account).await
    }

    /// Deletes the live bot `account`'s uploads, erases it in Ergo, which
    /// disconnects it and frees the name there, and marks it deleted. An
    /// account Ergo has no record of was erased already.
    pub async fn remove(&self, account: &BotName) -> anyhow::Result<()> {
        let uploads = self.db.uploads_by(account).await?;
        self.uploads.delete(self.db, &uploads).await?;
        let _: Erasure = self
            .ergo
            .erase(account)
            .await
            .with_context(|| format!("Ergo didn't erase {account}"))?;
        self.db.remove_bot(account, db::now()).await?;
        Ok(())
    }

    pub async fn set_vhost(&self, account: &BotName, owner: &MemberName) -> anyhow::Result<()> {
        self.ergo
            .set_vhost(account, vhost(owner))
            .await
            .with_context(|| format!("Ergo didn't give {account} its vhost"))
    }
}

/// The host everyone sees `owner`'s bots connect from, so WHOIS and joins
/// say whose bot it is, and one ban keeps every bot out of a channel.
fn vhost(owner: &MemberName) -> String {
    format!("{VHOST_PREFIX}{owner}")
}

const VHOST_PREFIX: &str = "weblounge/bot/";

/// Owners' names keep their bots' vhosts within Ergo's limit.
const _: () = assert!(VHOST_PREFIX.len() + nick::MAX_LENGTH <= vhost::MAX_LENGTH);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_owner_under_the_networks_bot_namespace() {
        assert_eq!(vhost(&MemberName::new("luiz")), "weblounge/bot/luiz");
        assert_eq!(vhost(&MemberName::new("Zoe")), "weblounge/bot/Zoe");
    }
}
