use logforth::{
    append::{
        self,
        rolling_file::{RollingFileBuilder, Rotation},
    },
    diagnostic::{FastraceDiagnostic, StaticDiagnostic},
    filter::{EnvFilter, env_filter::EnvFilterBuilder},
    layout,
};
use std::borrow::Cow;

use crate::config::settings::TelemetryConfig;

use super::runtime::Runtime;

/// 文本布局:与 logforth 默认文本格式一致,但不输出 target(模块路径)。
#[derive(Debug)]
struct ConciseTextLayout;

impl logforth::layout::Layout for ConciseTextLayout {
    fn format(
        &self,
        record: &log::Record,
        diagnostics: &[Box<dyn logforth::diagnostic::Diagnostic>],
    ) -> anyhow::Result<Vec<u8>> {
        let time = jiff::Zoned::now();
        let level = record.level();
        let file = record
            .file()
            .map(|f| f.rsplit('/').next().unwrap_or(f))
            .unwrap_or("");
        let line = record.line().unwrap_or_default();
        let message = record.args();
        let text = format!("{time:.6} {level:>5} {file}:{line} {message}");
        struct KvWriter(String);
        impl<'kvs> log::kv::VisitSource<'kvs> for KvWriter {
            fn visit_pair(
                &mut self,
                key: log::kv::Key<'kvs>,
                value: log::kv::Value<'kvs>,
            ) -> Result<(), log::kv::Error> {
                use std::fmt::Write;
                write!(&mut self.0, " {key}={value}")?;
                Ok(())
            }
        }
        impl logforth::diagnostic::Visitor for KvWriter {
            fn visit(&mut self, key: Cow<str>, value: Cow<str>) -> anyhow::Result<()> {
                use std::fmt::Write;
                write!(&mut self.0, " {key}={value}")?;
                Ok(())
            }
        }
        let mut visitor = KvWriter(text);
        record.key_values().visit(&mut visitor)?;
        for d in diagnostics {
            d.visit(&mut visitor)?;
        }
        Ok(visitor.0.into_bytes())
    }
}

pub fn init(
    rt: &Runtime,
    service_name: &'static str,
    config: TelemetryConfig,
) -> Vec<Box<dyn Send + Sync + 'static>> {
    let mut drop_guards = vec![];
    drop_guards.extend(init_logs(rt, service_name, &config));
    drop_guards
}

fn init_logs(
    rt: &Runtime,
    service_name: &'static str,
    config: &TelemetryConfig,
) -> Vec<Box<dyn Send + Sync + 'static>> {
    let _ = rt;
    let static_diagnostic = StaticDiagnostic::default();

    let mut drop_guards: Vec<Box<dyn Send + Sync + 'static>> = Vec::new();
    let mut builder = logforth::builder();

    if let Some(file) = &config.logs.file {
        let (rolling, guard) = RollingFileBuilder::new(&file.dir)
            .layout(layout::JsonLayout::default())
            .rotation(Rotation::Hourly)
            .filename_prefix(service_name)
            .filename_suffix("log")
            .max_log_files(file.max_files)
            .build()
            .expect("failed to initialize rolling file appender");
        drop_guards.push(guard);
        builder = builder.dispatch(|b| {
            b.filter(make_rust_log_filter(&file.filter))
                .diagnostic(FastraceDiagnostic::default())
                .diagnostic(static_diagnostic.clone())
                .append(rolling)
        });
    }

    if let Some(stderr) = &config.logs.stderr {
        builder = builder.dispatch(|b| {
            b.filter(make_rust_log_filter_with_default_env(&stderr.filter))
                .diagnostic(FastraceDiagnostic::default())
                .diagnostic(static_diagnostic.clone())
                .append(append::Stderr::default().with_layout(ConciseTextLayout))
        });
    }

    let _ = builder.try_apply();

    drop_guards
}

fn make_rust_log_filter(filter: &str) -> EnvFilter {
    let builder = EnvFilterBuilder::new()
        .try_parse(filter)
        .unwrap_or_else(|_| panic!("failed to parse filter: {filter}"));
    EnvFilter::new(builder)
}

fn make_rust_log_filter_with_default_env(filter: &str) -> EnvFilter {
    if let Ok(filter) = std::env::var("RUST_LOG") {
        make_rust_log_filter(&filter)
    } else {
        make_rust_log_filter(filter)
    }
}
