//! Refuses requests that change something unless they come from one of
//! our own pages, on top of the session cookie's `SameSite=Lax`.

use axum::{
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, Uri, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::origin::{SiteOrigin, WebchatOrigin};

/// Whether a request with these headers may go ahead on the site at
/// `public_url`. Browsers send `Origin` on every POST; `Referer` stands in
/// for the few that leave it out.
pub fn allows(method: &Method, headers: &HeaderMap, public_url: &str) -> bool {
    if matches!(*method, Method::GET | Method::HEAD) {
        return true;
    }
    let header = |name| headers.get(name).and_then(|value| value.to_str().ok());
    match (header(header::ORIGIN), header(header::REFERER)) {
        (Some(origin), _) => origin == public_url,
        (None, Some(referer)) => origin_of(referer).as_deref() == Some(public_url),
        (None, None) => false,
    }
}

fn origin_of(url: &str) -> Option<String> {
    let uri: Uri = url.parse().ok()?;
    Some(format!("{}://{}", uri.scheme()?, uri.authority()?))
}

/// Guards the members' site's own pages.
pub async fn guard_site(
    State(origin): State<SiteOrigin>,
    request: Request,
    next: Next,
) -> Response {
    guard(origin.as_str(), request, next).await
}

/// Guards the endpoints the web client posts to.
pub async fn guard_webchat(
    State(origin): State<WebchatOrigin>,
    request: Request,
    next: Next,
) -> Response {
    guard(origin.as_str(), request, next).await
}

async fn guard(origin: &str, request: Request, next: Next) -> Response {
    if allows(request.method(), request.headers(), origin) {
        next.run(request).await
    } else {
        (StatusCode::FORBIDDEN, "Cross-origin request refused.\n").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: &str = "https://members.weblounge.org";

    fn allows_with(method: Method, headers: &[(header::HeaderName, &str)]) -> bool {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(name, value.parse().unwrap());
        }
        allows(&method, &map, SITE)
    }

    #[test]
    fn reads_go_ahead_from_anywhere() {
        for method in [Method::GET, Method::HEAD] {
            assert!(allows_with(method.clone(), &[]));
            assert!(allows_with(
                method,
                &[(header::ORIGIN, "https://evil.example")]
            ));
        }
    }

    #[test]
    fn writes_from_our_own_origin_go_ahead() {
        for method in [Method::POST, Method::PUT, Method::DELETE, Method::PATCH] {
            assert!(allows_with(method, &[(header::ORIGIN, SITE)]));
        }
    }

    #[test]
    fn writes_from_other_origins_are_refused() {
        for origin in [
            "https://evil.example",
            "null",
            "http://members.weblounge.org",
            "https://members.weblounge.org:8443",
            "https://weblounge.org",
            "https://members.weblounge.org.evil.example",
            "https://members.weblounge.org/",
        ] {
            assert!(
                !allows_with(Method::POST, &[(header::ORIGIN, origin)]),
                "{origin}"
            );
        }
    }

    #[test]
    fn origin_wins_over_referer() {
        assert!(!allows_with(
            Method::POST,
            &[
                (header::ORIGIN, "https://evil.example"),
                (header::REFERER, "https://members.weblounge.org/signin"),
            ]
        ));
    }

    #[test]
    fn without_origin_the_referer_decides() {
        let referer = |url| allows_with(Method::POST, &[(header::REFERER, url)]);
        assert!(referer("https://members.weblounge.org/signin?next=%2F"));
        assert!(referer("https://members.weblounge.org"));
        assert!(!referer(
            "https://evil.example/https://members.weblounge.org"
        ));
        assert!(!referer("https://members.weblounge.org.evil.example/"));
        assert!(!referer("/signin"));
        assert!(!referer("not a url"));
    }

    #[test]
    fn writes_with_neither_header_are_refused() {
        assert!(!allows_with(Method::POST, &[]));
        assert!(!allows_with(Method::OPTIONS, &[]));
    }

    #[test]
    fn keeps_a_forwarded_port_in_the_comparison() {
        let mut headers = HeaderMap::new();
        let vm = "https://members.weblounge.localhost:10443";
        headers.insert(header::ORIGIN, vm.parse().unwrap());
        assert!(allows(&Method::POST, &headers, vm));
        headers.insert(header::ORIGIN, SITE.parse().unwrap());
        assert!(!allows(&Method::POST, &headers, vm));
    }
}
