//! The account home: how to connect a client, and the member's settings.

use askama::Template;
use axum::{
    Form,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use super::{Layout, internal_error, invites::Invites, presence::Presence, render_with};
use crate::{
    app::{App, AppState},
    auth::CurrentMember,
    clients::{CLIENTS, Client},
    config::IrcServer,
    db::{self, Member},
    email::{self, Email},
    oauth::Webchat,
    password_rules::FieldErrors,
};

#[derive(Template)]
#[template(path = "account.html")]
struct Account<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    irc: &'a IrcServer,
    clients: &'a [Client],
    webchat: &'a Webchat,
    invites: Invites,
    flash: Flash,
    settings: SettingsForm<'a>,
    password: PasswordForm,
    /// The IRC settings, or why they can't be shown.
    presence: Result<Presence, &'static str>,
    presence_error: Option<&'static str>,
    presence_defaults: Presence,
    email_max_length: usize,
    email_pattern: &'a str,
}

/// What the last change, redirected here, says it did.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Flash {
    saved: Option<String>,
    invited: Option<String>,
    password: Option<String>,
    irc: Option<String>,
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

/// What's wrong with a password change; never what was typed.
#[derive(Default)]
pub struct PasswordForm {
    pub current_error: Option<&'static str>,
    pub new: FieldErrors,
}

/// The parts of the account home a refused form fills in.
#[derive(Default)]
pub struct Forms<'a> {
    flash: Flash,
    settings: Option<SettingsForm<'a>>,
    password: PasswordForm,
    presence_error: Option<&'static str>,
}

impl Forms<'_> {
    pub fn password(password: PasswordForm) -> Self {
        Self {
            password,
            ..Self::default()
        }
    }

    pub fn presence_error(error: &'static str) -> Self {
        Self {
            presence_error: Some(error),
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
    home(&app, &me.member, StatusCode::OK, forms).await
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
            return home(&app, &me.member, StatusCode::UNPROCESSABLE_ENTITY, forms).await;
        }
    };
    match app.db.update_settings(account, email.as_str(), news).await {
        Ok(()) => Redirect::to("/?saved").into_response(),
        Err(error) => internal_error("Saving settings", error),
    }
}

/// The account home of `member`, with `forms` filled in over what's stored.
pub async fn home(app: &App, member: &Member, status: StatusCode, forms: Forms<'_>) -> Response {
    let invites = match Invites::of(app, member, db::now()).await {
        Ok(invites) => invites,
        Err(error) => return internal_error("Listing invites", error),
    };
    let presence = match app.ergo.settings(member.account.as_str()).await {
        Ok(settings) => Ok(Presence::of(&settings, app.presence_defaults)),
        Err(error) => {
            eprintln!("Reading IRC settings failed: {error}");
            Err("Can't reach the chat server.")
        }
    };
    let settings = forms.settings.unwrap_or_else(|| SettingsForm {
        email: member.email.as_deref().unwrap_or(""),
        news: member.news_opt_in,
        email_error: None,
    });
    let page = Account {
        layout: Layout {
            assets: &app.assets,
            nick: Some(&member.account),
        },
        nick: &member.account,
        irc: &app.irc_server,
        clients: &CLIENTS,
        webchat: &app.webchat,
        invites,
        flash: forms.flash,
        settings,
        password: forms.password,
        presence_error: forms.presence_error.filter(|_| presence.is_ok()),
        presence,
        presence_defaults: app.presence_defaults,
        email_max_length: email::MAX_LENGTH,
        email_pattern: email::HTML_PATTERN,
    };
    render_with(status, &page)
}
