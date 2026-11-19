use async_trait::async_trait;
use error_stack::ResultExt;
use mea::mpsc::UnboundedSender;
use reqwest::header;
use serde::{Deserialize, Serialize};

use crate::{
    AppResult,
    domain::models::vuln_information::{CreateVulnInformation, Severity},
    errors::Error,
    output::plugins::vuln::{VulnPlugin, register_plugin},
    utils::http_client::HttpClient,
};

const LIST_URL: &str = "https://stack.chaitin.com/api/v2/vuln/list/";
const LINK: &str = "https://stack.chaitin.com/vuldb/index";
/// The API caps page_size at 15 (larger values are silently ignored).
const PAGE_SIZE: i32 = 15;
/// Out-of-range pages echo page 1's data instead of erroring, so never walk
/// far: the list is curated (one page spans weeks) and the worker dedupes by
/// key anyway.
const MAX_PAGES: i32 = 5;

#[derive(Debug, Clone)]
pub struct ChaitinPlugin {
    name: String,
    display_name: String,
    link: String,
    http_client: HttpClient,
    sender: UnboundedSender<CreateVulnInformation>,
}

#[async_trait]
impl VulnPlugin for ChaitinPlugin {
    fn get_name(&self) -> String {
        self.name.to_string()
    }

    fn get_display_name(&self) -> String {
        self.display_name.to_string()
    }

    fn get_link(&self) -> String {
        self.link.to_string()
    }

    async fn update(&self, page_limit: i32) -> AppResult<()> {
        for page in 1..=page_limit.clamp(1, MAX_PAGES) {
            let url = format!("{LIST_URL}?page={page}&page_size={PAGE_SIZE}");
            let resp: ChaitinListResp = self
                .http_client
                .get_json(&url)
                .await
                .map_err(|e| {
                    Error::Message(format!("chaitin vuldb page {page} request failed: {e}"))
                })?
                .json()
                .await
                .map_err(|e| {
                    Error::Message(format!("Failed to parse chaitin vuldb response: {e}"))
                })?;
            if resp.code != 0 {
                return Err(Error::Message(format!(
                    "chaitin vuldb returned error {}: {}",
                    resp.code, resp.msg
                ))
                .into());
            }
            let has_more = resp.data.next.is_some();
            for v in resp.data.list {
                let data = self.parse_vuln(v);
                self.sender.send(data).change_context_lazy(|| {
                    Error::Message("Failed to send vuln information to channel".to_string())
                })?;
            }
            if !has_more {
                break;
            }
        }
        Ok(())
    }
}

impl ChaitinPlugin {
    pub fn try_new(sender: UnboundedSender<CreateVulnInformation>) -> AppResult<ChaitinPlugin> {
        let mut headers: reqwest::header::HeaderMap = header::HeaderMap::new();
        headers.insert("Referer", header::HeaderValue::from_static(LINK));
        headers.insert(
            "Origin",
            header::HeaderValue::from_static("https://stack.chaitin.com"),
        );
        headers.insert(
            "Accept",
            header::HeaderValue::from_static("application/json"),
        );
        let http_client = HttpClient::try_new_sync_with_headers(headers)?;
        let chaitin = ChaitinPlugin {
            name: "ChaitinPlugin".to_string(),
            display_name: "长亭安全应急响应中心-漏洞库".to_string(),
            link: LINK.to_string(),
            http_client,
            sender,
        };
        register_plugin(chaitin.name.clone(), Box::new(chaitin.clone()));
        Ok(chaitin)
    }

    pub fn get_severity(&self, detail_severity: &str) -> Severity {
        match detail_severity {
            "critical" => Severity::Critical,
            "high" => Severity::High,
            "medium" => Severity::Medium,
            _ => Severity::Low,
        }
    }

