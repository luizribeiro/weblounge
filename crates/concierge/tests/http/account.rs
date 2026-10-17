use concierge::{db, tree, verification_rules};

use crate::{Site, body_text, location};

const HOUR: i64 = 60 * 60;

/// The Account tab of alice, whose first verification email went out
/// `ago` seconds ago, if one did.
async fn account_tab_mailed(ago: Option<i64>) -> (String, i64) {
    let site = Site::start().await;
    site.add_member("alice").await;
    let first_sent_at = ago.map(|ago| db::now() - ago);
    if let Some(first_sent_at) = first_sent_at {
        site.execute(&format!(
            "UPDATE members SET verification_first_sent_at = {first_sent_at}"
        ))
        .await;
    }
    let cookie = site.sign_in("alice").await;
    let page = body_text(site.get("/account", Some(&cookie)).await).await;
    let deadline = first_sent_at.map(verification_rules::deadline);
    (page, deadline.unwrap_or_default())
}

#[tokio::test]
async fn an_unconfirmed_email_shows_when_it_is_due() {
    let (page, deadline) = account_tab_mailed(Some(26 * HOUR)).await;
    let due = tree::date(deadline);
    assert!(
        page.contains(&format!(
            r#"<p class="notice">Open the link we emailed you to confirm it. Please confirm by {due} (5 days left).</p>"#
        )),
        "{page}"
    );
    assert!(
        page.contains(">Not confirmed yet. For account recovery.</p>"),
        "{page}"
    );
}

#[tokio::test]
async fn an_email_past_its_week_says_when_it_was_due() {
    let (page, deadline) = account_tab_mailed(Some(8 * 24 * HOUR)).await;
    let due = tree::date(deadline);
    assert!(
        page.contains(&format!("to confirm it. It was due by {due}.</p>")),
        "{page}"
    );
}

#[tokio::test]
async fn an_email_never_mailed_says_so() {
    let (page, _) = account_tab_mailed(None).await;
    assert!(
        page.contains("We haven&#39;t sent you a link to confirm it yet."),
        "{page}"
    );
}

#[tokio::test]
async fn a_confirmed_email_says_so_and_nothing_more() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.execute("UPDATE members SET email_verified_at = 1, verification_first_sent_at = 0")
        .await;
    let cookie = site.sign_in("alice").await;
    let page = body_text(site.get("/account", Some(&cookie)).await).await;
    assert!(
        page.contains(">Confirmed. For account recovery.</p>"),
        "{page}"
    );
    assert!(!page.contains("link we emailed you"), "{page}");
}

#[tokio::test]
async fn shows_the_email_and_news_settings() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let page = body_text(site.get("/account", Some(&cookie)).await).await;
    assert!(
        page.contains(r#"action="/settings" data-save-when-changed>"#),
        "{page}"
    );
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
async fn members_without_bots_see_nothing_about_them() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.add_member("luiz").await;
    site.execute(
        "INSERT INTO bots (account, owner, created_at) VALUES ('bender', 'luiz', 0);
         INSERT INTO bots (account, owner, created_at, removed_at) VALUES ('dalek', 'alice', 0, 1)",
    )
    .await;
    let cookie = site.sign_in("alice").await;
    let page = body_text(site.get("/account", Some(&cookie)).await).await;
    assert!(!page.contains(r#"id="bots""#), "{page}");
    assert!(
        !page.contains("bender") && !page.contains("dalek"),
        "{page}"
    );
}

#[tokio::test]
async fn lists_the_members_live_bots_and_their_notes() {
    let site = Site::start().await;
    site.add_member("luiz").await;
    site.add_member("alice").await;
    site.execute(
        "INSERT INTO bots (account, owner, note, created_at)
         VALUES ('bender', 'luiz', 'answers !ping', 0),
                ('Marvin', 'luiz', 'posts the <weather> & tides', 0),
                ('hal', 'luiz', NULL, 0),
                ('eliza', 'alice', 'talks back', 0);
         INSERT INTO bots (account, owner, created_at, removed_at) VALUES ('dalek', 'luiz', 0, 1)",
    )
    .await;
    let cookie = site.sign_in("luiz").await;

    let page = body_text(site.get("/account", Some(&cookie)).await).await;

    let card = page.split(r#"id="bots""#).nth(1).expect("a Bots card");
    let items: Vec<_> = card
        .split("<li>")
        .skip(1)
        .map(|item| item.split("</li>").next().unwrap())
        .collect();
    assert_eq!(
        items,
        [
            r#"<strong>bender</strong> <span class="muted">· answers !ping</span>"#,
            "<strong>hal</strong>",
            r#"<strong>Marvin</strong> <span class="muted">· posts the &#60;weather&#62; &#38; tides</span>"#,
        ]
    );
    assert!(!page.contains("dalek") && !page.contains("eliza"), "{page}");
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
