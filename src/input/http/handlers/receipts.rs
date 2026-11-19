use poem::{
    Request, Response, handler,
    http::StatusCode,
    web::{Data, Json, Path, Query},
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, FromQueryResult, QueryFilter,
    Statement,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    cli::Ctx,
    domain::{models::extension_data::ExtensionData, ports::VulnService},
    input::http::response::ApiError,
    output::{
        feishu_receipts::{strip_table, submitted_card},
        push::feishu_app::{FeishuApp, FeishuAppError},
        receipt_delivery,
        receipts::{self, ReceiptError},
    },
};

fn orm_error(e: sea_orm::DbErr) -> ApiError {
    log::error!("receipt database error: {e}");
    ApiError::InternalServerError("receipt database error".into())
}

#[derive(Deserialize)]
pub struct CaseRequest {
    source_type: String,
    source_id: i64,
}

/// Resolve one open_id to a display name via the per-user contact API,
/// independent of the directory cache — covers manually appended mention
/// open_ids that never appeared in any receipt or department pull.
#[handler]
pub async fn lookup_open_id<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    poem::web::Path(open_id): poem::web::Path<String>,
) -> Result<Json<Value>, ApiError> {
    let open_id = open_id.trim().to_owned();
    if !open_id.starts_with("ou_") || open_id.len() > 100 {
        return Err(ApiError::BadRequest("Open ID 无效（需 ou_ 开头）".into()));
    }
    let row = state
        .db
        .orm()
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT app_id,app_secret FROM feishu_app WHERE enabled=1 ORDER BY id LIMIT 1",
        ))
        .await
        .map_err(orm_error)?
        .ok_or_else(|| ApiError::BadRequest("没有启用的飞书应用".into()))?;
    let app_id: String = row.try_get("", "app_id").map_err(orm_error)?;
    let app_secret: String = row.try_get("", "app_secret").map_err(orm_error)?;
    let client = crate::output::push::feishu_app::FeishuApp::new(app_id, app_secret)
        .map_err(|e| ApiError::InternalServerError(format!("飞书应用初始化失败：{e}")))?;
    let (name, departments) = client
        .user_info(&open_id)
        .await
        .map_err(|e| ApiError::BadRequest(format!("查询用户失败：{e}")))?;
    if name.is_empty() {
        return Err(ApiError::BadRequest(
            "通讯录未返回该用户姓名（检查数据范围或 Open ID）".into(),
        ));
    }
    Ok(Json(
        json!({"open_id": open_id, "name": name, "departments": departments}),
    ))
}

/// Distinct people seen in responses: the source for @ mention pickers.
#[handler]
pub async fn list_people<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
) -> Result<Json<Value>, ApiError> {
    let rows = state
        .db
        .orm()
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT operator_open_id, MAX(COALESCE(NULLIF(display_name,''),operator_open_id)) AS name FROM receipt_response WHERE deleted=0 GROUP BY operator_open_id ORDER BY name",
        ))
        .await
        .map_err(orm_error)?;
    let people: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "open_id": row.try_get::<String>("", "operator_open_id").unwrap_or_default(),
                "name": row.try_get::<String>("", "name").unwrap_or_default(),
            })
        })
        .collect();
    Ok(Json(json!({"data": people})))
}

#[derive(Serialize, FromQueryResult, Debug, Default, Deserialize)]
pub struct ListCasesQuery {
    #[serde(default)]
    pub page_no: Option<i64>,
    #[serde(default)]
    pub page_size: Option<i64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub source_type: Option<String>,
}

