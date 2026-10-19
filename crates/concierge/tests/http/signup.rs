use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use axum::{Json, Router, http::header, routing::post};
use ergo_api::fake::Failure;
use serde_json::json;
use tokio::net::TcpListener;

use concierge::{email::Email, name::MemberName, verification_mail};

use crate::{HISTORY_EXPIRY, Site, body_text, execute, session_cookie, verification_link};

const CODE: &str = "q7Hx2mZp9vLcT4wRfBa-_w";
const INVITE: &str = "/invite";
const NEW_PASSWORD: &str = "xylophone-quokka-42";

fn signup<'a>(nick: &'a str, password: &'a str, confirmation: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        ("nick", nick),
        ("email", "bob@example.org"),
        ("password", password),
        ("confirmation", confirmation),
        ("news", "on"),
        ("code", CODE),
    ]
}

fn bob() -> Vec<(&'static str, &'static str)> {
    signup("Bob", NEW_PASSWORD, NEW_PASSWORD)
}

async fn with_invite_from_alice() -> Site {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.add_invite(CODE, Some("alice")).await;
    site
}

async fn invite_is_open(site: &Site) -> bool {
    let invite = site.db.invite(CODE).await.unwrap().unwrap();
    invite.status(concierge::db::now()) == concierge::invite_rules::Status::Open
}

