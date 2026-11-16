use std::{path::Path, time::Duration};

use error_stack::ResultExt;
use sqlx::{
    Pool, Sqlite,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

use crate::{AppResult, config::settings::Config, errors::Error};

#[derive(Debug, Clone)]
pub struct Db {
    connection: sea_orm::DatabaseConnection,
}

impl Db {
    /// All application queries use this SeaORM connection and its configured SQLite pool.
    pub fn orm(&self) -> sea_orm::DatabaseConnection {
        self.connection.clone()
    }

    fn from_pool(pool: Pool<Sqlite>) -> Self {
        Self {
            connection: sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool),
        }
    }

    /// Retain the existing SQLx migration history and checksums for deployed databases.
    ///
    /// Table-rebuild migrations DROP parent tables (receipt_task,
    /// receipt_response), which SQLite rejects while foreign keys are
    /// enforced — and the pragma is a no-op inside the migrator's
    /// per-migration transaction, so it must be toggled on the connection
    /// itself. Turn FK off for the run and restore it afterwards.
    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        let mut conn = self
            .connection
            .get_sqlite_connection_pool()
            .acquire()
            .await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        let result = sqlx::migrate!("./migrations").run(&mut *conn).await;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await?;
        result
    }

    #[cfg(test)]
    pub(crate) fn test_pool(pool: Pool<Sqlite>) -> Self {
        Self::from_pool(pool)
    }

    #[cfg(test)]
    pub(crate) fn sqlx_pool(&self) -> &Pool<Sqlite> {
        self.connection.get_sqlite_connection_pool()
    }
    pub async fn new(config: &Config) -> AppResult<Self> {
        if let Some(parent) = Path::new(&config.database.path).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).change_context_lazy(|| {
                Error::Message(format!(
                    "failed to create database directory {}",
                    parent.display()
                ))
            })?;
        }
        let opts = SqliteConnectOptions::new()
            .filename(&config.database.path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(30))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .connect_with(opts)
            .await
            .change_context_lazy(|| Error::Message("failed to connect to database".to_string()))?;
        Ok(Self::from_pool(pool))
    }
}

/// Acquire SQLite's write reservation before any reads in a read-modify-write
/// transaction. A deferred transaction that reads first can otherwise fail with
/// SQLITE_BUSY_SNAPSHOT even when busy_timeout is configured.
pub async fn begin_write(
    db: &sea_orm::DatabaseConnection,
) -> Result<sea_orm::DatabaseTransaction, sea_orm::DbErr> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
    let tx = db.begin().await?;
    tx.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        "UPDATE vuln_information SET id=id WHERE 0",
    ))
    .await?;
    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::Db;
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    use sqlx::{migrate::Migrator, sqlite::SqlitePoolOptions};
    use std::borrow::Cow;

    #[tokio::test]
    async fn existing_sqlx_database_upgrades_without_losing_data() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let all = sqlx::migrate!("./migrations");
        let old = Migrator {
            migrations: Cow::Owned(
                all.iter()
                    .filter(|m| m.version < 20261029091000)
                    .cloned()
                    .collect(),
            ),
            ..Migrator::DEFAULT
        };
        old.run(&pool).await.unwrap();
        sqlx::query("INSERT INTO vuln_information(key,title) VALUES('existing','保留数据')")
            .execute(&pool)
            .await
            .unwrap();
        let db = Db::test_pool(pool);
        db.migrate().await.unwrap();
        db.migrate().await.unwrap();
        let row = db
            .orm()
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT title FROM vuln_information WHERE key = 'existing'",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<String>("", "title").unwrap(), "保留数据");
        let row = db
            .orm()
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM _sqlx_migrations WHERE success = 1",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.try_get::<i64>("", "count").unwrap(),
            all.iter().count() as i64
        );
    }
}

#[cfg(test)]
mod identity_model_tests {
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn identity_binds_open_id_per_app() {
        let opts = sqlx::sqlite::SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO feishu_app(name,app_id,app_secret,tenant_key) VALUES('A','a','secret','tenant'),('B','b','secret','tenant')",
            "INSERT INTO receipt_person(name) VALUES('person')",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        // Same open_id under two apps is two distinct identities.
        sqlx::query("INSERT INTO feishu_identity(person_id,app_id,tenant_key,open_id) VALUES(1,1,'tenant','alice')").execute(&pool).await.unwrap();
        assert!(sqlx::query("INSERT INTO feishu_identity(person_id,app_id,tenant_key,open_id) VALUES(1,1,'tenant','alice')").execute(&pool).await.is_err());
        sqlx::query("INSERT INTO feishu_identity(person_id,app_id,tenant_key,open_id) VALUES(1,2,'tenant','alice')").execute(&pool).await.unwrap();
    }
}

#[cfg(test)]
mod concurrency_tests {
    use super::{Db, begin_write};
    use crate::config::settings::Config;
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    use std::time::Duration;

