use axum::http::header;
use concierge::{name::MemberName, oauth::CODE_LIFETIME};
use serde_json::Value;

use base64::{Engine, engine::general_purpose::STANDARD};

use crate::{BETA_URL, INTROSPECTION_SECRET, PUBLIC_URL, Site, WEBCHAT_URL, body_text, location};

impl Site {
    /// Asks for a code as gamja does, with the session in `cookie`.
    async fn authorization_code(&self, cookie: &str) -> String {
        let response = self.get(&authorize_path(&[]), Some(cookie)).await;
        code_in(location(&response))
    }

    /// Posts to the token endpoint from gamja's page, with changes to what
    /// gamja sends.
    async fn exchange(&self, code: &str, changes: &[(&str, &str)]) -> axum::response::Response {
        self.exchange_from(code, changes, WEBCHAT_URL).await
    }

    /// A member's access token, the way gamja gets one.
    pub(crate) async fn access_token(&self, cookie: &str) -> String {
        let code = self.authorization_code(cookie).await;
        let answer = json(self.exchange(&code, &[]).await).await;
        answer["access_token"].as_str().unwrap().to_owned()
    }

    /// Introspects `token` the way Ergo does, with these credentials.
    async fn introspect_as(&self, credentials: &str, token: &str) -> axum::response::Response {
        let basic = format!("Basic {}", STANDARD.encode(credentials));
        let headers = [(header::AUTHORIZATION.as_str(), basic.as_str())];
        self.post_with("/oauth/introspect", &[("token", token)], None, &headers)
            .await
    }

    pub(crate) async fn introspect(&self, token: &str) -> Value {
        let response = self
            .introspect_as(&format!("ergo:{INTROSPECTION_SECRET}"), token)
            .await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        json(response).await
    }

    async fn exchange_from(
        &self,
        code: &str,
        changes: &[(&str, &str)],
        origin: &str,
    ) -> axum::response::Response {
        let headers = [(header::ORIGIN.as_str(), origin)];
        self.exchange_with(code, changes, &headers).await
    }

    async fn exchange_with(
        &self,
        code: &str,
        changes: &[(&str, &str)],
        headers: &[(&str, &str)],
    ) -> axum::response::Response {
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
            ("client_id", "gamja"),
            ("code_verifier", VERIFIER),
        ];
        for (name, value) in changes {
            form.retain(|(field, _)| field != name);
            form.push((name, value));
        }
        self.post_with("/oauth/token", &form, None, headers).await
    }
}

async fn json(response: axum::response::Response) -> Value {
    serde_json::from_str(&body_text(response).await).unwrap()
}

/// RFC 7636, appendix B.
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
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
    code_sent_to(REDIRECT_URI, redirect)
}

/// The code in a redirect back to `redirect_uri`, checking the state came
/// along.
fn code_sent_to(redirect_uri: &str, redirect: &str) -> String {
    let query = redirect
        .strip_prefix(&format!("{redirect_uri}?"))
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
    assert_eq!(location(&response), crate::sign_in_url(&path));
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

#[tokio::test]
async fn a_code_and_its_verifier_buy_a_token_that_lasts_as_long_as_the_session() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let code = site.authorization_code(&cookie).await;

    let response = site.exchange(&code, &[]).await;

    assert_eq!(response.status(), 200);
    let headers = response.headers();
    assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], WEBCHAT_URL);
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    let answer = json(response).await;
    assert_eq!(answer["token_type"], "Bearer");
    assert_eq!(answer["access_token"].as_str().unwrap().len(), 43);
    let stored: i64 = site.query_scalar("SELECT COUNT(*) FROM oauth_tokens").await;
    assert_eq!(stored, 1);
    let expires_at: i64 = site
        .query_scalar("SELECT expires_at FROM oauth_tokens")
        .await;
    assert_eq!(Some(expires_at), site.session_expiry(&cookie).await);
    let lifetime: i64 = site
        .query_scalar("SELECT expires_at - created_at FROM oauth_tokens")
        .await;
    assert_eq!(answer["expires_in"], lifetime, "{answer}");
}

#[tokio::test]
async fn trading_codes_shares_the_per_address_limit_with_asking_for_them() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let code = site.authorization_code(&cookie).await;
    let path = authorize_path(&[]);
    for _ in 0..concierge::rate_limit::OAUTH_REQUESTS_PER_ADDRESS {
        site.get_with(&path, &cookie, "192.0.2.7").await;
    }
    let from = |address| {
        [
            (header::ORIGIN.as_str(), WEBCHAT_URL),
            ("x-real-ip", address),
        ]
    };
    let limited = site.exchange_with(&code, &[], &from("192.0.2.7")).await;
    assert_eq!(limited.status(), 429);
    assert_eq!(limited.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        json(limited).await,
        serde_json::json!({"error": "invalid_request"})
    );
    let elsewhere = site.exchange_with(&code, &[], &from("198.51.100.9")).await;
    assert_eq!(elsewhere.status(), 200);
}

