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
            topic("/clients", "Use your own app"),
            topic("/help/several-devices", "Use weblounge on several devices"),
            topic("/help/start-a-channel", "Start a channel and keep it"),
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
        if href.starts_with("/help/") {
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
            topic("/clients", "Use your own app"),
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
