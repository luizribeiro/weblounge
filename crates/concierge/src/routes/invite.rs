//! The invite page: who's inviting, the signup form, and redeeming the
//! invite into an Ergo account and a member.

use std::time::Instant;

use askama::Template;
use axum::{
    Form,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use ergo_api::{Passphrase, Registration};
use serde::Deserialize;

use super::{Layout, internal_error, render, render_with};
use crate::{
    app::{App, AppState},
    auth::{self, Visitor},
    clients::{self, CLIENTS, Client},
    config::IrcServer,
    db::{self, Claim, Invite, NewMember, Redemption},
    email::{self, Email},
    invite_rules,
    nick::{self, Nick},
    oauth::Webchat,
    password_rules::{self, FieldErrors},
    platform::VisitorPlatform,
    rate_limit::{self, ClientAddress},
};

#[derive(Template)]
#[template(path = "invite.html")]
struct InvitePage<'a> {
    layout: Layout<'a>,
    code: &'a str,
    inviter: Option<&'a str>,
    form: SignupForm<'a>,
    nick_max_length: usize,
    nick_pattern: String,
    nick_rule: &'a str,
    email_max_length: usize,
    email_pattern: &'a str,
}

/// What the form shows: what was typed, except passwords, and what's wrong.
#[derive(Default)]
struct SignupForm<'a> {
    nick: &'a str,
    email: &'a str,
    news: bool,
    nick_error: Option<String>,
    email_error: Option<String>,
    password: FieldErrors,
}

#[derive(Template)]
#[template(path = "welcome.html")]
struct Welcome<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    irc: &'a IrcServer,
    recommended: Vec<&'a Client>,
    clients: &'a [Client],
    webchat: &'a Webchat,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Signup {
    nick: String,
    email: String,
    password: String,
    confirmation: String,
    news: Option<String>,
}

/// A signup that follows every rule.
struct NewAccount {
    nick: Nick,
    email: Email,
    password: Passphrase,
    news: bool,
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

pub async fn redeem(
    State(app): State<AppState>,
    visitor: Visitor,
    platform: VisitorPlatform,
    address: ClientAddress,
    Path(code): Path<String>,
    Form(signup): Form<Signup>,
) -> Response {
    let invite = match open_invite(&app, &code).await {
        Ok(Some(invite)) => invite,
        Ok(None) => return gone(&app, &visitor),
        Err(error) => return internal_error("Looking up an invite", error),
    };
    if visitor.0.is_some() {
        return already_a_member(&app, &visitor);
    }
    let refuse = |form| render_with(StatusCode::UNPROCESSABLE_ENTITY, &page(&app, &invite, form));
    let account = match validate(&signup) {
        Ok(account) => account,
        Err(form) => return refuse(*form),
    };
    // Each attempt tells whether a nick is taken, as a lookup does.
    if !app.lookups.allows(address.0, Instant::now()) {
        let form = SignupForm {
            nick_error: Some(rate_limit::TRY_LATER.to_owned()),
            ..form_for(&signup)
        };
        return render_with(StatusCode::TOO_MANY_REQUESTS, &page(&app, &invite, form));
    }
    let taken = || SignupForm {
        nick_error: Some(format!("{} is taken.", account.nick)),
        ..form_for(&signup)
    };
    match app.db.name_held(account.nick.as_str()).await {
        Ok(false) => {}
        Ok(true) => return refuse(taken()),
        Err(error) => return internal_error("Looking up a name", error),
    }
    let now = db::now();
    let claim = match app.db.claim_invite(&code, now).await {
        Ok(Some(claim)) => claim,
        Ok(None) => return gone(&app, &visitor),
        Err(error) => return internal_error("Claiming an invite", error),
    };
    match create_account(&app, &claim, &account, now).await {
        Ok(()) => {}
        Err(Failure::Taken) => return refuse(taken()),
        Err(Failure::Unreachable) => return unreachable(&app, &visitor),
        Err(Failure::Unfinished) => return unfinished(&app, &visitor),
    }
    let nick = account.nick.as_str();
    let cookie = match auth::start_session(&app, &visitor, nick).await {
        Ok(cookie) => Some(cookie),
        Err(error) => {
            eprintln!("Starting a new member's session failed: {error}");
            None
        }
    };
    welcome(&app, nick, platform, cookie)
}

/// Why [`create_account`] created no member.
enum Failure {
    Taken,
    Unreachable,
    /// Ergo has the account, but no member was recorded: the invite was
    /// revoked or its claim taken over meanwhile, or the database failed.
    Unfinished,
}

/// Creates the Ergo account, then the member who redeemed `claim`,
/// releasing the claim if that fails in a way the invite survives.
async fn create_account(
    app: &App,
    claim: &Claim,
    account: &NewAccount,
    now: i64,
) -> Result<(), Failure> {
    let registered = app
        .ergo
        .register(account.nick.clone(), &account.password)
        .await;
    match registered {
        Ok(Registration::Created) => {}
        Ok(Registration::NameTaken) => {
            release(app, claim).await;
            return Err(Failure::Taken);
        }
        Err(error) => {
            eprintln!("Creating an Ergo account failed: {error}");
            release(app, claim).await;
            return Err(Failure::Unreachable);
        }
    }
    let member = NewMember {
        account: account.nick.as_str(),
        email: Some(account.email.as_str()),
        news_opt_in: account.news,
        invited_by: claim.created_by.as_deref(),
        joined_at: now,
        invite_quota: invite_rules::DEFAULT_QUOTA,
        is_admin: false,
    };
    match app.db.redeem_invite(claim, &member).await {
        Ok(Redemption::Redeemed) => Ok(()),
        Ok(Redemption::Lost) => {
            eprintln!(
                "Ergo account {} was created, but its invite was revoked or taken over \
                 meanwhile; member reconcile lists it",
                member.account
            );
            Err(Failure::Unfinished)
        }
        Err(error) => {
            eprintln!(
                "Ergo account {} was created, but recording the member failed: {error}; \
                 member reconcile lists it",
                member.account
            );
            release(app, claim).await;
            Err(Failure::Unfinished)
        }
    }
}

fn validate(signup: &Signup) -> Result<NewAccount, Box<SignupForm<'_>>> {
    let nick = Nick::parse(&signup.nick);
    let email = Email::parse(&signup.email);
    let password = password_rules::check(&signup.password, &signup.confirmation);
    match (nick, email, password) {
        (Ok(nick), Ok(email), Ok(password)) => Ok(NewAccount {
            nick,
            email,
            password,
            news: signup.news.is_some(),
        }),
        (nick, email, password) => Err(Box::new(SignupForm {
            nick_error: nick.err().map(|error| error.to_string()),
            email_error: email.err().map(|error| error.to_string()),
            password: password.err().map(FieldErrors::from).unwrap_or_default(),
            ..form_for(signup)
        })),
    }
}

