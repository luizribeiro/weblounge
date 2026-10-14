pub mod index;
pub mod pages;

use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

use crate::assets::Assets;

/// What the layout shared by every page needs.
pub struct Layout<'a> {
    pub assets: &'a Assets,
}

pub fn render(template: &impl Template) -> Response {
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(error) => {
            eprintln!("Rendering a page failed: {error}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
