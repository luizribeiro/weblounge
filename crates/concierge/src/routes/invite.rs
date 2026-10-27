//! The invite page: the signup form, and redeeming the invite whose code
//! it carries into an Ergo account and a member. Links carry the code in
//! their fragment, which no server sees; the form posts it.

use std::time::{Duration, Instant};

use askama::Template;
use axum::{
    Form,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use ergo_api::{AccountName, Passphrase, Registration};
use secret::SecretString;
use serde::Deserialize;

use super::{
    Layout, Tab, internal_error, render, render_with,
    verify::{self, Mailed},
};
use crate::{
    app::{App, AppState},
    auth::{self, Visitor},
    bots::{self, NameClaim},
    clients::{self, Client},
    db::{self, Claim, Invite, NewMember, Redemption},
    email::{self, Email},
    invite_code, invite_rules,
    name::MemberName,
    nick::{self, Nick},
    oauth::Webchat,
    password_rules::{self, FieldErrors},
    platform::VisitorPlatform,
    rate_limit::{self, ClientAddress, Recipient},
};

#[derive(Template)]
#[template(path = "invite.html")]
struct InvitePage<'a> {
    layout: Layout<'a>,
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
    code: &'a str,
    nick: &'a str,
    email: &'a str,
    news: bool,
    code_error: Option<String>,
    nick_error: Option<String>,
    email_error: Option<String>,
    password: FieldErrors,
}

#[derive(Template)]
#[template(path = "welcome.html")]
struct Welcome<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    /// Where the verification link went, if one did.
    mailed_to: Option<&'a str>,
    recommended: Vec<&'a Client>,
    webchat: &'a Webchat,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Signup {
    code: SecretString,
    nick: String,
    email: String,
    password: SecretString,
    confirmation: SecretString,
    news: Option<String>,
}

/// A signup that follows every rule.
struct NewAccount {
    /// The member it makes, under the nick they chose.
    name: MemberName,
    email: Email,
    password: Passphrase,
    news: bool,
}

pub async fn show(State(app): State<AppState>, visitor: Visitor) -> Response {
    if visitor.0.is_some() {
        return already_a_member(&app, &visitor);
    }
    render(&page(&app, None, SignupForm::default()))
}

pub async fn redeem(
    State(app): State<AppState>,
    visitor: Visitor,
    platform: VisitorPlatform,
    address: ClientAddress,
    Form(signup): Form<Signup>,
) -> Response {
    if visitor.0.is_some() {
        return already_a_member(&app, &visitor);
    }
    let code = invite_code::from_input(signup.code.expose());
    let invite = match open_invite(&app, code).await {
        Ok(invite) => invite,
        Err(error) => return internal_error("Looking up an invite", error),
    };
    let inviter = invite
        .as_ref()
        .and_then(|invite| invite.created_by.as_ref())
        .map(MemberName::as_str);
    let refuse = |form| render_with(StatusCode::UNPROCESSABLE_ENTITY, &page(&app, inviter, form));
    let account = match validate(&signup, invite.is_some()) {
        Ok(account) => account,
        Err(form) => return refuse(*form),
    };
    // Each attempt tells whether a nick is taken, as a lookup does.
    if !app.lookups.allows(address.0, Instant::now()) {
        let form = SignupForm {
            nick_error: Some(rate_limit::TRY_LATER.to_owned()),
            ..form_for(&signup)
        };
        return render_with(StatusCode::TOO_MANY_REQUESTS, &page(&app, inviter, form));
    }
    let taken = || SignupForm {
        nick_error: Some(format!("{} is taken.", account.name)),
        ..form_for(&signup)
    };
    match name_held(&app, &account.name).await {
        Ok(false) => {}
        Ok(true) => return refuse(taken()),
        Err(error) => return internal_error("Looking up a name", error),
    }
    let email_taken = app.db.email_taken(&account.email, &account.name);
    match email_taken.await {
        Ok(false) => {}
        Ok(true) => {
            return refuse(SignupForm {
                email_error: Some(email::TAKEN.to_owned()),
                ..form_for(&signup)
            });
        }
        Err(error) => return internal_error("Looking up an email", error),
    }
    let now = db::now();
    let claim = match app.db.claim_invite(code, now).await {
        Ok(Some(claim)) => claim,
        Ok(None) => {
            return refuse(SignupForm {
                code_error: Some(unusable_code()),
                ..form_for(&signup)
            });
        }
        Err(error) => return internal_error("Claiming an invite", error),
    };
    let name = match create_account(&app, &claim, &account, now).await {
        Ok(name) => name,
        Err(Failure::Taken) => return refuse(taken()),
        Err(Failure::Unreachable) => return unreachable(&app, &visitor),
        Err(Failure::Unfinished) => return unfinished(&app, &visitor),
    };
    let name = &name;
    let mailed = verify::mail_link(&app, name, Recipient::NewAddress).await == Mailed::Sent;
    let mailed_to = mailed.then_some(account.email.as_str());
    let cookie = match auth::start_session(&app, &visitor, name).await {
        Ok(cookie) => Some(cookie),
        Err(error) => {
            eprintln!("Starting a new member's session failed: {error}");
            None
        }
    };
    welcome(&app, name, mailed_to, platform, cookie)
}

