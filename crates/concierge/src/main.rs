use clap::Parser;
use concierge::{
    admin::{self, Command},
    app,
    assets::Assets,
    config::Config,
    db::Db,
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
    config.read_session_key()?;
    config.ergo_client()?;
    Db::open(&config.database_path()).await?;
    let assets = Assets::load(&config.assets_dir)?;
    let listener = TcpListener::bind(config.listen).await?;
    eprintln!("Serving {} on {}", config.public_url, config.listen);
    axum::serve(listener, app::router(assets)).await?;
    Ok(())
}
