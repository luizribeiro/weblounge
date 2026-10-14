use axum::{Router, routing::get};

pub fn router() -> Router {
    Router::new().route("/healthz", get(|| async { "ok" }))
}
