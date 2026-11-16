//! Resolve Feishu display names and departments outside the time-limited
//! card callback. The first department becomes the response's 团队.
use crate::output::{db::entities::feishu_app, push::feishu_app::FeishuApp};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, Statement,
};

fn q(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

pub async fn run(db: DatabaseConnection) {
    loop {
        if let Err(e) = process_pending(&db).await {
            log::error!("receipt name/department lookup loop failed: {e}");
        }
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

/// Public so the card callback can trigger an immediate pass: fresh queue
/// rows (last_attempt=0) resolve within ~a second instead of the 30s scan.
pub async fn process_pending(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute(Statement::from_string(DatabaseBackend::Sqlite,"DELETE FROM receipt_name_lookup WHERE response_id IN (SELECT id FROM receipt_response WHERE deleted=1 OR ((display_name<>'' AND team<>'') AND (owners='' OR owners NOT LIKE '%\"name\":\"\"%')))")).await?;
    let now = chrono::Utc::now().timestamp();
    let rows=db.query_all(q("SELECT r.id,r.task_id,r.app_id,r.tenant_key,r.operator_open_id,r.version FROM receipt_name_lookup n JOIN receipt_response r ON r.id=n.response_id WHERE n.last_attempt<=? AND (r.display_name='' OR r.team='' OR r.owners LIKE '%\"name\":\"\"%') AND r.deleted=0 ORDER BY n.last_attempt,r.id LIMIT 10",vec![(now-300).into()])).await?;
    for row in rows {
        let id: i64 = row.try_get("", "id")?;
        let task_id: i64 = row.try_get("", "task_id")?;
        let app_id: String = row.try_get("", "app_id")?;
        let tenant: String = row.try_get("", "tenant_key")?;
        let open_id: String = row.try_get("", "operator_open_id")?;
        let version: i64 = row.try_get("", "version")?;
        db.execute(q(
            "UPDATE receipt_name_lookup SET last_attempt=? WHERE response_id=?",
            vec![now.into(), id.into()],
        ))
        .await?;
        let app = feishu_app::Entity::find()
            .filter(feishu_app::Column::AppId.eq(&app_id))
            .filter(feishu_app::Column::TenantKey.eq(&tenant))
            .filter(feishu_app::Column::Enabled.eq(true))
            .one(db)
            .await?;
        let Some(app) = app else {
            log::warn!(
                "cannot resolve receipt name/department (response #{id}): app missing or disabled"
            );
            db.execute(q(
                "UPDATE receipt_name_lookup SET last_error=? WHERE response_id=?",
                vec!["应用不存在或已停用".into(), id.into()],
            ))
            .await?;
            continue;
        };
        let client = match FeishuApp::new(app.app_id, app.app_secret) {
            Ok(client) => client,
            Err(e) => {
                log::warn!("cannot resolve receipt name/department (response #{id}): {e}");
                db.execute(q(
                    "UPDATE receipt_name_lookup SET last_error=? WHERE response_id=?",
                    vec![e.to_string().into(), id.into()],
                ))
                .await?;
                continue;
            }
        };
        // Fill in names of the chosen 处置负责人 owners first; failures here
        // do not block the operator resolution below.
        if let Err(e) = resolve_owner_names(db, &client, id, task_id).await {
            log::warn!("cannot resolve owner names (response #{id}): {e}");
        }
        match client.user_info(&open_id).await {
            Ok((name, departments)) => {
                let mut department = match departments.first() {
                    Some(department_id) => match client.department_name(department_id).await {
                        Ok(department) => Some(department),
                        Err(e) => {
                            log::warn!(
                                "failed to resolve department (response #{id}, dept {department_id}): {e}"
                            );
                            None
                        }
                    },
                    None => None,
                };
                // The user-object department_ids field is often field-gated;
                // fall back to the on-demand in-memory department resolver.
                let mut department_final = false;
                if department.is_none() {
                    let (cached, reliable) =
                        crate::output::receipt_department_cache::department_of(&client, &open_id)
                            .await;
                    department = cached;
                    department_final = reliable;
                }
                save_resolved(
                    db,
                    id,
                    task_id,
                    version,
                    &name,
                    department.as_deref(),
                    department_final,
                )
                .await?;
            }
            Err(e) => {
                // "Incomplete response" here almost always means the app can
                // reach the contact API but may not read user fields.
                let error = if matches!(
                    e,
                    crate::output::push::feishu_app::FeishuAppError::InvalidResponse
                ) {
                    format!(
                        "{e}; usually the app lacks the contact-directory read scope or the user is invisible to it; the team column needs user-organization and department read scopes"
                    )
                } else {
                    e.to_string()
                };
                log::warn!("cannot resolve receipt name/department (response #{id}): {error}");
                db.execute(q(
                    "UPDATE receipt_name_lookup SET last_error=? WHERE response_id=?",
                    vec![e.to_string().into(), id.into()],
                ))
                .await?;
            }
        }
    }
    Ok(())
}

/// Resolve empty owner names in the stored owners JSON and persist them
/// (best effort; a failed lookup retries on the next pass).
async fn resolve_owner_names(
    db: &DatabaseConnection,
    client: &FeishuApp,
    id: i64,
    task_id: i64,
) -> Result<(), DbErr> {
    let row = db
        .query_one(q(
            "SELECT owners FROM receipt_response WHERE id=? AND deleted=0",
            vec![id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let owners_json: String = row.try_get("", "owners")?;
    let Ok(serde_json::Value::Array(mut entries)) = serde_json::from_str(&owners_json) else {
        return Ok(());
    };
    let mut changed = false;
    for entry in entries.iter_mut() {
        let open_id = entry["open_id"].as_str().unwrap_or_default().to_owned();
        let filled = entry["name"].as_str().is_some_and(|name| !name.is_empty());
        if filled || !open_id.starts_with("ou_") {
            continue;
        }
        if let Ok((name, _)) = client.user_info(&open_id).await
            && !name.is_empty()
            && let Some(object) = entry.as_object_mut()
        {
            object.insert("name".to_owned(), serde_json::json!(name));
            changed = true;
        }
    }
    if changed {
        let updated = serde_json::to_string(&entries).unwrap_or(owners_json);
        db.execute(q(
            "UPDATE receipt_response SET owners=? WHERE id=? AND deleted=0",
            vec![updated.into(), id.into()],
        ))
        .await?;
        db.execute(q(
            "INSERT INTO receipt_card_refresh(task_id) VALUES(?) ON CONFLICT(task_id) DO UPDATE SET revision=revision+1,last_error=''",
            vec![task_id.into()],
        ))
        .await?;
    }
    Ok(())
}

/// Persist whatever could be resolved; audit name writes (as before). The
/// queue row is dropped once the name is filled and the team is either
/// resolved or known to be absent (`department_final`).
async fn save_resolved(
    db: &DatabaseConnection,
    id: i64,
    task_id: i64,
    version: i64,
    name: &str,
    department: Option<&str>,
    department_final: bool,
) -> Result<(), DbErr> {
    let tx = crate::output::db::sqlite::begin_write(db).await?;
    if !name.is_empty() {
        tx.execute(q(
            "UPDATE receipt_response SET display_name=?,version=version+1 WHERE id=? AND version=? AND display_name='' AND deleted=0",
            vec![name.into(), id.into(), version.into()],
        ))
        .await?;
    }
    if let Some(department) = department {
        tx.execute(q(
            "UPDATE receipt_response SET team=? WHERE id=? AND team='' AND deleted=0",
            vec![department.into(), id.into()],
        ))
        .await?;
    }
    if !name.is_empty() && (department.is_some() || department_final) {
        tx.execute(q(
            "DELETE FROM receipt_name_lookup WHERE response_id=?",
            vec![id.into()],
        ))
        .await?;
    }
    tx.execute(q(
        "INSERT INTO receipt_card_refresh(task_id) VALUES(?) ON CONFLICT(task_id) DO UPDATE SET revision=revision+1,last_error=''",
        vec![task_id.into()],
    ))
    .await?;
    tx.execute(q(
        "DELETE FROM receipt_name_lookup WHERE response_id=? AND response_id IN (SELECT id FROM receipt_response WHERE display_name<>'' AND team<>'' AND (owners='' OR owners NOT LIKE '%\"name\":\"\"%'))",
        vec![id.into()],
    ))
    .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn resolved_name_is_saved_audited_and_does_not_override_admin() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO receipt_case(source_type,source_id,title) VALUES('vuln',1,'test')",
            "INSERT INTO receipt_task(case_id) VALUES(1)",
            "INSERT INTO receipt_response(task_id,app_id,tenant_key,operator_open_id,status,details) VALUES(1,'app','tenant','ou_test','unaffected','ok')",
            "INSERT INTO receipt_name_lookup(response_id) VALUES(1)",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        let db = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool.clone());
        save_resolved(&db, 1, 1, 1, "飞书姓名", Some("基础架构部"), false)
            .await
            .unwrap();
        let row: (String, String) =
            sqlx::query_as("SELECT display_name,team FROM receipt_response WHERE id=1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row, ("飞书姓名".to_owned(), "基础架构部".to_owned()));
        let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM receipt_name_lookup")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(queued, 0);
        // Admin-set values are never overridden by later lookups.
        save_resolved(&db, 1, 1, 99, "不能覆盖", Some("别的部门"), false)
            .await
            .unwrap();
        let row: (String, String) =
            sqlx::query_as("SELECT display_name,team FROM receipt_response WHERE id=1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row, ("飞书姓名".to_owned(), "基础架构部".to_owned()));
    }
}
