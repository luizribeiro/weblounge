//! The invite page: who's inviting, and the signup form.

use askama::Template;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Response,
};

use super::{Layout, internal_error, render, render_with};
use crate::{
    app::{App, AppState},
    auth::Visitor,
    db::{self, Invite},
    email, invite_rules, nick,
    password_rules::{self, Rule},
};

#[derive(Template)]
#[template(path = "invite.html")]
struct InvitePage<'a> {
    layout: Layout<'a>,
    code: &'a str,
    inviter: Option<&'a str>,
    form: SignupForm<'a>,
    rules: [Rule; 5],
    nick_max_length: usize,
    nick_pattern: String,
    nick_rule: &'a str,
    email_max_length: usize,
    email_pattern: &'a str,
    password_min_length: usize,
}

/// What the form shows: what was typed, except passwords, and what's wrong.
#[derive(Default)]
struct SignupForm<'a> {
    nick: &'a str,
    email: &'a str,
    news: bool,
    nick_error: Option<String>,
    email_error: Option<String>,
    password_error: Option<String>,
    confirmation_error: Option<String>,
}

#[derive(Template)]
#[template(path = "message.html")]
struct Message<'a> {
    layout: Layout<'a>,
    eyebrow: &'a str,
    title: &'a str,
    lines: &'a [&'a str],
}

pub async fn show(
    State(app): State<AppState>,
    visitor: Visitor,
    Path(code): Path<String>,
) -> Response {
    let invite = match open_invite(&app, &code).await {
        Ok(Some(invite)) => invite,
        Ok(None) => return gone(&app, &visitor),
        Err(error) => return internal_error("Looking up an invite", error),
    };
    if visitor.0.is_some() {
        return already_a_member(&app, &visitor);
    }
    render(&page(&app, &invite, SignupForm::default()))
}

/// The invite `code`, if anyone could redeem it right now.
async fn open_invite(app: &App, code: &str) -> sqlx::Result<Option<Invite>> {
    let invite = app.db.invite(code).await?;
    Ok(invite.filter(|invite| invite.status(db::now()) == invite_rules::Status::Open))
}

fn page<'a>(app: &'a App, invite: &'a Invite, form: SignupForm<'a>) -> InvitePage<'a> {
    InvitePage {
        layout: Layout {
            assets: &app.assets,
            nick: None,
        },
        code: &invite.code,
        inviter: invite.created_by.as_deref(),
        form,
        rules: Rule::ALL,
        nick_max_length: nick::MAX_LENGTH,
        nick_pattern: nick::html_pattern(),
        nick_rule: nick::RULE,
        email_max_length: email::MAX_LENGTH,
        email_pattern: email::HTML_PATTERN,
        password_min_length: password_rules::MIN_LENGTH,
    }
}

fn message(
    app: &App,
    visitor: &Visitor,
    status: StatusCode,
    title: &str,
    lines: &[&str],
) -> Response {
    let page = Message {
        layout: Layout {
            assets: &app.assets,
            nick: visitor.nick(),
        },
        eyebrow: "Invitation",
        title,
        lines,
    };
    render_with(status, &page)
}

/// The one answer for every link that can't be redeemed, whatever the
/// reason, so a link gives nothing away.
fn gone(app: &App, visitor: &Visitor) -> Response {
    message(
        app,
        visitor,
        StatusCode::NOT_FOUND,
        "This invite link doesn't work.",
        &["Invites are single-use and expire after a week. Ask for a new one."],
    )
}

fn already_a_member(app: &App, visitor: &Visitor) -> Response {
    message(
        app,
        visitor,
        StatusCode::OK,
        "You already have an account.",
        &["This link is for someone new. Pass it on, or sign out to use it here."],
    )
}
