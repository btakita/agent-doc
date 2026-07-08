//! Preflight tmux layout health and repair adapters.

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Trigger an automatic `resync --fix` when session-drift has been detected
/// on two consecutive preflights.
///
/// The drift counter lives at `.agent-doc/state/drift.count`. Each call either
/// increments it (drift present) or deletes it (drift absent). When the counter
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
    let state_dir = project_root.join(".agent-doc/state");
    let counter_path = state_dir.join("drift.count");

    if !has_drift {
        if counter_path.exists() {
            let _ = std::fs::remove_file(&counter_path);
        }
        return;
    }

    let current: u32 = std::fs::read_to_string(&counter_path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let next = current + 1;

    if let Err(e) = std::fs::create_dir_all(&state_dir) {
        eprintln!("[preflight] drift state dir create failed: {}", e);
        return;
    }
    if let Err(e) = std::fs::write(&counter_path, next.to_string()) {
        eprintln!("[preflight] drift counter write failed: {}", e);
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
            let _ = std::fs::remove_file(&counter_path);
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
    let tmux = tmux_router::Tmux::default_server();
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

fn clear_base_index_repair_counter(file: &Path) {
    let Ok(canonical) = file.canonicalize() else {
        return;
    };
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return;
    };
    let counter_path = project_root.join(".agent-doc/state/base-index-repair.count");
    if counter_path.exists() {
        let _ = std::fs::remove_file(counter_path);
    }
}

fn current_tmux_session_name() -> Option<String> {
    tmux_router::Tmux::default_server().current_session()
}

pub fn maybe_auto_repair_base_index(file: &Path, layout_issues: &[String]) -> bool {
    let has_base_index_issue = layout_issues
        .iter()
        .any(|i| i.contains("window index 0 missing"));

    if !has_base_index_issue {
        clear_base_index_repair_counter(file);
        return false;
    }

    // Older builds used a consecutive-detection counter before repairing.
    // Once the issue is visible in preflight, leaving it for the next turn makes
    // the active response cycle nondeterministic, so clean the stale marker and
    // repair immediately.
    clear_base_index_repair_counter(file);

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

    eprintln!("[preflight] window index 0 missing — running repair_layout immediately");
    agent_doc_ops_log_io::log_op(
        file,
        &format!("auto_repair_base_index immediate session={}", name),
    );
    let tmux = tmux_router::Tmux::default_server();
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

    let Some(session_name) = tmux_router::Tmux::default_server().current_session() else {
        return issues;
    };

    if session_name.is_empty() {
        return issues;
    }

    let tmux_runner = agent_doc_tmux_io::ProcessTmuxRunner::default_binary();
    let window_output = match agent_doc_tmux_io::list_windows(
        &tmux_runner,
        Some(&format!("{}:", session_name)),
        "#{window_index}\t#{window_name}\t#{window_panes}",
    ) {
        Ok(output) => output,
        Err(_) => return issues,
    };

    let windows: Vec<u32> = window_output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let index: u32 = parts.next()?.parse().ok()?;
            Some(index)
        })
        .collect();

    if !windows.contains(&0) {
        issues.push(format!(
            "window index 0 missing in session '{}' (base-index compliance)",
            session_name,
        ));
    }

    let registry: Option<tmux_router::Registry> = agent_doc_session_registry_io::load().ok();
    if let Some(registry) = registry {
        let mut pane_sessions: HashSet<String> = HashSet::new();
        for entry in registry.values() {
            let pane = &entry.pane;
            if let Some(pane_sess) = agent_doc_tmux_io::target_session_name(&tmux_runner, pane) {
                pane_sessions.insert(pane_sess);
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

        issues.extend(detect_duplicate_claims(&registry));
    }

    issues
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
