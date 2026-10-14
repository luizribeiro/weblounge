pub mod index;
pub mod pages;
pub mod signin;

use std::fmt::Display;

use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

use crate::assets::Assets;

/// What the shared layout around every page needs: the assets it links and
/// who to greet in the header.
pub struct Layout<'a> {
    pub assets: &'a Assets,
    pub nick: Option<&'a str>,
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
