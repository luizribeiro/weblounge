//! The site, driven through its router with a temporary database and a
//! fake Ergo.

mod account;
mod certificates;
mod checks;
mod clients;
mod home;
mod invite;
mod invites;
mod oauth;
mod pages;
mod password;
mod presence;
mod security;
mod signin;
mod signup;
mod tabs;
mod tree;
mod verify;

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
    config::{IrcServer, Presence},
    db::{self, Db, NewInvite, NewMember},
    invite_rules,
    oauth::Webchat,
    outbox::{Outbox, QueuedMessage},
    rate_limit::{
        AddressLimiter, LOOKUPS_PER_ADDRESS, Limiter, MailLimiter, OAUTH_REQUESTS_PER_ADDRESS,
        VERIFICATIONS_PER_ADDRESS,
    },
    session::{self, Token},
};
use ergo_api::{Passphrase, Registration, fake::FakeErgo};
use http_body_util::BodyExt;
use tempfile::TempDir;
use tower::ServiceExt;

const PUBLIC_URL: &str = "https://members.weblounge.org";
const WEBCHAT_URL: &str = "https://irc.weblounge.org";
const JOIN_URL: &str = "https://join.weblounge.org";
const SESSION_KEY: &[u8] = b"a session key for tests, at least 32 bytes";
const PASSWORD: &str = "correct-horse-battery";
const INTROSPECTION_SECRET: &str = "a secret Ergo introspects tokens with";
/// The label of the checkbox for announcements, on signup and the Account tab.
const NEWS_LABEL: &str = r#"<span>Email me about new features and planned downtime <span class="hint check-hint">A few times a year at most.</span></span>"#;

struct Site {
    router: Router,
    db: Db,
    outbox: Outbox,
    ergo: FakeErgo,
    state: TempDir,
}

impl Site {
    async fn start() -> Self {
        Self::start_with(|_| {}).await
    }

    /// A site whose app `change` adjusts, for example to talk to another Ergo
    /// than its fake.
    async fn start_with(change: impl FnOnce(&mut App)) -> Self {
        let fake = FakeErgo::start().await;
        let state = tempfile::tempdir().unwrap();
        let db = Db::migrate(&state.path().join("concierge.db"))
            .await
            .unwrap();
        let outbox = Outbox::migrate(&state.path().join("outbox.db"))
            .await
            .unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/assets");
        let mut app = App {
            assets: Assets::load(&fixtures).unwrap(),
            db: db.clone(),
            outbox: outbox.clone(),
            ergo: fake.client(),
            sessions: session::Hasher::new(SESSION_KEY),
            limiter: Limiter::default(),
            lookups: AddressLimiter::new(LOOKUPS_PER_ADDRESS),
            oauth_requests: AddressLimiter::new(OAUTH_REQUESTS_PER_ADDRESS),
            verifications: AddressLimiter::new(VERIFICATIONS_PER_ADDRESS),
            mailed_links: MailLimiter::default(),
            public_url: PUBLIC_URL.into(),
            invite_links: format!("{JOIN_URL}/#"),
            webchat: Webchat {
                origin: WEBCHAT_URL.into(),
            },
            introspection_secret: INTROSPECTION_SECRET.into(),
            irc_server: irc_server(),
            presence_defaults: Presence {
                always_on: true,
                auto_away: true,
            },
        };
        change(&mut app);
        Self {
            router: app::router(app),
            db,
            outbox,
            ergo: fake,
            state,
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

    /// A GET the way another site's script makes it, from `origin`.
    async fn get_from(&self, path: &str, origin: &str) -> Response {
        let request = Request::get(path).header(header::ORIGIN, origin);
        self.send(request.body(Body::empty()).unwrap()).await
    }

    /// A GET from the client address nginx passes on.
    async fn get_with(&self, path: &str, cookie: &str, address: &str) -> Response {
        let request = Request::get(path)
            .header(header::COOKIE, cookie)
            .header("x-real-ip", address);
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
            email: "someone@example.org",
            news_opt_in: false,
            invited_by: None,
            joined_at: 1_760_000_000,
            is_admin: false,
        };
        self.db.insert_member(&member).await.unwrap();
    }

    /// Runs SQL the site has no query for, to set up a test.
    async fn execute(&self, sql: &str) {
        execute(&self.database_path(), sql).await;
    }

    async fn query_scalar<T>(&self, sql: &str) -> T
    where
        T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + Unpin,
    {
        let url = format!("sqlite://{}", self.database_path().display());
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        let value = sqlx::query_scalar(sql).fetch_one(&pool).await.unwrap();
        pool.close().await;
        value
    }

    /// The mail waiting in the outbox, oldest first.
    async fn mail(&self) -> Vec<QueuedMessage> {
        self.outbox.due(db::now()).await.unwrap()
    }

    fn database_path(&self) -> std::path::PathBuf {
        self.state.path().join("concierge.db")
    }

    /// An open invite from `created_by`, or a root invite.
    async fn add_invite(&self, code: &str, created_by: Option<&str>) {
        let now = db::now();
        let invite = NewInvite {
            code,
            created_by,
            created_at: now,
            expires_at: invite_rules::expires_at(now),
            makes_admin: false,
        };
        self.db.insert_invite(&invite).await.unwrap();
    }

    /// Sets `assignments` on `code`'s invite, which no query of the site's
    /// does, to set up a test.
    async fn update_invite(&self, code: &str, assignments: &str) {
        let url = format!("sqlite://{}", self.database_path().display());
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        sqlx::query(&format!(
            "UPDATE invites SET {assignments} WHERE code_hash = ?"
        ))
        .bind(concierge::invite_code::hash(code))
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
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

async fn execute(database: &Path, sql: &str) {
    let url = format!("sqlite://{}", database.display());
    let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
    sqlx::raw_sql(sql).execute(&pool).await.unwrap();
    pool.close().await;
}

fn irc_server() -> IrcServer {
    IrcServer {
        host: "irc.weblounge.org".into(),
        port: 6697,
    }
}

/// The `name=value` part of the session cookie a response sets.
fn session_cookie(response: &Response) -> Option<String> {
    let set_cookie = response.headers().get(header::SET_COOKIE)?.to_str().ok()?;
    let pair = set_cookie.split(';').next()?;
    pair.starts_with(session::COOKIE).then(|| pair.to_owned())
}

/// Whether a page leads with the button that opens the web client.
fn opens_the_chat(page: &str) -> bool {
    page.contains(
        r#"<a class="button button-primary" href="https://irc.weblounge.org/">Open chat</a>"#,
    ) && page.contains(r#"<code class="muted">irc.weblounge.org in your browser</code>"#)
}

/// Whether a page is the tab at `path`, marked open in the row of tabs.
fn is_tab(page: &str, path: &str) -> bool {
    page.contains(&format!(r#"<a href="{path}" aria-current="page">"#))
}

fn location(response: &Response) -> &str {
    response.headers()[header::LOCATION].to_str().unwrap()
}

/// The verification link in `message`, and the code it carries.
fn verification_link(message: &QueuedMessage) -> (&str, &str) {
    let prefix = format!("{PUBLIC_URL}/verify#");
    let link = message
        .body
        .lines()
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("a verification link in {}", message.body));
    (link, &link[prefix.len()..])
}

/// Posts `code` the way the verify page's Confirm button does, and
/// answers the status and page.
async fn confirm(site: &Site, code: &str) -> (u16, String) {
    let response = site.post("/verify", &[("code", code)], None).await;
    assert!(!response.headers().contains_key("set-cookie"));
    (response.status().as_u16(), body_text(response).await)
}

async fn body_text(response: Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}
