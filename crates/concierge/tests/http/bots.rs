use axum::{http::header, response::Response};
use concierge::{
    bot_rules::MAX_LIVE,
    rate_limit::{BOT_CHANGES_PER_ADDRESS, TRY_LATER},
};
use ergo_api::fake::Failure;
use regex::Regex;

use crate::{PUBLIC_URL, Site, body_text, is_tab};

/// Posts a form from our own pages, from the client address `address`.
async fn post_from(
    site: &Site,
    path: &str,
    form: &[(&str, &str)],
    cookie: &str,
    address: &str,
) -> Response {
    let headers = [
        (header::ORIGIN.as_str(), PUBLIC_URL),
        ("x-real-ip", address),
    ];
    site.post_with(path, form, Some(cookie), &headers).await
}

/// Makes the bot `name` as the member signed in with `cookie`, answering
/// the status and page.
async fn make(site: &Site, cookie: &str, name: &str) -> (u16, String) {
    let response = site.post("/bots", &[("name", name)], Some(cookie)).await;
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    (response.status().as_u16(), body_text(response).await)
}

/// The generated passwords a page shows.
fn passwords(page: &str) -> Vec<String> {
    let password = Regex::new(r#"<code>([a-z2-9]{26})</code>"#).unwrap();
    password
        .captures_iter(page)
        .map(|c| c[1].to_owned())
        .collect()
}

/// The password the dialog on `page` shows, once.
fn shown_password(page: &str) -> String {
    let [password] = passwords(page).try_into().unwrap();
    assert!(
        page.contains(&format!(r#"data-copy="{password}""#)),
        "{page}"
    );
    password
}

async fn signs_in_to_ergo(site: &Site, account: &str, password: &str) -> bool {
    let client = site.ergo.client();
    client
        .check_auth(account, password)
        .await
        .unwrap()
        .is_some()
}

async fn bot_count(site: &Site) -> i64 {
    site.query_scalar("SELECT COUNT(*) FROM bots").await
}

/// The name field as a refused form gives it back.
fn name_field(value: &str) -> String {
    format!(r#"name="name" value="{value}" required"#)
}

fn name_error(error: &str) -> String {
    let rule = concierge::nick::RULE;
    format!(
        r#"<p class="field-error" id="bot-name-hint" data-nick-status data-hint="Its nick on IRC, for good. {rule}">{error}</p>"#
    )
}

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

#[tokio::test]
async fn making_a_bot_shows_its_password_once_in_a_dialog() {
    let site = Site::start().await;
    site.add_verified_member("alice").await;
    let cookie = site.sign_in("alice").await;

    let (status, page) = make(&site, &cookie, "Bender").await;

    assert_eq!(status, 200, "{page}");
    let password = shown_password(&page);
    assert!(
        page.contains(r#"<dialog class="card" id="bot-password" open data-show-modal"#),
        "{page}"
    );
    assert!(
        page.contains("Copy it now. It won't be shown again; if you lose it, make a new one."),
        "{page}"
    );
    assert!(page.contains(r#"data-copy="Bender""#), "{page}");
    assert!(signs_in_to_ergo(&site, "bender", &password).await);
    assert_eq!(listed(&page), ["<strong>Bender</strong>"]);
    let bot = site.db.bot("bender").await.unwrap().unwrap();
    assert_eq!(
        (bot.account.as_str(), bot.owner.as_str()),
        ("Bender", "alice")
    );
    assert_eq!(
        site.ergo.vhost("bender").as_deref(),
        Some("weblounge/bot/alice")
    );
    let settings = site.ergo.client().settings("bender").await.unwrap();
    assert_eq!(settings.always_on, ergo_api::Preference::Off);

    let later = bots_tab(&site, &cookie).await;
    assert!(passwords(&later).is_empty(), "{later}");
    assert!(
        !later.contains(&password) && !later.contains("<dialog"),
        "{later}"
    );
    assert_eq!(listed(&later), ["<strong>Bender</strong>"]);
}

#[tokio::test]
async fn the_tab_offers_the_form_to_make_one_with_the_servers_name_rules() {
    let site = Site::start().await;
    site.add_verified_member("alice").await;
    let cookie = site.sign_in("alice").await;

    let page = bots_tab(&site, &cookie).await;

    let pattern = concierge::nick::html_pattern();
    assert!(
        page.contains(&format!(r#"maxlength="32" pattern="{pattern}""#)),
        "{page}"
    );
    assert!(page.contains(&name_field("")), "{page}");
    assert!(
        page.contains(r#"data-availability="/signup/nick""#),
        "{page}"
    );
    assert!(
        page.contains(r#"<button class="button button-primary" type="submit">Make a bot</button>"#),
        "{page}"
    );
}

#[tokio::test]
async fn a_name_the_rules_refuse_is_given_back_with_why() {
    let site = Site::start().await;
    site.add_verified_member("alice").await;
    let cookie = site.sign_in("alice").await;

    for (name, error) in [
        ("2pac", "Nicks start with a letter."),
        (
            "NickServ",
            "That nick is reserved for the network&#39;s services.",
        ),
        (
            "bob!",
            &format!(
                "Nicks can&#39;t contain &#39;!&#39;. {}",
                concierge::nick::RULE
            ),
        ),
        ("", "Choose a nick."),
    ] {
        let (status, page) = make(&site, &cookie, name).await;
        assert_eq!(status, 422, "{name}");
        assert!(page.contains(&name_field(name)), "{name}: {page}");
        assert!(page.contains(&name_error(error)), "{name}: {page}");
        assert!(page.contains(r#"aria-invalid="true""#), "{name}: {page}");
        assert!(passwords(&page).is_empty(), "{page}");
    }
    assert_eq!(bot_count(&site).await, 0);
}

#[tokio::test]
async fn a_name_a_member_or_bot_holds_is_taken_even_once_ergo_forgot_it() {
    let site = Site::start().await;
    site.add_verified_member("alice").await;
    site.add_member("luiz").await;
    site.register("orphan").await;
    site.execute(
        "INSERT INTO members (account, joined_at, removed_at) VALUES ('gone', 0, 1);
         INSERT INTO bots (account, owner, created_at, removed_at) VALUES ('dalek', 'luiz', 0, 1)",
    )
    .await;
    let cookie = site.sign_in("alice").await;

    for name in ["LUIZ", "Gone", "dalek", "Orphan"] {
        let (status, page) = make(&site, &cookie, name).await;
        assert_eq!(status, 422, "{name}");
        assert!(page.contains(&name_field(name)), "{name}: {page}");
        assert!(
            page.contains(&name_error(&format!("{name} is taken."))),
            "{name}: {page}"
        );
    }
    assert_eq!(bot_count(&site).await, 1);
}

#[tokio::test]
async fn a_member_makes_three_and_no_more() {
    let site = Site::start().await;
    site.add_verified_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for name in ["bender", "hal", "marvin"] {
        assert_eq!(make(&site, &cookie, name).await.0, 200, "{name}");
    }

    let (status, page) = make(&site, &cookie, "eliza").await;

    assert_eq!(status, 409, "{page}");
    assert!(
        page.contains("That&#39;s as many bots as you can have. Delete one to make another."),
        "{page}"
    );
    assert!(page.contains(&count(MAX_LIVE)), "{page}");
    assert!(!page.contains("Make a bot"), "{page}");
    assert_eq!(site.db.bot("eliza").await.unwrap(), None);
    assert!(site.ergo.client().info("eliza").await.unwrap().is_none());
}

#[tokio::test]
async fn bots_an_admin_made_count_and_deleted_ones_dont() {
    let site = Site::start().await;
    site.add_verified_member("alice").await;
    site.execute(
        "INSERT INTO bots (account, owner, created_at)
         VALUES ('bender', 'alice', 0), ('hal', 'alice', 0), ('marvin', 'alice', 0);
         INSERT INTO bots (account, owner, created_at, removed_at) VALUES ('dalek', 'alice', 0, 1)",
    )
    .await;
    let cookie = site.sign_in("alice").await;
    assert_eq!(make(&site, &cookie, "eliza").await.0, 409);

    site.execute("UPDATE bots SET removed_at = 1 WHERE account = 'hal'")
        .await;

    assert_eq!(make(&site, &cookie, "eliza").await.0, 200);
}

#[tokio::test]
async fn an_unconfirmed_member_makes_none() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    assert!(!bots_tab(&site, &cookie).await.contains("Make a bot"));

    let (status, page) = make(&site, &cookie, "bender").await;

    assert_eq!(status, 403, "{page}");
    assert!(page.contains(NEEDS_A_CONFIRMED_EMAIL), "{page}");
    assert!(!page.contains("Make a bot"), "{page}");
    assert_eq!(bot_count(&site).await, 0);
    assert!(site.ergo.client().info("bender").await.unwrap().is_none());
}

#[tokio::test]
async fn guests_and_other_origins_make_nothing() {
    let site = Site::start().await;
    site.add_verified_member("alice").await;
    let cookie = site.sign_in("alice").await;

    let guest = site.post("/bots", &[("name", "bender")], None).await;
    assert_eq!(guest.status(), 303);
    let headers = [(header::ORIGIN.as_str(), "https://evil.example")];
    let forged = site
        .post_with("/bots", &[("name", "bender")], Some(&cookie), &headers)
        .await;
    assert_eq!(forged.status(), 403);
    assert_eq!(body_text(forged).await, "Cross-origin request refused.\n");

    assert_eq!(bot_count(&site).await, 0);
}

#[tokio::test]
async fn an_address_gets_a_limited_number_of_bot_changes() {
    let site = Site::start().await;
    site.add_verified_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for _ in 0..BOT_CHANGES_PER_ADDRESS {
        let response = post_from(&site, "/bots", &[("name", "2pac")], &cookie, "192.0.2.7").await;
        assert_eq!(response.status(), 422);
    }

    let response = post_from(&site, "/bots", &[("name", "bender")], &cookie, "192.0.2.7").await;

    assert_eq!(response.status(), 429);
    let page = body_text(response).await;
    assert!(page.contains(TRY_LATER), "{page}");
    assert!(page.contains(&name_field("bender")), "{page}");
    assert_eq!(bot_count(&site).await, 0);
    let elsewhere = post_from(&site, "/bots", &[("name", "bender")], &cookie, "192.0.2.8").await;
    assert_eq!(elsewhere.status(), 200);
}

#[tokio::test]
async fn nothing_is_made_when_ergo_fails() {
    let site = Site::start().await;
    site.add_verified_member("alice").await;
    let cookie = site.sign_in("alice").await;
    site.ergo.fail_next(Failure::UnknownError);

    let (status, page) = make(&site, &cookie, "bender").await;

    assert_eq!(status, 503, "{page}");
    assert!(
        page.contains("Can&#39;t reach the chat server. Nothing changed; try again in a minute."),
        "{page}"
    );
    assert!(page.contains(&name_field("bender")), "{page}");
    assert_eq!(bot_count(&site).await, 0);
}

#[tokio::test]
async fn says_so_when_ergo_makes_the_bot_but_doesnt_set_it_up() {
    let site = Site::start().await;
    site.add_verified_member("alice").await;
    let cookie = site.sign_in("alice").await;
    site.ergo.pass_next();
    site.ergo.fail_next(Failure::UnknownError);

    let (status, page) = make(&site, &cookie, "bender").await;

    assert_eq!(status, 503, "{page}");
    assert!(
        page.contains(
            "Made bender, but the chat server didn&#39;t finish setting it up. \
             Make it a new password in a minute to try again."
        ),
        "{page}"
    );
    assert!(passwords(&page).is_empty(), "{page}");
    assert_eq!(listed(&page), ["<strong>bender</strong>"]);
}
