use axum::http::header;
use concierge::invite_rules::LIFETIME;
use regex::Regex;

use crate::{Site, body_text};

const CODE: &str = "q7Hx2mZp9vLcT4wRfBa-_w";

/// A signup that's right in every way but, perhaps, its `code`.
fn signup(code: &str) -> [(&str, &str); 5] {
    [
        ("code", code),
        ("nick", "bob"),
        ("email", "bob@example.org"),
        ("password", "xylophone-quokka-42"),
        ("confirmation", "xylophone-quokka-42"),
    ]
}

fn code_error(page: &str) -> Option<String> {
    let error = Regex::new(r#"<p class="field-error" id="code-hint">([^<]*)</p>"#).unwrap();
    error.captures(page).map(|c| c[1].to_owned())
}

#[tokio::test]
async fn the_signup_page_asks_for_the_code_and_the_account() {
    let site = Site::start().await;

    let response = site.get("/invite", None).await;

    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let page = body_text(response).await;
    assert!(page.contains("<h1>You're invited.</h1>"), "{page}");
    assert!(page.contains(r#"action="/invite""#), "{page}");
    for field in [
        r#"name="code" value="" required autocomplete="off""#,
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
async fn a_mistake_keeps_the_code_and_names_whoever_sent_it() {
    let site = Site::start().await;
    site.add_member("Alice").await;
    site.add_invite(CODE, Some("Alice")).await;
    let mut form = signup(CODE);
    form[4].1 = "something-else-entirely";

    let response = site.post("/invite", &form, None).await;

    assert_eq!(response.status(), 422);
    let page = body_text(response).await;
    assert!(page.contains("<h1>Alice saved you a seat.</h1>"), "{page}");
    assert!(
        page.contains(&format!(r#"name="code" value="{CODE}""#)),
        "{page}"
    );
    assert_eq!(code_error(&page), None, "{page}");
}

#[tokio::test]
async fn links_with_the_code_in_their_path_lead_to_it_in_the_fragment() {
    let site = Site::start().await;
    for (path, location) in [
        (format!("/invite/{CODE}"), format!("/invite#{CODE}")),
        ("/invite/not%20a%0d%0acode".into(), "/invite".into()),
    ] {
        let response = site.get(&path, None).await;
        assert_eq!(response.status(), 303, "{path}");
        assert_eq!(response.headers()[header::LOCATION], location.as_str());
    }
}

#[tokio::test]
async fn every_unusable_code_gets_the_same_answer() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let expired_at = concierge::db::now() - 1;
    for code in ["used", "revoked", "expired", "claimed"] {
        site.add_invite(code, Some("alice")).await;
    }
    site.execute(&format!(
        "INSERT INTO members (account, joined_at) VALUES ('carol', 0);
         UPDATE invites SET redeemed_by = 'carol', redeemed_at = 1 WHERE code = 'used';
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

    let mut errors = Vec::new();
    for code in ["used", "revoked", "expired", "claimed", "unknown", ""] {
        let response = site.post("/invite", &signup(code), None).await;
        assert_eq!(response.status(), 422, "{code}");
        let page = body_text(response).await;
        assert!(!page.contains("alice"), "{page}");
        errors.push(code_error(&page));
    }

    assert!(
        errors.windows(2).all(|pair| pair[0] == pair[1]),
        "{errors:?}"
    );
    let error = errors[0].as_deref().unwrap();
    assert!(error.contains("This invite doesn&#39;t work."), "{error}");
    assert!(error.contains("expire after 24 hours"), "{error}");
    assert_eq!(site.db.member("bob").await.unwrap(), None);
}

#[tokio::test]
async fn a_signed_in_member_gets_a_note_instead_of_the_form() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;

    let response = site.get("/invite", Some(&cookie)).await;

    assert_eq!(response.status(), 200);
    let page = body_text(response).await;
    assert!(page.contains("You already have an account."), "{page}");
    assert!(!page.contains("<form class=\"form\""), "{page}");
}
