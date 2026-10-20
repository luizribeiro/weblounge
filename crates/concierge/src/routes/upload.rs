//! The upload endpoint IRC clients find in the FILEHOST token, on
//! files.weblounge.org: a member's client posts an image, with the
//! credentials it logs in to IRC with, and gets back the link to it.
//! Concierge reads none of the image; the scrubber does.

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use filehost::{UploadId, handoff::Answer};

use crate::{
    app::{App, AppState},
    db::{self, NewUpload},
    name::{BotName, MemberName},
    session::Token,
    upload_rules,
    uploads::HandOffError,
};

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
}

impl Credentials {
    fn of(headers: &HeaderMap) -> Option<Self> {
        let authorization = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
        let (scheme, value) = authorization.split_once(' ')?;
        if scheme.eq_ignore_ascii_case("bearer") {
            Token::parse(value.trim()).map(Self::Bearer)
        } else {
            None
        }
    }
}

pub async fn upload(State(app): State<AppState>, headers: HeaderMap, body: Body) -> Response {
    let uploader = match authenticate(&app, &headers).await {
        Ok(Some(uploader)) => uploader,
        Ok(None) => return unauthorized(),
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
async fn authenticate(app: &App, headers: &HeaderMap) -> anyhow::Result<Option<Uploader>> {
    let Some(credentials) = Credentials::of(headers) else {
        return Ok(None);
    };
    match credentials {
        Credentials::Bearer(token) => {
            let hash = app.sessions.hash(&token);
            let token = app.db.access_token(&hash, db::now()).await?;
            Ok(token.map(|token| Uploader {
                owner: token.account,
                bot: None,
            }))
        }
    }
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
