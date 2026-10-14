//! The account home: how to connect a client.

use askama::Template;
use axum::{extract::State, response::Response};

use super::{Layout, render};
use crate::{app::AppState, auth::CurrentMember, config::IrcServer};

#[derive(Template)]
#[template(path = "account.html")]
struct Account<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    irc: &'a IrcServer,
}

pub async fn show(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    let nick = &me.member.account;
    render(&Account {
        layout: Layout {
            assets: &app.assets,
            nick: Some(nick),
        },
        nick,
        irc: &app.irc_server,
    })
}
