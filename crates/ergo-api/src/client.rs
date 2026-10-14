use std::{fmt, time::Duration};

use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{Error, ErrorCode, Result};

const TIMEOUT: Duration = Duration::from_secs(10);

/// A request to one of Ergo's API endpoints.
///
/// Implemented with the crate's `endpoint!` macro: one line per endpoint.
pub trait Endpoint: Serialize {
    /// The endpoint's path, such as `/v1/check_auth`.
    const PATH: &'static str;
    /// The fields of a successful response.
    type Response: DeserializeOwned;
}

/// A client for Ergo's API on a trusted loopback listener. Cheap to clone.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: Url,
    token: String,
}

impl Client {
    /// A client for the API listening at `base_url`, such as
    /// `http://127.0.0.1:8089`, authenticating with the bearer `token`.
    ///
    /// Only plain HTTP is supported: the API belongs on loopback.
    pub fn new(base_url: Url, token: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(TIMEOUT)
            .build()
            .expect("a client without TLS needs nothing that could fail to load");
        Self {
            http,
            base_url,
            token: token.into(),
        }
    }

    /// Sends `request` to its endpoint and decodes a successful response.
    ///
    /// A response with `success: false` becomes [`Error::Refused`].
    pub async fn call<E: Endpoint>(&self, request: &E) -> Result<E::Response> {
        let mut url = self.base_url.clone();
        url.set_path(E::PATH);
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(request)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        match status {
            StatusCode::OK => decode(&body),
            StatusCode::UNAUTHORIZED => Err(Error::Unauthorized(body.trim_end().to_owned())),
            status => Err(Error::Status {
                status,
                body: body.trim_end().to_owned(),
            }),
        }
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("base_url", &self.base_url.as_str())
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Outcome {
    success: bool,
    error_code: Option<ErrorCode>,
    error: Option<String>,
}

/// Failures omit the response's fields, so `success` is checked first.
pub(crate) fn decode<T: DeserializeOwned>(body: &str) -> Result<T> {
    let json: serde_json::Value = serde_json::from_str(body)?;
    let outcome = Outcome::deserialize(&json)?;
    if outcome.success {
        Ok(T::deserialize(json)?)
    } else {
        Err(Error::Refused {
            code: outcome.error_code,
            message: outcome.error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stub::Stub;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Greet {
        account_name: String,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Greeting {
        text: String,
    }

    endpoint!(Greet, "/v1/greet" => Greeting);

    fn greet() -> Greet {
        Greet {
            account_name: "alice".into(),
        }
    }

    #[tokio::test]
    async fn posts_the_request_as_json_with_the_bearer_token() {
        let stub = Stub::answering(200, r#"{"success":true,"text":"hi alice"}"#).await;

        let greeting = stub.client().call(&greet()).await.unwrap();

        assert_eq!(greeting.text, "hi alice");
        let [request] = stub.requests().try_into().unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/greet");
        assert_eq!(request.authorization.as_deref(), Some("Bearer s3cret"));
        assert_eq!(request.body, serde_json::json!({"accountName": "alice"}));
    }

    #[tokio::test]
    async fn a_rejected_token_carries_ergos_plain_text_answer() {
        let stub = Stub::answering(401, "Unauthorized\n").await;
        let error = stub.client().call(&greet()).await.unwrap_err();
        assert!(matches!(error, Error::Unauthorized(body) if body == "Unauthorized"));
    }

    #[tokio::test]
    async fn other_statuses_keep_their_plain_text_body() {
        let body = "failed to deserialize json request: unexpected EOF\n";
        let stub = Stub::answering(400, body).await;
        let error = stub.client().call(&greet()).await.unwrap_err();
        assert!(
            matches!(&error, Error::Status { status, body: got } if *status == 400 && got == body.trim_end()),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_refusal_carries_its_code_and_message_without_decoding_the_response() {
        let stub = Stub::answering(
            200,
            r#"{"success":false,"error":"Name reserved due to a prior registration","errorCode":"ACCOUNT_EXISTS"}"#,
        )
        .await;
        let error = stub.client().call(&greet()).await.unwrap_err();
        assert!(
            matches!(&error, Error::Refused { code: Some(ErrorCode::AccountExists), message: Some(m) }
                if m == "Name reserved due to a prior registration"),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_bare_refusal_has_neither_code_nor_message() {
        let stub = Stub::answering(200, r#"{"success":false}"#).await;
        let error = stub.client().call(&greet()).await.unwrap_err();
        assert!(
            matches!(
                error,
                Error::Refused {
                    code: None,
                    message: None
                }
            ),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn answers_that_are_not_the_expected_json_fail_to_decode() {
        for body in [
            "<html>",
            r#"{"text":"no success field"}"#,
            r#"{"success":true}"#,
        ] {
            let stub = Stub::answering(200, body).await;
            let error = stub.client().call(&greet()).await.unwrap_err();
            assert!(matches!(error, Error::Decode(_)), "{body}: {error:?}");
        }
    }

    #[tokio::test]
    async fn an_unreachable_server_is_a_transport_error() {
        let stub = Stub::answering(200, "{}").await;
        let client = stub.client();
        drop(stub);
        let error = client.call(&greet()).await.unwrap_err();
        assert!(matches!(error, Error::Transport(_)), "{error:?}");
    }

    #[test]
    fn debug_output_leaves_the_token_out() {
        let client = Client::new("http://127.0.0.1:8089".parse().unwrap(), "s3cret");
        assert!(!format!("{client:?}").contains("s3cret"));
    }
}
