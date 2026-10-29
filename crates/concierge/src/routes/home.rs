//! The Home tab: a welcome, and the way into the chat.

use askama::Template;
use axum::{extract::State, response::Response};

use super::{Layout, Tab, render};
use crate::{
    app::AppState,
    auth::CurrentMember,
    oauth::WebClient,
    platform::{Platform, VisitorPlatform},
};

#[derive(Template)]
#[template(path = "home.html")]
struct Home<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    webchat: &'a WebClient,
    platform: Option<Platform>,
}

pub async fn show(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    VisitorPlatform(platform): VisitorPlatform,
) -> Response {
    let nick = me.member.account.as_str();
    render(&Home {
        layout: Layout::member(&app, &me.member).on(Tab::Home),
        nick,
        webchat: &app.web_clients.webchat,
        platform,
    })
}
