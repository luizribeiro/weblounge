use axum::{
    body::Body,
    http::{Request, header},
    response::Response,
};
use concierge::{oauth::SESSION_TOKEN_LIFETIME, rate_limit::OAUTH_REQUESTS_PER_ADDRESS};
use serde_json::json;

use crate::{BETA_URL, PUBLIC_URL, Site, WEBCHAT_URL, oauth::json};

const PATH: &str = "/oauth/session-token";
const JSON: (&str, &str) = ("content-type", "application/json");

fn from(origin: &str) -> [(&str, &str); 2] {
    [("origin", origin), JSON]
}

impl Site {
    /// Posts to the session-token endpoint with the session in `cookie`
    /// and `headers`; the beta client's page sends [`from`] its origin.
    async fn session_token_with(&self, cookie: Option<&str>, headers: &[(&str, &str)]) -> Response {
        let mut request = Request::post(PATH);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        self.send(request.body(Body::from("{}")).unwrap()).await
    }

    async fn stored_tokens(&self) -> i64 {
        self.query_scalar("SELECT COUNT(*) FROM oauth_tokens").await
    }
}

fn header_of(response: &Response, name: header::HeaderName) -> Option<&str> {
    response
        .headers()
        .get(name)
        .map(|value| value.to_str().unwrap())
}

fn assert_readable_by(response: &Response, origin: &str) {
    let granted = |name| header_of(response, name);
    assert_eq!(granted(header::ACCESS_CONTROL_ALLOW_ORIGIN), Some(origin));
    assert_eq!(
        granted(header::ACCESS_CONTROL_ALLOW_CREDENTIALS),
        Some("true")
    );
    assert_eq!(granted(header::VARY), Some("origin"));
}

fn assert_unreadable(response: &Response) {
    let cors: Vec<_> = response
        .headers()
        .keys()
        .filter(|name| name.as_str().starts_with("access-control-"))
        .collect();
    assert!(cors.is_empty(), "{cors:?}");
}

#[tokio::test]
async fn a_signed_in_member_gets_a_short_lived_token_from_the_beta_clients_page() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;

    let response = site
        .session_token_with(Some(&cookie), &from(BETA_URL))
        .await;

    assert_eq!(response.status(), 200);
    assert_readable_by(&response, BETA_URL);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let answer = json(response).await;
    assert_eq!(answer["token_type"], "Bearer");
    assert_eq!(answer["expires_in"], SESSION_TOKEN_LIFETIME);
    let token = answer["access_token"].as_str().unwrap();
    let lifetime: i64 = site
        .query_scalar("SELECT expires_at - created_at FROM oauth_tokens")
        .await;
    assert_eq!(lifetime, SESSION_TOKEN_LIFETIME);
    let introspected = site.introspect(token).await;
    assert_eq!(introspected["active"], true);
    assert_eq!(introspected["username"], "alice");
    assert_eq!(introspected["client_id"], "weblounge-client");
}

#[tokio::test]
async fn the_token_is_for_the_client_whose_page_asked() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let response = site
        .session_token_with(Some(&cookie), &from(WEBCHAT_URL))
        .await;
    assert_eq!(response.status(), 200);
    assert_readable_by(&response, WEBCHAT_URL);
    let token = json(response).await["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(site.introspect(&token).await["client_id"], "gamja");
}

#[tokio::test]
async fn the_token_expires_after_fifteen_minutes_and_with_the_session() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let answer = json(
        site.session_token_with(Some(&cookie), &from(BETA_URL))
            .await,
    )
    .await;
    let token = answer["access_token"].as_str().unwrap();
    site.execute(&format!(
        "UPDATE oauth_tokens SET created_at = created_at - {SESSION_TOKEN_LIFETIME},
                                 expires_at = expires_at - {SESSION_TOKEN_LIFETIME}"
    ))
    .await;
    assert_eq!(site.introspect(token).await["active"], false);

    let answer = json(
        site.session_token_with(Some(&cookie), &from(BETA_URL))
            .await,
    )
    .await;
    let token = answer["access_token"].as_str().unwrap();
    assert_eq!(site.introspect(token).await["active"], true);
    site.post("/signout", &[], Some(&cookie)).await;
    assert_eq!(site.introspect(token).await["active"], false);
}

#[tokio::test]
async fn without_a_session_the_page_learns_to_sign_the_member_in() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    site.post("/signout", &[], Some(&cookie)).await;
    for cookie in [None, Some(cookie.as_str()), Some("__Host-session=garbage")] {
        let response = site.session_token_with(cookie, &from(BETA_URL)).await;
        assert_eq!(response.status(), 401, "{cookie:?}");
        assert_readable_by(&response, BETA_URL);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(json(response).await, json!({"error": "login_required"}));
    }
    assert_eq!(site.stored_tokens().await, 0);
}

