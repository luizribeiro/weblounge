use crate::{Site, body_text};

/// The Home tab of Alice, whose confirmed email puts no reminder over it.
async fn home_of_alice() -> String {
    let site = Site::start().await;
    site.add_verified_member("Alice").await;
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
async fn offers_an_app_for_the_device_just_under_the_chat_and_nothing_else() {
    let site = Site::start().await;
    site.add_verified_member("Alice").await;
    let cookie = site.sign_in("alice").await;
    for (user_agent, offer) in [
        (crate::IPHONE, "Or set up an app on your iPhone or iPad"),
        (crate::MAC, "Or set up an app on your Mac"),
        (
            crate::ANDROID,
            "Or set up an app on your Android phone or tablet",
        ),
        (crate::CHROMEOS, "Or set up an app"),
    ] {
        let page = body_text(site.get_on("/", &cookie, user_agent).await).await;
        let chat = page.find(">Open web chat<").unwrap();
        let link = format!(r#"<a class="quiet-link" href="/help/apps">{offer}</a>"#);
        let apps = page.find(&link);
        assert!(apps.is_some_and(|apps| apps > chat), "{link}: {page}");
        let main = &page[page.find("<main").unwrap()..page.find("</main>").unwrap()];
        assert_eq!(main.matches("<a ").count(), 2, "{main}");
        for crowd in ["<form", "<dl", "data-copy", "invite-link"] {
            assert!(!main.contains(crowd), "{crowd}: {main}");
        }
    }
}

#[tokio::test]
async fn draws_the_landing_pages_chair_inline_for_the_script_to_animate() {
    let page = home_of_alice().await;
    let chair = page.find(r#"<svg xmlns="http://www.w3.org/2000/svg" id="chair""#);
    assert!(
        chair.is_some_and(|chair| chair < page.find(">Open web chat<").unwrap()),
        "{page}"
    );
}
