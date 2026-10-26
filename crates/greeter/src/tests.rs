//! The greeter's ticks, against a real database and a fake IRC.

use std::{collections::HashSet, sync::Mutex};

use concierge::{
    db::{Db, NewInvite, NewMember},
    email::Email,
    invite_rules,
    name::MemberName,
    origin::SiteOrigin,
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
    /// Lines that time out, as if Ergo went down as they were sent.
    lost: Mutex<HashSet<String>>,
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

    /// What the greeter said in `target`, oldest first.
    fn said_to(&self, target: &str) -> Vec<String> {
        let said = self.said.lock().unwrap();
        said.iter()
            .filter(|(to, _)| to == target)
            .map(|(_, text)| text.clone())
            .collect()
    }

    fn time_out(&self, timing_out: bool) {
        *self.timing_out.lock().unwrap() = timing_out;
    }

    fn lose(&self, text: &str) {
        self.lost.lock().unwrap().insert(text.to_owned());
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
        if *self.timing_out.lock().unwrap() || self.lost.lock().unwrap().contains(text) {
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
                public_url: public_url(),
                weekly_host: Some(MemberName::new("luiz")),
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

    /// `account` signs up at `joined_at` with an invite from `inviter`, as
    /// the invite page redeems it.
    async fn join(&self, account: &str, inviter: &str, joined_at: i64) {
        let inviter = Some(MemberName::new(inviter));
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
                irc: FakeIrc::default(),
                ..self.greeter
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
    fixture.join("erin", "luiz", JOINED).await;

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
    fixture.join("erin", "luiz", JOINED).await;
    fixture.join("frank", "luiz", JOINED + 1).await;

    fixture.tick(JOINED + 5).await;

    assert_eq!(fixture.irc().asked(), ["erin", "frank"]);
}

const TOLD_LUIZ: &str =
    "erin just connected for the first time and was welcomed in #lounge. Say hi!";

#[tokio::test]
async fn welcomes_a_newcomer_at_first_sight_then_tells_their_inviter() {
    let fixture = Fixture::start().await;
    fixture.join("erin", "luiz", JOINED).await;
    fixture.irc().attach("erin");

    fixture.tick(JOINED + 5).await;
    fixture.tick(JOINED + 10).await;

    let welcome = line("#lounge", "Welcome erin, invited by luiz!");
    let hello = hello("erin").into_iter().map(|text| line("erin", &text));
    let said: Vec<_> = [welcome, line("luiz", TOLD_LUIZ)]
        .into_iter()
        .chain(hello)
        .collect();
    assert_eq!(fixture.irc().said(), said);
    let arrival = fixture.arrival("erin").await;
    assert_eq!(arrival.announced_at, Some(JOINED + 5));
    assert_eq!(arrival.inviter_told_at, Some(JOINED + 5));
    assert_eq!(arrival.oriented_at, Some(JOINED + 5));
}

fn public_url() -> SiteOrigin {
    SiteOrigin::parse("https://members.weblounge.org").unwrap()
}

/// The hello to `newcomer`, invited by luiz.
fn hello(newcomer: &str) -> Vec<String> {
    let inviter = MemberName::new("luiz");
    lines::hello(&MemberName::new(newcomer), &inviter, &public_url())
}

#[tokio::test]
async fn a_hello_that_failed_goes_out_once_the_newcomer_can_get_it() {
    let fixture = Fixture::start().await;
    fixture.join("erin", "luiz", JOINED).await;
    fixture.irc().attach("erin");
    fixture.irc().set_absent("erin", true);
    fixture.tick(JOINED + 5).await;
    assert_eq!(fixture.arrival("erin").await.oriented_at, None);

    fixture.irc().set_absent("erin", false);
    fixture.tick(JOINED + 10).await;

    assert_eq!(fixture.irc().said_to("erin"), hello("erin"));
    assert_eq!(fixture.arrival("erin").await.oriented_at, Some(JOINED + 10));
}

#[tokio::test]
async fn greets_nobody_an_inviter_removed_since_brought() {
    let fixture = Fixture::start().await;
    fixture.join("erin", "luiz", JOINED).await;
    let luiz = MemberName::new("luiz");
    fixture.db().remove_member(&luiz, JOINED + 1).await.unwrap();
    fixture.irc().attach("erin");

    fixture.tick(JOINED + 5).await;

    assert_eq!(fixture.irc().said(), []);
    assert_eq!(fixture.irc().asked(), Vec::<String>::new());
}

#[tokio::test]
async fn tells_an_inviter_who_was_unreachable_once_they_are_back() {
    let fixture = Fixture::start().await;
    fixture.join("erin", "luiz", JOINED).await;
    fixture.irc().attach("erin");
    fixture.irc().set_absent("luiz", true);

    fixture.tick(JOINED + 5).await;
    assert_eq!(fixture.irc().said_to("erin"), hello("erin"));
    assert_eq!(fixture.arrival("erin").await.inviter_told_at, None);
    fixture.irc().set_absent("luiz", false);
    fixture.tick(JOINED + 60).await;

    assert_eq!(fixture.irc().said_to("luiz"), [TOLD_LUIZ]);
    assert_eq!(
        fixture.arrival("erin").await.inviter_told_at,
        Some(JOINED + 60)
    );
}

const TWO_DAYS: i64 = 2 * 24 * 60 * 60;
const NUDGE: &str =
    "erin hasn't connected since accepting your invite. A nudge from you might help.";

#[tokio::test]
async fn says_nothing_while_the_newcomer_stays_away_but_the_nudge() {
    let fixture = Fixture::start().await;
    fixture.join("erin", "luiz", JOINED).await;

    fixture.tick(JOINED + 600).await;
    fixture.tick(JOINED + TWO_DAYS - 1).await;
    assert_eq!(fixture.irc().said(), []);
    fixture.tick(JOINED + TWO_DAYS).await;
    fixture.tick(JOINED + TWO_DAYS + 5).await;

    assert_eq!(fixture.irc().said(), [line("luiz", NUDGE)]);
    let nudged_at = fixture.arrival("erin").await.nudged_at;
    assert_eq!(nudged_at, Some(JOINED + TWO_DAYS));
}

#[tokio::test]
async fn welcomes_a_newcomer_who_first_connects_after_the_nudge() {
    let fixture = Fixture::start().await;
    fixture.join("erin", "luiz", JOINED).await;
    fixture.tick(JOINED + TWO_DAYS).await;

    fixture.irc().attach("erin");
    fixture.tick(JOINED + 5 * TWO_DAYS).await;

    assert_eq!(fixture.irc().said_to("luiz"), [NUDGE, TOLD_LUIZ]);
    assert_eq!(
        fixture.irc().said_to("#lounge"),
        ["Welcome erin, invited by luiz!"]
    );
    assert_eq!(fixture.irc().said_to("erin"), hello("erin"));
}

#[tokio::test]
async fn nudges_nobody_once_the_newcomer_came() {
    let fixture = Fixture::start().await;
    fixture.join("erin", "luiz", JOINED).await;
    fixture.irc().attach("erin");
    fixture.tick(JOINED + 5).await;

    fixture.tick(JOINED + TWO_DAYS).await;

    assert_eq!(fixture.irc().said_to("luiz"), [TOLD_LUIZ]);
}

#[tokio::test]
async fn a_welcome_that_didnt_go_out_goes_out_on_a_later_tick() {
    let fixture = Fixture::start().await;
    fixture.join("erin", "luiz", JOINED).await;
    fixture.irc().attach("erin");
    fixture.irc().time_out(true);

    assert!(fixture.greeter.tick(JOINED + 5).await.is_err());
    assert_eq!(fixture.arrival("erin").await.announced_at, None);
    fixture.irc().time_out(false);
    fixture.tick(JOINED + 10).await;

    assert_eq!(fixture.irc().said().len(), 6);
    assert_eq!(
        fixture.arrival("erin").await.announced_at,
        Some(JOINED + 10)
    );
}

#[tokio::test]
async fn forgets_a_newcomer_two_weeks_after_they_joined() {
    let fixture = Fixture::start().await;
    fixture.join("erin", "luiz", JOINED).await;
    fixture.join("frank", "luiz", JOINED + 1).await;
    let two_weeks = 14 * 24 * 60 * 60;

    fixture.tick(JOINED + two_weeks).await;

    let arrivals = fixture.db().arrivals().await.unwrap();
    let left: Vec<_> = arrivals.iter().map(|a| a.account.as_str()).collect();
    assert_eq!(left, ["frank"]);
    assert_eq!(fixture.irc().asked(), ["frank"]);
}

/// Monday 2026-10-26, 10:00 in California.
const MONDAY_AT_TEN: i64 = 1_793_034_000;

#[tokio::test]
async fn reminds_luiz_once_on_monday_morning_to_post_the_weekly_thread() {
    let fixture = Fixture::start().await;

    fixture.tick(MONDAY_AT_TEN - 5).await;
    fixture.tick(MONDAY_AT_TEN).await;
    let fixture = fixture.restart();
    fixture.tick(MONDAY_AT_TEN + 60 * 60).await;

    assert_eq!(fixture.irc().said(), []);
    assert!(
        fixture
            .db()
            .weekly_reminder_sent("2026-10-26")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn the_reminder_carries_a_prompt() {
    let fixture = Fixture::start().await;

    fixture.tick(MONDAY_AT_TEN).await;

    let [reminder] = &fixture.irc().said_to("luiz")[..] else {
        panic!("one reminder: {:?}", fixture.irc().said());
    };
    assert_eq!(reminder, &weekly::reminder("2026-10-26"));
}

#[tokio::test]
async fn a_reminder_luiz_cant_get_is_tried_until_he_can() {
    let fixture = Fixture::start().await;
    fixture.irc().set_absent("luiz", true);
    fixture.tick(MONDAY_AT_TEN).await;
    assert!(
        !fixture
            .db()
            .weekly_reminder_sent("2026-10-26")
            .await
            .unwrap()
    );

    fixture.irc().set_absent("luiz", false);
    fixture.tick(MONDAY_AT_TEN + 60).await;

    assert_eq!(fixture.irc().said_to("luiz").len(), 1);
}

#[tokio::test]
async fn reminds_nobody_without_a_weekly_host() {
    let mut fixture = Fixture::start().await;
    fixture.greeter.weekly_host = None;

    fixture.tick(MONDAY_AT_TEN).await;

    assert_eq!(fixture.irc().said(), []);
}

#[tokio::test]
async fn a_hello_cut_short_is_never_repeated() {
    let fixture = Fixture::start().await;
    fixture.join("frank", "luiz", JOINED).await;
    fixture.irc().attach("frank");
    let hello = hello("frank");
    fixture.irc().lose(&hello[1]);

    fixture.tick(JOINED + 5).await;
    fixture.tick(JOINED + 10).await;

    assert_eq!(fixture.irc().said_to("frank"), hello[..1]);
    assert_eq!(fixture.arrival("frank").await.oriented_at, Some(JOINED + 5));
}

#[tokio::test]
async fn a_restart_repeats_nothing() {
    let fixture = Fixture::start().await;
    fixture.join("erin", "luiz", JOINED).await;
    fixture.irc().attach("erin");
    fixture.tick(JOINED + 5).await;

    let fixture = fixture.restart();
    fixture.tick(JOINED + 700).await;

    assert_eq!(fixture.irc().said(), []);
    assert_eq!(fixture.irc().asked(), Vec::<String>::new());
}
