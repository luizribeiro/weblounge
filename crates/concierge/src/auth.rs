//! Who's asking: the session middleware, and the extractors pages use to
//! greet a member or send a signed-out visitor to the sign-in page.

use std::sync::Arc;

use axum::{
    extract::{FromRequestParts, Request, State},
    http::{HeaderValue, Method, header, request::Parts},
    middleware::Next,
    response::{Redirect, Response},
};

use crate::{
    app::{App, AppState},
    db::{self, Member, NewSession},
    name::MemberName,
    routes::internal_error,
    session::{self, Token},
};

pub const SIGN_IN_PATH: &str = "/signin";
/// Longer paths aren't ones our pages link to.
const MAX_NEXT_LENGTH: usize = 512;

pub struct SignedIn {
    pub member: Member,
    pub token_hash: Vec<u8>,
}

/// The visitor's session, if they're signed in.
#[derive(Clone, Default)]
pub struct Visitor(pub Option<Arc<SignedIn>>);

impl Visitor {
    pub fn nick(&self) -> Option<&str> {
        self.0
            .as_ref()
            .map(|signed_in| signed_in.member.account.as_str())
    }
}

/// Looks the session cookie up, renews the session at most once a day, and
/// hands the result to the handler as a [`Visitor`].
pub async fn resolve(State(app): State<AppState>, mut request: Request, next: Next) -> Response {
    let now = db::now();
    let mut renewal = None;
    let mut visitor = Visitor::default();
    if let Some(token) = Token::from_cookies(request.headers()) {
        let token_hash = app.sessions.hash(&token);
        let session = match app.db.session(&token_hash, now).await {
            Ok(session) => session,
            Err(error) => return internal_error("Looking up a session", error),
        };
        if let Some(session) = session {
            if session::needs_renewal(session.expires_at, now) {
                let renewed = app.db.renew_session(&token_hash, session::expires_at(now));
                if let Err(error) = renewed.await {
                    return internal_error("Renewing a session", error);
                }
                renewal = Some(token.cookie());
            }
            visitor = Visitor(Some(Arc::new(SignedIn {
                member: session.member,
                token_hash,
            })));
        }
    }
    request.extensions_mut().insert(visitor);
    let mut response = next.run(request).await;
    if let Some(cookie) = renewal
        && !response.headers().contains_key(header::SET_COOKIE)
    {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}

impl<S: Send + Sync> FromRequestParts<S> for Visitor {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(parts.extensions.get::<Self>().cloned().unwrap_or_default())
    }
}

/// A signed-in member; anyone else is sent to sign in, and brought back to
/// the page they asked for afterwards.
pub struct CurrentMember(pub Arc<SignedIn>);

impl<S: Send + Sync> FromRequestParts<S> for CurrentMember {
    type Rejection = Redirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Ok(Visitor(signed_in)) = Visitor::from_request_parts(parts, state).await;
        signed_in.map(Self).ok_or_else(|| {
            let wanted = parts.uri.path_and_query().map_or("/", |path| path.as_str());
            let comes_back = parts.method == Method::GET && wanted != "/";
            Redirect::to(&if comes_back {
                sign_in_url(wanted)
            } else {
                SIGN_IN_PATH.to_owned()
            })
        })
    }
}

/// Signs `account` in with a fresh token, ending the session the browser
/// had, if any, and cleaning up expired ones while at it. Answers the
/// cookie to set.
pub async fn start_session(
    app: &App,
    visitor: &Visitor,
    account: &MemberName,
) -> sqlx::Result<HeaderValue> {
    let token = Token::generate();
    let token_hash = app.sessions.hash(&token);
    let now = db::now();
    app.db.end_expired_sessions(now).await?;
    if let Some(signed_in) = &visitor.0 {
        app.db.end_session(&signed_in.token_hash).await?;
    }
    app.db
        .insert_session(&NewSession {
            token_hash: &token_hash,
            account,
            created_at: now,
            expires_at: session::expires_at(now),
        })
        .await?;
    Ok(token.cookie())
}

pub fn sign_in_url(next: &str) -> String {
    let next: String = form_urlencoded::byte_serialize(next.as_bytes()).collect();
    format!("{SIGN_IN_PATH}?next={next}")
}

/// `next` if it's a path on this site, so a sign-in link can't send
/// members anywhere else afterwards.
pub fn local_path(next: &str) -> Option<&str> {
    let local = next.starts_with('/')
        && !next.starts_with("//")
        && next.len() <= MAX_NEXT_LENGTH
        && next.bytes().all(|c| c.is_ascii_graphic() && c != b'\\');
    local.then_some(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_paths_on_this_site() {
        for path in [
            "/",
            "/rules",
            "/invite/q7Hx2mZp9vLcT4wRfBa-_w",
            "/?saved",
            "/a%2F%2Fb",
        ] {
            assert_eq!(local_path(path), Some(path));
        }
    }

    #[test]
    fn refuses_anything_that_could_leave_the_site() {
        for next in [
            "",
            "rules",
            "https://evil.example/",
            "//evil.example",
            "/\\evil.example",
            "\\\\evil.example",
            "/\t/evil.example",
            "/ /evil.example",
            "/\n//evil.example",
            "javascript:alert(1)",
            "/caf\u{e9}",
        ] {
            assert_eq!(local_path(next), None, "{next:?}");
        }
        assert_eq!(
            local_path(&format!("/{}", "a".repeat(MAX_NEXT_LENGTH))),
            None
        );
    }

    #[test]
    fn the_sign_in_url_carries_the_path_encoded() {
        assert_eq!(
            sign_in_url("/rules?x=1&y"),
            "/signin?next=%2Frules%3Fx%3D1%26y"
        );
    }
}
