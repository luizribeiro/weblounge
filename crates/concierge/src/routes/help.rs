//! The Help tab: an index of topics, and an article on each.

use askama::Template;
use axum::{
    extract::{Path, State},
    response::Response,
};

use super::{Layout, Tab, not_found, render};
use crate::{
    app::AppState,
    auth::CurrentMember,
    help::{self, Article, Facts, Topic},
};

pub const PATH: &str = "/help";

#[derive(Template)]
#[template(path = "help.html")]
struct Index<'a> {
    layout: Layout<'a>,
    topics: Vec<Topic>,
    lounge: String,
}

#[derive(Template)]
#[template(path = "help-article.html")]
struct ArticlePage<'a> {
    layout: Layout<'a>,
    article: Article,
    lounge: String,
}

pub async fn index(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    let facts = Facts::new(&app, me.member.account.as_str());
    render(&Index {
        layout: Layout::member(&app, &me.member).on(Tab::Help),
        topics: help::index(&facts),
        lounge: facts.webchat,
    })
}

pub async fn show(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    Path(slug): Path<String>,
) -> Response {
    let facts = Facts::new(&app, me.member.account.as_str());
    let Some(article) = help::article(&slug, &facts) else {
        return not_found();
    };
    render(&ArticlePage {
        layout: Layout::member(&app, &me.member).on(Tab::Help),
        article,
        lounge: facts.webchat,
    })
}
