use askama::Template;
use axum::{extract::State, response::Response};

use super::Layout;
use crate::app::AppState;

#[derive(Template)]
#[template(path = "index.html")]
struct Index<'a> {
    layout: Layout<'a>,
}

pub async fn show(State(app): State<AppState>) -> Response {
    super::render(&Index {
        layout: Layout {
            assets: &app.assets,
        },
    })
}
