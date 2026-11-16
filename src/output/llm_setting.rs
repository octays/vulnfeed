//! DB-backed LLM settings (admin UI) and the client built from them.
use crate::output::llm::LlmClient;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmSetting {
    pub enabled: bool,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub timeout_secs: u64,
}

impl Default for LlmSetting {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: String::new(),
            api_key: String::new(),
            model: "deepseek-chat".to_string(),
            timeout_secs: 20,
        }
    }
}

impl LlmSetting {
    /// Redacted copy for API responses: the key never leaves the server.
    pub fn redacted(&self) -> Self {
        Self {
            api_key: String::new(),
            ..self.clone()
        }
    }

    fn into_config(self) -> crate::config::settings::LlmConfig {
        crate::config::settings::LlmConfig {
            enabled: self.enabled,
            base_url: self.base_url,
            api_key: self.api_key,
            model: self.model,
            timeout_secs: self.timeout_secs,
        }
    }
}

pub async fn load(db: &DatabaseConnection) -> Result<LlmSetting, DbErr> {
    let row = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT enabled,base_url,api_key,model,timeout_secs FROM llm_setting WHERE id=1",
        ))
        .await?;
    let Some(row) = row else {
        return Ok(LlmSetting::default());
    };
    Ok(LlmSetting {
        enabled: row.try_get("", "enabled")?,
        base_url: row.try_get("", "base_url")?,
        api_key: row.try_get("", "api_key")?,
        model: row.try_get("", "model")?,
        timeout_secs: row.try_get::<i64>("", "timeout_secs")?.max(3) as u64,
    })
}

/// Update all editable fields; an empty api_key keeps the stored one so the
/// masked edit form never wipes the secret.
pub async fn save(db: &DatabaseConnection, setting: &LlmSetting) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE llm_setting SET enabled=?,base_url=?,api_key=CASE WHEN ? THEN ? ELSE api_key END,model=?,timeout_secs=? WHERE id=1",
        [
            setting.enabled.into(),
            setting.base_url.trim().into(),
            setting.api_key.trim().is_empty().into(),
            setting.api_key.trim().into(),
            setting.model.trim().into(),
            (setting.timeout_secs.max(3) as i64).into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Build a ready client from the stored settings; None when disabled or
/// incomplete (callers silently fall back to the keyword classifier).
pub async fn client(db: &DatabaseConnection) -> Option<LlmClient> {
    let setting = load(db).await.ok()?;
    LlmClient::new(setting.into_config()).ok()
}
