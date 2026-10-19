use axum::http::header;
use concierge::{
    db,
    rate_limit::{LINKS_PER_DAY, PER_ACCOUNT, TRY_LATER},
    verification_mail, verification_rules,
};
use ergo_api::fake::Failure;

use crate::{
    NEWS_LABEL, PASSWORD, SEND_LINK_FORM, Site, body_text, confirm, execute, location, send_link,
    verification_link,
};

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

/// The notices and alerts in the Account tab's Email card.
fn email_notices(page: &str) -> Vec<&str> {
    let card = page
        .split(r#"id="email-and-news""#)
        .nth(1)
        .expect("an Email card");
    let card = card.split("</section>").next().unwrap();
    card.split(r#"<p class=""#)
        .filter_map(|paragraph| {
            paragraph
                .strip_prefix(r#"notice" role="status">"#)
                .or_else(|| paragraph.strip_prefix(r#"alert" role="alert">"#))
        })
        .map(|notice| notice.split("</p>").next().unwrap())
        .collect()
}

/// The email field's hint, under the address.
fn email_hint(page: &str) -> &str {
    let hint = page
        .split(r#"<p class="hint" id="email-hint">"#)
        .nth(1)
        .expect("a hint");
    hint.split("</p>").next().unwrap()
}

/// The deadline as the Email card writes it.
fn by_deadline(deadline: i64) -> String {
    format!(
        r#"by <time class="deadline" datetime="{}">{}</time>"#,
        concierge::tree::date(deadline),
        verification_rules::deadline_in_words(deadline)
    )
}

#[tokio::test]
async fn an_unconfirmed_email_says_when_it_is_due_and_offers_a_new_link() {
    let (page, deadline) = account_tab_mailed(Some(26 * HOUR)).await;
    assert_eq!(
        email_hint(&page),
        format!(
            "Not confirmed yet. Confirm {} (5 days left), or {}.",
            by_deadline(deadline),
            send_link("send a new link")
        )
    );
    assert!(page.contains(SEND_LINK_FORM), "{page}");
    assert_eq!(email_notices(&page), Vec::<&str>::new());
}

#[tokio::test]
async fn an_email_past_its_week_offers_a_new_link_without_the_date() {
    let (page, _) = account_tab_mailed(Some(8 * 24 * HOUR)).await;
    assert_eq!(
        email_hint(&page),
        format!(
            "Not confirmed yet. {} to confirm it.",
            send_link("Send a new link")
        )
    );
}

#[tokio::test]
async fn an_email_never_mailed_offers_a_first_link() {
    let (page, _) = account_tab_mailed(None).await;
    assert_eq!(
        email_hint(&page),
        format!(
            "Not confirmed yet. {} to confirm it.",
            send_link("Send a link")
        )
    );
    assert!(page.contains(SEND_LINK_FORM), "{page}");
}

#[tokio::test]
async fn a_confirmed_email_has_no_hint() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.execute("UPDATE members SET email_verified_at = 1, verification_first_sent_at = 0")
        .await;
    let cookie = site.sign_in("alice").await;
    let page = body_text(site.get("/account", Some(&cookie)).await).await;
    assert!(!page.contains(r#"id="email-hint""#), "{page}");
    assert!(!page.contains(r#"form="send-link""#), "{page}");
    assert!(!page.contains(SEND_LINK_FORM), "{page}");
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

    let form = [
        ("email", " alice@example.net "),
        ("news", "on"),
        ("current", PASSWORD),
    ];
    let response = site.post("/settings", &form, Some(&cookie)).await;

    assert_eq!(response.status(), 303);
    assert_eq!(location(&response), "/account?saved&sent#email-and-news");
    let member = site.db.member("alice").await.unwrap().unwrap();
    assert_eq!(member.email.as_deref(), Some("alice@example.net"));
    assert!(member.news_opt_in);
    let page = body_text(site.get("/account?saved", Some(&cookie)).await).await;
    assert_eq!(email_notices(&page), ["Saved."]);
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
async fn leaves_bots_to_their_own_tab() {
    let site = Site::start().await;
    site.add_member("luiz").await;
    site.execute("INSERT INTO bots (account, owner, created_at) VALUES ('bender', 'luiz', 0)")
        .await;
    let cookie = site.sign_in("luiz").await;
    let page = body_text(site.get("/account", Some(&cookie)).await).await;
    assert!(!page.contains("bender"), "{page}");
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
const SPAM: &str = "If it doesn't arrive in a few minutes, check your spam folder.";

/// alice, signed in, holding the code a link mailed her before.
async fn with_a_code_mailed() -> (Site, String, String) {
    let site = Site::start().await;
    site.add_member("alice").await;
    let issued = site.db.issue_verification_code("alice", db::now()).await;
    let cookie = site.sign_in("alice").await;
    (site, cookie, issued.unwrap().unwrap().code)
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
    assert_eq!(
        email_notices(&page),
        [format!(
            "We sent a new link to <strong>someone@example.org</strong>. {SPAM}"
        )]
    );
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
    assert_eq!(
        email_notices(&page),
        [
            "A link went out a few minutes ago. Give it time to arrive, and check your spam \
             folder, before asking for another."
        ]
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
    assert_eq!(
        email_notices(&page),
        ["Couldn&#39;t send a link just now. Try again in a minute."]
    );
    execute(&outbox, "ALTER TABLE away RENAME TO outbox").await;
    let response = site.post(RESEND, &[], Some(&cookie)).await;
    assert_eq!(location(&response), "/account?sent#email-and-news");
    assert_eq!(site.mail().await.len(), 1);
}

#[tokio::test]
async fn while_the_mailer_is_down_a_link_waits_and_the_last_one_keeps_working() {
    let (site, cookie, old) = with_a_code_mailed().await;
    site.outbox.withdraw_key().await.unwrap();

    let response = site.post(RESEND, &[], Some(&cookie)).await;

    assert_eq!(response.status(), 503);
    let page = body_text(response).await;
    assert_eq!(
        email_notices(&page),
        ["Couldn&#39;t send a link just now. Try again in a minute."]
    );
    let on_file = site.db.verification_code(&old).await.unwrap().unwrap();
    assert_eq!(on_file.status(db::now()), verification_rules::Status::Live);
    site.outbox
        .publish_key(&site.mailer_key.public())
        .await
        .unwrap();
    let response = site.post(RESEND, &[], Some(&cookie)).await;
    assert_eq!(location(&response), "/account?sent#email-and-news");
    assert_eq!(site.mail().await.len(), 1);
}

#[tokio::test]
async fn a_link_the_mailer_deleted_unsent_costs_no_wait() {
    let (site, cookie, _) = with_a_code_mailed().await;
    let response = site.post(RESEND, &[], Some(&cookie)).await;
    assert_eq!(location(&response), "/account?sent#email-and-news");
    let restarted = outbox::testing::TestKey::generate().public();
    site.outbox.publish_key(&restarted).await.unwrap();
    site.outbox
        .publish_key(&site.mailer_key.public())
        .await
        .unwrap();

    let response = site.post(RESEND, &[], Some(&cookie)).await;

    assert_eq!(location(&response), "/account?sent#email-and-news");
    assert_eq!(site.mail().await.len(), 1);
    let response = site.post(RESEND, &[], Some(&cookie)).await;
    assert_eq!(response.status(), 429);
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

    let form = [("email", "alice@example.net"), ("current", PASSWORD)];
    let response = site.post("/settings", &form, Some(&cookie)).await;

    assert_eq!(location(&response), "/account?saved&sent#email-and-news");
    let [message] = &site.mail().await[..] else {
        panic!("one message");
    };
    assert_eq!(message.recipient, "alice@example.net");
    let page = body_text(site.get("/account?saved&sent", Some(&cookie)).await).await;
    let alice = site.db.member("alice").await.unwrap().unwrap();
    let deadline = verification_rules::deadline(alice.verification_first_sent_at.unwrap());
    assert_eq!(
        email_notices(&page),
        [format!(
            "Saved. We sent a link to <strong>alice@example.net</strong>; open it to confirm {}. {SPAM}",
            by_deadline(deadline)
        )]
    );
    assert!(
        email_hint(&page).starts_with("Not confirmed yet. Confirm by"),
        "{page}"
    );
    assert_eq!(confirm(&site, &old).await.0, 422);
    assert_eq!(confirm(&site, verification_link(message).1).await.0, 200);
    let member = site.db.member("alice").await.unwrap().unwrap();
    assert!(member.email_verified_at.is_some());
}

#[tokio::test]
async fn a_new_address_past_the_week_gets_a_link_without_the_date() {
    let (site, cookie, _) = with_a_code_mailed().await;
    site.execute("UPDATE members SET verification_first_sent_at = 0")
        .await;

    let form = [("email", "alice@example.net"), ("current", PASSWORD)];
    site.post("/settings", &form, Some(&cookie)).await;

    let page = body_text(site.get("/account?saved&sent", Some(&cookie)).await).await;
    assert_eq!(
        email_notices(&page),
        [format!(
            "Saved. We sent a link to <strong>alice@example.net</strong>; open it to confirm your email. {SPAM}"
        )]
    );
}

#[tokio::test]
async fn a_new_address_gets_its_link_without_waiting_on_the_last() {
    let (site, cookie, _) = with_a_code_mailed().await;
    site.post(RESEND, &[], Some(&cookie)).await;

    let form = [("email", "alice@example.net"), ("current", PASSWORD)];
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
        let form = [("email", email.as_str()), ("current", PASSWORD)];
        site.post("/settings", &form, Some(&cookie)).await;
    }

    let form = [("email", "alice@example.net"), ("current", PASSWORD)];
    let response = site.post("/settings", &form, Some(&cookie)).await;

    assert_eq!(response.status(), 429);
    let page = body_text(response).await;
    assert_eq!(
        email_notices(&page),
        ["Saved. That&#39;s all the links for today. Try again tomorrow."]
    );
    assert!(page.contains(r#"value="alice@example.net""#), "{page}");
    assert!(page.contains(&send_link("send a new link")), "{page}");
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

    let form = [
        ("email", "BOB@example.org"),
        ("news", "on"),
        ("current", PASSWORD),
    ];
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

/// alice, signed in, sending her Email card with a new address and `current`.
async fn change_email(site: &Site, cookie: &str, current: &str) -> axum::response::Response {
    let form = [("email", "alice@example.net"), ("current", current)];
    site.post("/settings", &form, Some(cookie)).await
}

/// What the Email card says under its password field.
fn password_error(page: &str) -> &str {
    let error = page
        .split(r#"<p class="field-error" id="email-password-hint">"#)
        .nth(1)
        .expect("an error under the password");
    error.split("</p>").next().unwrap()
}

async fn email_unchanged(site: &Site) {
    let alice = site.db.member("alice").await.unwrap().unwrap();
    assert_eq!(alice.email.as_deref(), Some("someone@example.org"));
    assert!(site.mail().await.is_empty());
}

#[tokio::test]
async fn the_email_form_asks_for_the_password_to_change_the_address() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let page = body_text(site.get("/account", Some(&cookie)).await).await;
    let card = page.split(r#"id="email-and-news""#).nth(1).unwrap();
    assert!(
        card.contains(
            r#"<div class="field" data-needed-to-change="email" data-saved="someone@example.org">
            <label for="email-password">Current password</label>"#
        ),
        "{card}"
    );
    assert!(
        card.contains(r#"<input id="email-password" name="current" type="password" autocomplete="current-password" aria-describedby="email-password-hint">"#),
        "{card}"
    );
    assert!(
        card.contains(
            r#"<p class="hint" id="email-password-hint">Needed to change your email.</p>"#
        ),
        "{card}"
    );
}

#[tokio::test]
async fn a_wrong_or_missing_password_changes_no_email_and_keeps_what_was_typed() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for (current, status, error) in [
        ("not-the-password", 401, "Wrong password."),
        ("", 422, "Enter your password to change your email."),
    ] {
        let response = change_email(&site, &cookie, current).await;

        assert_eq!(response.status(), status, "{current:?}");
        let page = body_text(response).await;
        assert_eq!(password_error(&page), error);
        assert!(page.contains(r#"value="alice@example.net""#), "{page}");
        assert!(
            page.contains(r#"name="current" type="password" autocomplete="current-password" aria-describedby="email-password-hint" aria-invalid="true">"#),
            "{page}"
        );
        assert!(!page.contains("not-the-password"), "{page}");
        assert!(crate::is_tab(&page, "/account"), "{page}");
    }
    email_unchanged(&site).await;
}

#[tokio::test]
async fn wrong_passwords_for_the_email_count_against_signing_in() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for _ in 0..PER_ACCOUNT {
        let response = change_email(&site, &cookie, "guess-guess").await;
        assert_eq!(response.status(), 401);
    }

    let response = change_email(&site, &cookie, PASSWORD).await;

    assert_eq!(response.status(), 429);
    let page = body_text(response).await;
    assert_eq!(password_error(&page), TRY_LATER);
    email_unchanged(&site).await;
    assert_eq!(site.sign_in_form("alice", PASSWORD).await.status(), 429);
}

#[tokio::test]
async fn an_email_change_ergo_cannot_check_changes_nothing() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    site.ergo.fail_next(Failure::UnknownError);

    let response = change_email(&site, &cookie, PASSWORD).await;

    assert_eq!(response.status(), 503);
    let page = body_text(response).await;
    assert!(password_error(&page).contains("Nothing changed"), "{page}");
    email_unchanged(&site).await;
}

#[tokio::test]
async fn the_news_setting_alone_needs_no_password() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;

    let form = [
        ("email", "someone@example.org"),
        ("news", "on"),
        ("current", ""),
    ];
    let response = site.post("/settings", &form, Some(&cookie)).await;

    assert_eq!(location(&response), "/account?saved#email-and-news");
    assert!(site.db.member("alice").await.unwrap().unwrap().news_opt_in);
}
