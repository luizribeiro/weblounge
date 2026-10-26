//! The greeter's ticks, against a real database and a fake IRC.

use std::{collections::HashSet, sync::Mutex};

use concierge::{
    db::{Db, NewInvite, NewMember},
    email::Email,
    invite_rules,
    name::MemberName,
};

use super::*;

const JOINED: i64 = 1_760_000_000;

/// Ergo as the greeter sees it: who has a session attached, and what the
/// greeter said where.
#[derive(Default)]
struct FakeIrc {
    attached: Mutex<HashSet<String>>,
    asked: Mutex<Vec<String>>,
    said: Mutex<Vec<(String, String)>>,
    /// Nicks nobody holds, whom messages can't reach.
    absent: Mutex<HashSet<String>>,
    /// Whether sending times out, as it does while Ergo is down.
    timing_out: Mutex<bool>,
}

impl FakeIrc {
    fn attach(&self, nick: &str) {
        self.attached.lock().unwrap().insert(nick.to_owned());
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }

    /// What the greeter said, as (target, text), oldest first.
    fn said(&self) -> Vec<(String, String)> {
        self.said.lock().unwrap().clone()
    }

    fn time_out(&self, timing_out: bool) {
        *self.timing_out.lock().unwrap() = timing_out;
    }

    fn set_absent(&self, nick: &str, absent: bool) {
        let mut nicks = self.absent.lock().unwrap();
        if absent {
            nicks.insert(nick.to_owned());
        } else {
            nicks.remove(nick);
        }
    }
}

impl Irc for FakeIrc {
    async fn attached(&self, nick: &str) -> Result<bool, papo::Error> {
        self.asked.lock().unwrap().push(nick.to_owned());
        Ok(self.attached.lock().unwrap().contains(nick))
    }

    async fn say(&self, target: &str, text: &str) -> Result<Option<String>, papo::Error> {
        if *self.timing_out.lock().unwrap() {
            return Err(papo::Error::Timeout);
        }
        if self.absent.lock().unwrap().contains(target) {
            let params = vec!["concierge".into(), target.into(), "No such nick".into()];
            return Err(papo::Error::Server(papo::ServerError::from_numeric(
                401, params,
            )));
        }
        let mut said = self.said.lock().unwrap();
        said.push((target.to_owned(), text.to_owned()));
        Ok(Some(format!("msg-{}", said.len())))
    }
}

/// A line said in `target`.
fn line(target: &str, text: &str) -> (String, String) {
    (target.to_owned(), text.to_owned())
}

struct Fixture {
    greeter: Greeter<FakeIrc>,
    _dir: tempfile::TempDir,
}

impl Fixture {
    /// luiz, a member since before arrivals were recorded.
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::migrate(&dir.path().join("concierge.db")).await.unwrap();
        let luiz = NewMember {
            account: &MemberName::new("luiz"),
            email: &email("luiz"),
            news_opt_in: false,
            invited_by: None,
            joined_at: 0,
            is_admin: true,
        };
        db.insert_member(&luiz).await.unwrap();
        Self {
            greeter: Greeter {
                db,
                irc: FakeIrc::default(),
            },
            _dir: dir,
        }
    }

    fn db(&self) -> &Db {
        &self.greeter.db
    }

    fn irc(&self) -> &FakeIrc {
        &self.greeter.irc
    }

    /// `account` signs up at `joined_at` with an invite from `inviter`, or
    /// a root invite, as the invite page redeems it.
    async fn join(&self, account: &str, inviter: Option<&str>, joined_at: i64) {
        let inviter = inviter.map(MemberName::new);
        let invite = NewInvite {
            code: account,
            created_by: inviter.as_ref(),
            created_at: joined_at,
            expires_at: invite_rules::expires_at(joined_at),
            makes_admin: false,
        };
        self.db().insert_invite(&invite).await.unwrap();
        let claim = self.db().claim_invite(account, joined_at).await.unwrap();
        let member = NewMember {
            account: &MemberName::new(account),
            email: &email(account),
            news_opt_in: false,
            invited_by: inviter.as_ref(),
            joined_at,
            is_admin: false,
        };
        self.db()
            .redeem_invite(&claim.unwrap(), &member)
            .await
            .unwrap();
    }

    async fn tick(&self, now: i64) {
        self.greeter.tick(now).await.unwrap();
    }

    /// The greeter after a restart: the same database, a new connection.
    fn restart(self) -> Self {
        Self {
            greeter: Greeter {
                db: self.greeter.db,
                irc: FakeIrc::default(),
            },
            _dir: self._dir,
        }
    }

    async fn arrival(&self, account: &str) -> Arrival {
        let arrivals = self.db().arrivals().await.unwrap();
        let account = MemberName::new(account);
        arrivals.into_iter().find(|a| a.account == account).unwrap()
    }
}

fn email(account: &str) -> Email {
    Email::parse(&format!("{account}@example.org")).unwrap()
}

