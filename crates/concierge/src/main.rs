use clap::Parser;
use concierge::{app, assets::Assets, config::Config, db::Db, status};
use tokio::net::TcpListener;

/// The members' site of weblounge.org, and the admin commands behind it.
#[derive(Parser)]
enum Command {
    /// Serve the site on CONCIERGE_LISTEN.
    Serve,
    /// Show the configuration, state, secrets, database and Ergo this process sees.
    Status,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let command = Command::parse();
    let config = Config::from_env()?;
    match command {
        Command::Serve => serve(config).await,
        Command::Status => status::report(&config)
            .await
            .map(|report| print!("{report}")),
    }
}

async fn serve(config: Config) -> anyhow::Result<()> {
    config.read_session_key()?;
    config.ergo_client()?;
    Db::open(&config.database_path()).await?;
    let assets = Assets::load(&config.assets_dir)?;
    let listener = TcpListener::bind(config.listen).await?;
    eprintln!("Serving {} on {}", config.public_url, config.listen);
    axum::serve(listener, app::router(assets)).await?;
    Ok(())
}
