use concierge::invite_rules::DEFAULT_QUOTA;
use ergo_api::{Passphrase, Registration, SettingsChanges};

use crate::Fixture;

const OLD_PASSWORD: &str = "correct-horse-battery";

impl Fixture {
    async fn register(&self, account: &str) {
        let passphrase: Passphrase = OLD_PASSWORD.parse().unwrap();
        let registration = self.ergo.client().register(account, &passphrase).await;
        assert_eq!(registration.unwrap(), Registration::Created);
    }
}

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
        assert_eq!(member.invite_quota, DEFAULT_QUOTA);
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
