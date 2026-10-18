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

    fn vhost(&self, name: &str) -> Option<String> {
        self.ergo.vhost(name)
    }

    async fn in_ergo(&self, name: &str) -> bool {
        self.ergo.client().info(name).await.unwrap().is_some()
    }

    /// Records a bot created on 2025-10-09, without touching Ergo.
    async fn insert_bot(&self, name: &str, owner: &str, note: Option<&str>) {
        let bot = NewBot {
            account: name,
            owner,
            note,
            created_at: 1_760_000_000,
        };
        self.db().await.insert_bot(&bot).await.unwrap();
    }

    /// A bot of luiz's that was deleted and that Ergo has since erased.
    async fn insert_deleted_bot(&self, name: &str) {
        self.insert_bot(name, "luiz", None).await;
        let removed = self.db().await.remove_bot(name, 1_760_100_000).await;
        assert!(removed.unwrap());
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
    async fn gives_the_bot_its_owners_vhost() {
        let fixture = Fixture::with_owners().await;
        fixture
            .run(&["bot", "create", "LUIZ", "bender"])
            .await
            .unwrap();
        assert_eq!(
            fixture.vhost("bender").as_deref(),
            Some("weblounge/bot/luiz")
        );
    }

    #[tokio::test]
    async fn gives_a_vhost_ergo_takes_even_for_the_longest_owner() {
        let fixture = Fixture::with_owners().await;
        let owner = "Zoe_the-longest-nick-anyone-has0";
        assert_eq!(owner.len(), concierge::nick::MAX_LENGTH);
        fixture.add_member(owner, Some("luiz")).await;

        fixture
            .run(&["bot", "create", owner, "bender"])
            .await
            .unwrap();

        let vhost = format!("weblounge/bot/{owner}");
        assert_eq!(fixture.vhost("bender"), Some(vhost));
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

    #[tokio::test]
    async fn says_so_when_ergo_refuses_the_vhost() {
        let fixture = Fixture::with_owners().await;
        fixture.ergo.pass_next();
        fixture.ergo.pass_next();
        fixture.ergo.fail_next(Failure::UnknownError);

        let error = fixture
            .run(&["bot", "create", "luiz", "bender"])
            .await
            .unwrap_err();

        assert_eq!(
            format!("{error:#}"),
            "bender was created, but not set up; bot reset-password bender tries again: \
             Ergo didn't give bender its vhost: \
             Ergo refused the request: UNKNOWN_ERROR (simulated failure)"
        );
        assert_eq!(fixture.vhost("bender"), None);
    }
}

mod reset_password {
    use ergo_api::SettingsChanges;

    use super::*;
    use crate::OLD_PASSWORD;

    async fn with_bender() -> (Fixture, String) {
        let fixture = Fixture::with_owners().await;
        let output = fixture
            .run(&["bot", "create", "luiz", "bender"])
            .await
            .unwrap();
        let password = printed_password(&output).to_owned();
        (fixture, password)
    }

    #[tokio::test]
    async fn sets_a_new_password_and_turns_always_on_off_and_the_vhost_on_again() {
        let (fixture, old) = with_bender().await;
        let always_on = SettingsChanges {
            always_on: Some(Preference::On),
            ..Default::default()
        };
        let client = fixture.ergo.client();
        client.update_settings("bender", always_on).await.unwrap();
        client.set_vhost("bender", "").await.unwrap();

        let output = fixture
            .run(&["bot", "reset-password", "BENDER"])
            .await
            .unwrap();

        let password = printed_password(&output);
        assert!(fixture.signs_in_to_ergo("bender", password).await);
        assert!(!fixture.signs_in_to_ergo("bender", &old).await);
        let settings = client.settings("bender").await.unwrap();
        assert_eq!(settings.always_on, Preference::Off);
        assert_eq!(
            fixture.vhost("bender").as_deref(),
            Some("weblounge/bot/luiz")
        );
    }

    #[tokio::test]
    async fn refuses_members_and_deleted_bots() {
        let (fixture, _) = with_bender().await;
        fixture.insert_deleted_bot("dalek").await;
        for (name, message) in [
            ("luiz", "luiz is not a bot"),
            ("Dalek", "dalek was deleted"),
            ("nobody", "nobody is not a bot"),
        ] {
            let error = fixture
                .run(&["bot", "reset-password", name])
                .await
                .unwrap_err();
            assert_eq!(error.to_string(), message);
        }
        assert!(fixture.signs_in_to_ergo("luiz", OLD_PASSWORD).await);
    }

    #[tokio::test]
    async fn keeps_the_password_when_ergo_refuses_it() {
        let (fixture, old) = with_bender().await;
        fixture.ergo.pass_next();
        fixture.ergo.pass_next();
        fixture.ergo.fail_next(Failure::UnknownError);

        let error = fixture
            .run(&["bot", "reset-password", "bender"])
            .await
            .unwrap_err();

        assert_eq!(
            format!("{error:#}"),
            "Ergo didn't take bender's new password: \
             Ergo refused the request: UNKNOWN_ERROR (simulated failure)"
        );
        assert!(fixture.signs_in_to_ergo("bender", &old).await);
    }
}

mod delete {
    use super::*;

    #[tokio::test]
    async fn locks_the_bot_out_keeps_its_name_and_reminds_to_erase_it() {
        let fixture = Fixture::with_owners().await;
        let output = fixture
            .run(&["bot", "create", "luiz", "Bender"])
            .await
            .unwrap();
        let password = printed_password(&output).to_owned();

        let output = fixture.run(&["bot", "delete", "bender"]).await.unwrap();

        assert_eq!(
            output,
            "removed bot Bender\n\
             Ergo's API can't delete accounts: an oper must run /NS ERASE Bender\n"
        );
        assert!(!fixture.signs_in_to_ergo("bender", &password).await);
        assert!(fixture.bot("bender").await.unwrap().removed_at.is_some());
        assert_eq!(fixture.db().await.bots(None).await.unwrap(), []);
        let error = fixture
            .run(&["bot", "create", "luiz", "bender"])
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "bender is taken");
    }

    #[tokio::test]
    async fn refuses_members_and_deleted_bots() {
        let fixture = Fixture::with_owners().await;
        fixture.insert_deleted_bot("dalek").await;
        for (name, message) in [
            ("luiz", "luiz is not a bot"),
            ("dalek", "dalek was deleted"),
        ] {
            let error = fixture.run(&["bot", "delete", name]).await.unwrap_err();
            assert_eq!(error.to_string(), message);
        }
    }

    #[tokio::test]
    async fn keeps_the_bot_when_ergo_fails() {
        let fixture = Fixture::with_owners().await;
        fixture
            .run(&["bot", "create", "luiz", "bender"])
            .await
            .unwrap();
        fixture.ergo.fail_next(Failure::UnknownError);

        let error = fixture.run(&["bot", "delete", "bender"]).await.unwrap_err();

        assert!(
            error.to_string().starts_with("Ergo didn't take bender's"),
            "{error:#}"
        );
        assert_eq!(fixture.bot("bender").await.unwrap().removed_at, None);
    }
}

