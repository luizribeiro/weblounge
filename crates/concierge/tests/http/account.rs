use crate::{Site, body_text, location};

#[tokio::test]
async fn shows_the_email_and_news_settings() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let page = body_text(site.get("/account", Some(&cookie)).await).await;
    assert!(
        page.contains(r#"type="email" value="someone@example.org" required maxlength="254""#),
        "{page}"
    );
    assert!(
        page.contains(r#"<input name="news" type="checkbox">"#),
        "{page}"
    );
    assert!(!page.contains("Saved."), "{page}");
}

#[tokio::test]
async fn saves_the_settings_and_says_so() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;

    let form = [("email", " alice@example.net "), ("news", "on")];
    let response = site.post("/settings", &form, Some(&cookie)).await;

    assert_eq!(response.status(), 303);
    assert_eq!(location(&response), "/account?saved#email-and-news");
    let member = site.db.member("alice").await.unwrap().unwrap();
    assert_eq!(member.email.as_deref(), Some("alice@example.net"));
    assert!(member.news_opt_in);
    let page = body_text(site.get("/account?saved", Some(&cookie)).await).await;
    assert!(page.contains("Saved."), "{page}");
    assert!(page.contains(r#"id="email-and-news""#), "{page}");
    assert!(!page.contains("IRC settings saved."), "{page}");
    assert!(page.contains(r#"value="alice@example.net""#), "{page}");
    assert!(
        page.contains(r#"<input name="news" type="checkbox" checked>"#),
        "{page}"
    );

    site.post(
        "/settings",
        &[("email", "alice@example.net")],
        Some(&cookie),
    )
    .await;
    assert!(!site.db.member("alice").await.unwrap().unwrap().news_opt_in);
}

#[tokio::test]
async fn refuses_an_invalid_email_and_keeps_what_was_typed() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;

    let form = [("email", "alice@example"), ("news", "on")];
    let response = site.post("/settings", &form, Some(&cookie)).await;

    assert_eq!(response.status(), 422);
    let page = body_text(response).await;
    assert!(
        page.contains("That doesn&#39;t look like an email address."),
        "{page}"
    );
    assert!(page.contains(r#"value="alice@example""#), "{page}");
    assert!(page.contains(r#"aria-invalid="true""#), "{page}");
    assert!(crate::is_tab(&page, "/account"), "{page}");
    assert!(page.contains(r#"type="checkbox" checked"#), "{page}");
    let member = site.db.member("alice").await.unwrap().unwrap();
    assert_eq!(member.email.as_deref(), Some("someone@example.org"));
    assert!(!member.news_opt_in);
}

#[tokio::test]
async fn the_email_field_checks_the_servers_rule() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let page = body_text(site.get("/account", Some(&cookie)).await).await;
    let pattern = concierge::email::HTML_PATTERN;
    assert!(page.contains(&format!(r#"pattern="{pattern}""#)), "{page}");
}

#[tokio::test]
async fn guests_cannot_change_settings() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let response = site
        .post("/settings", &[("email", "mallory@example.org")], None)
        .await;
    assert_eq!(response.status(), 303);
    assert_eq!(location(&response), "/signin");
    let member = site.db.member("alice").await.unwrap().unwrap();
    assert_eq!(member.email.as_deref(), Some("someone@example.org"));
}
