use axum::http::header;
use concierge::{
    db,
    rate_limit::{LINKS_PER_HOUR, RECOVERIES_PER_ADDRESS},
    reset_mail,
};
use outbox::{Body, QueuedMessage};

use crate::{PUBLIC_URL, Site, body_text};

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
