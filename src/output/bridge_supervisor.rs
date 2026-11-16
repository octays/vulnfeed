//! Supervise the local Feishu bridge when a local bridge binary and token exist.
//! Container deployments can continue running a separate bridge with an env token.
use crate::config::settings::Config;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

pub fn token_path(config: &Config) -> PathBuf {
    let parent = Path::new(&config.database.path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    parent.join(".feishu-bridge-token")
}

pub fn configured_token(config: &Config) -> Option<String> {
    if let Ok(value) = std::env::var("VULNFEED_FEISHU_BRIDGE_TOKEN") {
        return (value.len() >= 32).then_some(value);
    }
    let path = token_path(config);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(&path).ok()?;
        if meta.permissions().mode() & 0o077 != 0 {
            log::error!("feishu bridge token file permissions too broad; owner-only expected");
            return None;
        }
    }
    let value = std::fs::read_to_string(path).ok()?.trim().to_owned();
    (value.len() >= 32).then_some(value)
}

pub struct BridgeGuard {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Drop for BridgeGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn start_local(config: &Config) -> Option<BridgeGuard> {
    // An explicit token indicates an externally managed bridge (e.g. Compose).
    if std::env::var_os("VULNFEED_FEISHU_BRIDGE_TOKEN").is_some() {
        return None;
    }
    start_with_binary(config, Path::new("target/feishu-bridge"))
}

fn start_with_binary(config: &Config, binary: &Path) -> Option<BridgeGuard> {
    if !binary.is_file() {
        return None;
    }
    let token = match configured_token(config) {
        Some(token) => token,
        None => {
            use std::io::Write;
            let path = token_path(config);
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let token = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            match options.open(&path) {
                Ok(mut file) => {
                    if let Err(e) = file.write_all(token.as_bytes()) {
                        log::error!("failed to persist feishu bridge token: {e}");
                        return None;
                    }
                    token
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    configured_token(config)?
                }
                Err(e) => {
                    log::error!("failed to create feishu bridge token: {e}");
                    return None;
                }
            }
        }
    };
    let port = config
        .server
        .listen_addr
        .rsplit(':')
        .next()?
        .parse::<u16>()
        .ok()?;
    let endpoint = format!("http://127.0.0.1:{port}/api/internal/feishu/card-action");
    let binary = binary.to_path_buf();
    let stop = Arc::new(AtomicBool::new(false));
    let state = stop.clone();
    let thread = thread::spawn(move || {
        while !state.load(Ordering::Acquire) {
            let mut child = match Command::new(&binary)
                .arg("--bridge")
                .env("VULNFEED_FEISHU_BRIDGE_TOKEN", &token)
                .env("VULNFEED_FEISHU_BRIDGE_URL", &endpoint)
                .stdin(Stdio::null())
                .spawn()
            {
                Ok(child) => child,
                Err(e) => {
                    log::error!("failed to start local feishu bridge: {e}");
                    thread::sleep(Duration::from_secs(5));
                    continue;
                }
            };
            log::info!("local feishu bridge started");
            loop {
                if state.load(Ordering::Acquire) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
                match child.try_wait() {
                    Ok(Some(status)) => {
                        log::warn!("feishu bridge exited ({status}); restarting");
                        break;
                    }
                    Ok(None) => thread::sleep(Duration::from_millis(250)),
                    Err(e) => {
                        log::error!("failed to poll feishu bridge status: {e}");
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
            for _ in 0..20 {
                if state.load(Ordering::Acquire) {
                    return;
                }
                thread::sleep(Duration::from_millis(250));
            }
        }
    });
    Some(BridgeGuard {
        stop,
        thread: Some(thread),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn local_bridge_creates_private_token_and_starts_with_server() {
        if std::env::var_os("VULNFEED_FEISHU_BRIDGE_TOKEN").is_some() {
            return;
        }
        let dir =
            std::env::temp_dir().join(format!("vulnfeed-bridge-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let binary = dir.join("bridge");
        let marker = dir.join("started");
        std::fs::write(
            &binary,
            format!(
                "#!/bin/sh\nprintf started > '{}'\nexec sleep 30\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut config = Config::default();
        config.database.path = dir.join("db").to_string_lossy().into_owned();
        let guard = start_with_binary(&config, &binary).expect("bridge starts");
        for _ in 0..50 {
            if marker.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(marker.exists());
        let token = configured_token(&config).expect("token created");
        assert!(token.len() >= 32);
        assert_eq!(
            std::fs::metadata(token_path(&config))
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
        drop(guard);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
