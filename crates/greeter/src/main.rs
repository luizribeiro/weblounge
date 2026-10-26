use anyhow::Context;
use concierge::config::GreeterConfig;
use greeter::irc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    common::make_undumpable().context("can't keep the greeter's memory out of core dumps")?;
    let config = GreeterConfig::from_env()?;
    let (_client, events) = irc::connect(&config.irc_server, &config.read_password()?).await?;
    Err(irc::watch(events).await)
}
