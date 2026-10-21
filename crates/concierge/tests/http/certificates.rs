use crate::body_text;

#[tokio::test]
async fn members_get_halloys_guide_with_their_nick() {
    let (site, cookie) = crate::signed_in_alice().await;
    let response = site.get("/clients/halloy/certificate", Some(&cookie)).await;
    assert_eq!(response.status(), 200);
    let page = body_text(response).await;
    assert!(
        page.contains("<h1>Log in to Halloy with a certificate</h1>"),
        "{page}"
    );
    assert!(
        page.contains("ec_paramgen_curve:prime256v1") && page.contains("-subj /CN=Alice"),
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
    assert!(page.contains("<code>irc.weblounge.org</code>"), "{page}");
    assert!(page.contains("[servers.weblounge.sasl.external]"), "{page}");
    assert!(!page.contains("sasl_mechanism external"), "{page}");
    assert!(page.contains(r#"<a href="/clients/halloy">Back to Halloy</a>"#));
}

#[tokio::test]
async fn weechat_gets_its_one_file_and_its_commands_only() {
    let page = body_text(crate::alices("/clients/weechat/certificate").await).await;
    assert!(
        page.contains("cat weblounge.key weblounge.crt &#62; ~/.config/weechat/tls/weblounge.pem"),
        "{page}"
    );
    assert!(page.contains("sasl_mechanism external"), "{page}");
    assert!(!page.contains("sasl.external"), "{page}");
}

#[tokio::test]
async fn apps_that_cant_present_one_have_no_certificate_guide() {
    let (site, cookie) = crate::signed_in_alice().await;
    for slug in ["gamja", "senpai", "goguma", "mango", "hexchat"] {
        let response = site
            .get(&format!("/clients/{slug}/certificate"), Some(&cookie))
            .await;
        assert_eq!(response.status(), 404, "{slug}");
    }
}

#[tokio::test]
async fn only_the_guides_of_apps_that_can_link_to_it() {
    let (site, cookie) = crate::signed_in_alice().await;
    for slug in ["halloy", "weechat"] {
        let page = body_text(site.get(&format!("/clients/{slug}"), Some(&cookie)).await).await;
        let link =
            format!(r#"<a href="/clients/{slug}/certificate">Log in with a certificate</a>"#);
        assert!(page.contains(&link), "{slug}: {page}");
        assert!(
            page.contains("Rather not keep a password in the config?"),
            "{page}"
        );
    }
    for slug in ["gamja", "senpai", "goguma", "mango"] {
        let page = body_text(site.get(&format!("/clients/{slug}"), Some(&cookie)).await).await;
        assert!(!page.contains("/certificate"), "{slug}: {page}");
    }
}

#[tokio::test]
async fn the_clients_page_no_longer_walks_through_certificates() {
    let page = body_text(crate::alices("/clients").await).await;
    assert!(!page.contains("certificate"), "{page}");
}
