use anyhow::Context;
use concierge::{config::GreeterConfig, db::Db};
use greeter::irc::{self, Connection};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    common::make_undumpable().context("can't keep the greeter's memory out of core dumps")?;
    let config = GreeterConfig::from_env()?;
    let db = Db::open(&config.database).await?;
    let (client, events) = irc::connect(&config.irc_server, &config.read_password()?).await?;
    let connection = Connection::default();
    Err(tokio::select! {
        error = irc::watch(events, connection.clone()) => error,
        error = greeter::run(config, db, client, connection) => error,
    })
}