#[tokio::test]
async fn a_code_works_once_even_after_a_failed_try() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let code = site.authorization_code(&cookie).await;
    assert_eq!(site.exchange(&code, &[]).await.status(), 200);
    let reused = site.exchange(&code, &[]).await;
    assert_eq!(reused.status(), 400);
    assert_eq!(reused.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        json(reused).await,
        serde_json::json!({"error": "invalid_grant"})
    );

    let code = site.authorization_code(&cookie).await;
    let wrong = "eBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    assert_eq!(
        site.exchange(&code, &[("code_verifier", wrong)])
            .await
            .status(),
        400
    );
    assert_eq!(site.exchange(&code, &[]).await.status(), 400);
}

#[tokio::test]
async fn a_code_is_bound_to_its_verifier_redirect_and_client() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for (change, status, error) in [
        (
            (
                "code_verifier",
                "eBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            ),
            400,
            "invalid_grant",
        ),
        (("code_verifier", ""), 400, "invalid_grant"),
        (
            ("redirect_uri", "https://evil.example/"),
            400,
            "invalid_grant",
        ),
        (("client_id", "ergo"), 401, "invalid_client"),
        (
            ("grant_type", "refresh_token"),
            400,
            "unsupported_grant_type",
        ),
    ] {
        let code = site.authorization_code(&cookie).await;
        let response = site.exchange(&code, &[change]).await;
        assert_eq!(response.status(), status, "{change:?}");
        assert_eq!(json(response).await["error"], error, "{change:?}");
    }
    let made_up = site
        .exchange("q7Hx2mZp9vLcT4wRfBa-_wq7Hx2mZp9vLcT4wRfBa-_", &[])
        .await;
    assert_eq!(made_up.status(), 400);
    let stored: i64 = site.query_scalar("SELECT COUNT(*) FROM oauth_tokens").await;
    assert_eq!(stored, 0);
}

#[tokio::test]
async fn an_expired_code_buys_nothing() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let code = site.authorization_code(&cookie).await;
    site.execute(&format!(
        "UPDATE oauth_codes SET expires_at = expires_at - {CODE_LIFETIME}"
    ))
    .await;
    assert_eq!(site.exchange(&code, &[]).await.status(), 400);
}

#[tokio::test]
async fn a_code_dies_with_the_session_that_approved_it() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let code = site.authorization_code(&cookie).await;
    site.post("/signout", &[], Some(&cookie)).await;
    assert_eq!(site.exchange(&code, &[]).await.status(), 400);
}

#[tokio::test]
async fn only_the_web_client_may_post_to_the_token_endpoint() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let code = site.authorization_code(&cookie).await;
    for origin in ["https://evil.example", PUBLIC_URL, "null"] {
        let response = site.exchange_from(&code, &[], origin).await;
        assert_eq!(response.status(), 403, "{origin}");
        assert!(
            !response
                .headers()
                .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            "{origin}"
        );
    }
    let response = site.post_with("/oauth/token", &[], None, &[]).await;
    assert_eq!(response.status(), 403);
    assert_eq!(site.exchange(&code, &[]).await.status(), 200);
}

const BETA_REDIRECT_URI: &str = "https://beta.weblounge.org/";
const BETA_ID: &str = "weblounge-client";

impl Site {
    /// Asks for a code as the beta client does, with the session in
    /// `cookie`.
    async fn beta_authorization_code(&self, cookie: &str) -> String {
        let path = authorize_path(&[
            ("client_id", Some(BETA_ID)),
            ("redirect_uri", Some(BETA_REDIRECT_URI)),
        ]);
        let response = self.get(&path, Some(cookie)).await;
        code_sent_to(BETA_REDIRECT_URI, location(&response))
    }

    /// Posts to the token endpoint from the beta client's page, as it does.
    async fn beta_exchange(&self, code: &str) -> axum::response::Response {
        let form = [("client_id", BETA_ID), ("redirect_uri", BETA_REDIRECT_URI)];
        self.exchange_from(code, &form, BETA_URL).await
    }
}

