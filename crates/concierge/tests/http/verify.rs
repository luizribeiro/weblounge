use concierge::db;

use crate::{Site, body_text};

/// alice, whose email isn't verified, holding a code to verify it.
async fn with_a_code() -> (Site, String) {
    let site = Site::start().await;
    site.add_member("alice").await;
    let issued = site.db.issue_verification_code("alice", db::now()).await;
    (site, issued.unwrap().unwrap().code)
}

async fn verified(site: &Site, account: &str) -> bool {
    let member = site.db.member(account).await.unwrap().unwrap();
    member.email_verified_at.is_some()
}

#[tokio::test]
async fn the_page_asks_to_confirm_with_a_button() {
    let site = Site::start().await;

    let response = site.get("/verify", None).await;

    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let page = body_text(response).await;
    for part in [
        "<h1 id=\"verify-title\">Confirm your email.</h1>",
        r#"<form class="form" method="post" action="/verify" data-verify>"#,
        r#"name="code" required pattern="\s*(?:.*[#\/])?[A-Za-z0-9_\-]{43}\s*""#,
        r#"<button class="button button-primary" type="submit">Confirm my email</button>"#,
    ] {
        assert!(page.contains(part), "{part}: {page}");
    }
}

#[tokio::test]
async fn opening_the_page_verifies_nothing_whatever_the_url_carries() {
    let (site, code) = with_a_code().await;

    for path in ["/verify".to_owned(), format!("/verify?code={code}")] {
        assert_eq!(site.get(&path, None).await.status(), 200, "{path}");
    }

    assert!(!verified(&site, "alice").await);
    assert!(site.db.verification_code(&code).await.unwrap().is_some());
}
