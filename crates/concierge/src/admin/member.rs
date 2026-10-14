use anyhow::Context;
use clap::Subcommand;

use super::Admin;
use crate::{
    db::{self, Member, NewMember},
    invite_rules, password,
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
}

pub(super) async fn run(command: MemberCommand, admin: &Admin) -> anyhow::Result<String> {
    match command {
        MemberCommand::Adopt {
            account,
            admin: is_admin,
        } => adopt(admin, &account, is_admin).await,
        MemberCommand::ResetPassword { account } => reset_password(admin, &account).await,
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
    let member = current_member(admin, name).await?;
    let password = password::generate();
    admin
        .ergo
        .set_password(member.account.as_str(), &password)
        .await
        .with_context(|| format!("Ergo didn't take {}'s new password", member.account))?;
    admin.db.end_sessions(&member.account).await?;
    Ok(format!("{}\n", password.as_str()))
}

/// The member named `name`, unless there's none or they were removed.
async fn current_member(admin: &Admin, name: &str) -> anyhow::Result<Member> {
    let member = admin
        .db
        .member(name)
        .await?
        .with_context(|| format!("{name} is not a member"))?;
    anyhow::ensure!(
        member.removed_at.is_none(),
        "{} was removed",
        member.account
    );
    Ok(member)
}
