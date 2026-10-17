use concierge::{db, verification_rules};

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

async fn confirm(site: &Site, code: &str) -> (u16, String) {
    let response = site.post("/verify", &[("code", code)], None).await;
    assert!(!response.headers().contains_key("set-cookie"));
    (response.status().as_u16(), body_text(response).await)
}

#[tokio::test]
async fn confirming_verifies_the_email_without_signing_in() {
    let (site, code) = with_a_code().await;

    let (status, page) = confirm(&site, &code).await;

    assert_eq!(status, 200);
    assert!(page.contains("<h1>Your email is confirmed.</h1>"), "{page}");
    assert!(
        page.contains(r#"<a class="button button-primary" href="/account">Go to your account</a>"#),
        "{page}"
    );
    assert!(page.contains(r#"<a href="/signin">Sign in</a>"#), "{page}");
    assert!(verified(&site, "alice").await);
}

#[tokio::test]
async fn the_whole_link_pasted_in_works_too() {
    let (site, code) = with_a_code().await;
    let link = format!(" https://members.weblounge.org/verify#{code}\n");

    assert_eq!(confirm(&site, &link).await.0, 200);
    assert!(verified(&site, "alice").await);
}

#[tokio::test]
async fn used_unknown_expired_and_malformed_codes_get_the_same_answer() {
    let (site, used) = with_a_code().await;
    confirm(&site, &used).await;
    site.add_member("bob").await;
    let long_ago = db::now() - verification_rules::LIFETIME - 1;
    let expired = site.db.issue_verification_code("bob", long_ago).await;
    let expired = expired.unwrap().unwrap().code;
    let unknown = "A".repeat(verification_rules::CODE_LENGTH);
    let too_long = format!("{unknown}A");
    let odd = format!("{}+", &unknown[1..]);

    let mut answers = Vec::new();
    for code in [&used, &expired, &unknown, &too_long, &odd, "", "short"] {
        answers.push(confirm(&site, code).await);
    }

    assert!(
        answers.windows(2).all(|pair| pair[0] == pair[1]),
        "{answers:?}"
    );
    let (status, page) = &answers[0];
    assert_eq!(*status, 422);
    assert!(
        page.contains("<h1>This link doesn&#39;t work any more.</h1>"),
        "{page}"
    );
    assert!(page.contains("expire after 2 days"), "{page}");
    assert!(page.contains("spam folder"), "{page}");
    assert!(
        page.contains(r##"href="/account#email-and-news">Send a new link</a>"##),
        "{page}"
    );
    assert!(!verified(&site, "bob").await);
}

#[tokio::test]
async fn an_older_link_points_to_the_newer_email() {
    let (site, first) = with_a_code().await;
    site.db
        .issue_verification_code("alice", db::now())
        .await
        .unwrap()
        .unwrap();

    let (status, page) = confirm(&site, &first).await;

    assert_eq!(status, 422);
    assert!(page.contains("<h1>A newer email was sent.</h1>"), "{page}");
    assert!(page.contains("check your spam folder"), "{page}");
    assert!(!verified(&site, "alice").await);
}

#[tokio::test]
async fn a_link_to_an_address_the_member_left_verifies_nothing() {
    let (site, code) = with_a_code().await;
    site.execute("UPDATE members SET email = 'alice@example.net' WHERE account = 'alice'")
        .await;

    let (status, page) = confirm(&site, &code).await;

    assert_eq!(status, 422);
    assert!(
        page.contains("<h1>Your email changed since this link was sent.</h1>"),
        "{page}"
    );
    assert!(!verified(&site, "alice").await);
}

#[tokio::test]
async fn an_address_another_member_verified_first_stays_theirs() {
    let (site, alices) = with_a_code().await;
    site.add_member("bob").await;
    let bobs = site.db.issue_verification_code("bob", db::now()).await;
    confirm(&site, &bobs.unwrap().unwrap().code).await;

    let (status, page) = confirm(&site, &alices).await;

    assert_eq!(status, 422);
    assert!(
        page.contains("<h1>Another member uses that address.</h1>"),
        "{page}"
    );
    assert!(
        page.contains(r##"href="/account#email-and-news">Change your email</a>"##),
        "{page}"
    );
    assert!(!verified(&site, "alice").await);
    assert!(verified(&site, "bob").await);
}
