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
pub mod lines;
pub mod plan;

use std::time::Duration;

use concierge::{
    db::{self, Arrival, Db, Milestone},
    name::MemberName,
    origin::SiteOrigin,
};
use papo::Client;

use crate::{
    irc::{Connection, Irc},
    plan::Step,
};

/// How often the greeter looks for steps due.
const TICK: Duration = Duration::from_secs(5);

/// Greets newcomers over `client`, for as long as the process runs,
/// skipping ticks while `connection` is down. Its lines link to
/// `public_url`, the members' site.
pub async fn run(
    db: Db,
    client: Client,
    connection: Connection,
    public_url: SiteOrigin,
) -> anyhow::Error {
    let greeter = Greeter {
        db,
        irc: client,
        public_url,
    };
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
    public_url: SiteOrigin,
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
        let newcomer = &arrival.account;
        let (to, said, milestone) = match step {
            Step::Arrive => {
                self.db.record(newcomer, Milestone::Arrived, now).await?;
                return Ok(());
            }
            Step::Welcome => {
                let line = lines::welcome(newcomer, arrival.inviter.as_ref());
                let msgid = self.irc.say(irc::LOUNGE, &line).await?;
                let announced = Milestone::Announced(msgid.as_deref());
                self.db.record(newcomer, announced, now).await?;
                return Ok(());
            }
            Step::TellInviter => (
                arrival.inviter.as_ref(),
                vec![lines::inviter_told(newcomer)],
                Milestone::InviterTold,
            ),
            Step::Orient => (
                Some(newcomer),
                lines::orientation(newcomer, &self.public_url).to_vec(),
                Milestone::Oriented,
            ),
            Step::Nudge => (
                arrival.inviter.as_ref(),
                vec![lines::nudge(newcomer)],
                Milestone::Nudged,
            ),
        };
        let Some(to) = to else {
            return Ok(());
        };
        for line in &said {
            if !self.tell(to, line).await? {
                return Ok(());
            }
        }
        self.db.record(newcomer, milestone, now).await?;
        Ok(())
    }

    /// Sends `nick` a direct message, answering whether it went out. A
    /// nick nobody holds, such as a member's with always-on off who isn't
    /// connected, is tried again on later ticks until the plan gives up.
    async fn tell(&self, nick: &MemberName, text: &str) -> anyhow::Result<bool> {
        match self.irc.say(nick.as_str(), text).await {
            Ok(_) => Ok(true),
            Err(error) if irc::refused_as_absent(&error) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests;
