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

use super::{Flash, Layout, Tab, internal_error, presence::Presence, render_with};
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
    status: String,
}

impl Unconfirmed {
    fn of(member: &Member, now: i64) -> Option<Self> {
        if member.email_verified_at.is_some() {
            return None;
        }
        let Some(first_sent_at) = member.verification_first_sent_at else {
            return Some(Self {
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
    let email = match Email::parse(&settings.email) {
        Ok(email) => email,
        Err(error) => {
            let forms = Forms {
                settings: Some(SettingsForm {
                    email: &settings.email,
                    news,
                    email_error: Some(error.to_string()),
                }),
                ..Forms::default()
            };
            return page(&app, &me.member, StatusCode::UNPROCESSABLE_ENTITY, forms).await;
        }
    };
    match app.db.update_settings(account, email.as_str(), news).await {
        Ok(()) => Redirect::to("/account?saved#email-and-news").into_response(),
        Err(error) => internal_error("Saving settings", error),
    }
}

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
