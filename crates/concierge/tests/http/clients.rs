use concierge::clients::{CLIENTS, Group};

use crate::body_text;

#[tokio::test]
async fn the_apps_page_picks_apps_for_the_visitors_device() {
    let (site, cookie) = crate::signed_in_alice().await;
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
        let page = body_text(site.get_on("/help/apps", &cookie, user_agent).await).await;
        assert!(page.contains(&format!("<h1>{heading}</h1>")), "{page}");
        assert_eq!(crate::picks(&page), expected, "{page}");
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
    let (site, cookie) = crate::signed_in_alice().await;
    for user_agent in [crate::CHROMEOS, "curl/8.16.0"] {
        let page = body_text(site.get_on("/help/apps", &cookie, user_agent).await).await;
        assert!(page.contains("<h1>Chat on this device</h1>"), "{page}");
        assert!(crate::picks(&page).is_empty(), "{page}");
        assert!(!page.contains("Set up "), "{page}");
        assert!(!page.contains("skip the setup"), "{page}");
        assert!(crate::opens_the_chat(&page), "{page}");
    }
}

#[tokio::test]
async fn the_apps_page_ends_with_all_apps_and_nothing_more() {
    let (site, cookie) = crate::signed_in_alice().await;
    for user_agent in [crate::MAC, crate::CHROMEOS] {
        let page = body_text(site.get_on("/help/apps", &cookie, user_agent).await).await;
        let main = &page[page.find("<main").unwrap()..page.find("</main>").unwrap()];
        assert!(
            main.trim_end().ends_with(
                r#"<p class="see-also">Using something else? <a href="/help/apps/all">See all apps</a></p>"#
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
    let (site, cookie) = crate::signed_in_alice().await;

    let response = site.get("/help/apps/halloy", Some(&cookie)).await;

    assert_eq!(response.status(), 200);
    let page = body_text(response).await;
    assert!(page.contains("<h1>Set up Halloy</h1>"), "{page}");
    assert!(page.contains("nickname = &#34;Alice&#34;"), "{page}");
    assert!(
        page.contains("server = &#34;irc.weblounge.org&#34;"),
        "{page}"
    );
    assert!(page.contains(r#"data-copy="[servers.weblounge]"#), "{page}");
    assert!(!page.contains("button-small"), "{page}");
    assert_eq!(
        page.matches(r#"<button class="link-button copy-button" type="button" data-copy="#)
            .count(),
        page.matches(">Copy</button>").count(),
        "{page}"
    );
    assert!(
        page.contains(r#"<p class="ircs-link"><a href="ircs://irc.weblounge.org/#lounge"><code>ircs://irc.weblounge.org/#lounge</code></a></p>"#),
        "{page}"
    );
    assert!(page.contains(r#"data-copy="Alice""#), "{page}");
    assert!(!page.contains(crate::PASSWORD), "{page}");
}

#[tokio::test]
async fn every_guide_says_where_to_get_the_app_then_how_to_add_weblounge() {
    let (site, cookie) = crate::signed_in_alice().await;
    for client in CLIENTS.iter().filter(|client| !client.in_browser()) {
        let response = site.get(&client.guide_url(), Some(&cookie)).await;
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
        assert!(page.contains(r#"data-copy="Alice""#), "{page}");
        assert!(page.contains("<code>6697</code>"), "{page}");
        assert!(
            page.contains("<dd>The one you sign in with here.</dd>"),
            "{page}"
        );
    }
}

#[tokio::test]
async fn gamjas_guide_just_opens_the_chat() {
    let page = body_text(crate::alices("/help/apps/gamja").await).await;
    assert!(page.contains("<h1>Set up gamja</h1>"), "{page}");
    assert!(crate::opens_the_chat(&page), "{page}");
    assert!(!page.contains("<ol"), "{page}");
    assert!(!page.contains("codeberg.org/emersion/gamja"), "{page}");
    assert!(!page.contains("<code>PLAIN</code>"), "{page}");
    assert!(!page.contains("<pre>"), "{page}");
}

#[tokio::test]
async fn phone_clients_get_the_fields_and_no_config() {
    let page = body_text(crate::alices("/help/apps/goguma").await).await;
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
    let (site, cookie) = crate::signed_in_alice().await;
    for slug in ["halloy", "senpai", "weechat"] {
        let page = body_text(site.get(&format!("/help/apps/{slug}"), Some(&cookie)).await).await;
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
    let (site, cookie) = crate::signed_in_alice().await;
    for client in &CLIENTS {
        let page = body_text(site.get(&client.guide_url(), Some(&cookie)).await).await;
        let main = &page[page.find("<main").unwrap()..page.find("</main>").unwrap()];
        assert!(
            main.trim_end().ends_with(
                r#"<p class="see-also"><a href="/help/apps/all">Back to all apps</a></p>"#
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
    let page = body_text(crate::alices("/help/apps/all").await).await;
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
    let (site, cookie) = crate::signed_in_alice().await;
    let page = body_text(site.get("/help/apps/all", Some(&cookie)).await).await;
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
    let response = crate::alices("/help/apps/hexchat").await;
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn every_old_address_of_a_guide_moves_for_good_to_its_new_one() {
    let (site, cookie) = crate::signed_in_alice().await;
    for (old, new) in [
        ("/clients", "/help/apps"),
        ("/clients/all", "/help/apps/all"),
        ("/clients/halloy", "/help/apps/halloy"),
        (
            "/clients/halloy/certificate",
            "/help/apps/halloy/certificate",
        ),
        (
            "/clients/halloy?from=welcome",
            "/help/apps/halloy?from=welcome",
        ),
    ] {
        let response = site.get(old, Some(&cookie)).await;
        assert_eq!(response.status(), 308, "{old}");
        assert_eq!(crate::location(&response), new, "{old}");
        assert_eq!(site.get(new, Some(&cookie)).await.status(), 200, "{new}");
    }
}
