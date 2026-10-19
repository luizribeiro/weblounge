//! The public pages every footer links to.

use askama::Template;
use axum::{extract::State, response::Response};

use super::{Layout, render};
use crate::{
    app::AppState, auth::Visitor, invite_rules, rate_limit, reset_rules, verification_rules,
};

#[derive(Template)]
#[template(path = "rules.html")]
struct Rules<'a> {
    layout: Layout<'a>,
}

#[derive(Template)]
#[template(path = "privacy.html")]
struct Privacy<'a> {
    layout: Layout<'a>,
    failed_sign_in_minutes: u64,
    link_lifetime: String,
    reset_lifetime: String,
    outbox_wait: String,
}

pub async fn rules(State(app): State<AppState>, visitor: Visitor) -> Response {
    render(&Rules {
        layout: Layout::visitor(&app, &visitor),
    })
}

pub async fn privacy(State(app): State<AppState>, visitor: Visitor) -> Response {
    render(&Privacy {
        layout: Layout::visitor(&app, &visitor),
        failed_sign_in_minutes: rate_limit::WINDOW.as_secs() / 60,
        link_lifetime: invite_rules::in_words(verification_rules::LIFETIME),
        reset_lifetime: invite_rules::in_words(reset_rules::LIFETIME),
        outbox_wait: invite_rules::in_words(outbox::GIVE_UP_AFTER),
    })
}
