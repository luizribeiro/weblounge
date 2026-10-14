//! The invite tree, for members only.

use askama::Template;
use axum::{extract::State, response::Response};

use super::{Layout, internal_error, render};
use crate::{
    app::AppState,
    auth::CurrentMember,
    tree::{self, Line},
};

#[derive(Template)]
#[template(path = "tree.html")]
struct Tree<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    lines: Vec<Line>,
}

pub async fn show(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    let branches = match app.db.invite_tree().await {
        Ok(branches) => branches,
        Err(error) => return internal_error("Reading the invite tree", error),
    };
    let nick = me.member.account.as_str();
    render(&Tree {
        layout: Layout {
            assets: &app.assets,
            nick: Some(nick),
        },
        nick,
        lines: tree::lines(branches),
    })
}
