use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    middleware::{self, map_response},
    response::{IntoResponse, Redirect},
    routing::{get, post},
};
use ergo_api::Client;
use outbox::Outbox;

use crate::{
    assets::{self, Assets},
    auth,
    background::Background,
    config::{IrcServer, Presence},
    csrf,
    db::Db,
    oauth::Webchat,
    origin::{FilesOrigin, SiteOrigin},
    rate_limit::{AddressLimiter, Limiter, MailLimiter, UploadLimiter},
    reset_mail, routes, session,
    uploads::Store,
};

/// What every request handler shares.
pub struct App {
    pub assets: Assets,
    pub db: Db,
    /// Where the site leaves mail for concierge-mailer to send.
    pub outbox: Outbox,
    pub ergo: Client,
    pub sessions: session::Hasher,
    pub limiter: Limiter,
    pub lookups: AddressLimiter,
    pub oauth_requests: AddressLimiter,
    pub verifications: AddressLimiter,
    /// Work requests answer without waiting for.
    pub background: Background,
    /// Asking for reset links and using them, by client address.
    pub recoveries: AddressLimiter,
    /// Making, resetting and deleting bots, by client address.
    pub bot_changes: AddressLimiter,
    /// Verification links mailed lately.
    pub mailed_links: MailLimiter,
    pub reset_links: MailLimiter,
    /// The site's origin, which forms must post from.
    pub public_url: SiteOrigin,
    /// What invite links start with; the code follows.
    pub invite_links: String,
    /// The web client, which signs members in through our OAuth server.
    pub webchat: Webchat,
    /// What Ergo authenticates with to introspect the web client's tokens.
    pub introspection_secret: String,
    /// Where members point their IRC clients.
    pub irc_server: IrcServer,
    /// What Ergo turns on for members who haven't chosen.
    pub presence_defaults: Presence,
    /// How long Ergo keeps chat history, in seconds.
    pub history_expiry: i64,
    /// Where uploads are handed to the scrubber, and stored.
    pub uploads: Store,
    /// Where uploads are served from.
    pub files_url: FilesOrigin,
    /// Uploads stored lately, by account.
    pub recent_uploads: UploadLimiter,
}

pub type AppState = Arc<App>;

impl App {
    /// Stops counting the links the mailer deleted unsent against their
    /// members, who never got them.
    pub async fn give_back_unsent_links(&self) {
        match self.outbox.take_unsent().await {
            Ok(unsent) => {
                self.mailed_links.refund(&unsent);
                self.reset_links.refund(&unsent);
            }
            Err(error) => eprintln!("Reading the links deleted unsent failed: {error}"),
        }
    }
}

