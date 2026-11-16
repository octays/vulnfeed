//! Durable per-group message reservations for idempotent Feishu delivery.
use crate::output::db::entities::receipt_delivery::{self, Column, Model};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, QueryOrder, Statement,
};

pub async fn reserve(
    db: &DatabaseConnection,
    case_id: i64,
    task_id: i64,
    app_id: &str,
    tenant: &str,
    chat: &str,
    version: i64,
) -> Result<Model, DbErr> {
    let request_id = uuid::Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite,"INSERT INTO receipt_delivery(case_id,task_id,app_id,tenant_key,chat_id,request_id,task_version) VALUES(?,?,?,?,?,?,?) ON CONFLICT(task_id,app_id,tenant_key,chat_id,task_version) DO NOTHING", [case_id.into(),task_id.into(),app_id.into(),tenant.into(),chat.into(),request_id.into(),version.into()])).await?;
    receipt_delivery::Entity::find()
        .filter(Column::TaskId.eq(task_id))
        .filter(Column::AppId.eq(app_id))
        .filter(Column::TenantKey.eq(tenant))
        .filter(Column::ChatId.eq(chat))
        .filter(Column::TaskVersion.eq(version))
        .one(db)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("receipt delivery reservation".into()))
}

/// Reuse a live card even if the task has since advanced to a later version.
pub async fn find_active(
    db: &DatabaseConnection,
    task_id: i64,
    app_id: &str,
    tenant: &str,
    chat: &str,
) -> Result<Option<Model>, DbErr> {
    receipt_delivery::Entity::find()
        .filter(Column::TaskId.eq(task_id))
        .filter(Column::AppId.eq(app_id))
        .filter(Column::TenantKey.eq(tenant))
        .filter(Column::ChatId.eq(chat))
        .filter(Column::Withdrawn.eq(false))
        .filter(Column::MessageId.is_not_null())
        .order_by_desc(Column::Id)
        .one(db)
        .await
}

/// Reuse the logical delivery slot while retaining the withdrawn message snapshot.
pub async fn prepare_withdrawn_retry(db: &DatabaseConnection, id: i64) -> Result<Model, DbErr> {
    let tx = crate::output::db::sqlite::begin_write(db).await?;
    tx.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite,
        "INSERT INTO receipt_withdrawn_delivery(delivery_id,message_id,request_id) SELECT id,message_id,request_id FROM receipt_delivery WHERE id=? AND withdrawn=1 AND message_id IS NOT NULL",[id.into()])).await?;
    tx.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite,
        "UPDATE receipt_delivery SET message_id=NULL,withdrawn=0,request_id=? WHERE id=? AND withdrawn=1",[uuid::Uuid::new_v4().to_string().into(),id.into()])).await?;
    let row = receipt_delivery::Entity::find_by_id(id)
        .one(&tx)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("receipt delivery".into()))?;
    tx.commit().await?;
    Ok(row)
}

pub async fn mark_sent(
    db: &DatabaseConnection,
    delivery_id: i64,
    message_id: &str,
) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE receipt_delivery SET message_id=? WHERE id=? AND message_id IS NULL",
        [message_id.into(), delivery_id.into()],
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::SqlxSqliteConnector;
    #[tokio::test]
    async fn reservations_are_stable_per_group_and_version() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
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
        let db = SqlxSqliteConnector::from_sqlx_sqlite_pool(pool);
        let first = reserve(&db, 1, 1, "app", "tenant", "group1", 0)
            .await
            .unwrap();
        let again = reserve(&db, 1, 1, "app", "tenant", "group1", 0)
            .await
            .unwrap();
        assert_eq!(first.id, again.id);
        assert_eq!(first.request_id, again.request_id);
        let other = reserve(&db, 1, 1, "app", "tenant", "group2", 0)
            .await
            .unwrap();
        assert_ne!(first.request_id, other.request_id);
        mark_sent(&db, first.id, "om_1").await.unwrap();
        mark_sent(&db, first.id, "om_other").await.unwrap();
        assert_eq!(
            reserve(&db, 1, 1, "app", "tenant", "group1", 0)
                .await
                .unwrap()
                .message_id
                .as_deref(),
            Some("om_1")
        );
        assert_eq!(
            find_active(&db, 1, "app", "tenant", "group1")
                .await
                .unwrap()
                .unwrap()
                .id,
            first.id
        );
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE receipt_delivery SET withdrawn=1 WHERE id=?",
            [first.id.into()],
        ))
        .await
        .unwrap();
        assert!(
            find_active(&db, 1, "app", "tenant", "group1")
                .await
                .unwrap()
                .is_none()
        );
        let retry = prepare_withdrawn_retry(&db, first.id).await.unwrap();
        assert!(!retry.withdrawn);
        assert!(retry.message_id.is_none());
        assert_ne!(retry.request_id, first.request_id);
        let again = prepare_withdrawn_retry(&db, first.id).await.unwrap();
        assert_eq!(again.request_id, retry.request_id);
        let count: i64 = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM receipt_withdrawn_delivery",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "n")
            .unwrap();
        assert_eq!(count, 1);
        assert_ne!(
            reserve(&db, 1, 1, "app", "tenant", "group1", 1)
                .await
                .unwrap()
                .id,
            first.id
        );
    }
}
