//! An in-memory stand-in for Ergo's API, for tests.

use std::{
    collections::VecDeque,
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

use crate::{Client, Endpoint, ErrorCode, ServerStatus, Status, UserCounts};

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

/// An HTTP server on loopback that answers like Ergo's API. Stopping or dropping it makes calls fail with transport errors.
pub struct FakeErgo {
    url: Url,
    state: Shared,
    server: JoinHandle<()>,
}

type Shared = Arc<Mutex<FakeState>>;

#[derive(Default)]
struct FakeState {
    failures: VecDeque<Failure>,
}

impl FakeErgo {
    pub async fn start() -> Self {
        let state = Shared::default();
        let app = Router::new()
            .route(Status::PATH, post(status))
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

async fn status() -> Response {
    succeeded(ServerStatus {
        version: "2.19.1-fake".into(),
        start_time: TIMESTAMP.into(),
        users: UserCounts { total: 0, max: 0 },
        channels: 0,
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
