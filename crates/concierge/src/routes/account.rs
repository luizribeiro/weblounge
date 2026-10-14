//! The account home: how to connect a client, and the member's settings.

use askama::Template;
use axum::{
    Form,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use super::{Layout, internal_error, render, render_with};
use crate::{
    app::{App, AppState},
    auth::CurrentMember,
    clients::{CLIENTS, Client},
    config::IrcServer,
    email::{self, Email},
};

#[derive(Template)]
#[template(path = "account.html")]
struct Account<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    irc: &'a IrcServer,
    email: &'a str,
    email_error: Option<String>,
    news: bool,
    saved: bool,
    email_max_length: usize,
    email_pattern: &'a str,
    clients: &'a [Client],
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Flash {
    saved: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    email: String,
    news: Option<String>,
}

pub async fn show(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    Query(flash): Query<Flash>,
) -> Response {
    let member = &me.member;
    let page = Account {
        saved: flash.saved.is_some(),
        ..page(
            &app,
            &member.account,
            member.email.as_deref().unwrap_or(""),
            member.news_opt_in,
        )
    };
    render(&page)
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
            let page = Account {
                email_error: Some(error.to_string()),
                ..page(&app, account, &settings.email, news)
            };
            return render_with(StatusCode::UNPROCESSABLE_ENTITY, &page);
        }
    };
    match app.db.update_settings(account, email.as_str(), news).await {
        Ok(()) => Redirect::to("/?saved").into_response(),
        Err(error) => internal_error("Saving settings", error),
    }
}

fn page<'a>(app: &'a App, nick: &'a str, email: &'a str, news: bool) -> Account<'a> {
    Account {
        layout: Layout {
            assets: &app.assets,
            nick: Some(nick),
        },
        nick,
        irc: &app.irc_server,
        email,
        email_error: None,
        news,
        saved: false,
        email_max_length: email::MAX_LENGTH,
        email_pattern: email::HTML_PATTERN,
        clients: &CLIENTS,
    }
}
