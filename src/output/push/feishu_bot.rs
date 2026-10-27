use crate::{
    AppResult,
    errors::Error,
    output::push::{MessageBot, markdown_for_feishu},
    utils::{http_client::HttpClient, util::calc_hmac_sha256},
};
use async_trait::async_trait;
use base64::{Engine, prelude::BASE64_STANDARD};
use reqwest::header;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

const FEISHU_API_URL: &str = "https://open.feishu.cn/open-apis/bot/v2/hook/";

#[derive(Debug, Clone)]
pub struct FeishuBot {
    /// 飞书机器人 webhook token(存于 push_channel.access_token)
    pub hook_token: String,
    /// 可选加签密钥,空则不加签
    pub secret: String,
    pub http_client: HttpClient,
}

#[derive(Debug, Deserialize, Serialize)]
struct FeishuResponse {
    code: i64,
    msg: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Sign {
    timestamp: u128,
    sign: String,
}

#[async_trait]
impl MessageBot for FeishuBot {
    async fn push_markdown(&self, title: String, msg: String) -> AppResult<()> {
        let msg = markdown_for_feishu(&msg);
        let message = serde_json::json!({
            "msg_type": "interactive",
            "card": {
                "header": {
                    "title": { "tag": "plain_text", "content": title },
                },
                "elements": [
                    { "tag": "div", "text": { "tag": "lark_md", "content": msg } },
                ],
            },
        });
        let url = format!("{FEISHU_API_URL}{}", self.hook_token);
        let mut req = self.http_client.http_client.post(&url);
        if !self.secret.is_empty() {
            let sign = self.generate_sign()?;
            req = req
                .query(&[("timestamp", sign.timestamp.to_string())])
                .query(&[("sign", sign.sign)]);
        }
        let resp: FeishuResponse = req
            .json(&message)
            .send()
            .await
            .map_err(|e| Error::Message(format!("send feishu message failed: {e}")))?
            .json()
            .await
            .map_err(|e| Error::Message(format!("parse feishu response failed: {e}")))?;
        if resp.code != 0 {
            log::warn!("feishu push markdown error, err msg is {}", resp.msg);
            return Err(
                Error::Message("feishu push message response code not eq 0".to_owned()).into(),
            );
        }
        Ok(())
    }
}

impl FeishuBot {
    pub fn try_new(hook_token: String, secret: String) -> AppResult<Self> {
        let mut headers = header::HeaderMap::new();
        headers.insert("Accept-Charset", header::HeaderValue::from_static("utf8"));
        let http_client = HttpClient::try_new_with_headers(headers)?;
        Ok(Self {
            hook_token,
            secret,
            http_client,
        })
    }

    /// 飞书加签:base64(HMAC-SHA256(key="{timestamp}\n{secret}", msg=""))
    fn generate_sign(&self) -> AppResult<Sign> {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|e| Error::Message(format!("get timestamp failed: {e}")))?
            .as_millis();
        let string_to_sign = format!("{timestamp}\n{}", self.secret);
        let hmac_sha256 = calc_hmac_sha256(string_to_sign.as_bytes(), b"")?;
        let sign = BASE64_STANDARD.encode(hmac_sha256);
        Ok(Sign { timestamp, sign })
    }
}