/// Escape a user term for a SQLite LIKE … ESCAPE '\' pattern.
fn like_pattern(term: &str) -> String {
    format!(
        "%{}%",
        term.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

#[derive(Serialize, FromQueryResult)]
struct CaseListRow {
    id: i64,
    source_type: String,
    source_id: i64,
    title: String,
    status: String,
    created_at: String,
    response_count: i64,
    dims_done: i64,
    dims_total: i64,
    withdrawn_count: i64,
    unsent_teams: i64,
}
#[derive(FromQueryResult)]
struct CaseHeader {
    title: String,
    status: String,
}
#[derive(Serialize, FromQueryResult)]
struct TaskRow {
    id: i64,
    status: String,
    version: i64,
    refresh_failed: bool,
    dims_done: i64,
    dims_total: i64,
}
#[derive(Serialize, FromQueryResult)]
struct IndividualResponseRow {
    id: i64,
    task_id: i64,
    app_id: String,
    display_name: String,
    team: String,
    operator_open_id: String,
    status: String,
    details: String,
    dimension: String,
    dimension_name: String,
    owners: String,
    version: i64,
    updated_at: String,
    name_lookup_failed: bool,
}
#[derive(Serialize, FromQueryResult)]
struct AdminEventRow {
    task_id: Option<i64>,
    admin_user_id: i64,
    old_status: String,
    status: String,
    details: String,
    created_at: String,
}
#[derive(Serialize, FromQueryResult)]
struct DeliveryRow {
    task_id: i64,
    chat_id: String,
    chat_name: String,
    sent: bool,
    withdrawn: bool,
    task_version: i64,
}

#[derive(Deserialize)]
pub struct ProgressRequest {
    status: String,
    version: i64,
    details: String,
}
#[derive(Deserialize)]
pub struct ResponseUpdateRequest {
    version: i64,
    display_name: String,
    status: String,
    details: String,
    /// Optional reassignment to another confirmation dimension.
    #[serde(default)]
    dimension: Option<String>,
}
#[derive(Deserialize)]
pub struct ResponseDeleteRequest {
    version: i64,
}
#[handler]
pub async fn update_response<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    req: &Request,
    Path(id): Path<i64>,
    Json(body): Json<ResponseUpdateRequest>,
) -> Result<StatusCode, ApiError> {
    req.data::<ExtensionData>()
        .ok_or_else(|| ApiError::Unauthorized("未登录".into()))?;
    receipts::manage_response(
        &state.db.orm(),
        id,
        body.version,
        Some((
            &body.display_name,
            &body.status,
            &body.details,
            body.dimension.as_deref(),
        )),
    )
    .await
    .map_err(receipt_error)?;
    Ok(StatusCode::NO_CONTENT)
}
#[handler]
pub async fn delete_response<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    req: &Request,
    Path(id): Path<i64>,
    Json(body): Json<ResponseDeleteRequest>,
) -> Result<StatusCode, ApiError> {
    req.data::<ExtensionData>()
        .ok_or_else(|| ApiError::Unauthorized("未登录".into()))?;
    receipts::manage_response(&state.db.orm(), id, body.version, None)
        .await
        .map_err(receipt_error)?;
    Ok(StatusCode::NO_CONTENT)
}
#[handler]
pub async fn retry_response_name<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    req: &Request,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    req.data::<ExtensionData>()
        .ok_or_else(|| ApiError::Unauthorized("未登录".into()))?;
    let result=state.db.orm().execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite,"INSERT INTO receipt_name_lookup(response_id) SELECT id FROM receipt_response WHERE id=? AND display_name='' AND deleted=0 ON CONFLICT(response_id) DO UPDATE SET last_attempt=0,last_error=''",[id.into()])).await.map_err(orm_error)?;
    if result.rows_affected() == 0 {
        return Err(ApiError::BadRequest("回执不存在，或已有姓名".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
pub struct CloseRequest {
    details: String,
}

fn receipt_error(e: ReceiptError) -> ApiError {
    match e {
        ReceiptError::Invalid
        | ReceiptError::InvalidForm(_)
        | ReceiptError::Conflict
        | ReceiptError::TaskClosed
        | ReceiptError::NoPending
        | ReceiptError::NoChats
        | ReceiptError::Incomplete(_) => ApiError::BadRequest(e.to_string()),
        _ => {
            log::error!("receipt operation failed: {e}");
            ApiError::InternalServerError("确认单操作失败".into())
        }
    }
}

#[handler]
pub async fn progress<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    req: &Request,
    Path(id): Path<i64>,
    Json(body): Json<ProgressRequest>,
) -> Result<StatusCode, ApiError> {
    let admin = req
        .data::<ExtensionData>()
        .ok_or_else(|| ApiError::Unauthorized("未登录".into()))?;
    receipts::progress(
        &state.db.orm(),
        id,
        admin.user_id,
        body.version,
        &body.status,
        &body.details,
    )
    .await
    .map_err(receipt_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[handler]
pub async fn close_case<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    req: &Request,
    Path(id): Path<i64>,
    Json(body): Json<CloseRequest>,
) -> Result<StatusCode, ApiError> {
    let admin = req
        .data::<ExtensionData>()
        .ok_or_else(|| ApiError::Unauthorized("未登录".into()))?;
    receipts::close_case(&state.db.orm(), id, admin.user_id, &body.details)
        .await
        .map_err(|e| match e {
            ReceiptError::Conflict => {
                ApiError::BadRequest("确认单已关闭或不存在，请刷新后重试".into())
            }
            other => receipt_error(other),
        })?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct TriggerRequest {
    source_type: String,
    source_id: i64,
}

/// Manual actions deliberately bypass the automatic historical-date filter.
#[handler]
pub async fn trigger_case<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Json(body): Json<TriggerRequest>,
) -> Result<Json<Value>, ApiError> {
    if !matches!(body.source_type.as_str(), "vuln" | "notice") || body.source_id <= 0 {
        log::warn!(
            "发起确认参数无效：source_type={} source_id={}",
            body.source_type,
            body.source_id
        );
        return Err(ApiError::BadRequest("漏洞或公告信息无效".into()));
    }
    let db = state.db.orm();
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id FROM receipt_case WHERE source_type=? AND source_id=? AND status='open'",
            [body.source_type.clone().into(), body.source_id.into()],
        ))
        .await
        .map_err(orm_error)?;
    let (case_id, created) = if let Some(row) = existing {
        (row.try_get::<i64>("", "id").map_err(orm_error)?, false)
    } else {
        match receipts::create_case(&db, &body.source_type, body.source_id).await {
            Ok(id) => (id, true),
            Err(ReceiptError::AlreadyOpen(id)) => (id, false),
            Err(ReceiptError::Invalid) => {
                log::warn!(
                    "receipt trigger failed: source not found ({} #{})",
                    body.source_type,
                    body.source_id
                );
                return Err(ApiError::BadRequest("漏洞或公告不存在".into()));
            }
            Err(e) => return Err(receipt_error(e)),
        }
    };
    let tasks = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id FROM receipt_task WHERE case_id=? ORDER BY id",
            [case_id.into()],
        ))
        .await
        .map_err(orm_error)?;
    let mut sent_teams = Vec::new();
    let mut failures = Vec::new();
    for task in tasks {
        let id: i64 = task.try_get("", "id").map_err(orm_error)?;
        match deliver_task(&state.db, id).await {
            Ok(result) => sent_teams.push(result.0),
            Err(e) => failures.push(json!({"task_id": id, "message": e.to_string()})),
        }
    }
    Ok(Json(json!({
        "case_id": case_id,
        "created": created,
        "sent_teams": sent_teams,
        "failures": failures
    })))
}

