//! Durable retry queue for newly created automatic confirmation cards.
use crate::output::db::sqlite::Db;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

pub async fn run(db: Db) {
    loop {
        if let Err(error) = process(&db).await {
            log::error!("Automatic receipt delivery queue failed: {error}");
        }
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

async fn process(db: &Db) -> Result<(), sea_orm::DbErr> {
    let orm = db.orm();
    let now = chrono::Utc::now().timestamp();
    let rows = orm.query_all(Statement::from_sql_and_values(DatabaseBackend::Sqlite,
        "SELECT q.task_id FROM receipt_auto_delivery q JOIN receipt_task t ON t.id=q.task_id JOIN receipt_case c ON c.id=t.case_id WHERE q.state='pending' AND q.next_attempt<=? AND q.lease_until<=? AND c.status='open' ORDER BY q.next_attempt,q.task_id LIMIT 20",
        [now.into(), now.into()])).await?;
    for row in rows {
        let id: i64 = row.try_get("", "task_id")?;
        let now = chrono::Utc::now().timestamp();
        let claim = orm.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite,
            "UPDATE receipt_auto_delivery SET lease_until=?,attempts=attempts+1 WHERE task_id=? AND state='pending' AND next_attempt<=? AND lease_until<=?",
            [(now+300).into(), id.into(), now.into(), now.into()])).await?;
        if claim.rows_affected() == 0 {
            continue;
        }
        match crate::input::http::handlers::receipts::deliver_task(db, id).await {
            Ok(_) => {
                orm.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite,
                    "UPDATE receipt_auto_delivery SET state='sent',lease_until=0,last_error='' WHERE task_id=?",
                    [id.into()])).await?;
                log::info!("Automatic receipt card delivered for task {id}");
            }
            Err(error) => {
                let message = format!("{error:?}");
                log::warn!(
                    "Automatic receipt card delivery failed for task {id}; retrying: {message}"
                );
                orm.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite,
                    "UPDATE receipt_auto_delivery SET lease_until=0,next_attempt=?,last_error=? WHERE task_id=?",
                    [(chrono::Utc::now().timestamp()+60).into(), message.into(), id.into()])).await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn failed_delivery_is_retained_for_retry_and_closed_cases_are_skipped() {
        let path = std::env::temp_dir().join(format!("receipt-queue-{}.db", uuid::Uuid::new_v4()));
        let mut config = crate::config::settings::Config::default();
        config.database.path = path.to_string_lossy().into_owned();
        let db = Db::new(&config).await.unwrap();
        db.migrate().await.unwrap();
        let orm = db.orm();
        for sql in [
            "INSERT INTO sync_task(name,interval_minutes,status,auto_receipts) VALUES('sync',15,1,1)",
            "INSERT INTO vuln_information(key,title) VALUES('test','test')",
        ] {
            orm.execute(Statement::from_string(DatabaseBackend::Sqlite, sql))
                .await
                .unwrap();
        }
        let today = chrono::Local::now().date_naive().to_string();
        crate::output::receipts::auto_create(&orm, "vuln", 1, &today)
            .await
            .unwrap();
        process(&db).await.unwrap();
        process(&db).await.unwrap();
        let row = orm
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT attempts,last_error,state FROM receipt_auto_delivery",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<i64>("", "attempts").unwrap(), 1);
        assert!(!row.try_get::<String>("", "last_error").unwrap().is_empty());
        assert_eq!(row.try_get::<String>("", "state").unwrap(), "pending");
        orm.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "UPDATE receipt_case SET status='closed'",
        ))
        .await
        .unwrap();
        orm.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "UPDATE receipt_auto_delivery SET next_attempt=0",
        ))
        .await
        .unwrap();
        process(&db).await.unwrap();
        let row = orm
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT attempts FROM receipt_auto_delivery",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<i64>("", "attempts").unwrap(), 1);
        drop(orm);
        drop(db);
        let _ = std::fs::remove_file(path);
    }
}
