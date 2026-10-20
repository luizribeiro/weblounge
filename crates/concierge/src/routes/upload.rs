//! The upload endpoint IRC clients find in the FILEHOST token, on
//! files.weblounge.org: a member's client posts an image, with the
//! credentials it logs in to IRC with, and gets back the link to it.
//! Concierge reads none of the image; the scrubber does.

use std::time::Instant;

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use filehost::{UploadId, handoff::Answer};

use crate::{
    app::{App, AppState},
    db::{self, NewUpload},
    name::{BotName, MemberName},
    rate_limit::{self, ClientAddress},
    session::Token,
    upload_rules,
    uploads::HandOffError,
};

use super::signin;

pub const PATH: &str = "/upload";

/// Whose an upload is: a member's own, or one of their bots'.
pub struct Uploader {
    pub owner: MemberName,
    pub bot: Option<BotName>,
}

/// The credentials a client sent in `Authorization`.
enum Credentials {
    /// The web client's OAuth access token.
    Bearer(Token),
    /// A nick and password, as every other client logs in to IRC with:
    /// a member's, or a bot's.
    Basic { nick: String, password: String },
}

impl Credentials {
    fn of(headers: &HeaderMap) -> Option<Self> {
        let authorization = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
        let (scheme, value) = authorization.split_once(' ')?;
        if scheme.eq_ignore_ascii_case("bearer") {
            Token::parse(value.trim()).map(Self::Bearer)
        } else if scheme.eq_ignore_ascii_case("basic") {
            let decoded = String::from_utf8(STANDARD.decode(value.trim()).ok()?).ok()?;
            let (nick, password) = decoded.split_once(':')?;
            Some(Self::Basic {
                nick: nick.to_owned(),
                password: password.to_owned(),
            })
        } else {
            None
        }
    }
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
    let id = UploadId::generate();
    let answer = match app.uploads.hand_off(&id, body).await {
        Ok(answer) => answer,
        Err(HandOffError::TooLarge) => return too_large(),
        Err(HandOffError::Client(_)) => {
            return plain(StatusCode::BAD_REQUEST, "The upload was cut off.");
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
        Ok(true) => created(&app, &file_name),
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
    let uploader = match Credentials::of(headers) {
        None => None,
        Some(Credentials::Bearer(token)) => {
            let hash = app.sessions.hash(&token);
            let token = app.db.access_token(&hash, db::now()).await?;
            token.map(|token| Uploader {
                owner: token.account,
                bot: None,
            })
        }
        Some(Credentials::Basic { nick, password }) => {
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

fn too_large() -> Response {
    plain(
        StatusCode::PAYLOAD_TOO_LARGE,
        &format!(
            "Images can be {} at most.",
            upload_rules::in_megabytes(upload_rules::MAX_BYTES)
        ),
    )
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
