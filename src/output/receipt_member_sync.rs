//! Periodically cache Feishu chat member lists for response-rate display.
//! The cache is never used to authorize card submissions (see the receipt docs).
use crate::output::{db::sqlite::Db, push::feishu_app::FeishuApp};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

const INTERVAL_SECS: i64 = 6 * 3600;

pub async fn run(db: Db) {
    loop {
        if let Err(e) = process(&db).await {
            log::error!("Feishu chat member sync failed: {e}");
        }
        tokio::time::sleep(std::time::Duration::from_secs(INTERVAL_SECS as u64)).await;
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncSummary {
    pub synced: i64,
    pub failed: i64,
}

pub async fn process(db: &Db) -> Result<SyncSummary, sea_orm::DbErr> {
    let mut summary = SyncSummary::default();
    let orm = db.orm();
    let now = chrono::Utc::now().timestamp();
    let chats = orm.query_all(Statement::from_sql_and_values(DatabaseBackend::Sqlite,
        "SELECT DISTINCT c.id,c.chat_id,app.app_id,app.app_secret FROM feishu_chat c JOIN feishu_app app ON app.id=c.app_id AND app.enabled=1 WHERE c.enabled=1 AND c.last_member_sync<?",
        [(now - INTERVAL_SECS).into()])).await?;
    for chat in chats {
        let id: i64 = chat.try_get("", "id")?;
        let chat_id: String = chat.try_get("", "chat_id")?;
        let app_id: String = chat.try_get("", "app_id")?;
        let app_secret: String = chat.try_get("", "app_secret")?;
        let record_error = |error: String| {
            Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE feishu_chat SET last_member_sync=?,last_member_error=? WHERE id=?",
                [
                    now.into(),
                    error
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(240)
                        .collect::<String>()
                        .into(),
                    id.into(),
                ],
            )
        };
        let client = match FeishuApp::new(app_id, app_secret) {
            Ok(client) => client,
            Err(e) => {
                log::warn!("chat member sync failed for {chat_id}: {e}");
                orm.execute(record_error(e.to_string())).await?;
                summary.failed += 1;
                continue;
            }
        };
        match client.chat_members(&chat_id).await {
            Ok(members) => {
                let tx = crate::output::db::sqlite::begin_write(&orm).await?;
                tx.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "DELETE FROM feishu_chat_member WHERE chat_id=?",
                    [id.into()],
                ))
                .await?;
                for (open_id, name) in &members {
                    tx.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite,
                        "INSERT OR IGNORE INTO feishu_chat_member(chat_id,open_id,name) VALUES(?,?,?)",
                        [id.into(), open_id.as_str().into(), name.as_str().into()])).await?;
                }
                tx.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "UPDATE feishu_chat SET last_member_sync=?,last_member_error='' WHERE id=?",
                    [now.into(), id.into()],
                ))
                .await?;
                // Membership changed the pending list; refresh live cards.
                tx.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite,
                    "INSERT INTO receipt_card_refresh(task_id) SELECT t.id FROM receipt_task t JOIN receipt_case rc ON rc.id=t.case_id AND rc.status='open' ON CONFLICT(task_id) DO UPDATE SET revision=revision+1,last_error=''",
                    [id.into()])).await?;
                tx.commit().await?;
                log::info!("synced {} chat members for {chat_id}", members.len());
                summary.synced += 1;
            }
            Err(e) => {
                log::warn!("chat member sync failed for {chat_id}: {e}");
                orm.execute(record_error(e.to_string())).await?;
                summary.failed += 1;
            }
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sync_without_bound_chats_is_a_noop() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        // A chat exists but is not bound to any team: it must not be selected.
        sqlx::query("INSERT INTO feishu_app(name,app_id,app_secret,tenant_key) VALUES('app','app','secret','')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO feishu_chat(app_id,tenant_key,chat_id,name) VALUES(1,'','oc_x','群')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let db = crate::output::db::sqlite::Db::test_pool(pool);
        process(&db).await.unwrap();
        let count: i64 = db
            .orm()
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM feishu_chat_member",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "n")
            .unwrap();
        assert_eq!(count, 0);
    }
}
