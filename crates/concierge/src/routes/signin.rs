//! Signing in with the IRC password, which Ergo checks, and signing out.

use std::time::Instant;

use askama::Template;
use axum::{
    Form,
    extract::{Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use super::{Layout, internal_error, render, render_with};
use crate::{
    app::{App, AppState},
    auth::{self, Visitor},
    db::Member,
    nick, rate_limit, session,
};

/// The same answer whether the account exists or not.
const WRONG: &str = "Wrong nick or password.";
const UNAVAILABLE: &str = "Can't check passwords right now. Try again in a minute.";
/// Ergo's limit; anything longer can't be anyone's password.
const MAX_PASSWORD_BYTES: usize = 300;

#[derive(Template)]
#[template(path = "signin.html")]
struct SignIn<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    next: Option<&'a str>,
    error: Option<&'a str>,
    nick_max_length: usize,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Next {
    next: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Credentials {
    nick: String,
    password: String,
    next: String,
}

pub async fn show(
    State(app): State<AppState>,
    visitor: Visitor,
    Query(Next { next }): Query<Next>,
) -> Response {
    let next = auth::local_path(&next);
    if visitor.0.is_some() {
        return Redirect::to(next.unwrap_or("/")).into_response();
    }
    render(&page(&app, "", next, None))
}

pub async fn submit(
    State(app): State<AppState>,
    visitor: Visitor,
    address: rate_limit::ClientAddress,
    Form(credentials): Form<Credentials>,
) -> Response {
    let Credentials {
        nick,
        password,
        next,
    } = &credentials;
    let next = auth::local_path(next);
    let refuse = |status, error| render_with(status, &page(&app, nick, next, Some(error)));
    if !plausible(nick, password) {
        return refuse(StatusCode::UNAUTHORIZED, WRONG);
    }
    let now = Instant::now();
    if !app.limiter.allows(nick, address.0, now) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, rate_limit::TRY_LATER);
    }
    match authenticate(&app, nick, password).await {
        Ok(Some(member)) => {
            app.limiter.record_success(nick);
            start_session(&app, &visitor, &member, next.unwrap_or("/")).await
        }
        Ok(None) => {
            app.limiter.record_failure(nick, address.0, now);
            refuse(StatusCode::UNAUTHORIZED, WRONG)
        }
        Err(error) => {
            eprintln!("Checking a password failed: {error:#}");
            refuse(StatusCode::SERVICE_UNAVAILABLE, UNAVAILABLE)
        }
    }
}

pub async fn sign_out(State(app): State<AppState>, visitor: Visitor) -> Response {
    if let Some(signed_in) = &visitor.0
        && let Err(error) = app.db.end_session(&signed_in.token_hash).await
    {
        return internal_error("Ending a session", error);
    }
    let cookie = [(header::SET_COOKIE, session::removal_cookie())];
    (cookie, Redirect::to(auth::SIGN_IN_PATH)).into_response()
}

fn page<'a>(
    app: &'a App,
    nick: &'a str,
    next: Option<&'a str>,
    error: Option<&'a str>,
) -> SignIn<'a> {
    SignIn {
        layout: Layout {
            assets: &app.assets,
            nick: None,
        },
        nick,
        next,
        error,
        nick_max_length: nick::MAX_LENGTH,
    }
}

/// Whether the form holds something worth asking Ergo about, and counting
/// against the limits.
fn plausible(nick: &str, password: &str) -> bool {
    (1..=nick::MAX_LENGTH).contains(&nick.len())
        && (1..=MAX_PASSWORD_BYTES).contains(&password.len())
}

/// The current member the password opens, under Ergo's spelling of the name.
async fn authenticate(app: &App, nick: &str, password: &str) -> anyhow::Result<Option<Member>> {
    let Some(account) = app.ergo.check_auth(nick, password).await? else {
        return Ok(None);
    };
    let member = app.db.member(account.as_str()).await?;
    Ok(member.filter(|member| member.removed_at.is_none()))
}

async fn start_session(app: &App, visitor: &Visitor, member: &Member, next: &str) -> Response {
    match auth::start_session(app, visitor, &member.account).await {
        Ok(cookie) => ([(header::SET_COOKIE, cookie)], Redirect::to(next)).into_response(),
        Err(error) => internal_error("Starting a session", error),
    }
}
