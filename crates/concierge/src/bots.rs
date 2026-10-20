//! Members' bots: Ergo accounts concierge records as theirs. The admin
//! commands and the Bots tab both go through these.

use anyhow::Context;
use ergo_api::{Client, Erasure, Passphrase, Preference, Registration, SettingsChanges};

use crate::{
    bot_rules,
    db::{self, Bot, BotInsertion, Db, NewBot},
    name::{BotName, MemberName},
    nick::{self, InvalidNick, Nick},
    password,
    uploads::Store,
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
        let nick = match Nick::parse(name) {
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
        let registration = self.ergo.register(&account, password).await;
        if !matches!(registration, Ok(Registration::Created)) {
            self.db.take_back_bot(&account, replacing.as_ref()).await?;
        }
        match registration.with_context(|| format!("Ergo didn't create {account}"))? {
            Registration::Created => {}
            Registration::NameTaken => return Ok(Err(Refusal::Taken)),
        }
        self.set_up(&account, owner).await.with_context(|| {
            format!(
                "{account} was created, but not set up; bot reset-password {account} tries again"
            )
        })?;
        Ok(Ok(account))
    }

    /// Sets the live bot `account` of `owner` up again and gives it a new
    /// generated password, answering it.
    pub async fn reset_password(
        &self,
        account: &BotName,
        owner: &MemberName,
    ) -> anyhow::Result<Passphrase> {
        self.set_up(account, owner).await?;
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

    /// Turns the bot's always-on and auto-away off and gives it its owner's
    /// vhost.
    pub async fn set_up(&self, account: &BotName, owner: &MemberName) -> anyhow::Result<()> {
        self.stay_offline_when_stopped(account).await?;
        self.set_vhost(account, owner).await
    }

    pub async fn set_vhost(&self, account: &BotName, owner: &MemberName) -> anyhow::Result<()> {
        self.ergo
            .set_vhost(account, vhost(owner))
            .await
            .with_context(|| format!("Ergo didn't give {account} its vhost"))
    }

    /// Turns the bot's always-on and auto-away off, so a stopped bot leaves
    /// the channels instead of lingering in them.
    async fn stay_offline_when_stopped(&self, account: &BotName) -> anyhow::Result<()> {
        let changes = SettingsChanges {
            always_on: Some(Preference::Off),
            auto_away: Some(Preference::Off),
            ..Default::default()
        };
        self.ergo
            .update_settings(account, changes)
            .await
            .with_context(|| format!("Ergo didn't turn {account}'s always-on and auto-away off"))?;
        Ok(())
    }
}

/// The host everyone sees `owner`'s bots connect from, so WHOIS and joins
/// say whose bot it is, and one ban keeps every bot out of a channel.
fn vhost(owner: &MemberName) -> String {
    format!("{VHOST_PREFIX}{owner}")
}

const VHOST_PREFIX: &str = "weblounge/bot/";

/// Ergo's default `accounts.vhosts.max-length`, which owners' names stay
/// within.
const ERGO_VHOST_MAX_LENGTH: usize = 64;
const _: () = assert!(VHOST_PREFIX.len() + nick::MAX_LENGTH <= ERGO_VHOST_MAX_LENGTH);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_owner_under_the_networks_bot_namespace() {
        assert_eq!(vhost(&MemberName::new("luiz")), "weblounge/bot/luiz");
        assert_eq!(vhost(&MemberName::new("Zoe")), "weblounge/bot/Zoe");
    }
}