    /// Map one vuldb list entry onto the CreateVulnInformation record.
    pub fn parse_vuln(&self, v: ChaitinVuln) -> CreateVulnInformation {
        let mut tags = Vec::new();
        if let Some(weakness) = v.weakness.as_deref().filter(|w| !w.trim().is_empty()) {
            tags.push(weakness_label(weakness).to_string());
        }
        if is_date_set(&v.poc_disclosure_date) {
            tags.push("有PoC".to_string());
        }
        if is_date_set(&v.exp_disclosure_date) {
            tags.push("有EXP".to_string());
        }
        let reference_links = v
            .references
            .as_deref()
            .unwrap_or_default()
            .split('\n')
            .map(str::trim)
            .filter(|link| !link.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        let key = v
            .ct_id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| v.id.clone());
        CreateVulnInformation {
            key,
            title: v.title,
            description: v.summary.unwrap_or_default(),
            severity: self.get_severity(&v.severity).to_string(),
            cve: v.cve_id.unwrap_or_default(),
            disclosure: v.disclosure_date.unwrap_or_default(),
            reference_links: reference_links.into(),
            solutions: v.fix_steps.unwrap_or_default(),
            source: LINK.to_string(),
            source_name: self.name.to_string(),
            tags: tags.into(),
            reasons: Vec::new().into(),
            github_search: vec![].into(),
            pushed: false,
            detail_link: format!("https://stack.chaitin.com/vuldb/detail/{}", v.id),
        }
    }
}

fn is_date_set(date: &Option<String>) -> bool {
    date.as_deref()
        .map(|d| !d.trim().is_empty())
        .unwrap_or(false)
}

