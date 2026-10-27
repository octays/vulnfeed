use crate::{
    cli::Ctx,
    domain::{
        models::admin_user::{AdminUserPassword, AdminUserPasswordError},
        ports::VulnService,
    },
    input::http::response::{ApiError, ApiSuccess},
};
use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json},
};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ChangePasswordHttpRequestBody {
    pub old_password: String,
    pub new_password: String,
}

impl ChangePasswordHttpRequestBody {
    pub fn try_into_domain(
        self,
    ) -> Result<(AdminUserPassword, AdminUserPassword), ParseChangePasswordError> {
        Ok((
            AdminUserPassword::try_new(self.old_password)?,
            AdminUserPassword::try_new(self.new_password)?,
        ))
    }
}

#[derive(Debug, Clone, Error)]
pub enum ParseChangePasswordError {
    #[error(transparent)]
    InvalidPassword(#[from] AdminUserPasswordError),
}

impl From<ParseChangePasswordError> for ApiError {
    fn from(e: ParseChangePasswordError) -> Self {
        ApiError::UnprocessableEntity(format!("Password is invalid: {e}"))
    }
}

#[handler]
pub async fn change_password<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Json(body): Json<ChangePasswordHttpRequestBody>,
) -> Result<ApiSuccess<()>, ApiError> {
    let (old_password, new_password) = body.try_into_domain()?;
    state
        .vuln_service
        .change_admin_password(old_password, new_password)
        .await
        .map_err(ApiError::from)?;
    Ok(ApiSuccess::new(StatusCode::OK, ()))
}
