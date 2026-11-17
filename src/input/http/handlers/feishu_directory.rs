use crate::{cli::Ctx, domain::ports::VulnService, input::http::response::ApiError};
use poem::{
    handler,
    web::{Data, Json, Path},
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value};
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use std::sync::Arc;

fn err(e: sea_orm::DbErr) -> ApiError {
    log::error!("Feishu directory error: {e}");
    ApiError::BadRequest("配置无效、重复或关联不存在".into())
}
fn q(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}
async fn insert(db: &DatabaseConnection, sql: &str, values: Vec<Value>) -> Result<i64, ApiError> {
    Ok(db
        .execute(q(sql, values))
        .await
        .map_err(err)?
        .last_insert_id() as i64)
}
fn text_valid(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 200
}
fn valid_open_id(s: &str) -> bool {
    let id = s.trim();
    id.starts_with("ou_")
        && id.len() > 3
        && id.len() <= 200
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
#[derive(Deserialize)]
pub struct GroupRequest {
    app_id: i64,
    chat_id: String,
    name: String,
    #[serde(default)]
    stacks: String,
}
#[derive(Deserialize)]
pub struct PersonRequest {
    name: String,
}
#[derive(Deserialize)]
pub struct IdentityRequest {
    app_id: i64,
    open_id: String,
}
#[handler]
pub async fn group<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Json(body): Json<GroupRequest>,
) -> Result<Json<JsonValue>, ApiError> {
    if !text_valid(&body.name) || !text_valid(&body.chat_id) || !body.chat_id.starts_with("oc_") {
        return Err(ApiError::BadRequest("群名称或 Chat ID 无效".into()));
    }
    let db = state.db.orm();
    let result=db.execute(q("INSERT INTO feishu_chat(app_id,tenant_key,chat_id,name,stacks) SELECT id,tenant_key,?,?,? FROM feishu_app WHERE id=? AND enabled=1",vec![body.chat_id.into(),body.name.trim().into(),body.stacks.trim().into(),body.app_id.into()])).await.map_err(err)?;
    if result.rows_affected() != 1 {
        return Err(ApiError::BadRequest("应用不存在或已停用".into()));
    }
    Ok(Json(json!({"id": result.last_insert_id()})))
}
#[handler]
pub async fn person<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Json(body): Json<PersonRequest>,
) -> Result<Json<JsonValue>, ApiError> {
    if !text_valid(&body.name) {
        return Err(ApiError::BadRequest("姓名无效".into()));
    }
    let id = insert(
        &state.db.orm(),
        "INSERT INTO receipt_person(name) VALUES(?)",
        vec![body.name.trim().into()],
    )
    .await?;
    Ok(Json(json!({"id":id})))
}
#[handler]
pub async fn identity<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Path(person_id): Path<i64>,
    Json(body): Json<IdentityRequest>,
) -> Result<Json<JsonValue>, ApiError> {
    if !valid_open_id(&body.open_id) {
        return Err(ApiError::BadRequest(
            "请填写该应用下以 ou_ 开头的飞书 Open ID，不能填写姓名或账号".into(),
        ));
    }
    let db = state.db.orm();
    let existing = db
        .query_one(q(
            "SELECT id FROM feishu_identity WHERE person_id=? AND app_id=?",
            vec![person_id.into(), body.app_id.into()],
        ))
        .await
        .map_err(err)?;
    if existing.is_some() {
        return Err(ApiError::BadRequest(
            "该确认人已绑定此应用，请在已有身份旁点击“修改身份”更新 Open ID".into(),
        ));
    }
    let result=db.execute(q("INSERT INTO feishu_identity(person_id,app_id,tenant_key,open_id) SELECT ?,id,tenant_key,? FROM feishu_app WHERE id=? AND enabled=1",vec![person_id.into(),body.open_id.trim().into(),body.app_id.into()])).await.map_err(err)?;
    if result.rows_affected() != 1 {
        return Err(ApiError::BadRequest("应用不存在或已停用".into()));
    }
    Ok(Json(json!({"id":result.last_insert_id()})))
}
#[derive(serde::Serialize, sea_orm::FromQueryResult)]
struct ChatRow {
    id: i64,
    app_id: i64,
    name: String,
    chat_id: String,
    stacks: String,
    mention_open_ids: String,
    enabled: bool,
}
#[derive(serde::Serialize, sea_orm::FromQueryResult)]
struct PersonRow {
    id: i64,
    name: String,
    enabled: bool,
}
#[derive(serde::Serialize, sea_orm::FromQueryResult)]
struct IdentityRow {
    id: i64,
    person_id: i64,
    app_id: i64,
    open_id: String,
    enabled: bool,
}
#[handler]
pub async fn list<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
) -> Result<Json<JsonValue>, ApiError> {
    use sea_orm::FromQueryResult;
    let db = state.db.orm();
    let chats = ChatRow::find_by_statement(q(
        "SELECT id,app_id,name,chat_id,stacks,mention_open_ids,enabled FROM feishu_chat ORDER BY id",
        vec![],
    ))
    .all(&db)
    .await
    .map_err(err)?;
    let people = PersonRow::find_by_statement(q(
        "SELECT id,name,enabled FROM receipt_person ORDER BY id",
        vec![],
    ))
    .all(&db)
    .await
    .map_err(err)?;
    let identities = IdentityRow::find_by_statement(q(
        "SELECT id,person_id,app_id,open_id,enabled FROM feishu_identity ORDER BY id",
        vec![],
    ))
    .all(&db)
    .await
    .map_err(err)?;
    Ok(Json(
        json!({"chats":chats,"people":people,"identities":identities}),
    ))
}

