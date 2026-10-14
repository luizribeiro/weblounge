use axum::http::header;
use concierge::invite_rules::DEFAULT_QUOTA;
use regex::Regex;

use crate::{PUBLIC_URL, Site, body_text, location};

fn invite_links(page: &str) -> Vec<String> {
    let link = Regex::new(r#"<code class="invite-link">(https://[^<]+)</code>"#).unwrap();
    link.captures_iter(page).map(|c| c[1].to_owned()).collect()
}

async fn invites_tab(site: &Site, cookie: &str) -> String {
    body_text(site.get("/invites", Some(cookie)).await).await
}

#[tokio::test]
async fn a_member_creates_an_invite_and_gets_a_copyable_link() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;

    let response = site.post("/invites", &[], Some(&cookie)).await;

    assert_eq!(response.status(), 303);
    assert_eq!(location(&response), "/invites?invited");
    let response = site.get("/invites?invited", Some(&cookie)).await;
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let page = body_text(response).await;
    assert!(page.contains("Invite created."), "{page}");
    let [link] = invite_links(&page).try_into().unwrap();
    assert!(link.starts_with(&format!("{PUBLIC_URL}/invite/")), "{link}");
    assert!(page.contains(&format!(r#"data-copy="{link}""#)), "{page}");
    assert!(page.contains("7 days left"), "{page}");
    assert!(
        page.contains(&format!("{} left</span>", DEFAULT_QUOTA - 1)),
        "{page}"
    );

    let path = link.strip_prefix(PUBLIC_URL).unwrap();
    let invite = body_text(site.get(path, None).await).await;
    assert!(invite.contains("alice saved you a seat."), "{invite}");
}

#[tokio::test]
async fn the_quota_runs_out_and_says_so() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for _ in 0..DEFAULT_QUOTA + 2 {
        site.post("/invites", &[], Some(&cookie)).await;
    }
    let page = invites_tab(&site, &cookie).await;
    assert_eq!(invite_links(&page).len(), DEFAULT_QUOTA as usize);
    assert!(page.contains("No invites left."), "{page}");
    assert!(!page.contains(r#"action="/invites""#), "{page}");
}

#[tokio::test]
async fn two_invites_at_once_cannot_both_take_the_last_place() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.execute("UPDATE members SET invite_quota = 1 WHERE account = 'alice'")
        .await;
    let cookie = site.sign_in("alice").await;

    tokio::join!(
        site.post("/invites", &[], Some(&cookie)),
        site.post("/invites", &[], Some(&cookie))
    );

    assert_eq!(invite_links(&invites_tab(&site, &cookie).await).len(), 1);
}

#[tokio::test]
async fn admins_invite_beyond_the_quota() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.execute("UPDATE members SET is_admin = TRUE WHERE account = 'alice'")
        .await;
    let cookie = site.sign_in("alice").await;
    for _ in 0..DEFAULT_QUOTA + 2 {
        site.post("/invites", &[], Some(&cookie)).await;
    }
    let page = invites_tab(&site, &cookie).await;
    assert_eq!(invite_links(&page).len(), DEFAULT_QUOTA as usize + 2);
    assert!(page.contains("Admins have no quota."), "{page}");
}

#[tokio::test]
async fn revoking_kills_the_link_and_gives_the_invite_back() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    site.post("/invites", &[], Some(&cookie)).await;
    let [link] = invite_links(&invites_tab(&site, &cookie).await)
        .try_into()
        .unwrap();
    let code = link.rsplit('/').next().unwrap();

    let response = site
        .post("/invites/revoke", &[("code", code)], Some(&cookie))
        .await;

    assert_eq!(location(&response), "/invites");
    let page = invites_tab(&site, &cookie).await;
    assert!(invite_links(&page).is_empty(), "{page}");
    assert!(
        page.contains(&format!("{DEFAULT_QUOTA} left</span>")),
        "{page}"
    );
    let path = link.strip_prefix(PUBLIC_URL).unwrap();
    assert_eq!(site.get(path, None).await.status(), 404);
}

#[tokio::test]
async fn members_cannot_revoke_each_others_invites() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.add_member("mallory").await;
    site.add_invite("alices-invite", Some("alice")).await;
    let cookie = site.sign_in("mallory").await;
    site.post(
        "/invites/revoke",
        &[("code", "alices-invite")],
        Some(&cookie),
    )
    .await;
    assert_eq!(site.get("/invite/alices-invite", None).await.status(), 200);
}

