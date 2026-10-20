//! The pages concierge's errors get.

use axum::response::Response;
use concierge::{assets::Assets, db, session};

use crate::{Site, body_text, session_cookie};

const NOT_FOUND: &str = r#"<section class="card narrow">
        <p class="eyebrow">Not found</p>
        <h1>There&#39;s nothing here.</h1>
        <p class="lede">Check the address, or go back home.</p>
        <p><a class="button button-primary" href="/">Go home</a></p>
      </section>"#;

const INTERNAL_ERROR: &str = r#"<section class="card narrow">
        <p class="eyebrow">Error</p>
        <h1>Something went wrong on our end.</h1>
        <p class="lede">Nothing you did caused this. Try again in a minute.</p>
        <p><a class="button button-primary" href="/">Go home</a></p>
      </section>"#;

/// The page a response carries, once its status and type are checked.
async fn error_page(response: Response, status: u16) -> String {
    assert_eq!(response.status(), status);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    body_text(response).await
}

#[tokio::test]
async fn visitors_get_a_card_for_unknown_paths() {
    let site = Site::start().await;
    let page = error_page(site.get("/no-such-page", None).await, 404).await;
    assert!(page.contains(NOT_FOUND), "{page}");
    assert!(page.contains("<title>There&#39;s nothing here. · weblounge.org</title>"));
    assert!(page.contains(r#"<a href="/signin">Sign in</a>"#), "{page}");
}

#[tokio::test]
async fn members_get_the_card_under_their_own_header() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let page = error_page(site.get("/no-such-page", Some(&cookie)).await, 404).await;
    assert!(page.contains(NOT_FOUND), "{page}");
    assert!(
        page.contains(r#"<span class="session-nick">alice</span>"#),
        "{page}"
    );
    assert!(page.contains("Sign out"), "{page}");
}

#[tokio::test]
async fn the_card_keeps_a_renewed_session_cookie() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let hash = site.session_hash(&cookie).await;
    let two_days_old = session::expires_at(db::now()) - 2 * 24 * 60 * 60;
    site.db.renew_session(&hash, two_days_old).await.unwrap();
    let response = site.get("/no-such-page", Some(&cookie)).await;
    assert_eq!(session_cookie(&response).as_deref(), Some(cookie.as_str()));
    assert!(error_page(response, 404).await.contains(NOT_FOUND));
}

#[tokio::test]
async fn missing_client_guides_and_bots_get_the_same_card() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for (path, cookie) in [
        ("/clients/hexchat", None),
        ("/bots/nobody/password", Some(cookie.as_str())),
        ("/bots/nobody/delete", Some(cookie.as_str())),
    ] {
        let page = error_page(site.get(path, cookie).await, 404).await;
        assert!(page.contains(NOT_FOUND), "{path}: {page}");
    }
    let response = site.post("/bots/nobody/delete", &[], Some(&cookie)).await;
    assert!(error_page(response, 404).await.contains(NOT_FOUND));
}

#[tokio::test]
async fn a_failing_page_gives_members_the_error_card_under_their_header() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    site.execute("DROP TABLE bots").await;
    let page = error_page(site.get("/bots", Some(&cookie)).await, 500).await;
    assert!(page.contains(INTERNAL_ERROR), "{page}");
    assert!(
        page.contains(r#"<span class="session-nick">alice</span>"#),
        "{page}"
    );
}

#[tokio::test]
async fn a_failing_session_lookup_gives_the_error_card_to_a_visitor() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    site.execute("DROP TABLE sessions").await;
    let page = error_page(site.get("/rules", Some(&cookie)).await, 500).await;
    assert!(page.contains(INTERNAL_ERROR), "{page}");
    assert!(page.contains(r#"<a href="/signin">Sign in</a>"#), "{page}");
}

#[tokio::test]
async fn an_error_whose_page_cannot_render_answers_bare() {
    let built_nothing = tempfile::tempdir().unwrap();
    std::fs::write(
        built_nothing.path().join("manifest.json"),
        r#"{"outputs": {}}"#,
    )
    .unwrap();
    let site =
        Site::start_with(|app| app.assets = Assets::load(built_nothing.path()).unwrap()).await;
    for (path, status) in [("/rules", 500), ("/no-such-page", 404)] {
        let response = site.get(path, None).await;
        assert_eq!(response.status(), status, "{path}");
        assert!(!response.headers().contains_key("content-type"), "{path}");
        assert_eq!(body_text(response).await, "", "{path}");
    }
}
