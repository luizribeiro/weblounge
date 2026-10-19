//! The Account tab: the member's password, email, news setting and how
//! Ergo treats them while no client is open, and the bots they own.

use askama::Template;
use axum::{
    Form,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use super::{
    Flash, Layout, Tab, internal_error,
    password::{self, Refused},
    presence::Presence,
    render_with,
    verify::{self, Due, Mailed, Unconfirmed},
};
use crate::{
    app::{App, AppState},
    auth::CurrentMember,
    db::{self, Bot, Member},
    email::{self, Email},
    password_rules::FieldErrors,
    rate_limit::{ClientAddress, Recipient},
};

#[derive(Template)]
#[template(path = "account.html")]
struct Account<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    flash: Flash,
    link_error: Option<String>,
    /// The email as stored: changing it needs the password.
    saved_email: &'a str,
    settings: SettingsForm<'a>,
    /// Where confirming the stored email stands, until it's confirmed.
    unconfirmed: Option<Unconfirmed>,
    password: PasswordForm,
    email_max_length: usize,
    email_pattern: &'a str,
    /// The IRC settings, or why they can't be shown.
    presence: Result<Presence, &'static str>,
    defaults: Presence,
    irc_error: Option<&'static str>,
    bots: Vec<Bot>,
}

impl Account<'_> {
    fn deadline(&self) -> Option<&Due> {
        self.unconfirmed.as_ref()?.upcoming()
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    email: String,
    news: Option<String>,
    /// The member's password, which changing the email needs: otherwise
    /// a stolen session could move the account's recovery to another
    /// address.
    current: String,
}

/// The email and news form, as stored or as typed.
struct SettingsForm<'a> {
    email: &'a str,
    news: bool,
    email_error: Option<String>,
    /// What's wrong with the current password given; never what was typed.
    password_error: Option<&'static str>,
}

/// What the email form says when it's sent without the password.
const PASSWORD_NEEDED: &str = "Enter your password to change your email.";

/// What's wrong with a password change; never what was typed.
#[derive(Default)]
pub struct PasswordForm {
    pub current_error: Option<&'static str>,
    pub new: FieldErrors,
}

/// The parts of the Account tab a refused form fills in.
#[derive(Default)]
pub struct Forms<'a> {
    flash: Flash,
    settings: Option<SettingsForm<'a>>,
    password: PasswordForm,
    irc_error: Option<&'static str>,
    /// Why no verification link went out.
    link_error: Option<String>,
}

impl Forms<'_> {
    pub fn password(password: PasswordForm) -> Self {
        Self {
            password,
            ..Self::default()
        }
    }

    pub fn irc_error(error: &'static str) -> Self {
        Self {
            irc_error: Some(error),
            ..Self::default()
        }
    }
}

pub async fn show(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    Query(flash): Query<Flash>,
) -> Response {
    let forms = Forms {
        flash,
        ..Forms::default()
    };
    page(&app, &me.member, StatusCode::OK, forms).await
}

pub async fn update(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    address: ClientAddress,
    Form(settings): Form<Settings>,
) -> Response {
    let account = &me.member.account;
    let news = settings.news.is_some();
    let refuse = |status, email_error, password_error| {
        let forms = Forms {
            settings: Some(SettingsForm {
                email: &settings.email,
                news,
                email_error,
                password_error,
            }),
            ..Forms::default()
        };
        page(&app, &me.member, status, forms)
    };
    let email = match Email::parse(&settings.email) {
        Ok(email) => email,
        Err(error) => {
            return refuse(
                StatusCode::UNPROCESSABLE_ENTITY,
                Some(error.to_string()),
                None,
            )
            .await;
        }
    };
    let changed = me.member.email.as_deref() != Some(email.as_str());
    if changed
        && let Err(Refused { status, error }) =
            check_password(&app, account, &address, &settings.current).await
    {
        return refuse(status, None, Some(error)).await;
    }
    match app.db.email_taken(email.as_str(), account).await {
        Ok(false) => {}
        Ok(true) => {
            let taken = Some(email::TAKEN.to_owned());
            return refuse(StatusCode::UNPROCESSABLE_ENTITY, taken, None).await;
        }
        Err(error) => return internal_error("Looking up an email", error),
    }
    if let Err(error) = app.db.update_settings(account, email.as_str(), news).await {
        return internal_error("Saving settings", error);
    }
    if !changed {
        return Redirect::to("/account?saved#email-and-news").into_response();
    }
    let mailed = verify::mail_link(&app, account, Recipient::NewAddress).await;
    match app.db.member(account).await {
        Ok(Some(saved)) => answer_link(&app, &saved, mailed, true).await,
        Ok(None) => internal_error("Reading back saved settings", "no member"),
        Err(error) => internal_error("Reading back saved settings", error),
    }
}

