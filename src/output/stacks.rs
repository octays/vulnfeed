//! Tech-stack classification and subscription routing.
//!
//! A stack is a dictionary row (slug + keywords). Items are classified at
//! ingest by case-insensitive substring matching against the concatenated
//! title/description/tags/key text; channels and receipt chats subscribe to
//! slugs — an empty subscription receives everything, and an item with no
//! stack only reaches full-receive targets.
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement};
use serde_json::Value;

pub struct StackDict {
    pub slug: String,
    pub keywords: Vec<String>,
}

/// Whether a target subscribed to `subscription` (comma-separated slugs,
/// empty = receive everything) should receive an item tagged `item_stacks`
/// (comma-separated slugs; empty = unclassified, full-receive targets only).
pub fn stacks_allow(subscription: &str, item_stacks: &str) -> bool {
    let subscribed: Vec<&str> = subscription
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if subscribed.is_empty() {
        return true;
    }
    let item: Vec<&str> = item_stacks
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if item.is_empty() {
        return false;
    }
    item.iter()
        .any(|stack| subscribed.iter().any(|sub| sub.eq_ignore_ascii_case(stack)))
}

/// Classify `text` against the dictionary; slugs in dictionary order.
pub fn classify(text: &str, dict: &[StackDict]) -> Vec<String> {
    let lower = text.to_lowercase();
    dict.iter()
        .filter(|stack| {
            stack
                .keywords
                .iter()
                .any(|keyword| !keyword.is_empty() && lower.contains(&keyword.to_lowercase()))
        })
        .map(|stack| stack.slug.clone())
        .collect()
}

/// Parse the model's answer into dictionary-valid slugs, preserving dict
/// order. Accepts raw JSON, fenced ```json blocks, and stray text around it.
pub fn parse_llm_stacks(content: &str, dict: &[StackDict]) -> Vec<String> {
    let Some(start) = content.find('{') else {
        return Vec::new();
    };
    let Some(end) = content.rfind('}') else {
        return Vec::new();
    };
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&content[start..=end]) else {
        return Vec::new();
    };
    let Some(Value::Array(items)) = map.get("stacks") else {
        return Vec::new();
    };
    let answered: Vec<String> = items
        .iter()
        .filter_map(|item| item.as_str().map(str::trim))
        .filter(|slug| !slug.is_empty())
        .map(str::to_owned)
        .collect();
    dict.iter()
        .filter(|stack| {
            answered
                .iter()
                .any(|slug| slug.eq_ignore_ascii_case(&stack.slug))
        })
        .map(|stack| stack.slug.clone())
        .collect()
}

fn llm_system_prompt(dict: &[StackDict]) -> String {
    let lines = dict
        .iter()
        .map(|stack| format!("- {}: {}", stack.slug, stack.keywords.join(",")))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "你是漏洞情报分类助手。判断漏洞实际影响哪些技术栈。\n可选技术栈（只能从中选择 slug）：\n{lines}\n规则：\n1. 只选择漏洞受影响组件/产品真实所属的技术栈生态；描述里顺带提及的词不算（例如 PHP 漏洞的描述里提到 Windows 不代表影响 windows 栈）。\n2. 无法明确判断归属时返回空数组。\n3. 只输出 JSON，格式：{{\"stacks\":[\"slug1\",\"slug2\"]}}，不要输出其他内容。"
    )
}

fn llm_user_prompt(text: &str) -> String {
    format!(
        "漏洞信息：\n{}",
        text.chars().take(4000).collect::<String>()
    )
}

/// Ask the model to classify; any failure maps to None (caller keeps the
/// keyword result).
pub async fn classify_with_llm(
    client: &crate::output::llm::LlmClient,
    text: &str,
    dict: &[StackDict],
) -> Option<Vec<String>> {
    let content = match client
        .complete(&llm_system_prompt(dict), &llm_user_prompt(text))
        .await
    {
        Ok(content) => content,
        Err(e) => {
            log::warn!("LLM stack classification failed, keeping keyword result: {e}");
            return None;
        }
    };
    let stacks = parse_llm_stacks(&content, dict);
    Some(stacks)
}

