pub mod account;
pub mod certificates;
pub mod checks;
pub mod clients;
pub mod home;
pub mod invite;
pub mod invites;
pub mod oauth;
pub mod pages;
pub mod password;
pub mod presence;
pub mod signin;
pub mod verify;

use std::fmt::Display;

use askama::Template;
use axum::{
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
};

use serde::Deserialize;

use crate::{app::App, assets::Assets, auth::Visitor};

/// What pages say when Ergo's API doesn't answer as it should.
pub const ERGO_UNAVAILABLE: &str =
    "Can't reach the chat server. Nothing changed; try again in a minute.";

/// What the shared layout around every page needs: the assets it links,
/// who to greet in the header and which of their tabs is open.
pub struct Layout<'a> {
    pub assets: &'a Assets,
    pub nick: Option<&'a str>,
    tab: Option<Tab>,
}

impl<'a> Layout<'a> {
    pub fn new(app: &'a App, nick: Option<&'a str>) -> Self {
        Self {
            assets: &app.assets,
            nick,
            tab: None,
        }
    }

    pub fn on(self, tab: Tab) -> Self {
        Self {
            tab: Some(tab),
            ..self
        }
    }

    pub fn is_open(&self, tab: &Tab) -> bool {
        self.tab == Some(*tab)
    }
}

/// The member pages in the row under the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Home,
    Invites,
    Account,
}

impl Tab {
    pub const ALL: [Self; 3] = [Self::Home, Self::Invites, Self::Account];

    pub fn path(self) -> &'static str {
        match self {
            Self::Home => "/",
            Self::Invites => "/invites",
            Self::Account => "/account",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Invites => "Invites",
            Self::Account => "Account",
        }
    }
}

/// What the last change, redirected to a tab, says it did.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Flash {
    pub saved: Option<String>,
    pub password: Option<String>,
    pub irc: Option<String>,
}

#[derive(Template)]
#[template(path = "message.html")]
struct Message<'a> {
    layout: Layout<'a>,
    eyebrow: &'a str,
    title: &'a str,
    lines: &'a [&'a str],
}

/// A page that only says something.
pub fn message(
    app: &App,
    visitor: &Visitor,
    status: StatusCode,
    eyebrow: &str,
    title: &str,
    lines: &[&str],
) -> Response {
    let page = Message {
        layout: Layout::new(app, visitor.nick()),
        eyebrow,
        title,
        lines,
    };
    render_with(status, &page)
}

pub fn render(template: &impl Template) -> Response {
    render_with(StatusCode::OK, template)
}

pub fn render_with(status: StatusCode, template: &impl Template) -> Response {
    match template.render() {
        Ok(html) => (status, Html(html)).into_response(),
        Err(error) => internal_error("Rendering a page", error),
    }
}

/// Logs what failed, without anything about the request, and answers 500.
pub fn internal_error(what: &str, error: impl Display) -> Response {
    eprintln!("{what} failed: {error}");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// For pages that carry secrets (invite links, a signup's answer): no
/// cache keeps them.
pub async fn no_store(mut response: Response) -> Response {
    let no_store = HeaderValue::from_static("no-store");
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, no_store);
    response
}