#[handler]
pub async fn send_task<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    deliver_task(&state.db, id).await
}

/// Send an ephemeral nudge card listing non-respondents to the team's chats.
#[handler]
pub async fn remind_task<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    match crate::output::receipt_remind::send_reminder(&state.db.orm(), id).await {
        Ok(outcome) => Ok(Json(json!({
            "sent": outcome.sent.iter().map(|chat| json!({"chat_id": chat})).collect::<Vec<_>>(),
            "failed": outcome.failed.iter().map(|(chat, error)| json!({"chat_id": chat, "error": error})).collect::<Vec<_>>()
        }))),
        Err(e) => Err(receipt_error(e)),
    }
}

pub async fn deliver_task(
    db: &crate::output::db::sqlite::Db,
    id: i64,
) -> Result<Json<Value>, ApiError> {
    let orm = db.orm();
    let row = orm.query_one(Statement::from_sql_and_values(DatabaseBackend::Sqlite,"SELECT t.case_id,t.version,c.source_type,c.source_id FROM receipt_task t JOIN receipt_case c ON c.id=t.case_id WHERE t.id=? AND c.status='open'",[id.into()])).await.map_err(orm_error)?.ok_or_else(|| ApiError::BadRequest("确认任务不存在或已关闭".into()))?;
    let case_id: i64 = row.try_get("", "case_id").map_err(orm_error)?;
    let version: i64 = row.try_get("", "version").map_err(orm_error)?;
    let chats = orm.query_all(Statement::from_sql_and_values(DatabaseBackend::Sqlite,"SELECT c.chat_id,c.stacks,c.mention_open_ids,app.app_id,app.app_secret,app.tenant_key FROM feishu_chat c JOIN feishu_app app ON app.id=c.app_id AND app.enabled=1 WHERE c.enabled=1 ORDER BY c.id",[])).await.map_err(orm_error)?;
    if chats.is_empty() {
        return Err(ApiError::BadRequest("没有启用的接收群".into()));
    }
    let source_type: String = row.try_get("", "source_type").map_err(orm_error)?;
    let source_id: i64 = row.try_get("", "source_id").map_err(orm_error)?;
    let stacks_table = if source_type == "vuln" {
        "vuln_information"
    } else {
        "security_notice"
    };
    let item_stacks: String = orm
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            format!("SELECT stacks FROM {stacks_table} WHERE id=?"),
            [source_id.into()],
        ))
        .await
        .map_err(orm_error)?
        .and_then(|row| row.try_get::<String>("", "stacks").ok())
        .unwrap_or_default();
    let card = submitted_card(&orm, id).await.map_err(orm_error)?;
    // 回执固定 @ 三维度负责人；only when no owner is configured yet does the
    // chat-level mention list apply.
    let dimension_mentions: String = orm
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT owner_open_id FROM receipt_dimension WHERE enabled=1 AND owner_open_id LIKE 'ou_%' ORDER BY sort,id",
        ))
        .await
        .map_err(orm_error)?
        .iter()
        .filter_map(|row| row.try_get::<String>("", "owner_open_id").ok())
        .collect::<Vec<_>>()
        .join(",");
    let mut sent = Vec::new();
    let mut failed = Vec::new();
    for group in chats {
        let chat: String = group.try_get("", "chat_id").map_err(orm_error)?;
        let mentions: String = if dimension_mentions.is_empty() {
            group
                .try_get::<String>("", "mention_open_ids")
                .unwrap_or_default()
        } else {
            dimension_mentions.clone()
        };
        let chat_card = crate::output::feishu_receipts::with_mentions(&card, &mentions);
        let chat_stacks: String = group.try_get("", "stacks").map_err(orm_error)?;
        if !crate::output::stacks::stacks_allow(&chat_stacks, &item_stacks) {
            log::info!("chat {chat} not subscribed to stacks '{item_stacks}'; skipped");
            continue;
        }
        let app_id: String = group.try_get("", "app_id").map_err(orm_error)?;
        let tenant: String = group.try_get("", "tenant_key").map_err(orm_error)?;
        let app_secret: String = group.try_get("", "app_secret").map_err(orm_error)?;
        let client = match FeishuApp::new(app_id.clone(), app_secret) {
            Ok(client) => client,
            Err(e) => {
                log::error!("Feishu app initialization failed: {e}");
                failed.push(chat);
                continue;
            }
        };
        if let Some(existing) = receipt_delivery::find_active(&orm, id, &app_id, &tenant, &chat)
            .await
            .map_err(orm_error)?
        {
            // Verify the reused message is still live in Feishu. A live card
            // is refreshed in place and surfaced again via a quoting reply
            // instead of duplicating a second survey card in the group; a
            // recalled one is re-sent from scratch below.
            if let Some(message_id) = existing.message_id.clone() {
                let app_row = crate::output::db::entities::feishu_app::Entity::find()
                    .filter(crate::output::db::entities::feishu_app::Column::AppId.eq(&app_id))
                    .filter(crate::output::db::entities::feishu_app::Column::Enabled.eq(true))
                    .one(&orm)
                    .await
                    .map_err(orm_error)?;
                if let Some(app_row) = app_row
                    && let Ok(client) = FeishuApp::new(app_row.app_id, app_row.app_secret)
                {
                    match client.update_card(&message_id, &chat_card).await {
                        Err(FeishuAppError::ApiDetail { code: 230011, .. }) => {
                            orm.execute(Statement::from_sql_and_values(
                                DatabaseBackend::Sqlite,
                                "UPDATE receipt_delivery SET withdrawn=1 WHERE id=? AND message_id=?",
                                [existing.id.into(), message_id.into()],
                            ))
                            .await
                            .map_err(orm_error)?;
                            log::warn!("reused receipt card was recalled; re-sending: chat {chat}");
                        }
                        Err(e) => {
                            // Unknown message state: report it instead of
                            // claiming a silent success.
                            log::warn!("failed to update reused receipt card (chat {chat}): {e}");
                            failed.push(chat);
                            continue;
                        }
                        Ok(()) => {
                            log::info!(
                                "receipt card updated in place and quoted (chat {chat}, message {message_id})"
                            );
                            let reply = client
                                .reply_text(
                                    &message_id,
                                    "请在本条引用的确认卡片中提交影响确认；再次点击「提交（可覆盖变更）」可修改自己的答案。",
                                )
                                .await;
                            if let Err(e) = reply {
                                log::warn!("failed to send quoting reminder (chat {chat}): {e}");
                            }
                            sent.push(json!({"chat_id":chat,"message_id":message_id}));
                            continue;
                        }
                    }
                }
            }
        }
        let delivery =
            receipt_delivery::reserve(&orm, case_id, id, &app_id, &tenant, &chat, version)
                .await
                .map_err(orm_error)?;
        let delivery = if delivery.withdrawn {
            receipt_delivery::prepare_withdrawn_retry(&orm, delivery.id)
                .await
                .map_err(orm_error)?
        } else {
            delivery
        };
        let message_id = if let Some(message_id) = delivery.message_id {
            message_id
        } else {
            let mut result = client
                .send_card(&chat, &chat_card, &delivery.request_id)
                .await;
            // Feishu rejected the card payload (e.g. the table component);
            // retry once with the plain markdown view.
            if let Err(FeishuAppError::ApiDetail { status, code, .. }) = &result {
                log::warn!(
                    "card send rejected for chat {chat} (HTTP {status} code {code}); retrying with plain-text detail"
                );
                result = client
                    .send_card(&chat, &strip_table(&chat_card), &delivery.request_id)
                    .await;
            }
            match result {
                Ok(message_id) => {
                    receipt_delivery::mark_sent(&orm, delivery.id, &message_id)
                        .await
                        .map_err(orm_error)?;
                    message_id
                }
                Err(e) => {
                    log::error!("Feishu card delivery failed for group {chat}: {e}");
                    failed.push(chat);
                    continue;
                }
            }
        };
        sent.push(json!({"chat_id":chat,"message_id":message_id}));
    }
    if !failed.is_empty() {
        return Err(ApiError::BadRequest(format!(
            "部分群发送失败，请重试：{}",
            failed.join(", ")
        )));
    }
    Ok(Json(json!({"deliveries":sent})))
}

