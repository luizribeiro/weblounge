use crate::{Site, body_text};

#[tokio::test]
async fn shows_how_to_connect_a_client() {
    let site = Site::start().await;
    site.add_member("Alice").await;
    let cookie = site.sign_in("alice").await;

    let response = site.get("/", Some(&cookie)).await;

    assert_eq!(response.status(), 200);
    let page = body_text(response).await;
    assert!(page.contains("<h1>Alice</h1>"), "{page}");
    for value in ["irc.weblounge.org", "6697", "PLAIN", "Alice"] {
        assert!(
            page.contains(&format!("<code>{value}</code>")),
            "{value}: {page}"
        );
    }
    assert!(page.contains("· TLS"), "{page}");
}
