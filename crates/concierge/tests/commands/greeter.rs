use std::{
    fs,
    path::{Path, PathBuf},
};

use ergo_api::Preference;

use crate::Fixture;

const PASSWORD: &str = "0f3a9c51d2e84b7f6a0c19e2d4b5a7c8";

/// A file holding `contents`, as systemd hands the greeter's secret over.
fn password_file(fixture: &Fixture, contents: &str) -> PathBuf {
    let path = fixture.state_dir().join("concierge-greeter-password");
    fs::write(&path, contents).unwrap();
    path
}

async fn ensure_account(fixture: &Fixture, file: &Path) -> anyhow::Result<String> {
    let file = file.to_str().unwrap();
    let args = ["greeter", "ensure-account", "--password-file", file];
    fixture.run(&args).await
}

/// Whether concierge is set up the way the greeter needs it.
async fn set_up(fixture: &Fixture, password: &str) -> bool {
    let settings = fixture.ergo.client().settings("concierge").await.unwrap();
    fixture.signs_in_to_ergo("concierge", password).await
        && fixture.ergo.vhost("concierge").as_deref() == Some("weblounge/concierge")
        && settings.always_on == Preference::On
}

#[tokio::test]
async fn creates_the_house_account_with_the_files_password_vhost_and_always_on() {
    let fixture = Fixture::start().await;
    let file = password_file(&fixture, &format!("{PASSWORD}\n"));

    let output = ensure_account(&fixture, &file).await.unwrap();

    assert_eq!(output, "created concierge\n");
    assert!(set_up(&fixture, PASSWORD).await);
}

#[tokio::test]
async fn sets_the_password_again_on_every_start() {
    let fixture = Fixture::start().await;
    ensure_account(&fixture, &password_file(&fixture, PASSWORD))
        .await
        .unwrap();
    let next_boot = "7be2c0d94f1a63e85d0b2c47a9f1e3d6";

    let output = ensure_account(&fixture, &password_file(&fixture, next_boot)).await;

    assert_eq!(output.unwrap(), "updated concierge\n");
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
        let file = password_file(&fixture, PASSWORD);

        let error = ensure_account(&fixture, &file).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "a member or a bot holds concierge, the greeter's name"
        );
        assert!(!fixture.signs_in_to_ergo("concierge", PASSWORD).await);
    }
}

#[tokio::test]
async fn refuses_a_file_without_a_usable_password() {
    let fixture = Fixture::start().await;
    let file = password_file(&fixture, "short\n");

    let error = ensure_account(&fixture, &file).await.unwrap_err();

    assert!(
        error.to_string().contains("doesn't hold a usable password"),
        "{error}"
    );
    assert!(
        fixture
            .ergo
            .client()
            .info("concierge")
            .await
            .unwrap()
            .is_none()
    );
}
