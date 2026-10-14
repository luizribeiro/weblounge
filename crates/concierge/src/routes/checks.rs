//! What the signup form checks as you type: the server's password rules,
//! and whether a nick is free. Nicks are POSTed, so they stay out of URLs
//! and access logs.

use std::time::Instant;

use axum::{
    Form, Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::{app::AppState, nick::Nick, password_rules, rate_limit::ClientAddress};

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Lookup {
    nick: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Availability {
    Available,
    Taken,
    Invalid,
}

#[derive(Serialize)]
struct Answer {
    status: Availability,
}

pub async fn password_rules() -> Response {
    Json(password_rules::page_rules()).into_response()
}

pub async fn nick(
    State(app): State<AppState>,
    address: ClientAddress,
    Form(lookup): Form<Lookup>,
) -> Response {
    if !app.lookups.allows(address.0, Instant::now()) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let status = match Nick::parse(&lookup.nick) {
        Err(_) => Availability::Invalid,
        Ok(nick) => match taken(&app, &nick).await {
            Ok(true) => Availability::Taken,
            Ok(false) => Availability::Available,
            Err(error) => {
                eprintln!("Checking a nick's availability failed: {error:#}");
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        },
    };
    Json(Answer { status }).into_response()
}

/// Whether a member, a removed member's placeholder or an Ergo account
/// holds the nick.
async fn taken(app: &crate::app::App, nick: &Nick) -> anyhow::Result<bool> {
    if app.db.member(nick.as_str()).await?.is_some() {
        return Ok(true);
    }
    Ok(app.ergo.info(nick.clone()).await?.is_some())
}
