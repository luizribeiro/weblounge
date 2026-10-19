//! The Invites tab: members creating invites, the people they brought in,
//! and the invite tree. A new invite's link is shown once, as it's made;
//! a member who lost links revokes every invite they have open.

use askama::Template;
use axum::{
    extract::State,
    response::{IntoResponse, Redirect, Response},
};

use super::{Layout, Tab, internal_error, render, verify::Unconfirmed};
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
    lifetime: String,
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

#[derive(Template)]
#[template(path = "revoke-invites.html")]
struct RevokeInvitesPage<'a> {
    layout: Layout<'a>,
    invites: Invites,
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
        layout: Layout::new(&app, Some(member.account.as_str())).on(Tab::Invites),
        nick: member.account.as_str(),
        invites,
        lifetime: invite_rules::lifetime_in_words(),
        lines: tree::lines(branches),
    })
}

pub async fn create(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    let now = db::now();
    let code = invite_code::generate();
    let invite = NewInvite {
        code: &code,
        created_by: Some(&me.member.account),
        created_at: now,
        expires_at: invite_rules::expires_at(now),
        makes_admin: false,
    };
    let inviter = inviter(&me.member);
    let created = app.db.insert_invite_if(&invite, |invites| {
        let invites: Vec<_> = invites.iter().map(db::Invite::rules).collect();
        invite_rules::may_invite(&inviter, &invites, now).is_ok()
    });
    match created.await {
        Ok(true) => {
            let url = config::invite_url(&app.invite_links, &code);
            render(&NewInvitePage {
                layout: Layout::new(&app, Some(me.member.account.as_str())).on(Tab::Invites),
                svg: qr::svg(&url),
                url,
                lifetime: invite_rules::lifetime_in_words(),
            })
        }
        Ok(false) => Redirect::to("/invites").into_response(),
        Err(error) => internal_error("Creating an invite", error),
    }
}

/// Asks before revoking, for browsers that can't open the tab's dialog.
pub async fn confirm_revoke(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
) -> Response {
    let invites = match Invites::of(&app, &me.member, db::now()).await {
        Ok(invites) => invites,
        Err(error) => return internal_error("Listing invites", error),
    };
    if invites.open == 0 {
        return Redirect::to("/invites").into_response();
    }
    render(&RevokeInvitesPage {
        layout: Layout::new(&app, Some(me.member.account.as_str())).on(Tab::Invites),
        invites,
    })
}

pub async fn revoke(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    match app
        .db
        .revoke_open_invites(&me.member.account, db::now())
        .await
    {
        Ok(_) => Redirect::to("/invites").into_response(),
        Err(error) => internal_error("Revoking invites", error),
    }
}

/// A member's invites, as the Invites tab shows them.
pub struct Invites {
    pub invitees: Vec<Invitee>,
    /// How many are open now.
    pub open: i64,
    /// How many more may be open now.
    pub remaining: i64,
    /// How long until the first unused one expires, in words.
    pub first_expires_in: Option<String>,
    /// Where confirming the member's email stands, until they do; inviting
    /// waits on it.
    pub unconfirmed: Option<Unconfirmed>,
}

impl Invites {
    pub async fn of(app: &App, member: &Member, now: i64) -> sqlx::Result<Self> {
        let invites = app.db.invites_created_by(&member.account).await?;
        let rules: Vec<_> = invites.iter().map(db::Invite::rules).collect();
        Ok(Self {
            invitees: app.db.invitees(&member.account).await?,
            open: invite_rules::open(&rules, now),
            remaining: invite_rules::remaining(&inviter(member), &rules, now),
            first_expires_in: invite_rules::first_expiry_in(&rules, now)
                .map(invite_rules::wait_in_words),
            unconfirmed: Unconfirmed::of(member, now),
        })
    }
}

fn inviter(member: &Member) -> Inviter {
    Inviter {
        removed: member.removed_at.is_some(),
        verified: member.email_verified_at.is_some(),
    }
}
