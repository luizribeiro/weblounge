use std::path::Path;

use axum::{body::Body, http::Request, response::Response};
use concierge::assets::Assets;
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn get(path: &str) -> Response {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/assets");
    concierge::app::router(Assets::load(&fixtures).unwrap())
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
async fn pages_carry_the_live_reload_script_only_in_debug_builds() {
    let page = body_text(get("/").await).await;
    assert_eq!(
        page.contains("tower-livereload"),
        cfg!(debug_assertions),
        "{page}"
    );
}

#[tokio::test]
async fn the_index_links_its_hashed_stylesheet_and_favicon() {
    let page = body_text(get("/").await).await;
    assert!(
        page.contains(r#"<link rel="stylesheet" href="/assets/members-N4AQSMXV.css">"#),
        "{page}"
    );
    assert!(
        page.contains(r#"href="/assets/favicon-E6HOAGDL.svg""#),
        "{page}"
    );
}

#[tokio::test]
async fn hashed_assets_are_served_immutable() {
    let response = get("/assets/members-N4AQSMXV.css").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "text/css");
    assert_eq!(
        response.headers()["cache-control"],
        "public, max-age=31536000, immutable"
    );
    assert!(body_text(response).await.contains("--paper: #f3f1e9;"));
}

#[tokio::test]
async fn the_favicon_is_served_as_svg() {
    let response = get("/assets/favicon-E6HOAGDL.svg").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "image/svg+xml");
}

#[tokio::test]
async fn only_built_assets_are_served() {
    for path in [
        "/assets/manifest.json",
        "/assets/members.css",
        "/assets/..%2Fmanifest.json",
    ] {
        assert_eq!(get(path).await.status(), 404, "{path}");
    }
}

#[tokio::test]
async fn unknown_paths_are_not_found() {
    assert_eq!(get("/invite/nope").await.status(), 404);
}
