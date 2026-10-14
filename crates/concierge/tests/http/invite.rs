use concierge::invite_rules::LIFETIME;

use crate::{Site, body_text};

const CODE: &str = "q7Hx2mZp9vLcT4wRfBa-_w";
const INVITE: &str = "/invite/q7Hx2mZp9vLcT4wRfBa-_w";

#[tokio::test]
async fn a_members_invite_names_them_and_offers_the_signup_form() {
    let site = Site::start().await;
    site.add_member("Alice").await;
    site.add_invite(CODE, Some("Alice")).await;

    let response = site.get(INVITE, None).await;

    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let page = body_text(response).await;
    assert!(page.contains("<h1>Alice saved you a seat.</h1>"), "{page}");
    assert!(page.contains(&format!(r#"action="{INVITE}""#)), "{page}");
    for field in [
        r#"name="nick" value="" required maxlength="32" pattern="[A-Za-z][A-Za-z0-9\-_]{0,31}" autocomplete="username""#,
        r#"name="email" type="email""#,
        r#"name="password" type="password" required minlength="12" autocomplete="new-password""#,
        r#"name="confirmation" type="password" required autocomplete="new-password""#,
        r#"<input name="news" type="checkbox">"#,
        r#"<li data-rule="not-common">not a common password</li>"#,
        r#"<a href="/rules">house rules</a>"#,
    ] {
        assert!(page.contains(field), "{field}: {page}");
    }
}

#[tokio::test]
async fn a_root_invite_names_nobody() {
    let site = Site::start().await;
    site.add_invite(CODE, None).await;
    let page = body_text(site.get(INVITE, None).await).await;
    assert!(page.contains("<h1>You're invited.</h1>"), "{page}");
}

#[tokio::test]
async fn every_unusable_link_gets_the_same_404() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let expired_at = concierge::db::now() - 1;
    for code in ["used", "revoked", "expired", "claimed"] {
        site.add_invite(code, Some("alice")).await;
    }
    site.execute(&format!(
        "INSERT INTO members (account, joined_at, invite_quota) VALUES ('bob', 0, 5);
         UPDATE invites SET redeemed_by = 'bob', redeemed_at = 1 WHERE code = 'used';
         UPDATE invites SET revoked_at = 1 WHERE code = 'revoked';
         UPDATE invites SET created_at = {}, expires_at = {expired_at} WHERE code = 'expired';",
        expired_at - LIFETIME
    ))
    .await;
    site.db
        .claim_invite("claimed", concierge::db::now())
        .await
        .unwrap()
        .unwrap();

    let mut pages = Vec::new();
    for code in ["used", "revoked", "expired", "claimed", "unknown", CODE] {
        let response = site.get(&format!("/invite/{code}"), None).await;
        assert_eq!(response.status(), 404, "{code}");
        pages.push(body_text(response).await);
    }

    assert!(pages.windows(2).all(|pair| pair[0] == pair[1]));
    let response = site.get(INVITE, None).await;
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert!(
        pages[0].contains("This invite link doesn&#39;t work."),
        "{}",
        pages[0]
    );
    assert!(!pages[0].contains("alice"), "{}", pages[0]);
}

#[tokio::test]
async fn a_signed_in_member_gets_a_note_instead_of_the_form() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.add_invite(CODE, Some("alice")).await;
    let cookie = site.sign_in("alice").await;

    let response = site.get(INVITE, Some(&cookie)).await;

    assert_eq!(response.status(), 200);
    let page = body_text(response).await;
    assert!(page.contains("You already have an account."), "{page}");
    assert!(!page.contains("<form class=\"form\""), "{page}");
}
