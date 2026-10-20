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
    db,
    name::MemberName,
    password_rules,
    rate_limit::{self, ClientAddress},
    reset_mail,
};

const WRONG: &str = "Wrong password.";

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Change {
    current: String,
    password: String,
    confirmation: String,
}

/// Why the current password a form gave was refused, and what the form
/// says under it.
pub struct Refused {
    pub status: StatusCode,
    pub error: &'static str,
}

/// Checks that `current` is `account`'s password, counting wrong ones
/// against the limits on signing in.
pub async fn check_current(
    app: &App,
    account: &MemberName,
    address: &ClientAddress,
    current: &str,
) -> Result<(), Refused> {
    let refused = |status, error| Err(Refused { status, error });
    if !(1..=password_rules::MAX_BYTES).contains(&current.len()) {
        return refused(StatusCode::UNAUTHORIZED, WRONG);
    }
    let now = Instant::now();
    if !app.limiter.allows(account.as_str(), address.0, now) {
        return refused(StatusCode::TOO_MANY_REQUESTS, rate_limit::TRY_LATER);
    }
    match app.ergo.check_auth(account, current).await {
        Ok(Some(_)) => {
            app.limiter.record_success(account.as_str());
            Ok(())
        }
        Ok(None) => {
            app.limiter.record_failure(account.as_str(), address.0, now);
            refused(StatusCode::UNAUTHORIZED, WRONG)
        }
        Err(error) => {
            eprintln!("Checking a password failed: {error}");
            refused(StatusCode::SERVICE_UNAVAILABLE, ERGO_UNAVAILABLE)
        }
    }
}

pub async fn change(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    address: ClientAddress,
    Form(change): Form<Change>,
) -> Response {
    let member = &me.member;
    let account = &member.account;
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
            return refuse(StatusCode::UNPROCESSABLE_ENTITY, form).await;
        }
    };
    if let Err(refused) = check_current(&app, account, &address, &change.current).await {
        return refuse(refused.status, current_error(refused.error)).await;
    }
    if let Err(error) = app.ergo.set_password(account, &new).await {
        eprintln!("Changing a password failed: {error}");
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            current_error(ERGO_UNAVAILABLE),
        )
        .await;
    }
    if let Err(error) = app.db.end_other_sessions(account, &me.token_hash).await {
        return internal_error("Ending a member's other sessions", error);
    }
    // Only a confirmed address proves it's the member's; an unconfirmed
    // one could be anyone's.
    if let Some(email) = member
        .email
        .as_ref()
        .filter(|_| member.email_verified_at.is_some())
    {
        reset_mail::notify_changed(
            &app.outbox,
            account,
            email,
            app.public_url.as_str(),
            db::now(),
        )
        .await;
    }
    Redirect::to("/account?password#change-password").into_response()
}
