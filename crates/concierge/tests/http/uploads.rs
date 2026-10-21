use axum::http::header;
use regex::Regex;

use crate::{
    PASSWORD, Site, body_text, is_tab, location,
    upload::{PNG, basic},
};

impl Site {
    /// Uploads [`PNG`] as `uploader`, answering its id.
    async fn uploaded_by(&self, uploader: &str) -> String {
        let response = self.upload(PNG, Some(&basic(uploader, PASSWORD))).await;
        assert_eq!(response.status(), 201);
        let link = location(&response);
        let file = link.rsplit('/').next().unwrap();
        file.trim_end_matches(".png").to_owned()
    }

    async fn uploads_tab(&self, cookie: &str) -> String {
        let response = self.get("/uploads", Some(cookie)).await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        body_text(response).await
    }
}

/// The images a page lists, as each one's link reads.
fn listed(page: &str) -> Vec<String> {
    let image = Regex::new(r#"<a class="upload-image" href="([^"]+)">"#).unwrap();
    image.captures_iter(page).map(|c| c[1].to_owned()).collect()
}

fn link(id: &str) -> String {
    format!("https://files.weblounge.org/{id}.png")
}

#[tokio::test]
async fn a_member_without_uploads_learns_how_to_make_one_and_where_help_is() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let cookie = site.sign_in("alice").await;

    let page = site.uploads_tab(&cookie).await;

    assert!(is_tab(&page, "/uploads"), "{page}");
    assert!(page.contains("No uploads yet."), "{page}");
    assert!(
        page.contains(r#"paste or attach it in <a href="https://irc.weblounge.org/">the web client</a> or your IRC app"#),
        "{page}"
    );
    assert!(
        page.contains(r#"<span class="invite-count-number">0 MB of 500 MB</span> used"#),
        "{page}"
    );
    assert!(
        page.contains(r#"<progress class="usage-meter" max="500000000" value="0""#),
        "{page}"
    );
    assert!(
        page.contains(
            r#"The images you shared in the chat, kept for 30 days. <a href="/help/share-an-image">Learn how sharing works</a>"#
        ),
        "{page}"
    );
    assert!(!page.contains("How uploads work"), "{page}");
    assert!(listed(&page).is_empty());
}

#[tokio::test]
async fn a_member_sees_their_uploads_and_their_bots_newest_first_and_no_one_elses() {
    let site = Site::start().await;
    site.alice_and_marvin().await;
    site.add_member("bob").await;
    let alices = site.uploaded_by("alice").await;
    let marvins = site.uploaded_by("Marvin").await;
    let bobs = site.uploaded_by("bob").await;
    site.execute(&format!(
        "UPDATE uploads SET created_at = created_at - 86400 WHERE id = '{alices}'"
    ))
    .await;
    let cookie = site.sign_in("alice").await;

    let page = site.uploads_tab(&cookie).await;

    assert_eq!(listed(&page), [link(&marvins), link(&alices)]);
    assert!(!page.contains(&bobs), "{page}");
    assert!(!page.contains("No uploads yet."), "{page}");
    assert!(
        page.contains("30 days left · 0.1 MB · from Marvin"),
        "{page}"
    );
    assert!(page.contains("29 days left · 0.1 MB</span>"), "{page}");
    let image = format!(r#"<img src="{}" alt="Your upload of "#, link(&alices));
    assert!(page.contains(&image), "{page}");
    assert!(
        page.contains(r#"loading="lazy" decoding="async">"#),
        "{page}"
    );
    let used = 2 * PNG.len();
    assert!(
        page.contains(&format!(r#"max="500000000" value="{used}""#)),
        "{page}"
    );
    assert!(
        page.contains(&format!(
            r#"href="/uploads/{alices}/delete" data-dialog="delete-{alices}""#
        )),
        "{page}"
    );
    assert!(
        page.contains(&format!(
            r#"<form class="dialog-actions" method="post" action="/uploads/{marvins}/delete">"#
        )),
        "{page}"
    );
}

#[tokio::test]
async fn deleting_an_upload_takes_its_file_and_row_and_its_link_with_it() {
    let site = Site::start().await;
    site.alice_and_marvin().await;
    let marvins = site.uploaded_by("Marvin").await;
    let alices = site.uploaded_by("alice").await;
    let cookie = site.sign_in("alice").await;

    let asked = site
        .get(&format!("/uploads/{marvins}/delete"), Some(&cookie))
        .await;
    assert_eq!(asked.status(), 200);
    let question = body_text(asked).await;
    assert!(
        question.contains("Delete this image? Its link stops working right away"),
        "{question}"
    );
    let response = site
        .post(&format!("/uploads/{marvins}/delete"), &[], Some(&cookie))
        .await;

    assert_eq!(response.status(), 303);
    assert_eq!(location(&response), "/uploads");
    assert_eq!(site.scrubber.stored(), [format!("{alices}.png")]);
    let page = site.uploads_tab(&cookie).await;
    assert_eq!(listed(&page), [link(&alices)]);
}

#[tokio::test]
async fn nobody_deletes_anyone_elses_upload_or_one_that_isnt_there() {
    let site = Site::start().await;
    site.add_member("alice").await;
    site.add_member("bob").await;
    let bobs = site.uploaded_by("bob").await;
    let cookie = site.sign_in("alice").await;

    for id in [
        bobs.as_str(),
        "AAAAAAAAAAAAAAAAAAAAAA",
        "../../etc/passwd",
        "short",
    ] {
        let path = format!("/uploads/{id}/delete");
        assert_eq!(site.get(&path, Some(&cookie)).await.status(), 404, "{id}");
        assert_eq!(
            site.post(&path, &[], Some(&cookie)).await.status(),
            404,
            "{id}"
        );
    }
    assert_eq!(site.scrubber.stored(), [format!("{bobs}.png")]);
}

#[tokio::test]
async fn another_site_cannot_delete_a_members_upload() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let alices = site.uploaded_by("alice").await;
    let cookie = site.sign_in("alice").await;

    let origin = [(header::ORIGIN.as_str(), "https://evil.example")];
    let path = format!("/uploads/{alices}/delete");
    let response = site.post_with(&path, &[], Some(&cookie), &origin).await;

    assert_eq!(response.status(), 403);
    assert_eq!(site.scrubber.stored().len(), 1);
}

#[tokio::test]
async fn guests_are_sent_to_sign_in_and_delete_nothing() {
    let site = Site::start().await;
    site.add_member("alice").await;
    let alices = site.uploaded_by("alice").await;

    let response = site
        .post(&format!("/uploads/{alices}/delete"), &[], None)
        .await;

    assert_eq!(response.status(), 303);
    assert!(location(&response).starts_with("/signin"));
    assert_eq!(site.scrubber.stored().len(), 1);
}
