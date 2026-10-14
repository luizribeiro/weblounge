use axum::http::header;

use crate::{PASSWORD, PUBLIC_URL, Site, body_text};

const SIGN_IN: [(&str, &str); 2] = [("nick", "alice"), ("password", PASSWORD)];

#[tokio::test]
async fn posts_from_other_origins_are_refused() {
    let site = Site::start().await;
    site.add_member("alice").await;
    for headers in [
        vec![(header::ORIGIN.as_str(), "https://evil.example")],
        vec![(header::ORIGIN.as_str(), "null")],
        vec![(
            header::REFERER.as_str(),
            "https://evil.example/members.weblounge.org",
        )],
        vec![],
    ] {
        let response = site.post_with("/signin", &SIGN_IN, None, &headers).await;
        assert_eq!(response.status(), 403, "{headers:?}");
        assert_eq!(body_text(response).await, "Cross-origin request refused.\n");
    }
}

#[tokio::test]
async fn a_referer_from_our_origin_stands_in_for_a_missing_origin() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let referer = format!("{PUBLIC_URL}/signin");
    let headers = [(header::REFERER.as_str(), referer.as_str())];
    let response = site.post_with("/signin", &SIGN_IN, None, &headers).await;
    assert_eq!(response.status(), 303);
}

#[tokio::test]
async fn signing_out_from_another_origin_is_refused() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let headers = [(header::ORIGIN.as_str(), "https://evil.example")];
    let response = site
        .post_with("/signout", &[], Some(&cookie), &headers)
        .await;
    assert_eq!(response.status(), 403);
    assert!(site.session_expiry(&cookie).await.is_some());
}
