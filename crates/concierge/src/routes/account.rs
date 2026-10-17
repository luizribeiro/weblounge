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
    presence::Presence,
    render_with,
    verify::{self, Mailed},
};
use crate::{
    app::{App, AppState},
    auth::CurrentMember,
    db::{self, Bot, Member},
    email::{self, Email},
    invite_rules,
    password_rules::FieldErrors,
    tree, verification_rules,
};

#[derive(Template)]
#[template(path = "account.html")]
struct Account<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    flash: Flash,
    link_error: Option<String>,
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

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    email: String,
    news: Option<String>,
}

/// The email and news form, as stored or as typed.
struct SettingsForm<'a> {
    email: &'a str,
    news: bool,
    email_error: Option<String>,
}

/// A member's email that isn't confirmed yet.
struct Unconfirmed {
    /// Whether a link to confirm it went out.
    sent: bool,
    status: String,
}

impl Unconfirmed {
    fn of(member: &Member, now: i64) -> Option<Self> {
        if member.email_verified_at.is_some() {
            return None;
        }
        let Some(first_sent_at) = member.verification_first_sent_at else {
            return Some(Self {
                sent: false,
                status: "We haven't sent you a link to confirm it yet.".into(),
            });
        };
        let deadline = verification_rules::deadline(first_sent_at);
        let due = tree::date(deadline);
        let when = if now < deadline {
            format!(
                "Please confirm by {due} ({} left).",
                invite_rules::in_words(deadline - now)
            )
        } else {
            format!("It was due by {due}.")
        };
        Some(Self {
            sent: true,
            status: format!("Open the link we emailed you to confirm it. {when}"),
        })
    }
}

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
    Form(settings): Form<Settings>,
) -> Response {
    let account = &me.member.account;
    let news = settings.news.is_some();
    let refuse = |error: String| {
        let forms = Forms {
            settings: Some(SettingsForm {
                email: &settings.email,
                news,
                email_error: Some(error),
            }),
            ..Forms::default()
        };
        page(&app, &me.member, StatusCode::UNPROCESSABLE_ENTITY, forms)
    };
    let email = match Email::parse(&settings.email) {
        Ok(email) => email,
        Err(error) => return refuse(error.to_string()).await,
    };
    match app.db.email_taken(email.as_str(), account).await {
        Ok(false) => {}
        Ok(true) => return refuse(email::TAKEN.to_owned()).await,
        Err(error) => return internal_error("Looking up an email", error),
    }
    if let Err(error) = app.db.update_settings(account, email.as_str(), news).await {
        return internal_error("Saving settings", error);
    }
    if me.member.email.as_deref() == Some(email.as_str()) {
        return Redirect::to("/account?saved#email-and-news").into_response();
    }
    let mailed = verify::mail_link(&app, account).await;
    match app.db.member(account).await {
        Ok(Some(saved)) => answer_link(&app, &saved, mailed, true).await,
        Ok(None) => internal_error("Reading back saved settings", "no member"),
        Err(error) => internal_error("Reading back saved settings", error),
    }
}

/// Mails the member a new link to confirm their email.
pub async fn send_link(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    let mailed = verify::mail_link(&app, &me.member.account).await;
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
        Mailed::Failed => (StatusCode::SERVICE_UNAVAILABLE, UNSENT.to_owned()),
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

const UNSENT: &str = "Couldn't send a link just now. Try again in a minute.";

/// The Account tab of `member`, with `forms` filled in over what's stored.
pub async fn page(app: &App, member: &Member, status: StatusCode, forms: Forms<'_>) -> Response {
    let settings = forms.settings.unwrap_or_else(|| SettingsForm {
        email: member.email.as_deref().unwrap_or(""),
        news: member.news_opt_in,
        email_error: None,
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
