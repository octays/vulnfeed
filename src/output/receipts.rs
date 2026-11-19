//! Transactional core for receipt authorization and responses.
use sea_orm::DatabaseConnection;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReceiptError {
    #[error("database error: {0}")]
    Orm(#[from] sea_orm::DbErr),
    #[error("提交内容无效：请选择维度与五选一选项，并按选项规则选择处置负责人")]
    Invalid,
    /// Field-level form validation failure; carries the reason so the card
    /// toast can name the actual problem instead of the generic text above.
    #[error("提交内容无效：{0}")]
    InvalidForm(String),
    #[error("已有进行中的确认单 #{0}，所选团队与原确认单不同；请查看原确认单")]
    AlreadyOpen(i64),
    #[error("receipt task not found or not authorized")]
    Unauthorized,
    #[error("确认任务不存在或已关闭")]
    TaskClosed,
    #[error("没有可催办对象：所有维度均已回执")]
    NoPending,
    #[error("该团队没有启用的飞书应用群")]
    NoChats,
    #[error("确认单状态已变化，请刷新后重试")]
    Conflict,
    #[error("event identifier already used for a different response")]
    Duplicate,
    #[error("管理员未开放回执自删除")]
    DeletionDisabled,
    #[error("没有可删除的回执")]
    NoResponse,
    #[error("尚有维度未完成回执：{0}")]
    Incomplete(String),
}

/// Receipt options (SOP v24 五选一): slug → card/CSV label.
pub const RECEIPT_STATUSES: [(&str, &str); 5] = [
    ("none", "无影响，无需处置流程"),
    ("ops", "已规划运维处理"),
    ("iter", "已规划迭代处理"),
    ("both", "已规划运维 + 迭代处理"),
    ("special", "已规划专项处理（含客户侧）"),
];

/// A completed dimension answer: any of the five options (无影响 or a planned
/// handling with owners recorded).
pub const COMPLETED_STATUSES: [&str; 5] = ["none", "ops", "iter", "both", "special"];

pub fn is_receipt_status(status: &str) -> bool {
    RECEIPT_STATUSES.iter().any(|(slug, _)| *slug == status)
}

/// Label for a response/task status; legacy three-state values stay readable.
pub fn status_label(status: &str) -> &'static str {
    if let Some((_, label)) = RECEIPT_STATUSES.iter().find(|(slug, _)| *slug == status) {
        return label;
    }
    match status {
        "unaffected" => "无影响",
        "affected" => "有影响",
        "investigating" => "待排查",
        "in_progress" => "部分确认",
        "done" => "三维度齐备",
        "awaiting_verification" => "待验证",
        "verified" => "已验证",
        _ => "待确认",
    }
}

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

