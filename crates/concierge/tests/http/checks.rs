use axum::http::header;
use concierge::{
    password_rules::{self, MAX_BYTES, MIN_LENGTH},
    rate_limit::LOOKUPS_PER_ADDRESS,
};
use serde_json::Value;

use crate::{PUBLIC_URL, Site, body_text};

async fn rules(site: &Site) -> Value {
    let response = site.get("/password-rules.json", None).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    serde_json::from_str(&body_text(response).await).unwrap()
}

/// What signup.ts does with the rules, for passwords that match their
/// confirmation.
fn page_accepts(rules: &Value, password: &str) -> bool {
    let min = rules["minLength"].as_u64().unwrap() as usize;
    let max = rules["maxBytes"].as_u64().unwrap() as usize;
    let common = rules["common"].as_array().unwrap();
    password.chars().count() >= min
        && password.len() <= max
        && !password.chars().any(char::is_whitespace)
        && !password.starts_with(':')
        && !common.contains(&password.to_lowercase().into())
}

#[tokio::test]
async fn the_page_gets_the_rules_the_server_checks() {
    let rules = rules(&Site::start().await).await;
    assert_eq!(rules["minLength"], MIN_LENGTH);
    assert_eq!(rules["maxBytes"], MAX_BYTES);
    let common = rules["common"].as_array().unwrap();
    assert!(common.len() > 1000);
    let long = "x".repeat(MAX_BYTES + 1);
    let samples = [
        "xylophone-quokka-42",
        "QWERTY123456",
        "short-one",
        "has a space in it",
        ":colon-first-pass",
        "ëëëëëëëëëëëë",
        long.as_str(),
    ];
    let listed = common.iter().map(|password| password.as_str().unwrap());
    for password in samples.into_iter().chain(listed) {
        assert_eq!(
            page_accepts(&rules, password),
            password_rules::check(password, password).is_ok(),
            "{password:?}"
        );
    }
}

async fn lookup(site: &Site, nick: &str, address: &str) -> (u16, String) {
    let headers = [
        (header::ORIGIN.as_str(), PUBLIC_URL),
        ("x-real-ip", address),
    ];
    let response = site
        .post_with("/signup/nick", &[("nick", nick)], None, &headers)
        .await;
    (response.status().as_u16(), body_text(response).await)
}

fn status(answer: &str) -> String {
    format!(r#"{{"status":"{answer}"}}"#)
}

#[tokio::test]
async fn says_whether_a_nick_is_free_and_nothing_more() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.register("ergo-only").await;
    site.execute("INSERT INTO members (account, joined_at, removed_at) VALUES ('gone', 0, 1)")
        .await;
    for (nick, answer) in [
        ("bob", "available"),
        ("ALICE", "taken"),
        ("ergo-only", "taken"),
        ("Gone", "taken"),
        ("2pac", "invalid"),
        ("NickServ", "invalid"),
        ("", "invalid"),
    ] {
        assert_eq!(
            lookup(&site, nick, "192.0.2.7").await,
            (200, status(answer)),
            "{nick}"
        );
    }
}

#[tokio::test]
async fn an_address_gets_a_limited_number_of_lookups() {
    let site = Site::start().await;
    for _ in 0..LOOKUPS_PER_ADDRESS {
        assert_eq!(lookup(&site, "bob", "192.0.2.7").await.0, 200);
    }
    assert_eq!(lookup(&site, "bob", "192.0.2.7").await.0, 429);
    assert_eq!(lookup(&site, "bob", "198.51.100.9").await.0, 200);
}

async fn invite_lookup(site: &Site, code: &str, origin: &str) -> (u16, String) {
    let headers = [
        (header::ORIGIN.as_str(), origin),
        ("x-real-ip", "192.0.2.7"),
    ];
    let response = site
        .post_with("/signup/invite", &[("code", code)], None, &headers)
        .await;
    (response.status().as_u16(), body_text(response).await)
}

#[tokio::test]
async fn says_who_sent_an_open_invite_and_why_others_wont_work() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.add_invite("q7Hx2mZp9vLcT4wRfBa-_w", Some("alice"))
        .await;
    site.add_invite("rootRootRootRootRoot-_", None).await;
    site.add_invite("usedUsedUsedUsedUsed-_", Some("alice"))
        .await;
    site.update_invite("usedUsedUsedUsedUsed-_", "revoked_at = 1")
        .await;
    let from_alice = r#"{"status":"open","inviter":"alice"}"#;
    let unusable = r#"{"status":"unusable","error":"This invite doesn't work. Invites work once and expire after 24 hours; ask for a new one."}"#;
    for (code, answer) in [
        ("q7Hx2mZp9vLcT4wRfBa-_w", from_alice),
        (
            "https://members.weblounge.org/invite#q7Hx2mZp9vLcT4wRfBa-_w",
            from_alice,
        ),
        (
            "rootRootRootRootRoot-_",
            r#"{"status":"open","inviter":null}"#,
        ),
        ("usedUsedUsedUsedUsed-_", unusable),
        ("unknown", unusable),
        ("", unusable),
    ] {
        assert_eq!(
            invite_lookup(&site, code, PUBLIC_URL).await,
            (200, answer.to_owned()),
            "{code}"
        );
    }
}

#[tokio::test]
async fn invite_lookups_share_the_limit_and_refuse_other_origins() {
    let site = Site::start().await;
    for _ in 0..LOOKUPS_PER_ADDRESS {
        lookup(&site, "bob", "192.0.2.7").await;
    }
    assert_eq!(invite_lookup(&site, "x", PUBLIC_URL).await.0, 429);
    assert_eq!(
        invite_lookup(&site, "x", "https://evil.example").await.0,
        403
    );
}

#[tokio::test]
async fn lookups_from_other_origins_are_refused() {
    let site = Site::start().await;
    let headers = [(header::ORIGIN.as_str(), "https://evil.example")];
    let response = site
        .post_with("/signup/nick", &[("nick", "bob")], None, &headers)
        .await;
    assert_eq!(response.status(), 403);
}
