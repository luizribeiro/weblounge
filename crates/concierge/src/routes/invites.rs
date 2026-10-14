//! The Invites tab: members creating and revoking their own invites, and
//! the invite tree. A new invite's link is shown once, as it's created.

use askama::Template;
use axum::{
    Form,
    extract::State,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use super::{Layout, Tab, internal_error, render};
use crate::{
    app::{App, AppState},
    auth::CurrentMember,
    config,
    db::{self, Invitee, Member, NewInvite},
    invite_code,
    invite_rules::{self, Inviter},
    qr,
    tree::{self, Line},
};

#[derive(Template)]
#[template(path = "invites.html")]
struct InvitesPage<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    invites: Invites,
    lines: Vec<Line>,
}

#[derive(Template)]
#[template(path = "new-invite.html")]
struct NewInvitePage<'a> {
    layout: Layout<'a>,
    url: String,
    svg: String,
    lifetime: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Revocation {
    code: String,
}

pub async fn show(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    let member = &me.member;
    let invites = match Invites::of(&app, member, db::now()).await {
        Ok(invites) => invites,
        Err(error) => return internal_error("Listing invites", error),
    };
    let branches = match app.db.invite_tree().await {
        Ok(branches) => branches,
        Err(error) => return internal_error("Reading the invite tree", error),
    };
    render(&InvitesPage {
        layout: Layout::new(&app, Some(&member.account)).on(Tab::Invites),
        nick: &member.account,
        invites,
        lines: tree::lines(branches),
    })
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
        Ok(true) => {
            let url = config::invite_url(&app.public_url, &code);
            render(&NewInvitePage {
                layout: Layout::new(&app, Some(&me.member.account)).on(Tab::Invites),
                svg: qr::svg(&url),
                url,
                lifetime: invite_rules::in_words(invite_rules::LIFETIME),
            })
        }
        Ok(false) => Redirect::to("/invites").into_response(),
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
        Ok(_) => Redirect::to("/invites").into_response(),
        Err(error) => internal_error("Revoking an invite", error),
    }
}

/// A member's invites, as the Invites tab shows them.
pub struct Invites {
    pub invitees: Vec<Invitee>,
    /// `None` for admins, who have no quota.
    pub remaining: Option<i64>,
}

impl Invites {
    pub async fn of(app: &App, member: &Member, now: i64) -> sqlx::Result<Self> {
        let invites = app.db.invites_created_by(&member.account).await?;
        let rules: Vec<_> = invites.iter().map(db::Invite::rules).collect();
        Ok(Self {
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
