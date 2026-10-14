//! An in-memory stand-in for Ergo's API, for tests.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
};

use axum::{
    Json, Router,
    extract::{Request, State},
    http::{StatusCode, header::AUTHORIZATION},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::post,
};
use reqwest::Url;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};

use crate::{
    Account, AccountName, Ack, Authenticated, CheckAuth, Client, Endpoint, ErrorCode, Info, Passwd,
    Preference, ReplayJoins, SaRegister, ServerStatus, Settings, Status, UserCounts,
};

const TOKEN: &str = "fake-ergo-token";
const TIMESTAMP: &str = "2026-01-01T00:00:00.000Z";

/// A way to make the fake fail the next call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Failure {
    /// Answer 401, as Ergo does when the token isn't one it knows.
    Unauthorized,
    /// Refuse with `UNKNOWN_ERROR`, as Ergo does when its database fails.
    UnknownError,
}

/// An HTTP server on loopback that answers like Ergo's API, keeping accounts
/// in memory. Stopping or dropping it makes calls fail with transport errors.
pub struct FakeErgo {
    url: Url,
    state: Shared,
    server: JoinHandle<()>,
}

type Shared = Arc<Mutex<FakeState>>;

#[derive(Default)]
struct FakeState {
    accounts: BTreeMap<String, FakeAccount>,
    failures: VecDeque<Failure>,
}

struct FakeAccount {
    name: AccountName,
    passphrase: String,
    settings: Settings,
}

impl FakeErgo {
    pub async fn start() -> Self {
        let state = Shared::default();
        let app = Router::new()
            .route(Status::PATH, post(status))
            .route(SaRegister::PATH, post(saregister))
            .route(CheckAuth::PATH, post(check_auth))
            .route(Passwd::PATH, post(passwd))
            .route(Info::PATH, post(info))
            .layer(middleware::from_fn_with_state(state.clone(), gatekeeper))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback has a free port");
        let url = format!("http://{}", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });
        Self { url, state, server }
    }

    pub fn url(&self) -> Url {
        self.url.clone()
    }

    /// The bearer token the fake accepts.
    pub fn token(&self) -> &str {
        TOKEN
    }

    /// A client for the fake, with the token it accepts.
    pub fn client(&self) -> Client {
        Client::new(self.url(), TOKEN)
    }

    /// Makes the next call fail; queued failures apply in order.
    pub fn fail_next(&self, failure: Failure) {
        lock(&self.state).failures.push_back(failure);
    }

    /// Shuts the server down, once its port no longer accepts connections.
    pub async fn stop(mut self) {
        self.server.abort();
        (&mut self.server).await.ok();
    }
}

impl Drop for FakeErgo {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn lock(state: &Shared) -> MutexGuard<'_, FakeState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

async fn gatekeeper(State(state): State<Shared>, request: Request, next: Next) -> Response {
    let authorized = request
        .headers()
        .get(AUTHORIZATION)
        .is_some_and(|value| value.as_bytes() == format!("Bearer {TOKEN}").as_bytes());
    if !authorized {
        return unauthorized();
    }
    let failure = lock(&state).failures.pop_front();
    match failure {
        Some(Failure::Unauthorized) => unauthorized(),
        Some(Failure::UnknownError) => refused(ErrorCode::UnknownError, Some("simulated failure")),
        None => next.run(request).await,
    }
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, "Unauthorized\n").into_response()
}

fn succeeded(body: impl Serialize) -> Response {
    let mut body = serde_json::to_value(body).expect("responses serialize to JSON");
    body["success"] = Value::Bool(true);
    Json(body).into_response()
}

fn refused(code: ErrorCode, message: Option<&str>) -> Response {
    let mut body = json!({"success": false, "errorCode": code});
    if let Some(message) = message {
        body["error"] = message.into();
    }
    Json(body).into_response()
}

/// What Ergo answers when there's nothing to say beyond "no".
fn bare_refusal() -> Response {
    Json(json!({"success": false})).into_response()
}

/// Ergo's `ascii` casemapping, which also rejects names IRC can't carry.
fn casefold(name: &AccountName) -> Option<String> {
    let name = name.as_str();
    let valid = name.chars().all(|c| c.is_ascii_graphic())
        && !name.contains([',', '*', '?', '.', '!', '@', ':'])
        && !name.starts_with(['#', '~', '&', '@', '%', '+', '-']);
    (!name.is_empty() && valid).then(|| name.to_ascii_lowercase())
}

