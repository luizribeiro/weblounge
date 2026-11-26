use axum::{
    http::{StatusCode, header},
    response::Response,
};
use serde_json::json;

use crate::{
    BETA_URL, PUBLIC_URL, Site, WEBCHAT_URL,
    oauth::json,
    session_token::{
        JSON, assert_preflight_admits_the_web_clients_alone, assert_readable_by, assert_unreadable,
        from,
    },
};

const PATH: &str = "/oauth/signout";
const FORGOTTEN: &str = "__Host-session=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax";

impl Site {
    async fn client_sign_out(&self, cookie: Option<&str>, headers: &[(&str, &str)]) -> Response {
        self.post_from_page(PATH, cookie, headers).await
    }
}

fn assert_signed_out(response: &Response, origin: &str) {
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_readable_by(response, origin);
    assert_eq!(response.headers()[header::SET_COOKIE], FORGOTTEN);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn a_web_clients_page_signs_the_browser_out_and_its_tokens_die() {
    for origin in [BETA_URL, WEBCHAT_URL] {
        let site = Site::start().await;
        site.add_member("alice").await;
        let cookie = site.sign_in("alice").await;
        let other_device = site.sign_in("alice").await;
        let lasting = site.access_token(&cookie).await;
        let answer = json(site.session_token_with(Some(&cookie), &from(origin)).await).await;
        let short = answer["access_token"].as_str().unwrap().to_owned();

        let response = site.client_sign_out(Some(&cookie), &from(origin)).await;

        assert_signed_out(&response, origin);
        assert_eq!(site.session_expiry(&cookie).await, None, "{origin}");
        for token in [&lasting, &short] {
            assert_eq!(site.introspect(token).await["active"], false, "{origin}");
        }
        let again = site.session_token_with(Some(&cookie), &from(origin)).await;
        assert_eq!(again.status(), StatusCode::UNAUTHORIZED, "{origin}");
        assert!(
            site.session_expiry(&other_device).await.is_some(),
            "{origin}"
        );
    }
}

#[tokio::test]
async fn a_browser_already_signed_out_gets_the_same_answer() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let ended = site.sign_in("alice").await;
    let first = site.client_sign_out(Some(&ended), &from(BETA_URL)).await;
    assert_signed_out(&first, BETA_URL);
    let expired = site.sign_in("alice").await;
    site.execute("UPDATE sessions SET expires_at = 0").await;
    for sent in [
        None,
        Some(ended.as_str()),
        Some(expired.as_str()),
        Some("__Host-session=garbage"),
    ] {
        let response = site.client_sign_out(sent, &from(BETA_URL)).await;
        assert_signed_out(&response, BETA_URL);
    }
}

#[tokio::test]
async fn a_session_due_for_renewal_is_ended_not_renewed() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    site.execute("UPDATE sessions SET expires_at = expires_at - 2 * 86400")
        .await;

    let response = site.client_sign_out(Some(&cookie), &from(BETA_URL)).await;

    assert_signed_out(&response, BETA_URL);
    assert_eq!(
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .count(),
        1
    );
    assert_eq!(site.session_expiry(&cookie).await, None);
}

#[tokio::test]
async fn other_sites_cannot_sign_the_browser_out() {
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
            .chain([JSON])
            .collect();
        let response = site.client_sign_out(Some(&cookie), &headers).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{origin:?}");
        assert_unreadable(&response);
        assert!(!response.headers().contains_key(header::SET_COOKIE));
        assert_eq!(json(response).await, json!({"error": "invalid_client"}));
    }
    let referer_only = [JSON, ("referer", "https://beta.weblounge.org/")];
    let response = site.client_sign_out(Some(&cookie), &referer_only).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(site.session_expiry(&cookie).await.is_some());
}

#[tokio::test]
async fn a_post_a_form_could_make_signs_nobody_out() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for content_type in [
        None,
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
    ] {
        let mut headers = vec![("origin", BETA_URL)];
        headers.extend(content_type.map(|value| ("content-type", value)));
        let response = site.client_sign_out(Some(&cookie), &headers).await;
        assert_eq!(
            response.status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "{content_type:?}"
        );
        assert_eq!(json(response).await, json!({"error": "invalid_request"}));
    }
    assert!(site.session_expiry(&cookie).await.is_some());
}

#[tokio::test]
async fn browsers_get_leave_to_sign_out_from_the_web_clients_pages_alone() {
    assert_preflight_admits_the_web_clients_alone(PATH).await;
}
