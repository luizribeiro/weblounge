use axum::http::header;
use concierge::{
    db::{self, NewSession},
    name::MemberName,
    nick,
    secret_hash::SessionHash,
    session::{self, LIFETIME},
};
use ergo_api::fake::Failure;

use crate::{PASSWORD, Site, body_text, location, session_cookie};

const WRONG: &str = "Wrong nick or password.";

#[tokio::test]
async fn the_form_lets_password_managers_fill_it_in() {
    let page = body_text(Site::start().await.get("/signin", None).await).await;
    assert!(
        page.contains(r#"<form class="form" method="post" action="/signin">"#),
        "{page}"
    );
    assert!(
        page.contains(&format!(
            r#"required maxlength="{}" autocomplete="username""#,
            nick::MAX_LENGTH
        )),
        "{page}"
    );
    assert!(
        page.contains(r#"type="password" required autocomplete="current-password""#),
        "{page}"
    );
}

#[tokio::test]
async fn signs_a_member_in_under_ergos_spelling_of_their_name() {
    let site = Site::start().await;
    site.add_member("Alice").await;

    let response = site.sign_in_form("ALICE", PASSWORD).await;

    assert_eq!(response.status(), 303);
    assert_eq!(location(&response), "/");
    let set_cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(
        set_cookie.ends_with("; Path=/; Max-Age=2592000; HttpOnly; Secure; SameSite=Lax"),
        "{set_cookie}"
    );
    let cookie = session_cookie(&response).unwrap();
    let hash = site.session_hash(&cookie).await;
    let session = site.db.session(&hash, db::now()).await.unwrap().unwrap();
    assert_eq!(session.member.account.as_str(), "Alice");
}

#[tokio::test]
async fn brings_the_member_back_to_the_page_they_asked_for_on_this_site_only() {
    let site = Site::start().await;
    site.add_member("alice").await;
    for (next, destination) in [
        ("/rules", "/rules"),
        ("/?saved", "/?saved"),
        ("//evil.example/", "/"),
        ("https://evil.example/", "/"),
        ("/\\evil.example", "/"),
    ] {
        let form = [("nick", "alice"), ("password", PASSWORD), ("next", next)];
        let response = site.post("/signin", &form, None).await;
        assert_eq!(location(&response), destination, "{next}");
    }
}

#[tokio::test]
async fn the_form_carries_a_local_next_and_drops_any_other() {
    let site = Site::start().await;
    let page = body_text(site.get("/signin?next=%2Frules", None).await).await;
    assert!(
        page.contains(r#"<input type="hidden" name="next" value="/rules">"#),
        "{page}"
    );
    let page = body_text(site.get("/signin?next=%2F%2Fevil.example", None).await).await;
    assert!(!page.contains(r#"name="next""#), "{page}");
}

#[tokio::test]
async fn a_signed_in_member_skips_the_form() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let response = site.get("/signin?next=%2Frules", Some(&cookie)).await;
    assert_eq!(response.status(), 303);
    assert_eq!(location(&response), "/rules");
}

/// The page shown for a refused attempt at `nick`, with the nick taken out
/// so pages for different nicks compare equal.
async fn refusal(site: &Site, nick: &str, password: &str) -> (u16, String) {
    let response = site.sign_in_form(nick, password).await;
    assert!(session_cookie(&response).is_none(), "{nick}");
    let status = response.status().as_u16();
    let page = body_text(response).await;
    (
        status,
        page.replace(&format!(r#"value="{nick}""#), r#"value="""#),
    )
}

#[tokio::test]
async fn every_refusal_reads_the_same_whoever_the_nick_is() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.add_member("dave").await;
    site.db
        .remove_member(&MemberName::new("dave"), db::now())
        .await
        .unwrap();
    site.register("carol").await;

    let wrong_password = refusal(&site, "alice", "not-her-password").await;
    assert_eq!(wrong_password.0, 401);
    assert!(wrong_password.1.contains(WRONG), "{}", wrong_password.1);
    for (nick, password) in [
        ("nobody", PASSWORD),
        ("carol", PASSWORD),
        ("dave", PASSWORD),
        ("alice", ""),
    ] {
        assert_eq!(
            refusal(&site, nick, password).await,
            wrong_password,
            "{nick}"
        );
    }
    let (status, page) = refusal(&site, "", PASSWORD).await;
    assert_eq!(status, 401);
    assert!(page.contains(WRONG), "{page}");
}

#[tokio::test]
async fn a_removed_member_cannot_sign_in_and_their_sessions_stop_working() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    site.db
        .remove_member(&MemberName::new("alice"), db::now())
        .await
        .unwrap();
    assert_eq!(site.get("/", Some(&cookie)).await.status(), 303);
    assert_eq!(site.sign_in_form("alice", PASSWORD).await.status(), 401);
}

#[tokio::test]
async fn says_so_when_ergo_cannot_check_the_password() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.ergo.fail_next(Failure::UnknownError);
    let response = site.sign_in_form("alice", PASSWORD).await;
    assert_eq!(response.status(), 503);
    assert!(
        body_text(response)
            .await
            .contains("Can&#39;t check passwords right now.")
    );
}

#[tokio::test]
async fn signing_out_ends_the_session_and_forgets_the_cookie() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let other_device = site.sign_in("alice").await;

    let response = site.post("/signout", &[], Some(&cookie)).await;

    assert_eq!(response.status(), 303);
    assert_eq!(location(&response), "/signin");
    assert_eq!(
        response.headers()[header::SET_COOKIE],
        "__Host-session=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax"
    );
    assert_eq!(site.session_expiry(&cookie).await, None);
    assert_eq!(site.get("/", Some(&cookie)).await.status(), 303);
    assert_eq!(site.get("/", Some(&other_device)).await.status(), 200);
}

#[tokio::test]
async fn signing_in_again_replaces_the_browsers_session() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let old = site.sign_in("alice").await;
    let form = [("nick", "alice"), ("password", PASSWORD)];
    let response = site.post("/signin", &form, Some(&old)).await;
    let new = session_cookie(&response).unwrap();
    assert_ne!(old, new);
    assert_eq!(site.session_expiry(&old).await, None);
    assert!(site.session_expiry(&new).await.is_some());
}

#[tokio::test]
async fn an_expired_session_signs_nobody_in_and_is_cleaned_up_later() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.add_member("bob").await;
    let cookie = site.sign_in("alice").await;
    let hash = site.session_hash(&cookie).await;
    site.db.renew_session(&hash, db::now() - 1).await.unwrap();
    site.db
        .insert_session(&NewSession {
            token_hash: &SessionHash::from_bytes(b"bob's old laptop".to_vec()),
            account: &MemberName::new("bob"),
            created_at: 0,
            expires_at: 1,
        })
        .await
        .unwrap();

    let response = site.get("/", Some(&cookie)).await;
    assert_eq!(location(&response), "/signin");

    site.sign_in("bob").await;
    assert_eq!(
        site.db
            .end_sessions(&MemberName::new("alice"))
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        site.db.end_sessions(&MemberName::new("bob")).await.unwrap(),
        1
    );
}

#[tokio::test]
async fn a_session_in_use_is_renewed_at_most_daily() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;

    let fresh = site.get("/", Some(&cookie)).await;
    assert!(fresh.headers().get(header::SET_COOKIE).is_none());

    let hash = site.session_hash(&cookie).await;
    let two_days_old = session::expires_at(db::now()) - 2 * 24 * 60 * 60;
    site.db.renew_session(&hash, two_days_old).await.unwrap();
    let renewed = site.get("/rules", Some(&cookie)).await;
    assert_eq!(session_cookie(&renewed).as_deref(), Some(cookie.as_str()));
    let expiry = site.session_expiry(&cookie).await.unwrap();
    assert!(expiry >= db::now() + LIFETIME - 5, "{expiry}");
}

#[tokio::test]
async fn a_password_reset_ends_the_sessions() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    site.db
        .end_sessions(&MemberName::new("alice"))
        .await
        .unwrap();
    assert_eq!(location(&site.get("/", Some(&cookie)).await), "/signin");
}