mod ensure {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use ergo_api::SettingsChanges;

    use super::*;

    const PASSWORD: &str = "q7Hx-2mZp_9vLcT4wRfB";

    /// A file holding `contents`, as systemd hands a bot's secret over.
    fn password_file(fixture: &Fixture, contents: &str) -> PathBuf {
        let path = fixture.state_dir().join("bender-password");
        fs::write(&path, contents).unwrap();
        path
    }

    async fn ensure(fixture: &Fixture, owner: &str, file: &Path) -> anyhow::Result<String> {
        let file = file.to_str().unwrap();
        let args = ["bot", "ensure", owner, "bender", "--password-file", file];
        fixture.run(&args).await
    }

    #[tokio::test]
    async fn creates_a_missing_bot_with_the_files_password() {
        let fixture = Fixture::with_owners().await;
        let file = password_file(&fixture, &format!("{PASSWORD}\n"));

        let output = ensure(&fixture, "luiz", &file).await.unwrap();

        assert_eq!(output, "created bender\n");
        assert!(fixture.signs_in_to_ergo("bender", PASSWORD).await);
        assert_eq!(
            fixture.vhost("bender").as_deref(),
            Some("weblounge/bot/luiz")
        );
        let bot = fixture.bot("bender").await.unwrap();
        assert_eq!((bot.owner.as_str(), bot.note), ("luiz", None));
        let settings = fixture.ergo.client().settings("bender").await.unwrap();
        assert_eq!(settings.always_on, Preference::Off);
    }

