//! The clients page, connection details and every client at a glance,
//! and a setup guide per recommended client, prefilled for the visitor.

use askama::Template;
use axum::{
    extract::{Path, State},
    response::Response,
};

use super::{Layout, not_found, render};
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

#[derive(Template)]
#[template(path = "clients.html")]
struct Index<'a> {
    layout: Layout<'a>,
    nick: Option<&'a str>,
    irc: &'a IrcServer,
    clients: &'a [Client],
}

#[derive(Template)]
#[template(path = "clients-all.html")]
struct AllApps<'a> {
    layout: Layout<'a>,
    nick: Option<&'a str>,
    irc: &'a IrcServer,
}

pub async fn all(State(app): State<AppState>, visitor: Visitor) -> Response {
    render(&AllApps {
        layout: Layout::visitor(&app, &visitor),
        nick: visitor.nick(),
        irc: &app.irc_server,
    })
}

pub async fn index(State(app): State<AppState>, visitor: Visitor) -> Response {
    let nick = visitor.nick();
    render(&Index {
        layout: Layout::visitor(&app, &visitor),
        nick,
        irc: &app.irc_server,
        clients: &CLIENTS,
    })
}

pub async fn show(
    State(app): State<AppState>,
    visitor: Visitor,
    Path(slug): Path<String>,
) -> Response {
    let Some(client) = clients::by_slug(&slug) else {
        return not_found();
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
