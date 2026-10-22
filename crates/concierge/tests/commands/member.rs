use concierge::{
    db::{self, NewBot, NewInvite},
    email::Email,
    name::{BotName, MemberName},
};
use ergo_api::fake::Failure;
use outbox::{Message, Subject};

use crate::{Fixture, OLD_PASSWORD};

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
        assert_eq!(db.end_sessions(&MemberName::new("alice")).await.unwrap(), 0);
        assert_eq!(db.end_sessions(&MemberName::new("bob")).await.unwrap(), 1);
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
        assert_eq!(
            fixture
                .db()
                .await
                .end_sessions(&MemberName::new("alice"))
                .await
                .unwrap(),
            1
        );
    }
}

mod delete {
    use super::*;

    async fn invite(fixture: &Fixture, code: &str, created_by: &str) {
        let now = db::now();
        let invite = NewInvite {
            code,
            created_by: Some(&MemberName::new(created_by)),
            created_at: now,
            expires_at: now + 3600,
            makes_admin: false,
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
             An oper must still run /NS ERASE alice\n"
        );
        assert!(!fixture.signs_in_to_ergo("alice", OLD_PASSWORD).await);
        let db = fixture.db().await;
        let alice = db.member("alice").await.unwrap().unwrap();
        assert!(alice.removed_at.is_some());
        assert_eq!((alice.email, alice.is_admin), (None, false));
        let bob = db.member("bob").await.unwrap().unwrap();
        assert_eq!(
            bob.invited_by.as_ref().map(MemberName::as_str),
            Some("alice")
        );
        let invite = db.invite("q7Hx2mZp9vLcT4wRfBa-_w").await.unwrap().unwrap();
        assert!(invite.revoked_at.is_some());
        assert_eq!(db.end_sessions(&MemberName::new("alice")).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn deletes_the_mail_waiting_for_the_member_alone() {
        let fixture = Fixture::start().await;
        fixture.add_member("alice", None).await;
        fixture.add_member("bob", Some("alice")).await;
        let outbox = fixture.outbox().await;
        for account in ["alice", "bob"] {
            let message = Message {
                account: Some(account.into()),
                to: Email::parse("someone@example.org").unwrap(),
                subject: Subject::parse("Verify your email for weblounge").unwrap(),
                body: format!("Hi {account}, open the link to verify.\n"),
            };
            outbox.queue(&message, db::now()).await.unwrap();
        }

        fixture.run(&["member", "delete", "alice"]).await.unwrap();

        let due = outbox.due(db::now()).await.unwrap();
        let bodies: Vec<_> = due.iter().map(|message| message.text()).collect();
        assert_eq!(bodies, ["Hi bob, open the link to verify.\n"]);
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
             An oper must still run /NS ERASE alice\n\
             removed bot hal\n\
             removed bot Marvin\n"
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
        let alice = MemberName::new("alice");
        assert_eq!(db.bots(Some(&alice)).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn keeps_the_member_when_ergo_fails_to_lock_them_out() {
        let fixture = Fixture::start().await;
        fixture.add_member("alice", None).await;
        fixture.sign_in("alice", b"alice-laptop").await;
        fixture.ergo.fail_next(Failure::UnknownError);

        let error = fixture.run(&["member", "delete", "alice"]).await;

        assert!(error.is_err());
        assert!(fixture.signs_in_to_ergo("alice", OLD_PASSWORD).await);
        let db = fixture.db().await;
        assert_eq!(db.member("alice").await.unwrap().unwrap().removed_at, None);
        assert_eq!(db.end_sessions(&MemberName::new("alice")).await.unwrap(), 1);
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
    async fn a_removed_member_cannot_be_deleted_or_reset_again() {
        let fixture = Fixture::start().await;
        fixture.add_member("alice", None).await;
        fixture.run(&["member", "delete", "alice"]).await.unwrap();

        for (args, message) in [
            (["member", "delete", "alice"], "alice was removed"),
            (["member", "reset-password", "alice"], "alice was removed"),
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
        let db = fixture.db().await;
        db.remove_bot(&BotName::new("hal"), db::now())
            .await
            .unwrap();
        let ghost = NewBot {
            account: &BotName::new("ghost"),
            owner: &MemberName::new("alice"),
            note: None,
            created_at: 1_760_000_000,
        };
        db.insert_bot(&ghost, None, None).await.unwrap();
        db.remove_member(&MemberName::new("carol"), db::now())
            .await
            .unwrap();

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

mod send_verification {
    use super::*;
    use concierge::verification_mail::{PER_RUN, SUBJECT};

    /// alice, who joined before emails were confirmed, bob, who was sent a
    /// link, and carol, who confirmed hers.
    async fn with_members() -> Fixture {
        let fixture = Fixture::start().await;
        for account in ["alice", "bob", "carol"] {
            fixture.insert_member(account, None).await;
        }
        let db = fixture.db().await;
        db.issue_verification_code(&MemberName::new("bob"), db::now())
            .await
            .unwrap();
        let carols = db
            .issue_verification_code(&MemberName::new("carol"), db::now())
            .await;
        db.verify_email(carols.unwrap().unwrap().code.expose(), db::now())
            .await
            .unwrap();
        fixture
    }

    #[tokio::test]
    async fn unverified_mails_a_link_to_whoever_never_had_one() {
        let fixture = with_members().await;

        let output = fixture
            .run(&["member", "send-verification", "--unverified"])
            .await
            .unwrap();

        assert_eq!(output, "sent alice a link\n");
        let [message] = &fixture.mail().await[..] else {
            panic!("one message");
        };
        assert_eq!(message.subject, SUBJECT);
        assert!(
            message.text().starts_with("Hi alice,\n"),
            "{}",
            message.text()
        );
        let alice = fixture.db().await.member("alice").await.unwrap().unwrap();
        assert!(alice.verification_first_sent_at.is_some());

        let again = fixture
            .run(&["member", "send-verification", "--unverified"])
            .await
            .unwrap();
        assert_eq!(again, "every unconfirmed member was sent a link already\n");
        assert_eq!(fixture.mail().await.len(), 1);
    }

    #[tokio::test]
    async fn a_member_named_gets_a_new_link_even_after_one_went_out() {
        let fixture = with_members().await;

        let output = fixture
            .run(&["member", "send-verification", "BOB"])
            .await
            .unwrap();

        assert_eq!(output, "sent bob a link\n");
        let queued = fixture.mail().await;
        assert_eq!(queued.len(), 1);
        assert!(
            queued[0].text().starts_with("Hi bob,\n"),
            "{}",
            queued[0].text()
        );
    }

    #[tokio::test]
    async fn a_member_named_who_confirmed_gets_nothing() {
        let fixture = with_members().await;

        let output = fixture
            .run(&["member", "send-verification", "carol"])
            .await
            .unwrap();

        assert_eq!(output, "carol confirmed their email already\n");
        assert!(fixture.mail().await.is_empty());
    }

    #[tokio::test]
    async fn refuses_someone_who_is_not_a_member_and_needs_an_account_or_unverified() {
        let fixture = with_members().await;
        let not_a_member = fixture.run(&["member", "send-verification", "dave"]);
        assert_eq!(
            not_a_member.await.unwrap_err().to_string(),
            "dave is not a member"
        );
        for args in [
            &["member", "send-verification"][..],
            &["member", "send-verification", "alice", "--unverified"],
        ] {
            assert!(fixture.run(args).await.is_err(), "{args:?}");
        }
        assert!(fixture.mail().await.is_empty());
    }

    #[tokio::test]
    async fn mails_a_batch_a_run_and_says_how_many_are_left() {
        let fixture = Fixture::start().await;
        for n in 0..PER_RUN + 2 {
            fixture.insert_member(&format!("m{n:03}"), None).await;
        }

        let output = fixture
            .run(&["member", "send-verification", "--unverified"])
            .await
            .unwrap();

        let lines: Vec<_> = output.lines().collect();
        assert_eq!(lines.len(), PER_RUN + 1, "{output}");
        assert_eq!(lines[0], "sent m000 a link");
        assert_eq!(lines[PER_RUN], "2 more to send: run this again in an hour");
        assert_eq!(fixture.mail().await.len(), PER_RUN);
        let rest = fixture
            .run(&["member", "send-verification", "--unverified"])
            .await
            .unwrap();
        assert_eq!(
            rest,
            format!(
                "sent m{PER_RUN:03} a link\nsent m{:03} a link\n",
                PER_RUN + 1
            )
        );
    }
}

#[tokio::test]
async fn deleting_a_member_deletes_their_uploads_and_their_bots() {
    let fixture = Fixture::start().await;
    fixture.add_member("alice", None).await;
    fixture.add_member("bob", None).await;
    fixture
        .run(&["bot", "create", "alice", "Marvin"])
        .await
        .unwrap();
    fixture
        .add_upload("AAAAAAAAAAAAAAAAAAAAAA", "alice", "alice", 1)
        .await;
    fixture
        .add_upload("BBBBBBBBBBBBBBBBBBBBBB", "Marvin", "alice", 2)
        .await;
    fixture
        .add_upload("CCCCCCCCCCCCCCCCCCCCCC", "bob", "bob", 3)
        .await;

    fixture.run(&["member", "delete", "alice"]).await.unwrap();

    let bobs = vec!["CCCCCCCCCCCCCCCCCCCCCC".to_owned()];
    assert_eq!(fixture.uploads_left().await, (bobs.clone(), bobs));
}
