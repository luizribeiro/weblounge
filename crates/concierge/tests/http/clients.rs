use concierge::clients::{CLIENTS, Group, NICK_PLACEHOLDER};

use crate::{Site, body_text};

/// The names of the apps `page` picks, in its order.
fn picks(page: &str) -> Vec<&'static str> {
    let mut picks: Vec<_> = CLIENTS
        .iter()
        .filter_map(|client| {
            let card = format!(r#"<h2 id="pick-{}">{}</h2>"#, client.slug, client.name);
            Some((page.find(&card)?, client.name))
        })
        .collect();
    picks.sort();
    picks.into_iter().map(|(_, name)| name).collect()
}

#[tokio::test]
async fn the_clients_page_picks_apps_for_the_visitors_device() {
    let site = Site::start().await;
    for (user_agent, heading, expected) in [
        (crate::MAC, "Chat on your Mac", &["Halloy", "Mango"][..]),
        (crate::WINDOWS, "Chat on your PC", &["Halloy"][..]),
        (crate::LINUX, "Chat on your computer", &["Halloy"][..]),
        (
            crate::IPHONE,
            "Chat on your iPhone or iPad",
            &["Mango", "Goguma"][..],
        ),
        (
            crate::ANDROID,
            "Chat on your Android phone or tablet",
            &["Goguma"][..],
        ),
    ] {
        let page = body_text(site.get_on("/clients", user_agent).await).await;
        assert!(page.contains(&format!("<h1>{heading}</h1>")), "{page}");
        assert_eq!(picks(&page), expected, "{page}");
        let lede = match expected.len() {
            1 => "Our pick for this device. It takes",
            _ => "Our picks for this device. Each takes",
        };
        assert!(page.contains(lede), "{page}");
        for name in expected {
            let client = CLIENTS.iter().find(|client| client.name == *name).unwrap();
            let card = &page[page.find(&format!(r#"id="pick-{}""#, client.slug)).unwrap()..];
            let card = &card[..card.find("</section>").unwrap()];
            assert!(
                card.contains(&format!("{} · {}", client.runs_on, client.price)),
                "{card}"
            );
            assert!(card.contains(client.note), "{card}");
            let button = format!(
                r#"<a class="button" href="{}">Set up {name}</a>"#,
                client.guide_url()
            );
            assert!(card.contains(&button), "{card}");
        }
        let skip = page.find("Or skip the setup:").unwrap();
        assert!(skip > page.rfind("Set up ").unwrap(), "{page}");
        assert!(crate::opens_the_chat(&page[skip..]), "{page}");
    }
}

#[tokio::test]
async fn an_unknown_device_gets_just_the_web_client() {
    let site = Site::start().await;
    for user_agent in [crate::CHROMEOS, "curl/8.16.0"] {
        let page = body_text(site.get_on("/clients", user_agent).await).await;
        assert!(page.contains("<h1>Chat on this device</h1>"), "{page}");
        assert!(picks(&page).is_empty(), "{page}");
        assert!(!page.contains("Set up "), "{page}");
        assert!(!page.contains("skip the setup"), "{page}");
        assert!(crate::opens_the_chat(&page), "{page}");
    }
}

#[tokio::test]
async fn the_clients_page_ends_with_all_apps_and_nothing_more() {
    let site = Site::start().await;
    for user_agent in [crate::MAC, crate::CHROMEOS] {
        let page = body_text(site.get_on("/clients", user_agent).await).await;
        let main = &page[page.find("<main").unwrap()..page.find("</main>").unwrap()];
        assert!(
            main.trim_end().ends_with(
                r#"<p class="see-also">Using something else? <a href="/clients/all">See all apps</a></p>"#
            ),
            "{main}"
        );
        for crowd in ["<dl", "data-copy", "certificate", "chips", "<pre>"] {
            assert!(!main.contains(crowd), "{crowd}: {main}");
        }
    }
}

#[tokio::test]
async fn a_member_gets_a_config_with_their_nick_and_no_password() {
    let site = Site::start().await;
    site.add_member("Alice").await;
    let cookie = site.sign_in("alice").await;

    let response = site.get("/clients/halloy", Some(&cookie)).await;

    assert_eq!(response.status(), 200);
    let page = body_text(response).await;
    assert!(page.contains("<h1>Set up Halloy</h1>"), "{page}");
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
    for client in CLIENTS.iter().filter(|client| !client.in_browser()) {
        let response = site.get(&client.guide_url(), None).await;
        assert_eq!(response.status(), 200, "{}", client.slug);
        let page = body_text(response).await;
        assert!(
            page.contains(&format!("<h1>Set up {}</h1>", client.name)),
            "{page}"
        );
        let get = page.find("<h2>Get it</h2>").unwrap();
        let add = page.find("<h2>Add weblounge</h2>").unwrap();
        for link in client.links {
            let at = page.find(&format!(r#"<a href="{}">"#, link.url)).unwrap();
            assert!(get < at && at < add, "{page}");
        }
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
    assert!(page.contains("<h1>Set up gamja</h1>"), "{page}");
    assert!(crate::opens_the_chat(&page), "{page}");
    assert!(!page.contains("<ol"), "{page}");
    assert!(!page.contains("codeberg.org/emersion/gamja"), "{page}");
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
    assert!(!page.contains("<details"), "{page}");
    assert!(
        page.contains(r#"<div class="card connection-card">"#),
        "{page}"
    );
}

#[tokio::test]
async fn apps_with_a_config_fold_the_connection_details_away_under_it() {
    let site = Site::start().await;
    for slug in ["halloy", "senpai", "weechat"] {
        let page = body_text(site.get(&format!("/clients/{slug}"), None).await).await;
        let snippet = page.find("<pre>").unwrap();
        let details = page.find("<details").unwrap();
        assert!(snippet < details, "{page}");
        assert!(
            page[details..].starts_with(
                "<details class=\"more\">\n            <summary>Connection details</summary>"
            ),
            "{page}"
        );
        assert!(page[details..].contains("<code>PLAIN</code>"), "{page}");
    }
}

#[tokio::test]
async fn every_guide_ends_with_the_way_back_to_all_apps_and_no_chips() {
    let site = Site::start().await;
    for client in &CLIENTS {
        let page = body_text(site.get(&client.guide_url(), None).await).await;
        let main = &page[page.find("<main").unwrap()..page.find("</main>").unwrap()];
        assert!(
            main.trim_end().ends_with(
                r#"<p class="see-also"><a href="/clients/all">Back to all apps</a></p>"#
            ),
            "{main}"
        );
        assert!(!main.contains("chips"), "{main}");
        let others = CLIENTS.iter().filter(|other| other.slug != client.slug);
        for other in others {
            let link = format!(r#"href="{}""#, other.guide_url());
            assert!(!main.contains(&link), "{}: {main}", client.slug);
        }
    }
}

#[tokio::test]
async fn all_apps_are_names_by_where_they_run_linking_their_guides() {
    let page = body_text(Site::start().await.get("/clients/all", None).await).await;
    assert!(page.contains("<h1>All apps</h1>"), "{page}");
    let mut last = 0;
    for group in Group::ALL {
        let start = page.find(&format!(">{}</h2>", group.title())).unwrap();
        assert!(start > last, "{} is out of order: {page}", group.title());
        last = start;
        let list = &page[start..start + page[start..].find("</ul>").unwrap()];
        for client in group.clients() {
            let link = format!(r#"<a href="{}">{}</a>"#, client.guide_url(), client.name);
            assert!(list.contains(&link), "{link}: {list}");
        }
    }
    for client in &CLIENTS {
        assert!(!page.contains(client.price), "{}: {page}", client.slug);
    }
    assert!(!page.contains("<pre>"), "{page}");
}

#[tokio::test]
async fn all_apps_keep_the_connection_details_folded_away() {
    let site = Site::start().await;
    site.add_member("Alice").await;
    let cookie = site.sign_in("alice").await;
    let page = body_text(site.get("/clients/all", Some(&cookie)).await).await;
    let details = &page[page.find("<details").unwrap()..page.find("</details>").unwrap()];
    assert!(
        details.contains("<summary>Using an app that isn't here?</summary>"),
        "{details}"
    );
    assert!(
        details.contains("Any IRC app with TLS and SASL works."),
        "{details}"
    );
    for value in ["irc.weblounge.org", "6697", "Alice"] {
        assert!(
            details.contains(&format!("<code>{value}</code>")),
            "{details}"
        );
        assert!(
            details.contains(&format!(r#"data-copy="{value}" hidden>Copy</button>"#)),
            "{details}"
        );
    }
    assert!(details.contains("· TLS"), "{details}");
    assert!(details.contains("<code>PLAIN</code>"), "{details}");
    assert!(details.contains(r#"aria-live="polite""#), "{details}");
}

#[tokio::test]
async fn unknown_clients_are_not_found() {
    let response = Site::start().await.get("/clients/hexchat", None).await;
    assert_eq!(response.status(), 404);
}