/// The form as typed, passwords left out.
fn form_for(signup: &Signup) -> SignupForm<'_> {
    SignupForm {
        nick: &signup.nick,
        email: &signup.email,
        news: signup.news.is_some(),
        ..SignupForm::default()
    }
}

async fn release(app: &App, claim: &Claim) {
    if let Err(error) = app.db.release_claim(claim).await {
        eprintln!("Releasing an invite's claim failed: {error}");
    }
}

/// The welcome page is the signup's own answer, so nothing about it
/// outlives the visit.
fn welcome(
    app: &App,
    nick: &str,
    VisitorPlatform(platform): VisitorPlatform,
    cookie: Option<HeaderValue>,
) -> Response {
    let page = Welcome {
        layout: Layout::new(app, cookie.as_ref().map(|_| nick)),
        nick,
        irc: &app.irc_server,
        recommended: clients::recommended(platform),
        clients: &CLIENTS,
        webchat: &app.webchat,
    };
    let mut response = render(&page);
    if let Some(cookie) = cookie {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}

/// The invite `code`, if anyone could redeem it right now.
async fn open_invite(app: &App, code: &str) -> sqlx::Result<Option<Invite>> {
    let invite = app.db.invite(code).await?;
    Ok(invite.filter(|invite| invite.status(db::now()) == invite_rules::Status::Open))
}

fn page<'a>(app: &'a App, invite: &'a Invite, form: SignupForm<'a>) -> InvitePage<'a> {
    InvitePage {
        layout: Layout::new(app, None),
        code: &invite.code,
        inviter: invite.created_by.as_deref(),
        form,
        nick_max_length: nick::MAX_LENGTH,
        nick_pattern: nick::html_pattern(),
        nick_rule: nick::RULE,
        email_max_length: email::MAX_LENGTH,
        email_pattern: email::HTML_PATTERN,
    }
}

fn message(
    app: &App,
    visitor: &Visitor,
    status: StatusCode,
    title: &str,
    lines: &[&str],
) -> Response {
    super::message(app, visitor, status, "Invitation", title, lines)
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

fn unreachable(app: &App, visitor: &Visitor) -> Response {
    message(
        app,
        visitor,
        StatusCode::SERVICE_UNAVAILABLE,
        "Couldn't reach the chat server.",
        &["Nothing was created and your invite still works. Try again in a minute."],
    )
}

fn unfinished(app: &App, visitor: &Visitor) -> Response {
    message(
        app,
        visitor,
        StatusCode::INTERNAL_SERVER_ERROR,
        "Your account was created, but setup didn't finish.",
        &["Ask whoever invited you to get an admin to finish it."],
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