/// Runs `answer` on the named account, or answers `unknown` without one.
fn with_account(
    state: &Shared,
    name: &AccountName,
    unknown: impl FnOnce() -> Response,
    answer: impl FnOnce(&mut FakeAccount) -> Response,
) -> Response {
    let mut state = lock(state);
    match casefold(name).and_then(|key| state.accounts.get_mut(&key)) {
        Some(account) => answer(account),
        None => unknown(),
    }
}

/// [`with_account`] for the endpoints that refuse an empty name outright.
fn with_named_account(
    state: &Shared,
    name: &AccountName,
    unknown: impl FnOnce() -> Response,
    answer: impl FnOnce(&mut FakeAccount) -> Response,
) -> Response {
    if name.as_str().is_empty() {
        return refused(ErrorCode::InvalidRequest, None);
    }
    with_account(state, name, unknown, answer)
}

fn account_does_not_exist() -> Response {
    refused(ErrorCode::AccountDoesNotExist, None)
}

async fn status() -> Response {
    succeeded(ServerStatus {
        version: "2.19.1-fake".into(),
        start_time: TIMESTAMP.into(),
        users: UserCounts { total: 0, max: 0 },
        channels: 0,
    })
}

async fn saregister(State(state): State<Shared>, Json(request): Json<SaRegister>) -> Response {
    let Some(key) = casefold(&request.account_name) else {
        return refused(
            ErrorCode::UnknownError,
            Some("Account could not be created"),
        );
    };
    let mut state = lock(&state);
    if state.accounts.contains_key(&key) {
        return refused(
            ErrorCode::AccountExists,
            Some("Name reserved due to a prior registration"),
        );
    }
    state.accounts.insert(
        key,
        FakeAccount {
            name: request.account_name,
            passphrase: request.passphrase.into(),
            settings: Settings {
                always_on: Preference::Default,
                auto_away: Preference::Default,
                email: None,
                replay_joins: ReplayJoins::CommandsOnly,
            },
        },
    );
    succeeded(Ack {})
}

async fn check_auth(State(state): State<Shared>, Json(request): Json<CheckAuth>) -> Response {
    with_account(&state, &request.account_name, bare_refusal, |account| {
        if !request.passphrase.is_empty() && account.passphrase == request.passphrase {
            succeeded(Authenticated {
                account_name: account.name.clone(),
            })
        } else {
            bare_refusal()
        }
    })
}

async fn passwd(State(state): State<Shared>, Json(request): Json<Passwd>) -> Response {
    with_account(
        &state,
        &request.account_name,
        account_does_not_exist,
        |account| {
            account.passphrase = request.passphrase.into();
            succeeded(Ack {})
        },
    )
}

async fn info(State(state): State<Shared>, Json(request): Json<Info>) -> Response {
    with_named_account(&state, &request.account_name, bare_refusal, |account| {
        succeeded(Account {
            account_name: account.name.clone(),
            email: account.settings.email.clone(),
            registered_at: Some(TIMESTAMP.into()),
            channels: Vec::new(),
        })
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    #[tokio::test]
    async fn rejects_other_tokens() {
        let ergo = FakeErgo::start().await;
        let error = Client::new(ergo.url(), "stolen")
            .status()
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Unauthorized(_)), "{error:?}");
        ergo.client().status().await.unwrap();
    }

    #[tokio::test]
    async fn fails_the_queued_calls_in_order_then_recovers() {
        let ergo = FakeErgo::start().await;
        let client = ergo.client();
        ergo.fail_next(Failure::UnknownError);
        ergo.fail_next(Failure::Unauthorized);

        let error = client.status().await.unwrap_err();
        assert_eq!(error.code(), Some(&ErrorCode::UnknownError));
        let error = client.status().await.unwrap_err();
        assert!(matches!(error, Error::Unauthorized(_)), "{error:?}");
        client.status().await.unwrap();
    }

    #[tokio::test]
    async fn a_stopped_fake_is_unreachable() {
        let ergo = FakeErgo::start().await;
        let client = ergo.client();
        ergo.stop().await;
        let error = client.status().await.unwrap_err();
        assert!(matches!(error, Error::Transport(_)), "{error:?}");
    }
}
