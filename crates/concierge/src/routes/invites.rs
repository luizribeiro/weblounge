//! Members creating and revoking their own invites, from the account home.

use askama::Template;
use axum::{
    Form,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use super::{Layout, internal_error, render};
use crate::{
    app::{App, AppState},
    auth::{CurrentMember, Visitor},
    config,
    db::{self, Invitee, Member, NewInvite},
    invite_code,
    invite_rules::{self, Inviter, Status},
    qr,
};

#[derive(Template)]
#[template(path = "qr.html")]
struct QrPage<'a> {
    layout: Layout<'a>,
    invite: OpenInvite,
    svg: String,
}

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

/// An open invite's link as a QR code, big enough to scan from another
/// phone. Other members' invites, and dead ones, aren't found.
pub async fn show_qr(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    Path(id): Path<String>,
) -> Response {
    match open_invite(&app, &me.member, &id, db::now()).await {
        Ok(Some(invite)) => render(&QrPage {
            layout: Layout::new(&app, Some(&me.member.account)),
            svg: qr::svg(&invite.url),
            invite,
        }),
        Ok(None) => super::message(
            &app,
            &Visitor(Some(me.clone())),
            StatusCode::NOT_FOUND,
            "Invites",
            "No such invite.",
            &["It was used, revoked or expired, or it isn't yours."],
        ),
        Err(error) => internal_error("Looking up an invite", error),
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
    /// What URLs call it by, since the code is a secret.
    pub id: String,
    pub url: String,
    pub time_left: String,
}

impl Invites {
    pub async fn of(app: &App, member: &Member, now: i64) -> sqlx::Result<Self> {
        let invites = app.db.invites_created_by(&member.account).await?;
        let rules: Vec<_> = invites.iter().map(db::Invite::rules).collect();
        Ok(Self {
            open: open(app, invites, now).collect(),
            invitees: app.db.invitees(&member.account).await?,
            remaining: invite_rules::remaining(&inviter(member), &rules, now),
        })
    }
}

/// The open invite of `member`'s that URLs call `id`, if there is one.
pub async fn open_invite(
    app: &App,
    member: &Member,
    id: &str,
    now: i64,
) -> sqlx::Result<Option<OpenInvite>> {
    let invites = app.db.invites_created_by(&member.account).await?;
    Ok(open(app, invites, now).find(|invite| invite.id == id))
}

fn open(app: &App, invites: Vec<db::Invite>, now: i64) -> impl Iterator<Item = OpenInvite> {
    invites
        .into_iter()
        .filter(move |invite| matches!(invite.status(now), Status::Open | Status::Claimed))
        .map(move |invite| OpenInvite {
            id: invite_code::public_id(&invite.code),
            url: config::invite_url(&app.public_url, &invite.code),
            time_left: invite_rules::time_left(invite.expires_at, now),
            code: invite.code,
        })
}

fn inviter(member: &Member) -> Inviter {
    Inviter {
        invite_quota: member.invite_quota,
        is_admin: member.is_admin,
        removed: member.removed_at.is_some(),
    }
}