pub fn router(app: App) -> Router {
    let public_url = app.public_url.clone();
    let webchat_origin = app.webchat.origin.clone();
    let upload_origin = webchat_origin.clone();
    let state = Arc::new(app);
    let pages = Router::new()
        .route(
            "/",
            get(routes::home::show).layer(map_response(routes::no_store)),
        )
        .route(
            "/account",
            get(routes::account::show).layer(map_response(routes::no_store)),
        )
        .route(
            "/settings",
            post(routes::account::update).layer(map_response(routes::no_store)),
        )
        .route(
            "/verification-link",
            post(routes::account::send_link).layer(map_response(routes::no_store)),
        )
        .route(
            "/password",
            post(routes::password::change).layer(map_response(routes::no_store)),
        )
        .route(
            "/irc",
            get(|| async { Redirect::permanent("/account#irc-settings") }),
        )
        .route(
            "/irc-settings",
            post(routes::presence::update).layer(map_response(routes::no_store)),
        )
        .route(
            "/invites",
            get(routes::invites::show)
                .post(routes::invites::create)
                .layer(map_response(routes::no_store)),
        )
        .route(
            "/invites/revoke",
            get(routes::invites::confirm_revoke)
                .post(routes::invites::revoke)
                .layer(map_response(routes::no_store)),
        )
        .route(
            "/bots",
            get(routes::bots::show)
                .post(routes::bots::create)
                .layer(map_response(routes::no_store)),
        )
        .route(
            "/bots/{name}/password",
            get(routes::bots::confirm_reset)
                .post(routes::bots::reset)
                .layer(map_response(routes::no_store)),
        )
        .route(
            "/bots/{name}/delete",
            get(routes::bots::confirm_delete)
                .post(routes::bots::delete)
                .layer(map_response(routes::no_store)),
        )
        .route(
            routes::uploads::PATH,
            get(routes::uploads::show).layer(map_response(routes::no_store)),
        )
        .route(
            "/uploads/{id}/delete",
            get(routes::uploads::confirm_delete)
                .post(routes::uploads::delete)
                .layer(map_response(routes::no_store)),
        )
        .route(
            "/signin",
            get(routes::signin::show).post(routes::signin::submit),
        )
        .route("/signout", post(routes::signin::sign_out))
        .route("/signup/nick", post(routes::checks::nick))
        .route("/bots/name", post(routes::checks::bot_name))
        .route("/signup/invite", post(routes::checks::invite))
        .route("/password-rules.json", get(routes::checks::password_rules))
        .route("/rules", get(routes::pages::rules))
        .route("/privacy", get(routes::pages::privacy))
        .route(routes::help::PATH, get(routes::help::index))
        .route("/clients", get(routes::clients::index))
        .route("/clients/{slug}", get(routes::clients::show))
        .route(
            routes::oauth::AUTHORIZE_PATH,
            get(routes::oauth::authorize).layer(map_response(routes::no_store)),
        )
        .route(
            "/invite",
            get(routes::invite::show)
                .post(routes::invite::redeem)
                .layer(map_response(routes::no_store)),
        )
        .route(
            "/verify",
            get(routes::verify::show)
                .post(routes::verify::confirm)
                .layer(map_response(routes::no_store)),
        )
        .route(
            reset_mail::RESET_PATH,
            get(routes::reset::show)
                .post(routes::reset::reset)
                .layer(map_response(routes::no_store)),
        )
        .route(
            reset_mail::FORGOT_PATH,
            get(routes::reset::forgot)
                .post(routes::reset::ask)
                .layer(map_response(routes::no_store)),
        )
        .fallback(|| async { routes::not_found() })
        .layer(middleware::from_fn_with_state(state.clone(), auth::resolve))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            routes::error_pages,
        ));
    // The web client calls these from its own origin, so they sit outside
    // the guard for our pages; the token endpoint takes posts from that
    // origin alone, and PKCE and single-use codes protect it.
    let webchat_api = Router::new()
        .route(routes::oauth::METADATA_PATH, get(routes::oauth::metadata))
        .route(
            routes::oauth::TOKEN_PATH,
            post(routes::oauth::token)
                .layer(map_response(routes::no_store))
                .layer(middleware::from_fn_with_state(
                    webchat_origin.clone(),
                    csrf::guard_webchat,
                )),
        )
        .layer(middleware::from_fn_with_state(
            webchat_origin,
            routes::oauth::allow_webchat,
        ));
    let router = Router::new()
        .merge(pages)
        .route("/healthz", get(|| async { "ok" }))
        .route(&format!("{}{{file}}", assets::URL_PREFIX), get(asset))
        .layer(middleware::from_fn_with_state(public_url, csrf::guard_site))
        .merge(webchat_api)
        // Ergo calls this, not a browser, and authenticates with its secret.
        .route(
            routes::oauth::INTROSPECTION_PATH,
            post(routes::oauth::introspect).layer(map_response(routes::no_store)),
        )
        // IRC clients post uploads with the credentials they log in with,
        // through files.weblounge.org alone; nginx keeps it off this site.
        .route(
            routes::upload::PATH,
            post(routes::upload::upload)
                .options(routes::upload::preflight)
                .layer(middleware::from_fn_with_state(
                    upload_origin,
                    routes::upload::allow_webchat,
                )),
        )
        .with_state(state);
    // Pages reload themselves when the dev loop restarts concierge.
    #[cfg(debug_assertions)]
    let router = router.layer(tower_livereload::LiveReloadLayer::new());
    router
}

async fn asset(State(app): State<AppState>, Path(file): Path<String>) -> impl IntoResponse {
    app.assets.response(&file).ok_or(StatusCode::NOT_FOUND)
}
