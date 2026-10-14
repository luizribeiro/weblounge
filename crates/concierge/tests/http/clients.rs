use concierge::clients::{CLIENTS, NICK_PLACEHOLDER};

use crate::{Site, body_text};

#[tokio::test]
async fn a_member_gets_a_config_with_their_nick_and_no_password() {
    let site = Site::start().await;
    site.add_member("Alice").await;
    let cookie = site.sign_in("alice").await;

    let response = site.get("/clients/halloy", Some(&cookie)).await;

    assert_eq!(response.status(), 200);
    let page = body_text(response).await;
    assert!(page.contains("<h1>Halloy</h1>"), "{page}");
    assert!(page.contains("nickname = &#34;Alice&#34;"), "{page}");
    assert!(
        page.contains("server = &#34;irc.weblounge.org&#34;"),
        "{page}"
    );
    assert!(page.contains(r#"data-copy="[servers.weblounge]"#), "{page}");
    assert!(
        page.contains(r#"href="ircs://irc.weblounge.org/#lounge""#),
        "{page}"
    );
    assert!(page.contains(r#"data-copy="Alice""#), "{page}");
    assert!(!page.contains(crate::PASSWORD), "{page}");
}

#[tokio::test]
async fn guests_get_every_guide_with_a_placeholder_nick() {
    let site = Site::start().await;
    for client in CLIENTS.iter().filter(|client| !client.in_browser) {
        let response = site.get(&client.guide_url(), None).await;
        assert_eq!(response.status(), 200, "{}", client.slug);
        let page = body_text(response).await;
        assert!(
            page.contains(&format!("<h1>{}</h1>", client.name)),
            "{page}"
        );
        assert!(page.contains("<dd>Your nick.</dd>"), "{page}");
        assert!(page.contains("<code>6697</code>"), "{page}");
        let has_config = client.snippet(&crate::irc_server(), None).is_some();
        assert_eq!(page.contains(NICK_PLACEHOLDER), has_config, "{page}");
        assert!(page.contains(r#"<a href="/signin">Sign in</a>"#), "{page}");
    }
}

#[tokio::test]
async fn gamjas_guide_just_opens_the_chat() {
    let page = body_text(Site::start().await.get("/clients/gamja", None).await).await;
    assert!(page.contains("<h1>gamja</h1>"), "{page}");
    assert!(crate::opens_the_chat(&page), "{page}");
    assert!(!page.contains("<code>PLAIN</code>"), "{page}");
    assert!(!page.contains("<pre>"), "{page}");
}

#[tokio::test]
async fn phone_clients_get_the_fields_and_no_config() {
    let page = body_text(Site::start().await.get("/clients/goguma", None).await).await;
    assert!(
        page.contains("f-droid.org/packages/fr.emersion.goguma"),
        "{page}"
    );
    assert!(!page.contains("<pre>"), "{page}");
    assert!(!page.contains("ircs://"), "{page}");
}

#[tokio::test]
async fn every_guide_and_the_account_home_link_all_the_guides() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for path in ["/", "/clients/senpai"] {
        let page = body_text(site.get(path, Some(&cookie)).await).await;
        for client in &CLIENTS {
            let link = format!(r#"href="/clients/{}""#, client.slug);
            assert!(page.contains(&link), "{path} lacks {link}");
        }
    }
}

#[tokio::test]
async fn unknown_clients_are_not_found() {
    let response = Site::start().await.get("/clients/hexchat", None).await;
    assert_eq!(response.status(), 404);
}
