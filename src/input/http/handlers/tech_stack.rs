use crate::{cli::Ctx, domain::ports::VulnService, input::http::response::ApiError};
use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json, Path},
};
use sea_orm::{ConnectionTrait, DatabaseBackend, FromQueryResult, Statement};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize, FromQueryResult)]
struct StackRow {
    id: i64,
    slug: String,
    name: String,
    keywords: String,
    enabled: bool,
}

#[derive(Deserialize)]
pub struct StackRequest {
    slug: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    keywords: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

fn default_enabled() -> bool {
    true
}

fn valid_slug(slug: &str) -> bool {
    let slug = slug.trim();
    !slug.is_empty()
        && slug.chars().count() <= 30
        && slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[derive(Serialize)]
pub struct LlmSettingResponse {
    pub enabled: bool,
    pub base_url: String,
    /// Always empty: write-only secret, blank on save keeps the stored one.
    pub api_key: String,
    pub model: String,
    pub timeout_secs: u64,
}

#[handler]
pub async fn get_llm_setting<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let setting = crate::output::llm_setting::load(&state.db.orm())
        .await
        .map_err(|e| {
            log::error!("llm setting load failed: {e}");
            ApiError::InternalServerError("LLM 设置读取失败".into())
        })?
        .redacted();
    let has_key = !setting.api_key.is_empty();
    Ok(Json(json!({
        "enabled": setting.enabled,
        "base_url": setting.base_url,
        "api_key": "",
        "api_key_set": has_key,
        "model": setting.model,
        "timeout_secs": setting.timeout_secs
    })))
}

#[derive(Deserialize)]
pub struct LlmSettingRequest {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub base_url: String,
    /// Blank keeps the stored key (the form never shows it).
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_llm_model")]
    pub model: String,
    #[serde(default = "default_llm_timeout")]
    pub timeout_secs: u64,
}

fn default_llm_model() -> String {
    "deepseek-chat".to_string()
}

fn default_llm_timeout() -> u64 {
    20
}

#[handler]
pub async fn update_llm_setting<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Json(body): Json<LlmSettingRequest>,
) -> Result<poem::http::StatusCode, ApiError> {
    if body.base_url.trim().len() > 500
        || body.model.trim().is_empty()
        || body.model.trim().len() > 100
    {
        return Err(ApiError::BadRequest("模型或地址无效".into()));
    }
    if body.enabled && (body.base_url.trim().is_empty() || body.api_key.trim().is_empty()) {
        // Enabling with a blank key is allowed only if a key is already stored.
        let stored = crate::output::llm_setting::load(&state.db.orm())
            .await
            .map_err(|e| {
                log::error!("llm setting load failed: {e}");
                ApiError::InternalServerError("LLM 设置读取失败".into())
            })?;
        if stored.api_key.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "启用前需填写 base_url 和 api_key".into(),
            ));
        }
    }
    let setting = crate::output::llm_setting::LlmSetting {
        enabled: body.enabled,
        base_url: body.base_url.trim().to_owned(),
        api_key: body.api_key.trim().to_owned(),
        model: body.model.trim().to_owned(),
        timeout_secs: body.timeout_secs.clamp(3, 120),
    };
    crate::output::llm_setting::save(&state.db.orm(), &setting)
        .await
        .map_err(|e| {
            log::error!("llm setting save failed: {e}");
            ApiError::InternalServerError("LLM 设置保存失败".into())
        })?;
    Ok(poem::http::StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ReclassifyRequest {
    /// empty = only never-classified, keyword = non-LLM results, all = everything.
    #[serde(default)]
    pub only: Option<String>,
}

/// Re-run stack classification (keyword pass, plus LLM when configured).
#[handler]
pub async fn reclassify<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    body: Option<Json<ReclassifyRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let only = body
        .and_then(|Json(body)| body.only)
        .unwrap_or_else(|| "empty".to_string());
    if !matches!(only.as_str(), "empty" | "keyword" | "all") {
        return Err(ApiError::BadRequest("only 取值：empty/keyword/all".into()));
    }
    let llm = match crate::output::llm_setting::client(&state.db.orm()).await {
        Some(client) => Some(client),
        None => {
            let setting = crate::output::llm_setting::load(&state.db.orm())
                .await
                .map_err(|e| {
                    log::error!("llm setting load failed: {e}");
                    ApiError::InternalServerError("LLM 设置读取失败".into())
                })?;
            if setting.enabled {
                return Err(ApiError::BadRequest(
                    "LLM 已开启但配置不完整（需要 base_url/api_key/model）".into(),
                ));
            }
            None
        }
    };
    let (processed, updated) =
        crate::output::stacks::reclassify(&state.db.orm(), llm.as_ref(), &only)
            .await
            .map_err(|e| {
                log::error!("reclassify failed: {e}");
                ApiError::InternalServerError("重新判定失败".into())
            })?;
    Ok(Json(
        json!({"processed": processed, "updated": updated, "scope": only}),
    ))
}

