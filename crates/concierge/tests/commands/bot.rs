use concierge::db::{self, Bot, NewBot};
use ergo_api::{Preference, fake::Failure};

use crate::Fixture;

impl Fixture {
    /// luiz, a member, and alice, who was removed.
    async fn with_owners() -> Self {
        let fixture = Self::start().await;
        fixture.add_member("luiz", None).await;
        fixture.add_member("alice", Some("luiz")).await;
        fixture.run(&["member", "delete", "alice"]).await.unwrap();
        fixture
    }

    async fn bot(&self, name: &str) -> Option<Bot> {
        self.db().await.bot(name).await.unwrap()
    }

    async fn in_ergo(&self, name: &str) -> bool {
        self.ergo.client().info(name).await.unwrap().is_some()
    }

    /// A bot of luiz's that was deleted and that Ergo has since erased.
    async fn insert_deleted_bot(&self, name: &str) {
        let db = self.db().await;
        let bot = NewBot {
            account: name,
            owner: "luiz",
            note: None,
            created_at: 1_760_000_000,
        };
        db.insert_bot(&bot).await.unwrap();
        assert!(db.remove_bot(name, 1_760_100_000).await.unwrap());
    }
}

/// The password a command printed, alone on its line.
fn printed_password(output: &str) -> &str {
    let password = output.strip_suffix('\n').unwrap();
    assert_eq!(password.len(), 26, "{output:?}");
    password
}

mod create {
    use super::*;

    #[tokio::test]
    async fn registers_the_account_records_its_owner_and_prints_the_password_once() {
        let fixture = Fixture::with_owners().await;

        let output = fixture
            .run(&["bot", "create", "LUIZ", "Bender", "--note", "answers !ping"])
            .await
            .unwrap();

        let password = printed_password(&output);
        assert!(fixture.signs_in_to_ergo("bender", password).await);
        let bot = fixture.bot("bender").await.unwrap();
        assert_eq!(
            (
                bot.account.as_str(),
                bot.owner.as_str(),
                bot.note.as_deref()
            ),
            ("Bender", "luiz", Some("answers !ping"))
        );
        assert!(bot.created_at.abs_diff(db::now()) < 60);
        assert_eq!(bot.removed_at, None);
    }

    #[tokio::test]
    async fn turns_always_on_and_auto_away_off() {
        let fixture = Fixture::with_owners().await;
        fixture
            .run(&["bot", "create", "luiz", "bender"])
            .await
            .unwrap();
        let settings = fixture.ergo.client().settings("bender").await.unwrap();
        assert_eq!(
            (settings.always_on, settings.auto_away),
            (Preference::Off, Preference::Off)
        );
        assert_eq!(fixture.bot("bender").await.unwrap().note, None);
    }

    #[tokio::test]
    async fn needs_a_current_member_as_owner() {
        let fixture = Fixture::with_owners().await;
        for (owner, message) in [
            ("nobody", "nobody is not a member"),
            ("alice", "alice was removed"),
        ] {
            let error = fixture
                .run(&["bot", "create", owner, "bender"])
                .await
                .unwrap_err();
            assert_eq!(error.to_string(), message);
        }
        assert!(!fixture.in_ergo("bender").await);
        assert_eq!(fixture.bot("bender").await, None);
    }

    #[tokio::test]
    async fn needs_a_name_members_could_choose_as_their_nick() {
        let fixture = Fixture::with_owners().await;
        for (name, message) in [
            (
                "2pac",
                "2pac can't be a bot's name: Nicks start with a letter.",
            ),
            (
                "chanserv",
                "chanserv can't be a bot's name: That nick is reserved for the network's services.",
            ),
        ] {
            let error = fixture
                .run(&["bot", "create", "luiz", name])
                .await
                .unwrap_err();
            assert_eq!(format!("{error:#}"), message);
            assert_eq!(fixture.bot(name).await, None);
        }
    }

    #[tokio::test]
    async fn refuses_names_members_and_bots_hold_even_once_ergo_forgot_them() {
        let fixture = Fixture::with_owners().await;
        fixture.insert_member("Gone", Some("luiz")).await;
        fixture.insert_deleted_bot("dalek").await;
        fixture.register("Orphan").await;

        for name in ["LUIZ", "alice", "gone", "Dalek", "orphan"] {
            let error = fixture
                .run(&["bot", "create", "luiz", name])
                .await
                .unwrap_err();
            assert_eq!(error.to_string(), format!("{name} is taken"));
        }
        assert!(!fixture.in_ergo("gone").await);
        assert!(!fixture.in_ergo("dalek").await);
        assert_eq!(fixture.bot("orphan").await, None);
        assert_eq!(fixture.db().await.bots(None).await.unwrap(), []);
    }

    #[tokio::test]
    async fn records_nothing_when_ergo_fails() {
        let fixture = Fixture::with_owners().await;
        fixture.ergo.fail_next(Failure::UnknownError);

        let error = fixture
            .run(&["bot", "create", "luiz", "bender"])
            .await
            .unwrap_err();

        assert_eq!(
            format!("{error:#}"),
            "Ergo didn't create bender: \
             Ergo refused the request: UNKNOWN_ERROR (simulated failure)"
        );
        assert_eq!(fixture.bot("bender").await, None);
    }

    #[tokio::test]
    async fn says_so_when_ergo_keeps_the_bot_online() {
        let fixture = Fixture::with_owners().await;
        fixture.ergo.pass_next();
        fixture.ergo.fail_next(Failure::UnknownError);

        let error = fixture
            .run(&["bot", "create", "luiz", "bender"])
            .await
            .unwrap_err();

        assert_eq!(
            format!("{error:#}"),
            "bender was created, but not set up; bot reset-password bender tries again: \
             Ergo didn't turn bender's always-on and auto-away off: \
             Ergo refused the request: UNKNOWN_ERROR (simulated failure)"
        );
        assert!(fixture.bot("bender").await.is_some());
    }
}