#[handler]
pub async fn create_case<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Json(body): Json<CaseRequest>,
) -> Result<Json<Value>, ApiError> {
    let id = receipts::create_case(&state.db.orm(), &body.source_type, body.source_id)
        .await
        .map_err(|e| match e {
            ReceiptError::AlreadyOpen(id) => {
                ApiError::BadRequest(format!("已有进行中的确认单 #{id}"))
            }
            ReceiptError::Invalid => ApiError::BadRequest("漏洞或公告不存在".into()),
            other => receipt_error(other),
        })?;
    Ok(Json(json!({"id":id,"created":true})))
}

#[handler]
pub async fn list_cases<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Query(query): Query<ListCasesQuery>,
) -> Result<Json<Value>, ApiError> {
    let page_no = query.page_no.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let mut filters = String::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if matches!(query.status.as_deref(), Some("open") | Some("closed")) {
        filters.push_str(" AND c.status=?");
        values.push(query.status.clone().unwrap().into());
    }
    if matches!(query.source_type.as_deref(), Some("vuln") | Some("notice")) {
        filters.push_str(" AND c.source_type=?");
        values.push(query.source_type.clone().unwrap().into());
    }
    let term = query.title.clone().unwrap_or_default().trim().to_owned();
    if !term.is_empty() {
        // Fuzzy title match, plus an exact case-id match for numeric terms.
        filters.push_str(" AND (c.title LIKE ? ESCAPE '\\'");
        values.push(like_pattern(&term).into());
        if let Ok(id) = term.parse::<i64>() {
            filters.push_str(" OR c.id=?");
            values.push(id.into());
        }
        filters.push(')');
    }
    let db = state.db.orm();
    let total: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            format!("SELECT COUNT(*) AS total FROM receipt_case c WHERE 1=1{filters}"),
            values.clone(),
        ))
        .await
        .map_err(orm_error)?
        .ok_or_else(|| ApiError::InternalServerError("确认单计数失败".into()))?
        .try_get("", "total")
        .map_err(orm_error)?;
    values.push(page_size.into());
    values.push(((page_no - 1) * page_size).into());
    let rows = CaseListRow::find_by_statement(Statement::from_sql_and_values(DatabaseBackend::Sqlite, format!("SELECT c.id,c.source_type,c.source_id,c.title,c.status,c.created_at, (SELECT COUNT(*) FROM receipt_response r JOIN receipt_task t ON t.id=r.task_id WHERE t.case_id=c.id AND r.deleted=0) AS response_count, (SELECT COUNT(*) FROM receipt_dimension d WHERE d.enabled=1 AND EXISTS(SELECT 1 FROM receipt_task t JOIN receipt_response r ON r.task_id=t.id AND r.deleted=0 AND r.dimension=d.slug AND r.status IN ('none','ops','iter','both','special') WHERE t.case_id=c.id)) AS dims_done, (SELECT COUNT(*) FROM receipt_dimension d WHERE d.enabled=1) AS dims_total, (SELECT COUNT(*) FROM receipt_delivery d JOIN receipt_task t ON t.id=d.task_id WHERE t.case_id=c.id AND d.withdrawn=1 AND NOT EXISTS(SELECT 1 FROM receipt_delivery active WHERE active.task_id=d.task_id AND active.app_id=d.app_id AND active.tenant_key=d.tenant_key AND active.chat_id=d.chat_id AND active.withdrawn=0 AND active.message_id IS NOT NULL)) AS withdrawn_count, (SELECT COUNT(*) FROM receipt_task t WHERE t.case_id=c.id AND NOT EXISTS(SELECT 1 FROM receipt_delivery d WHERE d.task_id=t.id AND d.withdrawn=0 AND d.message_id IS NOT NULL)) AS unsent_teams FROM receipt_case c WHERE 1=1{filters} ORDER BY c.id DESC LIMIT ? OFFSET ?"), values)).all(&db).await.map_err(orm_error)?;
    Ok(Json(json!({"data":rows, "total_count":total})))
}

