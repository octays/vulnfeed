use crate::{AppResult, errors::Error, output::push::MessageBot, utils::http_client::HttpClient};
use async_trait::async_trait;
use reqwest::header;
use serde::{Deserialize, Serialize};

const WECOM_API_URL: &str = "https://qyapi.weixin.qq.com/cgi-bin/webhook/send";

#[derive(Debug, Clone)]
pub struct WeComBot {
    /// 企微机器人 webhook key(存于 push_channel.access_token)
    pub key: String,
    pub http_client: HttpClient,
}

#[derive(Debug, Deserialize, Serialize)]
struct WeComResponse {
    errcode: i64,
    errmsg: String,
}

#[async_trait]
impl MessageBot for WeComBot {
    async fn push_markdown(&self, title: String, msg: String) -> AppResult<()> {
        let content = format!("## {title}\n{msg}");
        let message = serde_json::json!({
            "msgtype": "markdown",
            "markdown": { "content": content },
        });
        let resp: WeComResponse = self
            .http_client
            .http_client
            .post(WECOM_API_URL)
            .query(&[("key", self.key.as_str())])
            .json(&message)
            .send()
            .await
            .map_err(|e| Error::Message(format!("send wecom message failed: {e}")))?
            .json()
            .await
            .map_err(|e| Error::Message(format!("parse wecom response failed: {e}")))?;
        if resp.errcode != 0 {
            log::warn!("wecom push markdown error, err msg is {}", resp.errmsg);
            return Err(Error::Message(
                "wecom push message response errorcode not eq 0".to_owned(),
            )
            .into());
        }
        Ok(())
    }
}

impl WeComBot {
    pub fn try_new(key: String) -> AppResult<Self> {
        let mut headers = header::HeaderMap::new();
        headers.insert("Accept-Charset", header::HeaderValue::from_static("utf8"));
        let http_client = HttpClient::try_new_with_headers(headers)?;
        Ok(Self { key, http_client })
    }
}
