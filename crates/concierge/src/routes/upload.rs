//! The upload endpoint IRC clients find in the FILEHOST token, on
//! files.weblounge.org: a member's client posts an image, with the
//! credentials it logs in to IRC with, and gets back the link to it.
//! Concierge reads none of the image; the scrubber does.

use std::time::Instant;

use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use filehost::{UploadId, handoff::Answer};

use crate::{
    app::{App, AppState},
    authorization::Credentials,
    db::{self, NewUpload},
    name::{BotName, MemberName},
    origin::WebchatOrigin,
    rate_limit::{self, ClientAddress},
    session::Token,
    upload_rules,
    uploads::HandOffError,
};

use super::signin;

pub const PATH: &str = "/upload";
const UPLOAD_LIMIT: HeaderName = HeaderName::from_static("upload-limit");

/// Whose an upload is: a member's own, or one of their bots'.
pub struct Uploader {
    pub owner: MemberName,
    pub bot: Option<BotName>,
}

impl Uploader {
    /// The account that uploads.
    fn account(&self) -> &str {
        self.bot
            .as_ref()
            .map_or(self.owner.as_str(), BotName::as_str)
    }
}

/// What the web client may send uploads with, and read of the answer.
const ALLOWED_HEADERS: &str = "Authorization, Content-Type, Content-Disposition";
const EXPOSED_HEADERS: &str = "Location, Upload-Limit";

/// Lets the web client's pages, and no other site's, post uploads with
/// their credentials and read the answer: browsers ask first, as its
/// requests carry an `Authorization`.
pub async fn allow_webchat(
    State(webchat): State<WebchatOrigin>,
    request: Request,
    next: Next,
) -> Response {
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .filter(|origin| origin.as_bytes() == webchat.as_str().as_bytes())
        .cloned();
    let preflight = request.method() == Method::OPTIONS;
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.append(header::VARY, HeaderValue::from_static("origin"));
    let Some(origin) = origin else {
        return response;
    };
    let static_value = HeaderValue::from_static;
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
        static_value("true"),
    );
    if preflight {
        headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, static_value("POST"));
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            static_value(ALLOWED_HEADERS),
        );
    } else {
        headers.insert(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            static_value(EXPOSED_HEADERS),
        );
    }
    response
}

/// What a browser asks before posting an upload from a page; the answer
/// is [`allow_webchat`]'s to give.
pub async fn preflight() -> StatusCode {
    StatusCode::NO_CONTENT
}

