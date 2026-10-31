//! Lets the web clients' pages, and no other site's, read the answers of
//! the endpoints they call from the browser. Browsers send the page's
//! `Origin`, and concierge echoes it back only when it's a client's.

use axum::{
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::Response,
};

use crate::oauth::WebClients;

/// How long, in seconds, a browser may go on trusting a preflight's
/// answer before asking again; some browsers cap it lower themselves.
const PREFLIGHT_MAX_AGE: &str = "3600";

/// What the web clients may send an endpoint with their credentials, and
/// read of its answer.
#[derive(Clone)]
pub struct Credentialed {
    pub clients: WebClients,
    pub allowed_headers: &'static str,
    pub exposed_headers: Option<&'static str>,
}

/// For endpoints the web clients read without credentials.
pub async fn allow_web_clients(
    State(clients): State<WebClients>,
    request: Request,
    next: Next,
) -> Response {
    let origin = clients.origin_of(request.headers());
    let mut response = next.run(request).await;
    allow_origin(&mut response, origin);
    response
}

/// For endpoints the web clients post to with their credentials, a cookie
/// or an `Authorization`, which browsers ask about first.
pub async fn allow_web_clients_with_credentials(
    State(cors): State<Credentialed>,
    request: Request,
    next: Next,
) -> Response {
    let origin = cors.clients.origin_of(request.headers());
    let preflight = request.method() == Method::OPTIONS;
    let mut response = next.run(request).await;
    if !allow_origin(&mut response, origin) {
        return response;
    }
    let headers = response.headers_mut();
    let static_value = HeaderValue::from_static;
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
        static_value("true"),
    );
    if preflight {
        headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, static_value("POST"));
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            static_value(cors.allowed_headers),
        );
        headers.insert(
            header::ACCESS_CONTROL_MAX_AGE,
            static_value(PREFLIGHT_MAX_AGE),
        );
    } else if let Some(exposed) = cors.exposed_headers {
        headers.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, static_value(exposed));
    }
    response
}

/// What a browser asks before posting with credentials; the answer is
/// [`allow_web_clients_with_credentials`]'s to give.
pub async fn preflight() -> StatusCode {
    StatusCode::NO_CONTENT
}

/// Marks `response` as one that differs by origin, and lets `origin` read
/// it if there is one. Answers whether it did.
fn allow_origin(response: &mut Response, origin: Option<HeaderValue>) -> bool {
    let headers = response.headers_mut();
    headers.append(header::VARY, HeaderValue::from_static("origin"));
    let Some(origin) = origin else {
        return false;
    };
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    true
}
