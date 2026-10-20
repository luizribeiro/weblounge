use axum::{
    body::Body,
    http::{Request, header},
    response::Response,
};
use regex::Regex;

use crate::{Site, body_text, location};

/// What starts like a PNG, which the fake scrubber stores as it is.
pub const PNG: &[u8] = b"\x89PNG\r\n\x1a\nthe rest of a screenshot";
const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"/>"#;

impl Site {
    /// Posts `body` to the upload endpoint as an IRC client does, with
    /// `authorization` and `headers` besides.
    pub(crate) async fn upload_with(
        &self,
        body: &[u8],
        authorization: Option<&str>,
        headers: &[(&str, &str)],
    ) -> Response {
        let mut request = Request::post("/upload")
            .header(header::CONTENT_TYPE, "image/png")
            .header(header::CONTENT_LENGTH, body.len())
            .header(
                header::CONTENT_DISPOSITION,
                r#"attachment; filename="Screenshot 2026-10-20.png""#,
            );
        if let Some(authorization) = authorization {
            request = request.header(header::AUTHORIZATION, authorization);
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        self.send(request.body(Body::from(body.to_vec())).unwrap())
            .await
    }

    pub(crate) async fn upload(&self, body: &[u8], authorization: Option<&str>) -> Response {
        self.upload_with(body, authorization, &[]).await
    }

    /// The `Authorization` gamja sends for the member signed in with
    /// `cookie`.
    pub(crate) async fn bearer(&self, cookie: &str) -> String {
        format!("Bearer {}", self.access_token(cookie).await)
    }

    /// A member signed in, and the `Authorization` gamja sends for them.
    pub(crate) async fn member_with_bearer(&self, account: &str) -> String {
        self.add_member(account).await;
        let cookie = self.sign_in(account).await;
        self.bearer(&cookie).await
    }

    async fn upload_rows(&self) -> i64 {
        self.query_scalar("SELECT COUNT(*) FROM uploads").await
    }

    /// Who uploaded `id`, whose it is, its type and its size.
    async fn upload_row(&self, id: &str) -> String {
        self.query_scalar(&format!(
            "SELECT account || ' ' || owner || ' ' || content_type || ' ' || size
             FROM uploads WHERE id = '{id}'"
        ))
        .await
    }
}

/// The id a link to an upload of type `extension` names.
fn id_in(link: &str, extension: &str) -> String {
    let pattern = format!(r"^https://files\.weblounge\.org/([A-Za-z0-9_-]{{22}})\.{extension}$");
    let captures = Regex::new(&pattern).unwrap().captures(link);
    captures.unwrap_or_else(|| panic!("{link}"))[1].to_owned()
}

#[tokio::test]
async fn the_web_clients_token_uploads_an_image_and_gets_its_link() {
    let site = Site::start().await;
    let bearer = site.member_with_bearer("alice").await;

    let response = site.upload(PNG, Some(&bearer)).await;

    assert_eq!(response.status(), 201);
    let link = location(&response).to_owned();
    let id = id_in(&link, "png");
    assert_eq!(body_text(response).await, format!("{link}\n"));
    assert_eq!(site.scrubber.stored(), [format!("{id}.png")]);
    assert_eq!(
        site.upload_row(&id).await,
        format!("alice alice image/png {}", PNG.len())
    );
}

#[tokio::test]
async fn every_upload_gets_a_new_id_and_the_clients_filename_is_dropped() {
    let site = Site::start().await;
    let bearer = site.member_with_bearer("alice").await;

    let first = site.upload(PNG, Some(&bearer)).await;
    let second = site.upload(PNG, Some(&bearer)).await;

    let links = [location(&first), location(&second)];
    assert_ne!(id_in(links[0], "png"), id_in(links[1], "png"));
    assert!(links.iter().all(|link| !link.contains("Screenshot")));
    assert_eq!(site.scrubber.stored().len(), 2);
}

#[tokio::test]
async fn wrong_or_missing_credentials_get_401_and_store_nothing() {
    let site = Site::start().await;
    site.member_with_bearer("alice").await;
    for authorization in [
        None,
        Some("Bearer q7Hx2mZp9vLcT4wRfBa-_wq7Hx2mZp9vLcT4wRfBa-_"),
        Some("Bearer not-a-token"),
        Some("Token q7Hx2mZp9vLcT4wRfBa-_wq7Hx2mZp9vLcT4wRfBa-_"),
        Some("Bearer"),
    ] {
        let response = site.upload(PNG, authorization).await;
        assert_eq!(response.status(), 401, "{authorization:?}");
        let challenge = &response.headers()[header::WWW_AUTHENTICATE];
        assert_eq!(challenge, r#"Basic realm="weblounge", Bearer"#);
    }
    assert!(site.scrubber.stored().is_empty());
    assert_eq!(site.upload_rows().await, 0);
}

#[tokio::test]
async fn a_removed_members_token_uploads_nothing() {
    let site = Site::start().await;
    let bearer = site.member_with_bearer("alice").await;
    site.execute("UPDATE members SET email = NULL, removed_at = 1 WHERE account = 'alice'")
        .await;

    assert_eq!(site.upload(PNG, Some(&bearer)).await.status(), 401);
}

#[tokio::test]
async fn what_isnt_an_image_gets_415_and_leaves_nothing() {
    let site = Site::start().await;
    let bearer = site.member_with_bearer("alice").await;
    for body in [
        SVG,
        b"<!doctype html><script>alert(1)</script>",
        b"%PDF-1.7\n",
    ] {
        let response = site.upload(body, Some(&bearer)).await;
        assert_eq!(response.status(), 415);
        assert_eq!(
            body_text(response).await,
            "Only images can be uploaded: PNG, JPEG, GIF or WebP.\n"
        );
    }
    assert!(site.scrubber.stored().is_empty());
    assert_eq!(site.upload_rows().await, 0);
}

#[tokio::test]
async fn while_the_scrubber_is_down_uploads_get_503() {
    let site =
        Site::start_with(|app| app.uploads.scrubber = "/nonexistent/scrubber.sock".into()).await;
    let bearer = site.member_with_bearer("alice").await;

    let response = site.upload(PNG, Some(&bearer)).await;

    assert_eq!(response.status(), 503);
    assert_eq!(site.upload_rows().await, 0);
}
