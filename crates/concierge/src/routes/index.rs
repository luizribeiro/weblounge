use askama::Template;
use axum::response::Response;

#[derive(Template)]
#[template(path = "index.html")]
struct Index;

pub async fn show() -> Response {
    super::render(&Index)
}
