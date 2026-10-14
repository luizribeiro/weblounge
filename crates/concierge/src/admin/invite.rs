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
        /// A reminder of whom the invite is for.
        #[arg(long)]
        note: Option<String>,
    },
}

pub(super) async fn run(command: InviteCommand, admin: &Admin) -> anyhow::Result<String> {
    match command {
        InviteCommand::Create { root: _, note } => create_root(admin, note.as_deref()).await,
    }
}

async fn create_root(admin: &Admin, note: Option<&str>) -> anyhow::Result<String> {
    let code = invite_code::generate();
    let now = db::now();
    admin
        .db
        .insert_invite(&NewInvite {
            code: &code,
            created_by: None,
            note,
            created_at: now,
            expires_at: invite_rules::expires_at(now),
        })
        .await?;
    Ok(format!("{}\n", admin.config.invite_url(&code)))
}
