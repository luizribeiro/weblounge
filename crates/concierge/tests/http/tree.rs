use crate::{Site, body_text};

/// alice (root) invited bob and carol; bob invited dave; carol left.
async fn site_with_a_tree() -> Site {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.add_member("bob").await;
    site.execute(
        "UPDATE members SET invited_by = 'alice', joined_at = 1760100000 WHERE account = 'bob';
         INSERT INTO members (account, invited_by, joined_at, removed_at)
         VALUES ('carol', 'alice', 1760200000, 1760300000);
         INSERT INTO members (account, email, invited_by, joined_at)
         VALUES ('dave', 'dave@example.org', 'bob', 1760400000);",
    )
    .await;
    site
}

/// The page's names, `(` and `)` where lists open and close.
fn outline(page: &str) -> String {
    let tree = &page[page.find(r#"<ul class="tree">"#).unwrap()..];
    let token = regex::Regex::new(r"<ul[^>]*>|</ul>|<(?:span|strong)[^>]*>([a-z]+)</").unwrap();
    token
        .captures_iter(tree)
        .map(|c| match &c[0] {
            "</ul>" => ")".to_owned(),
            open if open.starts_with("<ul") => "(".to_owned(),
            _ => c[1].to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[tokio::test]
async fn shows_who_invited_whom_from_the_root_down() {
    let site = site_with_a_tree().await;
    let cookie = site.sign_in("bob").await;

    let response = site.get("/invites", Some(&cookie)).await;

    assert_eq!(response.status(), 200);
    let page = body_text(response).await;
    assert_eq!(outline(&page), "( alice ( bob ( dave ) carol ) )", "{page}");
    assert!(
        page.contains(r#"<time class="muted" datetime="2025-10-10">2025-10-10</time>"#),
        "{page}"
    );
}

#[tokio::test]
async fn marks_the_viewer_and_who_left_and_shows_no_emails() {
    let site = site_with_a_tree().await;
    let cookie = site.sign_in("bob").await;
    let page = body_text(site.get("/invites", Some(&cookie)).await).await;
    assert!(
        page.contains(r#"<strong class="tree-you" aria-current="true">bob</strong> <span class="muted">(you)</span>"#),
        "{page}"
    );
    assert_eq!(page.matches("(you)").count(), 1, "{page}");
    assert!(
        page.contains(r#"<span>carol</span> <span class="muted">(left)</span>"#),
        "{page}"
    );
    assert!(!page.contains("someone@example.org"), "{page}");
}