/// Chinese label for the weakness machine name (mapping from the Chaitin
/// vuldb frontend); unknown names pass through unchanged.
fn weakness_label(weakness: &str) -> &str {
    match weakness {
        "backdoor" => "后门",
        "buffer_overflow" => "缓冲区溢出",
        "click_jacking" => "点击劫持",
        "command_injection" => "远程命令执行",
        "cors" => "跨域",
        "crlf" => "CRLF注入",
        "cryptography_problem" => "密码学问题",
        "csrf" => "CSRF",
        "default_passord" => "默认账户口令",
        "deserialization" => "反序列化漏洞",
        "directory_traversal" => "路径遍历",
        "dos" => "拒绝服务",
        "double_free" => "双重释放",
        "enum" => "枚举",
        "file_delete" => "文件删除",
        "file_inclusion" => "文件包含",
        "file_upload" => "文件上传",
        "heap_overflow" => "堆溢出",
        "idor" => "鉴权绕过",
        "integer_overflow" => "整数溢出",
        "ldap_injection" => "LDAP注入",
        "leak" => "敏感信息泄露",
        "logical" => "逻辑漏洞",
        "memory_safety" => "内存安全",
        "other" => "其他",
        "out_of_bounds" => "越界访问",
        "race_condition" => "条件竞争",
        "rce" => "远程代码执行",
        "readfile" => "文件读取",
        "rpo" => "相对路径覆盖",
        "sql_injection" => "SQL注入",
        "ssrf" => "SSRF",
        "ssti" => "SSTI",
        "stack_overflow" => "栈溢出",
        "type_confusion" => "类型混淆",
        "unauthorized_access" => "未授权访问",
        "uncontrolled_format_string" => "格式化字符串",
        "uninitialized_variable" => "未初始化变量",
        "unsafe_account" => "账户认证体系漏洞",
        "unsafe_config" => "不安全的配置",
        "url_redirection" => "URL跳转",
        "use_after_free" => "释放后重引用",
        "weak_password" => "弱口令",
        "writefile" => "文件写入",
        "xpath_injection" => "XPath注入",
        "xss" => "XSS",
        "xxe" => "XXE",
        other => other,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaitinListResp {
    pub code: i64,
    pub msg: String,
    pub data: ChaitinListData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaitinListData {
    pub count: i64,
    pub next: Option<String>,
    pub previous: Option<String>,
    pub list: Vec<ChaitinVuln>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaitinVuln {
    pub id: String,
    pub title: String,
    pub summary: Option<String>,
    pub weakness: Option<String>,
    pub severity: String,
    pub ct_id: Option<String>,
    pub cve_id: Option<String>,
    pub fix_steps: Option<String>,
    /// Newline-separated reference URLs.
    pub references: Option<String>,
    pub disclosure_date: Option<String>,
    pub poc_disclosure_date: Option<String>,
    pub exp_disclosure_date: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mea::mpsc::unbounded;

    fn plugin() -> ChaitinPlugin {
        let (sender, _receiver) = unbounded::<CreateVulnInformation>();
        ChaitinPlugin::try_new(sender).expect("Failed to create ChaitinPlugin")
    }

    #[test]
    fn parses_list_response_and_maps_fields() {
        let plugin = plugin();
        let body = r#"{"msg":"success","code":0,"data":{"count":249,"next":null,"previous":null,"list":[
            {"id":"ad95b374-47e5-43c6-9671-00e8c0f8803b","title":"nginx 邮件代理认证释放后使用远程代码执行漏洞",
             "summary":"攻击者在无需认证的情况下可触发会话释放后使用。","weakness":"use_after_free",
             "severity":"high","ct_id":"CT-6315626","cve_id":"CVE-2026-113425",
             "fix_steps":"升级至 1.31.9 或更高版本。","references":"https://nginx.org/en/security_advisories.html\n\nhttps://my.f5.com/ ",
             "disclosure_date":"2026-11-18","poc_disclosure_date":"2026-11-19","exp_disclosure_date":null}
        ]}}"#;
        let mut resp: ChaitinListResp =
            serde_json::from_str(body).expect("failed to parse vuldb response");
        assert_eq!(resp.code, 0);
        assert!(resp.data.next.is_none());

        let item = plugin.parse_vuln(resp.data.list.remove(0));
        assert_eq!(item.key, "CT-6315626");
        assert_eq!(item.title, "nginx 邮件代理认证释放后使用远程代码执行漏洞");
        assert_eq!(item.severity, "High");
        assert_eq!(item.cve, "CVE-2026-113425");
        assert_eq!(item.disclosure, "2026-11-18");
        assert_eq!(item.solutions, "升级至 1.31.9 或更高版本。");
        assert_eq!(
            item.detail_link,
            "https://stack.chaitin.com/vuldb/detail/ad95b374-47e5-43c6-9671-00e8c0f8803b"
        );
        // Blank lines and surrounding whitespace in references are dropped.
        assert_eq!(
            Vec::<String>::from(item.reference_links),
            vec![
                "https://nginx.org/en/security_advisories.html",
                "https://my.f5.com/"
            ]
        );
        // Weakness label plus PoC presence become tags; exp is unset.
        assert_eq!(
            Vec::<String>::from(item.tags),
            vec!["释放后重引用", "有PoC"]
        );
    }

    #[test]
    fn falls_back_to_uuid_key_and_maps_severities() {
        let plugin = plugin();
        for (raw, expected) in [
            ("critical", "Critical"),
            ("high", "High"),
            ("medium", "Medium"),
            ("low", "Low"),
            ("unknown", "Low"),
        ] {
            let item = plugin.parse_vuln(ChaitinVuln {
                id: "00000000-0000-0000-0000-000000000001".to_string(),
                title: "测试漏洞".to_string(),
                summary: None,
                weakness: None,
                severity: raw.to_string(),
                ct_id: None,
                cve_id: None,
                fix_steps: None,
                references: None,
                disclosure_date: None,
                poc_disclosure_date: None,
                exp_disclosure_date: None,
            });
            assert_eq!(item.severity, expected, "severity mapping for {raw}");
            assert_eq!(item.key, "00000000-0000-0000-0000-000000000001");
            assert!(Vec::<String>::from(item.tags).is_empty());
        }
    }
}
