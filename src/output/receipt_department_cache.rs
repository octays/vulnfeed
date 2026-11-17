//! On-demand open_id → department resolution. The forward API (user object
//! department_ids) is field-gated for most tenants, so departments are read
//! in reverse (department tree + member lists) into a short-lived in-memory
//! map, built lazily on first use and single-flighted — no persistent cache,
//! no cross-task races.
use crate::output::push::feishu_app::FeishuApp;
use std::{collections::HashMap, sync::OnceLock, time::Duration};
use tokio::sync::Mutex;

const TTL: Duration = Duration::from_secs(600);

struct State {
    members: HashMap<String, String>, // open_id -> department_id
    names: HashMap<String, String>,   // department_id -> name
    built_at: Option<std::time::Instant>,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(State {
            members: HashMap::new(),
            names: HashMap::new(),
            built_at: None,
        })
    })
}

/// Returns (department name, reliable). A `None` with reliable=true means the
/// user is genuinely not in any visible department; reliable=false means the
/// build failed and the caller should retry later.
pub async fn department_of(client: &FeishuApp, open_id: &str) -> (Option<String>, bool) {
    let mut guard = state().lock().await;
    if !guard.built_at.is_some_and(|at| at.elapsed() < TTL)
        && let Err(e) = build(client, &mut guard).await
    {
        log::warn!("department on-demand build failed: {e}");
        return (None, false);
    }
    let department = guard
        .members
        .get(open_id)
        .and_then(|id| guard.names.get(id))
        .cloned()
        .filter(|name| !name.trim().is_empty());
    (department, true)
}

/// Warm the in-memory map at startup so the first submission after a restart
/// doesn't pay the ~20s full build. Failures are silent; the first lookup
/// will retry the build.
pub async fn warm_up(client: &FeishuApp) {
    let mut guard = state().lock().await;
    if guard.built_at.is_none()
        && let Err(e) = build(client, &mut guard).await
    {
        log::warn!("department cache warm-up failed, will build on demand: {e}");
    }
}

async fn build(client: &FeishuApp, guard: &mut State) -> Result<(), String> {
    let departments = client
        .visible_departments()
        .await
        .map_err(|e| e.to_string())?;
    let mut members = HashMap::new();
    let mut names = HashMap::new();
    for (department_id, name) in &departments {
        names.insert(department_id.clone(), name.clone());
        match client.department_members(department_id).await {
            Ok(entries) => {
                for (open_id, _) in entries {
                    // First department wins, mirroring the stable ORDER BY of
                    // the previous persistent cache.
                    members
                        .entry(open_id)
                        .or_insert_with(|| department_id.clone());
                }
            }
            Err(e) => {
                log::warn!(
                    "department members fetch failed, skipping {name} ({department_id}): {e}"
                );
            }
        }
    }
    guard.members = members;
    guard.names = names;
    guard.built_at = Some(std::time::Instant::now());
    Ok(())
}
