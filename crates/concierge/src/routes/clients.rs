//! Setting up an app, a step at a time: our picks for the visitor's
//! device, every app by name, and each app's guide, prefilled for them.

use askama::Template;
use axum::{
    extract::{Path, State},
    response::Response,
};

use super::{Layout, not_found, render};
use crate::{
    app::AppState,
    auth::Visitor,
    clients::{self, Client, Snippet},
    config::IrcServer,
    oauth::Webchat,
    platform::{Platform, VisitorPlatform},
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
    webchat: &'a Webchat,
}

#[derive(Template)]
#[template(path = "clients.html")]
struct Index<'a> {
    layout: Layout<'a>,
    platform: Option<Platform>,
    picks: Vec<&'a Client>,
    webchat: &'a Webchat,
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

pub async fn index(
    State(app): State<AppState>,
    visitor: Visitor,
    VisitorPlatform(platform): VisitorPlatform,
) -> Response {
    render(&Index {
        layout: Layout::visitor(&app, &visitor),
        platform,
        picks: clients::recommended(platform),
        webchat: &app.webchat,
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
        webchat: &app.webchat,
    })
}
