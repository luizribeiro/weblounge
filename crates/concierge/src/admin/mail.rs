use clap::Subcommand;

use super::Admin;
use crate::{
    db,
    email::Email,
    outbox::{Message, Subject},
};

const TEST_SUBJECT: &str = "A test message from weblounge.org";
const TEST_BODY: &str = "An admin of weblounge.org sent this to check that the \
    network's mail goes out. There's nothing to do.\n";

#[derive(Subcommand)]
pub enum MailCommand {
    /// Queue a test message to an address, and print the id the mailer
    /// logs it by.
    Test { address: String },
}

pub(super) async fn run(command: MailCommand, admin: &Admin) -> anyhow::Result<String> {
    match command {
        MailCommand::Test { address } => test(admin, &address).await,
    }
}

async fn test(admin: &Admin, address: &str) -> anyhow::Result<String> {
    let message = Message {
        account: None,
        to: Email::parse(address)?,
        subject: Subject::parse(TEST_SUBJECT)?,
        body: TEST_BODY.into(),
    };
    let id = admin.outbox.queue(&message, db::now()).await?;
    Ok(format!("queued message {id}\n"))
}
