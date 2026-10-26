use std::fs;

use concierge::{
    config::{GREETER_PASSWORD_CREDENTIAL, GreeterAccountConfig},
    house::{self, Ensured},
};
use ergo_api::Preference;

use crate::Fixture;

const PASSWORD: &str = "0f3a9c51d2e84b7f6a0c19e2d4b5a7c8";

/// Runs `concierge ensure-greeter-account` with `password` in its
/// credential, as systemd hands the generated one over.
async fn ensure_account(fixture: &Fixture, password: &str) -> anyhow::Result<Ensured> {
    let config = &fixture.config;
    let credential = config.credentials_dir.join(GREETER_PASSWORD_CREDENTIAL);
    fs::write(credential, format!("{password}\n")).unwrap();
    let config = GreeterAccountConfig {
        database: config.database_path(),
        ergo_api: config.ergo_api.clone(),
        credentials_dir: config.credentials_dir.clone(),
    };
    house::ensure_account(&config).await
}

/// Whether concierge is set up the way the greeter needs it.
async fn set_up(fixture: &Fixture, password: &str) -> bool {
    let settings = fixture.ergo.client().settings("concierge").await.unwrap();
    fixture.signs_in_to_ergo("concierge", password).await
        && fixture.ergo.vhost("concierge").as_deref() == Some("weblounge/concierge")
        && settings.always_on == Preference::On
}

#[tokio::test]
async fn creates_the_house_account_with_its_password_vhost_and_always_on() {
    let fixture = Fixture::start().await;

    let ensured = ensure_account(&fixture, PASSWORD).await.unwrap();

    assert_eq!(ensured, Ensured::Created);
    assert!(set_up(&fixture, PASSWORD).await);
}

#[tokio::test]
async fn sets_the_password_again_on_every_start() {
    let fixture = Fixture::start().await;
    ensure_account(&fixture, PASSWORD).await.unwrap();
    let next_boot = "7be2c0d94f1a63e85d0b2c47a9f1e3d6";

    let ensured = ensure_account(&fixture, next_boot).await.unwrap();

    assert_eq!(ensured, Ensured::Updated);
    assert!(set_up(&fixture, next_boot).await);
    assert!(!fixture.signs_in_to_ergo("concierge", PASSWORD).await);
}

#[tokio::test]
async fn refuses_while_a_member_or_a_bot_holds_the_name() {
    for held_by in [
        "INSERT INTO members (account, joined_at, removed_at) VALUES ('Concierge', 0, 1)",
        "INSERT INTO members (account, email, joined_at) VALUES ('luiz', 'luiz@example.org', 0);
         INSERT INTO bots (account, owner, created_at) VALUES ('concierge', 'luiz', 0)",
    ] {
        let fixture = Fixture::start().await;
        fixture.execute(held_by).await;

        let error = ensure_account(&fixture, PASSWORD).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "a member or a bot holds concierge, the greeter's name"
        );
        assert!(!fixture.signs_in_to_ergo("concierge", PASSWORD).await);
    }
}

#[tokio::test]
async fn refuses_a_credential_without_a_usable_password() {
    let fixture = Fixture::start().await;

    let error = ensure_account(&fixture, "short").await.unwrap_err();

    assert!(
        error.to_string().contains("doesn't hold a usable password"),
        "{error}"
    );
    let account = fixture.ergo.client().info("concierge").await.unwrap();
    assert!(account.is_none());
}
