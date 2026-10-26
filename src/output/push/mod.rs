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
