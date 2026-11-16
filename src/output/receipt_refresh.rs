//! Durable, serialized refresh of cards after administrator actions.
use sea_orm::DatabaseConnection;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DbErr, EntityTrait, QueryFilter, Statement,
};

use crate::output::{
    db::entities::feishu_app,
    feishu_receipts::{strip_table, submitted_card},
    push::feishu_app::{FeishuApp, FeishuAppError},
};

pub async fn run(db: DatabaseConnection) {
    loop {
        if let Err(e) = refresh_batch(&db).await {
            log::error!("Receipt card refresh queue failed: {e}");
        }
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

async fn refresh_batch(db: &DatabaseConnection) -> Result<(), DbErr> {
    let rows = db.query_all(Statement::from_string(DatabaseBackend::Sqlite,"SELECT task_id,revision FROM receipt_card_refresh WHERE completed_revision<revision ORDER BY last_attempt,task_id LIMIT 50")).await?;
    for row in rows {
        let id: i64 = row.try_get("", "task_id")?;
        let revision: i64 = row.try_get("", "revision")?;
        let result = refresh_one(db, id).await;
        let error = result.as_ref().err().cloned().unwrap_or_default();
        if !error.is_empty() {
            log::warn!("Failed to refresh Feishu card for task {id}: {error}");
        }
        db.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite,"UPDATE receipt_card_refresh SET completed_revision=CASE WHEN ? THEN MAX(completed_revision,?) ELSE completed_revision END,last_error=?,last_attempt=? WHERE task_id=?", [result.is_ok().into(),revision.into(),error.into(),chrono::Utc::now().timestamp().into(),id.into()])).await?;
    }
    Ok(())
}

/// Push one task's card immediately (used after post-callback enrichment so
/// names appear within seconds instead of waiting for the 30s scan).
pub async fn refresh_task_now(db: &DatabaseConnection, task_id: i64) {
    let revision = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT revision FROM receipt_card_refresh WHERE task_id=?",
            [task_id.into()],
        ))
        .await
        .ok()
        .flatten()
        .and_then(|row| row.try_get::<i64>("", "revision").ok())
        .unwrap_or(0);
    let result = refresh_one(db, task_id).await;
    let error = result.as_ref().err().cloned().unwrap_or_default();
    if !error.is_empty() {
        log::warn!("immediate card refresh failed for task {task_id}: {error}");
    }
    let _ = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE receipt_card_refresh SET completed_revision=CASE WHEN ? THEN MAX(completed_revision,?) ELSE completed_revision END,last_error=? WHERE task_id=?",
            [
                result.is_ok().into(),
                revision.into(),
                error.into(),
                task_id.into(),
            ],
        ))
        .await;
}

async fn refresh_one(db: &DatabaseConnection, task_id: i64) -> Result<(), String> {
    let deliveries = db.query_all(Statement::from_sql_and_values(DatabaseBackend::Sqlite,"SELECT id,app_id,chat_id,message_id FROM receipt_delivery WHERE task_id=? AND message_id IS NOT NULL AND withdrawn=0 ORDER BY id", [task_id.into()])).await.map_err(|e| e.to_string())?;
    if deliveries.is_empty() {
        return Ok(());
    }
    let card = submitted_card(db, task_id)
        .await
        .map_err(|e| e.to_string())?;
    let mut failed = Vec::new();
    for delivery in deliveries {
        let chat: String = delivery
            .try_get("", "chat_id")
            .map_err(|e| format!("{e}"))?;
        let delivery_app: String = delivery.try_get("", "app_id").map_err(|e| format!("{e}"))?;
        let settings = feishu_app::Entity::find()
            .filter(feishu_app::Column::AppId.eq(&delivery_app))
            .filter(feishu_app::Column::Enabled.eq(true))
            .one(db)
            .await
            .map_err(|e| e.to_string())?;
        let Some(settings) = settings else {
            failed.push(format!("群 {chat} 的应用不存在或已停用"));
            continue;
        };
        let client = match FeishuApp::new(settings.app_id, settings.app_secret) {
            Ok(client) => client,
            Err(e) => {
                failed.push(format!("群 {chat}: {e}"));
                continue;
            }
        };
        let message_id: String = delivery
            .try_get("", "message_id")
            .map_err(|e| format!("{e}"))?;
        match client.update_card(&message_id, &card).await {
            Ok(()) => {}
            Err(FeishuAppError::ApiDetail { code: 230011, .. }) => {
                let delivery_id: i64 = delivery.try_get("", "id").map_err(|e| e.to_string())?;
                db.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "UPDATE receipt_delivery SET withdrawn=1 WHERE id=? AND message_id=?",
                    [delivery_id.into(), message_id.into()],
                ))
                .await
                .map_err(|e| e.to_string())?;
                log::warn!("receipt card recalled in Feishu; manual re-send needed: chat {chat}");
            }
            // Feishu rejected the card payload (e.g. the table component);
            // retry once with the plain markdown view before recording failure.
            Err(FeishuAppError::ApiDetail {
                status,
                code,
                message,
            }) => {
                log::warn!(
                    "card update rejected for chat {chat} (HTTP {status} code {code} {message}); retrying with plain-text detail"
                );
                let plain = strip_table(&card);
                if let Err(e) = client.update_card(&message_id, &plain).await {
                    failed.push(format!("群 {chat}: {e}"));
                }
            }
            Err(e) => failed.push(format!("群 {chat}: {e}")),
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refresh_queue_completes_without_deliveries() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let orm = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool.clone());
        sqlx::query(
            "INSERT INTO receipt_case(source_type,source_id,title) VALUES('vuln',1,'test')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO receipt_task(case_id) VALUES(1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO receipt_card_refresh(task_id,revision,completed_revision) VALUES(1,2,0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        refresh_batch(&orm).await.unwrap();
        let completed: i64 = sqlx::query_scalar(
            "SELECT completed_revision FROM receipt_card_refresh WHERE task_id=1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(completed, 2);
    }
}
