use crate::{
    cli::Ctx,
    domain::{
        models::push_channel::{
            CreatePushChannelRequest, PushAccessToken, PushAccessTokenError, PushChannelName,
            PushChannelNameError, PushChannelType, PushChannelTypeError, PushSecretToken,
            PushSecretTokenError,
        },
        ports::VulnService,
    },
    input::http::response::{ApiError, ApiSuccess},
    output::push::channel_bot,
};
use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json, Path},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PushChannelHttpRequestBody {
    pub name: String,
    pub channel_type: String,
    pub access_token: String,
    #[serde(default)]
    pub secret_token: String,
    pub status: bool,
}

impl PushChannelHttpRequestBody {
    pub fn try_into_domain(self) -> Result<CreatePushChannelRequest, ParsePushChannelError> {
        Ok(CreatePushChannelRequest {
            name: PushChannelName::try_new(self.name)?,
            channel_type: PushChannelType::try_new(self.channel_type)?,
            access_token: PushAccessToken::try_new(self.access_token)?,
            secret_token: PushSecretToken::try_new(self.secret_token)?,
            status: self.status,
        })
    }
}

#[derive(Debug, Clone, Error)]
pub enum ParsePushChannelError {
    #[error(transparent)]
    InvalidName(#[from] PushChannelNameError),
    #[error(transparent)]
    InvalidChannelType(#[from] PushChannelTypeError),
    #[error(transparent)]
    InvalidAccessToken(#[from] PushAccessTokenError),
    #[error(transparent)]
    InvalidSecretToken(#[from] PushSecretTokenError),
}

impl From<ParsePushChannelError> for ApiError {
    fn from(e: ParsePushChannelError) -> Self {
        let message = match e {
            ParsePushChannelError::InvalidName(e) => format!("Name is invalid: {e}"),
            ParsePushChannelError::InvalidChannelType(e) => {
                format!("Channel type is invalid: {e}")
            }
            ParsePushChannelError::InvalidAccessToken(e) => {
                format!("Access token is invalid: {e}")
            }
            ParsePushChannelError::InvalidSecretToken(e) => {
                format!("Secret token is invalid: {e}")
            }
        };
        ApiError::UnprocessableEntity(message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PushChannelIdHttpResponseData {
    pub id: i64,
}

#[handler]
pub async fn list_push_channels<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
) -> Result<ApiSuccess<Vec<crate::domain::models::push_channel::PushChannel>>, ApiError> {
    let channels = state
        .vuln_service
        .list_push_channels()
        .await
        .map_err(ApiError::from)?;
    Ok(ApiSuccess::new(StatusCode::OK, channels))
}

#[handler]
pub async fn create_push_channel<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Json(body): Json<PushChannelHttpRequestBody>,
) -> Result<ApiSuccess<PushChannelIdHttpResponseData>, ApiError> {
    let req = body.try_into_domain()?;
    let id = state
        .vuln_service
        .create_push_channel(req)
        .await
        .map_err(ApiError::from)?;
    Ok(ApiSuccess::new(
        StatusCode::OK,
        PushChannelIdHttpResponseData { id },
    ))
}

#[handler]
pub async fn update_push_channel<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Path(id): Path<i64>,
    Json(body): Json<PushChannelHttpRequestBody>,
) -> Result<ApiSuccess<()>, ApiError> {
    let req = body.try_into_domain()?;
    state
        .vuln_service
        .update_push_channel(id, req)
        .await
        .map_err(ApiError::from)?;
    Ok(ApiSuccess::new(StatusCode::OK, ()))
}

#[handler]
pub async fn delete_push_channel<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Path(id): Path<i64>,
) -> Result<ApiSuccess<()>, ApiError> {
    state
        .vuln_service
        .delete_push_channel(id)
        .await
        .map_err(ApiError::from)?;
    Ok(ApiSuccess::new(StatusCode::OK, ()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestPushChannelHttpResponseData {
    pub success: bool,
}

#[handler]
pub async fn test_push_channel<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Path(id): Path<i64>,
) -> Result<ApiSuccess<TestPushChannelHttpResponseData>, ApiError> {
    let channels = state
        .vuln_service
        .list_push_channels()
        .await
        .map_err(ApiError::from)?;
    let Some(ch) = channels.iter().find(|c| c.id == id) else {
        return Err(ApiError::BadRequest("push channel not found".to_owned()));
    };
    let bot = channel_bot(ch).map_err(ApiError::from)?;
    bot.push_markdown(
        "VulnFeed 测试消息".to_owned(),
        "**这是一条测试消息**\n\n如果你看到它,说明该推送渠道配置正确。".to_owned(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("test push failed: {e}")))?;
    Ok(ApiSuccess::new(
        StatusCode::OK,
        TestPushChannelHttpResponseData { success: true },
    ))
}
