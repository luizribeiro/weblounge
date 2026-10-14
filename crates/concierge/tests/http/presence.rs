use axum::http::header;
use ergo_api::{Preference, SettingsChanges, fake::Failure};

use crate::{Site, body_text, location};

const ALWAYS_ON: &str = r#"<input name="always_on" type="checkbox""#;
const AUTO_AWAY: &str = r#"<input name="auto_away" type="checkbox""#;

async fn signed_in_alice() -> (Site, String) {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    (site, cookie)
}

async fn preferences(site: &Site) -> (Preference, Preference) {
    let settings = site.ergo.client().settings("alice").await.unwrap();
    (settings.always_on, settings.auto_away)
}

fn checked(page: &str, input: &str) -> bool {
    page.contains(&format!("{input} checked>"))
}

#[tokio::test]
async fn the_network_default_shows_as_on() {
    let (site, cookie) = signed_in_alice().await;
    let page = body_text(site.get("/", Some(&cookie)).await).await;
    assert!(page.contains(r#"<form class="form" method="post" action="/irc-settings">"#));
    assert!(checked(&page, ALWAYS_ON), "{page}");
    assert!(checked(&page, AUTO_AWAY), "{page}");
}

#[tokio::test]
async fn an_explicit_choice_in_ergo_shows_as_it_is() {
    let (site, cookie) = signed_in_alice().await;
    let changes = SettingsChanges {
        always_on: Some(Preference::On),
        auto_away: Some(Preference::Off),
        ..SettingsChanges::default()
    };
    site.ergo
        .client()
        .update_settings("alice", changes)
        .await
        .unwrap();
    let page = body_text(site.get("/", Some(&cookie)).await).await;
    assert!(checked(&page, ALWAYS_ON), "{page}");
    assert!(!checked(&page, AUTO_AWAY), "{page}");
}

#[tokio::test]
async fn switching_off_sets_off_and_switching_on_goes_back_to_the_default() {
    let (site, cookie) = signed_in_alice().await;

    let response = site
        .post("/irc-settings", &[("auto_away", "on")], Some(&cookie))
        .await;

    assert_eq!(response.status(), 303);
    assert_eq!(location(&response), "/?irc#irc-settings");
    assert_eq!(
        preferences(&site).await,
        (Preference::Off, Preference::Default)
    );
    let page = body_text(site.get("/?irc", Some(&cookie)).await).await;
    assert!(!checked(&page, ALWAYS_ON), "{page}");
    assert!(checked(&page, AUTO_AWAY), "{page}");
    assert!(page.contains("IRC settings saved."), "{page}");

    let both = [("always_on", "on"), ("auto_away", "on")];
    site.post("/irc-settings", &both, Some(&cookie)).await;
    assert_eq!(
        preferences(&site).await,
        (Preference::Default, Preference::Default)
    );
}

#[tokio::test]
async fn the_home_still_shows_when_ergo_cannot_answer() {
    let (site, cookie) = signed_in_alice().await;
    site.ergo.fail_next(Failure::UnknownError);
    let response = site.get("/", Some(&cookie)).await;
    assert_eq!(response.status(), 200);
    let page = body_text(response).await;
    assert!(page.contains("Can&#39;t reach the chat server."), "{page}");
    assert!(!page.contains(ALWAYS_ON), "{page}");
}

#[tokio::test]
async fn a_failed_save_says_so_and_changes_nothing() {
    let (site, cookie) = signed_in_alice().await;
    site.ergo.fail_next(Failure::UnknownError);
    let response = site.post("/irc-settings", &[], Some(&cookie)).await;
    assert_eq!(response.status(), 503);
    let page = body_text(response).await;
    assert!(page.contains("Nothing changed"), "{page}");
    assert_eq!(
        preferences(&site).await,
        (Preference::Default, Preference::Default)
    );
}

#[tokio::test]
async fn a_failed_save_with_ergo_still_down_says_so_once() {
    let (site, cookie) = signed_in_alice().await;
    site.ergo.fail_next(Failure::UnknownError);
    site.ergo.fail_next(Failure::UnknownError);
    let response = site.post("/irc-settings", &[], Some(&cookie)).await;
    assert_eq!(response.status(), 503);
    let page = body_text(response).await;
    assert_eq!(
        page.matches("Can&#39;t reach the chat server.").count(),
        1,
        "{page}"
    );
}

#[tokio::test]
async fn guests_and_other_origins_change_nothing() {
    let (site, cookie) = signed_in_alice().await;
    let response = site.post("/irc-settings", &[], None).await;
    assert_eq!(location(&response), "/signin");
    let evil = [(header::ORIGIN.as_str(), "https://evil.example")];
    let response = site
        .post_with("/irc-settings", &[], Some(&cookie), &evil)
        .await;
    assert_eq!(response.status(), 403);
    assert_eq!(
        preferences(&site).await,
        (Preference::Default, Preference::Default)
    );
}
