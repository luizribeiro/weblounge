//! What the signup form checks as you type: the server's password rules,
//! whether a nick is free, and whether the invite code from the link
//! works; and whether a name is free for a member's next bot, on the Bots
//! tab. Nicks and codes are POSTed, so they stay out of URLs and access
//! logs.

use std::time::Instant;

use axum::{
    Form, Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use secret::SecretString;
use serde::{Deserialize, Serialize};

use super::invite;
use crate::{
    app::{App, AppState},
    auth::CurrentMember,
    bots::{self, NameClaim},
    invite_code,
    name::MemberName,
    nick::Nick,
    password_rules,
    rate_limit::ClientAddress,
};

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

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct InviteLookup {
    code: SecretString,
}

/// Who sent an open invite, or the field error an unusable one gets.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum InviteAnswer {
    Open { inviter: Option<String> },
    Unusable { error: String },
}

pub async fn password_rules() -> Response {
    Json(password_rules::page_rules()).into_response()
}

pub async fn nick(
    State(app): State<AppState>,
    address: ClientAddress,
    Form(lookup): Form<Lookup>,
) -> Response {
    availability(&app, address, &lookup.nick, None).await
}

/// Whether a name is free for the signed-in member's next bot, which may
/// reuse the names of their own deleted bots.
pub async fn bot_name(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    address: ClientAddress,
    Form(lookup): Form<Lookup>,
) -> Response {
    availability(&app, address, &lookup.nick, Some(&me.member.account)).await
}

/// Whether `claimant`, a member, or None for someone signing up, may take
/// `nick`.
async fn availability(
    app: &App,
    address: ClientAddress,
    nick: &str,
    claimant: Option<&MemberName>,
) -> Response {
    if !app.lookups.allows(address.0, Instant::now()) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let status = match Nick::parse(nick) {
        Err(_) => Availability::Invalid,
        Ok(nick) => match taken(app, &nick, claimant).await {
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

pub async fn invite(
    State(app): State<AppState>,
    address: ClientAddress,
    Form(lookup): Form<InviteLookup>,
) -> Response {
    if !app.lookups.allows(address.0, Instant::now()) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let code = invite_code::from_input(lookup.code.expose());
    let answer = match invite::open_invite(&app, code).await {
        Ok(Some(invite)) => InviteAnswer::Open {
            inviter: invite.created_by.map(|inviter| inviter.to_string()),
        },
        Ok(None) => InviteAnswer::Unusable {
            error: invite::unusable_code(),
        },
        Err(error) => {
            eprintln!("Looking up an invite failed: {error}");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    Json(answer).into_response()
}

/// Whether a member, removed or not, a bot or a deleted bot whose name
/// isn't free to `claimant` yet, or an Ergo account holds the nick.
async fn taken(app: &App, nick: &Nick, claimant: Option<&MemberName>) -> anyhow::Result<bool> {
    let claim = bots::name_claim(&app.db, nick.as_str(), claimant, app.history_expiry).await?;
    if claim == NameClaim::Held {
        return Ok(true);
    }
    Ok(app.ergo.info(nick.clone()).await?.is_some())
}
