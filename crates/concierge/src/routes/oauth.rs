//! The OAuth 2.0 endpoints the web client signs members in through.

use std::time::Instant;

use axum::{
    Form, Json,
    extract::{Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::internal_error;
use crate::{
    app::AppState,
    auth::{self, Visitor},
    db::{self, NewAccessToken, NewAuthorizationCode},
    oauth::{self, AuthorizationRequest, Refusal, TokenRequest, WEBCHAT_CLIENT},
    rate_limit::ClientAddress,
    session::Token,
};

pub const METADATA_PATH: &str = "/.well-known/oauth-authorization-server";
pub const AUTHORIZE_PATH: &str = "/oauth/authorize";
pub const TOKEN_PATH: &str = "/oauth/token";
pub const INTROSPECTION_PATH: &str = "/oauth/introspect";

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

/// Trades a code, with its PKCE verifier, for an access token that lasts
/// as long as the session that approved the code. gamja keeps no refresh
/// token and reuses its access token on every reconnection, so a token
/// shorter than the session would cut members off mid-session.
pub async fn token(
    State(app): State<AppState>,
    address: ClientAddress,
    Form(request): Form<TokenRequest>,
) -> Response {
    if !app.oauth_requests.allows(address.0, Instant::now()) {
        return token_error(StatusCode::TOO_MANY_REQUESTS, "temporarily_unavailable");
    }
    if request.grant_type != "authorization_code" {
        return token_error(StatusCode::BAD_REQUEST, "unsupported_grant_type");
    }
    if request.client_id != WEBCHAT_CLIENT {
        return token_error(StatusCode::UNAUTHORIZED, "invalid_client");
    }
    let Some(code) = Token::parse(&request.code) else {
        return token_error(StatusCode::BAD_REQUEST, "invalid_grant");
    };
    let now = db::now();
    let code = match app
        .db
        .take_authorization_code(&app.sessions.hash(&code))
        .await
    {
        Ok(code) => code.filter(|code| request.redeems(code, now)),
        Err(error) => return internal_error("Taking an authorization code", error),
    };
    let Some(code) = code else {
        return token_error(StatusCode::BAD_REQUEST, "invalid_grant");
    };
    let session = match app.db.session(&code.session_hash, now).await {
        Ok(Some(session)) => session,
        Ok(None) => return token_error(StatusCode::BAD_REQUEST, "invalid_grant"),
        Err(error) => return internal_error("Looking up a session", error),
    };
    let token = Token::generate();
    let issued = NewAccessToken {
        token_hash: &app.sessions.hash(&token),
        session_hash: &code.session_hash,
        client_id: WEBCHAT_CLIENT,
        created_at: now,
        expires_at: session.expires_at,
    };
    if let Err(error) = app.db.insert_access_token(&issued).await {
        return internal_error("Issuing an access token", error);
    }
    let answer = json!({
        "access_token": token.as_str(),
        "token_type": "Bearer",
        "expires_in": session.expires_at - now,
    });
    super::no_store(Json(answer).into_response()).await
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Introspection {
    token: String,
}

/// RFC 7662 introspection, for Ergo alone: it authenticates with its
/// secret, and learns whose token gamja logged in with, in the
/// `username` claim it reads.
pub async fn introspect(
    State(app): State<AppState>,
    headers: HeaderMap,
    Form(Introspection { token }): Form<Introspection>,
) -> Response {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    if !oauth::authenticates_introspection(authorization, &app.introspection_secret) {
        let challenge = [(header::WWW_AUTHENTICATE, "Basic realm=\"concierge\"")];
        let error = token_error(StatusCode::UNAUTHORIZED, "invalid_client");
        return (challenge, error).into_response();
    }
    let live = match Token::parse(&token) {
        Some(token) => {
            app.db
                .access_token(&app.sessions.hash(&token), db::now())
                .await
        }
        None => Ok(None),
    };
    let answer = match live {
        Ok(Some(token)) => json!({
            "active": true,
            "username": token.account,
            "client_id": token.client_id,
            "token_type": "Bearer",
            "exp": token.expires_at,
        }),
        Ok(None) => json!({ "active": false }),
        Err(error) => return internal_error("Introspecting a token", error),
    };
    super::no_store(Json(answer).into_response()).await
}

fn token_error(status: StatusCode, error: &str) -> Response {
    (status, Json(json!({ "error": error }))).into_response()
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
