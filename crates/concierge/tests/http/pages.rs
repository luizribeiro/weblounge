use crate::{Site, body_text, location};

#[tokio::test]
async fn healthz_answers_ok() {
    let response = Site::start().await.get("/healthz", None).await;
    assert_eq!(response.status(), 200);
    assert_eq!(body_text(response).await, "ok");
}

#[tokio::test]
async fn house_rules_and_privacy_are_public() {
    let site = Site::start().await;
    for (path, title, says) in [
        (
            "/rules",
            "House rules",
            "You vouch for the people you invite.",
        ),
        ("/privacy", "Privacy", "Members' IP addresses"),
    ] {
        let response = site.get(path, None).await;
        assert_eq!(response.status(), 200, "{path}");
        assert_eq!(
            response.headers()["content-type"],
            "text/html; charset=utf-8"
        );
        let page = body_text(response).await;
        assert!(
            page.contains(&format!("<title>{title} · weblounge.org</title>")),
            "{page}"
        );
        assert!(page.contains(says), "{page}");
    }
}

#[tokio::test]
async fn guests_see_a_sign_in_link_and_members_their_nick() {
    let site = Site::start().await;
    site.add_member("Alice").await;
    let guest = body_text(site.get("/rules", None).await).await;
    assert!(
        guest.contains(r#"<a href="/signin">Sign in</a>"#),
        "{guest}"
    );
    assert!(!guest.contains("Sign out"), "{guest}");

    let cookie = site.sign_in("alice").await;
    let member = body_text(site.get("/rules", Some(&cookie)).await).await;
    assert!(
        member.contains(r#"<span class="session-nick">Alice</span>"#),
        "{member}"
    );
    assert!(
        member.contains(r#"<form method="post" action="/signout">"#),
        "{member}"
    );
    assert!(!member.contains(r#"href="/signin""#), "{member}");
}

#[tokio::test]
async fn every_page_shares_the_header_footer_and_assets() {
    let site = Site::start().await;
    for path in ["/rules", "/privacy", "/signin"] {
        let page = body_text(site.get(path, None).await).await;
        for part in [
            r#"<link rel="stylesheet" href="/assets/members-N4AQSMXV.css">"#,
            r#"<use href="/assets/favicon-E6HOAGDL.svg#mark"/>"#,
            r#"<use href="/assets/wordmark-R5TY7UQ8.svg#wordmark"/>"#,
            r#"<a href="/rules">House rules</a>"#,
            r#"<a href="/privacy">Privacy</a>"#,
        ] {
            assert!(page.contains(part), "{path} lacks {part}: {page}");
        }
        for inline in ["<style", "style=", "<script>", " onclick="] {
            assert!(!page.contains(inline), "{path} has {inline}");
        }
    }
}

#[tokio::test]
async fn pages_carry_the_live_reload_script_only_in_debug_builds() {
    let page = body_text(Site::start().await.get("/rules", None).await).await;
    assert_eq!(
        page.contains("tower-livereload"),
        cfg!(debug_assertions),
        "{page}"
    );
}

#[tokio::test]
async fn hashed_assets_are_served_immutable_with_their_type() {
    let site = Site::start().await;
    for (path, content_type) in [
        ("/assets/members-N4AQSMXV.css", "text/css"),
        ("/assets/favicon-E6HOAGDL.svg", "image/svg+xml"),
        ("/assets/wordmark-R5TY7UQ8.svg", "image/svg+xml"),
    ] {
        let response = site.get(path, None).await;
        assert_eq!(response.status(), 200, "{path}");
        assert_eq!(response.headers()["content-type"], content_type, "{path}");
        assert_eq!(
            response.headers()["cache-control"],
            "public, max-age=31536000, immutable"
        );
    }
}

#[tokio::test]
async fn only_built_assets_are_served() {
    let site = Site::start().await;
    for path in [
        "/assets/manifest.json",
        "/assets/members.css",
        "/assets/..%2Fmanifest.json",
    ] {
        assert_eq!(site.get(path, None).await.status(), 404, "{path}");
    }
}

#[tokio::test]
async fn unknown_paths_are_not_found() {
    assert_eq!(
        Site::start().await.get("/invite/nope", None).await.status(),
        404
    );
}

#[tokio::test]
async fn guests_asking_for_the_account_home_are_sent_to_sign_in() {
    let site = Site::start().await;
    let response = site.get("/", None).await;
    assert_eq!(response.status(), 303);
    assert_eq!(location(&response), "/signin");
    let response = site.get("/?saved", None).await;
    assert_eq!(location(&response), "/signin?next=%2F%3Fsaved");
}

#[tokio::test]
async fn members_are_greeted_at_the_entrance() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let page = body_text(site.get("/", Some(&cookie)).await).await;
    assert!(page.contains("<h1>Members' entrance.</h1>"), "{page}");
}
