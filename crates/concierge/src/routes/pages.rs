//! The public pages every footer links to.

use askama::Template;
use axum::{extract::State, response::Response};

use super::{Layout, render};
use crate::{app::AppState, auth::Visitor};

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

pub async fn rules(State(app): State<AppState>, visitor: Visitor) -> Response {
    render(&Rules {
        layout: Layout::new(&app, visitor.nick()),
    })
}

pub async fn privacy(State(app): State<AppState>, visitor: Visitor) -> Response {
    render(&Privacy {
        layout: Layout::new(&app, visitor.nick()),
    })
}
