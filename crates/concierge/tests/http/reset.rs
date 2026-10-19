use axum::{http::header, response::Response};
use concierge::{
    db,
    rate_limit::{LINKS_PER_HOUR, PER_ACCOUNT, RECOVERIES_PER_ADDRESS},
    reset_mail,
};
use ergo_api::fake::Failure;
use outbox::{Body, QueuedMessage};

use crate::{PASSWORD, PUBLIC_URL, Site, body_text};

const FORGOT: &str = "/forgot-password";
const SENT: &str = "<h1>Check your email.</h1>";

impl Site {
    /// A member whose email, `account@example.org`, is confirmed.
    async fn add_confirmed_member(&self, account: &str) {
        self.add_member(account).await;
        self.execute(&format!(
            "UPDATE members SET email = '{account}@example.org', email_verified_at = 1
             WHERE account = '{account}'"
        ))
        .await;
    }

    /// Asks for a reset link for `who` from `address`, and answers the
    /// status and page.
    async fn forgot_from(&self, who: &str, address: &str) -> (u16, String) {
        let headers = [
            (header::ORIGIN.as_str(), PUBLIC_URL),
            ("x-real-ip", address),
        ];
        let response = self
            .post_with(FORGOT, &[("who", who)], None, &headers)
            .await;
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        (response.status().as_u16(), body_text(response).await)
    }

    async fn forgot(&self, who: &str) -> (u16, String) {
        self.forgot_from(who, "192.0.2.7").await
    }

    /// The reset emails waiting in the outbox, opened.
    async fn reset_mail(&self) -> Vec<QueuedMessage> {
        let mail = self.mail().await.into_iter();
        mail.filter(|message| message.subject == reset_mail::LINK_SUBJECT)
            .collect()
    }
}

/// The reset link in `message`, and the code it carries.
pub(crate) fn reset_link(message: &QueuedMessage) -> (&str, &str) {
    let prefix = format!("{PUBLIC_URL}/reset#");
    let link = message
        .text()
        .lines()
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("a reset link in {}", message.text()));
    (link, &link[prefix.len()..])
}

/// carol, whose email is confirmed, and dave, whose isn't.
async fn with_members() -> Site {
    let site = Site::start().await;
    site.add_confirmed_member("carol").await;
    site.add_member("dave").await;
    site
}

#[tokio::test]
async fn signing_in_offers_a_way_to_reset_the_password() {
    let site = Site::start().await;
    let page = body_text(site.get("/signin", None).await).await;
    assert!(
        page.contains(
            r#"<p class="links"><a href="/forgot-password">Forgot your password?</a></p>"#
        ),
        "{page}"
    );
}

#[tokio::test]
async fn the_page_asks_for_a_nick_or_an_email_as_long_as_the_server_takes() {
    let site = Site::start().await;

    let response = site.get(FORGOT, None).await;

    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let page = body_text(response).await;
    for part in [
        r#"<h1 id="forgot-title">Forgot your password?</h1>"#,
        r#"<form class="form" method="post" action="/forgot-password">"#,
        r#"<label for="who">Nick or email</label>"#,
        r#"<input id="who" name="who" value="" required maxlength="254" autocomplete="username""#,
        r#"<button class="button button-primary" type="submit">Send a link</button>"#,
    ] {
        assert!(page.contains(part), "{part}: {page}");
    }
}