/// Checks the password a change of email came with, which it needs.
async fn check_password(
    app: &App,
    account: &str,
    address: &ClientAddress,
    current: &str,
) -> Result<(), Refused> {
    if current.is_empty() {
        return Err(Refused {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            error: PASSWORD_NEEDED,
        });
    }
    password::check_current(app, account, address, current).await
}

/// Mails the member a new link to confirm their email.
pub async fn send_link(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    let mailed = verify::mail_link(&app, &me.member.account, Recipient::SameAddress).await;
    answer_link(&app, &me.member, mailed, false).await
}

/// The Account tab once a link was asked for, after saving the settings
/// if `saved`.
async fn answer_link(app: &App, member: &Member, mailed: Mailed, saved: bool) -> Response {
    let (status, error) = match mailed {
        Mailed::Sent | Mailed::NothingToVerify => {
            let query = match (saved, mailed == Mailed::Sent) {
                (true, true) => "?saved&sent",
                (true, false) => "?saved",
                (false, true) => "?sent",
                (false, false) => "",
            };
            return Redirect::to(&format!("/account{query}#email-and-news")).into_response();
        }
        Mailed::Refused(refusal) => (StatusCode::TOO_MANY_REQUESTS, refusal.to_string()),
        Mailed::Failed => (StatusCode::SERVICE_UNAVAILABLE, super::UNSENT.to_owned()),
    };
    let forms = Forms {
        flash: Flash {
            saved: saved.then(String::new),
            ..Flash::default()
        },
        link_error: Some(error),
        ..Forms::default()
    };
    page(app, member, status, forms).await
}

/// The Account tab of `member`, with `forms` filled in over what's stored.
pub async fn page(app: &App, member: &Member, status: StatusCode, forms: Forms<'_>) -> Response {
    let saved_email = member.email.as_deref().unwrap_or("");
    let settings = forms.settings.unwrap_or(SettingsForm {
        email: saved_email,
        news: member.news_opt_in,
        email_error: None,
        password_error: None,
    });
    let presence = match app.ergo.settings(member.account.as_str()).await {
        Ok(settings) => Ok(Presence::of(&settings, app.presence_defaults)),
        Err(error) => {
            eprintln!("Reading IRC settings failed: {error}");
            Err("Can't reach the chat server.")
        }
    };
    let bots = match app.db.bots(Some(&member.account)).await {
        Ok(bots) => bots,
        Err(error) => return internal_error("Listing a member's bots", error),
    };
    let page = Account {
        layout: Layout::new(app, Some(&member.account)).on(Tab::Account),
        nick: &member.account,
        flash: forms.flash,
        link_error: forms.link_error,
        saved_email,
        settings,
        unconfirmed: Unconfirmed::of(member, db::now()),
        password: forms.password,
        email_max_length: email::MAX_LENGTH,
        email_pattern: email::HTML_PATTERN,
        irc_error: forms.irc_error.filter(|_| presence.is_ok()),
        presence,
        defaults: app.presence_defaults,
        bots,
    };
    render_with(status, &page)
}
