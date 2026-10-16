use axum::http::header;
use concierge::invite_rules::MAX_OPEN;
use regex::Regex;

use crate::{JOIN_URL, Site, body_text, location};

fn invite_links(page: &str) -> Vec<String> {
    let link = Regex::new(r#"<code class="invite-link">(https://[^<]+)</code>"#).unwrap();
    link.captures_iter(page).map(|c| c[1].to_owned()).collect()
}

async fn invites_tab(site: &Site, cookie: &str) -> String {
    body_text(site.get("/invites", Some(cookie)).await).await
}

/// Creates an invite as the member signed in with `cookie`, and answers
/// with the link the page shows.
async fn create_invite(site: &Site, cookie: &str) -> String {
    let page = body_text(site.post("/invites", &[], Some(cookie)).await).await;
    let [link] = invite_links(&page).try_into().unwrap();
    link
}

async fn invite_count(site: &Site) -> i64 {
    site.query_scalar("SELECT COUNT(*) FROM invites").await
}

#[tokio::test]
async fn a_new_invite_shows_its_link_and_qr_code_once() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;

    let response = site.post("/invites", &[], Some(&cookie)).await;

    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let page = body_text(response).await;
    let [link] = invite_links(&page).try_into().unwrap();
    assert!(link.starts_with(&format!("{JOIN_URL}/#")), "{link}");
    assert!(page.contains(&format!(r#"data-copy="{link}""#)), "{page}");
    assert!(page.contains(&concierge::qr::svg(&link)), "{page}");
    assert!(page.contains("works once, for 24 hours"), "{page}");

    let tab = invites_tab(&site, &cookie).await;
    assert!(invite_links(&tab).is_empty(), "{tab}");
    assert!(tab.contains(&available(2)), "{tab}");
    let code = link.rsplit('#').next().unwrap();
    let invite = site.db.invite(code).await.unwrap().unwrap();
    assert_eq!(invite.created_by.as_deref(), Some("alice"));
}

/// How the Invites tab says `n` more invites may be made.
fn available(n: i64) -> String {
    format!(r#"<span class="invite-count-number">{n} of {MAX_OPEN}</span> invites available"#)
}

const INVITE_BUTTON: &str =
    r#"<button class="button button-primary" type="submit">Invite someone</button>"#;

/// alice, signed in with the answered cookie, with as many open invites
/// as anyone may have.
async fn alice_with_every_invite_open(site: &Site) -> String {
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for _ in 0..MAX_OPEN {
        create_invite(site, &cookie).await;
    }
    cookie
}

#[tokio::test]
async fn open_invites_are_capped_and_the_tab_says_so() {
    let site = Site::start().await;
    let cookie = alice_with_every_invite_open(&site).await;

    let response = site.post("/invites", &[], Some(&cookie)).await;

    assert_eq!(location(&response), "/invites");
    assert_eq!(invite_count(&site).await, MAX_OPEN);
    let page = invites_tab(&site, &cookie).await;
    assert!(page.contains(&available(0)), "{page}");
    assert!(
        page.contains(r#"type="submit" disabled>Invite someone</button>"#),
        "{page}"
    );
}

#[tokio::test]
async fn the_tab_counts_the_invites_left_and_says_when_the_oldest_expires() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let page = invites_tab(&site, &cookie).await;
    assert!(page.contains(&available(3)), "{page}");
    assert!(page.contains(INVITE_BUTTON), "{page}");
    assert!(page.contains("expires after 24 hours."), "{page}");
    assert!(!page.contains("oldest unused"), "{page}");

    create_invite(&site, &cookie).await;
    site.execute("UPDATE invites SET expires_at = unixepoch() + 5 * 3600 + 60")
        .await;
    create_invite(&site, &cookie).await;
    let page = invites_tab(&site, &cookie).await;
    assert!(page.contains(&available(1)), "{page}");
    assert!(page.contains(INVITE_BUTTON), "{page}");
    assert!(
        page.contains("Your oldest unused invite expires in 6 hours."),
        "{page}"
    );
}

#[tokio::test]
async fn an_expired_or_redeemed_invite_frees_its_place() {
    let site = Site::start().await;
    let cookie = alice_with_every_invite_open(&site).await;
    site.execute(
        "INSERT INTO members (account, invited_by, joined_at) VALUES ('bob', 'alice', 1);
         UPDATE invites SET redeemed_by = 'bob', redeemed_at = 1
         WHERE rowid = (SELECT MIN(rowid) FROM invites);
         UPDATE invites SET created_at = 0, expires_at = 1
         WHERE rowid = (SELECT MAX(rowid) FROM invites);",
    )
    .await;

    create_invite(&site, &cookie).await;
    create_invite(&site, &cookie).await;

    assert_eq!(invite_count(&site).await, MAX_OPEN + 2);
}

#[tokio::test]
async fn admins_are_held_to_the_same_cap() {
    let site = Site::start().await;
    site.add_member("root").await;
    site.execute("UPDATE members SET is_admin = TRUE WHERE account = 'root'")
        .await;
    let cookie = site.sign_in("root").await;
    for _ in 0..MAX_OPEN + 1 {
        site.post("/invites", &[], Some(&cookie)).await;
    }
    assert_eq!(invite_count(&site).await, MAX_OPEN);
}

#[tokio::test]
async fn two_invites_at_once_cannot_both_take_the_last_place() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for _ in 0..MAX_OPEN - 1 {
        create_invite(&site, &cookie).await;
    }

    tokio::join!(
        site.post("/invites", &[], Some(&cookie)),
        site.post("/invites", &[], Some(&cookie))
    );

    assert_eq!(invite_count(&site).await, MAX_OPEN);
}

#[tokio::test]
async fn revoking_frees_every_place_and_ends_the_links() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let mut links = Vec::new();
    for _ in 0..MAX_OPEN {
        links.push(create_invite(&site, &cookie).await);
    }
    let page = invites_tab(&site, &cookie).await;
    assert!(page.contains(r#"href="/invites/revoke""#), "{page}");

    let response = site.post("/invites/revoke", &[], Some(&cookie)).await;

    assert_eq!(location(&response), "/invites");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    for link in &links {
        let code = link.rsplit('#').next().unwrap();
        let invite = site.db.invite(code).await.unwrap().unwrap();
        assert!(invite.revoked_at.is_some(), "{link}");
    }
    let page = invites_tab(&site, &cookie).await;
    assert!(page.contains(&available(MAX_OPEN)), "{page}");
    assert!(!page.contains(r#"href="/invites/revoke""#), "{page}");
    create_invite(&site, &cookie).await;
}

#[tokio::test]
async fn revoking_asks_first_and_names_how_many() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let response = site.get("/invites/revoke", Some(&cookie)).await;
    assert_eq!(location(&response), "/invites");

    create_invite(&site, &cookie).await;
    create_invite(&site, &cookie).await;
    let response = site.get("/invites/revoke", Some(&cookie)).await;

    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let page = body_text(response).await;
    assert!(page.contains("Revoke your 2 unused invites?"), "{page}");
    assert!(
        page.contains(r#"<form method="post" action="/invites/revoke">"#),
        "{page}"
    );
    assert_eq!(
        site.query_scalar::<i64>("SELECT COUNT(*) FROM invites WHERE revoked_at IS NULL")
            .await,
        2
    );
}

#[tokio::test]
async fn guests_and_other_origins_revoke_nothing() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    create_invite(&site, &cookie).await;
    let response = site.post("/invites/revoke", &[], None).await;
    assert_eq!(location(&response), "/signin");
    let evil = [(header::ORIGIN.as_str(), "https://evil.example")];
    let response = site
        .post_with("/invites/revoke", &[], Some(&cookie), &evil)
        .await;
    assert_eq!(response.status(), 403);
    assert_eq!(
        site.query_scalar::<i64>("SELECT COUNT(*) FROM invites WHERE revoked_at IS NULL")
            .await,
        1
    );
}

#[tokio::test]
async fn lists_the_people_a_member_invited() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.execute(
        "INSERT INTO members (account, invited_by, joined_at) VALUES ('bob', 'alice', 1);
         INSERT INTO members (account, invited_by, joined_at, removed_at) VALUES ('carol', 'alice', 2, 3);",
    )
    .await;
    let cookie = site.sign_in("alice").await;
    let page = invites_tab(&site, &cookie).await;
    assert!(page.contains("<li>bob</li>"), "{page}");
    assert!(
        page.contains(r#"<li>carol <span class="muted">(left)</span></li>"#),
        "{page}"
    );
}

#[tokio::test]
async fn guests_and_other_origins_create_nothing() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let response = site.post("/invites", &[], None).await;
    assert_eq!(location(&response), "/signin");
    let cookie = site.sign_in("alice").await;
    let evil = [(header::ORIGIN.as_str(), "https://evil.example")];
    let response = site.post_with("/invites", &[], Some(&cookie), &evil).await;
    assert_eq!(response.status(), 403);
    assert_eq!(invite_count(&site).await, 0);
}
