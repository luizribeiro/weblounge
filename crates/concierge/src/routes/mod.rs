pub mod account;
pub mod checks;
pub mod clients;
pub mod invite;
pub mod invites;
pub mod oauth;
pub mod pages;
pub mod signin;

use std::fmt::Display;

use askama::Template;
use axum::{
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
};

use crate::{app::App, assets::Assets, auth::Visitor};

/// What the shared layout around every page needs: the assets it links and
/// who to greet in the header.
pub struct Layout<'a> {
    pub assets: &'a Assets,
    pub nick: Option<&'a str>,
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
        layout: Layout {
            assets: &app.assets,
            nick: visitor.nick(),
        },
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