/// The size a client says its upload is.
fn content_length(headers: &HeaderMap) -> Option<i64> {
    headers
        .get(header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

/// The credentials a client sent in `Authorization`: the web client's
/// OAuth access token, or the nick and password every other client logs
/// in to IRC with, a member's or a bot's.
fn credentials(headers: &HeaderMap) -> Option<Credentials<'_>> {
    Credentials::parse(headers.get(header::AUTHORIZATION)?.to_str().ok()?)
}

/// What came of checking an uploader's credentials.
enum Authentication {
    Uploader(Uploader),
    Refused,
    /// Too many wrong passwords for the nick or from the address lately.
    Throttled,
}

pub async fn upload(
    State(app): State<AppState>,
    address: ClientAddress,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let uploader = match authenticate(&app, &headers, &address).await {
        Ok(Authentication::Uploader(uploader)) => uploader,
        Ok(Authentication::Refused) => return unauthorized(),
        Ok(Authentication::Throttled) => {
            return plain(StatusCode::TOO_MANY_REQUESTS, rate_limit::TRY_LATER);
        }
        Err(error) => return unavailable("Checking an uploader's credentials", error),
    };
    let account = uploader.account();
    let announced = content_length(&headers);
    if announced.is_some_and(|length| length > upload_rules::MAX_BYTES) {
        return too_large();
    }
    let Some(reservation) = app.recent_uploads.reserve(account, Instant::now()) else {
        return plain(
            StatusCode::TOO_MANY_REQUESTS,
            &format!(
                "That's {} uploads in the last hour. Try again later.",
                upload_rules::PER_HOUR
            ),
        );
    };
    match app.db.upload_usage(&uploader.owner).await {
        Ok(used) if !upload_rules::fits(used, announced.unwrap_or(0)) => return no_room(),
        Ok(_) => {}
        Err(error) => return unavailable("Adding up an owner's uploads", error),
    }
    let id = UploadId::generate();
    let answer = match app.uploads.hand_off(&id, body).await {
        Ok(answer) => answer,
        Err(HandOffError::TooLarge) => return too_large(),
        Err(HandOffError::Client(_)) => {
            return plain(StatusCode::BAD_REQUEST, "The upload was cut off.");
        }
        Err(HandOffError::Stalled) => {
            return plain(StatusCode::REQUEST_TIMEOUT, "The upload stopped coming.");
        }
        Err(HandOffError::Scrubber(error)) => return unavailable("Handing an upload over", error),
    };
    let (kind, size) = match answer {
        Answer::Stored { kind, size } => (kind, size),
        Answer::NotAnImage => return not_an_image(),
        Answer::TooLarge => return too_large(),
    };
    let upload = NewUpload {
        id: &id,
        kind,
        owner: &uploader.owner,
        bot: uploader.bot.as_ref(),
        size: size as i64,
        created_at: db::now(),
    };
    let file_name = id.file_name(kind);
    let inserted = app
        .db
        .insert_upload(&upload, upload_rules::QUOTA_BYTES)
        .await;
    if !matches!(inserted, Ok(true))
        && let Err(error) = app.uploads.remove_file(&file_name)
    {
        eprintln!("Deleting an upload left without a row failed: {error}");
    }
    match inserted {
        Ok(true) => {
            reservation.keep();
            created(&app, &file_name)
        }
        Ok(false) => no_room(),
        Err(error) => unavailable("Recording an upload", error),
    }
}

/// The member, or member's bot, whose credentials `headers` carry.
/// Passwords are checked by Ergo, behind the sign-in form's limits.
async fn authenticate(
    app: &App,
    headers: &HeaderMap,
    address: &ClientAddress,
) -> anyhow::Result<Authentication> {
    let uploader = match credentials(headers) {
        None => None,
        Some(Credentials::Bearer(token)) => {
            let Some(token) = Token::parse(token) else {
                return Ok(Authentication::Refused);
            };
            let hash = app.sessions.hash(&token);
            let token = app.db.access_token(&hash, db::now()).await?;
            token.map(|token| Uploader {
                owner: token.account,
                bot: None,
            })
        }
        Some(Credentials::Basic {
            user: nick,
            password,
        }) => {
            if !signin::plausible(&nick, &password) {
                return Ok(Authentication::Refused);
            }
            let now = Instant::now();
            if !app.limiter.allows(&nick, address.0, now) {
                return Ok(Authentication::Throttled);
            }
            let uploader = match app
                .ergo
                .check_auth(nick.as_str(), password.as_str())
                .await?
            {
                Some(account) => uploader_named(app, account.as_str()).await?,
                None => None,
            };
            match &uploader {
                Some(_) => app.limiter.record_success(&nick),
                None => app.limiter.record_failure(&nick, address.0, now),
            }
            uploader
        }
    };
    Ok(uploader.map_or(Authentication::Refused, Authentication::Uploader))
}

/// The current member named `account`, or the live bot of a current
/// member.
async fn uploader_named(app: &App, account: &str) -> sqlx::Result<Option<Uploader>> {
    let current = |member: Option<db::Member>| member.filter(|member| member.removed_at.is_none());
    if let Some(member) = current(app.db.member(account).await?) {
        return Ok(Some(Uploader {
            owner: member.account,
            bot: None,
        }));
    }
    let bot = app.db.bot(account).await?;
    let Some(bot) = bot.filter(|bot| bot.removed_at.is_none()) else {
        return Ok(None);
    };
    let owner = current(app.db.member(bot.owner.as_str()).await?);
    Ok(owner.map(|owner| Uploader {
        owner: owner.account,
        bot: Some(bot.account),
    }))
}

/// `201 Created`, with the link to the file named `file_name`.
fn created(app: &App, file_name: &str) -> Response {
    let url = format!("{}/{file_name}", app.files_url);
    match HeaderValue::from_str(&url) {
        Ok(location) => (
            StatusCode::CREATED,
            [(header::LOCATION, location)],
            format!("{url}\n"),
        )
            .into_response(),
        Err(error) => unavailable("Linking to an upload", error),
    }
}

fn unauthorized() -> Response {
    let challenge = HeaderValue::from_static(r#"Basic realm="weblounge", Bearer"#);
    let mut response = plain(
        StatusCode::UNAUTHORIZED,
        "Log in with the nick and password, or the token, you use on IRC.",
    );
    response
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, challenge);
    response
}

/// `413`, with the limit in `Upload-Limit`, where senpai reads it.
fn too_large() -> Response {
    let limit = format!("maxsize={}", upload_rules::MAX_BYTES);
    let message = format!(
        "Images can be {} at most.",
        upload_rules::in_megabytes(upload_rules::MAX_BYTES)
    );
    let mut response = plain(StatusCode::PAYLOAD_TOO_LARGE, &message);
    if let Ok(limit) = HeaderValue::from_str(&limit) {
        response.headers_mut().insert(UPLOAD_LIMIT, limit);
    }
    response
}

fn no_room() -> Response {
    plain(
        StatusCode::INSUFFICIENT_STORAGE,
        &format!(
            "Your uploads take all of your {}. Delete some on the members' site to make room.",
            upload_rules::in_megabytes(upload_rules::QUOTA_BYTES)
        ),
    )
}

fn not_an_image() -> Response {
    plain(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        &format!(
            "Only images can be uploaded: {}.",
            upload_rules::types_in_words()
        ),
    )
}

fn unavailable(what: &str, error: impl std::fmt::Display) -> Response {
    eprintln!("{what} failed: {error}");
    plain(
        StatusCode::SERVICE_UNAVAILABLE,
        "Can't take uploads right now. Try again in a minute.",
    )
}

fn plain(status: StatusCode, message: &str) -> Response {
    (status, format!("{message}\n")).into_response()
}