#[tokio::test]
async fn lists_the_people_a_member_invited() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.execute(
        "INSERT INTO members (account, invited_by, joined_at, invite_quota) VALUES ('bob', 'alice', 1, 5);
         INSERT INTO members (account, invited_by, joined_at, invite_quota, removed_at) VALUES ('carol', 'alice', 2, 0, 3);",
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
    for path in ["/invites", "/invites/revoke"] {
        let response = site.post(path, &[("code", "x")], None).await;
        assert_eq!(location(&response), "/signin", "{path}");
    }
    let cookie = site.sign_in("alice").await;
    let evil = [(header::ORIGIN.as_str(), "https://evil.example")];
    let response = site.post_with("/invites", &[], Some(&cookie), &evil).await;
    assert_eq!(response.status(), 403);
    assert!(invite_links(&invites_tab(&site, &cookie).await).is_empty());
}

fn qr_paths(page: &str) -> Vec<String> {
    let path = Regex::new(r#"href="(/invites/[A-Za-z0-9_-]{16}/qr)""#).unwrap();
    path.captures_iter(page).map(|c| c[1].to_owned()).collect()
}

async fn alice_with_an_invite() -> (Site, String, String, String) {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    site.post("/invites", &[], Some(&cookie)).await;
    let page = invites_tab(&site, &cookie).await;
    let [link] = invite_links(&page).try_into().unwrap();
    let [qr] = qr_paths(&page).try_into().unwrap();
    (site, cookie, link, qr)
}

#[tokio::test]
async fn an_invites_qr_code_encodes_its_link_on_a_page_named_without_the_code() {
    let (site, cookie, link, qr) = alice_with_an_invite().await;
    let code = link.rsplit('/').next().unwrap();
    assert!(!qr.contains(code), "{qr}");

    let response = site.get(&qr, Some(&cookie)).await;

    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let page = body_text(response).await;
    assert!(page.contains(&concierge::qr::svg(&link)), "{page}");
    assert!(
        page.contains(&format!(r#"<code class="invite-link">{link}</code>"#)),
        "{page}"
    );
}

#[tokio::test]
async fn only_the_inviter_sees_the_qr_code() {
    let (site, _, _, qr) = alice_with_an_invite().await;
    site.add_member("mallory").await;
    let mallorys = site.sign_in("mallory").await;
    let response = site.get(&qr, Some(&mallorys)).await;
    assert_eq!(response.status(), 404);
    assert!(body_text(response).await.contains("No such invite."));
    let response = site.get(&qr, None).await;
    assert_eq!(
        location(&response),
        format!("/signin?next={}", qr.replace('/', "%2F"))
    );
    let response = site
        .get("/invites/AAAAAAAAAAAAAAAA/qr", Some(&mallorys))
        .await;
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn revoked_and_redeemed_invites_have_no_qr_code() {
    let (site, cookie, link, qr) = alice_with_an_invite().await;
    let code = link.rsplit('/').next().unwrap();
    site.post("/invites/revoke", &[("code", code)], Some(&cookie))
        .await;
    assert_eq!(site.get(&qr, Some(&cookie)).await.status(), 404);

    site.post("/invites", &[], Some(&cookie)).await;
    let page = invites_tab(&site, &cookie).await;
    let [link] = invite_links(&page).try_into().unwrap();
    let [qr] = qr_paths(&page).try_into().unwrap();
    let code = link.rsplit('/').next().unwrap();
    site.execute(&format!(
        "INSERT INTO members (account, invited_by, joined_at, invite_quota) VALUES ('bob', 'alice', 1, 5);
         UPDATE invites SET redeemed_by = 'bob', redeemed_at = 1 WHERE code = '{code}';"
    ))
    .await;
    assert_eq!(site.get(&qr, Some(&cookie)).await.status(), 404);
}

#[tokio::test]
async fn an_expired_invite_has_no_qr_code() {
    let (site, cookie, _, qr) = alice_with_an_invite().await;
    site.execute("UPDATE invites SET created_at = 0, expires_at = 1")
        .await;
    assert_eq!(site.get(&qr, Some(&cookie)).await.status(), 404);
}
