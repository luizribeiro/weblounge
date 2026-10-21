//! The Uploads tab: the images a member and their bots uploaded, how much
//! of their room they take, and deleting them. Members see and delete
//! their own alone; anyone else's are not found.

use askama::Template;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
};
use filehost::UploadId;

use super::{Layout, Tab, internal_error, not_found, render};
use crate::{
    app::{App, AppState},
    auth::CurrentMember,
    db::{self, Member, Upload},
    oauth::Webchat,
    tree, upload_rules,
};

pub const PATH: &str = "/uploads";

#[derive(Template)]
#[template(path = "uploads.html")]
struct UploadsPage<'a> {
    layout: Layout<'a>,
    uploads: Vec<Listed>,
    used: i64,
    rules: upload_rules::InWords,
    webchat: &'a Webchat,
}

/// An upload as the tab lists it.
struct Listed {
    id: String,
    url: String,
    /// The day it was made, as ISO for machines and as members read it.
    date: String,
    day: String,
    days_left: i64,
    size: String,
    /// The bot that uploaded it, if one did.
    bot: Option<String>,
}

impl Listed {
    fn new(app: &App, upload: Upload, now: i64) -> Self {
        Self {
            id: upload.id.as_str().to_owned(),
            url: format!("{}/{}", app.files_url, upload.file_name()),
            date: tree::date(upload.created_at),
            day: tree::day_in_words(upload.created_at),
            days_left: upload_rules::days_left(upload.created_at, now),
            size: upload_rules::in_megabytes(upload.size),
            bot: upload.bot.map(|bot| bot.to_string()),
        }
    }
}

/// What a member is asked before deleting an upload, for browsers that
/// can't open the tab's dialog.
#[derive(Template)]
#[template(path = "upload-question.html")]
struct QuestionPage<'a> {
    layout: Layout<'a>,
    upload: Listed,
}

pub async fn show(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    let member = &me.member;
    let (uploads, used) = match tokio::try_join!(
        app.db.uploads_of(&member.account),
        app.db.upload_usage(&member.account)
    ) {
        Ok(found) => found,
        Err(error) => return internal_error("Listing a member's uploads", error),
    };
    let now = db::now();
    render(&UploadsPage {
        layout: Layout::new(&app, Some(member.account.as_str())).on(Tab::Uploads),
        uploads: uploads
            .into_iter()
            .map(|upload| Listed::new(&app, upload, now))
            .collect(),
        used,
        rules: upload_rules::InWords::default(),
        webchat: &app.webchat,
    })
}

/// Asks before deleting an upload.
pub async fn confirm_delete(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    Path(id): Path<String>,
) -> Response {
    match owned_or_not_found(&app, &me.member, &id).await {
        Ok(upload) => render(&QuestionPage {
            layout: Layout::new(&app, Some(me.member.account.as_str())).on(Tab::Uploads),
            upload: Listed::new(&app, upload, db::now()),
        }),
        Err(response) => response,
    }
}

/// Deletes the member's upload `id`, file and row.
pub async fn delete(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    Path(id): Path<String>,
) -> Response {
    let upload = match owned_or_not_found(&app, &me.member, &id).await {
        Ok(upload) => upload,
        Err(response) => return response,
    };
    match app.uploads.delete(&app.db, &[upload]).await {
        Ok(()) => Redirect::to(PATH).into_response(),
        Err(error) => internal_error("Deleting an upload", error),
    }
}

/// `member`'s upload, or one of their bots', with the id `id`, or the
/// answer that none was found.
async fn owned_or_not_found(app: &App, member: &Member, id: &str) -> Result<Upload, Response> {
    let Some(id) = UploadId::parse(id) else {
        return Err(not_found());
    };
    match app.db.upload(&id).await {
        Ok(Some(upload)) if upload.owner == member.account => Ok(upload),
        Ok(_) => Err(not_found()),
        Err(error) => Err(internal_error("Looking up an upload", error)),
    }
}
