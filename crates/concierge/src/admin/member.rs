use anyhow::Context;
use clap::Subcommand;

use super::Admin;
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
}

pub(super) async fn run(command: MemberCommand, admin: &Admin) -> anyhow::Result<String> {
    match command {
        MemberCommand::Adopt {
            account,
            admin: is_admin,
        } => adopt(admin, &account, is_admin).await,
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