#[tokio::test]
async fn the_beta_client_signs_members_in_from_its_own_origin() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let metadata = site
        .get_from("/.well-known/oauth-authorization-server", BETA_URL)
        .await;
    assert_eq!(
        metadata.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        BETA_URL
    );
    let code = site.beta_authorization_code(&cookie).await;

    let response = site.beta_exchange(&code).await;

    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        BETA_URL
    );
    let token = json(response).await["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let answer = site.introspect(&token).await;
    assert_eq!(answer["username"], "alice");
    assert_eq!(answer["client_id"], BETA_ID);
}

#[tokio::test]
async fn a_code_is_redeemed_only_by_the_client_it_was_issued_to() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;

    let gamjas = site.authorization_code(&cookie).await;
    let response = site.beta_exchange(&gamjas).await;
    assert_eq!(response.status(), 400);
    assert_eq!(json(response).await["error"], "invalid_grant");

    let betas = site.beta_authorization_code(&cookie).await;
    let response = site.exchange(&betas, &[]).await;
    assert_eq!(response.status(), 400);
    assert_eq!(json(response).await["error"], "invalid_grant");

    let stored: i64 = site.query_scalar("SELECT COUNT(*) FROM oauth_tokens").await;
    assert_eq!(stored, 0);
}

#[tokio::test]
async fn our_own_pages_cannot_post_from_the_web_clients_origin() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let headers = [(header::ORIGIN.as_str(), WEBCHAT_URL)];
    let form = [("nick", "alice"), ("password", crate::PASSWORD)];
    let response = site.post_with("/signin", &form, None, &headers).await;
    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn ergo_learns_whose_token_it_is() {
    let site = Site::start().await;
    site.add_member("Alice").await;
    let cookie = site.sign_in("alice").await;
    let token = site.access_token(&cookie).await;

    let answer = site.introspect(&token).await;

    let expiry = site.session_expiry(&cookie).await.unwrap();
    assert_eq!(
        answer,
        serde_json::json!({
            "active": true,
            "username": "Alice",
            "client_id": "gamja",
            "token_type": "Bearer",
            "exp": expiry,
        })
    );
}

#[tokio::test]
async fn unknown_garbage_and_expired_tokens_are_inactive() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let token = site.access_token(&cookie).await;
    let inactive = serde_json::json!({"active": false});
    for other in ["", "garbage", "q7Hx2mZp9vLcT4wRfBa-_wq7Hx2mZp9vLcT4wRfBa-_"] {
        assert_eq!(site.introspect(other).await, inactive, "{other}");
    }
    let code = site.authorization_code(&cookie).await;
    assert_eq!(site.introspect(&code).await, inactive);
    site.execute("UPDATE oauth_tokens SET expires_at = 1").await;
    assert_eq!(site.introspect(&token).await, inactive);
}

#[tokio::test]
async fn a_token_dies_with_its_session_or_its_member() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.add_member("bob").await;
    let alices = site.sign_in("alice").await;
    let bobs = site.sign_in("bob").await;
    let (alice_token, bob_token) = (
        site.access_token(&alices).await,
        site.access_token(&bobs).await,
    );
    site.post("/signout", &[], Some(&alices)).await;
    assert_eq!(site.introspect(&alice_token).await["active"], false);
    site.db
        .remove_member(&MemberName::new("bob"), concierge::db::now())
        .await
        .unwrap();
    assert_eq!(site.introspect(&bob_token).await["active"], false);
}

#[tokio::test]
async fn only_ergo_with_its_secret_may_introspect() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let token = site.access_token(&cookie).await;
    for credentials in [
        "ergo:a wrong secret".to_owned(),
        "ergo:".to_owned(),
        format!("gamja:{INTROSPECTION_SECRET}"),
        format!("ergo:{INTROSPECTION_SECRET} "),
    ] {
        let response = site.introspect_as(&credentials, &token).await;
        assert_eq!(response.status(), 401, "{credentials}");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            response.headers()[header::WWW_AUTHENTICATE],
            "Basic realm=\"concierge\""
        );
        assert_eq!(
            json(response).await,
            serde_json::json!({"error": "invalid_client"})
        );
    }
    let anonymous = site
        .post_with("/oauth/introspect", &[("token", &token)], None, &[])
        .await;
    assert_eq!(anonymous.status(), 401);
    let from_gamja = site
        .post_with(
            "/oauth/introspect",
            &[("token", &token)],
            None,
            &[(header::ORIGIN.as_str(), WEBCHAT_URL)],
        )
        .await;
    assert_eq!(from_gamja.status(), 401);
    assert!(
        !from_gamja
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    );
}
