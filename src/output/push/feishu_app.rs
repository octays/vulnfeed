//! Feishu application API client for sending interactive cards.
use std::{
    collections::HashMap,
    sync::LazyLock,
    time::{Duration, Instant},
};

use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FeishuAppError {
    #[error("Feishu request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Feishu API returned error code {0}")]
    Api(i64),
    #[error("Feishu API returned an incomplete response")]
    InvalidResponse,
    #[error("Feishu API returned HTTP {status}, code {code}: {message}")]
    ApiDetail {
        status: u16,
        code: i64,
        message: String,
    },
    #[error("Feishu API returned HTTP {0} without a valid error response")]
    HttpStatus(u16),
}

// Do not derive Debug: the app secret must not appear in logs.
pub struct FeishuApp {
    client: Client,
    app_id: String,
    app_secret: String,
}

/// Process-wide tenant-token cache: every FeishuApp instance shares one token
/// per app instead of fetching its own. Feishu tokens live 2h; entries are
/// dropped 5 minutes early and refreshed on the next request.
static TOKEN_CACHE: LazyLock<tokio::sync::Mutex<HashMap<String, (String, Instant)>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));
const TOKEN_EARLY_REFRESH: Duration = Duration::from_secs(5 * 60);
const TOKEN_TTL: Duration = Duration::from_secs(2 * 60 * 60);

#[derive(Deserialize)]
struct TokenResponse {
    code: i64,
    tenant_access_token: Option<String>,
}

#[derive(Deserialize)]
struct MessageResponse {
    code: i64,
    #[serde(default)]
    msg: String,
    data: Option<MessageData>,
}

#[derive(Deserialize)]
struct MessageData {
    message_id: String,
}

#[derive(Deserialize)]
struct ApiResponse {
    code: i64,
    #[serde(default)]
    msg: String,
}

#[derive(Deserialize)]
struct UserResponse {
    code: i64,
    #[serde(default)]
    msg: String,
    data: Option<UserData>,
}
#[derive(Deserialize)]
struct UserData {
    user: Option<FeishuUser>,
}
#[derive(Deserialize)]
struct MembersResponse {
    code: i64,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Option<MembersData>,
}
#[derive(Deserialize)]
struct MembersData {
    #[serde(default)]
    items: Vec<ChatMember>,
    #[serde(default)]
    has_more: Option<bool>,
    #[serde(default)]
    page_token: Option<String>,
}
#[derive(Deserialize)]
struct ChatMember {
    member_id: String,
    #[serde(default)]
    name: Option<String>,
}
#[derive(Deserialize)]
struct DepartmentsResponse {
    code: i64,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Option<DepartmentsData>,
}
#[derive(Deserialize)]
struct DepartmentsData {
    #[serde(default)]
    items: Vec<FeishuDepartmentNode>,
    #[serde(default)]
    has_more: Option<bool>,
    #[serde(default)]
    page_token: Option<String>,
}
#[derive(Deserialize)]
struct FeishuDepartmentNode {
    #[serde(default)]
    department_id: String,
    #[serde(default)]
    name: Option<String>,
}
#[derive(Deserialize)]
struct DeptUsersResponse {
    code: i64,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Option<DeptUsersData>,
}
#[derive(Deserialize)]
struct DeptUsersData {
    #[serde(default)]
    items: Vec<DeptUserNode>,
    #[serde(default)]
    has_more: Option<bool>,
    #[serde(default)]
    page_token: Option<String>,
}
#[derive(Deserialize)]
struct DeptUserNode {
    #[serde(default)]
    open_id: String,
}
#[derive(Deserialize)]
struct FeishuUser {
    name: Option<String>,
    #[serde(default)]
    department_ids: Option<Vec<String>>,
}
#[derive(Deserialize)]
struct DepartmentResponse {
    code: i64,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Option<DepartmentData>,
}
#[derive(Deserialize)]
struct DepartmentData {
    #[serde(default)]
    department: Option<FeishuDepartment>,
}
#[derive(Deserialize)]
struct FeishuDepartment {
    name: Option<String>,
}

impl FeishuApp {
    pub fn new(app_id: String, app_secret: String) -> Result<Self, FeishuAppError> {
        if app_id.trim().is_empty() || app_secret.trim().is_empty() {
            return Err(FeishuAppError::InvalidResponse);
        }
        Ok(Self {
            // connect_timeout lets the connector give up on a black-holed CDN
            // node and fall through to the next resolved address within the
            // overall 10s budget.
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .connect_timeout(Duration::from_secs(4))
                .build()?,
            app_id,
            app_secret,
        })
    }