#[derive(Deserialize)]
pub struct UpdateRequest {
    name: Option<String>,
    enabled: Option<bool>,
    open_id: Option<String>,
    #[serde(default)]
    stacks: Option<String>,
    #[serde(default)]
    mention_open_ids: Option<String>,
}
#[handler]
pub async fn update<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Path((kind, id)): Path<(String, i64)>,
    Json(body): Json<UpdateRequest>,
) -> Result<poem::http::StatusCode, ApiError> {
    let table = match kind.as_str() {
        "people" => "receipt_person",
        "chats" => "feishu_chat",
        "identities" => "feishu_identity",
        _ => return Err(ApiError::BadRequest("无效类型".into())),
    };
    if body.name.as_ref().is_some_and(|s| !text_valid(s))
        || body.name.is_some() && !matches!(kind.as_str(), "people" | "chats")
        || body
            .open_id
            .as_ref()
            .is_some_and(|id| kind != "identities" || !valid_open_id(id))
        || body.name.is_none()
            && body.enabled.is_none()
            && body.open_id.is_none()
            && body.stacks.is_none()
            && body.mention_open_ids.is_none()
    {
        return Err(ApiError::BadRequest("配置无效".into()));
    }
    let db = state.db.orm();
    if let Some(open_id) = body.open_id {
        let result = db
            .execute(q(
                "UPDATE feishu_identity SET open_id=? WHERE id=?",
                vec![open_id.trim().into(), id.into()],
            ))
            .await
            .map_err(|e| {
                log::warn!("failed to update Feishu identity: {e}");
                ApiError::BadRequest("该 Open ID 已绑定其他确认人或身份不存在".into())
            })?;
        if result.rows_affected() == 0 {
            return Err(ApiError::BadRequest("身份不存在".into()));
        }
    }
    if let Some(mentions) = body.mention_open_ids {
        if kind != "chats" {
            return Err(ApiError::BadRequest("只有群可配置@人员".into()));
        }
        let normalized = mentions
            .split(',')
            .map(str::trim)
            .filter(|id| id.starts_with("ou_"))
            .collect::<Vec<_>>()
            .join(",");
        let result = db
            .execute(q(
                "UPDATE feishu_chat SET mention_open_ids=? WHERE id=?",
                vec![normalized.into(), id.into()],
            ))
            .await
            .map_err(err)?;
        if result.rows_affected() == 0 {
            return Err(ApiError::BadRequest("记录不存在".into()));
        }
    }
    if let Some(stacks) = body.stacks {
        let normalized = stacks
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        if kind != "chats" {
            return Err(ApiError::BadRequest("只有群可配置订阅".into()));
        }
        let result = db
            .execute(q(
                "UPDATE feishu_chat SET stacks=? WHERE id=?",
                vec![normalized.into(), id.into()],
            ))
            .await
            .map_err(err)?;
        if result.rows_affected() == 0 {
            return Err(ApiError::BadRequest("记录不存在".into()));
        }
    }
    if let Some(name) = body.name {
        let result = db
            .execute(q(
                &format!("UPDATE {table} SET name=? WHERE id=?"),
                vec![name.trim().into(), id.into()],
            ))
            .await
            .map_err(err)?;
        if result.rows_affected() == 0 {
            return Err(ApiError::BadRequest("记录不存在".into()));
        }
    }
    if let Some(enabled) = body.enabled {
        let result = db
            .execute(q(
                &format!("UPDATE {table} SET enabled=? WHERE id=?"),
                vec![enabled.into(), id.into()],
            ))
            .await
            .map_err(err)?;
        if result.rows_affected() == 0 {
            return Err(ApiError::BadRequest("记录不存在".into()));
        }
    }
    Ok(poem::http::StatusCode::NO_CONTENT)
}
/// Run one member-cache sync pass now instead of waiting for the periodic loop.
#[handler]
pub async fn sync_members<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
) -> Result<Json<JsonValue>, ApiError> {
    let summary = crate::output::receipt_member_sync::process(&state.db)
        .await
        .map_err(|e| {
            log::error!("member sync failed: {e}");
            ApiError::InternalServerError("群成员同步失败".into())
        })?;
    Ok(Json(
        json!({"synced": summary.synced, "failed": summary.failed}),
    ))
}