pub async fn load_dict(db: &DatabaseConnection) -> Result<Vec<StackDict>, DbErr> {
    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT slug,keywords FROM tech_stack WHERE enabled=1 ORDER BY id",
        ))
        .await?;
    rows.iter()
        .map(|row| {
            let slug: String = row.try_get("", "slug")?;
            let keywords: String = row.try_get("", "keywords")?;
            Ok(StackDict {
                slug,
                keywords: keywords
                    .split([',', '，', '\n', '\r'])
                    .map(str::trim)
                    .filter(|k| !k.is_empty())
                    .map(str::to_owned)
                    .collect(),
            })
        })
        .collect()
}

/// Classify one stored item (vuln or notice) and persist its stacks.
/// When an LLM client is supplied it runs after the keyword pass and its
/// answer (possibly empty) wins; the source column records which pass did it.
pub async fn tag_item(
    db: &DatabaseConnection,
    source_type: &str,
    source_id: i64,
    dict: &[StackDict],
    llm: Option<&crate::output::llm::LlmClient>,
) -> Result<(), DbErr> {
    let (text_sql, update_sql, source_update_sql) = match source_type {
        "vuln" => (
            "SELECT title,description,tags,key,cve FROM vuln_information WHERE id=?",
            "UPDATE vuln_information SET stacks=? WHERE id=?",
            Some("UPDATE vuln_information SET stacks_source=? WHERE id=?"),
        ),
        _ => (
            "SELECT title,product_name,key FROM security_notice WHERE id=?",
            "UPDATE security_notice SET stacks=? WHERE id=?",
            Some("UPDATE security_notice SET stacks_source=? WHERE id=?"),
        ),
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            text_sql,
            [source_id.into()],
        ))
        .await?;
    let Some(row) = row else { return Ok(()) };
    let mut text = String::new();
    for column in ["title", "description", "product_name", "tags", "key", "cve"] {
        if let Ok(value) = row.try_get::<String>("", column) {
            text.push_str(&value);
            text.push('\n');
        }
    }
    let (mut stacks, mut source) = (classify(&text, dict), "kw");
    if let Some(client) = llm
        && let Some(llm_stacks) = classify_with_llm(client, &text, dict).await
    {
        stacks = llm_stacks;
        source = "llm";
    }
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        update_sql,
        [stacks.join(",").into(), source_id.into()],
    ))
    .await?;
    if let Some(source_update) = source_update_sql {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            source_update,
            [source.into(), source_id.into()],
        ))
        .await?;
    }
    Ok(())
}

/// Backfill classification for rows stored before the feature existed.
pub async fn backfill(db: &DatabaseConnection) {
    backfill_with(db, None).await
}

/// Re-run classification over stored rows; `only` filters which rows:
/// "empty" = never classified, "keyword" = classified by keyword pass only,
/// "all" = everything. Used by both boot backfill (empty) and the admin
/// reclassify action.
pub async fn reclassify(
    db: &DatabaseConnection,
    llm: Option<&crate::output::llm::LlmClient>,
    only: &str,
) -> Result<(u32, u32), DbErr> {
    let dict = load_dict(db).await?;
    if dict.is_empty() {
        return Ok((0, 0));
    }
    let mut processed = 0u32;
    let mut updated = 0u32;
    for source_type in ["vuln", "notice"] {
        let table = if source_type == "vuln" {
            "vuln_information"
        } else {
            "security_notice"
        };
        let filter = match only {
            "all" => String::new(),
            "keyword" => " WHERE stacks_source<>'llm' OR stacks_source IS NULL".to_string(),
            _ => " WHERE stacks=''".to_string(),
        };
        let sql = format!("SELECT id FROM {table}{filter}");
        let rows = db
            .query_all(Statement::from_string(DatabaseBackend::Sqlite, sql))
            .await?;
        for row in rows {
            let id: i64 = row.try_get("", "id").unwrap_or_default();
            if id <= 0 {
                continue;
            }
            let before = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    format!("SELECT stacks FROM {table} WHERE id=?").as_str(),
                    [id.into()],
                ))
                .await?
                .and_then(|r| r.try_get::<String>("", "stacks").ok())
                .unwrap_or_default();
            tag_item(db, source_type, id, &dict, llm).await?;
            let after = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    format!("SELECT stacks FROM {table} WHERE id=?").as_str(),
                    [id.into()],
                ))
                .await?
                .and_then(|r| r.try_get::<String>("", "stacks").ok())
                .unwrap_or_default();
            processed += 1;
            if before != after {
                updated += 1;
            }
        }
    }
    Ok((processed, updated))
}

