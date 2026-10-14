use concierge::db::{self, NewBot, NewInvite};
use ergo_api::{SettingsChanges, fake::Failure};

use crate::{Fixture, OLD_PASSWORD};

mod adopt {
    use super::*;

    #[tokio::test]
    async fn imports_an_ergo_account_under_its_canonical_name_at_the_root() {
        let fixture = Fixture::start().await;
        fixture.register("Alice").await;
        let email = SettingsChanges {
            email: Some("alice@example.org".into()),
            ..Default::default()
        };
        fixture
            .ergo
            .client()
            .update_settings("Alice", email)
            .await
            .unwrap();

        let output = fixture.run(&["member", "adopt", "ALICE"]).await.unwrap();

        assert_eq!(output, "adopted Alice\n");
        let member = fixture.db().await.member("alice").await.unwrap().unwrap();
        assert_eq!(member.account, "Alice");
        assert_eq!(member.email.as_deref(), Some("alice@example.org"));
        assert_eq!(member.invited_by, None);
        assert!(!member.is_admin);
        assert_eq!(member.removed_at, None);
    }

    #[tokio::test]
    async fn can_make_the_member_an_admin() {
        let fixture = Fixture::start().await;
        fixture.register("luiz").await;
        let output = fixture
            .run(&["member", "adopt", "luiz", "--admin"])
            .await
            .unwrap();
        assert_eq!(output, "adopted luiz as an admin\n");
        let member = fixture.db().await.member("luiz").await.unwrap().unwrap();
        assert!(member.is_admin);
        assert_eq!(member.email, None);
    }

    #[tokio::test]
    async fn refuses_an_account_ergo_does_not_have() {
        let fixture = Fixture::start().await;
        let error = fixture
            .run(&["member", "adopt", "nobody"])
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Ergo has no verified, unsuspended account named nobody"
        );
        assert_eq!(fixture.db().await.member("nobody").await.unwrap(), None);
    }

    #[tokio::test]
    async fn refuses_a_member_twice() {
        let fixture = Fixture::start().await;
        fixture.register("alice").await;
        fixture.run(&["member", "adopt", "alice"]).await.unwrap();
        let error = fixture
            .run(&["member", "adopt", "Alice"])
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "alice is already a member");
    }
}

mod reset_password {
    use super::*;

    #[tokio::test]
    async fn sets_a_new_password_in_ergo_prints_it_and_ends_the_members_sessions() {
        let fixture = Fixture::start().await;
        fixture.add_member("alice", None).await;
        fixture.add_member("bob", None).await;
        fixture.sign_in("alice", b"alice-laptop").await;
        fixture.sign_in("alice", b"alice-phone").await;
        fixture.sign_in("bob", b"bob-laptop").await;

        let output = fixture
            .run(&["member", "reset-password", "ALICE"])
            .await
            .unwrap();

        let password = output.strip_suffix('\n').unwrap();
        assert_eq!(password.len(), 26, "{output:?}");
        assert!(fixture.signs_in_to_ergo("alice", password).await);
        assert!(!fixture.signs_in_to_ergo("alice", OLD_PASSWORD).await);
        let db = fixture.db().await;
        assert_eq!(db.end_sessions("alice").await.unwrap(), 0);
        assert_eq!(db.end_sessions("bob").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn refuses_someone_who_is_not_a_member() {
        let fixture = Fixture::start().await;
        fixture.register("carol").await;
        let error = fixture
            .run(&["member", "reset-password", "carol"])
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "carol is not a member");
        assert!(fixture.signs_in_to_ergo("carol", OLD_PASSWORD).await);
    }

    #[tokio::test]
    async fn keeps_the_sessions_when_ergo_refuses_the_password() {
        let fixture = Fixture::start().await;
        fixture.add_member("alice", None).await;
        fixture.sign_in("alice", b"alice-laptop").await;
        fixture.ergo.fail_next(Failure::UnknownError);

        let error = fixture
            .run(&["member", "reset-password", "alice"])
            .await
            .unwrap_err();

        assert_eq!(
            format!("{error:#}"),
            "Ergo didn't take alice's new password: \
             Ergo refused the request: UNKNOWN_ERROR (simulated failure)"
        );
        assert!(fixture.signs_in_to_ergo("alice", OLD_PASSWORD).await);
        assert_eq!(fixture.db().await.end_sessions("alice").await.unwrap(), 1);
    }
}

mod delete {
    use super::*;

