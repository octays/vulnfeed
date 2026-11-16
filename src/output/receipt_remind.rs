//! One-shot and scheduled reminder cards for tasks with pending dimensions.
//! Reminders are ephemeral group messages: no delivery rows, no card refresh.
use crate::output::{db::sqlite::Db, push::feishu_app::FeishuApp, receipts::ReceiptError};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

/// Nudge at most once per 24h; SOP v24: no reminder-count cap, and from the
/// 3rd reminder on the escalation target is @-ed as well.
const REMIND_INTERVAL_SECS: i64 = 24 * 3600;
/// remind_count recorded before the 3rd send.
const ESCALATE_FROM_COUNT: i64 = 2;
/// Scan cadence for the background loop.
const SCAN_INTERVAL_SECS: u64 = 30 * 60;

pub struct ReminderOutcome {
    pub sent: Vec<String>,
    pub failed: Vec<(String, String)>,
}

/// Dimensions still owing a receipt for this task.
async fn pending_dimensions(
    db: &DatabaseConnection,
    task_id: i64,
) -> Result<Vec<(String, String, String)>, sea_orm::DbErr> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT name,owner_name,owner_open_id FROM receipt_dimension WHERE enabled=1 AND NOT EXISTS(SELECT 1 FROM receipt_response r WHERE r.task_id=? AND r.deleted=0 AND r.dimension=receipt_dimension.slug AND r.status IN ('none','ops','iter','both','special')) ORDER BY sort,id",
            [task_id.into()],
        ))
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let open_id: String = row.try_get("", "owner_open_id").unwrap_or_default();
            (
                row.try_get("", "name").unwrap_or_default(),
                row.try_get("", "owner_name").unwrap_or_default(),
                open_id,
            )
        })
        .collect())
}

/// Build and send the nudge card for one task; manual and auto paths share this.
pub async fn send_reminder(
    db: &DatabaseConnection,
    task_id: i64,
) -> Result<ReminderOutcome, ReceiptError> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT t.case_id,c.title FROM receipt_task t JOIN receipt_case c ON c.id=t.case_id WHERE t.id=? AND c.status='open'",
            [task_id.into()],
        ))
        .await?
        .ok_or(ReceiptError::TaskClosed)?;
    let case_id: i64 = row.try_get("", "case_id")?;
    let title: String = row.try_get("", "title")?;
    let pending = pending_dimensions(db, task_id).await?;
    if pending.is_empty() {
        return Err(ReceiptError::NoPending);
    }
    let remind_count: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT remind_count FROM receipt_task WHERE id=?",
            [task_id.into()],
        ))
        .await?
        .map(|row| row.try_get("", "remind_count"))
        .transpose()?
        .unwrap_or(0);
    let escalate_open_id: String = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT remind_escalate_open_id FROM sync_task ORDER BY id LIMIT 1",
        ))
        .await?
        .map(|row| row.try_get::<String>("", "remind_escalate_open_id"))
        .transpose()?
        .unwrap_or_default();
    // @ the owners of pending dimensions; from the 3rd reminder add the
    // escalation target (提醒升级).
    let mut mention_ids: Vec<String> = pending
        .iter()
        .map(|(_, _, open_id)| open_id.clone())
        .filter(|open_id| open_id.starts_with("ou_"))
        .collect();
    if remind_count >= ESCALATE_FROM_COUNT
        && escalate_open_id.starts_with("ou_")
        && !mention_ids.contains(&escalate_open_id)
    {
        mention_ids.push(escalate_open_id);
    }
    let chats = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT c.chat_id,app.app_id,app.app_secret FROM feishu_chat c JOIN feishu_app app ON app.id=c.app_id AND app.enabled=1 WHERE c.enabled=1 ORDER BY c.id",
            [],
        ))
        .await?;
    if chats.is_empty() {
        return Err(ReceiptError::NoChats);
    }
    let base_card = crate::output::feishu_receipts::reminder_card(
        case_id,
        &title,
        &pending
            .iter()
            .map(|(name, owner, _)| (name.clone(), owner.clone()))
            .collect::<Vec<_>>(),
    );
    let mut outcome = ReminderOutcome {
        sent: Vec::new(),
        failed: Vec::new(),
    };
    for chat in chats {
        let chat_id: String = chat.try_get("", "chat_id")?;
        let app_id: String = chat.try_get("", "app_id")?;
        let app_secret: String = chat.try_get("", "app_secret")?;
        let request_id = uuid::Uuid::new_v4().to_string();
        let card =
            crate::output::feishu_receipts::with_mentions(&base_card, &mention_ids.join(","));
        let result = match FeishuApp::new(app_id, app_secret) {
            Ok(client) => client.send_card(&chat_id, &card, &request_id).await,
            Err(e) => {
                log::error!("Feishu app initialization failed: {e}");
                outcome.failed.push((chat_id, e.to_string()));
                continue;
            }
        };
        match result {
            Ok(_) => outcome.sent.push(chat_id),
            Err(e) => {
                log::error!("Feishu reminder delivery failed for group {chat_id}: {e}");
                outcome.failed.push((chat_id, e.to_string()));
            }
        }
    }
    Ok(outcome)
}

