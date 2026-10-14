//! The Home tab: how to connect a client.

use askama::Template;
use axum::{extract::State, response::Response};

use super::{Layout, Tab, render};
use crate::{
    app::AppState,
    auth::CurrentMember,
    clients::{CLIENTS, Client},
    config::IrcServer,
    oauth::Webchat,
};

#[derive(Template)]
#[template(path = "home.html")]
struct Home<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    irc: &'a IrcServer,
    clients: &'a [Client],
    webchat: &'a Webchat,
}

pub async fn show(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    let member = &me.member;
    render(&Home {
        layout: Layout::new(&app, Some(&member.account)).on(Tab::Home),
        nick: &member.account,
        irc: &app.irc_server,
        clients: &CLIENTS,
        webchat: &app.webchat,
    })
}
