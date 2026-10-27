use std::{path::PathBuf, str::FromStr};

use error_stack::{ResultExt, bail};
use serde::de::IntoDeserializer;
use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

use crate::AppResult;
use crate::errors::Error;

pub struct LoadConfigResult {
    pub config: Config,
    pub warnings: Vec<String>,
}

pub fn load_config(config_file: Option<PathBuf>) -> AppResult<LoadConfigResult> {
    let content = match config_file {
        Some(config_file) => std::fs::read_to_string(&config_file).change_context_lazy(|| {
            Error::Message(format!(
                "failed to read config file {}",
                config_file.display()
            ))
        })?,
        // 未提供配置文件时使用内置默认配置(与 dev/config.toml 一致),env 覆盖仍然生效
        None => toml_edit::ser::to_string(&Config::default()).change_context_lazy(|| {
            Error::Message("failed to serialize default config".to_string())
        })?,
    };

    let mut config = DocumentMut::from_str(&content)
        .change_context_lazy(|| Error::Message("failed to parse config file".to_string()))?;

    let env = std::env::vars()
        .filter(|(key, _)| key.starts_with("VULNFEED_CONFIG_"))
        .collect::<std::collections::HashMap<String, String>>();

    fn set_toml_path(
        doc: &mut DocumentMut,
        key: &str,
        path: &'static str,
        value: toml_edit::Item,
    ) -> Vec<String> {
        let mut current = doc.as_item_mut();
        let mut warnings = vec![];
        let parts = path.split('.').collect::<Vec<_>>();
        let len = parts.len();
        assert!(len > 0, "path must not be empty");
        for part in parts.iter().take(len - 1) {
            if current.get(part).is_none() {
                warnings.push(format!(
                    "[key={key}] config path '{path}' has missing parent '{part}'; created",
                ));
            }
            current = &mut current[part];
        }
        current[parts[len - 1]] = value;
        warnings
    }
    let known_option_entries = known_option_entries();
    let mut warnings = vec![];
    for (k, v) in env {
        let Some(ent) = known_option_entries.iter().find(|e| e.env_name == k) else {
            bail!(Error::Message(format!(
                "failed to parse unknown environment variable {k} with value {v}"
            )))
        };
        let (path, item) = match ent.ent_type {
            "string" => {
                let path = ent.ent_path;
                let value = toml_edit::value(v);
                (path, value)
            }
            "integer" => {
                let path = ent.ent_path;
                let value = v.parse::<i64>().change_context_lazy(|| {
                    Error::Message(format!("failed to parse integer value {v} of key {k}"))
                })?;
                let value = toml_edit::value(value);
                (path, value)
            }
            "boolean" => {
                let path = ent.ent_path;
                let value = v.parse::<bool>().change_context_lazy(|| {
                    Error::Message(format!("failed to parse boolean value {v} of key {k}"))
                })?;
                let value = toml_edit::value(value);
                (path, value)
            }
            ty => {
                bail!(Error::Message(format!(
                    "failed to parse environment variable {k} with value {v} and resolved type {ty}"
                )))
            }
        };
        let new_warnings = set_toml_path(&mut config, &k, path, item);
        warnings.extend(new_warnings);
    }

    let config = Config::deserialize(config.into_deserializer())
        .change_context_lazy(|| Error::Message("failed to deserialize config".to_string()))?;
    Ok(LoadConfigResult { config, warnings })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub server: Server,
    pub auth: Auth,
    pub database: Database,
    #[serde(default)]
    pub push: PushConfig,
    pub telemetry: TelemetryConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    /// 静态资源目录;空 = 使用内嵌资源(单二进制),非空则从磁盘目录伺服 SPA
    #[serde(default = "default_static_dir")]
    pub static_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Auth {
    pub jwt: Jwt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Jwt {
    #[serde(default = "default_jwt_secret")]
    pub secret: String,
    #[serde(default = "default_jwt_expiration")]
    pub expiration: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Database {
    #[serde(default = "default_database_path")]
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushConfig {
    /// 是否自动补推已入库但未推送的条目;false 时此类条目只能手动推送
    #[serde(default = "default_push_backlog")]
    pub backlog: bool,
}

impl Default for PushConfig {
    fn default() -> Self {
        Self {
            backlog: default_push_backlog(),
        }
    }
}

fn default_push_backlog() -> bool {
    false
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetryConfig {
    #[serde(default = "LogsConfig::disabled")]
    pub logs: LogsConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<FileAppenderConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<StderrAppenderConfig>,
}

impl LogsConfig {
    pub fn disabled() -> Self {
        Self {
            file: None,
            stderr: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileAppenderConfig {
    pub filter: String,
    pub dir: String,
    pub max_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StderrAppenderConfig {
    pub filter: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: Server {
                listen_addr: default_listen_addr(),
                static_dir: default_static_dir(),
            },
            auth: Auth {
                jwt: Jwt {
                    secret: "LSJ74YUWkzGheiWMRxcH".to_string(),
                    expiration: 604800,
                },
            },
            database: Database {
                path: default_database_path(),
            },
            telemetry: TelemetryConfig {
                logs: LogsConfig {
                    file: Some(FileAppenderConfig {
                        filter: "INFO".to_string(),
                        dir: "logs".to_string(),
                        max_files: 64,
                    }),
                    stderr: Some(StderrAppenderConfig {
                        filter: "INFO".to_string(),
                    }),
                },
            },
            push: PushConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub struct OptionEntry {
    pub env_name: &'static str,
    pub ent_path: &'static str,
    pub ent_type: &'static str,
}

pub const fn known_option_entries() -> &'static [OptionEntry] {
    &[
        OptionEntry {
            env_name: "VULNFEED_CONFIG_SERVER_LISTEN_ADDR",
            ent_path: "server.listen_addr",
            ent_type: "string",
        },
        OptionEntry {
            env_name: "VULNFEED_CONFIG_SERVER_STATIC_DIR",
            ent_path: "server.static_dir",
            ent_type: "string",
        },
        OptionEntry {
            env_name: "VULNFEED_CONFIG_AUTH_JWT_SECRET",
            ent_path: "auth.jwt.secret",
            ent_type: "string",
        },
        OptionEntry {
            env_name: "VULNFEED_CONFIG_AUTH_JWT_EXPIRATION",
            ent_path: "auth.jwt.expiration",
            ent_type: "integer",
        },
        OptionEntry {
            env_name: "VULNFEED_CONFIG_DATABASE_PATH",
            ent_path: "database.path",
            ent_type: "string",
        },
    ]
}

fn default_static_dir() -> String {
    String::new()
}

fn default_listen_addr() -> String {
    "0.0.0.0:9000".to_string()
}

fn default_jwt_secret() -> String {
    "LSJ74YUWkzGheiWMRxcH".to_string()
}

fn default_jwt_expiration() -> u64 {
    604800
}

fn default_database_path() -> String {
    "data/vulnfeed.db".to_string()
}
