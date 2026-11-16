//! Card action processing. Only call this after authenticating the Feishu transport.
use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, FromQueryResult, Statement};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    domain::models::{security_notice::SecuritNotice, vuln_information::VulnInformation},
    output::receipts::{self, ReceiptError},
};

pub enum ReceiptSource {
    Vulnerability(VulnInformation),
    Notice(SecuritNotice),
}

pub async fn load_source(
    db: &sea_orm::DatabaseConnection,
    source_type: &str,
    source_id: i64,
) -> Result<Option<ReceiptSource>, DbErr> {
    let q = |sql: &str| {
        Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, [source_id.into()])
    };
    match source_type {
        "vuln" => Ok(VulnInformation::find_by_statement(q(
            "SELECT * FROM vuln_information WHERE id=?",
        ))
        .one(db)
        .await?
        .map(ReceiptSource::Vulnerability)),
        "notice" => Ok(SecuritNotice::find_by_statement(q(
            "SELECT * FROM security_notice WHERE id=?",
        ))
        .one(db)
        .await?
        .map(ReceiptSource::Notice)),
        _ => Err(DbErr::Custom("invalid receipt source type".into())),
    }
}

fn display(value: &str) -> &str {
    if value.trim().is_empty() {
        "暂无"
    } else {
        value.trim()
    }
}

fn list(values: &[String]) -> String {
    if values.is_empty() {
        "暂无".to_string()
    } else {
        values.join("\n")
    }
}

