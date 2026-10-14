use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{get, post},
};
use ergo_api::Client;

use crate::{
    assets::{self, Assets},
    auth, csrf,
    db::Db,
    routes, session,
};

/// What every request handler shares.
pub struct App {
    pub assets: Assets,
    pub db: Db,
    pub ergo: Client,
    pub sessions: session::Hasher,
    /// The site's origin, which forms must post from.
    pub public_url: String,
}

pub type AppState = Arc<App>;

pub fn router(app: App) -> Router {
    let public_url = app.public_url.clone();
    let state = Arc::new(app);
    let pages = Router::new()
        .route("/", get(routes::index::show))
        .route(
            "/signin",
            get(routes::signin::show).post(routes::signin::submit),
        )
        .route("/signout", post(routes::signin::sign_out))
        .route("/rules", get(routes::pages::rules))
        .route("/privacy", get(routes::pages::privacy))
        .layer(middleware::from_fn_with_state(state.clone(), auth::resolve));
    let router = Router::new()
        .merge(pages)
        .route("/healthz", get(|| async { "ok" }))
        .route(&format!("{}{{file}}", assets::URL_PREFIX), get(asset))
        .layer(middleware::from_fn_with_state(public_url, csrf::guard))
        .with_state(state);
    // Pages reload themselves when the dev loop restarts concierge.
    #[cfg(debug_assertions)]
    let router = router.layer(tower_livereload::LiveReloadLayer::new());
    router
}

async fn asset(State(app): State<AppState>, Path(file): Path<String>) -> impl IntoResponse {
    app.assets.response(&file).ok_or(StatusCode::NOT_FOUND)
}
