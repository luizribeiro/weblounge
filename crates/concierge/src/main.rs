use clap::Parser;
use std::{net::SocketAddr, path::Path};

use concierge::{
    admin::{self, Command},
    app::{self, App},
    assets::Assets,
    config::{self, Config, MailerConfig},
    db::{self, Db},
    mailer::{self, Mailer},
    oauth::Webchat,
    rate_limit::{AddressLimiter, LOOKUPS_PER_ADDRESS, Limiter, OAUTH_REQUESTS_PER_ADDRESS},
    session,
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Command::parse() {
        Command::Migrate => migrate(&config::database_from_env()?).await,
        Command::Serve => serve(Config::from_env()?).await,
        Command::Mailer => send_mail(MailerConfig::from_env()?).await,
        Command::Admin(command) => admin::run(command, &Config::from_env()?)
            .await
            .map(|output| print!("{output}")),
    }
}

async fn migrate(path: &Path) -> anyhow::Result<()> {
    let db = Db::migrate(path).await?;
    let version = db.migration_version().await?.unwrap_or_default();
    eprintln!("{} is at migration {version}", path.display());
    Ok(())
}

async fn send_mail(config: MailerConfig) -> anyhow::Result<()> {
    let db = Db::open(&config.database_path()).await?;
    let mailer = Mailer::new(db, mailer::smtp(&config)?, config.from.clone());
    eprintln!(
        "Sending mail through {}:{}",
        config.smtp_host, config.smtp_port
    );
    mailer.run().await
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
        invite_links: config.invite_links.clone(),
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
