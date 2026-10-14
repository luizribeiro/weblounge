use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    middleware::{self, map_response},
    response::IntoResponse,
    routing::{get, post},
};
use ergo_api::Client;

use crate::{
    assets::{self, Assets},
    auth,
    config::IrcServer,
    csrf,
    db::Db,
    oauth::Webchat,
    rate_limit::{AddressLimiter, Limiter},
    routes, session,
};

/// What every request handler shares.
pub struct App {
    pub assets: Assets,
    pub db: Db,
    pub ergo: Client,
    pub sessions: session::Hasher,
    pub limiter: Limiter,
    pub lookups: AddressLimiter,
    pub oauth_requests: AddressLimiter,
    /// The site's origin, which forms must post from.
    pub public_url: String,
    /// The web client, which signs members in through our OAuth server.
    pub webchat: Webchat,
    /// Where members point their IRC clients.
    pub irc_server: IrcServer,
}

pub type AppState = Arc<App>;

pub fn router(app: App) -> Router {
    let public_url = app.public_url.clone();
    let webchat_origin = app.webchat.origin.clone();
    let state = Arc::new(app);
    let pages = Router::new()
        .route(
            "/",
            get(routes::account::show).layer(map_response(routes::no_store)),
        )
        .route(
            "/settings",
            post(routes::account::update).layer(map_response(routes::no_store)),
        )
        .route("/invites", post(routes::invites::create))
        .route("/invites/revoke", post(routes::invites::revoke))
        .route(
            "/signin",
            get(routes::signin::show).post(routes::signin::submit),
        )
        .route("/signout", post(routes::signin::sign_out))
        .route("/signup/nick", post(routes::checks::nick))
        .route("/password-rules.json", get(routes::checks::password_rules))
        .route("/rules", get(routes::pages::rules))
        .route("/privacy", get(routes::pages::privacy))
        .route("/clients/{slug}", get(routes::clients::show))
        .route(
            routes::oauth::AUTHORIZE_PATH,
            get(routes::oauth::authorize).layer(map_response(routes::no_store)),
        )
        .route(
            "/invite/{code}",
            get(routes::invite::show)
                .post(routes::invite::redeem)
                .layer(map_response(routes::no_store)),
        )
        .layer(middleware::from_fn_with_state(state.clone(), auth::resolve));
    // The web client reads this from its own origin.
    let webchat_api = Router::new()
        .route(routes::oauth::METADATA_PATH, get(routes::oauth::metadata))
        .layer(middleware::from_fn_with_state(
            webchat_origin,
            routes::oauth::allow_webchat,
        ));
    let router = Router::new()
        .merge(pages)
        .route("/healthz", get(|| async { "ok" }))
        .route(&format!("{}{{file}}", assets::URL_PREFIX), get(asset))
        .layer(middleware::from_fn_with_state(public_url, csrf::guard))
        .merge(webchat_api)
        .with_state(state);
    // Pages reload themselves when the dev loop restarts concierge.
    #[cfg(debug_assertions)]
    let router = router.layer(tower_livereload::LiveReloadLayer::new());
    router
}

async fn asset(State(app): State<AppState>, Path(file): Path<String>) -> impl IntoResponse {
    app.assets.response(&file).ok_or(StatusCode::NOT_FOUND)
}
