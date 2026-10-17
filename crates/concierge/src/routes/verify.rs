//! The page a verification link opens. Links carry the code in their
//! fragment, which no server sees, and mail scanners open links, so the
//! page verifies nothing until its button posts the code.

use askama::Template;
use axum::{extract::State, response::Response};

use super::{Layout, render};
use crate::{app::AppState, auth::Visitor, link_code, verification_rules};

#[derive(Template)]
#[template(path = "verify.html")]
struct VerifyPage<'a> {
    layout: Layout<'a>,
    code_pattern: String,
}

pub async fn show(State(app): State<AppState>, visitor: Visitor) -> Response {
    render(&VerifyPage {
        layout: Layout::new(&app, visitor.nick()),
        code_pattern: link_code::html_pattern(verification_rules::CODE_LENGTH),
    })
}
