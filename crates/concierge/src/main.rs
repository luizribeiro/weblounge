use clap::Parser;
use std::{net::SocketAddr, path::Path};

use concierge::{
    admin::{self, Command},
    app::{self, App},
    assets::Assets,
    config::{Config, Databases, RemindConfig},
    db::{self, Db},
    oauth::Webchat,
    rate_limit::{
        AddressLimiter, LOOKUPS_PER_ADDRESS, Limiter, MailLimiter, OAUTH_REQUESTS_PER_ADDRESS,
        VERIFICATIONS_PER_ADDRESS,
    },
    session, verification_mail,
};
use outbox::Outbox;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Command::parse() {
        Command::Migrate => migrate(&Databases::from_env()?).await,
        Command::Serve => serve(Config::from_env()?).await,
        Command::Remind => remind(&RemindConfig::from_env()?).await,
        Command::Admin(command) => admin::run(command, &Config::from_env()?)
            .await
            .map(|output| print!("{output}")),
    }
}

async fn migrate(databases: &Databases) -> anyhow::Result<()> {
    let db = Db::migrate(&databases.concierge).await?;
    report_migration(&databases.concierge, db.migration_version().await?);
    let outbox = Outbox::migrate(&databases.outbox).await?;
    report_migration(&databases.outbox, outbox.migration_version().await?);
    Ok(())
}

fn report_migration(path: &Path, version: Option<i64>) {
    eprintln!(
        "{} is at migration {}",
        path.display(),
        version.unwrap_or_default()
    );
}

async fn remind(config: &RemindConfig) -> anyhow::Result<()> {
    let db = Db::open(&config.databases.concierge).await?;
    let outbox = Outbox::open(&config.databases.outbox).await?;
    let batch = verification_mail::remind(&db, &outbox, &config.public_url, db::now()).await?;
    let plural = if batch.sent.len() == 1 { "" } else { "s" };
    eprintln!("Queued {} reminder{plural}", batch.sent.len());
    if batch.left > 0 {
        eprintln!("{} more are due; the next run mails them", batch.left);
    }
    Ok(())
}

async fn serve(config: Config) -> anyhow::Result<()> {
    let db = Db::open(&config.database_path()).await?;
    db.end_expired_sessions(db::now()).await?;
    let app = App {
        assets: Assets::load(&config.assets_dir)?,
        db,
        outbox: Outbox::open(&config.outbox_path).await?,
        ergo: config.ergo_client()?,
        sessions: session::Hasher::new(&config.read_session_key()?),
        limiter: Limiter::default(),
        lookups: AddressLimiter::new(LOOKUPS_PER_ADDRESS),
        oauth_requests: AddressLimiter::new(OAUTH_REQUESTS_PER_ADDRESS),
        verifications: AddressLimiter::new(VERIFICATIONS_PER_ADDRESS),
        mailed_links: MailLimiter::default(),
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
