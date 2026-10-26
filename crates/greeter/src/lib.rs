//! The greeter: concierge's house account on IRC, which welcomes newcomers
//! in #lounge and tells their inviters. It parses what the network sends
//! it, so it holds as little as it can: concierge's database, and the
//! house account's password, never Ergo's API token.
//!
//! Every few seconds it reads the arrivals on record, asks Ergo what it
//! needs to know, and takes the steps [`plan`] says are due, recording
//! each once it's done. Nothing lives in memory, so a restart picks up
//! where the last run left off.

pub mod irc;
pub mod plan;

use std::time::Duration;

use concierge::db::{self, Arrival, Db, Milestone};
use papo::Client;

use crate::{
    irc::{Connection, Irc},
    plan::Step,
};

/// How often the greeter looks for steps due.
const TICK: Duration = Duration::from_secs(5);

/// Greets newcomers over `client`, for as long as the process runs,
/// skipping ticks while `connection` is down.
pub async fn run(db: Db, client: Client, connection: Connection) -> anyhow::Error {
    let greeter = Greeter { db, irc: client };
    let mut ticks = tokio::time::interval(TICK);
    loop {
        ticks.tick().await;
        if !connection.is_up() {
            continue;
        }
        if let Err(error) = greeter.tick(db::now()).await {
            eprintln!("Greeting failed: {error:#}");
        }
    }
}

struct Greeter<I> {
    db: Db,
    irc: I,
}

impl<I: Irc> Greeter<I> {
    async fn tick(&self, now: i64) -> anyhow::Result<()> {
        for arrival in self.db.arrivals().await? {
            let attached = plan::needs_presence(&arrival)
                && self.irc.attached(arrival.account.as_str()).await?;
            for step in plan::due(&arrival, attached, now) {
                self.take(step, &arrival, now).await?;
            }
        }
        Ok(())
    }

    async fn take(&self, step: Step, arrival: &Arrival, now: i64) -> anyhow::Result<()> {
        match step {
            Step::Arrive => {
                self.db
                    .record(&arrival.account, Milestone::Arrived, now)
                    .await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
