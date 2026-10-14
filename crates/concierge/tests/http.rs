use axum::{body::Body, http::Request, response::Response};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn get(path: &str) -> Response {
    concierge::app::router()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body_text(response: Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn healthz_answers_ok() {
    let response = get("/healthz").await;
    assert_eq!(response.status(), 200);
    assert_eq!(body_text(response).await, "ok");
}

#[tokio::test]
async fn the_index_greets_visitors_at_the_members_entrance() {
    let response = get("/").await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    let page = body_text(response).await;
    assert!(
        page.contains("<title>Members · weblounge.org</title>"),
        "{page}"
    );
    assert!(page.contains("<h1>Members' entrance.</h1>"), "{page}");
}

#[tokio::test]
async fn unknown_paths_are_not_found() {
    assert_eq!(get("/invite/nope").await.status(), 404);
}
