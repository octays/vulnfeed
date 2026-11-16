use crate::{
    cli::Ctx, domain::ports::VulnService, input::http::response::ApiError,
    output::db::entities::feishu_app,
};
use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json, Path},
};
use sea_orm::{ActiveModelTrait, EntityTrait, QueryOrder, Set};
use serde::Deserialize;
use serde_json::{Value, json};

fn db_error(e: sea_orm::DbErr) -> ApiError {
    log::error!("Feishu application database error: {e}");
    ApiError::InternalServerError("飞书应用操作失败".into())
}
#[derive(Deserialize)]
pub struct AppRequest {
    name: String,
    app_id: String,
    #[serde(default)]
    tenant_key: String,
    app_secret: Option<String>,
    #[serde(default = "enabled_default")]
    enabled: bool,
}
fn enabled_default() -> bool {
    true
}
fn validate(body: &AppRequest) -> Result<(), ApiError> {
    if [&body.name, &body.app_id]
        .iter()
        .any(|s| s.trim().is_empty() || s.len() > 200)
        || body.tenant_key.len() > 200
        || body.app_secret.as_ref().is_some_and(|s| s.len() > 512)
    {
        return Err(ApiError::BadRequest("飞书应用信息无效".into()));
    }
    Ok(())
}
#[handler]
pub async fn list<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
) -> Result<Json<Value>, ApiError> {
    let apps = feishu_app::Entity::find()
        .order_by_asc(feishu_app::Column::Id)
        .all(&state.db.orm())
        .await
        .map_err(db_error)?;
    Ok(Json(
        json!({"data":apps.into_iter().map(|a| json!({"id":a.id,"name":a.name,"app_id":a.app_id,"tenant_key":a.tenant_key,"enabled":a.enabled,"secret_configured":!a.app_secret.is_empty()})).collect::<Vec<_>>()}),
    ))
}
#[handler]
pub async fn create<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Json(body): Json<AppRequest>,
) -> Result<Json<Value>, ApiError> {
    validate(&body)?;
    let secret = body
        .app_secret
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ApiError::BadRequest("请填写 App Secret".into()))?;
    let row = feishu_app::ActiveModel {
        name: Set(body.name.trim().into()),
        app_id: Set(body.app_id.trim().into()),
        tenant_key: Set(body.tenant_key.trim().into()),
        app_secret: Set(secret),
        enabled: Set(body.enabled),
        revision: Set(1),
        ..Default::default()
    }
    .insert(&state.db.orm())
    .await
    .map_err(db_error)?;
    Ok(Json(json!({"id":row.id})))
}
#[handler]
pub async fn update<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Path(id): Path<i64>,
    Json(body): Json<AppRequest>,
) -> Result<StatusCode, ApiError> {
    validate(&body)?;
    let db = state.db.orm();
    let existing = feishu_app::Entity::find_by_id(id)
        .one(&db)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::BadRequest("飞书应用不存在".into()))?;
    if existing.app_id != body.app_id.trim() || existing.tenant_key != body.tenant_key.trim() {
        return Err(ApiError::BadRequest(
            "应用 ID 和租户不能修改，请新建应用".into(),
        ));
    }
    let revision = existing.revision + 1;
    let mut row: feishu_app::ActiveModel = existing.into();
    row.name = Set(body.name.trim().into());
    row.enabled = Set(body.enabled);
    row.revision = Set(revision);
    if let Some(secret) = body.app_secret.filter(|s| !s.is_empty()) {
        row.app_secret = Set(secret);
    }
    row.update(&db).await.map_err(db_error)?;
    Ok(StatusCode::NO_CONTENT)
}
