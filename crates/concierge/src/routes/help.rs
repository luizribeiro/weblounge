//! The Help tab: an index of topics.

use askama::Template;
use axum::{extract::State, response::Response};

use super::{Layout, Tab, render};
use crate::{
    app::AppState,
    auth::CurrentMember,
    help::{self, Topic},
};

pub const PATH: &str = "/help";

#[derive(Template)]
#[template(path = "help.html")]
struct Index<'a> {
    layout: Layout<'a>,
    topics: Vec<Topic>,
    lounge: String,
}

pub async fn index(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    render(&Index {
        layout: Layout::member(&app, &me.member).on(Tab::Help),
        topics: help::index(),
        lounge: app.webchat.redirect_uri(),
    })
}
