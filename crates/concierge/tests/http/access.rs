//! What the site shows someone signed out: what they need to get in, and
//! the sign-in page in place of everything else.

use crate::{PASSWORD, Site, body_text, location, session_cookie, sign_in_url};

/// The pages someone signed out reads, or uses to sign in, sign up, confirm
/// an email or reset a password.
const PUBLIC_PAGES: [&str; 8] = [
    "/signin",
    "/rules",
    "/privacy",
    "/invite",
    "/verify",
    "/forgot-password",
    "/reset",
    "/password-rules.json",
];

/// What machines ask for, signed in or not.
const MACHINE_PATHS: [&str; 3] = [
    "/healthz",
    "/.well-known/oauth-authorization-server",
    "/assets/members-N4AQSMXV.css",
];

/// Every page to look at that needs a member, with an unknown path, a
/// query, which the sign-in page must keep, and the guides' old addresses.
const MEMBER_PAGES: [&str; 20] = [
    "/?saved",
    "/account",
    "/irc",
    "/invites",
    "/invites/revoke",
    "/bots",
    "/bots/somebot/password",
    "/bots/somebot/delete",
    "/uploads",
    "/uploads/abc/delete",
    "/help/apps",
    "/help",
    "/help/make-a-bot",
    "/help/apps/all",
    "/help/apps/halloy",
    "/help/apps/halloy/certificate",
    "/help/apps/halloy?from=welcome",
    "/clients",
    "/clients/halloy?from=welcome",
    "/no-such-page",
];

#[tokio::test]
async fn what_someone_signed_out_needs_answers_them() {
    let site = Site::start().await;
    for path in PUBLIC_PAGES.iter().chain(&MACHINE_PATHS) {
        let response = site.get(path, None).await;
        assert_eq!(response.status(), 200, "{path}");
    }
}

#[tokio::test]
async fn the_checks_signup_makes_answer_signed_out() {
    let site = Site::start().await;
    for (path, field) in [("/signup/nick", "nick"), ("/signup/invite", "code")] {
        let response = site.post(path, &[(field, "someone")], None).await;
        assert_eq!(response.status(), 200, "{path}");
    }
}

#[tokio::test]
async fn every_other_page_sends_someone_signed_out_to_sign_in_and_back() {
    let site = Site::start().await;
    for path in MEMBER_PAGES {
        let response = site.get(path, None).await;
        assert_eq!(response.status(), 303, "{path}");
        assert_eq!(location(&response), sign_in_url(path), "{path}");
    }
}

#[tokio::test]
async fn posting_to_a_members_page_signed_out_leads_to_plain_sign_in() {
    let site = Site::start().await;
    for path in ["/invites", "/bots", "/bots/name", "/no-such-page"] {
        let response = site.post(path, &[], None).await;
        assert_eq!(response.status(), 303, "{path}");
        assert_eq!(location(&response), "/signin", "{path}");
    }
}

#[tokio::test]
async fn signing_in_from_a_members_page_lands_back_on_it() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let wanted = "/help/apps/halloy?from=welcome";
    let sign_in = site.get(wanted, None).await;
    let page = body_text(site.get(location(&sign_in), None).await).await;
    assert!(
        page.contains(&format!(
            r#"<input type="hidden" name="next" value="{}">"#,
            wanted.replace('&', "&#38;")
        )),
        "{page}"
    );

    let form = [("nick", "alice"), ("password", PASSWORD), ("next", wanted)];
    let response = site.post("/signin", &form, None).await;
    assert_eq!(response.status(), 303);
    assert_eq!(location(&response), wanted);
    let cookie = session_cookie(&response).unwrap();
    assert_eq!(site.get(wanted, Some(&cookie)).await.status(), 200);
}

#[tokio::test]
async fn a_sign_in_link_cannot_send_anyone_off_the_site() {
    let site = Site::start().await;
    site.add_member("alice").await;
    for next in [
        "//evil.example/clients",
        "https://evil.example/clients",
        "/\\evil.example",
        "evil.example",
    ] {
        let page = body_text(site.get(&sign_in_url(next), None).await).await;
        assert!(!page.contains(r#"name="next""#), "{next}: {page}");
        let form = [("nick", "alice"), ("password", PASSWORD), ("next", next)];
        let response = site.post("/signin", &form, None).await;
        assert_eq!(location(&response), "/", "{next}");
    }
}
