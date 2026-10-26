use concierge::rate_limit;

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
async fn privacy_counts_failed_sign_ins_for_as_long_as_the_rate_limiter() {
    let page = body_text(Site::start().await.get("/privacy", None).await).await;
    let minutes = rate_limit::WINDOW.as_secs() / 60;
    assert!(
        page.contains(&format!(
            "email confirmations and password resets are counted per address, \
             in memory, for {minutes} minutes."
        )),
        "{page}"
    );
}

#[tokio::test]
async fn privacy_says_how_long_mail_and_its_links_last() {
    let page = body_text(Site::start().await.get("/privacy", None).await).await;
    assert!(page.contains("We send email through MXroute"), "{page}");
    assert!(
        page.contains("Emails about new features and planned downtime are opt-in"),
        "{page}"
    );
    assert!(page.contains("until it's sent, 2 days at most;"), "{page}");
    assert!(
        page.contains(
            "only as a hash: links to confirm your address expire after 2 days, \
             and links to reset your password after 1 hour."
        ),
        "{page}"
    );
}

#[tokio::test]
async fn privacy_says_how_long_messages_last_and_that_others_may_keep_copies() {
    let page = body_text(Site::start().await.get("/privacy", None).await).await;
    assert!(
        page.contains("direct messages are kept on the server for 2 weeks, so you can scroll back"),
        "{page}"
    );
    assert!(
        page.contains("other people's apps may keep their own copy"),
        "{page}"
    );
    assert!(!page.contains("dm-history"), "{page}");
}

#[tokio::test]
async fn privacy_says_who_a_welcome_names_and_how_long_its_record_stays() {
    let page = body_text(Site::start().await.get("/privacy", None).await).await;
    assert!(
        page.contains(
            "The first time you connect after accepting an invite, the concierge, \
             the network's resident bot, posts a welcome in #lounge naming you and \
             whoever invited you, tells them by direct message, and sends you a hello \
             by direct message. If you haven't connected 2 days after accepting, \
             it tells whoever invited you, once."
        ),
        "{page}"
    );
    assert!(
        page.contains("We keep a record of when each of these happened for 2 weeks."),
        "{page}"
    );
}

#[tokio::test]
async fn privacy_says_how_long_ergo_keeps_messages_as_its_setting_does() {
    let site = Site::start_with(|app| app.history_expiry = 10 * 24 * 60 * 60).await;
    let page = body_text(site.get("/privacy", None).await).await;
    assert!(
        page.contains("direct messages are kept on the server for 10 days, so you can scroll back"),
        "{page}"
    );
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
            r#"<script src="/assets/members-7QK2WZ3D.js" defer></script>"#,
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
async fn only_members_see_the_footer_link_to_github() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let link = r#"<a href="https://github.com/luizribeiro/weblounge">GitHub</a>"#;
    let guest = body_text(site.get("/rules", None).await).await;
    assert!(!guest.contains("github.com"), "{guest}");

    let cookie = site.sign_in("alice").await;
    let member = body_text(site.get("/rules", Some(&cookie)).await).await;
    assert!(member.contains(link), "{member}");
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
        ("/assets/members-7QK2WZ3D.js", "text/javascript"),
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
        let response = site.get(path, None).await;
        assert_eq!(response.status(), 404, "{path}");
        assert_eq!(body_text(response).await, "", "{path}");
    }
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
async fn privacy_says_uploads_are_public_by_link_stripped_and_kept_thirty_days() {
    let page = body_text(Site::start().await.get("/privacy", None).await).await;
    for promise in [
        "<h2>Uploads</h2>",
        "is public to anyone with its link.",
        "Where and when a photo was taken, the camera and anything else stored in the file are removed",
        "never where from.",
        "Uploads are deleted after 30 days, and you can delete them sooner on your Uploads tab.",
        "your email, sessions, uploads and bots go",
    ] {
        assert!(page.contains(promise), "{promise}: {page}");
    }
}
