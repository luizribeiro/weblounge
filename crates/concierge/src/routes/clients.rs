//! Setting up an app, a step at a time: our picks for the visitor's
//! device, every app by name, and each app's guide, prefilled for the
//! member.

use askama::Template;
use axum::{
    extract::{Path, State},
    http::Uri,
    response::{Redirect, Response},
};

use super::{Layout, Tab, not_found, render};
use crate::{
    app::AppState,
    auth::CurrentMember,
    clients::{self, Client, Snippet},
    config::IrcServer,
    oauth::WebClient,
    platform::{Platform, VisitorPlatform},
};

/// Where the guides live, under the Help tab.
pub const PATH: &str = "/help/apps";
pub const ALL_PATH: &str = "/help/apps/all";
/// Where they lived before, which links out there still point to.
pub const OLD_PATH: &str = "/clients";

#[derive(Template)]
#[template(path = "client.html")]
struct Guide<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    irc: &'a IrcServer,
    client: &'a Client,
    snippet: Option<Snippet>,
    ircs_link: Option<String>,
    webchat: &'a WebClient,
}

#[derive(Template)]
#[template(path = "clients.html")]
struct Index<'a> {
    layout: Layout<'a>,
    platform: Option<Platform>,
    picks: Vec<&'a Client>,
    webchat: &'a WebClient,
}

#[derive(Template)]
#[template(path = "clients-all.html")]
struct AllApps<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    irc: &'a IrcServer,
}

pub async fn all(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    render(&AllApps {
        layout: Layout::member(&app, &me.member).on(Tab::Help),
        nick: me.member.account.as_str(),
        irc: &app.irc_server,
    })
}

pub async fn index(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    VisitorPlatform(platform): VisitorPlatform,
) -> Response {
    render(&Index {
        layout: Layout::member(&app, &me.member).on(Tab::Help),
        platform,
        picks: clients::recommended(platform),
        webchat: &app.web_clients.webchat,
    })
}

pub async fn show(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    Path(slug): Path<String>,
) -> Response {
    let Some(client) = clients::by_slug(&slug) else {
        return not_found();
    };
    let nick = me.member.account.as_str();
    render(&Guide {
        layout: Layout::member(&app, &me.member).on(Tab::Help),
        nick,
        irc: &app.irc_server,
        client,
        snippet: client.snippet(&app.irc_server, nick),
        ircs_link: client.ircs_link(&app.irc_server),
        webchat: &app.web_clients.webchat,
    })
}

/// Sends a link to an old guide's address on to its new one, query and all.
pub async fn moved(uri: Uri) -> Redirect {
    let old = uri.path_and_query().map_or(OLD_PATH, |path| path.as_str());
    let rest = old.strip_prefix(OLD_PATH).unwrap_or_default();
    Redirect::permanent(&format!("{PATH}{rest}"))
}
