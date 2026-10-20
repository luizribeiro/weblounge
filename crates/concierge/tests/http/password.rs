use axum::http::header;
use concierge::{rate_limit::PER_ACCOUNT, reset_mail};
use ergo_api::fake::Failure;
use outbox::Body;

use crate::{PASSWORD, Site, body_text, location};

const NEW_PASSWORD: &str = "lantern-orchard-71";

impl Site {
    async fn change_password(
        &self,
        cookie: &str,
        current: &str,
        new: &str,
        confirmation: &str,
    ) -> axum::response::Response {
        let form = [
            ("current", current),
            ("password", new),
            ("confirmation", confirmation),
        ];
        self.post("/password", &form, Some(cookie)).await
    }

    async fn opens(&self, account: &str, password: &str) -> bool {
        let checked = self.ergo.client().check_auth(account, password).await;
        checked.unwrap().is_some()
    }
}

async fn signed_in_alice() -> (Site, String) {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    (site, cookie)
}

/// Alice, as `signed_in_alice` makes her, with her email confirmed.
async fn signed_in_confirmed_alice() -> (Site, String) {
    let site = Site::start().await;
    site.add_verified_member("alice").await;
    let cookie = site.sign_in("alice").await;
    (site, cookie)
}

#[tokio::test]
async fn the_form_lets_password_managers_update_the_saved_password() {
    let (site, cookie) = signed_in_alice().await;
    let page = body_text(site.get("/account", Some(&cookie)).await).await;
    assert!(
        page.contains(r#"<form class="form" method="post" action="/password""#),
        "{page}"
    );
    assert!(
        page.contains(r#"<input class="visually-hidden" type="text" value="alice" autocomplete="username" readonly tabindex="-1" aria-hidden="true">"#),
        "{page}"
    );
    assert!(
        page.contains(r#"name="current" type="password" required autocomplete="current-password""#),
        "{page}"
    );
    assert!(
        page.contains(r#"<label for="confirmation">Confirm new password</label>"#),
        "{page}"
    );
    assert_eq!(page.matches(r#"autocomplete="new-password""#).count(), 2);
    assert!(
        page.contains(r#"data-password-rules="/password-rules.json""#),
        "{page}"
    );
    assert!(
        page.contains("IRC apps that are connected now stay connected."),
        "{page}"
    );
}

// Bitwarden takes the username from a field before the password, and would
// otherwise fill the nick into the email field.
#[tokio::test]
async fn no_text_field_precedes_the_current_password() {
    let (site, cookie) = signed_in_alice().await;
    let page = body_text(site.get("/account", Some(&cookie)).await).await;
    let current = page.find(r#"id="current-password""#).unwrap();
    assert!(page.find(r#"id="email""#).unwrap() > current, "{page}");
}

#[tokio::test]
async fn changes_the_password_and_ends_every_other_session_and_its_tokens() {
    let (site, cookie) = signed_in_alice().await;
    let phone = site.sign_in("alice").await;
    let phone_token = site.access_token(&phone).await;
    let own_token = site.access_token(&cookie).await;
    site.add_member("bob").await;
    let bobs = site.sign_in("bob").await;

    let response = site
        .change_password(&cookie, PASSWORD, NEW_PASSWORD, NEW_PASSWORD)
        .await;

    assert_eq!(response.status(), 303);
    assert_eq!(location(&response), "/account?password#change-password");
    assert!(site.opens("alice", NEW_PASSWORD).await);
    assert!(!site.opens("alice", PASSWORD).await);
    assert_eq!(site.session_expiry(&phone).await, None);
    assert_eq!(site.introspect(&phone_token).await["active"], false);
    assert!(site.session_expiry(&cookie).await.is_some());
    assert_eq!(site.introspect(&own_token).await["active"], true);
    assert!(site.session_expiry(&bobs).await.is_some());
    let page = body_text(site.get("/account?password", Some(&cookie)).await).await;
    assert!(page.contains("Password changed."), "{page}");
}

#[tokio::test]
async fn a_confirmed_members_change_tells_their_confirmed_address() {
    let (site, cookie) = signed_in_confirmed_alice().await;

    let response = site
        .change_password(&cookie, PASSWORD, NEW_PASSWORD, NEW_PASSWORD)
        .await;

    assert_eq!(response.status(), 303);
    let mail = site.mail().await;
    let notices: Vec<_> = mail
        .iter()
        .filter(|message| message.subject == reset_mail::CHANGED_SUBJECT)
        .collect();
    let [notice] = notices[..] else {
        panic!("one notice, got {mail:?}");
    };
    assert_eq!(notice.recipient, "someone@example.org");
    assert!(matches!(notice.body, Body::Plain(_)), "{notice:?}");
}

#[tokio::test]
async fn an_unconfirmed_members_change_sends_no_notice() {
    let (site, cookie) = signed_in_alice().await;

    let response = site
        .change_password(&cookie, PASSWORD, NEW_PASSWORD, NEW_PASSWORD)
        .await;

    assert_eq!(response.status(), 303);
    let mail = site.mail().await;
    assert!(
        !mail
            .iter()
            .any(|message| message.subject == reset_mail::CHANGED_SUBJECT),
        "{mail:?}"
    );
}

#[tokio::test]
async fn a_wrong_current_password_changes_nothing() {
    let (site, cookie) = signed_in_confirmed_alice().await;
    let phone = site.sign_in("alice").await;
    for current in ["not-the-password", ""] {
        let response = site
            .change_password(&cookie, current, NEW_PASSWORD, NEW_PASSWORD)
            .await;
        assert_eq!(response.status(), 401, "{current:?}");
        let page = body_text(response).await;
        assert!(page.contains("Wrong password."), "{page}");
        assert!(crate::is_tab(&page, "/account"), "{page}");
        assert!(!page.contains(NEW_PASSWORD), "{page}");
    }
    assert!(site.opens("alice", PASSWORD).await);
    assert!(site.session_expiry(&phone).await.is_some());
    assert!(site.mail().await.is_empty());
}

#[tokio::test]
async fn the_new_password_follows_the_signup_rules() {
    let (site, cookie) = signed_in_alice().await;
    for (new, confirmation, error, field) in [
        (
            "short-one",
            "short-one",
            "Passwords have at least 12 characters.",
            "password",
        ),
        (
            "qwerty123456",
            "qwerty123456",
            "That password is too common.",
            "password",
        ),
        (
            NEW_PASSWORD,
            "lantern-orchard-72",
            "The passwords don&#39;t match.",
            "confirmation",
        ),
    ] {
        let response = site
            .change_password(&cookie, PASSWORD, new, confirmation)
            .await;
        assert_eq!(response.status(), 422, "{new}");
        let page = body_text(response).await;
        let field_html = format!(r#"id="{field}" name="{field}""#);
        let after_field = &page[page.find(&field_html).unwrap()..];
        assert!(
            after_field.contains(&format!(r#"<p class="field-error">{error}</p>"#)),
            "{new}: {page}"
        );
        assert!(!page.contains(PASSWORD) && !page.contains(new), "{page}");
    }
    assert!(site.opens("alice", PASSWORD).await);
}

#[tokio::test]
async fn wrong_current_passwords_are_rate_limited_like_signing_in() {
    let (site, cookie) = signed_in_alice().await;
    for _ in 0..PER_ACCOUNT {
        let response = site
            .change_password(&cookie, "guess-guess", NEW_PASSWORD, NEW_PASSWORD)
            .await;
        assert_eq!(response.status(), 401);
    }
    let response = site
        .change_password(&cookie, PASSWORD, NEW_PASSWORD, NEW_PASSWORD)
        .await;
    assert_eq!(response.status(), 429);
    assert!(body_text(response).await.contains("Too many attempts."));
    assert!(site.opens("alice", PASSWORD).await);
    assert_eq!(site.sign_in_form("alice", PASSWORD).await.status(), 429);
}

#[tokio::test]
async fn ergo_failing_to_check_or_to_change_changes_nothing_and_says_so() {
    let (site, cookie) = signed_in_alice().await;
    let phone = site.sign_in("alice").await;
    for passes in [0, 1] {
        for _ in 0..passes {
            site.ergo.pass_next();
        }
        site.ergo.fail_next(Failure::UnknownError);
        let response = site
            .change_password(&cookie, PASSWORD, NEW_PASSWORD, NEW_PASSWORD)
            .await;
        assert_eq!(response.status(), 503, "{passes}");
        assert!(body_text(response).await.contains("Nothing changed"));
    }
    assert!(site.opens("alice", PASSWORD).await);
    assert!(site.session_expiry(&phone).await.is_some());
}

#[tokio::test]
async fn the_answer_is_never_cached() {
    let (site, cookie) = signed_in_alice().await;
    let response = site
        .change_password(&cookie, "wrong-password", NEW_PASSWORD, NEW_PASSWORD)
        .await;
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn guests_and_other_origins_change_nothing() {
    let (site, cookie) = signed_in_alice().await;
    let form = [
        ("current", PASSWORD),
        ("password", NEW_PASSWORD),
        ("confirmation", NEW_PASSWORD),
    ];
    let response = site.post("/password", &form, None).await;
    assert_eq!(location(&response), "/signin");
    let evil = [(header::ORIGIN.as_str(), "https://evil.example")];
    let response = site
        .post_with("/password", &form, Some(&cookie), &evil)
        .await;
    assert_eq!(response.status(), 403);
    assert!(site.opens("alice", PASSWORD).await);
}