/// Why [`create_account`] created no member.
enum Failure {
    Taken,
    Unreachable,
    /// Ergo has the account, but no member was recorded: the invite was
    /// revoked or its claim taken over meanwhile, or the database failed.
    Unfinished,
}

/// How many times, [`RECHECK_INTERVAL`] apart, signup asks Ergo whether
/// a registration it got no answer to created the account. Each ask can
/// take Ergo's 10s, as checking a password is as slow as setting one, so
/// with the registration's own 10s these keep a signup within the 60s
/// nginx waits for an answer.
const LOST_REPLY_CHECKS: usize = 3;
const RECHECK_INTERVAL: Duration = Duration::from_secs(2);

/// Creates the Ergo account, then the member who redeemed `claim`,
/// releasing the claim if that fails in a way the invite survives.
/// Answers the member's name as Ergo spells it.
async fn create_account(
    app: &App,
    claim: &Claim,
    account: &NewAccount,
    now: i64,
) -> Result<MemberName, Failure> {
    let name = match register(app, account).await {
        Ok(name) => name,
        Err(failure) => {
            release(app, claim).await;
            return Err(failure);
        }
    };
    let member = NewMember {
        account: &name,
        email: &account.email,
        news_opt_in: account.news,
        invited_by: claim.created_by.as_ref(),
        joined_at: now,
        is_admin: claim.makes_admin,
    };
    match app.db.redeem_invite(claim, &member).await {
        Ok(Redemption::Redeemed) => Ok(name),
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

/// Registers `account`'s Ergo account, answering its name. When Ergo
/// doesn't answer, it may have registered it all the same, so this looks.
async fn register(app: &App, account: &NewAccount) -> Result<MemberName, Failure> {
    match app.ergo.register(&account.name, &account.password).await {
        Ok(Registration::Created) => Ok(account.name.clone()),
        Ok(Registration::NameTaken) => Err(Failure::Taken),
        Err(error) if error.unanswered() => {
            eprintln!(
                "Ergo didn't answer creating account {}: {error}",
                account.name
            );
            unclaimed_account(app, account, LOST_REPLY_CHECKS)
                .await?
                .ok_or(Failure::Unreachable)
        }
        Err(error) => {
            eprintln!("Creating an Ergo account failed: {error}");
            Err(Failure::Unreachable)
        }
    }
}

/// The name of the Ergo account under `account`'s name, if `account`'s
/// password opens it; [`Failure::Taken`] if a member or a bot holds the
/// name. Asks Ergo up to `checks` times while it doesn't answer or has
/// no such account, as a registration may still be finishing.
async fn unclaimed_account(
    app: &App,
    account: &NewAccount,
    checks: usize,
) -> Result<Option<MemberName>, Failure> {
    match name_held(app, &account.name).await {
        Ok(false) => {}
        Ok(true) => return Err(Failure::Taken),
        Err(error) => {
            eprintln!("Looking up a name failed: {error}");
            return Err(Failure::Unreachable);
        }
    }
    match opened_account(app, account, checks).await {
        Ok(opened) => Ok(opened
            .map(|opened| MemberName::new(opened.as_str()))
            .filter(|opened| *opened == account.name)),
        Err(error) => {
            eprintln!(
                "Checking account {}'s password failed: {error}",
                account.name
            );
            Err(Failure::Unreachable)
        }
    }
}

/// The account `account`'s name and password open, asking Ergo up to
/// `checks` times, [`RECHECK_INTERVAL`] apart, until one does.
async fn opened_account(
    app: &App,
    account: &NewAccount,
    checks: usize,
) -> ergo_api::Result<Option<AccountName>> {
    let mut opened = Ok(None);
    for check in 0..checks {
        if check > 0 {
            tokio::time::sleep(RECHECK_INTERVAL).await;
        }
        opened = app
            .ergo
            .check_auth(&account.name, account.password.expose())
            .await;
        if matches!(opened, Ok(Some(_))) {
            break;
        }
    }
    opened
}

/// Whether a member or a bot holds `name`, so no newcomer may take it.
async fn name_held(app: &App, name: &MemberName) -> sqlx::Result<bool> {
    let claim = bots::name_claim(&app.db, name.as_str(), None, app.history_expiry).await?;
    Ok(claim == NameClaim::Held)
}

/// Checks `signup` against every rule, including that its invite is open.
fn validate(signup: &Signup, invite_open: bool) -> Result<NewAccount, Box<SignupForm<'_>>> {
    let code_error = (!invite_open).then(unusable_code);
    let nick = Nick::parse(&signup.nick);
    let email = Email::parse(&signup.email);
    let password = password_rules::check(signup.password.expose(), signup.confirmation.expose());
    match (code_error, nick, email, password) {
        (None, Ok(nick), Ok(email), Ok(password)) => Ok(NewAccount {
            name: nick.into(),
            email,
            password,
            news: signup.news.is_some(),
        }),
        (code_error, nick, email, password) => Err(Box::new(SignupForm {
            code_error,
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
        code: signup.code.expose(),
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
    name: &MemberName,
    mailed_to: Option<&str>,
    VisitorPlatform(platform): VisitorPlatform,
    cookie: Option<HeaderValue>,
) -> Response {
    let nick = name.as_str();
    let page = Welcome {
        layout: Layout::new(app, cookie.as_ref().map(|_| nick)).on(Tab::Home),
        nick,
        mailed_to,
        recommended: clients::recommended(platform),
        webchat: &app.webchat,
    };
    let mut response = render(&page);
    if let Some(cookie) = cookie {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}

/// The invite `code`, if anyone could redeem it right now.
pub(super) async fn open_invite(app: &App, code: &str) -> sqlx::Result<Option<Invite>> {
    let invite = app.db.invite(code).await?;
    Ok(invite.filter(|invite| invite.status(db::now()) == invite_rules::Status::Open))
}

fn page<'a>(app: &'a App, inviter: Option<&'a str>, form: SignupForm<'a>) -> InvitePage<'a> {
    InvitePage {
        layout: Layout::new(app, None),
        inviter,
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

/// The one answer for every code that can't be redeemed, whatever the
/// reason, so a code gives nothing away.
pub(super) fn unusable_code() -> String {
    format!(
        "This invite doesn't work. Invites work once and expire after {}; ask for a new one.",
        invite_rules::in_words(invite_rules::LIFETIME)
    )
}

fn unreachable(app: &App, visitor: &Visitor) -> Response {
    message(
        app,
        visitor,
        StatusCode::SERVICE_UNAVAILABLE,
        "Couldn't reach the chat server.",
        &["Your invite still works. Try again in a minute."],
    )
}

fn unfinished(app: &App, visitor: &Visitor) -> Response {
    message(
        app,
        visitor,
        StatusCode::INTERNAL_SERVER_ERROR,
        "Your account was created, but setup didn't finish.",
        &["Ask whoever invited you to tell an admin, who can clear the nick for a new invite."],
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
