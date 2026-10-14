use std::collections::BTreeMap;

use anyhow::Context;
use clap::Subcommand;

use super::{Admin, bot, erase_reminder, print_once};
use crate::{
    db::{self, NewMember},
    invite_rules,
};

#[derive(Subcommand)]
pub enum MemberCommand {
    /// Make an existing Ergo account a member, at the root of the invite tree.
    Adopt {
        account: String,
        /// Make the member an admin.
        #[arg(long)]
        admin: bool,
    },
    /// Give a member a new generated password, end their sessions, and print
    /// the password once.
    ResetPassword { account: String },
    /// Remove a member's personal data, sessions and bots and revoke their
    /// open invites, keeping their place in the invite tree.
    Delete { account: String },
    /// Raise a member's invite quota by N.
    GrantInvites {
        account: String,
        #[arg(value_parser = clap::value_parser!(u16).range(1..))]
        n: u16,
    },
    /// List Ergo accounts that are neither members nor bots, members and
    /// bots Ergo lacks, removed ones whose accounts are still to be erased,
    /// and bots whose owner was removed.
    Reconcile,
}

pub(super) async fn run(command: MemberCommand, admin: &Admin) -> anyhow::Result<String> {
    match command {
        MemberCommand::Adopt {
            account,
            admin: is_admin,
        } => adopt(admin, &account, is_admin).await,
        MemberCommand::ResetPassword { account } => reset_password(admin, &account).await,
        MemberCommand::Delete { account } => delete(admin, &account).await,
        MemberCommand::GrantInvites { account, n } => grant_invites(admin, &account, n).await,
        MemberCommand::Reconcile => reconcile(admin).await,
    }
}

async fn adopt(admin: &Admin, name: &str, is_admin: bool) -> anyhow::Result<String> {
    let account = admin
        .ergo
        .info(name)
        .await?
        .with_context(|| format!("Ergo has no verified, unsuspended account named {name}"))?;
    let account_name = account.account_name.as_str();
    if let Some(member) = admin.db.member(account_name).await? {
        let state = match member.removed_at {
            Some(_) => "was removed, and keeps their place in the invite tree",
            None => "is already a member",
        };
        anyhow::bail!("{} {state}", member.account);
    }
    admin
        .db
        .insert_member(&NewMember {
            account: account_name,
            email: account.email.as_deref(),
            news_opt_in: false,
            invited_by: None,
            joined_at: db::now(),
            invite_quota: invite_rules::DEFAULT_QUOTA,
            is_admin,
        })
        .await?;
    let role = if is_admin { " as an admin" } else { "" };
    Ok(format!("adopted {account_name}{role}\n"))
}

async fn reset_password(admin: &Admin, name: &str) -> anyhow::Result<String> {
    let member = admin.current_member(name).await?;
    let password = admin.set_generated_password(&member.account).await?;
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
    let removal = admin.db.remove_member(account, db::now()).await?;
    Ok(format!(
        "removed {account} (sessions ended: {}, invites revoked: {})\n{}{bots_removed}",
        removal.sessions_ended,
        removal.invites_revoked,
        erase_reminder(account),
    ))
}

async fn grant_invites(admin: &Admin, name: &str, n: u16) -> anyhow::Result<String> {
    let member = admin.current_member(name).await?;
    let quota = admin
        .db
        .grant_invites(&member.account, n.into())
        .await?
        .with_context(|| format!("{} was removed", member.account))?;
    Ok(format!(
        "{}'s invite quota is now {quota}\n",
        member.account
    ))
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
        .map(|member| ("", member.account, member.removed))
        .chain(
            bots.into_iter()
                .map(|bot| ("bot ", bot.account, bot.removed)),
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