#[handler]
pub async fn list<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = StackRow::find_by_statement(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT id,slug,name,keywords,enabled FROM tech_stack ORDER BY id",
    ))
    .all(&state.db.orm())
    .await
    .map_err(|e| {
        log::error!("tech stack list failed: {e}");
        ApiError::InternalServerError("技术栈查询失败".into())
    })?;
    Ok(Json(json!({"data": rows})))
}

#[handler]
pub async fn create<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Json(body): Json<StackRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !valid_slug(&body.slug) {
        return Err(ApiError::BadRequest(
            "标识无效：仅限字母数字、- 和 _，最长 30 字符".into(),
        ));
    }
    if body.name.trim().is_empty() || body.name.chars().count() > 30 {
        return Err(ApiError::BadRequest("名称无效".into()));
    }
    if body.keywords.len() > 2000 {
        return Err(ApiError::BadRequest("关键词过长".into()));
    }
    let result = state
        .db
        .orm()
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO tech_stack(slug,name,keywords,enabled) VALUES(?,?,?,?)",
            [
                body.slug.trim().into(),
                body.name.trim().into(),
                body.keywords.trim().into(),
                body.enabled.into(),
            ],
        ))
        .await
        .map_err(|_| ApiError::BadRequest("该标识已存在".into()))?;
    Ok(Json(json!({"id": result.last_insert_id() as i64})))
}

#[handler]
pub async fn update<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Path(id): Path<i64>,
    Json(body): Json<StackRequest>,
) -> Result<StatusCode, ApiError> {
    if body.name.trim().is_empty() || body.name.chars().count() > 30 {
        return Err(ApiError::BadRequest("名称无效".into()));
    }
    if body.keywords.len() > 2000 {
        return Err(ApiError::BadRequest("关键词过长".into()));
    }
    let updated = state
        .db
        .orm()
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE tech_stack SET name=?,keywords=?,enabled=? WHERE id=?",
            [
                body.name.trim().into(),
                body.keywords.trim().into(),
                body.enabled.into(),
                id.into(),
            ],
        ))
        .await
        .map_err(|e| {
            log::error!("tech stack update failed: {e}");
            ApiError::InternalServerError("技术栈更新失败".into())
        })?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::BadRequest("技术栈不存在".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[handler]
pub async fn delete<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let db = state.db.orm();
    let slug: String = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT slug FROM tech_stack WHERE id=?",
            [id.into()],
        ))
        .await
        .map_err(|e| {
            log::error!("tech stack delete failed: {e}");
            ApiError::InternalServerError("技术栈删除失败".into())
        })?
        .ok_or_else(|| ApiError::BadRequest("技术栈不存在".into()))?
        .try_get("", "slug")
        .map_err(|_| ApiError::InternalServerError("技术栈删除失败".into()))?;
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "DELETE FROM tech_stack WHERE id=?",
        [id.into()],
    ))
    .await
    .map_err(|e| {
        log::error!("tech stack delete failed: {e}");
        ApiError::InternalServerError("技术栈删除失败".into())
    })?;
    // Drop the slug from every subscription so channels keep matching cleanly.
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE push_channel SET stacks=TRIM(BOTH ',' FROM REPLACE(','||stacks||',', ?, ','))",
        [format!(",{slug},").into()],
    ))
    .await
    .map_err(|e| {
        log::error!("push channel subscription cleanup failed: {e}");
        ApiError::InternalServerError("技术栈删除失败".into())
    })?;
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE feishu_chat SET stacks=TRIM(BOTH ',' FROM REPLACE(','||stacks||',', ?, ','))",
        [format!(",{slug},").into()],
    ))
    .await
    .map_err(|e| {
        log::error!("chat subscription cleanup failed: {e}");
        ApiError::InternalServerError("技术栈删除失败".into())
    })?;
    Ok(StatusCode::NO_CONTENT)
}
