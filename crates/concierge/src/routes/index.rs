use askama::Template;
use axum::{extract::State, response::Response};

use crate::{app::AppState, assets::Assets};

#[derive(Template)]
#[template(path = "index.html")]
struct Index<'a> {
    assets: &'a Assets,
}

pub async fn show(State(assets): State<AppState>) -> Response {
    super::render(&Index { assets: &assets })
}
