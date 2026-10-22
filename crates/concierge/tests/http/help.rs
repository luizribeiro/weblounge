use concierge::{bot_rules::MAX_LIVE, invite_rules, upload_rules};
use regex::Regex;

use crate::{Site, body_text, is_tab};

const STILL_STUCK: &str = r#"<p class="help-stuck">Still stuck? Ask in <a href="https://irc.weblounge.org/">#lounge</a>.</p>"#;

impl Site {
    /// The Help page at `path`, as alice reads it.
    async fn help_page(&self, path: &str) -> String {
        self.add_member("alice").await;
        let cookie = self.sign_in("alice").await;
        let response = self.get(path, Some(&cookie)).await;
        assert_eq!(response.status(), 200, "{path}");
        body_text(response).await
    }
}

/// The links in the list of `class` on `page`, as (href, title).
fn links(page: &str, class: &str) -> Vec<(String, String)> {
    let link = Regex::new(r#"<li><a href="([^"]+)">([^<]+)</a></li>"#).unwrap();
    let list = &page[page.find(&format!(r#"<ul class="{class}">"#)).expect(page)..];
    let list = &list[..list.find("</ul>").unwrap()];
    link.captures_iter(list)
        .map(|c| (c[1].to_owned(), c[2].to_owned()))
        .collect()
}

fn topics(page: &str) -> Vec<(String, String)> {
    links(page, "help-topics")
}

fn topic(href: &str, title: &str) -> (String, String) {
    (href.to_owned(), title.to_owned())
}

#[tokio::test]
async fn the_help_index_lists_its_topics_and_where_to_ask_next() {
    let page = Site::start().await.help_page("/help").await;

    assert!(is_tab(&page, "/help"), "{page}");
    assert!(
        page.contains("<title>Help · weblounge.org</title>"),
        "{page}"
    );
    assert!(page.contains("<h1>How can we help?</h1>"), "{page}");
    assert_eq!(
        topics(&page),
        [
            topic("/help/apps", "Use your own app"),
            topic("/help/several-devices", "Use weblounge on several devices"),
            topic("/help/start-a-channel", "Start a channel and keep it"),
            topic("/help/make-a-bot", "Make a bot"),
            topic("/help/share-an-image", "Share an image"),
        ]
    );
    assert!(page.contains(STILL_STUCK), "{page}");
}

#[tokio::test]
async fn every_help_topic_and_related_link_leads_to_a_page() {
    let site = Site::start().await;
    let index = site.help_page("/help").await;
    let cookie = site.sign_in("alice").await;
    for (href, title) in topics(&index) {
        let response = site.get(&href, Some(&cookie)).await;
        assert_eq!(response.status(), 200, "{href}");
        if href != concierge::help::OWN_APP.href {
            let article = body_text(response).await;
            assert!(article.contains(&format!("<h1>{title}</h1>")), "{article}");
            for (related, _) in links(&article, "help-related") {
                assert_ne!(related, href, "{article}");
                let response = site.get(&related, Some(&cookie)).await;
                assert_eq!(response.status(), 200, "{related}");
            }
        }
    }
}

#[tokio::test]
async fn an_article_leads_back_to_help_and_ends_with_where_to_ask_next() {
    let page = Site::start().await.help_page("/help/several-devices").await;

    assert!(is_tab(&page, "/help"), "{page}");
    assert!(
        page.contains("<title>Use weblounge on several devices · weblounge.org</title>"),
        "{page}"
    );
    assert!(
        page.contains(
            r#"<nav class="help-crumbs" aria-label="Breadcrumb"><a href="/help">Help</a></nav>"#
        ),
        "{page}"
    );
    assert!(page.contains(STILL_STUCK), "{page}");
}

#[tokio::test]
async fn several_devices_says_where_to_turn_always_on_off_and_how_long_history_lasts() {
    let page = Site::start_with(|app| app.history_expiry = 3 * 24 * 60 * 60)
        .await
        .help_page("/help/several-devices")
        .await;

    assert!(
        page.contains(r#"<li>Go to the <a href="/account#irc-settings">Account</a> tab.</li>"#),
        "{page}"
    );
    assert!(
        page.contains(
            "<li>Under <strong>IRC while you&#39;re away</strong>, untick \
             <strong>Always on</strong> or <strong>Auto-away</strong>.</li>"
        ),
        "{page}"
    );
    assert!(
        page.contains("<li>Messages are deleted from the server after 3 days.</li>"),
        "{page}"
    );
    assert_eq!(
        links(&page, "help-related"),
        [
            topic("/help/apps", "Use your own app"),
            topic("/help/start-a-channel", "Start a channel and keep it"),
        ]
    );
}

#[tokio::test]
async fn start_a_channel_gives_the_commands_to_start_register_and_close_one() {
    let page = Site::start().await.help_page("/help/start-a-channel").await;

    for line in [
        "<li>Run <code>/join #name</code>. If nobody&#39;s in it, you start it and become its op (@).</li>",
        "<li>While you&#39;re its op, run <code>/msg ChanServ REGISTER #name</code>. \
         That makes you its founder (~).</li>",
        "<li>Run <code>/mode #name +i</code>, so people can join only when invited.</li>",
        "<li>Invite someone with <code>/invite nick #name</code>.</li>",
    ] {
        assert!(page.contains(line), "{line}\n{page}");
    }
    assert!(page.contains("Only ops can change the topic."), "{page}");
}

#[tokio::test]
async fn make_a_bot_shows_the_members_own_bot_host_and_how_many_bots_they_may_have() {
    let page = Site::start().await.help_page("/help/make-a-bot").await;

    for line in [
        r#"<li>Make its account on the <a href="/bots">Bots</a> tab, and copy its password."#,
        "<li>Start from our template: <code>nix flake init -t github:luizribeiro/weblounge#bot</code>.</li>",
        r#"The <a href="https://github.com/luizribeiro/weblounge/tree/main/templates/bot">template&#39;s guide</a>"#,
        "<li>Your bot connects as <code>weblounge/bot/alice</code>, so everyone can tell it&#39;s yours.</li>",
        "<li>It can&#39;t join #lounge. Use #bots, or a channel of your own.</li>",
        &format!(
            "<li>You can have up to {MAX_LIVE} bots once you&#39;ve confirmed your email.</li>"
        ),
    ] {
        assert!(page.contains(line), "{line}\n{page}");
    }
}

#[tokio::test]
async fn share_an_image_gives_the_upload_limits_from_the_rules() {
    let page = Site::start().await.help_page("/help/share-an-image").await;
    let mb = upload_rules::in_megabytes;

    for line in [
        r#"<li>Open a channel or a chat in <a href="https://irc.weblounge.org/">the web client</a>, or in your IRC app if it can upload.</li>"#.to_owned(),
        format!(
            "<li>Images can be {}, up to {} each and {} an hour.</li>",
            upload_rules::types_in_words(),
            mb(upload_rules::MAX_BYTES),
            upload_rules::PER_HOUR
        ),
        format!(
            "<li>You have {} in all, and your bots&#39; uploads count toward it.</li>",
            mb(upload_rules::QUOTA_BYTES)
        ),
        format!(
            r#"<li>Each image is deleted after {}. Your <a href="/uploads">Uploads</a> tab"#,
            invite_rules::in_words(upload_rules::RETENTION)
        ),
        "Anyone with an image&#39;s link can see it.".to_owned(),
    ] {
        assert!(page.contains(&line), "{line}\n{page}");
    }
    assert!(
        page.contains("Images can be PNG, JPEG, GIF or WebP, up to 10 MB each and 30 an hour."),
        "{page}"
    );
}

#[tokio::test]
async fn unknown_articles_are_not_found() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for path in ["/help/no-such-article", "/help/clients", "/help/"] {
        let response = site.get(path, Some(&cookie)).await;
        assert_eq!(response.status(), 404, "{path}");
    }
}

#[tokio::test]
async fn guests_are_sent_to_sign_in_from_articles() {
    let response = Site::start().await.get("/help/several-devices", None).await;
    assert_eq!(response.status(), 303);
    assert!(crate::location(&response).starts_with("/signin"));
}
