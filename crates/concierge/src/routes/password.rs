//! A member changing their password: the current one, checked by Ergo and
//! rate-limited like signing in, and a new one under the signup rules.

use std::time::Instant;

use axum::{
    Form,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use super::{
    ERGO_UNAVAILABLE,
    account::{self, Forms, PasswordForm},
    internal_error,
};
use crate::{
    app::{App, AppState},
    auth::CurrentMember,
    password_rules,
    rate_limit::{self, ClientAddress},
};

const WRONG: &str = "Wrong password.";

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Change {
    current: String,
    password: String,
    confirmation: String,
}

pub async fn change(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    address: ClientAddress,
    Form(change): Form<Change>,
) -> Response {
    let member = &me.member;
    let account = member.account.as_str();
    let refuse = |status, password| account::page(&app, member, status, Forms::password(password));
    let current_error = |error| PasswordForm {
        current_error: Some(error),
        ..PasswordForm::default()
    };
    let new = match password_rules::check(&change.password, &change.confirmation) {
        Ok(new) => new,
        Err(refusal) => {
            let form = PasswordForm {
                new: refusal.into(),
                ..PasswordForm::default()
            };
            return refuse(StatusCode::UNPROCESSABLE_ENTITY, form);
        }
    };
    if !(1..=password_rules::MAX_BYTES).contains(&change.current.len()) {
        return refuse(StatusCode::UNAUTHORIZED, current_error(WRONG));
    }
    let now = Instant::now();
    if !app.limiter.allows(account, address.0, now) {
        let form = current_error(rate_limit::TRY_LATER);
        return refuse(StatusCode::TOO_MANY_REQUESTS, form);
    }
    match verify(&app, account, &change.current).await {
        Ok(true) => app.limiter.record_success(account),
        Ok(false) => {
            app.limiter.record_failure(account, address.0, now);
            return refuse(StatusCode::UNAUTHORIZED, current_error(WRONG));
        }
        Err(error) => {
            eprintln!("Checking a password failed: {error}");
            return refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                current_error(ERGO_UNAVAILABLE),
            );
        }
    }
    if let Err(error) = app.ergo.set_password(account, &new).await {
        eprintln!("Changing a password failed: {error}");
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            current_error(ERGO_UNAVAILABLE),
        );
    }
    match app.db.end_other_sessions(account, &me.token_hash).await {
        Ok(_) => Redirect::to("/account?password#change-password").into_response(),
        Err(error) => internal_error("Ending a member's other sessions", error),
    }
}

async fn verify(app: &App, account: &str, password: &str) -> ergo_api::Result<bool> {
    Ok(app.ergo.check_auth(account, password).await?.is_some())
}
