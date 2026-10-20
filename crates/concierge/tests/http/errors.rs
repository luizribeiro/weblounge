//! The pages concierge's errors get.

use axum::response::Response;

use crate::{Site, body_text};

const NOT_FOUND: &str = r#"<section class="card narrow">
        <p class="eyebrow">Not found</p>
        <h1>There&#39;s nothing here.</h1>
        <p class="lede">Check the address, or go back home.</p>
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
