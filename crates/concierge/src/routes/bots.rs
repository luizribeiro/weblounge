//! The Bots tab: the bots a member owns, and making, resetting and
//! deleting them. A bot's password is shown once, in the answer to making
//! it or resetting it, and nowhere else. Members act on their own bots
//! only; anyone else's are not found.

use std::time::Instant;

use askama::Template;
use axum::{
    Form,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use ergo_api::Passphrase;
use serde::Deserialize;

use super::{
    ERGO_UNAVAILABLE, Layout, Tab, internal_error, not_found, render, render_with,
    verify::Unconfirmed,
};
use crate::{
    app::{App, AppState},
    auth::CurrentMember,
    bot_rules,
    bots::{Bots, Refusal},
    db::{self, Bot, Member},
    invite_rules,
    name::BotName,
    nick, password,
    rate_limit::{ClientAddress, TRY_LATER},
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
    forms: Forms<'a>,
    name_max_length: usize,
    name_pattern: String,
    name_rule: &'a str,
    /// How long until others may take a deleted bot's name, in words.
    history_kept: String,
}

/// What the last change left on the tab.
#[derive(Default)]
struct Forms<'a> {
    /// The name typed into the form to make a bot.
    name: &'a str,
    name_error: Option<String>,
    /// Why the last change didn't go through.
    alert: Option<String>,
    /// A password just set, the only time it's shown.
    shown: Option<Shown>,
}

/// A bot's password, as it's set.
struct Shown {
    bot: String,
    password: Passphrase,
}

/// What a bot's owner is asked before a change they can't take back, for
/// browsers that can't open the tab's dialog.
#[derive(Template)]
#[template(path = "bot-question.html")]
struct QuestionPage<'a> {
    layout: Layout<'a>,
    bot: String,
    question: Question,
    history_kept: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Question {
    Reset,
    Delete,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct NewBot {
    name: String,
}

pub async fn show(State(app): State<AppState>, CurrentMember(me): CurrentMember) -> Response {
    page(&app, &me.member, StatusCode::OK, Forms::default()).await
}

pub async fn create(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    address: ClientAddress,
    Form(form): Form<NewBot>,
) -> Response {
    let member = &me.member;
    let refuse = |status, name_error, alert| {
        let forms = Forms {
            name: &form.name,
            name_error,
            alert,
            shown: None,
        };
        page(&app, member, status, forms)
    };
    if !app.bot_changes.allows(address.0, Instant::now()) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, None, Some(TRY_LATER.into())).await;
    }
    let live = match app.db.bots(Some(&member.account)).await {
        Ok(bots) => bots.len() as i64,
        Err(error) => return internal_error("Listing a member's bots", error),
    };
    let verified = member.email_verified_at.is_some();
    if let Err(refusal) = bot_rules::may_create(verified, live) {
        return match refusal {
            bot_rules::Refusal::Unverified => refuse(StatusCode::FORBIDDEN, None, None).await,
            bot_rules::Refusal::TooMany => {
                refuse(StatusCode::CONFLICT, None, Some(TOO_MANY.into())).await
            }
        };
    }
    let password = password::generate();
    let created = bots(&app)
        .create(
            &member.account,
            &form.name,
            None,
            &password,
            Some(bot_rules::MAX_LIVE),
        )
        .await;
    match created {
        Ok(Ok(bot)) => {
            let shown = Shown {
                bot: bot.to_string(),
                password,
            };
            show_password(&app, member, shown).await
        }
        Ok(Err(Refusal::InvalidName(invalid))) => {
            refuse(
                StatusCode::UNPROCESSABLE_ENTITY,
                Some(invalid.to_string()),
                None,
            )
            .await
        }
        Ok(Err(Refusal::Taken)) => {
            let taken = format!("{} is taken.", form.name);
            refuse(StatusCode::UNPROCESSABLE_ENTITY, Some(taken), None).await
        }
        Ok(Err(Refusal::TooMany)) => {
            refuse(StatusCode::CONFLICT, None, Some(TOO_MANY.into())).await
        }
        Err(error) => {
            eprintln!("Making a bot failed: {error:#}");
            let alert = match owned_bot(&app, member, &form.name).await {
                Ok(Some(bot)) => half_made(&bot.account),
                _ => ERGO_UNAVAILABLE.into(),
            };
            refuse(StatusCode::SERVICE_UNAVAILABLE, None, Some(alert)).await
        }
    }
}

/// Asks before resetting a bot's password.
pub async fn confirm_reset(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    Path(name): Path<String>,
) -> Response {
    ask(&app, &me.member, &name, Question::Reset).await
}

/// Asks before deleting a bot.
pub async fn confirm_delete(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    Path(name): Path<String>,
) -> Response {
    ask(&app, &me.member, &name, Question::Delete).await
}

async fn ask(app: &App, member: &Member, name: &str, question: Question) -> Response {
    let bot = match owned_or_not_found(app, member, name).await {
        Ok(bot) => bot,
        Err(response) => return response,
    };
    render(&QuestionPage {
        layout: Layout::new(app, Some(member.account.as_str())).on(Tab::Bots),
        bot: bot.account.to_string(),
        question,
        history_kept: invite_rules::in_words(app.history_expiry),
    })
}

