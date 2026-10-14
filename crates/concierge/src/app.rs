use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};

use crate::{
    assets::{self, Assets},
    routes,
};

pub type AppState = Arc<Assets>;

pub fn router(assets: Assets) -> Router {
    Router::new()
        .route("/", get(routes::index::show))
        .route("/healthz", get(|| async { "ok" }))
        .route(&format!("{}{{file}}", assets::URL_PREFIX), get(asset))
        .with_state(Arc::new(assets))
}

async fn asset(State(assets): State<AppState>, Path(file): Path<String>) -> impl IntoResponse {
    assets.response(&file).ok_or(StatusCode::NOT_FOUND)
}
