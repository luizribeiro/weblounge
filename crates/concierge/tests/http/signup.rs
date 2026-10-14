use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use axum::{Json, Router, http::header, routing::post};
use ergo_api::fake::Failure;
use serde_json::json;
use tokio::net::TcpListener;

use crate::{Site, body_text, execute, session_cookie};

const CODE: &str = "q7Hx2mZp9vLcT4wRfBa-_w";
const INVITE: &str = "/invite/q7Hx2mZp9vLcT4wRfBa-_w";
const NEW_PASSWORD: &str = "xylophone-quokka-42";

fn signup<'a>(nick: &'a str, password: &'a str, confirmation: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        ("nick", nick),
        ("email", "bob@example.org"),
        ("password", password),
        ("confirmation", confirmation),
        ("news", "on"),
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
    let response = site.get(INVITE, None).await;
    response.status() == 200
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
    assert_eq!(member.account, "Bob");
    assert_eq!(member.invited_by.as_deref(), Some("alice"));
    assert_eq!(member.email.as_deref(), Some("bob@example.org"));
    assert!(member.news_opt_in);
    let invite = site.db.invite(CODE).await.unwrap().unwrap();
    assert_eq!(invite.redeemed_by.as_deref(), Some("Bob"));
    let ergo = site.ergo.client();
    let account = ergo.check_auth("bob", NEW_PASSWORD).await.unwrap();
    assert_eq!(account.unwrap().as_str(), "Bob");

    let home = body_text(site.get("/", Some(&cookie)).await).await;
    assert!(home.contains("<h1>Bob</h1>"), "{home}");
    assert!(!invite_is_open(&site).await);
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
async fn a_removed_members_name_stays_taken_after_ergo_erased_it() {
    let site = with_invite_from_alice().await;
    site.execute(
        "INSERT INTO members (account, joined_at, invite_quota, removed_at) VALUES ('Bob', 0, 0, 1)",
    )
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
    let site = Site::start_with_ergo(Some(unreachable)).await;
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
    assert_eq!(statuses, [200, 404]);
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
    let site = Site::start_with_ergo(Some(ergo_api::Client::new(url.parse().unwrap(), "t"))).await;
    database.set(site.database_path()).unwrap();
    site.add_invite(CODE, None).await;
    site
}

#[tokio::test]
async fn an_invite_revoked_while_ergo_works_adds_no_member_and_says_so() {
    let site = with_ergo_that_runs("UPDATE invites SET revoked_at = 1").await;

    let response = site.post(INVITE, &bob(), None).await;

    assert_eq!(response.status(), 500);
    assert!(
        body_text(response)
            .await
            .contains("setup didn&#39;t finish")
    );
    assert_eq!(site.db.member("bob").await.unwrap(), None);
}

#[tokio::test]
async fn failing_to_record_the_member_after_ergo_said_yes_says_so() {
    let site = with_ergo_that_runs(
        "INSERT INTO members (account, joined_at, invite_quota) VALUES ('bob', 0, 5)",
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
    assert_eq!(site.post(INVITE, &bob(), None).await.status(), 404);
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
