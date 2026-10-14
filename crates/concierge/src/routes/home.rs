//! The Home tab: how to connect a client, and the member's invites.

use askama::Template;
use axum::{
    extract::{Query, State},
    response::Response,
};

use super::{Flash, Layout, Tab, internal_error, invites::Invites, render};
use crate::{
    app::AppState,
    auth::CurrentMember,
    clients::{CLIENTS, Client},
    config::IrcServer,
    db,
    oauth::Webchat,
};

#[derive(Template)]
#[template(path = "home.html")]
struct Home<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    irc: &'a IrcServer,
    clients: &'a [Client],
    webchat: &'a Webchat,
    invites: Invites,
    flash: Flash,
}

pub async fn show(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    Query(flash): Query<Flash>,
) -> Response {
    let member = &me.member;
    let invites = match Invites::of(&app, member, db::now()).await {
        Ok(invites) => invites,
        Err(error) => return internal_error("Listing invites", error),
    };
    render(&Home {
        layout: Layout::new(&app, Some(&member.account)).on(Tab::Home),
        nick: &member.account,
        irc: &app.irc_server,
        clients: &CLIENTS,
        webchat: &app.webchat,
        invites,
        flash,
    })
}
