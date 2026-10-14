use axum::http::header;
use concierge::rate_limit::{PER_ACCOUNT, PER_ADDRESS};

use crate::{PASSWORD, PUBLIC_URL, Site, body_text, location};

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

#[tokio::test]
async fn an_account_takes_five_wrong_passwords_then_waits() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.add_member("bob").await;
    for _ in 0..PER_ACCOUNT {
        assert_eq!(site.sign_in_form("Alice", "guess").await.status(), 401);
    }
    let response = site.sign_in_form("alice", PASSWORD).await;
    assert_eq!(response.status(), 429);
    assert!(body_text(response).await.contains("Too many attempts."));
    assert_eq!(site.sign_in_form("bob", PASSWORD).await.status(), 303);
}

#[tokio::test]
async fn a_successful_sign_in_clears_the_accounts_failures() {
    let site = Site::start().await;
    site.add_member("alice").await;
    for _ in 0..PER_ACCOUNT - 1 {
        site.sign_in_form("alice", "guess").await;
    }
    site.sign_in("alice").await;
    for _ in 0..PER_ACCOUNT - 1 {
        site.sign_in_form("alice", "guess").await;
    }
    assert_eq!(site.sign_in_form("alice", PASSWORD).await.status(), 303);
}

async fn sign_in_from(site: &Site, address: &str, nick: &str, password: &str) -> u16 {
    let headers = [
        (header::ORIGIN.as_str(), PUBLIC_URL),
        ("x-real-ip", address),
    ];
    let form = [("nick", nick), ("password", password)];
    let response = site.post_with("/signin", &form, None, &headers).await;
    response.status().as_u16()
}

#[tokio::test]
async fn an_address_takes_twenty_wrong_passwords_across_accounts_then_waits() {
    let site = Site::start().await;
    site.add_member("alice").await;
    for n in 0..PER_ADDRESS {
        let status = sign_in_from(&site, "192.0.2.7", &format!("guess{n}"), "guess").await;
        assert_eq!(status, 401);
    }
    assert_eq!(
        sign_in_from(&site, "192.0.2.7", "alice", PASSWORD).await,
        429
    );
    assert_eq!(
        sign_in_from(&site, "198.51.100.9", "alice", PASSWORD).await,
        303
    );
}

#[tokio::test]
async fn ergo_being_down_costs_no_attempts() {
    let site = Site::start().await;
    site.add_member("alice").await;
    for _ in 0..PER_ACCOUNT {
        site.ergo.fail_next(ergo_api::fake::Failure::UnknownError);
        assert_eq!(site.sign_in_form("alice", PASSWORD).await.status(), 503);
    }
    assert_eq!(location(&site.sign_in_form("alice", PASSWORD).await), "/");
}
