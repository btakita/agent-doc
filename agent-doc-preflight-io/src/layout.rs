//! Preflight tmux layout health and repair adapters.

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Trigger an automatic `resync --fix` when session-drift has been detected
/// on two consecutive preflights.
///
/// The drift counter lives in the project `state.db`. Each call either
/// increments it (drift present) or clears it (drift absent). When the counter
/// reaches >= 2 we invoke `agent_doc_sync_io::resync::run(true, None, None)` and
/// reset it to 0 so we do not loop on every cycle.
pub fn maybe_auto_resync_on_drift(file: &Path, layout_issues: &[String]) {
    let has_drift = layout_issues
        .iter()
        .any(|i| i.starts_with("session drift:"));

    let Ok(canonical) = file.canonicalize() else {
        return;
    };
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return;
    };
    if !has_drift {
        let _ = agent_doc_controller_io::project_controller::clear_coordination_lease(
            &project_root,
            "preflight_layout_drift",
            "project",
        );
        return;
    }

    let current: u32 = agent_doc_controller_io::project_controller::load_coordination_lease(
        &project_root,
        "preflight_layout_drift",
        "project",
    )
    .ok()
    .flatten()
    .and_then(|lease| lease.holder.parse().ok())
    .unwrap_or(0);
    let next = current + 1;

    if let Err(e) = agent_doc_controller_io::project_controller::upsert_coordination_lease(
        &project_root,
        &agent_doc_sqlite::state_store::CoordinationLeaseRecord {
            scope_kind: "preflight_layout_drift".to_string(),
            scope_id: "project".to_string(),
            holder: next.to_string(),
            holder_pid: Some(std::process::id()),
            heartbeat_secs: agent_doc_sqlite::state_store::timestamp_secs(),
        },
    ) {
        eprintln!("[preflight] drift state update failed: {}", e);
    }

    if next >= 2 {
        eprintln!(
            "[preflight] session drift detected {}x consecutively — running `resync --fix`",
            next
        );
        agent_doc_ops_log_io::log_op(file, &format!("auto_resync_on_drift consecutive={}", next));
        if let Err(e) = agent_doc_sync_io::resync::run(true, None, None) {
            eprintln!("[preflight] auto-resync failed: {}", e);
        } else {
            let _ = agent_doc_controller_io::project_controller::clear_coordination_lease(
                &project_root,
                "preflight_layout_drift",
                "project",
            );
            close_superseded_drift_sessions(file);
        }
    } else {
        eprintln!(
            "[preflight] session drift detected (count={}) — will auto-resync on next detection",
            next
        );
    }
}

/// After an auto-resync, close tmux sessions superseded by the canonical
/// (active agent-doc window) session when registered panes still span more than
/// one session. Best effort: never blocks a cycle.
fn close_superseded_drift_sessions(file: &Path) {
    let tmux = agent_doc_tmux_io::configured_tmux();
    let registry = match agent_doc_session_registry_io::load() {
        Ok(registry) => registry,
        Err(e) => {
            eprintln!(
                "[preflight] session-drift close: registry load failed: {}",
                e
            );
            return;
        }
    };
    let drift_sessions = agent_doc_sync_io::resync::registered_pane_sessions(&tmux, &registry);
    if drift_sessions.len() <= 1 {
        return;
    }
    let Some(canonical) =
        agent_doc_sync_io::resync::canonical_session_for_document(&tmux, &registry, file)
    else {
        eprintln!(
            "[preflight] session-drift: no canonical agent-doc session resolved for {}; preserving all sessions",
            file.display()
        );
        return;
    };
    match agent_doc_sync_io::resync::close_superseded_drift_sessions(
        &tmux,
        &canonical,
        &drift_sessions,
    ) {
        Ok(0) => {}
        Ok(n) => eprintln!(
            "[preflight] session-drift: closed {} superseded session(s) around canonical '{}'",
            n, canonical
        ),
        Err(e) => eprintln!("[preflight] session-drift superseded close failed: {}", e),
    }
}

