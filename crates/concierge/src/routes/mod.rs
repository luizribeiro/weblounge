pub mod account;
pub mod bots;
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
pub mod reset;
pub mod signin;
pub mod upload;
pub mod verify;

use std::fmt::Display;

use askama::Template;
use axum::{
    Extension,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{Html, IntoResponse, Response},
};

use serde::Deserialize;

use crate::{
    app::{App, AppState},
    assets::Assets,
    auth::Visitor,
    db::{self, Member},
};
use verify::Unconfirmed;

/// What pages say when a link couldn't be mailed, as while no mailer runs.
pub const UNSENT: &str = "Couldn't send a link just now. Try again in a minute.";

/// What pages say when Ergo's API doesn't answer as it should.
pub const ERGO_UNAVAILABLE: &str =
    "Can't reach the chat server. Nothing changed; try again in a minute.";

/// What the shared layout around every page needs: the assets it links,
/// who to greet in the header, which of their tabs is open and whether to
/// remind them to confirm their email.
pub struct Layout<'a> {
    pub assets: &'a Assets,
    pub nick: Option<&'a str>,
    tab: Option<Tab>,
    pub unconfirmed: Option<Unconfirmed>,
}

impl<'a> Layout<'a> {
    pub fn new(app: &'a App, nick: Option<&'a str>) -> Self {
        Self {
            assets: &app.assets,
            nick,
            tab: None,
            unconfirmed: None,
        }
    }

    /// The layout of `member`'s pages, reminding them to confirm their
    /// email until they do. The Account, Invites and Bots tabs say so in
    /// their own cards, so they use [`Layout::new`].
    pub fn member(app: &'a App, member: &'a Member) -> Self {
        Self {
            unconfirmed: Unconfirmed::of(member, db::now()),
            ..Self::new(app, Some(member.account.as_str()))
        }
    }

    /// The layout of a page anyone may see, as [`Layout::member`] for
    /// members.
    pub fn visitor(app: &'a App, visitor: &'a Visitor) -> Self {
        match &visitor.0 {
            Some(signed_in) => Self::member(app, &signed_in.member),
            None => Self::new(app, None),
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
    Bots,
    Account,
}

impl Tab {
    pub const ALL: [Self; 4] = [Self::Home, Self::Invites, Self::Bots, Self::Account];

    pub fn path(self) -> &'static str {
        match self {
            Self::Home => "/",
            Self::Invites => "/invites",
            Self::Bots => "/bots",
            Self::Account => "/account",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Invites => "Invites",
            Self::Bots => "Bots",
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
    /// A link to confirm the email went out.
    pub sent: Option<String>,
}

#[derive(Template)]
#[template(path = "message.html")]
struct Message<'a> {
    layout: Layout<'a>,
    eyebrow: &'a str,
    title: &'a str,
    lines: &'a [&'a str],
    next: Option<Link<'a>>,
    /// Errors stand alone on a card, as the sign-in form does.
    card: bool,
}

/// Where a page sends the reader next.
pub struct Link<'a> {
    pub href: &'a str,
    pub label: &'a str,
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
    message_with_link(app, visitor, status, eyebrow, title, lines, None)
}

/// A page that says something and offers a way on.
pub fn message_with_link(
    app: &App,
    visitor: &Visitor,
    status: StatusCode,
    eyebrow: &str,
    title: &str,
    lines: &[&str],
    next: Option<Link>,
) -> Response {
    let page = Message {
        layout: Layout::new(app, visitor.nick()),
        eyebrow,
        title,
        lines,
        next,
        card: false,
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

/// What a page that doesn't exist answers.
pub fn not_found() -> Response {
    (StatusCode::NOT_FOUND, Extension(ErrorPage::NotFound)).into_response()
}

/// An error [`error_pages`] gives a page of its own.
#[derive(Clone, Copy)]
enum ErrorPage {
    NotFound,
    Internal,
}

impl ErrorPage {
    /// The page's eyebrow, title and line.
    fn message(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::NotFound => (
                "Not found",
                "There's nothing here.",
                "Check the address, or go back home.",
            ),
            Self::Internal => (
                "Error",
                "Something went wrong on our end.",
                "Nothing you did caused this. Try again in a minute.",
            ),
        }
    }
}

/// Gives the errors handlers answer a page, for whoever asked: members
/// keep their header and tabs. The errors' status and headers stay, and
/// an error whose page can't render either, as when assets are missing,
/// answers bare.
pub async fn error_pages(State(app): State<AppState>, request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    let Some(&error) = response.extensions().get::<ErrorPage>() else {
        return response;
    };
    let visitor = response.extensions().get::<Visitor>().cloned();
    let (eyebrow, title, line) = error.message();
    let page = Message {
        layout: Layout::new(&app, visitor.as_ref().and_then(Visitor::nick)),
        eyebrow,
        title,
        lines: &[line],
        next: Some(Link {
            href: Tab::Home.path(),
            label: "Go home",
        }),
        card: true,
    };
    let (parts, body) = response.into_parts();
    match page.render() {
        Ok(html) => (parts, Html(html)).into_response(),
        Err(error) => {
            eprintln!("Rendering an error page failed: {error}");
            Response::from_parts(parts, body)
        }
    }
}

/// Logs what failed, without anything about the request, and answers 500.
pub fn internal_error(what: &str, error: impl Display) -> Response {
    eprintln!("{what} failed: {error}");
    let error = Extension(ErrorPage::Internal);
    (StatusCode::INTERNAL_SERVER_ERROR, error).into_response()
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
