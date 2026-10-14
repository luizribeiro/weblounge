use clap::Parser;
use std::net::SocketAddr;

use concierge::{
    admin::{self, Command},
    app::{self, App},
    assets::Assets,
    config::Config,
    db::{self, Db},
    oauth::Webchat,
    rate_limit::{AddressLimiter, LOOKUPS_PER_ADDRESS, Limiter, OAUTH_REQUESTS_PER_ADDRESS},
    session,
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let command = Command::parse();
    let config = Config::from_env()?;
    match command {
        Command::Serve => serve(config).await,
        Command::Admin(command) => admin::run(command, &config)
            .await
            .map(|output| print!("{output}")),
    }
}

async fn serve(config: Config) -> anyhow::Result<()> {
    let db = Db::open(&config.database_path()).await?;
    db.end_expired_sessions(db::now()).await?;
    let app = App {
        assets: Assets::load(&config.assets_dir)?,
        db,
        ergo: config.ergo_client()?,
        sessions: session::Hasher::new(&config.read_session_key()?),
        limiter: Limiter::default(),
        lookups: AddressLimiter::new(LOOKUPS_PER_ADDRESS),
        oauth_requests: AddressLimiter::new(OAUTH_REQUESTS_PER_ADDRESS),
        public_url: config.public_url.clone(),
        webchat: Webchat {
            origin: config.webchat_url.clone(),
        },
        introspection_secret: config.read_introspection_secret()?,
        irc_server: config.irc_server.clone(),
        presence_defaults: config.presence_defaults,
    };
    let listener = TcpListener::bind(config.listen).await?;
    eprintln!("Serving {} on {}", config.public_url, config.listen);
    let service = app::router(app).into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, service).await?;
    Ok(())
}
