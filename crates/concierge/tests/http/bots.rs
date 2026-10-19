use axum::http::header;
use concierge::bot_rules::MAX_LIVE;

use crate::{Site, body_text, is_tab};

async fn bots_tab(site: &Site, cookie: &str) -> String {
    let response = site.get("/bots", Some(cookie)).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    body_text(response).await
}

/// The bots a page lists, as each item reads.
fn listed(page: &str) -> Vec<&str> {
    let Some(list) = page.split(r#"<ul class="bots">"#).nth(1) else {
        return Vec::new();
    };
    let list = &list[..list.find("</ul>").unwrap()];
    list.split("<li>")
        .skip(1)
        .map(|item| item.split("</li>").next().unwrap())
        .collect()
}

/// How the tab counts `n` bots out of the most a member may have.
fn count(n: i64) -> String {
    format!(r#"<span class="invite-count-number">{n} of {MAX_LIVE}</span> bots"#)
}

const NEEDS_A_CONFIRMED_EMAIL: &str =
    r#"<p class="invite-reason">You can't make bots until you confirm your email.</p>"#;

#[tokio::test]
async fn a_member_without_bots_sees_none_and_their_room_for_more() {
    let site = Site::start().await;
    site.add_verified_member("alice").await;
    site.add_member("luiz").await;
    site.execute(
        "INSERT INTO bots (account, owner, created_at) VALUES ('bender', 'luiz', 0);
         INSERT INTO bots (account, owner, created_at, removed_at) VALUES ('dalek', 'alice', 0, 1)",
    )
    .await;
    let cookie = site.sign_in("alice").await;

    let page = bots_tab(&site, &cookie).await;

    assert!(is_tab(&page, "/bots"), "{page}");
    assert!(page.contains("No bots yet."), "{page}");
    assert!(page.contains(&count(0)), "{page}");
    assert!(page.contains("You can have up to 3 at a time."), "{page}");
    assert!(page.contains("<code>weblounge/bot/alice</code>"), "{page}");
    assert!(listed(&page).is_empty(), "{page}");
    assert!(
        !page.contains("bender") && !page.contains("dalek"),
        "{page}"
    );
}

#[tokio::test]
async fn lists_the_members_live_bots_and_their_notes() {
    let site = Site::start().await;
    site.add_verified_member("luiz").await;
    site.add_member("alice").await;
    site.execute(
        "INSERT INTO bots (account, owner, note, created_at)
         VALUES ('bender', 'luiz', 'answers !ping', 0),
                ('Marvin', 'luiz', 'posts the <weather> & tides', 0),
                ('eliza', 'alice', 'talks back', 0);
         INSERT INTO bots (account, owner, created_at, removed_at) VALUES ('dalek', 'luiz', 0, 1)",
    )
    .await;
    let cookie = site.sign_in("luiz").await;

    let page = bots_tab(&site, &cookie).await;

    assert_eq!(
        listed(&page),
        [
            r#"<strong>bender</strong> <span class="muted">· answers !ping</span>"#,
            r#"<strong>Marvin</strong> <span class="muted">· posts the &#60;weather&#62; &#38; tides</span>"#,
        ]
    );
    assert!(page.contains(&count(2)), "{page}");
    assert!(!page.contains("No bots yet."), "{page}");
    assert!(!page.contains("dalek") && !page.contains("eliza"), "{page}");
}

#[tokio::test]
async fn says_when_a_member_has_as_many_bots_as_they_may() {
    let site = Site::start().await;
    site.add_verified_member("luiz").await;
    site.execute(
        "INSERT INTO bots (account, owner, created_at)
         VALUES ('bender', 'luiz', 0), ('hal', 'luiz', 0), ('marvin', 'luiz', 0)",
    )
    .await;
    let cookie = site.sign_in("luiz").await;

    let page = bots_tab(&site, &cookie).await;

    assert!(page.contains(&count(MAX_LIVE)), "{page}");
    assert!(
        page.contains("That's as many as you can have. Delete one to make another."),
        "{page}"
    );
}

#[tokio::test]
async fn an_unconfirmed_member_is_told_to_confirm_first_and_still_sees_their_bots() {
    let site = Site::start().await;
    site.add_member("luiz").await;
    site.execute("INSERT INTO bots (account, owner, created_at) VALUES ('bender', 'luiz', 0)")
        .await;
    let cookie = site.sign_in("luiz").await;

    let page = bots_tab(&site, &cookie).await;

    assert!(page.contains(NEEDS_A_CONFIRMED_EMAIL), "{page}");
    assert!(
        page.contains(r##"<a class="button button-primary" href="/account#email-and-news">Confirm your email</a>"##),
        "{page}"
    );
    assert!(
        page.contains(
            "We haven't sent you a link to confirm it yet; send one from the Account tab."
        ),
        "{page}"
    );
    assert!(!page.contains(" bots</p>"), "{page}");
    assert_eq!(listed(&page), ["<strong>bender</strong>"]);
    assert!(!page.contains(r#"class="notice email-reminder""#), "{page}");
}
