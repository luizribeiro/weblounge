use axum::{Router, routing::get};

use crate::routes;

pub fn router() -> Router {
    Router::new()
        .route("/", get(routes::index::show))
        .route("/healthz", get(|| async { "ok" }))
}
