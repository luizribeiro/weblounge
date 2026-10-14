//! A setup guide per recommended IRC client, prefilled for the visitor.

use askama::Template;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::{Layout, render};
use crate::{
    app::AppState,
    auth::Visitor,
    clients::{self, CLIENTS, Client, Snippet},
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
        layout: Layout::new(&app, nick),
        nick,
        irc: &app.irc_server,
        client,
        snippet: client.snippet(&app.irc_server, nick),
        ircs_link: client.ircs_link(&app.irc_server),
        clients: &CLIENTS,
        webchat: &app.webchat,
    })
}
