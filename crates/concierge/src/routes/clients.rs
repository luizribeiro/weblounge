//! The clients page, connection details and every client at a glance,
//! and a setup guide per recommended client, prefilled for the visitor.

use askama::Template;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::{Layout, certificates::Certificates, render};
use crate::{
    app::AppState,
    auth::Visitor,
    clients::{self, CLIENTS, Client, NICK_PLACEHOLDER, Snippet},
    config::IrcServer,
    oauth::Webchat,
};

#[derive(Template)]
#[template(path = "client.html")]
struct Guide<'a> {
    layout: Layout<'a>,
    nick: Option<&'a str>,
    irc: &'a IrcServer,
    client: &'a Client,
    snippet: Option<Snippet>,
    ircs_link: Option<String>,
    clients: &'a [Client],
    webchat: &'a Webchat,
}

#[derive(Template)]
#[template(path = "clients.html")]
struct Index<'a> {
    layout: Layout<'a>,
    nick: Option<&'a str>,
    irc: &'a IrcServer,
    clients: &'a [Client],
    certificates: Certificates,
}

pub async fn index(State(app): State<AppState>, visitor: Visitor) -> Response {
    let nick = visitor.nick();
    render(&Index {
        layout: Layout::visitor(&app, &visitor),
        nick,
        irc: &app.irc_server,
        clients: &CLIENTS,
        certificates: Certificates::for_nick(nick.unwrap_or(NICK_PLACEHOLDER)),
    })
}

pub async fn show(
    State(app): State<AppState>,
    visitor: Visitor,
    Path(slug): Path<String>,
) -> Response {
    let Some(client) = clients::by_slug(&slug) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let nick = visitor.nick();
    render(&Guide {
        layout: Layout::visitor(&app, &visitor),
        nick,
        irc: &app.irc_server,
        client,
        snippet: client.snippet(&app.irc_server, nick),
        ircs_link: client.ircs_link(&app.irc_server),
        clients: &CLIENTS,
        webchat: &app.webchat,
    })
}
