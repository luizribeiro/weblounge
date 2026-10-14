use crate::{Site, body_text};

async fn home_of_alice() -> String {
    let site = Site::start().await;
    site.add_member("Alice").await;
    let cookie = site.sign_in("alice").await;
    body_text(site.get("/", Some(&cookie)).await).await
}

#[tokio::test]
async fn welcomes_the_member_by_name_and_opens_the_chat() {
    let page = home_of_alice().await;
    assert!(
        page.contains(r#"<h1 id="home-title">Pull up a chair, Alice.</h1>"#),
        "{page}"
    );
    assert!(crate::opens_the_chat(&page), "{page}");
}

#[tokio::test]
async fn leads_to_other_clients_just_under_the_chat_and_nothing_else() {
    let page = home_of_alice().await;
    let chat = page.find(">Open chat<").unwrap();
    let clients = page.find(r#"<a href="/clients">Use another client</a>"#);
    assert!(clients.is_some_and(|clients| clients > chat), "{page}");
    for crowd in ["<form", "<dl", "data-copy", "invite-link"] {
        let main = &page[page.find("<main").unwrap()..page.find("</main>").unwrap()];
        assert!(!main.contains(crowd), "{crowd}: {main}");
    }
}
