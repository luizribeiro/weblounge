use std::sync::{Arc, Mutex};

use axum::{
    Router,
    body::Bytes,
    http::{HeaderMap, Method, StatusCode, Uri, header::AUTHORIZATION},
};
use reqwest::Url;
use tokio::{net::TcpListener, task::JoinHandle};

use crate::Client;

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub authorization: Option<String>,
    pub body: serde_json::Value,
}

/// An HTTP server that gives every request the same answer and records it.
pub struct Stub {
    url: Url,
    requests: Arc<Mutex<Vec<Request>>>,
    server: JoinHandle<()>,
}

impl Stub {
    pub async fn answering(status: u16, body: &'static str) -> Self {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let app = Router::new().fallback(
            move |method: Method, uri: Uri, headers: HeaderMap, request: Bytes| async move {
                recorded.lock().unwrap().push(Request {
                    method: method.to_string(),
                    path: uri.path().to_owned(),
                    authorization: headers
                        .get(AUTHORIZATION)
                        .map(|value| value.to_str().unwrap().to_owned()),
                    body: serde_json::from_slice(&request).unwrap(),
                });
                (StatusCode::from_u16(status).unwrap(), body)
            },
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            url,
            requests,
            server,
        }
    }

    pub fn client(&self) -> Client {
        Client::new(self.url.clone(), "s3cret")
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.server.abort();
    }
}
