//! Resetting a forgotten password with a link to the member's confirmed
//! email. What the pages answer never tells whether a nick or an address
//! belongs to a member: refusals that say anything more depend on the
//! visitor's address and on the site as a whole, never on the member.

use std::time::Instant;

use askama::Template;
use axum::{
    Form,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::{ERGO_UNAVAILABLE, Layout, Link, UNSENT, internal_error, render, render_with};
use crate::{
    app::{App, AppState},
    auth::{self, Visitor},
    db, invite_rules, link_code,
    name::MemberName,
    password_rules::{self, FieldErrors},
    rate_limit::{self, ClientAddress, MailRefusal, Recipient},
    reset_mail,
    reset_rules::{self, Lookup},
    session,
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
/// if there's one and the limits allow, and answers the same either way,
/// and as soon: the mailing happens after the answer.
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
        Ok(Some(member)) => {
            let app = app.clone();
            app.background
                .clone()
                .spawn(async move { mail_link(&app, &member.account, now).await });
        }
        Ok(None) => {}
        Err(error) => return internal_error("Looking up a member to reset", error),
    }
    sent(&app, &visitor)
}

/// Mails `account` a reset link if their own limits allow. Whatever stops
/// it is the member's business, not the visitor's, so it's only logged
/// when it fails.
async fn mail_link(app: &App, account: &MemberName, now: Instant) {
    if app
        .reset_links
        .check(account, Recipient::SameAddress, now)
        .is_err()
    {
        return;
    }
    let sent = reset_mail::send_link(
        &app.db,
        &app.outbox,
        app.public_url.as_str(),
        account,
        db::now(),
    );
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

#[derive(Template)]
#[template(path = "reset.html")]
struct ResetPage<'a> {
    layout: Layout<'a>,
    /// The code a refused form came with; the link's fragment otherwise.
    code: &'a str,
    code_pattern: String,
    password: FieldErrors,
    alert: Option<&'static str>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct NewPassword {
    code: String,
    password: String,
    confirmation: String,
}

/// The page a reset link opens. Links carry the code in their fragment,
/// which no server sees, and mail scanners open links, so the page uses
/// nothing until its form posts the code.
pub async fn show(State(app): State<AppState>, visitor: Visitor) -> Response {
    render(&reset_page(
        &app,
        &visitor,
        "",
        FieldErrors::default(),
        None,
    ))
}

/// Sets the new password of the member the posted code was mailed to,
/// ends every session and web client token of theirs, and tells their
/// confirmed address. Never signs anyone in: it sends them to sign in.
pub async fn reset(
    State(app): State<AppState>,
    visitor: Visitor,
    address: ClientAddress,
    Form(form): Form<NewPassword>,
) -> Response {
    if !app.recoveries.allows(address.0, Instant::now()) {
        return message(
            &app,
            StatusCode::TOO_MANY_REQUESTS,
            rate_limit::TRY_LATER,
            &[],
            None,
        );
    }
    let Some(code) = link_code::parse(&form.code, link_code::MAILED_LENGTH) else {
        return lapsed(&app);
    };
    let refuse = |status, password, alert| {
        render_with(status, &reset_page(&app, &visitor, code, password, alert))
    };
    let new = match password_rules::check(&form.password, &form.confirmation) {
        Ok(new) => new,
        Err(refusal) => return refuse(StatusCode::UNPROCESSABLE_ENTITY, refusal.into(), None),
    };
    let taken = match app.db.take_reset_code(code, db::now()).await {
        Ok(Some(taken)) => taken,
        Ok(None) => return lapsed(&app),
        Err(error) => return internal_error("Taking a reset code", error),
    };
    if let Err(error) = app.ergo.set_password(&taken.account, &new).await {
        eprintln!("Resetting a password failed: {error}");
        if let Err(error) = app.db.restore_reset_code(&taken).await {
            return internal_error("Putting a reset code back", error);
        }
        let unavailable = Some(ERGO_UNAVAILABLE);
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            FieldErrors::default(),
            unavailable,
        );
    }
    app.limiter.record_success(taken.account.as_str());
    if let Err(error) = app.db.end_sessions(&taken.account).await {
        return internal_error("Ending a reset member's sessions", error);
    }
    notify(&app, &taken.account, &taken.email).await;
    let ended_here = visitor
        .0
        .as_ref()
        .is_some_and(|me| me.member.account == taken.account);
    if ended_here {
        let cookie = [(header::SET_COOKIE, session::removal_cookie())];
        (cookie, changed(&app)).into_response()
    } else {
        changed(&app)
    }
}

/// Tells `account`'s confirmed address their password changed, in mail
/// that carries no secret, so it waits for the mailer as it is.
async fn notify(app: &App, account: &MemberName, email: &str) {
    let queued = match reset_mail::changed_message(account, email, app.public_url.as_str()) {
        Ok(message) => app
            .outbox
            .queue(&message, db::now())
            .await
            .map_err(Into::into),
        Err(error) => Err(error),
    };
    if let Err(error) = queued {
        eprintln!("Queueing a password-changed email failed: {error}");
    }
}

fn reset_page<'a>(
    app: &'a App,
    visitor: &'a Visitor,
    code: &'a str,
    password: FieldErrors,
    alert: Option<&'static str>,
) -> ResetPage<'a> {
    ResetPage {
        layout: Layout::visitor(app, visitor),
        code,
        code_pattern: link_code::html_pattern(link_code::MAILED_LENGTH),
        password,
        alert,
    }
}

/// A page that only says something, to someone shown as signed out:
/// whoever reset a password signs in afresh.
fn message(
    app: &App,
    status: StatusCode,
    title: &str,
    lines: &[&str],
    next: Option<Link>,
) -> Response {
    let signed_out = Visitor::default();
    super::message_with_link(app, &signed_out, status, "Password", title, lines, next)
}

const SIGN_IN: Link = Link {
    href: auth::SIGN_IN_PATH,
    label: "Sign in",
};

fn changed(app: &App) -> Response {
    message(
        app,
        StatusCode::OK,
        "Your password is changed.",
        &[
            "You're signed out of this site and the web client everywhere. \
             Sign in with your new password.",
            "IRC apps that were connected stay connected, and logins with a \
             client certificate keep working.",
        ],
        Some(SIGN_IN),
    )
}

/// The one answer for unknown, used, expired, replaced and malformed
/// codes alike.
fn lapsed(app: &App) -> Response {
    let lifetime = format!(
        "Links work once and expire after {}, and only the newest one works. \
         If you set a new password with it already, there's nothing more to do.",
        invite_rules::in_words(reset_rules::LIFETIME)
    );
    message(
        app,
        StatusCode::UNPROCESSABLE_ENTITY,
        "This link doesn't work any more.",
        &[&lifetime, "Otherwise, ask for a new one."],
        Some(Link {
            href: reset_mail::FORGOT_PATH,
            label: "Get a new link",
        }),
    )
}
