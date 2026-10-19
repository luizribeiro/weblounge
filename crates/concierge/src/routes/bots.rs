//! The Bots tab: the bots a member owns, and making, resetting and
//! deleting them.

use askama::Template;
use axum::{extract::State, http::StatusCode, response::Response};

use super::{Layout, Tab, internal_error, render_with, verify::Unconfirmed};
use crate::{
    app::{App, AppState},
    auth::CurrentMember,
    bot_rules,
    db::{self, Bot, Member},
};

#[derive(Template)]
#[template(path = "bots.html")]
struct BotsPage<'a> {
    layout: Layout<'a>,
    nick: &'a str,
    bots: Vec<Bot>,
    /// How many more the member may make now.
    remaining: i64,
    /// Where confirming the member's email stands, until they do; making
    /// bots waits on it.
    unconfirmed: Option<Unconfirmed>,
}

pub async fn show(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    page(&app, &me.member, StatusCode::OK).await
}

/// The Bots tab of `member`.
async fn page(app: &App, member: &Member, status: StatusCode) -> Response {
    let bots = match app.db.bots(Some(&member.account)).await {
        Ok(bots) => bots,
        Err(error) => return internal_error("Listing a member's bots", error),
    };
    let verified = member.email_verified_at.is_some();
    let page = BotsPage {
        layout: Layout::new(app, Some(&member.account)).on(Tab::Bots),
        nick: &member.account,
        remaining: bot_rules::remaining(verified, bots.len() as i64),
        bots,
        unconfirmed: Unconfirmed::of(member, db::now()),
    };
    render_with(status, &page)
}
