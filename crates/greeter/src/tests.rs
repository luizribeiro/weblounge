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

/// Ergo as the greeter sees it: who has a session attached.
#[derive(Default)]
struct FakeIrc {
    attached: Mutex<HashSet<String>>,
    asked: Mutex<Vec<String>>,
}

impl FakeIrc {
    fn attach(&self, nick: &str) {
        self.attached.lock().unwrap().insert(nick.to_owned());
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Irc for FakeIrc {
    async fn attached(&self, nick: &str) -> Result<bool, papo::Error> {
        self.asked.lock().unwrap().push(nick.to_owned());
        Ok(self.attached.lock().unwrap().contains(nick))
    }
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
