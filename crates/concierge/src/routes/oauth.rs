//! The OAuth 2.0 endpoints the web client signs members in through.

use std::time::Instant;

use axum::{
    Json,
    extract::{Query, Request, State},
    http::{HeaderValue, StatusCode, Uri, header},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use serde::Serialize;

use super::internal_error;
use crate::{
    app::AppState,
    auth::{self, Visitor},
    db::{self, NewAuthorizationCode},
    oauth::{self, AuthorizationRequest, Refusal, WEBCHAT_CLIENT},
    rate_limit::ClientAddress,
    session::Token,
};

pub const METADATA_PATH: &str = "/.well-known/oauth-authorization-server";
pub const AUTHORIZE_PATH: &str = "/oauth/authorize";
pub const TOKEN_PATH: &str = "/oauth/token";

/// RFC 8414's metadata. The introspection endpoint is left out: it's for
/// Ergo alone, and gamja would call it from the browser if it were listed.
#[derive(Serialize)]
struct Metadata<'a> {
    issuer: &'a str,
    authorization_endpoint: String,
    token_endpoint: String,
    response_types_supported: [&'a str; 1],
    grant_types_supported: [&'a str; 1],
    code_challenge_methods_supported: [&'a str; 1],
    token_endpoint_auth_methods_supported: [&'a str; 1],
}

pub async fn metadata(State(app): State<AppState>) -> Response {
    let issuer = &app.public_url;
    Json(Metadata {
        issuer,
        authorization_endpoint: format!("{issuer}{AUTHORIZE_PATH}"),
        token_endpoint: format!("{issuer}{TOKEN_PATH}"),
        response_types_supported: ["code"],
        grant_types_supported: ["authorization_code"],
        code_challenge_methods_supported: ["S256"],
        token_endpoint_auth_methods_supported: ["none"],
    })
    .into_response()
}

/// Issues a code to a signed-in member without asking: the web client is
/// our own. Anyone else signs in first and comes back here.
pub async fn authorize(
    State(app): State<AppState>,
    visitor: Visitor,
    address: ClientAddress,
    uri: Uri,
    Query(request): Query<AuthorizationRequest>,
) -> Response {
    let authorization = match request.check(&app.webchat) {
        Ok(authorization) => authorization,
        Err(refusal) => return refuse(&app, &visitor, &refusal),
    };
    let Some(signed_in) = &visitor.0 else {
        let wanted = uri
            .path_and_query()
            .map_or(AUTHORIZE_PATH, |path| path.as_str());
        return Redirect::to(&auth::sign_in_url(wanted)).into_response();
    };
    if !app.oauth_requests.allows(address.0, Instant::now()) {
        return Redirect::to(&authorization.error_redirect("temporarily_unavailable"))
            .into_response();
    }
    let code = Token::generate();
    let now = db::now();
    let issued = NewAuthorizationCode {
        code_hash: &app.sessions.hash(&code),
        session_hash: &signed_in.token_hash,
        client_id: WEBCHAT_CLIENT,
        redirect_uri: &authorization.redirect_uri,
        code_challenge: &authorization.code_challenge,
        expires_at: now + oauth::CODE_LIFETIME,
    };
    if let Err(error) = app.db.insert_authorization_code(&issued, now).await {
        return internal_error("Issuing an authorization code", error);
    }
    Redirect::to(&oauth::code_redirect(&authorization, code.as_str())).into_response()
}

fn refuse(app: &crate::app::App, visitor: &Visitor, refusal: &Refusal) -> Response {
    match refusal.redirect() {
        Some(url) => Redirect::to(&url).into_response(),
        None => super::message(
            app,
            visitor,
            StatusCode::BAD_REQUEST,
            "Sign-in",
            "This sign-in link isn't one of ours.",
            &["Open the chat from the account page instead."],
        ),
    }
}

/// Lets the web client's pages read the answers of the endpoints it
/// calls from the browser, and no other site's.
pub async fn allow_webchat(State(origin): State<String>, request: Request, next: Next) -> Response {
    let from_webchat = request
        .headers()
        .get(header::ORIGIN)
        .is_some_and(|value| value.as_bytes() == origin.as_bytes());
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.append(header::VARY, HeaderValue::from_static("origin"));
    if from_webchat && let Ok(origin) = HeaderValue::from_str(&origin) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    }
    response
}
