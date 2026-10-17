use clap::Parser;
use std::{net::SocketAddr, path::Path};

use concierge::{
    admin::{self, Command},
    app::{self, App},
    assets::Assets,
    config::{Config, Databases, MailerConfig},
    db::{self, Db},
    mailer::{self, Mailer},
    oauth::Webchat,
    outbox::Outbox,
    rate_limit::{
        AddressLimiter, LOOKUPS_PER_ADDRESS, Limiter, MailLimiter, OAUTH_REQUESTS_PER_ADDRESS,
        VERIFICATIONS_PER_ADDRESS,
    },
    session,
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Command::parse() {
        Command::Migrate => migrate(&Databases::from_env()?).await,
        Command::Serve => serve(Config::from_env()?).await,
        Command::Mailer { check: true } => mailer::check(&MailerConfig::from_env()?)
            .await
            .map(|output| print!("{output}")),
        Command::Mailer { check: false } => send_mail(MailerConfig::from_env()?).await,
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

async fn send_mail(config: MailerConfig) -> anyhow::Result<()> {
    let outbox = Outbox::open(&config.outbox_path).await?;
    let mailer = Mailer::new(outbox, mailer::smtp(&config)?, config.from.clone());
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
