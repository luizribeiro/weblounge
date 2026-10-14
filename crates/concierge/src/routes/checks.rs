//! What the signup form checks as you type.

use axum::{
    Json,
    response::{IntoResponse, Response},
};

use crate::password_rules;

/// The server's password rules, for the form's live checklist.
pub async fn password_rules() -> Response {
    Json(password_rules::page_rules()).into_response()
}