/// Create a case only for a newly inserted source record published within the
/// freshness window (local date >= yesterday). Vulns and notices have separate
/// opt-in switches (`auto_receipts` / `auto_receipts_notice`).
pub async fn auto_create(
    db: &DatabaseConnection,
    source_type: &str,
    source_id: i64,
    date: &str,
) -> Result<Option<i64>, ReceiptError> {
    let flag_sql = match source_type {
        "notice" => "SELECT auto_receipts_notice AS enabled FROM sync_task ORDER BY id LIMIT 1",
        _ => "SELECT auto_receipts AS enabled FROM sync_task ORDER BY id LIMIT 1",
    };
    let enabled = db
        .query_one(statement(flag_sql, vec![]))
        .await?
        .map(|row| row.try_get::<bool>("", "enabled"))
        .transpose()?;
    if enabled != Some(true) {
        log::debug!("auto receipts disabled on the sync task; skip {source_type} #{source_id}");
        return Ok(None);
    }
    // Sources mostly publish during the local evening while scraping often
    // lands after local midnight; compare against yesterday so day-old items
    // still pass instead of being dropped by a calendar-day check.
    let cutoff = chrono::Local::now().date_naive() - chrono::Duration::days(1);
    if !crate::utils::push_date::eligible_for_push(date, false, cutoff) {
        log::info!(
            "auto receipt skipped for {source_type} #{source_id}: publication date {date:?} outside the freshness window"
        );
        return Ok(None);
    }
    let exists: bool = db.query_one(statement("SELECT EXISTS(SELECT 1 FROM receipt_case WHERE source_type=? AND source_id=?) AS found",vec![source_type.into(),source_id.into()])).await?.ok_or(ReceiptError::Invalid)?.try_get("", "found")?;
    if exists {
        return Ok(None);
    }
    match create_case_inner(db, source_type, source_id, true).await {
        Ok((id, true)) => Ok(Some(id)),
        Ok((_, false)) | Err(ReceiptError::AlreadyOpen(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

pub async fn create_case(
    db: &DatabaseConnection,
    source_type: &str,
    source_id: i64,
) -> Result<i64, ReceiptError> {
    create_case_inner(db, source_type, source_id, false)
        .await
        .map(|(id, _)| id)
}

async fn create_case_inner(
    db: &DatabaseConnection,
    source_type: &str,
    source_id: i64,
    automatic: bool,
) -> Result<(i64, bool), ReceiptError> {
    if !matches!(source_type, "vuln" | "notice") {
        return Err(ReceiptError::Invalid);
    }
    let tx = crate::output::db::sqlite::begin_write(db).await?;
    if let Some(row) = tx
        .query_one(statement(
            "SELECT id FROM receipt_case WHERE source_type=? AND source_id=? AND status='open'",
            vec![source_type.into(), source_id.into()],
        ))
        .await?
    {
        let id: i64 = row.try_get("", "id")?;
        tx.commit().await?;
        return Ok((id, false));
    }
    let source_sql = if source_type == "vuln" {
        "SELECT title FROM vuln_information WHERE id=?"
    } else {
        "SELECT title FROM security_notice WHERE id=?"
    };
    let title: String = tx
        .query_one(statement(source_sql, vec![source_id.into()]))
        .await?
        .ok_or(ReceiptError::Invalid)?
        .try_get("", "title")?;
    let id = tx
        .execute(statement(
            "INSERT INTO receipt_case(source_type,source_id,title) VALUES(?,?,?)",
            vec![source_type.into(), source_id.into(), title.into()],
        ))
        .await?
        .last_insert_id() as i64;
    let task = tx
        .execute(statement(
            // Seed the reminder clock so a fresh case gets its first automatic
            // nudge only after a full 24h window, not on the next scan.
            "INSERT INTO receipt_task(case_id,last_reminded_at) VALUES(?,CAST(strftime('%s','now') AS INTEGER))",
            vec![id.into()],
        ))
        .await?;
    if automatic {
        tx.execute(statement(
            "INSERT INTO receipt_auto_delivery(task_id) VALUES(?)",
            vec![(task.last_insert_id() as i64).into()],
        ))
        .await?;
    }
    tx.commit().await?;
    Ok((id, true))
}

/// Field-level checks on a card answer (dimension picked, five-option valid,
/// owner count matching the option) so the toast names the actual problem.
/// none needs no owner; ops/iter pick exactly one; both/special multi-select.
fn validate_answer(dimension: &str, status: &str, owners: &[String]) -> Result<(), ReceiptError> {
    if dimension.trim().is_empty() {
        return Err(ReceiptError::InvalidForm("请选择回执维度".into()));
    }
    if !is_receipt_status(status) {
        return Err(ReceiptError::InvalidForm(
            "五选一选项无效或为旧版选项，请等待卡片刷新后重新提交".into(),
        ));
    }
    if owners.len() > 5
        || owners
            .iter()
            .any(|owner| !owner.starts_with("ou_") || owner.len() > 64)
    {
        return Err(ReceiptError::InvalidForm(
            "处置负责人选择无效（需为飞书通讯录用户，最多 5 名），请重新选择".into(),
        ));
    }
    match (status, owners.len()) {
        ("none", 0) | ("ops" | "iter", 1) | ("both" | "special", 1..=5) => Ok(()),
        ("none", _) => Err(ReceiptError::InvalidForm(
            "「无影响，无需处置流程」不需要选择处置负责人".into(),
        )),
        ("ops" | "iter", 0) => Err(ReceiptError::InvalidForm(format!(
            "「{}」需选择 1 名处置负责人",
            status_label(status)
        ))),
        ("ops" | "iter", _) => Err(ReceiptError::InvalidForm(format!(
            "「{}」只能选择 1 名处置负责人，多人处置请改选运维+迭代或专项处理",
            status_label(status)
        ))),
        _ => Err(ReceiptError::InvalidForm(format!(
            "「{}」需选择 1-5 名处置负责人",
            status_label(status)
        ))),
    }
}

/// Save the authenticated operator's answer for one dimension. Each dimension
/// keeps a single latest answer (回执确认只需一次，可覆盖变更)； owners are the
/// 处置负责人 picked from the Feishu directory, recorded for traceability.
#[allow(clippy::too_many_arguments)]
pub async fn respond_from_group(
    db: &DatabaseConnection,
    task_id: i64,
    app_id: &str,
    tenant: &str,
    chat: &str,
    message: &str,
    open_id: &str,
    event_id: &str,
    dimension: &str,
    status: &str,
    owners: &[String],
) -> Result<(), ReceiptError> {
    if [app_id, tenant, chat, message, open_id, event_id]
        .iter()
        .any(|v| v.trim().is_empty())
    {
        return Err(ReceiptError::Invalid);
    }
    validate_answer(dimension, status, owners)?;
    // Canonical storage: [{"open_id":"ou_..","name":""}]; the name-lookup
    // loop fills the names asynchronously.
    let owners_json = serde_json::to_string(
        &owners
            .iter()
            .map(|open_id| serde_json::json!({"open_id": open_id, "name": ""}))
            .collect::<Vec<_>>(),
    )
    .map_err(|_| ReceiptError::Invalid)?;
    let tx = crate::output::db::sqlite::begin_write(db).await?;
    let dimension_row = tx
        .query_one(statement(
            "SELECT name FROM receipt_dimension WHERE slug=? AND enabled=1",
            vec![dimension.into()],
        ))
        .await?;
    if dimension_row.is_none() {
        return Err(ReceiptError::InvalidForm(
            "回执维度无效或已停用，请等待卡片刷新后重新提交".into(),
        ));
    }
    let bound=tx.execute(statement("UPDATE feishu_app SET tenant_key=? WHERE app_id=? AND tenant_key='' AND EXISTS(SELECT 1 FROM receipt_delivery WHERE task_id=? AND app_id=? AND tenant_key='' AND chat_id=? AND message_id=?)",vec![tenant.into(),app_id.into(),task_id.into(),app_id.into(),chat.into(),message.into()])).await?;
    if bound.rows_affected() == 1 {
        tx.execute(statement(
            "UPDATE receipt_delivery SET tenant_key=? WHERE app_id=? AND tenant_key=''",
            vec![tenant.into(), app_id.into()],
        ))
        .await?;
        tx.execute(statement(
            "UPDATE receipt_response SET tenant_key=? WHERE app_id=? AND tenant_key=''",
            vec![tenant.into(), app_id.into()],
        ))
        .await?;
    }
    let row=tx.query_one(statement("SELECT t.status,t.version FROM receipt_delivery d JOIN receipt_task t ON t.id=d.task_id AND t.case_id=d.case_id JOIN receipt_case rc ON rc.id=t.case_id AND rc.status='open' JOIN feishu_app app ON app.app_id=d.app_id AND app.tenant_key=d.tenant_key AND app.enabled=1 JOIN feishu_chat chat ON chat.app_id=app.id AND chat.chat_id=d.chat_id WHERE t.id=? AND d.app_id=? AND d.tenant_key=? AND d.chat_id=? AND d.message_id=? AND d.withdrawn=0",vec![task_id.into(),app_id.into(),tenant.into(),chat.into(),message.into()])).await?.ok_or(ReceiptError::Unauthorized)?;
    let prior=tx.query_one(statement("SELECT task_id,operator_open_id,new_status,details FROM receipt_event WHERE app_id=? AND event_id=?",vec![app_id.into(),event_id.into()])).await?;
    if let Some(event) = prior {
        if event.try_get::<i64>("", "task_id")? == task_id
            && event.try_get::<String>("", "operator_open_id")? == open_id
            && event.try_get::<String>("", "new_status")? == status
            && event.try_get::<String>("", "details")? == owners_json
        {
            tx.commit().await?;
            return Ok(());
        }
        return Err(ReceiptError::Duplicate);
    }
    // Prefer an administrator-verified mapping; the per-user Feishu lookup
    // (name + department) resolves the rest asynchronously after commit.
    let mapped_name = tx.query_one(statement("SELECT p.name FROM feishu_app a JOIN feishu_identity i ON i.app_id=a.id AND i.tenant_key=a.tenant_key JOIN receipt_person p ON p.id=i.person_id WHERE a.app_id=? AND a.tenant_key=? AND i.open_id=? AND i.enabled=1 AND p.enabled=1",vec![app_id.into(),tenant.into(),open_id.into()])).await?.map(|r| r.try_get::<String>("", "name")).transpose()?.unwrap_or_default();
    // One current answer per dimension; resubmitting replaces it (the operator
    // on record is whoever submitted the latest answer).
    let old = tx
        .query_one(statement(
            "SELECT id,display_name FROM receipt_response WHERE task_id=? AND dimension=?",
            vec![task_id.into(), dimension.into()],
        ))
        .await?;
    let response_id = if let Some(old) = old {
        let id: i64 = old.try_get("", "id")?;
        let old_name: String = old.try_get("", "display_name")?;
        let new_name = if old_name.is_empty() {
            mapped_name.clone()
        } else {
            old_name
        };
        tx.execute(statement("UPDATE receipt_response SET app_id=?,tenant_key=?,operator_open_id=?,display_name=?,status=?,owners=?,version=version+1,deleted=0,updated_at=CURRENT_TIMESTAMP WHERE id=?",vec![app_id.into(),tenant.into(),open_id.into(),new_name.into(),status.into(),owners_json.clone().into(),id.into()])).await?;
        id
    } else {
        tx.execute(statement("INSERT INTO receipt_response(task_id,app_id,tenant_key,operator_open_id,display_name,status,details,dimension,owners) VALUES(?,?,?,?,?,?, '', ?,?)",vec![task_id.into(),app_id.into(),tenant.into(),open_id.into(),mapped_name.into(),status.into(),dimension.into(),owners_json.clone().into()])).await?.last_insert_id() as i64
    };
    let previous: String = row.try_get("", "status")?;
    tx.execute(statement("INSERT INTO receipt_event(task_id,app_id,event_id,operator_open_id,old_status,new_status,details) VALUES(?,?,?,?,?,?,?)",vec![task_id.into(),app_id.into(),event_id.into(),open_id.into(),previous.clone().into(),status.into(),owners_json.clone().into()])).await?;
    tx.execute(statement("INSERT INTO receipt_name_lookup(response_id) SELECT id FROM receipt_response WHERE id=? AND (display_name='' OR team='' OR owners LIKE '%\"name\":\"\"%') ON CONFLICT(response_id) DO UPDATE SET last_attempt=0,last_error=''",vec![response_id.into()])).await?;
    recompute_task(&tx, task_id).await?;
    tx.execute(statement("INSERT INTO receipt_card_refresh(task_id) VALUES(?) ON CONFLICT(task_id) DO UPDATE SET revision=revision+1,last_error=''",vec![task_id.into()])).await?;
    tx.commit().await?;
    Ok(())
}

/// A respondent withdraws their own answer via the card's delete button.
/// Same card-origin checks as a submission, plus the global switch.
#[allow(clippy::too_many_arguments)]
pub async fn delete_own_response(
    db: &DatabaseConnection,
    task_id: i64,
    app_id: &str,
    tenant: &str,
    chat: &str,
    message: &str,
    open_id: &str,
    event_id: &str,
) -> Result<(), ReceiptError> {
    if [app_id, tenant, chat, message, open_id, event_id]
        .iter()
        .any(|v| v.trim().is_empty())
    {
        return Err(ReceiptError::Invalid);
    }
    let allowed = db
        .query_one(statement(
            "SELECT allow_response_deletion FROM sync_task ORDER BY id LIMIT 1",
            vec![],
        ))
        .await?
        .map(|row| row.try_get::<bool>("", "allow_response_deletion"))
        .transpose()?
        .unwrap_or(false);
    if !allowed {
        return Err(ReceiptError::DeletionDisabled);
    }
    let tx = crate::output::db::sqlite::begin_write(db).await?;
    let row = tx.query_one(statement("SELECT t.status FROM receipt_delivery d JOIN receipt_task t ON t.id=d.task_id AND t.case_id=d.case_id JOIN receipt_case rc ON rc.id=t.case_id AND rc.status='open' JOIN feishu_app app ON app.app_id=d.app_id AND app.tenant_key=d.tenant_key AND app.enabled=1 JOIN feishu_chat chat ON chat.app_id=app.id AND chat.chat_id=d.chat_id WHERE t.id=? AND d.app_id=? AND d.tenant_key=? AND d.chat_id=? AND d.message_id=? AND d.withdrawn=0",vec![task_id.into(),app_id.into(),tenant.into(),chat.into(),message.into()])).await?.ok_or(ReceiptError::Unauthorized)?;
    let previous: String = row.try_get("", "status")?;
    let prior = tx
        .query_one(statement(
            "SELECT task_id,operator_open_id FROM receipt_event WHERE app_id=? AND event_id=?",
            vec![app_id.into(), event_id.into()],
        ))
        .await?;
    if let Some(event) = prior {
        if event.try_get::<i64>("", "task_id")? == task_id
            && event.try_get::<String>("", "operator_open_id")? == open_id
        {
            tx.commit().await?;
            return Ok(());
        }
        return Err(ReceiptError::Duplicate);
    }
    let old = tx.query_one(statement("SELECT id FROM receipt_response WHERE task_id=? AND app_id=? AND tenant_key=? AND operator_open_id=? AND deleted=0",vec![task_id.into(),app_id.into(),tenant.into(),open_id.into()])).await?;
    let Some(old) = old else {
        return Err(ReceiptError::NoResponse);
    };
    let response_id: i64 = old.try_get("", "id")?;
    tx.execute(statement("UPDATE receipt_response SET deleted=1,version=version+1,updated_at=CURRENT_TIMESTAMP WHERE id=?",vec![response_id.into()])).await?;
    tx.execute(statement("INSERT INTO receipt_event(task_id,app_id,event_id,operator_open_id,old_status,new_status,details) VALUES(?,?,?,?,?,?,'填写人删除了自己的回执')",vec![task_id.into(),app_id.into(),event_id.into(),open_id.into(),previous.clone().into(),"deleted".into()])).await?;
    recompute_task(&tx, task_id).await?;
    tx.execute(statement("INSERT INTO receipt_card_refresh(task_id) VALUES(?) ON CONFLICT(task_id) DO UPDATE SET revision=revision+1,last_error=''",vec![task_id.into()])).await?;
    tx.commit().await?;
    Ok(())
}

/// Task status from dimension completion: done only when every enabled
/// dimension has a completed receipt (三维度齐备才可关闭).
async fn recompute_task(
    tx: &sea_orm::DatabaseTransaction,
    task_id: i64,
) -> Result<(), ReceiptError> {
    let row=tx.query_one(statement("SELECT COUNT(*) AS total, COALESCE(SUM(EXISTS(SELECT 1 FROM receipt_response r WHERE r.task_id=? AND r.deleted=0 AND r.dimension=d.slug AND r.status IN ('none','ops','iter','both','special'))),0) AS done FROM receipt_dimension d WHERE d.enabled=1",vec![task_id.into(),task_id.into()])).await?.ok_or(ReceiptError::Invalid)?;
    let total: i64 = row.try_get("", "total")?;
    let done: i64 = row.try_get("", "done")?;
    let status = if total > 0 && done >= total {
        "done"
    } else if done > 0 {
        "in_progress"
    } else {
        "pending"
    };
    tx.execute(statement(
        "UPDATE receipt_task SET status=?,version=version+1 WHERE id=?",
        vec![status.into(), task_id.into()],
    ))
    .await?;
    Ok(())
}

/// Enabled dimensions without a completed receipt, for the close gate and
/// reminders. Returns (slug, name, owner_name) triples.
pub async fn pending_dimensions(
    tx: &impl ConnectionTrait,
    task_id: i64,
) -> Result<Vec<(String, String, String)>, ReceiptError> {
    let rows = tx
        .query_all(statement(
            "SELECT slug,name,owner_name FROM receipt_dimension WHERE enabled=1 AND NOT EXISTS(SELECT 1 FROM receipt_response r WHERE r.task_id=? AND r.deleted=0 AND r.dimension=receipt_dimension.slug AND r.status IN ('none','ops','iter','both','special')) ORDER BY sort,id",
            vec![task_id.into()],
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok((
                row.try_get("", "slug")?,
                row.try_get("", "name")?,
                row.try_get("", "owner_name")?,
            ))
        })
        .collect()
}

/// Administrator edits or soft-deletes an individual answer; audit and card refresh are atomic.
/// `change` is (display_name, status, details, optional dimension reassignment);
/// legacy three-state statuses stay editable for rows recorded before v24.
pub async fn manage_response(
    db: &DatabaseConnection,
    response_id: i64,
    version: i64,
    change: Option<(&str, &str, &str, Option<&str>)>,
) -> Result<(), ReceiptError> {
    if let Some((name, status, details, _dimension)) = change
        && (name.len() > 200
            || details.chars().count() > 50
            || (!is_receipt_status(status)
                && !matches!(status, "investigating" | "unaffected" | "affected")))
    {
        return Err(ReceiptError::Invalid);
    }
    let tx = crate::output::db::sqlite::begin_write(db).await?;
    if let Some((_, _, _, Some(dimension))) = change {
        let valid = tx
            .query_one(statement(
                "SELECT id FROM receipt_dimension WHERE slug=? AND enabled=1",
                vec![dimension.into()],
            ))
            .await?;
        if valid.is_none() {
            return Err(ReceiptError::Invalid);
        }
    }
    let row=tx.query_one(statement("SELECT r.task_id,r.display_name,r.status,r.details,r.version,t.status AS task_status FROM receipt_response r JOIN receipt_task t ON t.id=r.task_id JOIN receipt_case c ON c.id=t.case_id WHERE r.id=? AND r.deleted=0 AND c.status='open'",vec![response_id.into()])).await?.ok_or(ReceiptError::Invalid)?;
    if row.try_get::<i64>("", "version")? != version {
        return Err(ReceiptError::Conflict);
    }
    let task_id: i64 = row.try_get("", "task_id")?;
    if let Some((name, status, details, dimension)) = change {
        tx.execute(statement("UPDATE receipt_response SET display_name=?,status=?,details=?,dimension=COALESCE(?,dimension),version=version+1,updated_at=CURRENT_TIMESTAMP WHERE id=?",vec![name.trim().into(),status.into(),details.trim().into(),dimension.map(str::to_owned).into(),response_id.into()])).await?;
    } else {
        tx.execute(statement("UPDATE receipt_response SET deleted=1,version=version+1,updated_at=CURRENT_TIMESTAMP WHERE id=?",vec![response_id.into()])).await?;
    }
    tx.execute(statement("INSERT INTO receipt_name_lookup(response_id) SELECT id FROM receipt_response WHERE id=? AND display_name='' AND deleted=0 ON CONFLICT(response_id) DO UPDATE SET last_attempt=0,last_error=''",vec![response_id.into()])).await?;
    recompute_task(&tx, task_id).await?;
    tx.execute(statement("INSERT INTO receipt_card_refresh(task_id) VALUES(?) ON CONFLICT(task_id) DO UPDATE SET revision=revision+1,last_error=''",vec![task_id.into()])).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn progress(
    db: &DatabaseConnection,
    task_id: i64,
    admin_id: i64,
    version: i64,
    status: &str,
    details: &str,
) -> Result<(), ReceiptError> {
    if details.trim().is_empty() || details.len() > 10000 {
        return Err(ReceiptError::Invalid);
    }
    let tx = crate::output::db::sqlite::begin_write(db).await?;
    let row = tx.query_one(statement("SELECT t.case_id,t.status,t.version FROM receipt_task t JOIN receipt_case c ON c.id=t.case_id WHERE t.id=? AND c.status='open'",vec![task_id.into()])).await?.ok_or(ReceiptError::Invalid)?;
    let old: String = row.try_get("", "status")?;
    let allowed = matches!(
        (old.as_str(), status),
        ("affected", "in_progress")
            | ("in_progress", "awaiting_verification")
            | ("awaiting_verification", "verified")
            | ("awaiting_verification", "in_progress")
    );
    if !allowed || row.try_get::<i64>("", "version")? != version {
        return Err(ReceiptError::Conflict);
    }
    let updated = tx
        .execute(statement(
            "UPDATE receipt_task SET status=?,version=version+1 WHERE id=? AND version=?",
            vec![status.into(), task_id.into(), version.into()],
        ))
        .await?;
    if updated.rows_affected() != 1 {
        return Err(ReceiptError::Conflict);
    }
    tx.execute(statement("INSERT INTO receipt_admin_event(case_id,task_id,admin_user_id,old_status,new_status,details) VALUES(?,?,?,?,?,?)",vec![row.try_get::<i64>("", "case_id")?.into(),task_id.into(),admin_id.into(),old.into(),status.into(),details.into()])).await?;
    tx.execute(statement("INSERT INTO receipt_card_refresh(task_id) VALUES(?) ON CONFLICT(task_id) DO UPDATE SET revision=revision+1,last_error=''",vec![task_id.into()])).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn close_case(
    db: &DatabaseConnection,
    case_id: i64,
    admin_id: i64,
    details: &str,
) -> Result<(), ReceiptError> {
    if details.trim().is_empty() || details.len() > 10000 {
        return Err(ReceiptError::Invalid);
    }
    let tx = crate::output::db::sqlite::begin_write(db).await?;
    // 三维度齐备才可关闭：every enabled dimension must hold a completed
    // receipt before the case can close.
    let task = tx
        .query_one(statement(
            "SELECT t.id FROM receipt_task t JOIN receipt_case c ON c.id=t.case_id WHERE c.id=? AND c.status='open'",
            vec![case_id.into()],
        ))
        .await?
        .ok_or(ReceiptError::Conflict)?;
    let task_id: i64 = task.try_get("", "id")?;
    let pending = pending_dimensions(&tx, task_id).await?;
    if !pending.is_empty() {
        let names = pending
            .iter()
            .map(|(_, name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join("、");
        return Err(ReceiptError::Incomplete(names));
    }
    let updated = tx.execute(statement("UPDATE receipt_case SET status='closed',closed_at=CURRENT_TIMESTAMP WHERE id=? AND status='open'",vec![case_id.into()])).await?;
    if updated.rows_affected() != 1 {
        return Err(ReceiptError::Conflict);
    }
    tx.execute(statement("INSERT INTO receipt_admin_event(case_id,admin_user_id,old_status,new_status,details) VALUES(?,?,'open','closed',?)",vec![case_id.into(),admin_id.into(),details.into()])).await?;
    tx.execute(statement("INSERT INTO receipt_card_refresh(task_id) SELECT id FROM receipt_task WHERE case_id=? ON CONFLICT(task_id) DO UPDATE SET revision=revision+1,last_error=''",vec![case_id.into()])).await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn seed_member(pool: &sqlx::SqlitePool, name: &str, open_id: &str) {
        sqlx::query("INSERT OR IGNORE INTO feishu_app(id,name,app_id,app_secret,tenant_key) VALUES(1,'app','app','secret','tenant')").execute(pool).await.unwrap();
        let person = sqlx::query("INSERT INTO receipt_person(name) VALUES(?)")
            .bind(name)
            .execute(pool)
            .await
            .unwrap()
            .last_insert_rowid();
        sqlx::query("INSERT INTO feishu_identity(person_id,app_id,tenant_key,open_id) VALUES(?,1,'tenant',?)").bind(person).bind(open_id).execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn automatic_cases_require_opt_in_and_current_date() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let orm = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool.clone());
        sqlx::query("INSERT INTO sync_task(name,interval_minutes,status) VALUES('sync',15,1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO vuln_information(key,title) VALUES('test','test')")
            .execute(&pool)
            .await
            .unwrap();
        seed_member(&pool, "Alice", "alice").await;
        let today = chrono::Local::now().date_naive().to_string();
        assert_eq!(auto_create(&orm, "vuln", 1, &today).await.unwrap(), None);
        sqlx::query("UPDATE sync_task SET auto_receipts=1")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            auto_create(&orm, "vuln", 1, "2000-01-01").await.unwrap(),
            None
        );
        assert_eq!(auto_create(&orm, "vuln", 1, "").await.unwrap(), None);
        assert_eq!(auto_create(&orm, "vuln", 1, &today).await.unwrap(), Some(1));
        assert_eq!(auto_create(&orm, "vuln", 1, &today).await.unwrap(), None);
        // Items published yesterday and scraped after local midnight (the
        // evening-publish/overnight-scrape pattern) still auto-create.
        sqlx::query("INSERT INTO vuln_information(key,title) VALUES('stale','test')")
            .execute(&pool)
            .await
            .unwrap();
        let yesterday = (chrono::Local::now().date_naive() - chrono::Duration::days(1)).to_string();
        assert_eq!(
            auto_create(&orm, "vuln", 2, &yesterday).await.unwrap(),
            Some(2)
        );
        let queued: i64 =
            sqlx::query_scalar("SELECT count(*) FROM receipt_auto_delivery WHERE state='pending'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(queued, 2);
    }

    #[tokio::test]
    async fn notice_cases_need_their_own_switch() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let orm = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool.clone());
        // Vuln receipts on, notice receipts off (the shipped default).
        sqlx::query("INSERT INTO sync_task(name,interval_minutes,status,auto_receipts) VALUES('sync',15,1,1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO security_notice(key,title) VALUES('n1','t')")
            .execute(&pool)
            .await
            .unwrap();
        let today = chrono::Local::now().date_naive().to_string();
        assert_eq!(
            auto_create(&orm, "notice", 1, &today).await.unwrap(),
            None,
            "公告自动确认默认关"
        );
        sqlx::query("UPDATE sync_task SET auto_receipts_notice=1")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            auto_create(&orm, "notice", 1, &today).await.unwrap(),
            Some(1)
        );
    }

    #[tokio::test]
    async fn concurrent_case_requests_reuse_one_case_and_new_case_after_close() {
        let path = std::env::temp_dir().join(format!("vulnfeed-cases-{}.db", uuid::Uuid::new_v4()));
        let mut config = crate::config::settings::Config::default();
        config.database.path = path.to_string_lossy().into_owned();
        let db = crate::output::db::sqlite::Db::new(&config).await.unwrap();
        db.migrate().await.unwrap();
        let orm = db.orm();
        orm.execute(statement(
            "INSERT INTO vuln_information(key,title) VALUES('test','test')",
            vec![],
        ))
        .await
        .unwrap();
        let (first, second) = tokio::join!(
            create_case_inner(&orm, "vuln", 1, false),
            create_case_inner(&orm, "vuln", 1, false)
        );
        let (first, second) = (first.unwrap(), second.unwrap());
        // Both racers land on the same case; exactly one created it.
        assert_eq!(first.0, second.0);
        assert_ne!(first.1, second.1);
        // One task per case, no team dimension.
        let tasks: i64 = orm
            .query_one(statement(
                "SELECT COUNT(*) AS n FROM receipt_task WHERE case_id=?",
                vec![first.0.into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "n")
            .unwrap();
        assert_eq!(tasks, 1);
        orm.execute(statement(
            "UPDATE receipt_case SET status='closed' WHERE id=?",
            vec![first.0.into()],
        ))
        .await
        .unwrap();
        let next = create_case_inner(&orm, "vuln", 1, false).await.unwrap();
        assert!(next.1);
        assert_ne!(first.0, next.0);
        drop(orm);
        drop(db);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }
}
