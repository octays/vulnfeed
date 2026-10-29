use error_stack::ResultExt;
use reqwest::header::{self, HeaderMap};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{AppResult, errors::Error};

#[derive(Debug, Clone)]
pub struct HttpClient {
    pub http_client: reqwest::Client,
    insecure_client: Option<reqwest::Client>,
}

static SKIP_SYNC_CERT_VERIFY: AtomicBool = AtomicBool::new(false);

pub fn set_skip_sync_cert_verify(skip: bool) {
    SKIP_SYNC_CERT_VERIFY.store(skip, Ordering::Relaxed);
}

impl HttpClient {
    pub fn try_new_with_headers(mut headers: HeaderMap) -> AppResult<Self> {
        headers.insert("User-Agent", header::HeaderValue::from_static("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36"));
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .danger_accept_invalid_certs(true)
            .default_headers(headers)
            .build()
            .change_context_lazy(|| Error::Message("Failed to create HTTP client".to_string()))?;
        Ok(Self {
            http_client: client,
            insecure_client: None,
        })
    }
    pub fn try_new() -> AppResult<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .danger_accept_invalid_certs(true)
            .build()
            .change_context_lazy(|| Error::Message("Failed to create HTTP client".to_string()))?;
        Ok(Self {
            http_client: client,
            insecure_client: None,
        })
    }

    pub fn try_new_sync() -> AppResult<Self> {
        Self::try_new_sync_with_headers(HeaderMap::new())
    }

    pub fn try_new_sync_with_headers(mut headers: HeaderMap) -> AppResult<Self> {
        headers.insert("User-Agent", header::HeaderValue::from_static("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36"));
        let build = |insecure| {
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .danger_accept_invalid_certs(insecure)
                .default_headers(headers.clone())
                .build()
                .change_context_lazy(|| Error::Message("Failed to create HTTP client".to_string()))
        };
        Ok(Self {
            http_client: build(false)?,
            insecure_client: Some(build(true)?),
        })
    }

    fn client(&self) -> &reqwest::Client {
        if SKIP_SYNC_CERT_VERIFY.load(Ordering::Relaxed) {
            self.insecure_client.as_ref().unwrap_or(&self.http_client)
        } else {
            &self.http_client
        }
    }

    pub async fn get_json(&self, url: &str) -> AppResult<reqwest::Response> {
        let content = self
            .client()
            .get(url)
            .send()
            .await
            .change_context_lazy(|| Error::Message("Failed to send HTTP request".to_string()))?;
        Ok(content)
    }
    pub async fn get_html_content(&self, url: &str) -> AppResult<String> {
        let content = self
            .client()
            .get(url)
            .send()
            .await
            .change_context_lazy(|| Error::Message("Failed to send HTTP request".to_string()))?
            .text()
            .await
            .change_context_lazy(|| {
                Error::Message("Failed to parse response body as text".to_string())
            })?;
        Ok(content)
    }

    pub async fn post_json<Body>(&self, url: &str, body: &Body) -> AppResult<reqwest::Response>
    where
        Body: serde::Serialize,
    {
        let content = self
            .client()
            .post(url)
            .json(body)
            .send()
            .await
            .change_context_lazy(|| Error::Message("Failed to send HTTP request".to_string()))?;
        Ok(content)
    }

    pub async fn get(&self, url: &str) -> AppResult<reqwest::Response> {
        let content = self
            .client()
            .get(url)
            .send()
            .await
            .change_context_lazy(|| Error::Message("Failed to send HTTP request".to_string()))?;
        Ok(content)
    }
}
