use clap::Subcommand;

use super::Admin;
use crate::{
    db::{self, NewInvite},
    invite_code, invite_rules,
};

#[derive(Subcommand)]
pub enum InviteCommand {
    /// Mint an invite nobody vouches for, and print its link.
    Create {
        /// Members create their own invites on the site; here, only root
        /// invites can be made.
        #[arg(long, required = true)]
        root: bool,
        /// Make whoever redeems it an admin.
        #[arg(long)]
        admin: bool,
    },
}

pub(super) async fn run(command: InviteCommand, admin: &Admin) -> anyhow::Result<String> {
    match command {
        InviteCommand::Create {
            root: _,
            admin: makes_admin,
        } => create_root(admin, makes_admin).await,
    }
}

async fn create_root(admin: &Admin, makes_admin: bool) -> anyhow::Result<String> {
    let code = invite_code::generate();
    let now = db::now();
    admin
        .db
        .insert_invite(&NewInvite {
            code: code.expose(),
            created_by: None,
            created_at: now,
            expires_at: invite_rules::expires_at(now),
            makes_admin,
        })
        .await?;
    Ok(format!("{}\n", admin.config.invite_url(code.expose())))
}
