use std::path::PathBuf;

use clap::Subcommand;

use super::{Admin, read_password};
use crate::house::{self, Ensured};

#[derive(Subcommand)]
pub enum GreeterCommand {
    /// Create concierge, the house account the greeter connects as, with
    /// the password in a file, or set its password to the file's, and give
    /// it its vhost and always-on. Meant to run before the greeter starts,
    /// every time.
    EnsureAccount {
        /// The file holding the password, or - for standard input.
        #[arg(long)]
        password_file: PathBuf,
    },
}

pub(super) async fn run(command: GreeterCommand, admin: &Admin) -> anyhow::Result<String> {
    match command {
        GreeterCommand::EnsureAccount { password_file } => {
            let password = read_password(&password_file)?;
            let done = match house::ensure(&admin.db, &admin.ergo, &password).await? {
                Ensured::Created => "created",
                Ensured::Updated => "updated",
            };
            Ok(format!("{done} {}\n", house::ACCOUNT))
        }
    }
}
