//! The page a verification link opens. Links carry the code in their
//! fragment, which no server sees, and mail scanners open links, so the
//! page verifies nothing until its button posts the code.

use std::time::Instant;

use askama::Template;
use axum::{Form, extract::State, http::StatusCode, response::Response};
use serde::Deserialize;

use super::{Layout, Link, Tab, internal_error, render};
use crate::{
    app::{App, AppState},
    auth::Visitor,
    db::{self, Verification},
    email, invite_rules, link_code,
    rate_limit::{self, ClientAddress, MailRefusal},
    verification_mail, verification_rules,
};

#[derive(Template)]
#[template(path = "verify.html")]
struct VerifyPage<'a> {
    layout: Layout<'a>,
    code_pattern: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Confirmation {
    code: String,
}

const ACCOUNT_EMAIL: &str = "/account#email-and-news";

pub async fn show(State(app): State<AppState>, visitor: Visitor) -> Response {
    render(&VerifyPage {
        layout: Layout::new(&app, visitor.nick()),
        code_pattern: link_code::html_pattern(verification_rules::CODE_LENGTH),
    })
}

/// Verifies the email the posted code was sent to. Never signs anyone
/// in: holding the link proves the address, not the account.
pub async fn confirm(
    State(app): State<AppState>,
    visitor: Visitor,
    address: ClientAddress,
    Form(confirmation): Form<Confirmation>,
) -> Response {
    if !app.verifications.allows(address.0, Instant::now()) {
        return too_many(&app, &visitor);
    }
    let outcome = match link_code::parse(&confirmation.code, verification_rules::CODE_LENGTH) {
        Some(code) => match app.db.verify_email(code, db::now()).await {
            Ok(outcome) => outcome,
            Err(error) => return internal_error("Verifying an email", error),
        },
        None => Verification::Unknown,
    };
    match outcome {
        Verification::Verified { .. } => verified(&app, &visitor),
        Verification::Unknown | Verification::Expired => lapsed(&app, &visitor),
        Verification::Replaced => replaced(&app, &visitor),
        Verification::AddressChanged => address_changed(&app, &visitor),
        Verification::AddressTaken => address_taken(&app, &visitor),
    }
}

/// What came of mailing a member a new verification link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mailed {
    Sent,
    /// Their address is verified already, or they're gone.
    NothingToVerify,
    Refused(MailRefusal),
    /// Queueing failed, and was logged.
    Failed,
}

/// Mails `account` a new verification link, replacing their last, if the
/// limits on mail allow.
pub async fn mail_link(app: &App, account: &str) -> Mailed {
    if let Err(refusal) = app.mailed_links.check(account, Instant::now()) {
        return Mailed::Refused(refusal);
    }
    let sent = verification_mail::send(&app.db, &app.outbox, &app.public_url, account, db::now());
    match sent.await {
        Ok(true) => {
            app.mailed_links.record(account, Instant::now());
            Mailed::Sent
        }
        Ok(false) => Mailed::NothingToVerify,
        Err(error) => {
            eprintln!("Queueing a verification email failed: {error}");
            Mailed::Failed
        }
    }
}

fn message(
    app: &App,
    visitor: &Visitor,
    status: StatusCode,
    title: &str,
    lines: &[&str],
    next: Link,
) -> Response {
    super::message_with_link(app, visitor, status, "Email", title, lines, Some(next))
}

const NEW_LINK: Link = Link {
    href: ACCOUNT_EMAIL,
    label: "Send a new link",
};

fn verified(app: &App, visitor: &Visitor) -> Response {
    message(
        app,
        visitor,
        StatusCode::OK,
        "Your email is confirmed.",
        &["Thanks. There's nothing else to do."],
        Link {
            href: Tab::Account.path(),
            label: "Go to your account",
        },
    )
}

/// The one answer for unknown, used, expired and malformed codes alike.
fn lapsed(app: &App, visitor: &Visitor) -> Response {
    let lifetime = format!(
        "Links work once and expire after {}. If you've confirmed your email already, \
         there's nothing more to do.",
        invite_rules::in_words(verification_rules::LIFETIME)
    );
    message(
        app,
        visitor,
        StatusCode::UNPROCESSABLE_ENTITY,
        "This link doesn't work any more.",
        &[
            &lifetime,
            "Otherwise, send a new one from the Account tab. New links can land in the spam folder.",
        ],
        NEW_LINK,
    )
}

fn replaced(app: &App, visitor: &Visitor) -> Response {
    message(
        app,
        visitor,
        StatusCode::UNPROCESSABLE_ENTITY,
        "A newer email was sent.",
        &["Use the link in that one: only the newest link works. \
           If you can't find it, check your spam folder."],
        NEW_LINK,
    )
}

fn address_changed(app: &App, visitor: &Visitor) -> Response {
    message(
        app,
        visitor,
        StatusCode::UNPROCESSABLE_ENTITY,
        "Your email changed since this link was sent.",
        &["A link confirms only the address it went to. \
           Send one to your new address from the Account tab."],
        NEW_LINK,
    )
}

fn address_taken(app: &App, visitor: &Visitor) -> Response {
    message(
        app,
        visitor,
        StatusCode::UNPROCESSABLE_ENTITY,
        email::TAKEN,
        &["Each address belongs to one member. Use another from the Account tab."],
        Link {
            href: ACCOUNT_EMAIL,
            label: "Change your email",
        },
    )
}

fn too_many(app: &App, visitor: &Visitor) -> Response {
    super::message(
        app,
        visitor,
        StatusCode::TOO_MANY_REQUESTS,
        "Email",
        rate_limit::TRY_LATER,
        &[],
    )
}
