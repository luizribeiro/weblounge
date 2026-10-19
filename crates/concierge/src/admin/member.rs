use std::collections::BTreeMap;

use clap::Subcommand;

use super::{Admin, bot, erase_reminder, print_once};
use crate::{
    db,
    name::MemberName,
    password,
    verification_mail::{self, Kind},
};

#[derive(Subcommand)]
pub enum MemberCommand {
    /// Give a member a new generated password, end their sessions, and print
    /// the password once.
    ResetPassword { account: String },
    /// Lock a member out of IRC, remove their personal data, sessions and
    /// bots, and revoke their open invites, keeping their place in the
    /// invite tree.
    Delete { account: String },
    /// List Ergo accounts that are neither members nor bots, members and
    /// bots Ergo lacks, removed ones whose accounts are still to be erased,
    /// and bots whose owner was removed.
    Reconcile,
    /// Mail a member a link to confirm their email, replacing any they
    /// had. Temporary: for the members who joined before emails were
    /// confirmed, to be removed once they all have.
    SendVerification {
        #[arg(required_unless_present = "unverified", conflicts_with = "unverified")]
        account: Option<String>,
        /// Instead, every unconfirmed member who was never sent a link, as
        /// many as one batch takes; run it again for the rest.
        #[arg(long)]
        unverified: bool,
    },
}

pub(super) async fn run(command: MemberCommand, admin: &Admin) -> anyhow::Result<String> {
    match command {
        MemberCommand::ResetPassword { account } => reset_password(admin, &account).await,
        MemberCommand::Delete { account } => delete(admin, &account).await,
        MemberCommand::Reconcile => reconcile(admin).await,
        MemberCommand::SendVerification { account, .. } => {
            send_verification(admin, account.as_deref()).await
        }
    }
}

async fn reset_password(admin: &Admin, name: &str) -> anyhow::Result<String> {
    let member = admin.current_member(name).await?;
    let password = password::set_generated(&admin.ergo, &member.account).await?;
    admin.db.end_sessions(&member.account).await?;
    Ok(print_once(&password))
}

async fn delete(admin: &Admin, name: &str) -> anyhow::Result<String> {
    let member = admin.current_member(name).await?;
    let account = &member.account;
    let mut bots_removed = String::new();
    for bot in admin.db.bots(Some(account)).await? {
        bots_removed += &bot::remove(admin, &bot.account).await?;
    }
    password::set_generated(&admin.ergo, account).await?;
    let mail = admin.outbox.delete_for(account.as_str()).await?;
    let removal = admin.db.remove_member(account, db::now()).await?;
    let lingering = if removal.off_the_disk && mail.off_the_disk {
        ""
    } else {
        "their old data stays on disk until the hourly vacuum\n"
    };
    Ok(format!(
        "removed {account} (sessions ended: {}, invites revoked: {})\n{lingering}{}{bots_removed}",
        removal.sessions_ended,
        removal.invites_revoked,
        erase_reminder(account),
    ))
}

async fn send_verification(admin: &Admin, name: Option<&str>) -> anyhow::Result<String> {
    let accounts = match name {
        Some(name) => vec![admin.current_member(name).await?.account],
        None => never_sent_a_link(admin).await?,
    };
    if accounts.is_empty() {
        return Ok("every unconfirmed member was sent a link already\n".into());
    }
    let public_url = admin.config.public_url.as_str();
    let batch = verification_mail::send_batch(
        &admin.db,
        &admin.outbox,
        public_url,
        &accounts,
        Kind::Link,
        db::now(),
    )
    .await?;
    let mut lines: String = accounts
        .iter()
        .take(verification_mail::PER_RUN)
        .map(|account| {
            if batch.sent.contains(account) {
                format!("sent {account} a link\n")
            } else {
                format!("{account} confirmed their email already\n")
            }
        })
        .collect();
    if batch.left > 0 {
        lines += &format!("{} more to send: run this again in an hour\n", batch.left);
    }
    Ok(lines)
}

/// The unconfirmed members who were never sent a link.
async fn never_sent_a_link(admin: &Admin) -> anyhow::Result<Vec<MemberName>> {
    let progress = admin.db.verification_progress().await?;
    Ok(progress
        .into_iter()
        .filter(|(_, progress)| !progress.verified && progress.first_sent_at.is_none())
        .map(|(account, _)| account)
        .collect())
}

async fn reconcile(admin: &Admin) -> anyhow::Result<String> {
    let mut in_ergo: BTreeMap<String, String> = admin
        .ergo
        .accounts()
        .await?
        .into_iter()
        .map(|account| String::from(account.account_name.as_str()))
        .map(|name| (name.to_ascii_lowercase(), name))
        .collect();
    let members = admin.db.member_accounts().await?;
    let bots = admin.db.bot_accounts().await?;
    let mut lines: Vec<_> = bots
        .iter()
        .filter(|bot| bot.owner_removed && !bot.removed)
        .map(|bot| format!("bot of a removed member: {} ({})", bot.account, bot.owner))
        .collect();
    let known = members
        .into_iter()
        .map(|member| ("", member.account.to_string(), member.removed))
        .chain(
            bots.into_iter()
                .map(|bot| ("bot ", bot.account.to_string(), bot.removed)),
        );
    for (kind, account, removed) in known {
        let known_to_ergo = in_ergo.remove(&account.to_ascii_lowercase()).is_some();
        match (removed, known_to_ergo) {
            (false, false) => lines.push(format!("only in concierge: {kind}{account}")),
            (true, true) => lines.push(format!("removed but still in ergo: {kind}{account}")),
            _ => {}
        }
    }
    lines.extend(
        in_ergo
            .into_values()
            .map(|name| format!("only in ergo: {name}")),
    );
    lines.sort();
    Ok(lines.into_iter().map(|line| line + "\n").collect())
}