    async fn token(&self) -> Result<String, FeishuAppError> {
        let mut cache = TOKEN_CACHE.lock().await;
        if let Some((token, at)) = cache.get(&self.app_id)
            && at.elapsed() < TOKEN_TTL - TOKEN_EARLY_REFRESH
        {
            return Ok(token.clone());
        }
        let token: TokenResponse = self
            .client
            .post("https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal")
            .json(&json!({"app_id": self.app_id, "app_secret": self.app_secret}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if token.code != 0 {
            return Err(FeishuAppError::Api(token.code));
        }
        let token = token
            .tenant_access_token
            .filter(|s| !s.is_empty())
            .ok_or(FeishuAppError::InvalidResponse)?;
        cache.insert(self.app_id.clone(), (token.clone(), Instant::now()));
        Ok(token)
    }

    /// Look up a submitted user's display name using the app's directory permissions.
    pub async fn user_name(&self, open_id: &str) -> Result<String, FeishuAppError> {
        if !open_id.starts_with("ou_")
            || open_id.len() <= 3
            || !open_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(FeishuAppError::InvalidResponse);
        }
        let token = self.token().await?;
        let response = self
            .client
            .get(format!(
                "https://open.feishu.cn/open-apis/contact/v3/users/{open_id}"
            ))
            .query(&[("user_id_type", "open_id")])
            .bearer_auth(token)
            .send()
            .await?;
        let status = response.status();
        let result: UserResponse = response
            .json()
            .await
            .map_err(|_| FeishuAppError::HttpStatus(status.as_u16()))?;
        if !status.is_success() || result.code != 0 {
            return Err(FeishuAppError::ApiDetail {
                status: status.as_u16(),
                code: result.code,
                message: result
                    .msg
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(240)
                    .collect(),
            });
        }
        result
            .data
            .and_then(|d| d.user)
            .and_then(|u| u.name)
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty() && n.chars().count() <= 200)
            .ok_or(FeishuAppError::InvalidResponse)
    }