/// Cap the directory listing so a huge tenant cannot stall the request.
const DIRECTORY_MEMBERS_CAP: usize = 2000;
const DIRECTORY_MEMBERS_TTL_SECS: u64 = 3600;

type MemberCache = tokio::sync::RwLock<Option<(std::time::Instant, Arc<Vec<JsonValue>>)>>;
static MEMBER_CACHE: std::sync::LazyLock<MemberCache> =
    std::sync::LazyLock::new(|| tokio::sync::RwLock::new(None));

/// All tenant members visible to the app as {open_id, name, department},
/// pulled per-department in parallel and cached in-process for an hour.
/// Powers the owner/escalation pickers in the admin UI; requires the app's
/// contact data scope to cover the members.
#[handler]
pub async fn list_members<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
) -> Result<Json<JsonValue>, ApiError> {
    if let Some((built_at, cached)) = MEMBER_CACHE.read().await.as_ref()
        && built_at.elapsed().as_secs() < DIRECTORY_MEMBERS_TTL_SECS
    {
        return Ok(Json(json!({"data": cached.clone(), "cached": true})));
    }
    let row = state
        .db
        .orm()
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT app_id,app_secret FROM feishu_app WHERE enabled=1 ORDER BY id LIMIT 1",
        ))
        .await
        .map_err(err)?
        .ok_or_else(|| ApiError::BadRequest("没有启用的飞书应用".into()))?;
    let app_id: String = row.try_get("", "app_id").map_err(err)?;
    let app_secret: String = row.try_get("", "app_secret").map_err(err)?;
    let client = crate::output::push::feishu_app::FeishuApp::new(app_id, app_secret)
        .map_err(|e| ApiError::InternalServerError(format!("飞书应用初始化失败：{e}")))?;
    let departments = client
        .visible_departments()
        .await
        .map_err(|e| ApiError::InternalServerError(format!("读取部门失败：{e}")))?;
    let mut seen = std::collections::HashMap::<String, (String, String)>::new();
    let mut jobs = tokio::task::JoinSet::new();
    for (department_id, department_name) in departments {
        let client = client.clone();
        jobs.spawn(async move {
            (
                department_name,
                client.department_members(&department_id).await,
            )
        });
    }
    while let Some(job) = jobs.join_next().await
        && let Ok((department, result)) = job
    {
        match result {
            Ok(members) => {
                for (open_id, name) in members {
                    seen.entry(open_id).or_insert_with(|| (name, department.clone()));
                }
            }
            Err(e) => {
                log::warn!("directory member fetch failed for {department}: {e}");
            }
        }
    }
    let mut members: Vec<(String, (String, String))> = seen.into_iter().collect();
    members.sort_by(|a, b| a.1 .0.cmp(&b.1 .0).then(a.0.cmp(&b.0)));
    members.truncate(DIRECTORY_MEMBERS_CAP);
    let data: Vec<JsonValue> = members
        .into_iter()
        .map(|(open_id, (name, department))| {
            json!({"open_id": open_id, "name": name, "department": department})
        })
        .collect();
    let data = Arc::new(data);
    *MEMBER_CACHE.write().await = Some((std::time::Instant::now(), data.clone()));
    Ok(Json(json!({"data": data, "cached": false})))
}
