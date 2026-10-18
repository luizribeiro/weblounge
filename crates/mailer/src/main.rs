use anyhow::Context;
use clap::Parser;
use mailer::{
    config::{self, MailerConfig},
    send::{self, Mailer},
};
use outbox::Outbox;
use std::path::Path;

/// Send the mail concierge queues in CONCIERGE_OUTBOX, through
/// CONCIERGE_SMTP_HOST.
#[derive(Parser)]
struct Args {
    /// Log in to the SMTP server and leave, sending nothing.
    #[arg(long)]
    check: bool,
    /// Withdraw the published key and delete the mail sealed to it, as
    /// the mailer stops.
    #[arg(long, conflicts_with = "check")]
    withdraw_key: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    common::make_undumpable().context("can't keep the mailer's memory out of core dumps")?;
    let args = Args::parse();
    if args.withdraw_key {
        return withdraw_key(&config::outbox_path_from_env()?).await;
    }
    let config = MailerConfig::from_env()?;
    if args.check {
        print!("{}", send::check(&config).await?);
        return Ok(());
    }
    send_mail(config).await
}

async fn withdraw_key(outbox_path: &Path) -> anyhow::Result<()> {
    let outbox = Outbox::open(outbox_path).await?;
    let deletion = outbox.withdraw_key().await?;
    eprintln!(
        "Withdrew the mailer's key; deleted {} messages sealed to it, unsent",
        deletion.messages
    );
    Ok(())
}

async fn send_mail(config: MailerConfig) -> anyhow::Result<()> {
    let outbox = Outbox::open(&config.outbox_path).await?;
    let mailer = Mailer::new(outbox, send::smtp(&config)?, config.from.clone());
    mailer.publish_key().await?;
    eprintln!(
        "Sending mail through {}:{}",
        config.smtp_host, config.smtp_port
    );
    mailer.run().await
}