#[tokio::test]
async fn a_confirmed_member_gets_a_sealed_link_by_nick_or_email_in_any_case() {
    for who in ["carol", "CAROL", "carol@example.org", " Carol@Example.ORG "] {
        let site = with_members().await;

        let (status, page) = site.forgot(who).await;

        assert_eq!(status, 200, "{who}");
        assert!(page.contains(SENT), "{page}");
        assert!(
            page.contains("If that matches a member with a confirmed email, we&#39;ve sent a link to it. It works once, within 1 hour."),
            "{page}"
        );
        assert!(
            page.contains(r#"<a class="button button-primary" href="/signin">Back to sign in</a>"#),
            "{page}"
        );
        let [message] = &site.reset_mail().await[..] else {
            panic!("one reset email for {who}");
        };
        assert_eq!(message.recipient, "carol@example.org");
        assert!(message.text().starts_with("Hi carol,\n"));
        let due = site.outbox.due(db::now()).await.unwrap();
        assert!(matches!(due[0].body, Body::Sealed { .. }), "{due:?}");
        assert_eq!(reset_link(message).1.len(), 43);
    }
}

#[tokio::test]
async fn everyone_gets_the_same_answer_but_only_a_confirmed_member_gets_mail() {
    let site = with_members().await;
    site.add_confirmed_member("erin").await;
    site.db.remove_member("erin", db::now()).await.unwrap();

    let mut answers = Vec::new();
    for who in [
        "carol",
        "dave",
        "someone@example.org",
        "erin",
        "erin@example.org",
        "nobody",
        "nobody@example.org",
        "carol+irc@example.org",
    ] {
        answers.push(site.forgot(who).await);
    }

    assert!(
        answers.windows(2).all(|pair| pair[0] == pair[1]),
        "{answers:?}"
    );
    let recipients: Vec<_> = site.mail().await.into_iter().map(|m| m.recipient).collect();
    assert_eq!(recipients, ["carol@example.org"]);
}

#[tokio::test]
async fn asking_again_soon_answers_the_same_and_mails_nothing_more() {
    let site = with_members().await;
    let first = site.forgot("carol").await;

    let again = site.forgot("carol@example.org").await;

    assert_eq!(first, again);
    assert_eq!(site.reset_mail().await.len(), 1);
}

#[tokio::test]
async fn a_link_the_mailer_deleted_unsent_costs_no_wait() {
    let site = with_members().await;
    site.forgot("carol").await;
    site.outbox
        .publish_key(&outbox::testing::TestKey::generate().public())
        .await
        .unwrap();
    site.outbox
        .publish_key(&site.mailer_key.public())
        .await
        .unwrap();
    assert!(site.reset_mail().await.is_empty());

    site.forgot("carol").await;

    assert_eq!(site.reset_mail().await.len(), 1);
}

#[tokio::test]
async fn one_address_gets_a_few_tries_whoever_it_names() {
    let site = with_members().await;
    for n in 0..RECOVERIES_PER_ADDRESS {
        assert_eq!(site.forgot(&format!("nobody{n}")).await.0, 200);
    }

    let (status, page) = site.forgot("carol").await;

    assert_eq!(status, 429);
    assert!(
        page.contains(
            r#"<p class="alert" role="alert">Too many attempts. Try again in 15 minutes.</p>"#
        ),
        "{page}"
    );
    assert!(page.contains(r#"value="carol""#), "{page}");
    assert!(site.reset_mail().await.is_empty());
    assert_eq!(site.forgot_from("carol", "198.51.100.9").await.0, 200);
    assert_eq!(site.reset_mail().await.len(), 1);
}

#[tokio::test]
async fn once_the_hours_links_are_out_everyone_hears_to_try_later() {
    let site = with_members().await;
    for n in 0..LINKS_PER_HOUR {
        let account = format!("m{n:02}");
        site.add_confirmed_member(&account).await;
        let address = format!("192.0.2.{n}");
        assert_eq!(site.forgot_from(&account, &address).await.0, 200);
    }

    for who in ["carol", "nobody"] {
        let (status, page) = site.forgot_from(who, "198.51.100.9").await;
        assert_eq!(status, 429, "{who}");
        assert!(
            page.contains("A lot of mail is going out right now. Try again in an hour."),
            "{page}"
        );
    }
    assert_eq!(site.reset_mail().await.len(), LINKS_PER_HOUR);
}

#[tokio::test]
async fn while_the_mailer_is_down_everyone_hears_to_try_again_and_no_code_is_issued() {
    let site = with_members().await;
    site.outbox.withdraw_key().await.unwrap();

    let mut answers = Vec::new();
    for who in ["carol", "nobody"] {
        let (status, page) = site.forgot(who).await;
        answers.push((status, page.replace(&format!(r#"value="{who}""#), "")));
    }

    assert_eq!(answers[0], answers[1]);
    let (status, page) = &answers[0];
    assert_eq!(*status, 503);
    assert!(
        page.contains(r#"<p class="alert" role="alert">Couldn&#39;t send a link just now. Try again in a minute.</p>"#),
        "{page}"
    );
    let issued: i64 = site
        .query_scalar("SELECT count(*) FROM password_reset_codes")
        .await;
    assert_eq!(issued, 0);
}

#[tokio::test]
async fn an_empty_or_overlong_answer_is_refused_under_the_field() {
    let site = with_members().await;
    for (who, error) in [
        ("  ", "Enter your nick or email."),
        (
            &"x".repeat(255),
            "Nicks and emails have at most 254 characters.",
        ),
    ] {
        let (status, page) = site.forgot(who).await;
        assert_eq!(status, 422);
        assert!(
            page.contains(&format!(
                r#"<p class="field-error" id="who-error">{error}</p>"#
            )),
            "{page}"
        );
        assert!(page.contains(r#"aria-invalid="true""#), "{page}");
    }
    assert!(site.mail().await.is_empty());
}

#[tokio::test]
async fn other_sites_cannot_ask_for_links() {
    let site = with_members().await;
    let evil = [(header::ORIGIN.as_str(), "https://evil.example")];
    let response = site
        .post_with(FORGOT, &[("who", "carol")], None, &evil)
        .await;
    assert_eq!(response.status(), 403);
    assert!(site.mail().await.is_empty());
}

const NEW_PASSWORD: &str = "lantern-orchard-71";
const LAPSED: &str = "<h1>This link doesn&#39;t work any more.</h1>";

impl Site {
    /// Issues `account` a reset code, as mailing them a link does.
    async fn reset_code_of(&self, account: &str) -> String {
        let issued = self.db.issue_reset_code(account, db::now()).await;
        issued.unwrap().expect("a confirmed member").code
    }

    async fn reset_code(&self) -> String {
        self.reset_code_of("carol").await
    }

    /// Posts the reset page's form, as its button does.
    async fn reset_with(&self, code: &str, password: &str, confirmation: &str) -> Response {
        let form = [
            ("code", code),
            ("password", password),
            ("confirmation", confirmation),
        ];
        let headers = [
            (header::ORIGIN.as_str(), PUBLIC_URL),
            ("x-real-ip", "192.0.2.7"),
        ];
        self.post_with("/reset", &form, None, &headers).await
    }

    async fn reset(&self, code: &str) -> (u16, String) {
        let response = self.reset_with(code, NEW_PASSWORD, NEW_PASSWORD).await;
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        (response.status().as_u16(), body_text(response).await)
    }

    async fn carols_password_is(&self, password: &str) -> bool {
        let checked = self.ergo.client().check_auth("carol", password).await;
        checked.unwrap().is_some()
    }

    async fn reset_codes(&self) -> i64 {
        self.query_scalar("SELECT count(*) FROM password_reset_codes")
            .await
    }
}

#[tokio::test]
async fn the_link_opens_a_form_for_a_new_password_under_the_signup_rules() {
    let site = Site::start().await;

    let response = site.get("/reset", None).await;

    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let page = body_text(response).await;
    for part in [
        r#"<h1 id="reset-title">Choose a new password.</h1>"#,
        r#"<form class="form" method="post" action="/reset">"#,
        r#"<input id="code" name="code" required pattern="\s*(?:.*[#\/])?[A-Za-z0-9_\-]{43}\s*" autocomplete="off""#,
        r#"<label for="password">New password</label>"#,
        r#"<label for="confirmation">Confirm new password</label>"#,
        r#"data-password-rules="/password-rules.json""#,
        "IRC apps that are connected now stay connected, and logins with a client certificate keep working.",
        r#"<button class="button button-primary" type="submit">Set my password</button>"#,
    ] {
        assert!(page.contains(part), "{part}: {page}");
    }
}

#[tokio::test]
async fn opening_the_link_uses_nothing_whatever_the_url_carries() {
    let site = with_members().await;
    let code = site.reset_code().await;

    for path in ["/reset".to_owned(), format!("/reset?code={code}")] {
        assert_eq!(site.get(&path, None).await.status(), 200, "{path}");
    }

    assert_eq!(site.reset_codes().await, 1);
    assert_eq!(site.reset(&code).await.0, 200);
}

#[tokio::test]
async fn resetting_sets_the_password_ends_every_session_and_token_and_mails_a_notice() {
    let site = with_members().await;
    let laptop = site.sign_in("carol").await;
    let phone = site.sign_in("carol").await;
    let token = site.access_token(&phone).await;
    site.add_confirmed_member("erin").await;
    let erins = site.sign_in("erin").await;
    let code = site.reset_code().await;

    let response = site.reset_with(&code, NEW_PASSWORD, NEW_PASSWORD).await;

    assert_eq!(response.status(), 200);
    let set_cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(set_cookie.contains("Max-Age=0"), "{set_cookie}");
    let page = body_text(response).await;
    assert!(
        page.contains("<h1>Your password is changed.</h1>"),
        "{page}"
    );
    assert!(
        page.contains(r#"<a class="button button-primary" href="/signin">Sign in</a>"#),
        "{page}"
    );
    assert!(
        page.contains("logins with a client certificate keep working"),
        "{page}"
    );
    assert!(site.carols_password_is(NEW_PASSWORD).await);
    assert!(!site.carols_password_is(PASSWORD).await);
    for cookie in [&laptop, &phone] {
        assert_eq!(site.session_expiry(cookie).await, None);
    }
    assert_eq!(site.introspect(&token).await["active"], false);
    assert!(site.session_expiry(&erins).await.is_some());
    let due = site.outbox.due(db::now()).await.unwrap();
    let notice = due
        .iter()
        .find(|message| message.subject == reset_mail::CHANGED_SUBJECT)
        .expect("a notice");
    assert_eq!(notice.recipient, "carol@example.org");
    assert!(matches!(notice.body, Body::Plain(_)), "{notice:?}");
    assert_eq!(site.sign_in_form("carol", NEW_PASSWORD).await.status(), 303);
}

#[tokio::test]
async fn a_used_unknown_expired_replaced_or_malformed_code_gets_the_same_answer() {
    let site = with_members().await;
    let replaced = site.reset_code().await;
    let used = site.reset_code().await;
    site.reset(&used).await;
    site.execute(
        "UPDATE members SET email_verified_at = 1, email = 'dave@example.org' WHERE account = 'dave'",
    )
    .await;
    let expired = site.reset_code_of("dave").await;
    site.execute("UPDATE password_reset_codes SET expires_at = 0")
        .await;
    let unknown = "A".repeat(43);

    let mut answers = Vec::new();
    for code in [
        &used,
        &replaced,
        &expired,
        &unknown,
        &unknown[1..],
        "",
        "short",
    ] {
        answers.push(site.reset(code).await);
    }

    assert!(
        answers.windows(2).all(|pair| pair[0] == pair[1]),
        "{answers:?}"
    );
    let (status, page) = &answers[0];
    assert_eq!(*status, 422);
    assert!(page.contains(LAPSED), "{page}");
    assert!(page.contains("expire after 1 hour"), "{page}");
    assert!(
        page.contains(r#"href="/forgot-password">Get a new link</a>"#),
        "{page}"
    );
    assert!(site.carols_password_is(NEW_PASSWORD).await);
    let dave = site.ergo.client().check_auth("dave", PASSWORD).await;
    assert!(dave.unwrap().is_some());
}

#[tokio::test]
async fn a_code_stops_working_once_the_address_changes() {
    let site = with_members().await;
    let code = site.reset_code().await;
    site.execute("UPDATE members SET email = 'carol@example.net' WHERE account = 'carol'")
        .await;

    let (status, page) = site.reset(&code).await;

    assert_eq!(status, 422);
    assert!(page.contains(LAPSED), "{page}");
    assert!(site.carols_password_is(PASSWORD).await);
}

#[tokio::test]
async fn the_new_password_follows_the_signup_rules_and_the_form_keeps_the_code() {
    let site = with_members().await;
    let code = site.reset_code().await;
    for (new, confirmation, error) in [
        (
            "short-one",
            "short-one",
            "Passwords have at least 12 characters.",
        ),
        (
            "qwerty123456",
            "qwerty123456",
            "That password is too common.",
        ),
        (
            NEW_PASSWORD,
            "lantern-orchard-72",
            "The passwords don&#39;t match.",
        ),
    ] {
        let response = site.reset_with(&code, new, confirmation).await;

        assert_eq!(response.status(), 422, "{new}");
        let page = body_text(response).await;
        assert!(
            page.contains(&format!(r#"<p class="field-error">{error}</p>"#)),
            "{new}: {page}"
        );
        assert!(page.contains(&format!(r#"value="{code}""#)), "{page}");
        assert!(!page.contains(new), "{page}");
    }
    assert!(site.carols_password_is(PASSWORD).await);
    assert_eq!(site.reset(&code).await.0, 200);
}

#[tokio::test]
async fn when_ergo_fails_nothing_changes_and_the_link_keeps_working() {
    let site = with_members().await;
    let code = site.reset_code().await;
    site.ergo.fail_next(Failure::UnknownError);

    let (status, page) = site.reset(&code).await;

    assert_eq!(status, 503);
    assert!(page.contains("Nothing changed"), "{page}");
    assert!(site.carols_password_is(PASSWORD).await);
    assert_eq!(site.reset(&code).await.0, 200);
    assert!(site.carols_password_is(NEW_PASSWORD).await);
}

#[tokio::test]
async fn resetting_clears_the_wrong_guesses_that_locked_the_member_out() {
    let site = with_members().await;
    for _ in 0..PER_ACCOUNT {
        site.sign_in_form("carol", "a-wrong-guess").await;
    }
    assert_eq!(site.sign_in_form("carol", PASSWORD).await.status(), 429);
    let code = site.reset_code().await;

    site.reset(&code).await;

    assert_eq!(site.sign_in_form("carol", NEW_PASSWORD).await.status(), 303);
}

#[tokio::test]
async fn one_address_gets_a_few_posts_of_the_form() {
    let site = with_members().await;
    for _ in 0..RECOVERIES_PER_ADDRESS {
        site.reset_with("short", NEW_PASSWORD, NEW_PASSWORD).await;
    }
    let response = site.reset_with("short", NEW_PASSWORD, NEW_PASSWORD).await;
    assert_eq!(response.status(), 429);
    assert!(body_text(response).await.contains("Too many attempts."));
}

#[tokio::test]
async fn other_sites_cannot_post_the_form() {
    let site = with_members().await;
    let code = site.reset_code().await;
    let evil = [(header::ORIGIN.as_str(), "https://evil.example")];
    let form = [
        ("code", code.as_str()),
        ("password", NEW_PASSWORD),
        ("confirmation", NEW_PASSWORD),
    ];
    let response = site.post_with("/reset", &form, None, &evil).await;
    assert_eq!(response.status(), 403);
    assert!(site.carols_password_is(PASSWORD).await);
}
