//! The public pages every footer links to.

use askama::Template;
use axum::{extract::State, response::Response};

use super::{Layout, render};
use crate::app::AppState;

#[derive(Template)]
#[template(path = "rules.html")]
struct Rules<'a> {
    layout: Layout<'a>,
}

#[derive(Template)]
#[template(path = "privacy.html")]
struct Privacy<'a> {
    layout: Layout<'a>,
}

pub async fn rules(State(app): State<AppState>) -> Response {
    render(&Rules {
        layout: Layout {
            assets: &app.assets,
        },
    })
}

pub async fn privacy(State(app): State<AppState>) -> Response {
    render(&Privacy {
        layout: Layout {
            assets: &app.assets,
        },
    })
}
