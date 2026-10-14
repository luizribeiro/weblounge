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
    let router = Router::new()
        .route("/", get(routes::index::show))
        .route("/rules", get(routes::pages::rules))
        .route("/privacy", get(routes::pages::privacy))
        .route("/healthz", get(|| async { "ok" }))
        .route(&format!("{}{{file}}", assets::URL_PREFIX), get(asset))
        .with_state(Arc::new(assets));
    // Pages reload themselves when the dev loop restarts concierge.
    #[cfg(debug_assertions)]
    let router = router.layer(tower_livereload::LiveReloadLayer::new());
    router
}

async fn asset(State(assets): State<AppState>, Path(file): Path<String>) -> impl IntoResponse {
    assets.response(&file).ok_or(StatusCode::NOT_FOUND)
}