#[handler]
pub async fn get_case<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let db = state.db.orm();
    let q = |sql: &str| Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, [id.into()]);
    let case = CaseHeader::find_by_statement(q("SELECT title,status FROM receipt_case WHERE id=?"))
        .one(&db)
        .await
        .map_err(orm_error)?
        .ok_or_else(|| ApiError::BadRequest("确认单不存在".into()))?;
    let tasks = TaskRow::find_by_statement(q("SELECT t.id,t.status,t.version,COALESCE(r.last_error,'')<>'' AS refresh_failed, (SELECT COUNT(*) FROM receipt_dimension d WHERE d.enabled=1 AND EXISTS(SELECT 1 FROM receipt_response r WHERE r.task_id=t.id AND r.deleted=0 AND r.dimension=d.slug AND r.status IN ('none','ops','iter','both','special'))) AS dims_done, (SELECT COUNT(*) FROM receipt_dimension d WHERE d.enabled=1) AS dims_total FROM receipt_task t LEFT JOIN receipt_card_refresh r ON r.task_id=t.id WHERE t.case_id=? ORDER BY t.id")).all(&db).await.map_err(orm_error)?;
    let responses=IndividualResponseRow::find_by_statement(q("SELECT r.id,r.task_id,r.app_id,COALESCE(NULLIF(r.display_name,''),(SELECT m.name FROM feishu_chat_member m WHERE m.open_id=r.operator_open_id AND m.name<>'' LIMIT 1),r.operator_open_id) AS display_name,r.team,r.operator_open_id,r.status,r.details,r.dimension,COALESCE((SELECT d.name FROM receipt_dimension d WHERE d.slug=r.dimension),'') AS dimension_name,r.owners,r.version,r.updated_at,EXISTS(SELECT 1 FROM receipt_name_lookup n WHERE n.response_id=r.id AND n.last_error<>'') AS name_lookup_failed FROM receipt_response r JOIN receipt_task t ON t.id=r.task_id WHERE t.case_id=? AND r.deleted=0 ORDER BY r.updated_at DESC,r.id DESC")).all(&db).await.map_err(orm_error)?;
    let admin_events = AdminEventRow::find_by_statement(q("SELECT task_id,admin_user_id,old_status,new_status AS status,details,created_at FROM receipt_admin_event WHERE case_id=? ORDER BY id")).all(&db).await.map_err(orm_error)?;
    let deliveries = DeliveryRow::find_by_statement(q("SELECT d.task_id,d.chat_id,d.message_id IS NOT NULL AS sent,d.withdrawn,d.task_version,COALESCE(g.name,'已解绑的群') AS chat_name FROM receipt_delivery d JOIN receipt_task t ON t.id=d.task_id LEFT JOIN feishu_app a ON a.app_id=d.app_id AND a.tenant_key=d.tenant_key LEFT JOIN feishu_chat g ON g.app_id=a.id AND g.tenant_key=d.tenant_key AND g.chat_id=d.chat_id WHERE d.case_id=? ORDER BY d.id")).all(&db).await.map_err(orm_error)?;
    Ok(Json(
        json!({"id":id,"title":case.title,"status":case.status,"tasks":tasks,"deliveries":deliveries,"responses":responses,"admin_events":admin_events}),
    ))
}

fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// Compact receipt outcome shown inline on vuln/notice list rows.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ReceiptSummary {
    pub case_id: i64,
    pub status: String,
    pub teams: i64,
    pub responses: i64,
    pub dims_done: i64,
    pub dims_total: i64,
}

/// Latest-case summary per source id, used by the vuln and notice list handlers.
pub async fn receipt_summaries(
    db: &sea_orm::DatabaseConnection,
    source_type: &str,
    source_ids: &[i64],
) -> Result<std::collections::HashMap<i64, ReceiptSummary>, sea_orm::DbErr> {
    let mut summaries = std::collections::HashMap::new();
    if source_ids.is_empty() {
        return Ok(summaries);
    }
    let placeholders = vec!["?"; source_ids.len()].join(",");
    let sql = format!(
        "SELECT rc.id,rc.source_id,rc.status, (SELECT COUNT(*) FROM receipt_task t WHERE t.case_id=rc.id) AS teams, (SELECT COUNT(*) FROM receipt_response r JOIN receipt_task t ON t.id=r.task_id WHERE t.case_id=rc.id AND r.deleted=0) AS responses, (SELECT COUNT(*) FROM receipt_dimension d WHERE d.enabled=1 AND EXISTS(SELECT 1 FROM receipt_task t JOIN receipt_response r ON r.task_id=t.id AND r.deleted=0 AND r.dimension=d.slug AND r.status IN ('none','ops','iter','both','special') WHERE t.case_id=rc.id)) AS dims_done, (SELECT COUNT(*) FROM receipt_dimension d WHERE d.enabled=1) AS dims_total FROM receipt_case rc WHERE rc.source_type=? AND rc.source_id IN ({placeholders})"
    );
    let mut values = vec![source_type.into()];
    values.extend(source_ids.iter().map(|id| (*id).into()));
    for row in db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            sql,
            values,
        ))
        .await?
    {
        let summary = ReceiptSummary {
            case_id: row.try_get("", "id")?,
            status: row.try_get("", "status")?,
            teams: row.try_get("", "teams")?,
            responses: row.try_get("", "responses")?,
            dims_done: row.try_get("", "dims_done")?,
            dims_total: row.try_get("", "dims_total")?,
        };
        let source_id: i64 = row.try_get("", "source_id")?;
        // A source can hold older closed cases; keep the newest one.
        match summaries.get(&source_id) {
            Some(existing) if existing.case_id >= summary.case_id => {}
            _ => {
                summaries.insert(source_id, summary);
            }
        }
    }
    Ok(summaries)
}

