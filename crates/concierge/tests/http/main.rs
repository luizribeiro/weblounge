//! The site, driven through its router with a temporary database and a
//! fake Ergo.

mod account;
mod pages;
mod security;
mod signin;

use std::path::Path;

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, header},
    response::Response,
};
use concierge::{
    app::{self, App},
    assets::Assets,
    config::IrcServer,
    db::{self, Db, NewMember},
    invite_rules::DEFAULT_QUOTA,
    rate_limit::Limiter,
    session::{self, Token},
};
use ergo_api::{Passphrase, Registration, fake::FakeErgo};
use http_body_util::BodyExt;
use tempfile::TempDir;
use tower::ServiceExt;

const PUBLIC_URL: &str = "https://members.weblounge.org";
const SESSION_KEY: &[u8] = b"a session key for tests, at least 32 bytes";
const PASSWORD: &str = "correct-horse-battery";

struct Site {
    router: Router,
    db: Db,
    ergo: FakeErgo,
    _state: TempDir,
}

impl Site {
    async fn start() -> Self {
        let ergo = FakeErgo::start().await;
        let state = tempfile::tempdir().unwrap();
        let db = Db::open(&state.path().join("concierge.db")).await.unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/assets");
        let router = app::router(App {
            assets: Assets::load(&fixtures).unwrap(),
            db: db.clone(),
            ergo: ergo.client(),
            sessions: session::Hasher::new(SESSION_KEY),
            limiter: Limiter::default(),
            public_url: PUBLIC_URL.into(),
            irc_server: IrcServer {
                host: "irc.weblounge.org".into(),
                port: 6697,
            },
        });
        Self {
            router,
            db,
            ergo,
            _state: state,
        }
    }

    async fn send(&self, request: Request<Body>) -> Response {
        self.router.clone().oneshot(request).await.unwrap()
    }

    async fn get(&self, path: &str, cookie: Option<&str>) -> Response {
        let mut request = Request::get(path);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        self.send(request.body(Body::empty()).unwrap()).await
    }

    /// Posts a form the way our own pages do, from our origin.
    async fn post(&self, path: &str, form: &[(&str, &str)], cookie: Option<&str>) -> Response {
        self.post_with(path, form, cookie, &[(header::ORIGIN.as_str(), PUBLIC_URL)])
            .await
    }

    async fn post_with(
        &self,
        path: &str,
        form: &[(&str, &str)],
        cookie: Option<&str>,
        headers: &[(&str, &str)],
    ) -> Response {
        let body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form)
            .finish();
        let mut request =
            Request::post(path).header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        self.send(request.body(Body::from(body)).unwrap()).await
    }

    async fn register(&self, account: &str) {
        let passphrase: Passphrase = PASSWORD.parse().unwrap();
        let registration = self.ergo.client().register(account, &passphrase).await;
        assert_eq!(registration.unwrap(), Registration::Created);
    }

    /// An Ergo account that's also a member, with the email given.
    async fn add_member(&self, account: &str) {
        self.register(account).await;
        let member = NewMember {
            account,
            email: Some("someone@example.org"),
            news_opt_in: false,
            invited_by: None,
            joined_at: 1_760_000_000,
            invite_quota: DEFAULT_QUOTA,
            is_admin: false,
        };
        self.db.insert_member(&member).await.unwrap();
    }

    async fn sign_in_form(&self, nick: &str, password: &str) -> Response {
        self.post("/signin", &[("nick", nick), ("password", password)], None)
            .await
    }

    /// Signs in and answers with the `Cookie` header to send from then on.
    async fn sign_in(&self, nick: &str) -> String {
        let response = self.sign_in_form(nick, PASSWORD).await;
        assert_eq!(response.status(), 303);
        session_cookie(&response).expect("signing in sets the session cookie")
    }

    async fn session_hash(&self, cookie: &str) -> Vec<u8> {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, cookie.parse().unwrap());
        let token = Token::from_cookies(&headers).unwrap();
        session::Hasher::new(SESSION_KEY).hash(&token)
    }

    async fn session_expiry(&self, cookie: &str) -> Option<i64> {
        let hash = self.session_hash(cookie).await;
        let session = self.db.session(&hash, db::now()).await.unwrap();
        session.map(|session| session.expires_at)
    }
}

/// The `name=value` part of the session cookie a response sets.
fn session_cookie(response: &Response) -> Option<String> {
    let set_cookie = response.headers().get(header::SET_COOKIE)?.to_str().ok()?;
    let pair = set_cookie.split(';').next()?;
    pair.starts_with(session::COOKIE).then(|| pair.to_owned())
}

fn location(response: &Response) -> &str {
    response.headers()[header::LOCATION].to_str().unwrap()
}

async fn body_text(response: Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}
