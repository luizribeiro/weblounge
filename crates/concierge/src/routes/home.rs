//! The Home tab: a welcome, and the way into the chat.

use askama::Template;
use axum::{extract::State, response::Response};

use super::{Layout, Tab, render};
use crate::{app::AppState, auth::CurrentMember, oauth::Webchat};

#[derive(Template)]
#[template(path = "home.html")]
struct Home<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    webchat: &'a Webchat,
}

pub async fn show(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    let nick = me.member.account.as_str();
    render(&Home {
        layout: Layout::member(&app, &me.member).on(Tab::Home),
        nick,
        webchat: &app.webchat,
    })
}