#[tokio::test]
async fn notes_when_a_newcomer_is_first_seen_and_stops_asking() {
    let fixture = Fixture::start().await;
    fixture.join("erin", Some("luiz"), JOINED).await;

    fixture.tick(JOINED + 5).await;
    assert_eq!(fixture.arrival("erin").await.arrived_at, None);
    fixture.irc().attach("erin");
    fixture.tick(JOINED + 10).await;
    fixture.tick(JOINED + 15).await;

    assert_eq!(fixture.arrival("erin").await.arrived_at, Some(JOINED + 10));
    assert_eq!(fixture.irc().asked(), ["erin", "erin"]);
}

#[tokio::test]
async fn asks_about_every_newcomer_not_seen_yet_and_nobody_else() {
    let fixture = Fixture::start().await;
    fixture.join("erin", Some("luiz"), JOINED).await;
    fixture.join("frank", None, JOINED + 1).await;

    fixture.tick(JOINED + 5).await;

    assert_eq!(fixture.irc().asked(), ["erin", "frank"]);
}

const TOLD_LUIZ: &str =
    "erin accepted your invite and was welcomed in #lounge. Say hi when you get a chance.";

#[tokio::test]
async fn welcomes_a_newcomer_at_first_sight_then_tells_their_inviter() {
    let fixture = Fixture::start().await;
    fixture.join("erin", Some("luiz"), JOINED).await;
    fixture.irc().attach("erin");

    fixture.tick(JOINED + 5).await;
    fixture.tick(JOINED + 10).await;

    assert_eq!(
        fixture.irc().said(),
        [
            line("#lounge", "Welcome erin, invited by luiz!"),
            line("luiz", TOLD_LUIZ),
        ]
    );
    let arrival = fixture.arrival("erin").await;
    assert_eq!(arrival.announced_at, Some(JOINED + 5));
    assert_eq!(arrival.inviter_told_at, Some(JOINED + 5));
}

#[tokio::test]
async fn names_and_tells_no_inviter_removed_since() {
    let fixture = Fixture::start().await;
    fixture.join("erin", Some("luiz"), JOINED).await;
    let luiz = MemberName::new("luiz");
    fixture.db().remove_member(&luiz, JOINED + 1).await.unwrap();
    fixture.irc().attach("erin");

    fixture.tick(JOINED + 5).await;

    assert_eq!(fixture.irc().said(), [line("#lounge", "Welcome erin!")]);
}

#[tokio::test]
async fn tells_an_inviter_who_was_unreachable_once_they_are_back() {
    let fixture = Fixture::start().await;
    fixture.join("erin", Some("luiz"), JOINED).await;
    fixture.irc().attach("erin");
    fixture.irc().set_absent("luiz", true);

    fixture.tick(JOINED + 5).await;
    assert_eq!(fixture.irc().said().len(), 1);
    assert_eq!(fixture.arrival("erin").await.inviter_told_at, None);
    fixture.irc().set_absent("luiz", false);
    fixture.tick(JOINED + 60).await;

    assert_eq!(fixture.irc().said()[1], line("luiz", TOLD_LUIZ));
    assert_eq!(
        fixture.arrival("erin").await.inviter_told_at,
        Some(JOINED + 60)
    );
}

#[tokio::test]
async fn welcomes_a_root_invitee_ten_minutes_in_even_unseen() {
    let fixture = Fixture::start().await;
    fixture.join("frank", None, JOINED).await;

    fixture.tick(JOINED + 599).await;
    assert_eq!(fixture.irc().said(), []);
    fixture.tick(JOINED + 600).await;

    assert_eq!(fixture.irc().said(), [line("#lounge", "Welcome frank!")]);
}

#[tokio::test]
async fn skips_the_welcome_of_an_arrival_two_days_old() {
    let fixture = Fixture::start().await;
    fixture.join("erin", Some("luiz"), JOINED).await;
    fixture.irc().attach("erin");

    fixture.tick(JOINED + 2 * 24 * 60 * 60).await;

    assert_eq!(fixture.irc().said(), []);
    assert_eq!(fixture.arrival("erin").await.announced_at, None);
}

#[tokio::test]
async fn a_welcome_that_didnt_go_out_goes_out_on_a_later_tick() {
    let fixture = Fixture::start().await;
    fixture.join("erin", Some("luiz"), JOINED).await;
    fixture.irc().attach("erin");
    fixture.irc().time_out(true);

    assert!(fixture.greeter.tick(JOINED + 5).await.is_err());
    assert_eq!(fixture.arrival("erin").await.announced_at, None);
    fixture.irc().time_out(false);
    fixture.tick(JOINED + 10).await;

    assert_eq!(fixture.irc().said().len(), 2);
    assert_eq!(
        fixture.arrival("erin").await.announced_at,
        Some(JOINED + 10)
    );
}

#[tokio::test]
async fn a_restart_repeats_nothing() {
    let fixture = Fixture::start().await;
    fixture.join("erin", Some("luiz"), JOINED).await;
    fixture.irc().attach("erin");
    fixture.tick(JOINED + 5).await;

    let fixture = fixture.restart();
    fixture.tick(JOINED + 700).await;

    assert_eq!(fixture.irc().said(), []);
    assert_eq!(fixture.irc().asked(), Vec::<String>::new());
}