    /// The user's display name plus department ids (first is used as 团队).
    pub async fn user_info(&self, open_id: &str) -> Result<(String, Vec<String>), FeishuAppError> {
        if !open_id.starts_with("ou_")
            || !open_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(FeishuAppError::InvalidResponse);
        }
        let token = self.token().await?;
        let response = self
            .client
            .get(format!(
                "https://open.feishu.cn/open-apis/contact/v3/users/{open_id}"
            ))
            .query(&[
                ("user_id_type", "open_id"),
                ("department_id_type", "department_id"),
            ])
            .bearer_auth(token)
            .send()
            .await?;
        let status = response.status();
        let result: UserResponse = response
            .json()
            .await
            .map_err(|_| FeishuAppError::HttpStatus(status.as_u16()))?;
        if !status.is_success() || result.code != 0 {
            return Err(FeishuAppError::ApiDetail {
                status: status.as_u16(),
                code: result.code,
                message: result
                    .msg
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(240)
                    .collect(),
            });
        }
        let Some(user) = result.data.and_then(|d| d.user) else {
            return Err(FeishuAppError::InvalidResponse);
        };
        let name = user
            .name
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty() && n.chars().count() <= 200)
            .unwrap_or_default();
        let departments = user
            .department_ids
            .unwrap_or_default()
            .into_iter()
            .filter(|id| !id.trim().is_empty())
            .collect::<Vec<_>>();
        // A user object without any visible name/department fields means the
        // tenant token lacks the contact scopes; surface it instead of a
        // silent success that would retry forever without logging.
        if name.is_empty() && departments.is_empty() {
            return Err(FeishuAppError::ApiDetail {
                status: status.as_u16(),
                code: 0,
                message: "user fields not visible to this app token (check contact scopes granted to the app, not a user token)".to_owned(),
            });
        }
        Ok((name, departments))
    }

    pub async fn department_name(&self, department_id: &str) -> Result<String, FeishuAppError> {
        if department_id.trim().is_empty() || department_id.len() > 100 {
            return Err(FeishuAppError::InvalidResponse);
        }
        let token = self.token().await?;
        let response = self
            .client
            .get(format!(
                "https://open.feishu.cn/open-apis/contact/v3/departments/{department_id}"
            ))
            .query(&[("department_id_type", "department_id")])
            .bearer_auth(token)
            .send()
            .await?;
        let status = response.status();
        let result: DepartmentResponse = response
            .json()
            .await
            .map_err(|_| FeishuAppError::HttpStatus(status.as_u16()))?;
        if !status.is_success() || result.code != 0 {
            return Err(FeishuAppError::ApiDetail {
                status: status.as_u16(),
                code: result.code,
                message: result
                    .msg
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(240)
                    .collect(),
            });
        }
        result
            .data
            .and_then(|d| d.department)
            .and_then(|dept| dept.name)
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty() && n.chars().count() <= 100)
            .ok_or(FeishuAppError::InvalidResponse)
    }

    pub async fn update_card(&self, message_id: &str, card: &Value) -> Result<(), FeishuAppError> {
        if !message_id.starts_with("om_")
            || !message_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(FeishuAppError::InvalidResponse);
        }
        let token = self.token().await?;
        let response = self
            .client
            .patch(format!(
                "https://open.feishu.cn/open-apis/im/v1/messages/{message_id}"
            ))
            .bearer_auth(token)
            .json(&json!({"content":card.to_string()}))
            .send()
            .await?;
        let status = response.status();
        let result: ApiResponse = match response.json().await {
            Ok(body) => body,
            Err(_) => return Err(FeishuAppError::HttpStatus(status.as_u16())),
        };
        if !status.is_success() || result.code != 0 {
            let message = result
                .msg
                .chars()
                .filter(|c| !c.is_control())
                .take(240)
                .collect();
            return Err(FeishuAppError::ApiDetail {
                status: status.as_u16(),
                code: result.code,
                message,
            });
        }
        Ok(())
    }

    /// Reply to (quote) an existing message, e.g. to surface a buried card.
    pub async fn reply_text(&self, message_id: &str, text: &str) -> Result<String, FeishuAppError> {
        if !message_id.starts_with("om_")
            || !message_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            || text.trim().is_empty()
        {
            return Err(FeishuAppError::InvalidResponse);
        }
        let token = self.token().await?;
        let response = self
            .client
            .post(format!(
                "https://open.feishu.cn/open-apis/im/v1/messages/{message_id}/reply"
            ))
            .bearer_auth(token)
            .json(&json!({"msg_type": "text", "content": json!({"text": text}).to_string()}))
            .send()
            .await?;
        let status = response.status();
        let result: ApiResponse = match response.json().await {
            Ok(body) => body,
            Err(_) => return Err(FeishuAppError::HttpStatus(status.as_u16())),
        };
        if !status.is_success() || result.code != 0 {
            let message = result
                .msg
                .chars()
                .filter(|c| !c.is_control())
                .take(240)
                .collect();
            return Err(FeishuAppError::ApiDetail {
                status: status.as_u16(),
                code: result.code,
                message,
            });
        }
        Ok(message_id.to_owned())
    }

    pub async fn send_card(
        &self,
        chat_id: &str,
        card: &Value,
        request_id: &str,
    ) -> Result<String, FeishuAppError> {
        if chat_id.trim().is_empty() || request_id.trim().is_empty() {
            return Err(FeishuAppError::InvalidResponse);
        }
        let token = self.token().await?;
        let http = self.client.post("https://open.feishu.cn/open-apis/im/v1/messages")
            .query(&[("receive_id_type", "chat_id")])
            .bearer_auth(token)
            .json(&json!({"receive_id": chat_id, "msg_type": "interactive", "content": card.to_string(), "uuid": request_id}))
            .send().await?;
        let status = http.status();
        let body = http.text().await.unwrap_or_default();
        let Ok(response) = serde_json::from_str::<MessageResponse>(&body) else {
            return Err(FeishuAppError::ApiDetail {
                status: status.as_u16(),
                code: -1,
                message: format!("unparseable response: {}", body.chars().take(240).collect::<String>()),
            });
        };
        if !status.is_success() || response.code != 0 {
            let message = response
                .msg
                .chars()
                .filter(|c| !c.is_control())
                .take(240)
                .collect::<String>();
            return Err(FeishuAppError::ApiDetail {
                status: status.as_u16(),
                code: response.code,
                message,
            });
        }
        response
            .data
            .map(|d| d.message_id)
            .filter(|id| !id.is_empty())
            .ok_or(FeishuAppError::InvalidResponse)
    }

    /// All departments visible to the app, as (department_id, name), via the
    /// department tree (works without the user-object organization scope).
    pub async fn visible_departments(&self) -> Result<Vec<(String, String)>, FeishuAppError> {
        let token = self.token().await?;
        let mut departments = Vec::new();
        let mut page_token: Option<String> = None;
        for _ in 0..10 {
            let mut request = self
                .client
                .get("https://open.feishu.cn/open-apis/contact/v3/departments/0/children")
                .query(&[
                    ("department_id_type", "department_id"),
                    ("fetch_child", "true"),
                    ("page_size", "50"),
                ]);
            if let Some(value) = page_token.as_deref().filter(|v| !v.is_empty()) {
                request = request.query(&[("page_token", value)]);
            }
            let response = request.bearer_auth(&token).send().await?;
            let status = response.status();
            let result: DepartmentsResponse = response
                .json()
                .await
                .map_err(|_| FeishuAppError::HttpStatus(status.as_u16()))?;
            if !status.is_success() || result.code != 0 {
                let message = result
                    .msg
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(240)
                    .collect();
                return Err(FeishuAppError::ApiDetail {
                    status: status.as_u16(),
                    code: result.code,
                    message,
                });
            }
            let Some(data) = result.data else { break };
            for node in data.items {
                if !node.department_id.is_empty() {
                    departments.push((node.department_id, node.name.unwrap_or_default()));
                }
            }
            match data.has_more {
                Some(true) => page_token = data.page_token.filter(|t| !t.is_empty()),
                _ => break,
            }
        }
        Ok(departments)
    }

    /// The open_ids of a department's direct members (paginated).
    pub async fn department_member_ids(
        &self,
        department_id: &str,
    ) -> Result<Vec<String>, FeishuAppError> {
        if department_id.trim().is_empty() || department_id.len() > 100 {
            return Err(FeishuAppError::InvalidResponse);
        }
        let token = self.token().await?;
        let mut members = Vec::new();
        let mut page_token: Option<String> = None;
        for _ in 0..10 {
            let mut request = self
                .client
                .get("https://open.feishu.cn/open-apis/contact/v3/users/find_by_department")
                .query(&[
                    ("department_id", department_id),
                    ("department_id_type", "department_id"),
                    ("user_id_type", "open_id"),
                    ("page_size", "50"),
                ]);
            if let Some(value) = page_token.as_deref().filter(|v| !v.is_empty()) {
                request = request.query(&[("page_token", value)]);
            }
            let response = request.bearer_auth(&token).send().await?;
            let status = response.status();
            let result: DeptUsersResponse = response
                .json()
                .await
                .map_err(|_| FeishuAppError::HttpStatus(status.as_u16()))?;
            if !status.is_success() || result.code != 0 {
                let message = result
                    .msg
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(240)
                    .collect();
                return Err(FeishuAppError::ApiDetail {
                    status: status.as_u16(),
                    code: result.code,
                    message,
                });
            }
            let Some(data) = result.data else { break };
            for node in data.items {
                if node.open_id.starts_with("ou_") {
                    members.push(node.open_id);
                }
            }
            match data.has_more {
                Some(true) => page_token = data.page_token.filter(|t| !t.is_empty()),
                _ => break,
            }
        }
        Ok(members)
    }

    /// Snapshot of a chat's current human members as (open_id, display name).
    pub async fn chat_members(
        &self,
        chat_id: &str,
    ) -> Result<Vec<(String, String)>, FeishuAppError> {
        if !chat_id.starts_with("oc_")
            || !chat_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(FeishuAppError::InvalidResponse);
        }
        let token = self.token().await?;
        let mut members = Vec::new();
        let mut page_token: Option<String> = None;
        // Hard cap of 10 pages (100 per page) keeps a pathological loop bounded.
        for _ in 0..10 {
            let mut request = self
                .client
                .get(format!(
                    "https://open.feishu.cn/open-apis/im/v1/chats/{chat_id}/members"
                ))
                .query(&[("page_size", "100"), ("member_id_type", "open_id")]);
            if let Some(value) = page_token.as_deref().filter(|v| !v.is_empty()) {
                request = request.query(&[("page_token", value)]);
            }
            let response = request.bearer_auth(&token).send().await?;
            let status = response.status();
            let result: MembersResponse = response
                .json()
                .await
                .map_err(|_| FeishuAppError::HttpStatus(status.as_u16()))?;
            if !status.is_success() || result.code != 0 {
                return Err(FeishuAppError::ApiDetail {
                    status: status.as_u16(),
                    code: result.code,
                    message: result
                        .msg
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(240)
                        .collect(),
                });
            }
            let Some(data) = result.data else {
                break;
            };
            for member in data.items {
                // Only ou_ identities can match card operators; skip bots and apps.
                if member.member_id.starts_with("ou_") {
                    members.push((
                        member.member_id,
                        member.name.unwrap_or_default().trim().to_owned(),
                    ));
                }
            }
            match data.has_more {
                Some(true) => page_token = data.page_token.filter(|t| !t.is_empty()),
                _ => break,
            }
        }
        Ok(members)
    }
}