#[tokio::test]
async fn redeeming_creates_the_account_and_the_member_and_signs_them_in() {
    let site = with_invite_from_alice().await;

    let response = site.post(INVITE, &bob(), None).await;

    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let cookie = session_cookie(&response).expect("a session cookie");
    let page = body_text(response).await;
    assert!(page.contains("<h1>You're in, Bob.</h1>"), "{page}");
    assert!(crate::opens_the_chat(&page), "{page}");
    assert!(page.contains("The one you just set."), "{page}");
    assert!(page.contains(r#"data-copy="Bob""#), "{page}");
    assert!(!page.contains(NEW_PASSWORD), "{page}");

    let member = site.db.member("bob").await.unwrap().unwrap();
    assert_eq!(member.account.as_str(), "Bob");
    assert_eq!(
        member.invited_by.as_ref().map(MemberName::as_str),
        Some("alice")
    );
    assert_eq!(
        member.email.as_ref().map(Email::as_str),
        Some("bob@example.org")
    );
    assert!(member.news_opt_in);
    assert!(!member.is_admin);
    let invite = site.db.invite(CODE).await.unwrap().unwrap();
    assert_eq!(
        invite.redeemed_by.as_ref().map(MemberName::as_str),
        Some("Bob")
    );
    let ergo = site.ergo.client();
    let account = ergo.check_auth("bob", NEW_PASSWORD).await.unwrap();
    assert_eq!(account.unwrap().as_str(), "Bob");

    let home = body_text(site.get("/", Some(&cookie)).await).await;
    assert!(home.contains("Pull up a chair, Bob."), "{home}");
    assert!(!invite_is_open(&site).await);
}

#[tokio::test]
async fn signing_up_mails_a_link_that_verifies_the_new_members_email() {
    let site = with_invite_from_alice().await;

    let page = body_text(site.post(INVITE, &bob(), None).await).await;

    assert!(
        page.contains("We sent a link to <strong>bob@example.org</strong> to confirm your email.")
            && page.contains("check your spam folder"),
        "{page}"
    );
    let [message] = &site.mail().await[..] else {
        panic!("one message");
    };
    assert_eq!(message.recipient, "bob@example.org");
    assert_eq!(message.subject, verification_mail::SUBJECT);
    assert!(
        message.text().starts_with("Hi Bob,\n"),
        "{}",
        message.text()
    );
    let (link, code) = verification_link(message);
    assert!(!page.contains(code), "{page}");
    let bob = site.db.member("bob").await.unwrap().unwrap();
    let mailed_at = bob
        .verification_first_sent_at
        .expect("the grace period started");
    assert_eq!(message.created_at, mailed_at);

    let confirmed = site.post("/verify", &[("code", link)], None).await;

    assert_eq!(confirmed.status(), 200);
    let bob = site.db.member("bob").await.unwrap().unwrap();
    assert!(bob.email_verified_at.is_some());
}

#[tokio::test]
async fn signing_up_works_even_when_the_link_cannot_be_queued() {
    let site = with_invite_from_alice().await;
    execute(&site.state.path().join("outbox.db"), "DROP TABLE outbox").await;

    signs_bob_up_without_a_link(&site).await;
}

#[tokio::test]
async fn signing_up_while_the_mailer_is_down_issues_no_link() {
    let site = with_invite_from_alice().await;
    site.outbox.withdraw_key().await.unwrap();

    signs_bob_up_without_a_link(&site).await;

    let bob = site.db.member("bob").await.unwrap().unwrap();
    assert_eq!(bob.verification_first_sent_at, None);
    assert!(site.mail().await.is_empty());
}

/// Signs Bob up, and checks he's in but told to send himself a link.
async fn signs_bob_up_without_a_link(site: &Site) {
    let response = site.post(INVITE, &bob(), None).await;

    assert_eq!(response.status(), 200);
    assert!(session_cookie(&response).is_some());
    let page = body_text(response).await;
    assert!(page.contains("<h1>You're in, Bob.</h1>"), "{page}");
    assert!(
        page.contains("We couldn't send a link to confirm your email just now.")
            && page.contains(r##"<a href="/account#email-and-news">Account tab</a>"##),
        "{page}"
    );
    assert!(site.db.member("bob").await.unwrap().is_some());
}

#[tokio::test]
async fn a_root_invite_makes_a_root_member_without_news() {
    let site = Site::start().await;
    site.add_invite(CODE, None).await;
    let form: Vec<_> = bob()
        .into_iter()
        .filter(|(name, _)| *name != "news")
        .collect();
    assert_eq!(site.post(INVITE, &form, None).await.status(), 200);
    let member = site.db.member("bob").await.unwrap().unwrap();
    assert_eq!(member.invited_by, None);
    assert!(!member.news_opt_in);
}

#[tokio::test]
async fn an_admin_invite_makes_an_admin() {
    let site = Site::start().await;
    site.add_invite(CODE, None).await;
    site.update_invite(CODE, "makes_admin = TRUE").await;
    assert_eq!(site.post(INVITE, &bob(), None).await.status(), 200);
    assert!(site.db.member("bob").await.unwrap().unwrap().is_admin);
}

#[tokio::test]
async fn mistakes_come_back_with_what_was_typed_but_the_passwords() {
    let site = with_invite_from_alice().await;
    for (form, error) in [
        (
            signup("2pac", NEW_PASSWORD, NEW_PASSWORD),
            "Nicks start with a letter.",
        ),
        (signup("NickServ", NEW_PASSWORD, NEW_PASSWORD), "reserved"),
        (
            signup("bob", "short-one", "short-one"),
            "at least 12 characters",
        ),
        (signup("bob", "qwerty123456", "qwerty123456"), "too common"),
        (
            signup("bob", ":colon-first-pass", ":colon-first-pass"),
            "start with a colon",
        ),
        (
            signup("bob", NEW_PASSWORD, "xylophone-quokka-43"),
            "don&#39;t match",
        ),
    ] {
        let response = site.post(INVITE, &form, None).await;
        assert_eq!(response.status(), 422, "{error}");
        let page = body_text(response).await;
        assert!(page.contains(error), "{error}: {page}");
        assert!(page.contains(r#"value="bob@example.org""#), "{page}");
        assert!(
            page.contains(r#"<input name="news" type="checkbox" checked>"#),
            "{page}"
        );
        assert!(!page.contains(form[2].1), "{page}");
    }
    let mut bad_email = bob();
    bad_email[1].1 = "bob@example";
    let page = body_text(site.post(INVITE, &bad_email, None).await).await;
    assert!(page.contains(r#"value="Bob""#) && page.contains(r#"aria-invalid="true""#));
    assert!(invite_is_open(&site).await);
    assert!(site.ergo.client().accounts().await.unwrap().len() == 1);
}

#[tokio::test]
async fn a_name_ergo_already_has_is_taken_and_the_invite_still_works() {
    let site = with_invite_from_alice().await;
    site.register("bob").await;

    let response = site.post(INVITE, &bob(), None).await;

    assert_eq!(response.status(), 422);
    assert!(body_text(response).await.contains("Bob is taken."));
    assert!(invite_is_open(&site).await);
    assert_eq!(site.db.member("bob").await.unwrap(), None);
    let carol = signup("carol", NEW_PASSWORD, NEW_PASSWORD);
    assert_eq!(site.post(INVITE, &carol, None).await.status(), 200);
}

#[tokio::test]
async fn an_address_another_member_verified_is_refused_in_any_case() {
    let site = with_invite_from_alice().await;
    site.execute(
        "UPDATE members SET email = 'Bob@Example.org', email_verified_at = 1 WHERE account = 'alice'",
    )
    .await;

    let response = site.post(INVITE, &bob(), None).await;

    assert_eq!(response.status(), 422);
    let page = body_text(response).await;
    assert!(
        page.contains(
            r#"<p class="field-error" id="email-hint">Another member uses that address.</p>"#
        ),
        "{page}"
    );
    assert!(page.contains(r#"value="Bob""#), "{page}");
    assert!(invite_is_open(&site).await);
    assert!(site.ergo.client().info("bob").await.unwrap().is_none());
    assert!(site.mail().await.is_empty());
}

#[tokio::test]
async fn an_address_another_member_only_gave_is_still_free() {
    let site = with_invite_from_alice().await;
    site.execute("UPDATE members SET email = 'bob@example.org' WHERE account = 'alice'")
        .await;

    assert_eq!(site.post(INVITE, &bob(), None).await.status(), 200);
}

#[tokio::test]
async fn a_removed_members_name_stays_taken_after_ergo_erased_it() {
    let site = with_invite_from_alice().await;
    site.execute("INSERT INTO members (account, joined_at, removed_at) VALUES ('Bob', 0, 1)")
        .await;

    let response = site
        .post(INVITE, &signup("BOB", NEW_PASSWORD, NEW_PASSWORD), None)
        .await;

    assert_eq!(response.status(), 422);
    assert!(body_text(response).await.contains("BOB is taken."));
    assert!(site.ergo.client().info("bob").await.unwrap().is_none());
    assert!(invite_is_open(&site).await);
}

#[tokio::test]
async fn a_deleted_bots_name_stays_taken_until_its_history_expired() {
    let site = with_invite_from_alice().await;
    site.execute(&format!(
        "INSERT INTO bots (account, owner, created_at, removed_at)
         VALUES ('bob', 'alice', 0, unixepoch() - {HISTORY_EXPIRY} + 60)"
    ))
    .await;

    let response = site.post(INVITE, &bob(), None).await;

    assert_eq!(response.status(), 422);
    assert!(body_text(response).await.contains("Bob is taken."));
    assert!(site.ergo.client().info("bob").await.unwrap().is_none());
    assert!(invite_is_open(&site).await);
}

#[tokio::test]
async fn a_deleted_bots_name_is_anyones_once_its_history_expired() {
    let site = with_invite_from_alice().await;
    site.execute(&format!(
        "INSERT INTO bots (account, owner, created_at, removed_at)
         VALUES ('bob', 'alice', 0, unixepoch() - {HISTORY_EXPIRY})"
    ))
    .await;

    let response = site.post(INVITE, &bob(), None).await;

    assert_eq!(response.status(), 200);
    assert!(site.ergo.client().info("bob").await.unwrap().is_some());
    assert!(site.db.member("bob").await.unwrap().is_some());
    assert!(!invite_is_open(&site).await);
}

#[tokio::test]
async fn when_ergo_fails_nothing_is_created_and_the_invite_still_works() {
    let site = with_invite_from_alice().await;
    site.ergo.fail_next(Failure::UnknownError);

    let response = site.post(INVITE, &bob(), None).await;

    assert_eq!(response.status(), 503);
    let page = body_text(response).await;
    assert!(
        page.contains("Couldn&#39;t reach the chat server."),
        "{page}"
    );
    assert!(page.contains("your invite still works"), "{page}");
    assert_eq!(site.db.member("bob").await.unwrap(), None);
    assert_eq!(site.post(INVITE, &bob(), None).await.status(), 200);
}

#[tokio::test]
async fn when_ergo_is_unreachable_the_invite_still_works() {
    let unreachable = ergo_api::Client::new("http://127.0.0.1:9".parse().unwrap(), "token");
    let site = Site::start_with(|app| app.ergo = unreachable).await;
    site.add_invite(CODE, None).await;
    assert_eq!(site.post(INVITE, &bob(), None).await.status(), 503);
    assert!(invite_is_open(&site).await);
}

#[tokio::test]
async fn two_redemptions_of_one_invite_cannot_both_succeed() {
    let site = with_invite_from_alice().await;
    let (bob, carol) = (bob(), signup("carol", NEW_PASSWORD, NEW_PASSWORD));

    let (first, second) = tokio::join!(
        site.post(INVITE, &bob, None),
        site.post(INVITE, &carol, None)
    );

    let mut statuses = [first.status().as_u16(), second.status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 422]);
    assert_eq!(site.db.member_count().await.unwrap(), 2);
}

/// A site whose Ergo creates every account, after running `sql` on the
/// site's database.
async fn with_ergo_that_runs(sql: &'static str) -> Site {
    let database = Arc::new(OnceLock::<PathBuf>::new());
    let running = database.clone();
    let ergo = Router::new().route(
        "/v1/ns/saregister",
        post(move || async move {
            execute(running.get().unwrap(), sql).await;
            Json(json!({"success": true}))
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, ergo).await });
    let ergo = ergo_api::Client::new(url.parse().unwrap(), "t");
    let site = Site::start_with(|app| app.ergo = ergo).await;
    database.set(site.database_path()).unwrap();
    site.add_invite(CODE, None).await;
    site
}

#[tokio::test]
async fn an_invite_revoked_while_ergo_works_adds_no_member_and_says_so() {
    let site = with_ergo_that_runs("UPDATE invites SET revoked_at = 1").await;

    let response = site.post(INVITE, &bob(), None).await;

    assert_eq!(response.status(), 500);
    let page = body_text(response).await;
    assert!(page.contains("setup didn&#39;t finish"), "{page}");
    assert!(page.contains("clear the nick for a new invite"), "{page}");
    assert_eq!(site.db.member("bob").await.unwrap(), None);
}

#[tokio::test]
async fn failing_to_record_the_member_after_ergo_said_yes_says_so() {
    let site = with_ergo_that_runs(
        "INSERT INTO members (account, email, joined_at) VALUES ('bob', 'bob@example.org', 0)",
    )
    .await;

    let response = site.post(INVITE, &bob(), None).await;

    assert_eq!(response.status(), 500);
    assert!(
        body_text(response)
            .await
            .contains("setup didn&#39;t finish")
    );
    assert!(invite_is_open(&site).await);
}

#[tokio::test]
async fn a_signed_in_member_redeems_nothing() {
    let site = with_invite_from_alice().await;
    let cookie = site.sign_in("alice").await;
    let response = site.post(INVITE, &bob(), Some(&cookie)).await;
    assert!(
        body_text(response)
            .await
            .contains("You already have an account.")
    );
    assert_eq!(site.db.member("bob").await.unwrap(), None);
}

#[tokio::test]
async fn an_unusable_invite_redeems_nothing() {
    let site = Site::start().await;
    let response = site.post(INVITE, &bob(), None).await;
    assert_eq!(response.status(), 422);
    assert!(
        body_text(response)
            .await
            .contains("This invite doesn&#39;t work.")
    );
    assert_eq!(site.db.member("bob").await.unwrap(), None);
}

#[tokio::test]
async fn signing_up_from_another_origin_is_refused() {
    let site = with_invite_from_alice().await;
    let headers = [(header::ORIGIN.as_str(), "https://evil.example")];
    let response = site.post_with(INVITE, &bob(), None, &headers).await;
    assert_eq!(response.status(), 403);
    assert!(invite_is_open(&site).await);
}

#[tokio::test]
async fn the_welcome_page_leads_with_clients_for_the_visitors_platform() {
    let iphone = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15";
    for (user_agent, picks, not) in [
        (Some(iphone), &["Goguma", "Mango"][..], Some("Halloy")),
        (
            None,
            &["Halloy", "Goguma", "Mango", "senpai", "WeeChat"][..],
            None,
        ),
    ] {
        let site = with_invite_from_alice().await;
        let mut headers = vec![(header::ORIGIN.as_str(), crate::PUBLIC_URL)];
        headers.extend(user_agent.map(|agent| (header::USER_AGENT.as_str(), agent)));
        let page = body_text(site.post_with(INVITE, &bob(), None, &headers).await).await;
        for pick in picks {
            assert!(
                page.contains(&format!(">{pick}</a></h3>")),
                "{pick}: {page}"
            );
        }
        if let Some(not) = not {
            assert!(!page.contains(&format!(">{not}</a></h3>")), "{page}");
        }
    }
}

#[tokio::test]
async fn signup_attempts_count_as_nick_lookups() {
    let site = with_invite_from_alice().await;
    let headers = [
        (header::ORIGIN.as_str(), crate::PUBLIC_URL),
        ("x-real-ip", "192.0.2.7"),
    ];
    for _ in 0..concierge::rate_limit::LOOKUPS_PER_ADDRESS {
        site.post_with("/signup/nick", &[("nick", "bob")], None, &headers)
            .await;
    }
    let response = site.post_with(INVITE, &bob(), None, &headers).await;
    assert_eq!(response.status(), 429);
    assert!(body_text(response).await.contains("Too many attempts."));
    assert!(invite_is_open(&site).await);
}
