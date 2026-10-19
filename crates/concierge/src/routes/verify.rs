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
    db::{self, Member, Verification},
    email, invite_rules, link_code,
    rate_limit::{self, ClientAddress, MailRefusal, Recipient},
    tree, verification_mail, verification_rules,
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

/// Where a member sees how confirming their email stands, and asks for a link.
pub const ACCOUNT_EMAIL: &str = "/account#email-and-news";

pub async fn show(State(app): State<AppState>, visitor: Visitor) -> Response {
    render(&VerifyPage {
        layout: Layout::new(&app, visitor.nick()),
        code_pattern: link_code::html_pattern(link_code::MAILED_LENGTH),
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
    let outcome = match link_code::parse(&confirmation.code, link_code::MAILED_LENGTH) {
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

/// Where confirming a member's email stands, until they confirm it.
pub struct Unconfirmed {
    /// When their grace period ends, once a link to confirm it went out.
    pub due: Option<Due>,
}

pub struct Due {
    /// The date, as ISO for machines.
    pub date: String,
    /// The date as members read it.
    pub day: String,
    /// How long is left, in words, until it passes.
    pub left: Option<String>,
}

impl Unconfirmed {
    pub fn of(member: &Member, now: i64) -> Option<Self> {
        if member.email_verified_at.is_some() {
            return None;
        }
        let due = member.verification_first_sent_at.map(|first_sent_at| {
            let deadline = verification_rules::deadline(first_sent_at);
            Due {
                date: tree::date(deadline),
                day: verification_rules::deadline_in_words(deadline),
                left: (now < deadline).then(|| invite_rules::in_words(deadline - now)),
            }
        });
        Some(Self { due })
    }

    /// Whether a link to confirm it went out.
    pub fn sent(&self) -> bool {
        self.due.is_some()
    }

    /// When their grace period ends, until it passes.
    pub fn upcoming(&self) -> Option<&Due> {
        self.due.as_ref().filter(|due| due.left.is_some())
    }
}

/// Mails `account` a new verification link, replacing their last, if the
/// limits on mail allow.
pub async fn mail_link(app: &App, account: &str, recipient: Recipient) -> Mailed {
    give_back_unsent_links(app).await;
    if let Err(refusal) = app.mailed_links.check(account, recipient, Instant::now()) {
        return Mailed::Refused(refusal);
    }
    let sent = verification_mail::send(
        &app.db,
        &app.outbox,
        &app.public_url,
        account,
        verification_mail::Kind::Link,
        db::now(),
    );
    match sent.await {
        Ok(Some(id)) => {
            app.mailed_links.record(account, id, Instant::now());
            Mailed::Sent
        }
        Ok(None) => Mailed::NothingToVerify,
        Err(error) => {
            eprintln!("Queueing a verification email failed: {error}");
            Mailed::Failed
        }
    }
}

/// Stops counting the links the mailer deleted unsent against their
/// members, who never got them.
async fn give_back_unsent_links(app: &App) {
    match app.outbox.take_unsent().await {
        Ok(unsent) => app.mailed_links.refund(&unsent),
        Err(error) => eprintln!("Reading the links deleted unsent failed: {error}"),
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
