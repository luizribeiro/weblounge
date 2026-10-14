use axum::http::header;
use concierge::oauth::CODE_LIFETIME;
use serde_json::Value;

use crate::{PUBLIC_URL, Site, WEBCHAT_URL, body_text, location};

/// RFC 7636, appendix B.
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const STATE: &str = "q7Hx2mZp9vLcT4wRfBa-_w";
const REDIRECT_URI: &str = "https://irc.weblounge.org/";

/// The authorization request gamja makes, with some parameters replaced
/// or, given as None, left out.
fn authorize_path(changes: &[(&str, Option<&str>)]) -> String {
    let mut params = vec![
        ("response_type", "code"),
        ("client_id", "gamja"),
        ("redirect_uri", REDIRECT_URI),
        ("state", STATE),
        ("code_challenge", CHALLENGE),
        ("code_challenge_method", "S256"),
    ];
    for (name, value) in changes {
        params.retain(|(param, _)| param != name);
        if let Some(value) = value {
            params.push((name, value));
        }
    }
    let query = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish();
    format!("/oauth/authorize?{query}")
}

/// The code in a redirect back to gamja, checking the state came along.
fn code_in(redirect: &str) -> String {
    let query = redirect
        .strip_prefix(&format!("{REDIRECT_URI}?"))
        .unwrap_or_else(|| panic!("{redirect}"));
    let params: Vec<(String, String)> = form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    assert_eq!(params.len(), 2, "{redirect}");
    assert_eq!(params[1], ("state".into(), STATE.into()), "{redirect}");
    let (name, code) = &params[0];
    assert_eq!(name, "code", "{redirect}");
    code.clone()
}

#[tokio::test]
async fn publishes_its_metadata_to_the_web_client() {
    let site = Site::start().await;
    let response = site
        .get_from("/.well-known/oauth-authorization-server", WEBCHAT_URL)
        .await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        WEBCHAT_URL
    );
    let metadata: Value = serde_json::from_str(&body_text(response).await).unwrap();
    assert_eq!(
        metadata,
        serde_json::json!({
            "issuer": PUBLIC_URL,
            "authorization_endpoint": format!("{PUBLIC_URL}/oauth/authorize"),
            "token_endpoint": format!("{PUBLIC_URL}/oauth/token"),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["none"],
        })
    );
}

#[tokio::test]
async fn other_sites_cannot_read_the_metadata() {
    let site = Site::start().await;
    for origin in ["https://evil.example", PUBLIC_URL, "null"] {
        let response = site
            .get_from("/.well-known/oauth-authorization-server", origin)
            .await;
        assert_eq!(response.status(), 200);
        let headers = response.headers();
        assert!(
            !headers.contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            "{origin}"
        );
        assert_eq!(headers[header::VARY], "origin");
    }
}

#[tokio::test]
async fn a_signed_out_visitor_signs_in_and_comes_back() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let path = authorize_path(&[]);

    let response = site.get(&path, None).await;

    assert_eq!(response.status(), 303);
    let sign_in = location(&response).to_owned();
    let next: String = form_urlencoded::byte_serialize(path.as_bytes()).collect();
    assert_eq!(sign_in, format!("/signin?next={next}"));
    let form = [
        ("nick", "alice"),
        ("password", crate::PASSWORD),
        ("next", path.as_str()),
    ];
    let response = site.post("/signin", &form, None).await;
    assert_eq!(location(&response), path);
}

#[tokio::test]
async fn a_signed_in_member_gets_a_code_without_being_asked() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;

    let response = site.get(&authorize_path(&[]), Some(&cookie)).await;

    assert_eq!(response.status(), 303);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let code = code_in(location(&response));
    assert_eq!(code.len(), 43);
    let other = code_in(location(
        &site.get(&authorize_path(&[]), Some(&cookie)).await,
    ));
    assert_ne!(code, other);
}

#[tokio::test]
async fn codes_are_stored_hashed_and_expire_within_a_minute() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let before = concierge::db::now();
    let code = code_in(location(
        &site.get(&authorize_path(&[]), Some(&cookie)).await,
    ));
    let database = std::fs::read(site.database_path()).unwrap();
    let wal = std::fs::read(site.database_path().with_extension("db-wal")).unwrap_or_default();
    for bytes in [&database, &wal] {
        assert!(!bytes.windows(code.len()).any(|w| w == code.as_bytes()));
    }
    let expires_at: i64 = site
        .query_scalar("SELECT expires_at FROM oauth_codes")
        .await;
    assert!((before + CODE_LIFETIME..=concierge::db::now() + CODE_LIFETIME).contains(&expires_at));
}

#[tokio::test]
async fn unknown_clients_and_redirects_get_an_error_page_not_a_redirect() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for change in [
        ("client_id", Some("evil")),
        ("client_id", None),
        ("redirect_uri", Some("https://evil.example/")),
        (
            "redirect_uri",
            Some("https://irc.weblounge.org/?next=//evil.example"),
        ),
        ("redirect_uri", None),
    ] {
        let response = site.get(&authorize_path(&[change]), Some(&cookie)).await;
        assert_eq!(response.status(), 400, "{change:?}");
        assert!(!response.headers().contains_key(header::LOCATION));
        let page = body_text(response).await;
        assert!(
            page.contains("This sign-in link isn&#39;t one of ours."),
            "{page}"
        );
    }
}

#[tokio::test]
async fn requests_without_pkce_or_state_go_back_with_an_error_and_no_code() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for (change, redirect) in [
        (
            ("code_challenge", None),
            format!("{REDIRECT_URI}?error=invalid_request&state={STATE}"),
        ),
        (
            ("code_challenge_method", Some("plain")),
            format!("{REDIRECT_URI}?error=invalid_request&state={STATE}"),
        ),
        (
            ("state", None),
            format!("{REDIRECT_URI}?error=invalid_request"),
        ),
        (
            ("response_type", Some("token")),
            format!("{REDIRECT_URI}?error=unsupported_response_type&state={STATE}"),
        ),
    ] {
        let response = site.get(&authorize_path(&[change]), Some(&cookie)).await;
        assert_eq!(location(&response), redirect, "{change:?}");
    }
    let codes: i64 = site.query_scalar("SELECT COUNT(*) FROM oauth_codes").await;
    assert_eq!(codes, 0);
}

#[tokio::test]
async fn an_address_gets_a_limited_number_of_codes() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let path = authorize_path(&[]);
    for _ in 0..concierge::rate_limit::OAUTH_REQUESTS_PER_ADDRESS {
        let response = site.get_with(&path, &cookie, "192.0.2.7").await;
        code_in(location(&response));
    }
    let response = site.get_with(&path, &cookie, "192.0.2.7").await;
    assert_eq!(
        location(&response),
        format!("{REDIRECT_URI}?error=temporarily_unavailable&state={STATE}")
    );
    let response = site.get_with(&path, &cookie, "198.51.100.9").await;
    code_in(location(&response));
}

#[tokio::test]
async fn asking_for_a_code_renews_the_session() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    site.execute("UPDATE sessions SET expires_at = expires_at - 2 * 86400")
        .await;
    let before = site.session_expiry(&cookie).await.unwrap();
    let response = site.get(&authorize_path(&[]), Some(&cookie)).await;
    code_in(location(&response));
    assert!(site.session_expiry(&cookie).await.unwrap() > before);
}