#[tokio::test]
async fn other_sites_get_no_token_and_cannot_read_the_answer() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for origin in [
        Some("https://evil.example"),
        Some(PUBLIC_URL),
        Some("null"),
        Some("https://beta.weblounge.org.evil.example"),
        None,
    ] {
        let headers: Vec<_> = origin
            .map(|origin| ("origin", origin))
            .into_iter()
            .collect();
        let headers = [headers.as_slice(), &[JSON]].concat();
        let response = site.session_token_with(Some(&cookie), &headers).await;
        assert_eq!(response.status(), 403, "{origin:?}");
        assert_unreadable(&response);
        assert_eq!(json(response).await, json!({"error": "invalid_client"}));
    }
    let evil_form = [
        ("origin", "https://evil.example"),
        ("content-type", "text/plain"),
    ];
    let response = site.session_token_with(Some(&cookie), &evil_form).await;
    assert_eq!(response.status(), 403);
    let referer_only = [JSON, ("referer", "https://beta.weblounge.org/")];
    let response = site.session_token_with(Some(&cookie), &referer_only).await;
    assert_eq!(response.status(), 403);
    assert_eq!(site.stored_tokens().await, 0);
}

#[tokio::test]
async fn a_post_a_form_could_make_gets_no_token() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for content_type in [
        None,
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
        Some("multipart/form-data; boundary=x"),
    ] {
        let mut headers = vec![("origin", BETA_URL)];
        headers.extend(content_type.map(|value| ("content-type", value)));
        let response = site.session_token_with(Some(&cookie), &headers).await;
        assert_eq!(response.status(), 415, "{content_type:?}");
        assert_eq!(json(response).await, json!({"error": "invalid_request"}));
    }
    assert_eq!(site.stored_tokens().await, 0);
}

/// What a browser asks before the beta client's page posts.
async fn preflight(site: &Site, origin: &str) -> Response {
    let request = Request::options(PATH)
        .header(header::ORIGIN, origin)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type");
    site.send(request.body(Body::empty()).unwrap()).await
}

#[tokio::test]
async fn browsers_get_leave_to_post_from_the_web_clients_pages_alone() {
    let site = Site::start().await;
    for origin in [BETA_URL, WEBCHAT_URL] {
        let asked = preflight(&site, origin).await;
        assert_eq!(asked.status(), 204, "{origin}");
        assert_readable_by(&asked, origin);
        let granted = |name| header_of(&asked, name);
        assert_eq!(granted(header::ACCESS_CONTROL_ALLOW_METHODS), Some("POST"));
        assert_eq!(
            granted(header::ACCESS_CONTROL_ALLOW_HEADERS),
            Some("Content-Type")
        );
        assert_eq!(granted(header::ACCESS_CONTROL_MAX_AGE), Some("3600"));
    }
    for origin in ["https://evil.example", PUBLIC_URL, "null"] {
        assert_unreadable(&preflight(&site, origin).await);
    }
}

#[tokio::test]
async fn asking_renews_the_session() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    site.execute("UPDATE sessions SET expires_at = expires_at - 2 * 86400")
        .await;
    let before = site.session_expiry(&cookie).await.unwrap();

    let response = site
        .session_token_with(Some(&cookie), &from(BETA_URL))
        .await;

    assert_eq!(response.status(), 200);
    assert!(response.headers().contains_key(header::SET_COOKIE));
    assert!(site.session_expiry(&cookie).await.unwrap() > before);
}

#[tokio::test]
async fn an_address_gets_a_limited_number_of_tokens() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let from_address = |address| [("origin", BETA_URL), JSON, ("x-real-ip", address)];
    for _ in 0..OAUTH_REQUESTS_PER_ADDRESS {
        let response = site
            .session_token_with(Some(&cookie), &from_address("192.0.2.7"))
            .await;
        assert_eq!(response.status(), 200);
    }
    let limited = site
        .session_token_with(Some(&cookie), &from_address("192.0.2.7"))
        .await;
    assert_eq!(limited.status(), 429);
    assert_readable_by(&limited, BETA_URL);
    assert_eq!(json(limited).await, json!({"error": "invalid_request"}));
    let elsewhere = site
        .session_token_with(Some(&cookie), &from_address("198.51.100.9"))
        .await;
    assert_eq!(elsewhere.status(), 200);
}
