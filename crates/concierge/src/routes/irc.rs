//! The IRC tab: how Ergo treats the member while no client is open.

use askama::Template;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Response,
};

use super::{Flash, Layout, Tab, presence::Presence, render_with};
use crate::{
    app::{App, AppState},
    auth::CurrentMember,
    db::Member,
};

#[derive(Template)]
#[template(path = "irc.html")]
struct Irc<'a> {
    layout: Layout<'a>,
    flash: Flash,
    /// The IRC settings, or why they can't be shown.
    presence: Result<Presence, &'static str>,
    defaults: Presence,
    error: Option<&'static str>,
}

pub async fn show(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    Query(flash): Query<Flash>,
) -> Response {
    page(&app, &me.member, StatusCode::OK, flash, None).await
}

/// The IRC tab of `member`, saying `error` about the last change if it
/// failed.
pub async fn page(
    app: &App,
    member: &Member,
    status: StatusCode,
    flash: Flash,
    error: Option<&'static str>,
) -> Response {
    let presence = match app.ergo.settings(member.account.as_str()).await {
        Ok(settings) => Ok(Presence::of(&settings, app.presence_defaults)),
        Err(error) => {
            eprintln!("Reading IRC settings failed: {error}");
            Err("Can't reach the chat server.")
        }
    };
    let page = Irc {
        layout: Layout::new(app, Some(&member.account)).on(Tab::Irc),
        flash,
        error: error.filter(|_| presence.is_ok()),
        presence,
        defaults: app.presence_defaults,
    };
    render_with(status, &page)
}
