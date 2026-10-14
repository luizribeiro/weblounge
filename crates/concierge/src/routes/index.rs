use askama::Template;
use axum::{extract::State, response::Response};

use super::Layout;
use crate::{app::AppState, auth::CurrentMember};

#[derive(Template)]
#[template(path = "index.html")]
struct Index<'a> {
    layout: Layout<'a>,
}

pub async fn show(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    super::render(&Index {
        layout: Layout {
            assets: &app.assets,
            nick: Some(&me.member.account),
        },
    })
}
