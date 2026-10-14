use clap::Parser;
use concierge::{app, config::Config, status};
use tokio::net::TcpListener;

/// The members' site of weblounge.org, and the admin commands behind it.
#[derive(Parser)]
enum Command {
    /// Serve the site on CONCIERGE_LISTEN.
    Serve,
    /// Show the configuration, state directory and secrets this process sees.
    Status,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let command = Command::parse();
    let config = Config::from_env()?;
    match command {
        Command::Serve => serve(config).await,
        Command::Status => status::report(&config).map(|report| print!("{report}")),
    }
}

async fn serve(config: Config) -> anyhow::Result<()> {
    config.read_session_key()?;
    let listener = TcpListener::bind(config.listen).await?;
    eprintln!("Serving {} on {}", config.public_url, config.listen);
    axum::serve(listener, app::router()).await?;
    Ok(())
}