pub async fn run(db: Db) {
    loop {
        if let Err(e) = process(&db).await {
            log::error!("Automatic receipt reminder loop failed: {e}");
        }
        tokio::time::sleep(std::time::Duration::from_secs(SCAN_INTERVAL_SECS)).await;
    }
}

/// Open tasks with pending dimensions whose 24h interval allows another nudge.
async fn due_tasks(db: &DatabaseConnection) -> Result<Vec<i64>, sea_orm::DbErr> {
    let now = chrono::Utc::now().timestamp();
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT t.id FROM receipt_task t JOIN receipt_case c ON c.id=t.case_id AND c.status='open' WHERE EXISTS(SELECT 1 FROM sync_task s WHERE s.auto_remind=1) AND t.last_reminded_at<=? AND EXISTS(SELECT 1 FROM receipt_dimension d WHERE d.enabled=1 AND NOT EXISTS(SELECT 1 FROM receipt_response r WHERE r.task_id=t.id AND r.deleted=0 AND r.dimension=d.slug AND r.status IN ('none','ops','iter','both','special'))) ORDER BY t.id LIMIT 20",
            [(now - REMIND_INTERVAL_SECS).into()],
        ))
        .await?;
    rows.iter()
        .map(|row| row.try_get::<i64>("", "id"))
        .collect()
}

async fn process(db: &Db) -> Result<(), sea_orm::DbErr> {
    for task_id in due_tasks(&db.orm()).await? {
        let now = chrono::Utc::now().timestamp();
        match send_reminder(&db.orm(), task_id).await {
            Ok(outcome) if !outcome.sent.is_empty() => {
                db.orm()
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        "UPDATE receipt_task SET last_reminded_at=?,remind_count=remind_count+1 WHERE id=?",
                        [now.into(), task_id.into()],
                    ))
                    .await?;
                log::info!(
                    "auto-reminder sent for task {task_id} ({} chats ok, {} failed)",
                    outcome.sent.len(),
                    outcome.failed.len()
                );
            }
            // Nothing to remind, or every send failed: push the next attempt
            // out by one interval without burning the reminder budget.
            other => {
                if let Err(e) = other {
                    log::warn!("auto-reminder not sent for task {task_id}: {e}");
                }
                db.orm()
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        "UPDATE receipt_task SET last_reminded_at=? WHERE id=? AND last_reminded_at<?",
                        [now.into(), task_id.into(), now.into()],
                    ))
                    .await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn due_tasks_require_flag_interval_and_pending_dimension() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO sync_task(name,interval_minutes,status,auto_remind) VALUES('sync',15,1,1)",
            "INSERT INTO feishu_app(name,app_id,app_secret,tenant_key) VALUES('app','app','secret','')",
            "INSERT INTO feishu_chat(app_id,tenant_key,chat_id,name) VALUES(1,'','oc_1','群')",
            "INSERT INTO vuln_information(key,title) VALUES('k','t')",
            "INSERT INTO receipt_case(source_type,source_id,title) VALUES('vuln',1,'t1'),('vuln',2,'t2'),('vuln',3,'t3')",
            // Task 1: one dimension answered, two pending -> due.
            "INSERT INTO receipt_task(case_id) VALUES(1)",
            "INSERT INTO receipt_response(task_id,app_id,tenant_key,operator_open_id,status,details,dimension) VALUES(1,'app','','ou_done','none','','p')",
            // Task 2: all three dimensions answered -> not due.
            "INSERT INTO receipt_task(case_id) VALUES(2)",
            "INSERT INTO receipt_response(task_id,app_id,tenant_key,operator_open_id,status,details,dimension) VALUES(2,'app','','ou_a','none','','p'),(2,'app','','ou_b','ops','','novo'),(2,'app','','ou_c','special','','ops')",
            // Task 3: no answers at all -> due.
            "INSERT INTO receipt_task(case_id) VALUES(3)",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        let orm = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool.clone());
        let mut due = due_tasks(&orm).await.unwrap();
        due.sort_unstable();
        assert_eq!(due, vec![1, 3]);
        // No reminder cap: an oft-reminded task with pending dimensions still fires.
        sqlx::query("UPDATE receipt_task SET remind_count=9 WHERE id=1")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(due_tasks(&orm).await.unwrap(), vec![1, 3]);
        sqlx::query("UPDATE receipt_task SET last_reminded_at=9999999999 WHERE id=1")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(due_tasks(&orm).await.unwrap(), vec![3]);
        sqlx::query("UPDATE receipt_task SET last_reminded_at=9999999999 WHERE id=3")
            .execute(&pool)
            .await
            .unwrap();
        assert!(due_tasks(&orm).await.unwrap().is_empty());
        sqlx::query("UPDATE sync_task SET auto_remind=0")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE receipt_task SET last_reminded_at=0")
            .execute(&pool)
            .await
            .unwrap();
        assert!(due_tasks(&orm).await.unwrap().is_empty());
    }
}
