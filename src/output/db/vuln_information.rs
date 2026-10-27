use sea_query::Value;
use sqlx::{Sqlite, Transaction};

use crate::{
    AppResult,
    domain::models::{
        page_utils::PageFilter,
        vuln_information::{CreateVulnInformation, SearchParams, VulnInformation},
    },
    output::db::base::{
        Dao, DaoQueryBuilder, dao_create, dao_fetch_by_column, dao_fetch_by_id, dao_update,
        dao_update_field,
    },
};

const REASON_NEW_CREATED: &str = "漏洞创建";
const REASON_TAG_UPDATED: &str = "标签更新";
const REASON_SEVERITY_UPDATE: &str = "等级更新";

pub struct VulnInformationDao;

impl Dao for VulnInformationDao {
    const TABLE: &'static str = "vuln_information";
}

impl VulnInformationDao {
    pub async fn update_pushed(
        tx: &mut Transaction<'_, Sqlite>,
        id: i64,
        status: bool,
    ) -> AppResult<u64> {
        let row = dao_update_field::<Self>(tx, id, "pushed", Value::Bool(Some(status))).await?;
        Ok(row)
    }

    pub async fn create(
        tx: &mut Transaction<'_, Sqlite>,
        req: CreateVulnInformation,
    ) -> AppResult<i64> {
        let id = dao_create::<Self, _>(tx, req).await?;
        Ok(id)
    }

    /// 检查漏洞严重性是否发生变化，如果变化则记录日志并添加变更原因
    fn check_severity_update(vuln: &VulnInformation, req: &mut CreateVulnInformation) -> bool {
        let severity = vuln.severity.to_string();
        if severity != req.severity {
            log::info!(
                "{} from {} change severity from {} to {}",
                vuln.title.as_str(),
                vuln.source.as_str(),
                vuln.severity.as_str(),
                req.severity.as_str()
            );
            let reason = format!(
                "{}: {} => {}",
                REASON_SEVERITY_UPDATE,
                vuln.severity.as_str(),
                req.severity
            );
            req.reasons.push(reason);
            true
        } else {
            false
        }
    }

    /// 检查漏洞标签是否发生变化，如果变化则记录日志并添加变更原因
    fn check_tag_update(vuln: &VulnInformation, req: &mut CreateVulnInformation) -> bool {
        let new_tags = req
            .tags
            .iter()
            .filter(|&x| !vuln.tags.contains(x))
            .collect::<Vec<_>>();
        if !new_tags.is_empty() {
            log::info!(
                "{} from {} add new tag {:?}",
                vuln.title,
                vuln.source,
                new_tags
            );
            let reason = format!("{}: {:?} => {:?}", REASON_TAG_UPDATED, vuln.tags, req.tags);
            req.reasons.push(reason);
            true
        } else {
            false
        }
    }

    /// 跨源同 CVE 合并:保留首见源的 key/标题/链接/来源与推送状态,
    /// 严重度取更高者,tags/reasons 取并集
    fn merge_same_cve(existing: &VulnInformation, req: &mut CreateVulnInformation) {
        let rank = |s: &str| match s {
            "Critical" => 4,
            "High" => 3,
            "Medium" => 2,
            _ => 1,
        };
        if rank(&existing.severity) > rank(&req.severity) {
            req.severity = existing.severity.clone();
        }
        let mut tags: Vec<String> = existing.tags.clone().into();
        for t in Vec::<String>::from(req.tags.clone()) {
            if !tags.contains(&t) {
                tags.push(t);
            }
        }
        req.tags = tags.into();
        let mut reasons: Vec<String> = existing.reasons.clone().into();
        for r in Vec::<String>::from(req.reasons.clone()) {
            if !reasons.contains(&r) {
                reasons.push(r);
            }
        }
        req.reasons = reasons.into();
        req.key = existing.key.clone();
        req.title = existing.title.clone();
        req.description = existing.description.clone();
        req.disclosure = existing.disclosure.clone();
        req.solutions = existing.solutions.clone();
        req.reference_links = existing.reference_links.clone();
        req.source = existing.source.clone();
        req.source_name = existing.source_name.clone();
        req.detail_link = existing.detail_link.clone();
        req.pushed = existing.pushed;
    }

    pub async fn create_or_update(
        tx: &mut Transaction<'_, Sqlite>,
        mut req: CreateVulnInformation,
    ) -> AppResult<(i64, bool)> {
        // 二级去重:key 未命中且同 CVE 已有记录时并入已有行,不新建、不触发推送
        let by_key = dao_fetch_by_column::<Self, VulnInformation>(tx, "key", &req.key).await?;
        if by_key.is_none()
            && !req.cve.is_empty()
            && let Some(vuln) =
                dao_fetch_by_column::<Self, VulnInformation>(tx, "cve", &req.cve).await?
        {
            let incoming_source = req.source.clone();
            Self::merge_same_cve(&vuln, &mut req);
            dao_update::<Self, _>(tx, vuln.id, req).await?;
            log::info!(
                "Merged source {} record into existing CVE {} (row {})",
                incoming_source,
                vuln.cve,
                vuln.id
            );
            return Ok((vuln.id, false));
        }
        let mut as_new_vuln = false;
        if let Some(vuln) =
            dao_fetch_by_column::<Self, VulnInformation>(tx, "key", &req.key).await?
        {
            as_new_vuln |= VulnInformationDao::check_severity_update(&vuln, &mut req);
            as_new_vuln |= VulnInformationDao::check_tag_update(&vuln, &mut req);
            if as_new_vuln {
                req.pushed = false;
                dao_update::<Self, _>(tx, vuln.id, req).await?;
            } else {
                log::warn!("Vuln information already exists: {}", req.key);
            }
            Ok((vuln.id, as_new_vuln))
        } else {
            as_new_vuln = true;
            log::info!("New vulnerability created: {}", req.key);
            req.reasons.push(REASON_NEW_CREATED.to_string());
            let id = dao_create::<Self, _>(tx, req).await?;
            Ok((id, as_new_vuln))
        }
    }

