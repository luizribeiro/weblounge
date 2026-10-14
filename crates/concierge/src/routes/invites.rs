//! Members creating and revoking their own invites, from the account home.

use axum::{
    Form,
    extract::State,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use super::internal_error;
use crate::{
    app::{App, AppState},
    auth::CurrentMember,
    config,
    db::{self, Invitee, Member, NewInvite},
    invite_code,
    invite_rules::{self, Inviter, Status},
};

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Revocation {
    code: String,
}

pub async fn create(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    let now = db::now();
    let code = invite_code::generate();
    let invite = NewInvite {
        code: &code,
        created_by: Some(&me.member.account),
        note: None,
        created_at: now,
        expires_at: invite_rules::expires_at(now),
    };
    let inviter = inviter(&me.member);
    let created = app.db.insert_invite_if(&invite, |invites| {
        let invites: Vec<_> = invites.iter().map(db::Invite::rules).collect();
        invite_rules::may_invite(&inviter, &invites, now).is_ok()
    });
    match created.await {
        Ok(true) => Redirect::to("/?invited#invites").into_response(),
        Ok(false) => Redirect::to("/#invites").into_response(),
        Err(error) => internal_error("Creating an invite", error),
    }
}

pub async fn revoke(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    Form(revocation): Form<Revocation>,
) -> Response {
    let revoked = app
        .db
        .revoke_invite(&revocation.code, &me.member.account, db::now())
        .await;
    // Whether there was such an invite of theirs is nobody's business.
    match revoked {
        Ok(_) => Redirect::to("/#invites").into_response(),
        Err(error) => internal_error("Revoking an invite", error),
    }
}

/// A member's invites, as the account home shows them.
pub struct Invites {
    pub open: Vec<OpenInvite>,
    pub invitees: Vec<Invitee>,
    /// `None` for admins, who have no quota.
    pub remaining: Option<i64>,
}

pub struct OpenInvite {
    pub code: String,
    pub url: String,
    pub time_left: String,
}

impl Invites {
    pub async fn of(app: &App, member: &Member, now: i64) -> sqlx::Result<Self> {
        let invites = app.db.invites_created_by(&member.account).await?;
        let rules: Vec<_> = invites.iter().map(db::Invite::rules).collect();
        let open = invites
            .into_iter()
            .filter(|invite| matches!(invite.status(now), Status::Open | Status::Claimed))
            .map(|invite| OpenInvite {
                url: config::invite_url(&app.public_url, &invite.code),
                time_left: invite_rules::time_left(invite.expires_at, now),
                code: invite.code,
            })
            .collect();
        Ok(Self {
            open,
            invitees: app.db.invitees(&member.account).await?,
            remaining: invite_rules::remaining(&inviter(member), &rules, now),
        })
    }
}

fn inviter(member: &Member) -> Inviter {
    Inviter {
        invite_quota: member.invite_quota,
        is_admin: member.is_admin,
        removed: member.removed_at.is_some(),
    }
}
