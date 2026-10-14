use axum::http::header;
use concierge::password_rules::{self, MAX_BYTES, MIN_LENGTH};
use serde_json::Value;

use crate::{Site, body_text};

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