/// Erases the member's bot `name` in Ergo, which disconnects it at once,
/// and marks it deleted, keeping its name from others until the chat
/// history has expired for it.
pub async fn delete(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    address: ClientAddress,
    Path(name): Path<String>,
) -> Response {
    let member = &me.member;
    let bot = match owned_or_not_found(&app, member, &name).await {
        Ok(bot) => bot,
        Err(response) => return response,
    };
    if !app.bot_changes.allows(address.0, Instant::now()) {
        return alert(&app, member, StatusCode::TOO_MANY_REQUESTS, TRY_LATER).await;
    }
    match bots(&app).remove(&bot.account).await {
        Ok(()) => Redirect::to("/bots").into_response(),
        Err(error) => {
            eprintln!("Deleting a bot failed: {error:#}");
            alert(
                &app,
                member,
                StatusCode::SERVICE_UNAVAILABLE,
                ERGO_UNAVAILABLE,
            )
            .await
        }
    }
}

/// Gives the member's bot `name` a new generated password and shows it
/// once. Its current one stops working; Ergo keeps a connected bot
/// connected until it reconnects.
pub async fn reset(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    address: ClientAddress,
    Path(name): Path<String>,
) -> Response {
    let member = &me.member;
    let bot = match owned_or_not_found(&app, member, &name).await {
        Ok(bot) => bot,
        Err(response) => return response,
    };
    if !app.bot_changes.allows(address.0, Instant::now()) {
        return alert(&app, member, StatusCode::TOO_MANY_REQUESTS, TRY_LATER).await;
    }
    match bots(&app).reset_password(&bot.account, &bot.owner).await {
        Ok(password) => {
            let shown = Shown {
                bot: bot.account.to_string(),
                password,
            };
            show_password(&app, member, shown).await
        }
        Err(error) => {
            eprintln!("Resetting a bot's password failed: {error:#}");
            alert(
                &app,
                member,
                StatusCode::SERVICE_UNAVAILABLE,
                ERGO_UNAVAILABLE,
            )
            .await
        }
    }
}

/// What the tab says when a bot is refused for the member's having as many
/// as they may.
const TOO_MANY: &str = "That's as many bots as you can have. Delete one to make another.";

/// What the tab says when Ergo made a bot but didn't finish setting it up.
fn half_made(bot: &BotName) -> String {
    format!(
        "Made {bot}, but the chat server didn't finish setting it up. \
         Make it a new password in a minute to try again."
    )
}

fn bots(app: &App) -> Bots<'_> {
    Bots {
        db: &app.db,
        ergo: &app.ergo,
        history_expiry: app.history_expiry,
        uploads: &app.uploads,
    }
}

/// `member`'s live bot named `name`, in any case.
async fn owned_bot(app: &App, member: &Member, name: &str) -> sqlx::Result<Option<Bot>> {
    let bot = app.db.bot(name).await?;
    Ok(bot.filter(|bot| bot.removed_at.is_none() && bot.owner == member.account))
}

/// `member`'s live bot named `name`, or the answer that none was found.
async fn owned_or_not_found(app: &App, member: &Member, name: &str) -> Result<Bot, Response> {
    match owned_bot(app, member, name).await {
        Ok(Some(bot)) => Ok(bot),
        Ok(None) => Err(not_found()),
        Err(error) => Err(internal_error("Looking up a bot", error)),
    }
}

/// The tab saying why the last change didn't go through.
async fn alert(app: &App, member: &Member, status: StatusCode, alert: &str) -> Response {
    let forms = Forms {
        alert: Some(alert.to_owned()),
        ..Forms::default()
    };
    page(app, member, status, forms).await
}

/// The tab with `shown`'s password in a dialog over it.
async fn show_password(app: &App, member: &Member, shown: Shown) -> Response {
    let forms = Forms {
        shown: Some(shown),
        ..Forms::default()
    };
    page(app, member, StatusCode::OK, forms).await
}

/// The Bots tab of `member`, with `forms` filled in.
async fn page(app: &App, member: &Member, status: StatusCode, forms: Forms<'_>) -> Response {
    let bots = match app.db.bots(Some(&member.account)).await {
        Ok(bots) => bots,
        Err(error) => return internal_error("Listing a member's bots", error),
    };
    let verified = member.email_verified_at.is_some();
    let page = BotsPage {
        layout: Layout::new(app, Some(member.account.as_str())).on(Tab::Bots),
        nick: member.account.as_str(),
        remaining: bot_rules::remaining(verified, bots.len() as i64),
        bots,
        unconfirmed: Unconfirmed::of(member, db::now()),
        forms,
        name_max_length: nick::MAX_LENGTH,
        name_pattern: nick::html_pattern(),
        name_rule: nick::RULE,
        history_kept: invite_rules::in_words(app.history_expiry),
    };
    render_with(status, &page)
}