fn current_tmux_session_name() -> Option<String> {
    agent_doc_tmux_io::configured_tmux().current_session()
}

/// Immediately converge structural window drift found by [`check_layout`].
///
/// The historical name is retained for callers, but the repair now covers both
/// a missing base index and agent-doc panes split across visible windows.
pub fn maybe_auto_repair_base_index(file: &Path, layout_issues: &[String]) -> bool {
    let has_base_index_issue = layout_issues
        .iter()
        .any(|i| i.contains("window index 0 missing"));
    let has_window_drift = layout_issues.iter().any(|i| i.starts_with("window drift:"));

    if !has_base_index_issue && !has_window_drift {
        return false;
    }

    if !agent_doc_tmux_io::in_tmux() {
        eprintln!(
            "[preflight] window index 0 missing but no tmux context is available; run `agent-doc session doctor {} --repair` from the target tmux session",
            file.display()
        );
        return false;
    }

    let Some(name) = current_tmux_session_name() else {
        eprintln!(
            "[preflight] window index 0 missing but tmux session lookup failed; run `agent-doc session doctor {} --repair`",
            file.display()
        );
        return false;
    };

    let reason = if has_window_drift {
        "registered agent-doc panes span multiple visible windows"
    } else {
        "window index 0 missing"
    };
    eprintln!("[preflight] {reason} — running layout convergence immediately");
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "auto_repair_layout immediate session={} base_index_issue={} window_drift={}",
            name, has_base_index_issue, has_window_drift
        ),
    );
    let tmux = agent_doc_tmux_io::configured_tmux();
    // `repair_layout` directly merges duplicate `agent-doc` windows. For a
    // registered pane stranded in some other visible window, resync first
    // parks the deterministic outlier in stash; the retained layout can then
    // place it without preserving a second visible workspace.
    if has_window_drift && let Err(e) = agent_doc_sync_io::resync::run(true, None, None) {
        eprintln!("[preflight] window-drift resync failed: {e}");
    }
    if let Err(e) = agent_doc_sync_io::sync::repair_layout(&tmux, &name, "agent-doc") {
        eprintln!(
            "[preflight] auto repair_layout failed: {}; run `agent-doc session doctor {} --repair`",
            e,
            file.display()
        );
        return false;
    }

    true
}

