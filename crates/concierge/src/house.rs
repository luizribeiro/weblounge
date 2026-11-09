//! House accounts: Ergo accounts of the network's own, neither members
//! nor bots, which the host declares in `weblounge.ergo.houseAccounts`.
//! concierge, which the greeter connects as, shows as
//! `concierge!~u@weblounge/concierge`, outside the `weblounge/bot/*` ban
//! that keeps members' bots out of #lounge, where it welcomes newcomers.

use std::fmt;

use anyhow::Context;
use ergo_api::{Client, Passphrase, Registration};

use crate::{
    config::HouseAccountConfig, db::Db, name::HouseName, nick, password_rules, vhost::Vhost,
};

/// The house accounts' names, which nobody may sign up or make a bot
/// under, and `member reconcile` expects in Ergo alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HouseAccounts(Vec<HouseName>);

impl HouseAccounts {
    /// The names in `names`, separated by commas: systemd's
    /// `Environment=` splits a unit's variables at spaces.
    pub fn parse(names: &str) -> Result<Self, nick::InvalidNick> {
        if names.is_empty() {
            return Ok(Self::default());
        }
        names
            .split(',')
            .map(parse_name)
            .collect::<Result<_, _>>()
            .map(Self)
    }

    /// Whether `name` is a house account's, in any case.
    pub fn contains(&self, name: &str) -> bool {
        self.0.contains(&HouseName::new(name))
    }
}

/// The systemd credential `name`'s generated password reaches what
/// connects as it in.
pub fn password_credential(name: &HouseName) -> String {
    format!("{name}-password")
}

/// `name`, if it could be a house account's: one that follows the rules
/// for nicks.
pub fn parse_name(name: &str) -> Result<HouseName, nick::InvalidNick> {
    nick::check_shape(name)?;
    Ok(HouseName::new(name))
}

/// What [`ensure`] did to the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ensured {
    Created,
    Updated,
}

impl fmt::Display for Ensured {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Created => "created",
            Self::Updated => "updated",
        })
    }
}

/// [`ensure`]s `name` with the password in its credential.
pub async fn ensure_account(
    config: &HouseAccountConfig,
    name: &HouseName,
    vhost: &Vhost,
) -> anyhow::Result<Ensured> {
    let credential = config.read_password(name)?;
    let password = credential.expose();
    let password = password_rules::check(password, password).with_context(|| {
        format!(
            "the {} credential doesn't hold a usable password",
            password_credential(name)
        )
    })?;
    let db = Db::open(&config.database).await?;
    ensure(&db, &config.ergo_client()?, name, vhost, &password).await
}

/// Registers `name` with `password`, or sets its password to it, and
/// gives it `vhost`. Refuses while a member or a bot holds the name,
/// which only one from before it was reserved could.
pub async fn ensure(
    db: &Db,
    ergo: &Client,
    name: &HouseName,
    vhost: &Vhost,
    password: &Passphrase,
) -> anyhow::Result<Ensured> {
    let held = db.held_by_member(name.as_str()).await? || db.bot(name.as_str()).await?.is_some();
    anyhow::ensure!(
        !held,
        "a member or a bot holds {name}, a house account's name"
    );
    let ensured = match ergo.register(name, password).await? {
        Registration::Created => Ensured::Created,
        Registration::NameTaken => {
            ergo.set_password(name, password).await?;
            Ensured::Updated
        }
    };
    ergo.set_vhost(name, vhost.as_str())
        .await
        .with_context(|| format!("Ergo didn't give {name} its vhost"))?;
    Ok(ensured)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_the_house_accounts_in_any_case() {
        let house = HouseAccounts::parse("concierge,bender").unwrap();
        for name in ["concierge", "Bender", "CONCIERGE"] {
            assert!(house.contains(name), "{name}");
        }
        for name in ["concierge2", "luiz", "NickServ", ""] {
            assert!(!house.contains(name), "{name}");
        }
    }

    #[test]
    fn there_may_be_no_house_accounts_but_each_is_a_name() {
        assert_eq!(HouseAccounts::parse(""), Ok(HouseAccounts::default()));
        assert_eq!(
            HouseAccounts::parse("concierge,2pac"),
            Err(nick::InvalidNick::StartsWithoutALetter)
        );
    }

    #[test]
    fn a_house_accounts_password_is_in_the_credential_named_after_it() {
        let name = parse_name("bender").unwrap();
        assert_eq!(password_credential(&name), "bender-password");
    }

    #[test]
    fn house_accounts_follow_the_rules_for_nicks_but_may_be_reserved() {
        assert_eq!(parse_name("concierge").unwrap().as_str(), "concierge");
        assert_eq!(
            parse_name("2pac"),
            Err(nick::InvalidNick::StartsWithoutALetter)
        );
        assert_eq!(parse_name("a b"), Err(nick::InvalidNick::Forbidden(' ')));
    }
}
