use std::sync::Arc;
use tokio::task::JoinSet;

use async_trait::async_trait;
use dashmap::DashMap;
use lazy_static::lazy_static;
use serde_json::Value;

use crate::{
    AppResult,
    domain::models::{
        push_channel::PushChannel, security_notice::SecuritNotice,
        vuln_information::VulnInformation,
    },
    errors::Error,
    utils::util::render_string,
};

pub mod ding_bot;
pub mod feishu_bot;
pub mod wecom_bot;

lazy_static! {
    static ref PLUGINS: Arc<DashMap<String, Box<dyn MessageBot>>> = Arc::new(DashMap::new());
}

#[allow(clippy::double_must_use)]
#[async_trait]
pub trait MessageBot: Send + Sync {
    async fn push_markdown(&self, title: String, msg: String) -> AppResult<()>;
}

const VULN_INFO_MSG_TEMPLATE: &str = r####"
# {{ title }}

- CVE编号: {% if cve %} {{ cve }}{% else %}暂无 {% endif %}
- 危害定级: **{{ severity }}**
- 漏洞标签: {{ tags | join(sep=" ") }}
- 披露日期: **{{ disclosure }}**
- 推送原因: {{ reasons | join(sep=" ") }}
- 信息来源: [{{ source }}]
- 详情链接: {{ detail_link }}

{% if description %}### **漏洞描述**
{{ description }}
{% endif %}
{% if solutions %}### **修复方案**
{{ solutions }}
{% endif %}
{% if references%}### **参考链接**
{% for reference in reference_links %}{{ loop.index }}. {{ reference }}
{% endfor %}{% endif %}

{% if cve %}### **开源检索**
{% if github_search | length > 0 %}{% for link in github_search %}{{ loop.index }}. {{ link }}
{% endfor %}{% else %}暂未找到{% endif %}{% endif %}"####;

const SEC_NOTICE_MSG_TEMPLATE: &str = r####"
# {{ title }}

- 危害定级: **{{ risk_level }}**
- 披露日期: **{{ publish_time }}**
- ZeroDay: **{{ is_zero_day }}**
- 产品名称: {{ product_name }}
- 详情链接: {{ detail_link }}
"####;

const MAX_REFERENCE_LENGTH: usize = 8;

pub fn render_vulninfo(mut vuln: VulnInformation) -> AppResult<String> {
    if vuln.reference_links.len() > MAX_REFERENCE_LENGTH {
        vuln.reference_links = vuln.reference_links[..MAX_REFERENCE_LENGTH].to_vec().into();
    }
    let json_value: Value = serde_json::to_value(vuln)
        .map_err(|e| Error::Message(format!("Failed to serialize vuln info: {e}")))?;
    let markdown = render_string(VULN_INFO_MSG_TEMPLATE, &json_value)?;
    Ok(markdown)
}

pub fn render_sec_notice(sec_notice: SecuritNotice) -> AppResult<String> {
    let json_value: Value = serde_json::to_value(sec_notice)
        .map_err(|e| Error::Message(format!("Failed to serialize sec notice: {e}")))?;
    let markdown = render_string(SEC_NOTICE_MSG_TEMPLATE, &json_value)?;
    Ok(markdown)
}

pub fn escape_markdown(input: String) -> String {
    input
        .replace('_', "\\_")
        .replace('.', "\\.")
        .replace('*', "\\*")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('(', "\\(")
        .replace(')', "\\)")
        .replace('~', "\\~")
        .replace('`', "\\`")
        .replace('>', "\\>")
        .replace('#', "\\#")
        .replace('+', "\\+")
        .replace('-', "\\-")
        .replace('=', "\\=")
        .replace('|', "\\|")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('!', "\\!")
}

/// 裸 URL 转 markdown 链接(规范模板中的链接均为裸 URL)
fn linkify_urls(line: &str) -> String {
    line.split(' ')
        .map(|tok| {
            if tok.starts_with("http://") || tok.starts_with("https://") {
                format!("[{tok}]({tok})")
            } else {
                tok.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// 企微 markdown 方言:不支持 `-` 无序列表,转为 `•`;裸 URL 转链接以便点击
pub fn markdown_for_wecom(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    for line in msg.lines() {
        let line = line
            .strip_prefix("- ")
            .map(|rest| format!("\u{2022} {rest}"))
            .unwrap_or_else(|| line.to_string());
        out.push_str(&linkify_urls(&line));
        out.push('\n');
    }
    out
}

/// 飞书 lark_md 方言:不支持 `#` 标题(标题已展示在卡片 header,正文 H1 丢弃,
/// `###` 小节转纯加粗),`-` 列表转 `•`;裸 URL 转链接
pub fn markdown_for_feishu(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    for line in msg.lines() {
        let line = if line.starts_with("# ") {
            continue;
        } else if let Some(rest) = line.strip_prefix("### ") {
            rest.to_string()
        } else if let Some(rest) = line.strip_prefix("- ") {
            format!("\u{2022} {rest}")
        } else {
            line.to_string()
        };
        out.push_str(&linkify_urls(&line));
        out.push('\n');
    }
    out
}

/// 按渠道类型构造对应的机器人
pub fn channel_bot(ch: &PushChannel) -> AppResult<Box<dyn MessageBot>> {
    match ch.channel_type.as_str() {
        "dingtalk" => Ok(Box::new(ding_bot::DingBot::try_new(
            ch.access_token.clone(),
            ch.secret_token.clone(),
        )?)),
        "wecom" => Ok(Box::new(wecom_bot::WeComBot::try_new(
            ch.access_token.clone(),
        )?)),
        "feishu" => Ok(Box::new(feishu_bot::FeishuBot::try_new(
            ch.access_token.clone(),
            ch.secret_token.clone(),
        )?)),
        other => Err(Error::Message(format!("unknown push channel type: {other}")).into()),
    }
}

/// 并发推送到所有渠道,返回成功数;单渠道失败仅记日志不中断
pub async fn push_markdown_all(
    channels: &[PushChannel],
    title: String,
    msg: String,
) -> AppResult<usize> {
    let mut job_set = JoinSet::new();
    for ch in channels {
        let Ok(bot) = channel_bot(ch) else {
            log::error!("build push bot failed: {}({})", ch.name, ch.channel_type);
            continue;
        };
        let title = title.clone();
        let msg = msg.clone();
        job_set.spawn(async move {
            match bot.push_markdown(title, msg).await {
                Ok(()) => 1usize,
                Err(e) => {
                    log::error!("push markdown failed: {e:?}");
                    0
                }
            }
        });
    }
    let mut ok = 0;
    while let Some(ret) = job_set.join_next().await {
        ok += ret.unwrap_or(0);
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wecom_adapts_bullets_and_links() {
        let src = "# 标题\n- CVE编号: CVE-1\n- 危害定级: **高**\nhttps://example.com/a\n";
        let out = markdown_for_wecom(src);
        assert!(out.contains("\u{2022} CVE编号: CVE-1"));
        assert!(out.contains("[https://example.com/a](https://example.com/a)"));
        assert!(out.contains("# 标题"));
    }

    #[test]
    fn feishu_drops_h1_and_keeps_bold_sections() {
        let src = "# 标题\n### **漏洞描述**\n- 项目: x\n";
        let out = markdown_for_feishu(src);
        assert!(!out.contains("# 标题"));
        assert!(out.contains("**漏洞描述**"));
        assert!(out.contains("\u{2022} 项目: x"));
    }
}
