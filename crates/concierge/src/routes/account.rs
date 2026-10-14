//! The Account tab: the member's email, news setting and password.

use askama::Template;
use axum::{
    Form,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use super::{Flash, Layout, Tab, internal_error, render_with};
use crate::{
    app::{App, AppState},
    auth::CurrentMember,
    db::Member,
    email::{self, Email},
    password_rules::FieldErrors,
};

#[derive(Template)]
#[template(path = "account.html")]
struct Account<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    flash: Flash,
    settings: SettingsForm<'a>,
    password: PasswordForm,
    email_max_length: usize,
    email_pattern: &'a str,
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

/// The parts of the Account tab a refused form fills in.
#[derive(Default)]
pub struct Forms<'a> {
    flash: Flash,
    settings: Option<SettingsForm<'a>>,
    password: PasswordForm,
}

impl Forms<'_> {
    pub fn password(password: PasswordForm) -> Self {
        Self {
            password,
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
    page(&app, &me.member, StatusCode::OK, forms)
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
            return page(&app, &me.member, StatusCode::UNPROCESSABLE_ENTITY, forms);
        }
    };
    match app.db.update_settings(account, email.as_str(), news).await {
        Ok(()) => Redirect::to("/account?saved").into_response(),
        Err(error) => internal_error("Saving settings", error),
    }
}

/// The Account tab of `member`, with `forms` filled in over what's stored.
pub fn page(app: &App, member: &Member, status: StatusCode, forms: Forms<'_>) -> Response {
    let settings = forms.settings.unwrap_or_else(|| SettingsForm {
        email: member.email.as_deref().unwrap_or(""),
        news: member.news_opt_in,
        email_error: None,
    });
    let page = Account {
        layout: Layout::new(app, Some(&member.account)).on(Tab::Account),
        nick: &member.account,
        flash: forms.flash,
        settings,
        password: forms.password,
        email_max_length: email::MAX_LENGTH,
        email_pattern: email::HTML_PATTERN,
    };
    render_with(status, &page)
}