#[derive(Serialize, FromQueryResult)]
struct DimensionRow {
    id: i64,
    slug: String,
    name: String,
    owner_name: String,
    owner_open_id: String,
    enabled: bool,
    sort: i64,
}

#[handler]
pub async fn list_dimensions<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
) -> Result<Json<Value>, ApiError> {
    let rows = DimensionRow::find_by_statement(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT id,slug,name,owner_name,owner_open_id,enabled,sort FROM receipt_dimension ORDER BY sort,id",
    ))
    .all(&state.db.orm())
    .await
    .map_err(orm_error)?;
    Ok(Json(json!({"data": rows})))
}

#[derive(Deserialize)]
pub struct DimensionUpdateRequest {
    name: String,
    owner_name: String,
    owner_open_id: String,
    enabled: bool,
}

#[handler]
pub async fn update_dimension<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    req: &Request,
    Path(id): Path<i64>,
    Json(body): Json<DimensionUpdateRequest>,
) -> Result<StatusCode, ApiError> {
    req.data::<ExtensionData>()
        .ok_or_else(|| ApiError::Unauthorized("未登录".into()))?;
    let owner_open_id = body.owner_open_id.trim().to_owned();
    if body.name.trim().is_empty()
        || body.name.chars().count() > 40
        || body.owner_name.chars().count() > 40
        || (!owner_open_id.is_empty() && !owner_open_id.starts_with("ou_"))
    {
        return Err(ApiError::BadRequest("维度名称或负责人 Open ID 无效".into()));
    }
    let result = state
        .db
        .orm()
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE receipt_dimension SET name=?,owner_name=?,owner_open_id=?,enabled=? WHERE id=?",
            vec![
                body.name.trim().into(),
                body.owner_name.trim().into(),
                owner_open_id.into(),
                body.enabled.into(),
                id.into(),
            ],
        ))
        .await
        .map_err(orm_error)?;
    if result.rows_affected() == 0 {
        return Err(ApiError::BadRequest("维度不存在".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Export the current individual answers of one case as an Excel-friendly CSV.
#[handler]
pub async fn export_case<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    Path(id): Path<i64>,
) -> Result<poem::Response, ApiError> {
    let db = state.db.orm();
    CaseHeader::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "SELECT title,status FROM receipt_case WHERE id=?",
        [id.into()],
    ))
    .one(&db)
    .await
    .map_err(orm_error)?
    .ok_or_else(|| ApiError::BadRequest("确认单不存在".into()))?;
    let rows = db
        .query_all(Statement::from_sql_and_values(DatabaseBackend::Sqlite, "SELECT COALESCE(NULLIF((SELECT d.name FROM receipt_dimension d WHERE d.slug=r.dimension),''),'—（旧个人回执）') AS dimension,COALESCE(NULLIF(r.display_name,''),(SELECT m.name FROM feishu_chat_member m WHERE m.open_id=r.operator_open_id AND m.name<>'' LIMIT 1),r.operator_open_id) AS display_name,r.operator_open_id,r.status,r.owners,r.updated_at,t.status AS task_status FROM receipt_response r JOIN receipt_task t ON t.id=r.task_id WHERE t.case_id=? AND r.deleted=0 ORDER BY t.id,r.updated_at DESC,r.id DESC", [id.into()]))
        .await
        .map_err(orm_error)?;
    let mut csv = String::from(
        "\u{FEFF}维度,处置流程（五选一）,处置负责人,提交人,Open ID,提交时间,任务状态\r\n",
    );
    for row in rows {
        let owners_raw: String = row.try_get("", "owners").map_err(orm_error)?;
        let owners = crate::output::feishu_receipts::owners_display(&owners_raw, true);
        let cells = [
            row.try_get::<String>("", "dimension").map_err(orm_error)?,
            receipts::status_label(&row.try_get::<String>("", "status").map_err(orm_error)?)
                .to_owned(),
            owners,
            row.try_get::<String>("", "display_name")
                .map_err(orm_error)?,
            row.try_get::<String>("", "operator_open_id")
                .map_err(orm_error)?,
            row.try_get::<String>("", "updated_at").map_err(orm_error)?,
            receipts::status_label(
                &row.try_get::<String>("", "task_status")
                    .map_err(orm_error)?,
            )
            .to_owned(),
        ];
        csv.push_str(
            &cells
                .iter()
                .map(|cell| csv_field(cell))
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push_str("\r\n");
    }
    Ok(Response::builder()
        .content_type("text/csv; charset=utf-8")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"receipt-case-{id}.csv\""),
        )
        .body(csv))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_fields_quote_special_characters() {
        assert_eq!(csv_field("普通文本"), "普通文本");
        assert_eq!(csv_field("带,逗号"), "\"带,逗号\"");
        assert_eq!(csv_field("带\"引号"), "\"带\"\"引号\"");
        assert_eq!(csv_field("多行\n文本"), "\"多行\n文本\"");
    }
}
