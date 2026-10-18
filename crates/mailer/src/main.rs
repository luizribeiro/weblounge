use clap::Parser;
use mailer::{
    config::MailerConfig,
    send::{self, Mailer},
};
use outbox::Outbox;

/// Send the mail concierge queues in CONCIERGE_OUTBOX, through
/// CONCIERGE_SMTP_HOST.
#[derive(Parser)]
struct Args {
    /// Log in to the SMTP server and leave, sending nothing.
    #[arg(long)]
    check: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = MailerConfig::from_env()?;
    if args.check {
        print!("{}", send::check(&config).await?);
        return Ok(());
    }
    send_mail(config).await
}

async fn send_mail(config: MailerConfig) -> anyhow::Result<()> {
    let outbox = Outbox::open(&config.outbox_path).await?;
    let mailer = Mailer::new(outbox, send::smtp(&config)?, config.from.clone());
    eprintln!(
        "Sending mail through {}:{}",
        config.smtp_host, config.smtp_port
    );
    mailer.run().await
}