    async fn invite(fixture: &Fixture, code: &str, created_by: &str) {
        let now = db::now();
        let invite = NewInvite {
            code,
            created_by: Some(created_by),
            note: Some("for my sister"),
            created_at: now,
            expires_at: now + 3600,
        };
        fixture.db().await.insert_invite(&invite).await.unwrap();
    }

    #[tokio::test]
    async fn leaves_a_placeholder_in_the_tree_and_reminds_to_erase_the_account() {
        let fixture = Fixture::start().await;
        fixture.add_member("alice", None).await;
        fixture.add_member("bob", Some("alice")).await;
        fixture.sign_in("alice", b"alice-laptop").await;
        invite(&fixture, "q7Hx2mZp9vLcT4wRfBa-_w", "alice").await;

        let output = fixture.run(&["member", "delete", "Alice"]).await.unwrap();

        assert_eq!(
            output,
            "removed alice (sessions ended: 1, invites revoked: 1)\n\
             Ergo's API can't delete accounts: an oper must run /NS ERASE alice\n"
        );
        let db = fixture.db().await;
        let alice = db.member("alice").await.unwrap().unwrap();
        assert!(alice.removed_at.is_some());
        assert_eq!((alice.email, alice.is_admin), (None, false));
        let bob = db.member("bob").await.unwrap().unwrap();
        assert_eq!(bob.invited_by.as_deref(), Some("alice"));
        let invite = db.invite("q7Hx2mZp9vLcT4wRfBa-_w").await.unwrap().unwrap();
        assert!(invite.revoked_at.is_some());
        assert_eq!(invite.note, None);
        assert_eq!(db.end_sessions("alice").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn deletes_the_members_bots_too() {
        let fixture = Fixture::start().await;
        fixture.add_member("alice", None).await;
        fixture.add_member("bob", Some("alice")).await;
        let mut passwords = Vec::new();
        for (owner, bot) in [("alice", "Marvin"), ("alice", "hal"), ("bob", "bender")] {
            let output = fixture.run(&["bot", "create", owner, bot]).await.unwrap();
            passwords.push(output.trim_end().to_owned());
        }

        let output = fixture.run(&["member", "delete", "alice"]).await.unwrap();

        assert_eq!(
            output,
            "removed alice (sessions ended: 0, invites revoked: 0)\n\
             Ergo's API can't delete accounts: an oper must run /NS ERASE alice\n\
             removed bot hal\n\
             Ergo's API can't delete accounts: an oper must run /NS ERASE hal\n\
             removed bot Marvin\n\
             Ergo's API can't delete accounts: an oper must run /NS ERASE Marvin\n"
        );
        let [marvin, hal, bender] = &passwords[..] else {
            unreachable!()
        };
        assert!(!fixture.signs_in_to_ergo("marvin", marvin).await);
        assert!(!fixture.signs_in_to_ergo("hal", hal).await);
        assert!(fixture.signs_in_to_ergo("bender", bender).await);
        let bots = fixture.db().await.bots(None).await.unwrap();
        assert_eq!(bots.len(), 1, "{bots:?}");
    }

    #[tokio::test]
    async fn keeps_the_member_when_ergo_fails_to_lock_a_bot_out() {
        let fixture = Fixture::start().await;
        fixture.add_member("alice", None).await;
        fixture
            .run(&["bot", "create", "alice", "hal"])
            .await
            .unwrap();
        fixture.ergo.fail_next(Failure::UnknownError);

        let error = fixture.run(&["member", "delete", "alice"]).await;

        assert!(error.is_err());
        let db = fixture.db().await;
        assert_eq!(db.member("alice").await.unwrap().unwrap().removed_at, None);
        assert_eq!(db.bots(Some("alice")).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn removes_a_member_with_no_sessions_or_invites() {
        let fixture = Fixture::start().await;
        fixture.add_member("carol", None).await;
        let output = fixture.run(&["member", "delete", "carol"]).await.unwrap();
        assert!(
            output.starts_with("removed carol (sessions ended: 0, invites revoked: 0)\n"),
            "{output}"
        );
    }

    #[tokio::test]
    async fn a_removed_member_cannot_be_deleted_reset_or_adopted_again() {
        let fixture = Fixture::start().await;
        fixture.add_member("alice", None).await;
        fixture.run(&["member", "delete", "alice"]).await.unwrap();

        for (args, message) in [
            (["member", "delete", "alice"], "alice was removed"),
            (["member", "reset-password", "alice"], "alice was removed"),
            (
                ["member", "adopt", "alice"],
                "alice was removed, and keeps their place in the invite tree",
            ),
        ] {
            let error = fixture.run(&args).await.unwrap_err();
            assert_eq!(error.to_string(), message, "{args:?}");
        }
    }

    #[tokio::test]
    async fn refuses_someone_who_is_not_a_member() {
        let fixture = Fixture::start().await;
        let error = fixture
            .run(&["member", "delete", "nobody"])
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "nobody is not a member");
    }
}

mod reconcile {
    use super::*;

    #[tokio::test]
    async fn prints_nothing_when_ergo_and_concierge_agree() {
        let fixture = Fixture::start().await;
        fixture.add_member("alice", None).await;
        fixture.add_member("bob", Some("alice")).await;
        assert_eq!(fixture.run(&["member", "reconcile"]).await.unwrap(), "");
    }

    #[tokio::test]
    async fn reports_orphans_missing_accounts_and_accounts_left_to_erase() {
        let fixture = Fixture::start().await;
        fixture.add_member("alice", None).await;
        fixture.register("Orphan").await;
        fixture.register("Zed").await;
        fixture.insert_member("Gone", Some("alice")).await;
        fixture.add_member("dave", Some("alice")).await;
        fixture.run(&["member", "delete", "dave"]).await.unwrap();
        fixture.add_member("erin", Some("alice")).await;
        fixture.run(&["member", "delete", "erin"]).await.unwrap();

        let output = fixture.run(&["member", "reconcile"]).await.unwrap();

        assert_eq!(
            output,
            "only in concierge: Gone\n\
             only in ergo: Orphan\n\
             only in ergo: Zed\n\
             removed but still in ergo: dave\n\
             removed but still in ergo: erin\n"
        );
    }

    #[tokio::test]
    async fn knows_bot_accounts_and_reports_the_ones_out_of_step() {
        let fixture = Fixture::start().await;
        fixture.add_member("alice", None).await;
        fixture.add_member("carol", Some("alice")).await;
        for (owner, bot) in [("alice", "Bender"), ("alice", "hal"), ("carol", "marvin")] {
            fixture.run(&["bot", "create", owner, bot]).await.unwrap();
        }
        fixture.run(&["bot", "delete", "hal"]).await.unwrap();
        let db = fixture.db().await;
        let ghost = NewBot {
            account: "ghost",
            owner: "alice",
            note: None,
            created_at: 1_760_000_000,
        };
        db.insert_bot(&ghost).await.unwrap();
        db.remove_member("carol", db::now()).await.unwrap();

        let output = fixture.run(&["member", "reconcile"]).await.unwrap();

        assert_eq!(
            output,
            "bot of a removed member: marvin (carol)\n\
             only in concierge: bot ghost\n\
             removed but still in ergo: bot hal\n\
             removed but still in ergo: carol\n"
        );
    }

    #[tokio::test]
    async fn matches_names_whatever_their_case() {
        let fixture = Fixture::start().await;
        fixture.register("Alice").await;
        fixture.insert_member("aLiCe", None).await;
        assert_eq!(fixture.run(&["member", "reconcile"]).await.unwrap(), "");
    }

    #[tokio::test]
    async fn fails_when_ergo_does() {
        let fixture = Fixture::start().await;
        fixture.ergo.fail_next(Failure::Unauthorized);
        let error = fixture.run(&["member", "reconcile"]).await.unwrap_err();
        assert!(error.to_string().contains("API token"), "{error}");
    }
}