    pub async fn filter_vulnfusion_information(
        tx: &mut Transaction<'_, Sqlite>,
        page_filter: &PageFilter,
        search_params: &SearchParams,
    ) -> AppResult<Vec<VulnInformation>> {
        let mut query_builder = DaoQueryBuilder::<Self>::new();

        if let Some(title) = &search_params.title {
            query_builder = query_builder.and_where_like("title", title);
        }

        if let Some(source_name) = &search_params.source_name {
            query_builder = query_builder.and_where_like("source_name", source_name);
        }

        if let Some(pushed) = &search_params.pushed {
            query_builder = query_builder.and_where_bool("pushed", *pushed);
        }

        if let Some(cve) = &search_params.cve {
            query_builder = query_builder.and_where_like("cve", cve);
        }

        let page_no = *page_filter.page_no().as_ref();
        let page_size = *page_filter.page_size().as_ref();
        let offset = (page_no - 1) * page_size;
        query_builder
            .order_by_desc("updated_at")
            .limit_offset(page_size as i64, offset as i64)
            .fetch_all(tx)
            .await
    }

    pub async fn filter_vulnfusion_information_count(
        tx: &mut Transaction<'_, Sqlite>,
        search_params: &SearchParams,
    ) -> AppResult<i64> {
        let mut query_builder = DaoQueryBuilder::<Self>::new();

        if let Some(title) = &search_params.title {
            query_builder = query_builder.and_where_like("title", title);
        }

        if let Some(source_name) = &search_params.source_name {
            query_builder = query_builder.and_where_like("source_name", source_name);
        }

        if let Some(pushed) = &search_params.pushed {
            query_builder = query_builder.and_where_bool("pushed", *pushed);
        }

        if let Some(cve) = &search_params.cve {
            query_builder = query_builder.and_where_like("cve", cve);
        }

        query_builder.count(tx).await
    }

    pub async fn fetch_by_id(
        tx: &mut Transaction<'_, Sqlite>,
        id: i64,
    ) -> AppResult<Option<VulnInformation>> {
        dao_fetch_by_id::<Self, VulnInformation>(tx, id).await
    }

    pub async fn fetch_by_key(
        tx: &mut Transaction<'_, Sqlite>,
        key: &str,
    ) -> AppResult<Option<VulnInformation>> {
        dao_fetch_by_column::<Self, VulnInformation>(tx, "key", key).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup() -> sqlx::SqlitePool {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");
        pool
    }

    #[allow(clippy::too_many_lines)]
    fn make_req(key: &str, cve: &str, severity: &str, tags: &[&str]) -> CreateVulnInformation {
        CreateVulnInformation {
            key: key.to_string(),
            title: format!("title-{key}"),
            description: "desc".to_string(),
            severity: severity.to_string(),
            cve: cve.to_string(),
            disclosure: String::new(),
            solutions: String::new(),
            reference_links: Vec::new().into(),
            tags: tags
                .iter()
                .map(|t| t.to_string())
                .collect::<Vec<_>>()
                .into(),
            github_search: Vec::new().into(),
            source: format!("src-{key}"),
            source_name: format!("src-{key}"),
            reasons: vec!["r1".to_string()].into(),
            pushed: false,
            detail_link: format!("https://example.com/{key}"),
        }
    }

    #[tokio::test]
    async fn same_cve_from_two_sources_merges_into_one_row() {
        let pool = setup().await;
        let mut tx = pool.begin().await.expect("begin tx");
        let (id1, is_new1) = VulnInformationDao::create_or_update(
            &mut tx,
            make_req("k-avd", "CVE-2026-1111", "Medium", &["tagA"]),
        )
        .await
        .expect("first create");
        let (id2, is_new2) = VulnInformationDao::create_or_update(
            &mut tx,
            make_req("k-ti", "CVE-2026-1111", "Critical", &["tagB"]),
        )
        .await
        .expect("second create");
        assert!(is_new1);
        assert!(!is_new2, "同 CVE 二源应并入而非新建");
        assert_eq!(id1, id2);
        let row = VulnInformationDao::fetch_by_id(&mut tx, id1)
            .await
            .expect("fetch")
            .expect("row exists");
        assert_eq!(row.severity, "Critical", "合并保留更严重等级");
        let tags: Vec<String> = row.tags.clone().into();
        assert!(tags.contains(&"tagA".to_string()) && tags.contains(&"tagB".to_string()));
        assert_eq!(row.key, "k-avd", "首见源 key 保留");
        tx.commit().await.expect("commit");
    }

    #[tokio::test]
    async fn same_key_resync_keeps_original_path() {
        let pool = setup().await;
        let mut tx = pool.begin().await.expect("begin tx");
        let r = make_req("k-x", "CVE-2026-2222", "High", &["t"]);
        let (id1, n1) = VulnInformationDao::create_or_update(&mut tx, r.clone())
            .await
            .expect("first");
        let (id2, n2) = VulnInformationDao::create_or_update(&mut tx, r)
            .await
            .expect("second");
        assert!(n1);
        assert!(!n2, "同 key 重同步不应视为新");
        assert_eq!(id1, id2);
        tx.commit().await.expect("commit");
    }
}
