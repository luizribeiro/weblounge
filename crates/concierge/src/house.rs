//! concierge, the network's own account, which the greeter connects as:
//! the house, neither a member nor a bot. It shows as
//! `concierge!~u@weblounge/concierge`, outside the `weblounge/bot/*` ban
//! that keeps members' bots out of #lounge, where it welcomes newcomers.

use anyhow::Context;
use ergo_api::{Client, Passphrase, Preference, Registration, SettingsChanges};

use crate::db::Db;

pub const ACCOUNT: &str = "concierge";
pub const VHOST: &str = "weblounge/concierge";

/// What [`ensure`] did to the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ensured {
    Created,
    Updated,
}

/// Registers the house account with `password`, or sets its password to
/// it, and gives it its vhost and always-on, so restarting the greeter
/// shows no quit or join. Refuses while a member or a bot holds the
/// name, which only one from before it was reserved could.
pub async fn ensure(db: &Db, ergo: &Client, password: &Passphrase) -> anyhow::Result<Ensured> {
    anyhow::ensure!(
        !db.held_by_member(ACCOUNT).await? && db.bot(ACCOUNT).await?.is_none(),
        "a member or a bot holds {ACCOUNT}, the greeter's name"
    );
    let ensured = match ergo.register(ACCOUNT, password).await? {
        Registration::Created => Ensured::Created,
        Registration::NameTaken => {
            ergo.set_password(ACCOUNT, password).await?;
            Ensured::Updated
        }
    };
    ergo.set_vhost(ACCOUNT, VHOST)
        .await
        .with_context(|| format!("Ergo didn't give {ACCOUNT} its vhost"))?;
    let always_on = SettingsChanges {
        always_on: Some(Preference::On),
        ..Default::default()
    };
    ergo.update_settings(ACCOUNT, always_on)
        .await
        .with_context(|| format!("Ergo didn't keep {ACCOUNT} always on"))?;
    Ok(ensured)
}
