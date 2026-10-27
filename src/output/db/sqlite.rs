use std::{path::Path, time::Duration};

use error_stack::ResultExt;
use sqlx::{
    Pool, Sqlite,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

use crate::{AppResult, config::settings::Config, errors::Error};

#[derive(Debug, Clone)]
pub struct Db {
    pub pool: Pool<Sqlite>,
}

impl Db {
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
        Ok(Self { pool })
    }
}
