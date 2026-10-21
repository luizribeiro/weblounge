use crate::{Site, body_text, location};

/// Each tab's path and label, in the row under the header.
const TABS: [(&str, &str); 6] = [
    ("/", "Home"),
    ("/invites", "Invites"),
    ("/bots", "Bots"),
    ("/uploads", "Uploads"),
    ("/account", "Account"),
    ("/help", "Help"),
];

fn tab_links(page: &str) -> Vec<(String, String)> {
    let link = regex::Regex::new(r#"<li><a href="([^"]+)"[^>]*>([^<]+)</a></li>"#).unwrap();
    let tabs = &page[page.find(r#"<nav class="tabs""#).expect(page)..];
    let tabs = &tabs[..tabs.find("</nav>").unwrap()];
    link.captures_iter(tabs)
        .map(|c| (c[1].to_owned(), c[2].to_owned()))
        .collect()
}

/// The path of the one tab marked as the current page, if any.
fn open_tab(page: &str) -> Option<&str> {
    let marked = regex::Regex::new(r#"<a href="([^"]+)" aria-current="page">"#).unwrap();
    let mut open = marked
        .captures_iter(page)
        .map(|c| c.get(1).unwrap().as_str());
    let first = open.next();
    assert_eq!(open.next(), None, "{page}");
    first
}

#[tokio::test]
async fn every_tab_renders_for_members_with_itself_marked_open() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    let all: Vec<_> = TABS
        .iter()
        .map(|(path, label)| (path.to_string(), label.to_string()))
        .collect();
    for (path, _) in TABS {
        let response = site.get(path, Some(&cookie)).await;
        assert_eq!(response.status(), 200, "{path}");
        let page = body_text(response).await;
        assert_eq!(tab_links(&page), all, "{path}");
        assert_eq!(open_tab(&page), Some(path), "{path}");
    }
}

#[tokio::test]
async fn guests_are_sent_to_sign_in_from_every_tab() {
    let site = Site::start().await;
    for (path, _) in TABS {
        let response = site.get(path, None).await;
        assert_eq!(response.status(), 303, "{path}");
        assert!(location(&response).starts_with("/signin"), "{path}");
    }
}

#[tokio::test]
async fn other_pages_show_members_the_tabs_with_none_open() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for path in ["/rules", "/no-such-page"] {
        let page = body_text(site.get(path, Some(&cookie)).await).await;
        assert_eq!(tab_links(&page).len(), TABS.len(), "{page}");
        assert_eq!(open_tab(&page), None, "{page}");
    }
}

#[tokio::test]
async fn guests_get_no_tabs() {
    let page = body_text(Site::start().await.get("/rules", None).await).await;
    assert!(!page.contains(r#"class="tabs""#), "{page}");
}