    #[tokio::test]
    async fn sets_an_existing_bots_password_settings_and_vhost_every_time() {
        let fixture = Fixture::with_owners().await;
        fixture
            .run(&["bot", "create", "luiz", "bender"])
            .await
            .unwrap();
        let file = password_file(&fixture, PASSWORD);
        let always_on = SettingsChanges {
            always_on: Some(Preference::On),
            ..Default::default()
        };
        let client = fixture.ergo.client();
        client.update_settings("bender", always_on).await.unwrap();
        client.set_vhost("bender", "elsewhere").await.unwrap();

        for _ in 0..2 {
            let output = ensure(&fixture, "luiz", &file).await.unwrap();
            assert_eq!(output, "updated bender\n");
        }

        assert!(fixture.signs_in_to_ergo("bender", PASSWORD).await);
        let settings = client.settings("bender").await.unwrap();
        assert_eq!(settings.always_on, Preference::Off);
        assert_eq!(
            fixture.vhost("bender").as_deref(),
            Some("weblounge/bot/luiz")
        );
        assert_eq!(fixture.db().await.bots(None).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn refuses_another_members_deleted_or_ownerless_bot() {
        let fixture = Fixture::with_owners().await;
        fixture.add_member("zoe", Some("luiz")).await;
        let file = password_file(&fixture, PASSWORD);
        fixture
            .run(&["bot", "create", "zoe", "bender"])
            .await
            .unwrap();

        for (owner, message) in [
            ("luiz", "bender belongs to zoe"),
            ("alice", "alice was removed"),
            ("nobody", "nobody is not a member"),
        ] {
            let error = ensure(&fixture, owner, &file).await.unwrap_err();
            assert_eq!(error.to_string(), message);
        }
        assert!(!fixture.signs_in_to_ergo("bender", PASSWORD).await);

        fixture.run(&["bot", "delete", "bender"]).await.unwrap();
        let error = ensure(&fixture, "zoe", &file).await.unwrap_err();
        assert_eq!(error.to_string(), "bender was deleted");
    }

    #[tokio::test]
    async fn refuses_a_password_file_it_cannot_use() {
        let fixture = Fixture::with_owners().await;
        let missing = fixture.state_dir().join("missing");
        let error = ensure(&fixture, "luiz", &missing).await.unwrap_err();
        assert!(error.to_string().starts_with("can't read "), "{error:#}");

        let short = password_file(&fixture, "hunter2\n");
        let error = ensure(&fixture, "luiz", &short).await.unwrap_err();
        assert_eq!(
            format!("{error:#}"),
            format!(
                "{} doesn't hold a usable password: Passwords have at least 12 characters.",
                short.display()
            )
        );
        assert!(!fixture.in_ergo("bender").await);
    }

    #[tokio::test]
    async fn fails_when_ergo_does() {
        let fixture = Fixture::with_owners().await;
        fixture
            .run(&["bot", "create", "luiz", "bender"])
            .await
            .unwrap();
        let file = password_file(&fixture, PASSWORD);
        fixture.ergo.fail_next(Failure::Unauthorized);

        let error = ensure(&fixture, "luiz", &file).await.unwrap_err();

        assert!(
            error.to_string().starts_with("Ergo didn't turn"),
            "{error:#}"
        );
        assert!(!fixture.signs_in_to_ergo("bender", PASSWORD).await);
    }
}

mod list {
    use super::*;

    async fn with_bots() -> Fixture {
        let fixture = Fixture::with_owners().await;
        fixture.add_member("Zoe", Some("luiz")).await;
        fixture
            .insert_bot("marvin", "Zoe", Some("posts the weather"))
            .await;
        fixture
            .insert_bot("Bender", "luiz", Some("answers !ping"))
            .await;
        fixture.insert_bot("hal", "luiz", None).await;
        fixture.insert_deleted_bot("dalek").await;
        fixture
    }

    #[tokio::test]
    async fn prints_every_live_bot_by_name() {
        let fixture = with_bots().await;
        assert_eq!(
            fixture.run(&["bot", "list"]).await.unwrap(),
            "Bender luiz 2025-10-09 answers !ping\n\
             hal luiz 2025-10-09\n\
             marvin Zoe 2025-10-09 posts the weather\n"
        );
    }

    #[tokio::test]
    async fn prints_one_members_bots() {
        let fixture = with_bots().await;
        assert_eq!(
            fixture.run(&["bot", "list", "zoe"]).await.unwrap(),
            "marvin Zoe 2025-10-09 posts the weather\n"
        );
        assert_eq!(fixture.run(&["bot", "list", "alice"]).await.unwrap(), "");
    }

    #[tokio::test]
    async fn refuses_someone_who_is_not_a_member() {
        let fixture = with_bots().await;
        let error = fixture.run(&["bot", "list", "nobody"]).await.unwrap_err();
        assert_eq!(error.to_string(), "nobody is not a member");
    }
}