    #[tokio::test]
    async fn competing_read_then_write_transactions_wait_and_commit() {
        let path = std::env::temp_dir().join(format!("vulnfeed-write-{}.db", uuid::Uuid::new_v4()));
        let mut config = Config::default();
        config.database.path = path.to_string_lossy().into_owned();
        let db = Db::new(&config).await.unwrap();
        db.migrate().await.unwrap();
        let conn = db.orm();
        let first = begin_write(&conn).await.unwrap();
        first
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM vuln_information",
            ))
            .await
            .unwrap();
        let second_conn = conn.clone();
        let mut second = tokio::spawn(async move {
            let tx = begin_write(&second_conn).await.unwrap();
            tx.query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM vuln_information",
            ))
            .await
            .unwrap();
            tx.execute(Statement::from_string(
                DatabaseBackend::Sqlite,
                "INSERT INTO vuln_information(key,title) VALUES('second','second')",
            ))
            .await
            .unwrap();
            tx.commit().await.unwrap();
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut second)
                .await
                .is_err(),
            "second writer must wait before reading"
        );
        first
            .execute(Statement::from_string(
                DatabaseBackend::Sqlite,
                "INSERT INTO vuln_information(key,title) VALUES('first','first')",
            ))
            .await
            .unwrap();
        first.commit().await.unwrap();
        second.await.unwrap();
        let row = conn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT count(*) AS n FROM vuln_information",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<i64>("", "n").unwrap(), 2);
        drop(conn);
        drop(db);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }
}

#[cfg(test)]
mod individual_migration_tests {
    use super::Db;
    use sqlx::{migrate::Migrator, sqlite::SqlitePoolOptions};
    use std::borrow::Cow;

    /// Regression for the FK-787 startup failure: upgrading a database that
    /// already holds receipt rows through the table-rebuild migrations
    /// (drop_teams, receipt_dimensions) must succeed on the FK-enabled
    /// application pool.
    #[tokio::test]
    async fn rebuild_migrations_pass_with_existing_receipt_rows() {
        let opts = sqlx::sqlite::SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        let all = sqlx::migrate!("./migrations");
        let before = Migrator {
            migrations: Cow::Owned(
                all.iter()
                    .filter(|m| m.version < 20261105120000)
                    .cloned()
                    .collect(),
            ),
            ..Migrator::DEFAULT
        };
        before.run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO vuln_information(key,title) VALUES('k','t')",
            "INSERT INTO receipt_case(source_type,source_id,title) VALUES('vuln',1,'t')",
            "INSERT INTO receipt_task(case_id,team_id,team_name) VALUES(1,(SELECT MAX(id) FROM receipt_team),'全部团队')",
            "INSERT INTO feishu_app(name,app_id,app_secret,tenant_key) VALUES('app','app','secret','tenant')",
            "INSERT INTO receipt_response(task_id,app_id,tenant_key,operator_open_id,status,details) VALUES(1,'app','tenant','ou_a','affected','存量回执')",
            "INSERT INTO receipt_name_lookup(response_id) VALUES(1)",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        let db = Db::test_pool(pool.clone());
        db.migrate().await.unwrap();
        // ids survive the rebuilds; legacy rows gain the new default columns.
        let row: (String, String, String, String) = sqlx::query_as(
            "SELECT status,details,dimension,owners FROM receipt_response WHERE id=1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row, ("affected".into(), "存量回执".into(), "".into(), "".into()));
        let dims: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM receipt_dimension WHERE enabled=1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(dims, 3);
        // FKs are back on for the application pool.
        let fk: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[tokio::test]
    async fn old_events_are_preserved_and_sent_cards_are_queued_for_refresh() {
        let opts = sqlx::sqlite::SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(false);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        let all = sqlx::migrate!("./migrations");
        let before = Migrator {
            migrations: Cow::Owned(
                all.iter()
                    .filter(|m| m.version < 20261029160000)
                    .cloned()
                    .collect(),
            ),
            ..Migrator::DEFAULT
        };
        before.run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO receipt_case(source_type,source_id,title) VALUES('vuln',1,'test')",
            "INSERT INTO receipt_team(name) VALUES('t')",
            "INSERT INTO receipt_task(case_id,team_id,team_name) VALUES(1,1,'t')",
            "INSERT INTO feishu_app(name,app_id,app_secret,tenant_key) VALUES('app','app','secret','tenant')",
            "INSERT INTO receipt_event(task_id,app_id,event_id,operator_open_id,old_status,new_status,details) VALUES(1,'app','event1','ou_a','pending','investigating','first'),(1,'app','event2','ou_a','investigating','affected','latest')",
            "INSERT INTO receipt_delivery(case_id,task_id,app_id,tenant_key,chat_id,message_id,request_id,task_version) VALUES(1,1,'app','tenant','chat','message','req',0)",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        all.run(&pool).await.unwrap();
        let answer: (String, String) =
            sqlx::query_as("SELECT status,details FROM receipt_response WHERE task_id=1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(answer, ("affected".into(), "latest".into()));
        let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM receipt_event")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(events, 2);
        let pending:i64=sqlx::query_scalar("SELECT COUNT(*) FROM receipt_card_refresh WHERE task_id=1 AND completed_revision<revision").fetch_one(&pool).await.unwrap();
        assert_eq!(pending, 1);
    }
}