/// Check tmux layout health for the current session.
///
/// Returns a list of human-readable issue strings. An empty vec means the
/// layout is healthy. This is read-only and returns an empty vec when not
/// running inside tmux.
pub fn check_layout() -> Vec<String> {
    if !agent_doc_tmux_io::in_tmux() {
        return vec![];
    }

    let mut issues = Vec::new();

    let Some(session_name) = agent_doc_tmux_io::configured_tmux().current_session() else {
        return issues;
    };

    if session_name.is_empty() {
        return issues;
    }

    let tmux_runner = agent_doc_tmux_io::ProcessTmuxRunner::default_binary();
    let window_output = match agent_doc_tmux_io::list_windows(
        &tmux_runner,
        Some(&format!("{}:", session_name)),
        "#{window_index}\t#{window_id}\t#{window_name}\t#{window_panes}",
    ) {
        Ok(output) => output,
        Err(_) => return issues,
    };

    let windows: Vec<(u32, String, String)> = window_output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            let index: u32 = parts.next()?.parse().ok()?;
            let id = parts.next()?.to_string();
            let name = parts.next()?.to_string();
            Some((index, id, name))
        })
        .collect();

    if !windows.iter().any(|(index, _, _)| *index == 0) {
        issues.push(format!(
            "window index 0 missing in session '{}' (base-index compliance)",
            session_name,
        ));
    }

    let target_window_ids: Vec<String> = windows
        .iter()
        .filter(|(_, _, name)| name == "agent-doc")
        .map(|(_, id, _)| id.clone())
        .collect();
    if target_window_ids.len() > 1 {
        let mut ids = target_window_ids.clone();
        ids.sort_by(|left, right| compare_tmux_window_ids(left, right));
        issues.push(format!(
            "window drift: session '{}' has {} 'agent-doc' windows: {}",
            session_name,
            ids.len(),
            ids.join(", "),
        ));
    }

    let registry: Option<tmux_router::Registry> = agent_doc_session_registry_io::load().ok();
    if let Some(registry) = registry {
        let mut pane_sessions: HashSet<String> = HashSet::new();
        let mut registered_visible_windows: HashMap<String, String> = HashMap::new();
        for entry in registry.values() {
            let pane = &entry.pane;
            if let Some(pane_sess) = agent_doc_tmux_io::target_session_name(&tmux_runner, pane) {
                pane_sessions.insert(pane_sess.clone());
                if pane_sess == session_name
                    && let Some(window_id) = agent_doc_tmux_io::target_window_id(&tmux_runner, pane)
                    && let Some(window_name) =
                        agent_doc_tmux_io::target_window_name(&tmux_runner, pane)
                    && !is_stash_window_name(&window_name)
                {
                    registered_visible_windows.insert(window_id, window_name);
                }
            }
        }
        if pane_sessions.len() > 1 {
            let mut sessions_vec: Vec<&str> = pane_sessions.iter().map(|s| s.as_str()).collect();
            sessions_vec.sort();
            issues.push(format!(
                "session drift: registered panes span {} tmux sessions: {}",
                pane_sessions.len(),
                sessions_vec.join(", "),
            ));
        }

        let mut visible_ids: Vec<String> = registered_visible_windows.keys().cloned().collect();
        visible_ids.sort_by(|left, right| compare_tmux_window_ids(left, right));
        let outside_canonical = target_window_ids.len() == 1
            && visible_ids.iter().any(|id| id != &target_window_ids[0]);
        if visible_ids.len() > 1 || outside_canonical {
            let placement = visible_ids
                .iter()
                .map(|id| {
                    format!(
                        "{}:{}",
                        id,
                        registered_visible_windows
                            .get(id)
                            .map(String::as_str)
                            .unwrap_or("?")
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            issues.push(format!(
                "window drift: registered panes in session '{}' span visible windows {}",
                session_name, placement
            ));
        }

        issues.extend(detect_duplicate_claims(&registry));
    }

    issues
}

fn is_stash_window_name(name: &str) -> bool {
    name == "stash" || name.starts_with("stash-")
}

fn compare_tmux_window_ids(left: &str, right: &str) -> std::cmp::Ordering {
    let number = |id: &str| {
        id.trim_start_matches('@')
            .parse::<u64>()
            .unwrap_or(u64::MAX)
    };
    number(left)
        .cmp(&number(right))
        .then_with(|| left.cmp(right))
}

/// Detect duplicate file claims in a registry snapshot.
///
/// Returns one issue string per file that has two or more sessions claiming it.
/// Entries with an empty `file` field are skipped.
pub fn detect_duplicate_claims(registry: &tmux_router::Registry) -> Vec<String> {
    let mut file_sessions: HashMap<String, Vec<String>> = HashMap::new();
    for (registry_key, entry) in registry {
        let file_identity = if Path::new(registry_key).is_absolute() {
            registry_key.clone()
        } else {
            entry.file.clone()
        };
        if file_identity.is_empty() {
            continue;
        }
        file_sessions
            .entry(file_identity)
            .or_default()
            .push(if entry.session_id.is_empty() {
                registry_key.clone()
            } else {
                entry.session_id.clone()
            });
    }
    let mut issues = Vec::new();
    for (file, session_ids) in &file_sessions {
        if session_ids.len() > 1 {
            let mut sorted = session_ids.clone();
            sorted.sort();
            issues.push(format!(
                "duplicate claims: {} sessions claim '{}': {}",
                session_ids.len(),
                file,
                sorted.join(", "),
            ));
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::compare_tmux_window_ids;

    #[test]
    fn tmux_window_ids_sort_numerically_for_deterministic_repair() {
        let mut ids = vec!["@10".to_string(), "@2".to_string(), "@1".to_string()];
        ids.sort_by(|left, right| compare_tmux_window_ids(left, right));
        assert_eq!(ids, ["@1", "@2", "@10"]);
    }
}