fn sanitize(value: &str, limit: usize) -> String {
    value
        .chars()
        .take(limit)
        .map(|c| match c {
            '\n' | '\r' | '[' | ']' | '<' | '>' => ' ',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Normalize stored timestamps for display: SQLite "YYYY-MM-DD HH:MM:SS" is
/// UTC and converts to local; RFC3339 values with an offset also convert;
/// naive source strings like the scraped publish_time only get reformatted
/// (their timezone is unknown, so shifting them would be a guess).
fn local_time(raw: &str) -> String {
    let raw = raw.trim();
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        && let chrono::LocalResult::Single(local) = naive.and_local_timezone(chrono::Local)
    {
        return local.format("%Y-%m-%d %H:%M").to_string();
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f") {
        return naive.format("%Y-%m-%d %H:%M").to_string();
    }
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(raw) {
        return parsed
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();
    }
    raw.to_owned()
}

struct SurveyRow {
    dimension: String,
    status: String,
    owners: String,
    member: String,
    time: String,
}

/// Survey-style detail table (card JSON 2.0 table component).
fn survey_table(rows: &[SurveyRow]) -> Value {
    json!({
        "tag": "table",
        "page_size": rows.len(),
        "row_height": "low",
        "header_style": {"text_align": "left", "text_size": "normal", "background_style": "grey", "text_color": "grey", "bold": false, "lines": 1},
        "columns": [
            {"name": "dimension", "display_name": "维度", "data_type": "text"},
            {"name": "status", "display_name": "处置流程（五选一）", "data_type": "text"},
            {"name": "owners", "display_name": "处置负责人", "data_type": "text"},
            {"name": "member", "display_name": "提交人", "data_type": "text"},
            {"name": "time", "display_name": "提交时间", "data_type": "text"}
        ],
        "rows": rows.iter().map(|row| json!({
            "dimension": row.dimension,
            "status": row.status,
            "owners": row.owners,
            "member": row.member,
            "time": row.time
        })).collect::<Vec<_>>()
    })
}

/// Replace table components with a plain markdown list. Delivery and refresh
/// fall back to this when Feishu rejects the table component, so committed
/// answers still reach the group card.
pub fn strip_table(card: &Value) -> Value {
    let mut card = card.clone();
    let Some(elements) = card["body"]["elements"].as_array_mut() else {
        return card;
    };
    let mut rebuilt = Vec::with_capacity(elements.len());
    let mut replaced = false;
    for element in elements.drain(..) {
        if element["tag"] == "table" {
            if !replaced {
                rebuilt.push(markdown_from_table(&element));
                replaced = true;
            }
        } else {
            rebuilt.push(element);
        }
    }
    *elements = rebuilt;
    card
}

fn markdown_from_table(table: &Value) -> Value {
    let mut content = String::from("**回执明细**\n");
    let rows = table["rows"].as_array();
    if rows.is_none_or(Vec::is_empty) {
        content.push_str("暂无回执\n");
    }
    if let Some(rows) = rows {
        for row in rows {
            let dimension = row["dimension"].as_str().unwrap_or_default();
            let status = row["status"].as_str().unwrap_or_default();
            let owners = row["owners"].as_str().unwrap_or_default();
            let member = row["member"].as_str().unwrap_or_default();
            let time = row["time"].as_str().unwrap_or_default();
            if member.is_empty() && status.is_empty() {
                continue;
            }
            content.push_str(&format!("• [{dimension}] {status}"));
            if !owners.is_empty() {
                content.push_str(&format!("（负责人：{owners}）"));
            }
            content.push('\n');
            if !member.is_empty() {
                content.push_str(&format!("  {member} {time}\n"));
            }
        }
    }
    json!({"tag": "markdown", "content": content})
}

/// Ephemeral nudge card listing dimensions that still owe a receipt. The
/// reminder loop prepends @ mentions for the dimension owners (plus the
/// escalation target from the 3rd reminder on) via `with_mentions`.
pub fn reminder_card(case_id: i64, title: &str, pending: &[(String, String)]) -> Value {
    let lines = pending
        .iter()
        .map(|(name, owner)| {
            format!(
                "• {}{}",
                sanitize(name, 40),
                if owner.trim().is_empty() {
                    String::new()
                } else {
                    format!("（{}）", sanitize(owner, 40))
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    json!({
        "schema": "2.0",
        "config": {"update_multi": true},
        "header": {"title": {"tag": "plain_text", "content": "催办：影响确认"}},
        "body": {"elements": [{"tag": "markdown", "content": format!(
            "**确认单**：#{case_id} {}\n\n⏳ **未回执 {} 个维度**：\n{lines}\n\n请在本群上方的影响确认卡片中提交回执；再次点击「提交（可覆盖变更）」可修改本维度答案。",
            sanitize(title, 200),
            pending.len()
        )}]}
    })
}

pub fn confirmation_card(
    source: &ReceiptSource,
    allow_delete: bool,
    task_id: i64,
    version: i64,
    dimensions: &[(String, String)],
) -> Value {
    let (kind, content) = match source {
        ReceiptSource::Vulnerability(v) => (
            "漏洞影响确认",
            format!(
                "**漏洞名称**：{}\n**漏洞编号**：{}\n**CVE**：{}\n**危害等级**：{}\n**披露日期**：{}\n**信息来源**：{}\n**来源地址**：{}\n**漏洞详情**：{}\n**标签**：{}\n\n**漏洞描述**\n{}\n\n**修复建议**\n{}\n\n**参考链接**\n{}\n\n**开源检索**\n{}",
                display(&v.title),
                display(&v.key),
                display(&v.cve),
                display(&v.severity),
                display(&v.disclosure),
                display(&v.source_name),
                display(&v.source),
                display(&v.detail_link),
                if v.tags.is_empty() {
                    "暂无".to_owned()
                } else {
                    v.tags.join("、")
                },
                display(&v.description),
                display(&v.solutions),
                list(&v.reference_links),
                list(&v.github_search)
            ),
        ),
        ReceiptSource::Notice(n) => (
            "安全公告影响确认",
            format!(
                "**公告标题**：{}\n**公告编号**：{}\n**产品名称**：{}\n**风险等级**：{}\n**发布日期**：{}\n**零日漏洞**：{}\n**信息来源**：{}\n**来源地址**：{}\n**公告详情**：{}",
                display(&n.title),
                display(&n.key),
                display(&n.product_name),
                display(&n.risk_level),
                local_time(&n.publish_time),
                if n.is_zero_day { "是" } else { "否" },
                display(&n.source_name),
                display(&n.source),
                display(&n.detail_link)
            ),
        ),
    };
    // Offered only when the admin enabled respondent self-deletion.
    let delete_button = allow_delete.then(|| json!({
        "tag": "button", "name": "delete-response", "text": {"tag": "plain_text", "content": "删除我的回执"},
        "type": "danger", "behaviors": [{"type": "callback", "value": {"task_id": task_id, "version": version, "delete": true}}]
    }));
    let mut card = json!({
        "schema": "2.0",
        "config": {"update_multi": true},
        "header": {"title": {"tag": "plain_text", "content": kind}},
        "body": {"elements": [
            {"tag": "markdown", "content": content},
            {"tag": "hr"},
            {"tag": "form", "name": "receipt", "elements": [
                {"tag": "select_static", "name": "dimension", "placeholder": {"tag": "plain_text", "content": "请选择回执维度"}, "required": true,
                 "options": dimensions.iter().map(|(slug, name)| json!({"text": {"tag": "plain_text", "content": name}, "value": slug})).collect::<Vec<_>>()},
                {"tag": "select_static", "name": "status", "placeholder": {"tag": "plain_text", "content": "五选一：处置流程"}, "required": true,
                 "options": receipts::RECEIPT_STATUSES.iter().map(|(slug, label)| json!({"text": {"tag": "plain_text", "content": label}, "value": slug})).collect::<Vec<_>>()},
                {"tag": "multi_select_person", "name": "owners", "placeholder": {"tag": "plain_text", "content": "处置负责人（飞书通讯录，无影响不选）"}},
                {"tag": "button", "name": "submit", "text": {"tag": "plain_text", "content": "提交（可覆盖变更）"}, "type": "primary", "form_action_type": "submit", "behaviors": [{"type": "callback", "value": {"task_id": task_id, "version": version}}]}
            ]},
            delete_button
        ]}
    });
    card["body"]["elements"]
        .as_array_mut()
        .expect("card elements are an array")
        .retain(|element| !element.is_null());
    card
}

/// Clone a card and prepend a mention line (`<at>` tags render as blue @name
/// in Feishu). Used per chat so each group @s its own configured people.
pub fn with_mentions(card: &Value, mention_open_ids: &str) -> Value {
    let mut card = card.clone();
    let ids: Vec<&str> = mention_open_ids
        .split(',')
        .map(str::trim)
        .filter(|id| id.starts_with("ou_"))
        .collect();
    if ids.is_empty() {
        return card;
    }
    let line = ids
        .iter()
        .map(|id| format!("<at user_id=\"{id}\"></at>"))
        .chain(std::iter::once("请关注本条影响确认".to_owned()))
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(elements) = card["body"]["elements"].as_array_mut() {
        elements.insert(0, json!({"tag": "markdown", "content": line}));
    }
    card
}

/// Build the shared-card view from committed state, never from untrusted form values.
pub async fn submitted_card(
    db: &sea_orm::DatabaseConnection,
    task_id: i64,
) -> Result<Value, DbErr> {
    let q =
        |sql: &str| Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, [task_id.into()]);
    let row = db.query_one(q("SELECT t.status,t.version,c.source_type,c.source_id,c.status AS case_status FROM receipt_task t JOIN receipt_case c ON c.id=t.case_id WHERE t.id=?")).await?.ok_or_else(|| DbErr::RecordNotFound("receipt task".into()))?;
    let source_id: i64 = row.try_get("", "source_id")?;
    let source_type: String = row.try_get("", "source_type")?;
    let source = load_source(db, &source_type, source_id)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("receipt source".into()))?;
    let dimensions = db
        .query_all(q(
            "SELECT slug,name FROM receipt_dimension WHERE enabled=1 ORDER BY sort,id",
        ))
        .await?;
    let dimension_pairs: Vec<(String, String)> = dimensions
        .iter()
        .map(|d| {
            Ok((
                d.try_get::<String>("", "slug")?,
                d.try_get::<String>("", "name")?,
            ))
        })
        .collect::<Result<_, DbErr>>()?;
    // Latest answer per dimension drives the survey summary; legacy
    // individual rows (dimension='') stay in the detail list only.
    let answers = db.query_all(q("SELECT dimension,display_name,operator_open_id,status,owners,updated_at,team FROM receipt_response WHERE task_id=? AND deleted=0 AND dimension<>'' ORDER BY updated_at DESC, id DESC")).await?;
    let legacy = db.query_all(q("SELECT '' AS dimension,team AS declared,display_name,operator_open_id,status,details,updated_at FROM receipt_response WHERE task_id=? AND deleted=0 AND dimension='' ORDER BY updated_at DESC, id DESC LIMIT 21")).await?;
    let case_status: String = row.try_get("", "case_status")?;
    let allow_delete: bool = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT allow_response_deletion FROM sync_task ORDER BY id LIMIT 1",
        ))
        .await?
        .map(|r| r.try_get::<bool>("", "allow_response_deletion"))
        .transpose()?
        .unwrap_or(false);
    let mut card = confirmation_card(
        &source,
        allow_delete,
        task_id,
        row.try_get("", "version")?,
        &dimension_pairs,
    );
    let elements = card["body"]["elements"]
        .as_array_mut()
        .expect("card elements are an array");
    // Detach the form and any trailing buttons (e.g. self-delete) so the
    // survey results render between the details and the entry points.
    let mut entry_points = Vec::new();
    while let Some(element) = elements.pop() {
        let is_form = element["tag"] == "form";
        entry_points.insert(0, element);
        if is_form {
            break;
        }
    }
    // Per-dimension summary: ✅ option (owners) or ⏳ pending.
    let mut survey = String::new();
    for (slug, name) in &dimension_pairs {
        let answered = answers.iter().find(|a| {
            a.try_get::<String>("", "dimension")
                .is_ok_and(|d| d == *slug)
        });
        match answered {
            Some(answer) => {
                let state: String = answer.try_get("", "status")?;
                let owners: String = answer.try_get("", "owners")?;
                let owners = owners_names(&owners);
                survey.push_str(&format!(
                    "✅ **{name}** — {}",
                    receipts::status_label(&state)
                ));
                if !owners.is_empty() {
                    survey.push_str(&format!("（负责人：{owners}）"));
                }
                survey.push('\n');
            }
            None => survey.push_str(&format!("⏳ **{name}** — 未回执\n")),
        }
    }
    let done = dimension_pairs
        .iter()
        .filter(|(slug, _)| {
            answers.iter().any(|a| {
                a.try_get::<String>("", "dimension")
                    .is_ok_and(|d| d == *slug)
                    && a.try_get::<String>("", "status")
                        .is_ok_and(|s| receipts::is_receipt_status(&s))
            })
        })
        .count();
    let survey = if dimension_pairs.is_empty() {
        format!("**回执统计**：{survey}")
    } else {
        format!(
            "**回执统计**：已完成 {done}/{} 维度\n{survey}",
            dimension_pairs.len()
        )
    };
    elements.push(json!({"tag":"markdown","content":survey}));
    let mut rows: Vec<SurveyRow> = answers
        .iter()
        .map(|answer| {
            let dimension: String = answer.try_get("", "dimension")?;
            let name: String = answer.try_get("", "display_name")?;
            let open_id: String = answer.try_get("", "operator_open_id")?;
            let state: String = answer.try_get("", "status")?;
            let owners: String = answer.try_get("", "owners")?;
            let at: String = answer.try_get("", "updated_at")?;
            Ok::<SurveyRow, DbErr>(SurveyRow {
                dimension: dimension_name(&dimension_pairs, &dimension),
                status: receipts::status_label(&state).to_owned(),
                owners: sanitize(&owners_names(&owners), 120),
                member: sanitize(
                    if name.trim().is_empty() {
                        &open_id
                    } else {
                        &name
                    },
                    80,
                ),
                time: local_time(&at),
            })
        })
        .collect::<Result<_, _>>()?;
    rows.extend(
        legacy
            .iter()
            .map(|answer| {
                let name: String = answer.try_get("", "display_name")?;
                let open_id: String = answer.try_get("", "operator_open_id")?;
                let state: String = answer.try_get("", "status")?;
                let details: String = answer.try_get("", "details")?;
                let at: String = answer.try_get("", "updated_at")?;
                Ok::<SurveyRow, DbErr>(SurveyRow {
                    dimension: "—（旧个人回执）".to_owned(),
                    status: receipts::status_label(&state).to_owned(),
                    owners: sanitize(&details, 120),
                    member: sanitize(
                        if name.trim().is_empty() {
                            &open_id
                        } else {
                            &name
                        },
                        80,
                    ),
                    time: local_time(&at),
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    rows.truncate(20);
    if !rows.is_empty() {
        elements.push(survey_table(&rows));
    }
    if case_status == "open" {
        for element in entry_points {
            elements.push(element);
        }
    }
    Ok(card)
}

fn dimension_name(dimensions: &[(String, String)], slug: &str) -> String {
    dimensions
        .iter()
        .find(|(candidate, _)| candidate == slug)
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| slug.to_owned())
}

/// Human-readable owner list from the stored owners JSON
/// ([{"open_id":"ou_..","name":".."}]); unresolved names fall back to the
/// raw open_id until the lookup loop fills them in. `with_ids` renders
/// "name (ou_..)" for CSV export.
pub fn owners_display(owners_json: &str, with_ids: bool) -> String {
    let Ok(serde_json::Value::Array(entries)) = serde_json::from_str(owners_json) else {
        return String::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let open_id = entry["open_id"].as_str().unwrap_or_default().to_owned();
            let name = entry["name"].as_str().unwrap_or_default();
            if open_id.is_empty() {
                None
            } else if name.trim().is_empty() {
                Some(open_id)
            } else if with_ids {
                Some(format!("{name} ({open_id})"))
            } else {
                Some(name.to_owned())
            }
        })
        .collect::<Vec<_>>()
        .join("、")
}

fn owners_names(owners_json: &str) -> String {
    owners_display(owners_json, false)
}

#[derive(Deserialize)]
pub struct CardCallback {
    pub header: Header,
    pub event: Event,
}

#[derive(Deserialize)]
pub struct Header {
    pub event_id: String,
    pub event_type: String,
    pub app_id: String,
    pub tenant_key: String,
}

#[derive(Deserialize)]
pub struct Event {
    pub operator: Operator,
    pub context: Context,
    pub action: Action,
}

#[derive(Deserialize)]
pub struct Operator {
    pub open_id: String,
    pub tenant_key: String,
}

#[derive(Deserialize)]
pub struct Context {
    pub open_message_id: String,
    pub open_chat_id: String,
}

#[derive(Deserialize)]
pub struct Action {
    pub value: ActionValue,
    // A standalone (non-form) button callback carries no form_value.
    #[serde(default)]
    pub form_value: FormValue,
}

#[derive(Deserialize)]
pub struct ActionValue {
    pub task_id: i64,
    pub version: i64,
    #[serde(default)]
    pub delete: bool,
}

#[derive(Deserialize, Default)]
pub struct FormValue {
    #[serde(default)]
    pub dimension: String,
    #[serde(default)]
    pub status: String,
    /// select_person submits an array of open_ids (or id objects); tolerate
    /// both, plus a plain comma-separated string.
    #[serde(default)]
    pub owners: Option<serde_json::Value>,
}

/// Extract open_ids from the owners form value.
fn owners_from_form(value: &Option<serde_json::Value>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    match value {
        serde_json::Value::Array(entries) => entries
            .iter()
            .filter_map(|entry| match entry {
                serde_json::Value::String(open_id) => Some(open_id.clone()),
                serde_json::Value::Object(_) => entry["open_id"]
                    .as_str()
                    .or_else(|| entry["id"].as_str())
                    .or_else(|| entry["value"].as_str())
                    .map(str::to_owned),
                _ => None,
            })
            .collect(),
        serde_json::Value::String(raw) => raw
            .split(',')
            .map(str::trim)
            .filter(|id| id.starts_with("ou_"))
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// What the card action did, so the bridge can toast the right message.
pub enum CardAction {
    Submitted,
    Deleted,
}

/// Validates card origin and then applies the response using the authenticated operator.
pub async fn handle_action(
    db: &sea_orm::DatabaseConnection,
    callback: CardCallback,
) -> Result<CardAction, ReceiptError> {
    let h = &callback.header;
    let e = &callback.event;
    if h.event_type != "card.action.trigger"
        || h.tenant_key.is_empty()
        || h.tenant_key != e.operator.tenant_key
    {
        return Err(ReceiptError::Unauthorized);
    }
    if e.action.value.delete {
        receipts::delete_own_response(
            db,
            e.action.value.task_id,
            &h.app_id,
            &h.tenant_key,
            &e.context.open_chat_id,
            &e.context.open_message_id,
            &e.operator.open_id,
            &h.event_id,
        )
        .await
        .map(|()| CardAction::Deleted)?;
        return Ok(CardAction::Deleted);
    }
    let result = receipts::respond_from_group(
        db,
        e.action.value.task_id,
        &h.app_id,
        &h.tenant_key,
        &e.context.open_chat_id,
        &e.context.open_message_id,
        &e.operator.open_id,
        &h.event_id,
        &e.action.form_value.dimension,
        &e.action.form_value.status,
        &owners_from_form(&e.action.form_value.owners),
    )
    .await;
    if result.is_err() {
        // e.g. an old-form card carried legacy values: refresh the card so the
        // group sees the current dimension/five-option form instead.
        if let Err(e) = db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO receipt_card_refresh(task_id) VALUES(?) ON CONFLICT(task_id) DO UPDATE SET revision=revision+1,last_error=''",
            [e.action.value.task_id.into()],
        ))
        .await
        {
            log::warn!("failed to enqueue card refresh after rejected action: {e}");
        }
    }
    result.map(|()| CardAction::Submitted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;

    #[test]
    fn mention_line_is_prepended_and_sanitized() {
        let notice = SecuritNotice {
            id: 1,
            key: "k".into(),
            title: "t".into(),
            product_name: "p".into(),
            risk_level: "High".into(),
            source: "s".into(),
            source_name: "sn".into(),
            is_zero_day: false,
            publish_time: "2026-11-03".into(),
            detail_link: String::new(),
            pushed: false,
            stacks: String::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let card = confirmation_card(
            &ReceiptSource::Notice(notice),
            false,
            1,
            0,
            &[
                ("p".into(), "P 产线".into()),
                ("novo".into(), "NovoOne 产线".into()),
                ("ops".into(), "运维".into()),
            ],
        );
        let text = card.to_string();
        assert!(text.contains("P 产线"));
        assert!(text.contains("已规划专项处理（含客户侧）"));
        assert!(text.contains("\"multi_select_person\""));
        let mentioned = with_mentions(&card, "ou_a, bad-id, ou_b");
        let elements = mentioned["body"]["elements"].as_array().unwrap();
        assert_eq!(elements[0]["tag"], "markdown");
        let content = elements[0]["content"].as_str().unwrap();
        assert!(content.contains("<at user_id=\"ou_a\"></at>"));
        assert!(content.contains("<at user_id=\"ou_b\"></at>"));
        assert!(!content.contains("bad-id"));
        assert_eq!(with_mentions(&card, "").to_string(), card.to_string());
    }

    #[test]
    fn reminder_card_lists_pending_dimensions_without_a_form() {
        let pending = vec![
            ("P 产线".to_owned(), "乔虎虎".to_owned()),
            ("运维".to_owned(), "郑荣超".to_owned()),
        ];
        let card = reminder_card(7, "远程代码执行漏洞", &pending);
        let text = card.to_string();
        assert!(text.contains("#7"));
        assert!(text.contains("远程代码执行漏洞"));
        assert!(text.contains("未回执 2 个维度"));
        assert!(text.contains("P 产线（乔虎虎）"));
        assert!(text.contains("运维（郑荣超）"));
        assert!(!text.contains("\"tag\":\"form\""));
    }

    #[tokio::test]
    async fn cards_include_source_details_and_confirmation_form() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("INSERT INTO vuln_information(key,title,cve,severity,description,solutions,reference_links,github_search) VALUES('key','漏洞测试','CVE-2026-1','High','描述内容','修复内容','[\"https://example.com/ref\"]','[\"https://example.com/poc\"]')").execute(&pool).await.unwrap();
        let vuln: VulnInformation = sqlx::query_as("SELECT * FROM vuln_information WHERE id=1")
            .fetch_one(&pool)
            .await
            .unwrap();
        let source = ReceiptSource::Vulnerability(vuln);
        let dimensions = vec![
            ("p".to_owned(), "P 产线".to_owned()),
            ("novo".to_owned(), "NovoOne 产线".to_owned()),
            ("ops".to_owned(), "运维".to_owned()),
        ];
        let card = confirmation_card(&source, false, 42, 3, &dimensions);
        let content = card["body"]["elements"][0]["content"].as_str().unwrap();
        for text in [
            "漏洞测试",
            "CVE-2026-1",
            "High",
            "描述内容",
            "修复内容",
            "https://example.com/ref",
            "https://example.com/poc",
        ] {
            assert!(content.contains(text), "missing {text}");
        }
        assert!(!content.contains("确认团队"));
        let form = &card["body"]["elements"][2];
        let fields = form["elements"].as_array().unwrap();
        // Dimension single-select, five-option single-select, directory owner
        // picker, submit button — no free-text fields anymore.
        assert!(!fields.iter().any(|element| element["name"] == "details"));
        assert_eq!(fields[0]["name"], "dimension");
        assert_eq!(fields[0]["options"].as_array().unwrap().len(), 3);
        assert_eq!(fields[1]["name"], "status");
        assert_eq!(fields[1]["options"].as_array().unwrap().len(), 5);
        assert_eq!(fields[2]["name"], "owners");
        assert_eq!(fields[2]["tag"], "multi_select_person");
        assert_eq!(form["elements"][3]["text"]["content"], "提交（可覆盖变更）");
        assert_eq!(form["elements"][3]["behaviors"][0]["value"]["task_id"], 42);
        assert_eq!(form["elements"][3]["behaviors"][0]["value"]["version"], 3);
        // With self-deletion enabled the card grows a danger button after the form.
        let with_delete = confirmation_card(&source, true, 42, 3, &dimensions);
        let elements = with_delete["body"]["elements"].as_array().unwrap();
        assert!(elements.iter().all(|element| !element.is_null()));
        let delete_button = elements
            .iter()
            .find(|element| element["name"] == "delete-response")
            .expect("delete button");
        assert_eq!(delete_button["type"], "danger");
        assert_eq!(delete_button["behaviors"][0]["value"]["delete"], true);
        assert!(
            card["body"]["elements"]
                .as_array()
                .unwrap()
                .iter()
                .all(|element| element["name"] != "delete-response")
        );
        sqlx::query("INSERT INTO security_notice(key,title,product_name,risk_level,source_name,is_zero_day,publish_time,detail_link) VALUES('n1','公告测试','产品A','High','厂商',1,'2026-10-29','https://example.com/notice')").execute(&pool).await.unwrap();
        let notice: SecuritNotice = sqlx::query_as("SELECT * FROM security_notice WHERE id=1")
            .fetch_one(&pool)
            .await
            .unwrap();
        let card = confirmation_card(&ReceiptSource::Notice(notice), false, 1, 0, &[]);
        let content = card["body"]["elements"][0]["content"].as_str().unwrap();
        for text in [
            "公告测试",
            "产品A",
            "High",
            "2026-10-29",
            "https://example.com/notice",
            "零日漏洞**：是",
        ] {
            assert!(content.contains(text), "missing {text}");
        }
        // A scraped ISO publish_time has no timezone: reformat, never shift it.
        sqlx::query("INSERT INTO security_notice(key,title,product_name,risk_level,source_name,is_zero_day,publish_time,detail_link) VALUES('n2','ISO公告','产品B','High','厂商',0,'2026-10-30T19:23:03.766914','https://example.com/n2')").execute(&pool).await.unwrap();
        let notice: SecuritNotice = sqlx::query_as("SELECT * FROM security_notice WHERE id=2")
            .fetch_one(&pool)
            .await
            .unwrap();
        let card = confirmation_card(&ReceiptSource::Notice(notice), false, 1, 0, &[]);
        let content = card["body"]["elements"][0]["content"].as_str().unwrap();
        assert!(
            content.contains("发布日期**：2026-10-30 19:23"),
            "{content}"
        );
    }

    fn callback(user: &str, chat: &str, id: &str) -> CardCallback {
        serde_json::from_value(json!({
            "header": {"event_id": id, "event_type": "card.action.trigger", "app_id": "app", "tenant_key": "tenant"},
            "event": {"operator": {"open_id": user, "tenant_key": "tenant"},
                "context": {"open_message_id": "message", "open_chat_id": chat},
                "action": {"value": {"task_id": 1, "version": 0}, "form_value": {"dimension": "p", "status": "none", "owners": []}}}
        })).unwrap()
    }

    fn dimension_callback(
        user: &str,
        chat: &str,
        id: &str,
        dimension: &str,
        status: &str,
        owners: serde_json::Value,
    ) -> CardCallback {
        serde_json::from_value(json!({
            "header": {"event_id": id, "event_type": "card.action.trigger", "app_id": "app", "tenant_key": "tenant"},
            "event": {"operator": {"open_id": user, "tenant_key": "tenant"},
                "context": {"open_message_id": "message", "open_chat_id": chat},
                "action": {"value": {"task_id": 1, "version": 0}, "form_value": {"dimension": dimension, "status": status, "owners": owners}}}
        })).unwrap()
    }

    #[tokio::test]
    async fn dimensions_submit_and_manage_their_answers() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO admin_user(name,password) VALUES('admin','hash')",
            "INSERT INTO vuln_information(key,title) VALUES('test','test')",
            "INSERT INTO feishu_app(name,app_id,app_secret,tenant_key) VALUES('app','app','secret','')",
            "INSERT INTO feishu_chat(app_id,tenant_key,chat_id,name) VALUES(1,'','chat','群')",
            "",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        let orm = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool.clone());
        receipts::create_case(&orm, "vuln", 1).await.unwrap();
        sqlx::query("INSERT INTO receipt_delivery(case_id,task_id,app_id,tenant_key,chat_id,message_id,request_id,task_version) VALUES(1,1,'app','','chat','message','req',0)").execute(&pool).await.unwrap();
        assert!(matches!(
            handle_action(&orm, callback("alice", "wrong", "bad")).await,
            Err(ReceiptError::Unauthorized)
        ));
        let tenant: String = sqlx::query_scalar("SELECT tenant_key FROM feishu_app")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(tenant.is_empty());
        // P 产线 answers "no impact" (no owners needed).
        handle_action(&orm, callback("alice", "chat", "one"))
            .await
            .unwrap();
        // NovoOne picks one 处置负责人 for the ops track.
        handle_action(
            &orm,
            dimension_callback("bob", "chat", "two", "novo", "ops", json!(["ou_owner1"])),
        )
        .await
        .unwrap();
        // Resubmitting for the same dimension replaces its single answer.
        handle_action(
            &orm,
            dimension_callback(
                "alice",
                "chat",
                "three",
                "p",
                "both",
                json!(["ou_owner1", "ou_owner2"]),
            ),
        )
        .await
        .unwrap();
        let card = submitted_card(&orm, 1).await.unwrap();
        let elements = card["body"]["elements"].as_array().unwrap();
        assert!(elements.iter().any(|item| item["tag"] == "form"));
        let content = elements
            .iter()
            .filter_map(|item| item["content"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(content.contains("已完成 2/3 维度"), "missing stats");
        assert!(content.contains("已规划运维 + 迭代处理"));
        assert!(content.contains("⏳ **运维** — 未回执"));
        let card_text = card.to_string();
        for text in ["alice", "bob", "P 产线", "NovoOne 产线"] {
            assert!(card_text.contains(text), "missing {text}");
        }
        // Unresolved owner names fall back to the raw open_id.
        assert!(card_text.contains("ou_owner1"));
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM receipt_response WHERE deleted=0")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 2);
        let task_status: String = sqlx::query_scalar("SELECT status FROM receipt_task WHERE id=1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(task_status, "in_progress");
        let version: i64 = sqlx::query_scalar(
            "SELECT version FROM receipt_response WHERE dimension='novo' AND deleted=0",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(matches!(
            receipts::manage_response(&orm, 2, version + 1, None).await,
            Err(ReceiptError::Conflict)
        ));
        receipts::manage_response(&orm, 2, version, Some(("Bob", "iter", "", None)))
            .await
            .unwrap();
        receipts::manage_response(&orm, 2, version + 1, None)
            .await
            .unwrap();
        // Bob's row was admin-deleted; only the P 产线 answer remains.
        let names: i64 =
            sqlx::query_scalar("SELECT count(*) FROM receipt_response WHERE deleted=0")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(names, 1);
        let card = submitted_card(&orm, 1).await.unwrap().to_string();
        assert!(!card.contains("✅ **NovoOne 产线**"));
        assert!(card.contains("⏳ **NovoOne 产线**"));
        // The dimension can answer again after an admin delete.
        handle_action(
            &orm,
            dimension_callback(
                "bob",
                "chat",
                "restore",
                "novo",
                "special",
                json!(["ou_owner1"]),
            ),
        )
        .await
        .unwrap();
        let restored: i64 =
            sqlx::query_scalar("SELECT count(*) FROM receipt_response WHERE deleted=0")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(restored, 2);
        let retry = dimension_callback(
            "bob",
            "chat",
            "restore",
            "novo",
            "special",
            json!(["ou_owner1"]),
        );
        handle_action(&orm, retry).await.unwrap();
        let duplicate = dimension_callback("bob", "chat", "restore", "novo", "none", json!([]));
        assert!(matches!(
            handle_action(&orm, duplicate).await,
            Err(ReceiptError::Duplicate)
        ));

        // Owner rules per option; legacy statuses and unknown dimensions are rejected.
        for (dimension, status, owners) in [
            ("p", "none", json!(["ou_owner1"])),
            ("p", "ops", json!([])),
            ("p", "ops", json!(["ou_owner1", "ou_owner2"])),
            ("p", "special", json!([])),
            ("p", "both", json!(["bad-id"])),
            ("unknown", "ops", json!(["ou_owner1"])),
            ("p", "investigating", json!([])),
        ] {
            let event_id = format!("invalid-{dimension}-{status}");
            let bad = dimension_callback("bob", "chat", &event_id, dimension, status, owners);
            assert!(
                matches!(handle_action(&orm, bad).await, Err(ReceiptError::Invalid)),
                "expected Invalid for {dimension}/{status}"
            );
        }
        sqlx::query("UPDATE receipt_delivery SET withdrawn=1 WHERE id=1")
            .execute(&pool)
            .await
            .unwrap();
        assert!(matches!(
            handle_action(&orm, callback("alice", "chat", "withdrawn")).await,
            Err(ReceiptError::Unauthorized)
        ));
        sqlx::query("UPDATE receipt_delivery SET withdrawn=0 WHERE id=1")
            .execute(&pool)
            .await
            .unwrap();
        // 关闭门闸：运维维度 still missing.
        assert!(matches!(
            receipts::close_case(&orm, 1, 1, "done").await,
            Err(ReceiptError::Incomplete(names)) if names.contains("运维")
        ));
        handle_action(
            &orm,
            dimension_callback(
                "zheng",
                "chat",
                "ops-dim",
                "ops",
                "iter",
                json!(["ou_owner1"]),
            ),
        )
        .await
        .unwrap();
        let task_status: String = sqlx::query_scalar("SELECT status FROM receipt_task WHERE id=1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(task_status, "done");
        receipts::close_case(&orm, 1, 1, "done").await.unwrap();
        let card = submitted_card(&orm, 1).await.unwrap();
        assert!(
            !card["body"]["elements"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["tag"] == "form")
        );
        assert!(matches!(
            handle_action(&orm, callback("alice", "chat", "late")).await,
            Err(ReceiptError::Unauthorized)
        ));
    }

    fn delete_callback(user: &str, chat: &str, id: &str) -> CardCallback {
        serde_json::from_value(json!({
            "header": {"event_id": id, "event_type": "card.action.trigger", "app_id": "app", "tenant_key": "tenant"},
            "event": {"operator": {"open_id": user, "tenant_key": "tenant"},
                "context": {"open_message_id": "message", "open_chat_id": chat},
                "action": {"value": {"task_id": 1, "version": 0, "delete": true}}}
        })).unwrap()
    }

    #[tokio::test]
    async fn respondents_can_delete_their_own_answer_when_enabled() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO admin_user(name,password) VALUES('admin','hash')",
            "INSERT INTO vuln_information(key,title) VALUES('test','test')",
            "INSERT INTO feishu_app(name,app_id,app_secret,tenant_key) VALUES('app','app','secret','tenant')",
            "INSERT INTO feishu_chat(app_id,tenant_key,chat_id,name) VALUES(1,'tenant','chat','群')",
            "",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        let orm = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool.clone());
        receipts::create_case(&orm, "vuln", 1).await.unwrap();
        sqlx::query("INSERT INTO receipt_delivery(case_id,task_id,app_id,tenant_key,chat_id,message_id,request_id,task_version) VALUES(1,1,'app','tenant','chat','message','req',0)").execute(&pool).await.unwrap();
        handle_action(&orm, callback("alice", "chat", "submit"))
            .await
            .unwrap();
        // Switch off by default: the delete button's callback is rejected.
        assert!(matches!(
            handle_action(&orm, delete_callback("alice", "chat", "d1")).await,
            Err(ReceiptError::DeletionDisabled)
        ));
        sqlx::query("INSERT INTO sync_task(name,interval_minutes,status,allow_response_deletion) VALUES('sync',15,1,1)")
            .execute(&pool)
            .await
            .unwrap();
        handle_action(&orm, delete_callback("alice", "chat", "d2"))
            .await
            .unwrap();
        let deleted: bool = sqlx::query_scalar(
            "SELECT deleted FROM receipt_response WHERE operator_open_id='alice'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(deleted);

        // The refreshed card no longer shows alice and offers the delete button.
        let card = submitted_card(&orm, 1).await.unwrap().to_string();
        assert!(!card.contains("alice"));
        assert!(card.contains("删除我的回执"));
        // Replaying the same event is a no-op; a second delete has nothing left.
        handle_action(&orm, delete_callback("alice", "chat", "d2"))
            .await
            .unwrap();
        assert!(matches!(
            handle_action(&orm, delete_callback("alice", "chat", "d3")).await,
            Err(ReceiptError::NoResponse)
        ));
    }

    #[tokio::test]
    async fn survey_card_orders_truncates_and_strips_to_markdown() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO vuln_information(key,title) VALUES('test','test')",
            "INSERT INTO feishu_app(name,app_id,app_secret,tenant_key) VALUES('app','app','secret','')",
            "INSERT INTO feishu_chat(app_id,tenant_key,chat_id,name) VALUES(1,'','chat','群')",
            "",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        let orm = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool.clone());
        // Task 1: 22 answers, mixed statuses, straight into receipt_response.
        receipts::create_case(&orm, "vuln", 1).await.unwrap();
        for i in 1..=22 {
            let status = if i == 1 {
                "affected"
            } else if i == 2 {
                "investigating"
            } else {
                "unaffected"
            };
            sqlx::query("INSERT INTO receipt_response(task_id,app_id,tenant_key,operator_open_id,status,details) VALUES(1,'app','',?,?,'说明')")
                .bind(format!("user{i}"))
                .bind(status)
                .execute(&pool)
                .await
                .unwrap();
        }
        // (Legacy member-cache rows: the display path no longer reads them.)
        for (open_id, name) in [
            ("user1", "张三"),
            ("user2", "李四"),
            ("user3", ""),
            ("ou_pending", "王二"),
        ] {
            sqlx::query("INSERT INTO feishu_chat_member(chat_id,open_id,name) VALUES(1,?,?)")
                .bind(open_id)
                .bind(name)
                .execute(&pool)
                .await
                .unwrap();
        }
        let card = submitted_card(&orm, 1).await.unwrap();
        let elements = card["body"]["elements"].as_array().unwrap();
        let table = elements
            .iter()
            .find(|item| item["tag"] == "table")
            .expect("survey table");
        let rows = table["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 20);
        // Legacy individual rows keep readable labels without emoji status;
        // newest first (user1/user2 fall outside the 20-row window).
        assert_eq!(rows[0]["member"], "user22");
        assert_eq!(rows[0]["dimension"], "—（旧个人回执）");
        assert_eq!(rows[0]["status"], "无影响");
        assert_eq!(rows[19]["member"], "user3");
        let card_text = card.to_string();
        // No dimension has answered: every dimension shows as pending.
        assert!(card_text.contains("已完成 0/3 维度"));
        assert!(card_text.contains("⏳ **运维** — 未回执"));
        assert!(!card_text.contains("✅"));
        // Task 2: no answers at all — dimensions pending, no table.
        sqlx::query("INSERT INTO vuln_information(key,title) VALUES('test2','test2')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO receipt_case(source_type,source_id,title) VALUES('vuln',2,'empty')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO receipt_task(case_id) VALUES(2)")
            .execute(&pool)
            .await
            .unwrap();
        let empty = submitted_card(&orm, 2).await.unwrap();
        assert!(empty.to_string().contains("⏳ **P 产线** — 未回执"));
        assert!(
            empty["body"]["elements"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["tag"] != "table")
        );
        // Fallback view keeps every committed answer and the form entry.
        let plain = strip_table(&card);
        let plain_text = plain.to_string();
        assert!(!plain_text.contains("\"tag\":\"table\""));
        assert!(plain_text.contains("**回执明细**"));
        assert!(plain_text.contains("user22"));
        assert!(plain_text.contains("无影响"));
        assert!(
            plain["body"]["elements"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["tag"] == "form")
        );
    }
}
