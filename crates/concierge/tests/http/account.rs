use axum::http::header;
use concierge::{db, rate_limit::LINKS_PER_DAY, verification_mail, verification_rules};

use crate::{NEWS_LABEL, Site, body_text, confirm, execute, location, verification_link};

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
    let due = verification_rules::deadline_in_words(deadline);
    assert!(
        page.contains(&format!(
            r#"<p>Open the link we emailed you to confirm it. Please confirm by {due} (5 days left).</p>"#
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
    let due = verification_rules::deadline_in_words(deadline);
    assert!(
        page.contains(&format!("to confirm it. It was due by {due}.</p>")),
        "{page}"
    );
}

#[tokio::test]
async fn an_email_never_mailed_says_so() {
    let (page, _) = account_tab_mailed(None).await;
    assert!(
        page.contains("We haven't sent you a link to confirm it yet."),
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
    assert!(page.contains(NEWS_LABEL), "{page}");
    assert!(page.contains(r#"id="email-title">Email</h2>"#), "{page}");
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
    assert_eq!(location(&response), "/account?saved&sent#email-and-news");
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

    let response = site
        .post(
            "/settings",
            &[("email", "alice@example.net")],
            Some(&cookie),
        )
        .await;
    assert_eq!(location(&response), "/account?saved#email-and-news");
    assert!(!site.db.member("alice").await.unwrap().unwrap().news_opt_in);
    assert_eq!(site.mail().await.len(), 1);
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

const RESEND: &str = "/verification-link";

/// alice, signed in, holding the code a link mailed her before.
async fn with_a_code_mailed() -> (Site, String, String) {
    let site = Site::start().await;
    site.add_member("alice").await;
    let issued = site.db.issue_verification_code("alice", db::now()).await;
    let cookie = site.sign_in("alice").await;
    (site, cookie, issued.unwrap().unwrap().code)
}

#[tokio::test]
async fn the_status_offers_a_link_or_a_new_one() {
    let (page, _) = account_tab_mailed(None).await;
    assert!(
        page.contains(&format!(
            r#"<form method="post" action="{RESEND}">
            <button class="button" type="submit">Send a link</button>"#
        )),
        "{page}"
    );
    let (page, _) = account_tab_mailed(Some(HOUR)).await;
    assert!(page.contains(">Send a new link</button>"), "{page}");
}

#[tokio::test]
async fn sending_a_new_link_mails_one_that_replaces_the_old() {
    let (site, cookie, old) = with_a_code_mailed().await;

    let response = site.post(RESEND, &[], Some(&cookie)).await;

    assert_eq!(response.status(), 303);
    assert_eq!(location(&response), "/account?sent#email-and-news");
    let [message] = &site.mail().await[..] else {
        panic!("one message");
    };
    assert_eq!(message.recipient, "someone@example.org");
    assert_eq!(message.subject, verification_mail::SUBJECT);
    let page = body_text(site.get("/account?sent", Some(&cookie)).await).await;
    assert!(page.contains("A link is on its way."), "{page}");
    let (status, page) = confirm(&site, &old).await;
    assert_eq!(status, 422);
    assert!(page.contains("A newer email was sent."), "{page}");
    assert_eq!(confirm(&site, verification_link(message).1).await.0, 200);
}

#[tokio::test]
async fn asking_again_at_once_is_refused_without_mailing() {
    let (site, cookie, _) = with_a_code_mailed().await;
    site.post(RESEND, &[], Some(&cookie)).await;

    let response = site.post(RESEND, &[], Some(&cookie)).await;

    assert_eq!(response.status(), 429);
    let page = body_text(response).await;
    assert!(
        page.contains(r#"<p class="alert" role="alert">A link went out a few minutes ago."#),
        "{page}"
    );
    assert!(crate::is_tab(&page, "/account"), "{page}");
    assert_eq!(site.mail().await.len(), 1);
}

#[tokio::test]
async fn a_confirmed_member_gets_no_link() {
    let (site, cookie, _) = with_a_code_mailed().await;
    site.execute("UPDATE members SET email_verified_at = 1")
        .await;

    let response = site.post(RESEND, &[], Some(&cookie)).await;

    assert_eq!(response.status(), 303);
    assert_eq!(location(&response), "/account#email-and-news");
    assert!(site.mail().await.is_empty());
}

#[tokio::test]
async fn a_link_that_cannot_be_queued_says_to_try_again_and_costs_no_wait() {
    let (site, cookie, _) = with_a_code_mailed().await;
    let outbox = site.state.path().join("outbox.db");
    execute(&outbox, "ALTER TABLE outbox RENAME TO away").await;

    let response = site.post(RESEND, &[], Some(&cookie)).await;

    assert_eq!(response.status(), 503);
    let page = body_text(response).await;
    assert!(
        page.contains("Couldn&#39;t send a link just now. Try again in a minute."),
        "{page}"
    );
    execute(&outbox, "ALTER TABLE away RENAME TO outbox").await;
    let response = site.post(RESEND, &[], Some(&cookie)).await;
    assert_eq!(location(&response), "/account?sent#email-and-news");
    assert_eq!(site.mail().await.len(), 1);
}

#[tokio::test]
async fn guests_and_other_sites_cannot_ask_for_links() {
    let (site, cookie, _) = with_a_code_mailed().await;
    let response = site.post(RESEND, &[], None).await;
    assert_eq!(location(&response), "/signin");
    let headers = [(header::ORIGIN.as_str(), "https://evil.example")];
    let response = site.post_with(RESEND, &[], Some(&cookie), &headers).await;
    assert_eq!(response.status(), 403);
    assert!(site.mail().await.is_empty());
}

#[tokio::test]
async fn a_new_address_gets_a_link_and_the_old_ones_stops_working() {
    let (site, cookie, old) = with_a_code_mailed().await;

    let form = [("email", "alice@example.net")];
    let response = site.post("/settings", &form, Some(&cookie)).await;

    assert_eq!(location(&response), "/account?saved&sent#email-and-news");
    let [message] = &site.mail().await[..] else {
        panic!("one message");
    };
    assert_eq!(message.recipient, "alice@example.net");
    let page = body_text(site.get("/account?saved&sent", Some(&cookie)).await).await;
    assert!(
        page.contains("Saved.") && page.contains("A link is on its way."),
        "{page}"
    );
    assert_eq!(confirm(&site, &old).await.0, 422);
    assert_eq!(confirm(&site, verification_link(message).1).await.0, 200);
    let member = site.db.member("alice").await.unwrap().unwrap();
    assert!(member.email_verified_at.is_some());
}

#[tokio::test]
async fn a_new_address_gets_its_link_without_waiting_on_the_last() {
    let (site, cookie, _) = with_a_code_mailed().await;
    site.post(RESEND, &[], Some(&cookie)).await;

    let form = [("email", "alice@example.net")];
    let response = site.post("/settings", &form, Some(&cookie)).await;

    assert_eq!(location(&response), "/account?saved&sent#email-and-news");
    let recipients: Vec<_> = site.mail().await.into_iter().map(|m| m.recipient).collect();
    assert_eq!(recipients, ["someone@example.org", "alice@example.net"]);
}

#[tokio::test]
async fn a_new_address_is_saved_even_when_no_link_may_go_out() {
    let (site, cookie, _) = with_a_code_mailed().await;
    for n in 0..LINKS_PER_DAY {
        let email = format!("alice{n}@example.net");
        site.post("/settings", &[("email", email.as_str())], Some(&cookie))
            .await;
    }

    let form = [("email", "alice@example.net")];
    let response = site.post("/settings", &form, Some(&cookie)).await;

    assert_eq!(response.status(), 429);
    let page = body_text(response).await;
    assert!(
        page.contains(r#"<p class="notice" role="status">Saved.</p>"#),
        "{page}"
    );
    assert!(
        page.contains("That&#39;s all the links for today."),
        "{page}"
    );
    assert!(page.contains(r#"value="alice@example.net""#), "{page}");
    assert!(page.contains(">Send a new link</button>"), "{page}");
    let member = site.db.member("alice").await.unwrap().unwrap();
    assert_eq!(member.email.as_deref(), Some("alice@example.net"));
    assert_eq!(site.mail().await.len(), LINKS_PER_DAY);
}

#[tokio::test]
async fn an_address_another_member_verified_is_refused_in_any_case() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.add_member("bob").await;
    site.execute(
        "UPDATE members SET email = 'bob@example.org', email_verified_at = 1 WHERE account = 'bob'",
    )
    .await;
    let cookie = site.sign_in("alice").await;

    let form = [("email", "BOB@example.org"), ("news", "on")];
    let response = site.post("/settings", &form, Some(&cookie)).await;

    assert_eq!(response.status(), 422);
    let page = body_text(response).await;
    assert!(
        page.contains(
            r#"<p class="field-error" id="email-hint">Another member uses that address.</p>"#
        ),
        "{page}"
    );
    assert!(page.contains(r#"value="BOB@example.org""#), "{page}");
    let alice = site.db.member("alice").await.unwrap().unwrap();
    assert_eq!(alice.email.as_deref(), Some("someone@example.org"));
    assert!(site.mail().await.is_empty());
}

#[tokio::test]
async fn a_member_keeps_their_own_verified_address() {
    let site = Site::start().await;
    site.add_verified_member("alice").await;
    let cookie = site.sign_in("alice").await;

    let form = [("email", "someone@example.org"), ("news", "on")];
    let response = site.post("/settings", &form, Some(&cookie)).await;

    assert_eq!(location(&response), "/account?saved#email-and-news");
    let alice = site.db.member("alice").await.unwrap().unwrap();
    assert!(alice.news_opt_in && alice.email_verified_at.is_some());
}
