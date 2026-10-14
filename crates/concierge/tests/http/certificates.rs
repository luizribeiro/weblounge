use crate::{Site, body_text};

#[tokio::test]
async fn guests_get_the_guide_with_a_placeholder_nick() {
    let site = Site::start().await;
    let response = site.get("/clients", None).await;
    assert_eq!(response.status(), 200);
    let page = body_text(response).await;
    assert!(
        page.contains(r#"<section class="faq" id="certificates""#),
        "{page}"
    );
    assert!(
        page.contains("ec_paramgen_curve:prime256v1") && page.contains("-subj /CN=YOUR_NICK"),
        "{page}"
    );
    assert!(
        page.contains("-fingerprint -sha256 \\\n  | cut -d= -f2 | tr -d : | tr A-F a-f"),
        "{page}"
    );
    assert!(
        page.contains("/msg NickServ CERT ADD &#60;fingerprint&#62;"),
        "{page}"
    );
    assert!(page.contains("[servers.weblounge.sasl.external]"), "{page}");
    assert!(page.contains("sasl_mechanism external"), "{page}");
    assert!(page.contains("<code>irc.weblounge.org</code>"), "{page}");
}

#[tokio::test]
async fn members_get_their_nick_in_the_certificate() {
    let site = Site::start().await;
    site.add_member("Alice").await;
    let cookie = site.sign_in("alice").await;
    let page = body_text(site.get("/clients", Some(&cookie)).await).await;
    assert!(page.contains("-subj /CN=Alice"), "{page}");
}

#[tokio::test]
async fn every_client_guide_links_to_it() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;
    for path in ["/clients/halloy", "/clients/gamja"] {
        let page = body_text(site.get(path, Some(&cookie)).await).await;
        assert!(
            page.contains(r#"href="/clients#certificates""#),
            "{path}: {page}"
        );
    }
}
