//! Resetting a forgotten password with a link to the member's confirmed
//! email. What the pages answer never tells whether a nick or an address
//! belongs to a member: refusals that say anything more depend on the
//! visitor's address and on the site as a whole, never on the member.

use std::time::Instant;

use askama::Template;
use axum::{Form, extract::State, http::StatusCode, response::Response};
use serde::Deserialize;

use super::{Layout, Link, UNSENT, internal_error, render, render_with};
use crate::{
    app::{App, AppState},
    auth::{self, Visitor},
    db, invite_rules,
    rate_limit::{self, ClientAddress, MailRefusal, Recipient},
    reset_mail,
    reset_rules::{self, Lookup},
};

#[derive(Template)]
#[template(path = "forgot.html")]
struct ForgotPage<'a> {
    layout: Layout<'a>,
    who: &'a str,
    field_error: Option<String>,
    alert: Option<String>,
    max_length: usize,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Ask {
    who: String,
}

pub async fn forgot(State(app): State<AppState>, visitor: Visitor) -> Response {
    render(&forgot_page(&app, &visitor, "", None, None))
}

/// Mails a reset link to the confirmed email of the member `who` names,
/// if there's one and the limits allow, and answers the same either way.
pub async fn ask(
    State(app): State<AppState>,
    visitor: Visitor,
    address: ClientAddress,
    Form(ask): Form<Ask>,
) -> Response {
    let refuse = |status, field_error, alert| {
        render_with(
            status,
            &forgot_page(&app, &visitor, &ask.who, field_error, alert),
        )
    };
    let lookup = match Lookup::parse(&ask.who) {
        Ok(lookup) => lookup,
        Err(error) => {
            return refuse(
                StatusCode::UNPROCESSABLE_ENTITY,
                Some(error.to_string()),
                None,
            );
        }
    };
    let now = Instant::now();
    if !app.recoveries.allows(address.0, now) {
        let try_later = Some(rate_limit::TRY_LATER.to_owned());
        return refuse(StatusCode::TOO_MANY_REQUESTS, None, try_later);
    }
    app.give_back_unsent_links().await;
    if app.reset_links.busy(now) {
        let busy = Some(MailRefusal::Busy.to_string());
        return refuse(StatusCode::TOO_MANY_REQUESTS, None, busy);
    }
    match app.outbox.published_key().await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                None,
                Some(UNSENT.to_owned()),
            );
        }
        Err(error) => return internal_error("Reading the mailer's key", error),
    }
    match app.db.recoverable_member(&lookup).await {
        Ok(Some(member)) => mail_link(&app, &member.account, now).await,
        Ok(None) => {}
        Err(error) => return internal_error("Looking up a member to reset", error),
    }
    sent(&app, &visitor)
}

/// Mails `account` a reset link if their own limits allow. Whatever stops
/// it is the member's business, not the visitor's, so it's only logged
/// when it fails.
async fn mail_link(app: &App, account: &str, now: Instant) {
    if app
        .reset_links
        .check(account, Recipient::SameAddress, now)
        .is_err()
    {
        return;
    }
    let sent = reset_mail::send_link(&app.db, &app.outbox, &app.public_url, account, db::now());
    match sent.await {
        Ok(Some(id)) => app.reset_links.record(account, id, now),
        Ok(None) => {}
        Err(error) => eprintln!("Queueing a reset email failed: {error}"),
    }
}

fn forgot_page<'a>(
    app: &'a App,
    visitor: &'a Visitor,
    who: &'a str,
    field_error: Option<String>,
    alert: Option<String>,
) -> ForgotPage<'a> {
    ForgotPage {
        layout: Layout::visitor(app, visitor),
        who,
        field_error,
        alert,
        max_length: reset_rules::MAX_LENGTH,
    }
}

fn sent(app: &App, visitor: &Visitor) -> Response {
    let lifetime = format!(
        "If that matches a member with a confirmed email, we've sent a link to it. \
         It works once, within {}.",
        invite_rules::in_words(reset_rules::LIFETIME)
    );
    super::message_with_link(
        app,
        visitor,
        StatusCode::OK,
        "Password",
        "Check your email.",
        &[
            &lifetime,
            "If nothing arrives in a few minutes, check your spam folder. \
             Without a confirmed email, only an admin can reset your password.",
        ],
        Some(Link {
            href: auth::SIGN_IN_PATH,
            label: "Back to sign in",
        }),
    )
}