async fn backfill_with(db: &DatabaseConnection, llm: Option<&crate::output::llm::LlmClient>) {
    let dict = match load_dict(db).await {
        Ok(dict) if !dict.is_empty() => dict,
        Ok(_) => return,
        Err(e) => {
            log::warn!("tech-stack dictionary load failed, backfill skipped: {e}");
            return;
        }
    };
    for source_type in ["vuln", "notice"] {
        let sql = if source_type == "vuln" {
            "SELECT id FROM vuln_information WHERE stacks=''"
        } else {
            "SELECT id FROM security_notice WHERE stacks=''"
        };
        let rows = match db
            .query_all(Statement::from_string(DatabaseBackend::Sqlite, sql))
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                log::warn!("tech-stack backfill select failed ({source_type}): {e}");
                continue;
            }
        };
        let mut count = 0u32;
        for row in rows {
            let id: i64 = row.try_get("", "id").unwrap_or_default();
            if id > 0 && tag_item(db, source_type, id, &dict, llm).await.is_ok() {
                count += 1;
            }
        }
        if count > 0 {
            log::info!("tech-stack backfill tagged {count} {source_type} rows");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> Vec<StackDict> {
        vec![
            StackDict {
                slug: "php".into(),
                keywords: vec!["php".into(), "laravel".into(), "thinkphp".into()],
            },
            StackDict {
                slug: "go".into(),
                keywords: vec!["golang".into(), "k8s.io".into()],
            },
            StackDict {
                slug: "rust".into(),
                keywords: vec!["rust".into(), "cargo".into()],
            },
        ]
    }

    #[test]
    fn classifies_case_insensitive_multi_word_and_multi_stack() {
        let stacks = classify(
            "ThinkPHP 远程代码执行，影响 Go 应用（golang.org/x/net）",
            &dict(),
        );
        assert_eq!(stacks, vec!["php".to_owned(), "go".to_owned()]);
        assert!(classify("Rust cargo 供应链漏洞", &dict()).contains(&"rust".to_owned()));
        assert!(classify("Apache HTTPD 路径穿越", &dict()).is_empty());
    }

    #[test]
    fn llm_answer_parsing_filters_to_dictionary() {
        let dict = dict();
        // Raw JSON.
        let stacks = parse_llm_stacks(r#"{"stacks":["php","go"]}"#, &dict);
        assert_eq!(stacks, vec!["php".to_owned(), "go".to_owned()]);
        // Fenced block with prose around it.
        let content =
            "结果如下：\n```json\n{\"stacks\": [\"RUST\", \"php\", \"not-a-stack\"]}\n```";
        assert_eq!(
            parse_llm_stacks(content, &dict),
            vec!["php".to_owned(), "rust".to_owned()]
        );
        // Empty answer is valid (genuinely no stack).
        assert!(parse_llm_stacks("{\"stacks\":[]}", &dict).is_empty());
        // Garbage maps to empty — caller keeps the keyword result.
        assert!(parse_llm_stacks("I cannot answer", &dict).is_empty());
        assert!(parse_llm_stacks("{\"no_stacks_key\":1}", &dict).is_empty());
    }

    #[test]
    fn subscription_semantics() {
        // Empty subscription receives everything.
        assert!(stacks_allow("", "php,go"));
        assert!(stacks_allow("", ""));
        // Subscribed targets only receive intersecting or... unclassified is rejected.
        assert!(stacks_allow("php,rust", "php"));
        assert!(!stacks_allow("php,rust", "go"));
        assert!(!stacks_allow("php,rust", ""));
        // Case-insensitive slug comparison.
        assert!(stacks_allow("PHP", "php"));
    }
}
