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

/// The topics the Help index links to, as (href, title).
fn topics(page: &str) -> Vec<(String, String)> {
    let link = Regex::new(r#"<li><a href="([^"]+)">([^<]+)</a></li>"#).unwrap();
    let list = &page[page.find(r#"<ul class="help-topics">"#).expect(page)..];
    let list = &list[..list.find("</ul>").unwrap()];
    link.captures_iter(list)
        .map(|c| (c[1].to_owned(), c[2].to_owned()))
        .collect()
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
        topics(&page)[0],
        ("/clients".to_owned(), "Use your own app".to_owned())
    );
    assert!(page.contains(STILL_STUCK), "{page}");
}
