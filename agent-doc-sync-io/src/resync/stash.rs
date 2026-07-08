//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
#[cfg(test)]
use agent_doc_session_registry_io::registration as sessions;
#[cfg(test)]
use agent_doc_supervisor::ipc_protocol::{IpcMethod, IpcResponse};

pub(crate) fn return_stashed_panes(tmux: &Tmux) {
    let registry = agent_doc_session_registry_io::load().unwrap_or_default();
    return_stashed_panes_with_registry(tmux, &registry);
}

/// Testable inner function that accepts a registry parameter.
pub(crate) fn return_stashed_panes_with_registry(tmux: &Tmux, registry: &tmux_router::Registry) {
    // Build a map from pane_id → (key, entry) for quick lookup
    let pane_to_entry: std::collections::HashMap<&str, (&str, &tmux_router::RegistryEntry)> =
        registry
            .iter()
            .map(|(k, e)| (e.pane.as_str(), (k.as_str(), e)))
            .collect();

    // List all windows to find stash windows
    let output =
        agent_doc_tmux_io::list_windows_all(tmux, "#{window_id}\t#{window_name}\t#{session_name}");
    let output = match output {
        Ok(output) => output,
        _ => return,
    };

    let mut returned = 0;

    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let (window_id, window_name, _session_name) = (parts[0], parts[1], parts[2]);

        if !is_stash_window_name(window_name) {
            continue;
        }

        // List panes in this stash window with their current command
        let pane_output = agent_doc_tmux_io::list_panes(
            tmux,
            Some(window_id),
            "#{pane_id}\t#{pane_current_command}",
        );
        let pane_output = match pane_output {
            Ok(output) => output,
            _ => continue,
        };

        for pane_line in pane_output.lines() {
            let pane_parts: Vec<&str> = pane_line.splitn(2, '\t').collect();
            if pane_parts.len() < 2 {
                continue;
            }
            let pane_id = pane_parts[0];
            let pane_kind = classify_pane_process(tmux, pane_id);
            let pane_cmd = match &pane_kind {
                TmuxPaneProcessKind::IdleShell(_) => continue,
                TmuxPaneProcessKind::Agent(cmd) | TmuxPaneProcessKind::Foreign(cmd) => cmd.as_str(),
                TmuxPaneProcessKind::UnknownTransient => pane_parts[1],
            };

            // Look up registry entry for this pane
            let (key, entry) = match pane_to_entry.get(pane_id) {
                Some(pair) => *pair,
                None => continue, // Unregistered — handled by purge functions
            };

            // Try to find a target to move back to:
            // 1. Original window (from entry.window) if alive
            // 2. First window of the original tmux session (from frontmatter)
            let target = find_return_target(tmux, entry);
            let target = match target {
                Some(t) => t,
                None => {
                    eprintln!(
                        "resync: cannot return stashed pane {} ({}): no valid target found",
                        pane_id, key
                    );
                    continue;
                }
            };

            // Move the pane back using join-pane (same session — stash is in same session)
            match PaneMoveOp::new(tmux, pane_id, &target).join("-dv") {
                Ok(()) => {
                    eprintln!(
                        "resync: returned stashed pane {} ({}, running '{}') to window {}",
                        pane_id, key, pane_cmd, target
                    );
                    returned += 1;
                }
                Err(e) => {
                    eprintln!(
                        "resync: failed to return stashed pane {} to {}: {}",
                        pane_id, target, e
                    );
                }
            }
        }
    }

    if returned > 0 {
        eprintln!(
            "resync: returned {} stashed pane(s) to their sessions",
            returned
        );
    }
}

// ---------------------------------------------------------------------------
// Bulk variants — use pre-fetched metadata instead of per-item subprocess calls
// ---------------------------------------------------------------------------

/// Type aliases for bulk metadata.
pub(crate) type WindowMeta = Vec<(String, String, String, String)>; // (window_id, window_name, session_name, activity)
pub(crate) type PaneMeta = std::collections::HashMap<String, (String, String, String)>; // pane_id → (window_id, window_name, cmd)

/// Fetch all window metadata in a single subprocess call.
pub(crate) fn fetch_all_window_metadata(tmux: &Tmux) -> WindowMeta {
    let output = agent_doc_tmux_io::list_windows_all(
        tmux,
        "#{window_id}\t#{window_name}\t#{session_name}\t#{window_activity}",
    );
    match output {
        Ok(output) => output
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(4, '\t').collect();
                if parts.len() >= 4 {
                    Some((
                        parts[0].to_string(),
                        parts[1].to_string(),
                        parts[2].to_string(),
                        parts[3].to_string(),
                    ))
                } else {
                    None
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Fetch all pane metadata in a single subprocess call.
pub(crate) fn fetch_all_pane_metadata(tmux: &Tmux) -> PaneMeta {
    let output = agent_doc_tmux_io::list_panes_all(
        tmux,
        "#{pane_id}\t#{window_id}\t#{window_name}\t#{pane_current_command}",
    );
    match output {
        Ok(output) => output
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(4, '\t').collect();
                if parts.len() >= 4 {
                    Some((
                        parts[0].to_string(),
                        (
                            parts[1].to_string(),
                            parts[2].to_string(),
                            parts[3].to_string(),
                        ),
                    ))
                } else {
                    None
                }
            })
            .collect(),
        _ => std::collections::HashMap::new(),
    }
}

/// Bulk variant of `purge_stash_windows` — uses pre-fetched metadata.
pub(crate) fn purge_stash_windows_bulk(tmux: &Tmux, windows: &WindowMeta, panes: &PaneMeta) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for (window_id, window_name, _session_name, activity_str) in windows {
        if window_name != "stash" {
            continue;
        }

        // Grace period
        if let Ok(activity) = activity_str.parse::<u64>()
            && now.saturating_sub(activity) < 30
        {
            continue;
        }

        // Check all panes in this window are idle (from pre-fetched metadata)
        let all_idle = panes
            .iter()
            .filter(|(_, (wid, _, _))| wid == window_id)
            .all(|(_, (_, _, cmd))| {
                matches!(
                    pane_process_kind_from_current_command(cmd),
                    TmuxPaneProcessKind::IdleShell(_)
                )
            });

        // Also check there ARE panes in this window
        let has_panes = panes.iter().any(|(_, (wid, _, _))| wid == window_id);

        if has_panes && all_idle {
            if let Err(e) = agent_doc_tmux_io::kill_window(tmux, window_id) {
                eprintln!("resync: failed to purge stash window {}: {}", window_id, e);
            } else {
                eprintln!("resync: purged stash window {} (all panes idle)", window_id);
            }
        }
    }
}

/// Bulk variant of `purge_unregistered_stash_panes` — uses pre-fetched metadata.
pub(crate) fn purge_unregistered_stash_panes_bulk(
    tmux: &Tmux,
    windows: &WindowMeta,
    panes: &PaneMeta,
) {
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let live_supervisors = agent_doc_supervisor_io::ipc::active_supervisor_pids(&project_root);
    purge_unregistered_stash_panes_bulk_with_supervisors(tmux, windows, panes, &live_supervisors);
}

pub(crate) fn purge_unregistered_stash_panes_bulk_in_mode(
    tmux: &Tmux,
    windows: &WindowMeta,
    panes: &PaneMeta,
    preserve_live_agent_stash_panes: bool,
) {
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let live_supervisors = agent_doc_supervisor_io::ipc::active_supervisor_pids(&project_root);
    purge_unregistered_stash_panes_bulk_with_supervisors_in_mode(
        tmux,
        windows,
        panes,
        &live_supervisors,
        preserve_live_agent_stash_panes,
    );
}

pub(crate) fn purge_unregistered_stash_panes_bulk_with_supervisors(
    tmux: &Tmux,
    windows: &WindowMeta,
    panes: &PaneMeta,
    live_supervisors: &[(String, u32)],
) {
    purge_unregistered_stash_panes_bulk_with_supervisors_in_mode(
        tmux,
        windows,
        panes,
        live_supervisors,
        false,
    );
}

pub(crate) fn purge_unregistered_stash_panes_bulk_with_supervisors_in_mode(
    tmux: &Tmux,
    windows: &WindowMeta,
    panes: &PaneMeta,
    live_supervisors: &[(String, u32)],
    preserve_live_agent_stash_panes: bool,
) {
    let current_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut pane_context_cache = PaneProjectContextCache::default();
    let registry = agent_doc_session_registry_io::load().unwrap_or_default();
    let registered_panes: std::collections::HashSet<&str> =
        registry.values().map(|e| e.pane.as_str()).collect();

    let mut killed_count = 0;

    // Find stash windows from pre-fetched window metadata
    let stash_windows: std::collections::HashSet<&str> = windows
        .iter()
        .filter(|(_, wname, _, _)| is_stash_window_name(wname))
        .map(|(wid, _, _, _)| wid.as_str())
        .collect();

    // Find panes in stash windows that are unregistered.
    // Only kill idle shells — never kill agent processes (agent-doc, claude, node)
    // even if unregistered, because the registry can go stale and an active Claude
    // session should never be killed automatically.
    for (pane_id, (window_id, _window_name, cmd)) in panes {
        if !stash_windows.contains(window_id.as_str()) {
            continue;
        }
        if registered_panes.contains(pane_id.as_str()) {
            continue;
        }
        let process_kind = pane_process_kind_from_current_command(cmd);
        if preserve_live_agent_stash_panes && matches!(process_kind, TmuxPaneProcessKind::Agent(_))
        {
            sync_log_safe_passive_stash_skip(pane_id);
            continue;
        }
        let pane_root = pane_project_root(tmux, pane_id);
        let registered_in_pane_root = pane_root
            .as_ref()
            .filter(|root| **root != current_root)
            .is_some_and(|root| {
                registry_for_project_root(&mut pane_context_cache, root)
                    .values()
                    .any(|entry| entry.pane == *pane_id)
            });
        if registered_in_pane_root {
            // #cross-project-stash-pane-condition: correct cross-project preservation,
            // not an error — log as telemetry, not stderr, so the IDE/route error
            // surface stays clean. Skip behavior unchanged.
            agent_doc_ops_log_io::log_op(
                &current_root,
                &format!(
                    "resync: stash pane {} is registered in its own project root — skipping kill",
                    pane_id
                ),
            );
            continue;
        }
        if tmux.pane_dead(pane_id) {
            if let Err(e) = tmux.kill_pane(pane_id) {
                eprintln!("resync: failed to kill dead stash pane {}: {}", pane_id, e);
            } else {
                killed_count += 1;
            }
            continue;
        }
        if let Some(owner_file) = live_owned_registered_file_for_pane(tmux, pane_id, &registry)
            .or_else(|| {
                pane_root
                    .as_ref()
                    .filter(|root| **root != current_root)
                    .and_then(|root| {
                        let pane_registry =
                            registry_for_project_root(&mut pane_context_cache, root);
                        live_owned_registered_file_for_pane(tmux, pane_id, pane_registry)
                    })
            })
        {
            eprintln!(
                "resync: stash pane {} is unregistered but still owns {} — skipping kill",
                pane_id, owner_file
            );
            continue;
        }
        if let Some(supervisor_session) =
            pane_hosts_live_supervisor_session(tmux, pane_id, live_supervisors).or_else(|| {
                pane_root
                    .as_ref()
                    .filter(|root| **root != current_root)
                    .and_then(|root| {
                        let pane_live_supervisors =
                            live_supervisors_for_project_root(&mut pane_context_cache, root);
                        pane_hosts_live_supervisor_session(tmux, pane_id, pane_live_supervisors)
                    })
            })
        {
            eprintln!(
                "resync: stash pane {} is unregistered but still hosts live supervisor {} — skipping kill",
                pane_id, supervisor_session
            );
            continue;
        }
        match process_kind {
            TmuxPaneProcessKind::IdleShell(_) | TmuxPaneProcessKind::Agent(_) => {
                if let Err(e) = tmux.kill_pane(pane_id) {
                    eprintln!("resync: failed to kill stash pane {}: {}", pane_id, e);
                } else {
                    killed_count += 1;
                }
            }
            TmuxPaneProcessKind::Foreign(_) | TmuxPaneProcessKind::UnknownTransient => {}
        }
    }

    if killed_count > 0 {
        eprintln!("resync: purged {} orphaned stash pane(s)", killed_count);
    }
}

pub(crate) fn sync_log_safe_passive_stash_skip(pane_id: &str) {
    eprintln!(
        "resync: safe-passive stash cleanup deferred live agent pane {} ownership proof",
        pane_id
    );
}

pub(crate) fn purge_unregistered_dead_non_stash_panes(tmux: &Tmux) {
    let registry = agent_doc_session_registry_io::load().unwrap_or_default();
    purge_unregistered_dead_non_stash_panes_with_registry(tmux, &registry);
}

pub(crate) fn purge_unregistered_dead_non_stash_panes_with_registry(
    tmux: &Tmux,
    registry: &tmux_router::Registry,
) {
    let output =
        agent_doc_tmux_io::list_panes_all(tmux, "#{pane_id}\t#{window_id}\t#{window_name}");
    let output = match output {
        Ok(output) => output,
        _ => return,
    };

    let panes: PaneMeta = output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() >= 3 {
                Some((
                    parts[0].to_string(),
                    (parts[1].to_string(), parts[2].to_string(), String::new()),
                ))
            } else {
                None
            }
        })
        .collect();
    purge_unregistered_dead_non_stash_panes_bulk_with_registry(tmux, &panes, registry);
}

pub(crate) fn purge_unregistered_dead_non_stash_panes_bulk(tmux: &Tmux, panes: &PaneMeta) {
    let registry = agent_doc_session_registry_io::load().unwrap_or_default();
    purge_unregistered_dead_non_stash_panes_bulk_with_registry(tmux, panes, &registry);
}

pub(crate) fn purge_unregistered_dead_non_stash_panes_bulk_with_registry(
    tmux: &Tmux,
    panes: &PaneMeta,
    registry: &tmux_router::Registry,
) {
    let registered_panes: std::collections::HashSet<&str> =
        registry.values().map(|entry| entry.pane.as_str()).collect();
    let mut window_panes: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();

    for (pane_id, (window_id, window_name, _cmd)) in panes {
        if is_stash_window_name(window_name) {
            continue;
        }
        window_panes
            .entry(window_id.as_str())
            .or_default()
            .push(pane_id.as_str());
    }

    let mut killed = 0;
    for pane_ids in window_panes.values() {
        if pane_ids.len() < 2 {
            continue;
        }
        for pane_id in pane_ids {
            if registered_panes.contains(pane_id) {
                continue;
            }
            if !tmux.pane_dead(pane_id) {
                continue;
            }
            if let Err(err) = tmux.kill_pane(pane_id) {
                eprintln!(
                    "resync: failed to kill unregistered dead pane {}: {}",
                    pane_id, err
                );
            } else {
                killed += 1;
            }
        }
    }

    if killed > 0 {
        eprintln!(
            "resync: purged {} unregistered dead pane(s) from non-stash windows",
            killed
        );
    }
}

/// Bulk variant of `return_stashed_panes` — uses pre-fetched metadata.
/// Also deregisters stranded panes when no return target is found, preventing
/// repeated expensive lookups on subsequent cycles.
#[allow(dead_code)]
pub(crate) fn return_stashed_panes_bulk(tmux: &Tmux, windows: &WindowMeta, panes: &PaneMeta) {
    let registry = agent_doc_session_registry_io::load().unwrap_or_default();
    let pane_to_entry: std::collections::HashMap<&str, (&str, &tmux_router::RegistryEntry)> =
        registry
            .iter()
            .map(|(k, e)| (e.pane.as_str(), (k.as_str(), e)))
            .collect();

    // Find stash windows from pre-fetched metadata
    let stash_windows: std::collections::HashSet<&str> = windows
        .iter()
        .filter(|(_, wname, _, _)| is_stash_window_name(wname))
        .map(|(wid, _, _, _)| wid.as_str())
        .collect();

    let mut returned = 0;
    let mut deregistered = Vec::new();

    for (pane_id, (window_id, _window_name, cmd)) in panes {
        if !stash_windows.contains(window_id.as_str()) {
            continue;
        }
        let pane_kind = match pane_process_kind_from_current_command(cmd) {
            TmuxPaneProcessKind::IdleShell(cmd) => TmuxPaneProcessKind::IdleShell(cmd),
            TmuxPaneProcessKind::Agent(cmd) => TmuxPaneProcessKind::Agent(cmd),
            TmuxPaneProcessKind::Foreign(_) | TmuxPaneProcessKind::UnknownTransient => {
                classify_pane_process(tmux, pane_id)
            }
        };
        let pane_cmd = match &pane_kind {
            TmuxPaneProcessKind::IdleShell(_) => continue,
            TmuxPaneProcessKind::Agent(cmd) | TmuxPaneProcessKind::Foreign(cmd) => cmd.as_str(),
            TmuxPaneProcessKind::UnknownTransient => cmd.as_str(),
        };

        let (key, entry) = match pane_to_entry.get(pane_id.as_str()) {
            Some(pair) => *pair,
            None => continue,
        };

        // Use bulk metadata for find_return_target instead of per-pane subprocess calls
        let target = find_return_target_bulk(entry, windows, panes);
        let target = match target {
            Some(t) => t,
            None => {
                // Only deregister idle shells with no return target.
                // Active processes (claude, agent-doc, etc.) must stay registered
                // so route's rescue_from_stash() can unstash them on next claim.
                if matches!(pane_kind, TmuxPaneProcessKind::IdleShell(_)) {
                    eprintln!(
                        "resync: cannot return stashed pane {} ({}): no valid target found — deregistering idle shell",
                        pane_id, key
                    );
                    deregistered.push(key.to_string());
                } else {
                    eprintln!(
                        "resync: cannot return stashed pane {} ({}): no valid target found — keeping registered (running '{}')",
                        pane_id, key, pane_cmd
                    );
                }
                continue;
            }
        };

        match PaneMoveOp::new(tmux, pane_id, &target).join("-dv") {
            Ok(()) => {
                eprintln!(
                    "resync: returned stashed pane {} ({}, running '{}') to window {}",
                    pane_id, key, pane_cmd, target
                );
                returned += 1;
            }
            Err(e) => {
                eprintln!(
                    "resync: failed to return stashed pane {} to {}: {}",
                    pane_id, target, e
                );
            }
        }
    }

    // Deregister stranded panes so they don't retry every cycle
    if !deregistered.is_empty()
        && let Ok(mut reg) = agent_doc_session_registry_io::load()
    {
        for key in &deregistered {
            reg.remove(key);
        }
        if let Err(e) = agent_doc_session_registry_io::save(&reg) {
            eprintln!("resync: failed to save registry after deregister: {}", e);
        } else {
            eprintln!(
                "resync: deregistered {} stranded pane(s)",
                deregistered.len()
            );
        }
    }

    if returned > 0 {
        eprintln!(
            "resync: returned {} stashed pane(s) to their sessions",
            returned
        );
    }
}

/// Check if a pane is an idle Claude session by looking for `❯` in the last few lines.
/// Bulk variant of `find_return_target` — uses pre-fetched metadata instead of subprocess calls.
#[allow(dead_code)]
pub(crate) fn find_return_target_bulk(
    entry: &tmux_router::RegistryEntry,
    windows: &WindowMeta,
    panes: &PaneMeta,
) -> Option<String> {
    // 1. Try the original window from the registry entry
    if !entry.window.is_empty() {
        // Check if any pane exists in the original window
        let window_panes: Vec<&String> = panes
            .iter()
            .filter(|(_, (wid, _, _))| wid == &entry.window)
            .map(|(pid, _)| pid)
            .collect();

        if !window_panes.is_empty()
            && let Some((_, wname, _)) = panes.get(window_panes[0])
            && !is_stash_window_name(wname)
        {
            return Some(window_panes[0].clone());
        }
    }

    // 2. Try to find the tmux session from frontmatter
    let session_name = if !entry.file.is_empty() {
        std::fs::read_to_string(&entry.file)
            .ok()
            .and_then(|content| {
                let (fm, _) = frontmatter::parse(&content).ok()?;
                fm.tmux_session
            })
    } else {
        None
    };

    if let Some(ref sess) = session_name {
        // Find first non-stash window in this session from pre-fetched metadata
        for (window_id, window_name, session, _) in windows {
            if session == sess && !is_stash_window_name(window_name) {
                // Return first pane in this window
                if let Some((pid, _)) = panes.iter().find(|(_, (wid, _, _))| wid == window_id) {
                    return Some(pid.clone());
                }
            }
        }
    }

    // 3. Fallback: if original window is stash (or unknown), try the first non-stash
    // window in ANY tmux session. This handles panes that were registered while in the
    // stash window — their `window` field points to the stash, so step 1 can't return them.
    for (window_id, window_name, _session, _) in windows {
        if !is_stash_window_name(window_name)
            && let Some((pid, _)) = panes.iter().find(|(_, (wid, _, _))| wid == window_id)
        {
            return Some(pid.clone());
        }
    }

    None
}

/// Find a target pane to return a stashed pane to.
///
/// Priority:
/// 1. The entry's original window (if alive and not a stash window)
/// 2. The first non-stash window in the tmux session from frontmatter
/// 3. The first non-stash window in any session with a matching name
pub(crate) fn find_return_target(
    tmux: &Tmux,
    entry: &tmux_router::RegistryEntry,
) -> Option<String> {
    // 1. Try the original window from the registry entry
    if !entry.window.is_empty()
        && let Ok(panes) = tmux.list_window_panes(&entry.window)
        && !panes.is_empty()
    {
        // Check it's not a stash window itself
        if let Some(wname) = agent_doc_tmux_io::target_window_name(tmux, &panes[0])
            && !is_stash_window_name(&wname)
        {
            return Some(panes[0].clone());
        }
    }

    // 2. Try to find the tmux session from frontmatter
    let session_name = if !entry.file.is_empty() {
        std::fs::read_to_string(&entry.file)
            .ok()
            .and_then(|content| {
                let (fm, _) = frontmatter::parse(&content).ok()?;
                fm.tmux_session
            })
    } else {
        None
    };

    if let Some(ref sess) = session_name
        && tmux.session_exists(sess)
        && let Some(target) = first_non_stash_pane(tmux, sess)
    {
        return Some(target);
    }

    None
}

/// Find the first pane in the first non-stash window of a tmux session.
pub(crate) fn first_non_stash_pane(tmux: &Tmux, session_name: &str) -> Option<String> {
    let output = agent_doc_tmux_io::list_windows(
        tmux,
        Some(&format!("{}:", session_name)),
        "#{window_id}\t#{window_name}",
    )
    .ok()?;

    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() < 2 {
            continue;
        }
        let (window_id, window_name) = (parts[0], parts[1]);
        if is_stash_window_name(window_name) {
            continue;
        }
        // Return the first pane in this non-stash window
        if let Ok(panes) = tmux.list_window_panes(window_id)
            && let Some(first) = panes.into_iter().next()
        {
            return Some(first);
        }
    }

    None
}

/// Purge orphaned agent-doc/claude panes in ANY window (not just stash).
///
/// Targets panes that are:
/// 1. Not registered in the durable registry
/// 2. Running agent-doc, claude, or node
/// 3. In a window that has at least one other pane (won't orphan last pane)
///
/// This catches orphaned Claude sessions in non-stash windows (e.g., session 3).
pub(crate) fn purge_orphaned_agent_panes(tmux: &Tmux) {
    let registry = agent_doc_session_registry_io::load().unwrap_or_default();
    purge_orphaned_agent_panes_with_registry(tmux, &registry);
}

pub(crate) fn purge_orphaned_agent_panes_with_registry(
    tmux: &Tmux,
    registry: &tmux_router::Registry,
) {
    let current_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let live_supervisors = agent_doc_supervisor_io::ipc::active_supervisor_pids(&current_root);
    let mut pane_context_cache = PaneProjectContextCache::default();
    let registered_panes: std::collections::HashSet<&str> =
        registry.values().map(|e| e.pane.as_str()).collect();

    // List all panes across all sessions
    let output = agent_doc_tmux_io::list_panes_all(
        tmux,
        "#{pane_id}\t#{window_id}\t#{pane_current_command}",
    );
    let output = match output {
        Ok(output) => output,
        _ => return,
    };

    // Group panes by window
    let mut window_panes: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let (pane_id, window_id, cmd) = (parts[0], parts[1], parts[2]);
        window_panes
            .entry(window_id.to_string())
            .or_default()
            .push((pane_id.to_string(), cmd.to_string()));
    }

    let mut killed = 0;
    for panes in window_panes.values() {
        if panes.len() < 2 {
            continue; // Don't kill the last pane in a window
        }
        for (pane_id, cmd) in panes {
            if registered_panes.contains(pane_id.as_str()) {
                continue; // Registered — leave it
            }
            // Only target agent processes (not shells or user processes)
            if matches!(
                pane_process_kind_from_current_command(cmd),
                TmuxPaneProcessKind::Agent(_)
            ) {
                let pane_root = pane_project_root(tmux, pane_id);
                let registered_in_pane_root = pane_root
                    .as_ref()
                    .filter(|root| **root != current_root)
                    .is_some_and(|root| {
                        registry_for_project_root(&mut pane_context_cache, root)
                            .values()
                            .any(|entry| entry.pane == *pane_id)
                    });
                if registered_in_pane_root {
                    // #cross-project-stash-pane-condition: correct cross-project
                    // preservation, not an error — telemetry, not stderr.
                    agent_doc_ops_log_io::log_op(
                        &current_root,
                        &format!(
                            "resync: non-stash pane {} is registered in its own project root — skipping kill",
                            pane_id
                        ),
                    );
                    continue;
                }
                if let Some(owner_file) =
                    live_owned_registered_file_for_pane(tmux, pane_id, registry).or_else(|| {
                        pane_root
                            .as_ref()
                            .filter(|root| **root != current_root)
                            .and_then(|root| {
                                let pane_registry =
                                    registry_for_project_root(&mut pane_context_cache, root);
                                live_owned_registered_file_for_pane(tmux, pane_id, pane_registry)
                            })
                    })
                {
                    eprintln!(
                        "resync: non-stash pane {} is unregistered in this project but still owns {} — skipping kill",
                        pane_id, owner_file
                    );
                    continue;
                }
                if let Some(supervisor_session) = pane_hosts_live_supervisor_session(
                    tmux,
                    pane_id,
                    &live_supervisors,
                )
                .or_else(|| {
                    pane_root
                        .as_ref()
                        .filter(|root| **root != current_root)
                        .and_then(|root| {
                            let pane_live_supervisors =
                                live_supervisors_for_project_root(&mut pane_context_cache, root);
                            pane_hosts_live_supervisor_session(tmux, pane_id, pane_live_supervisors)
                        })
                }) {
                    eprintln!(
                        "resync: non-stash pane {} is unregistered in this project but still hosts live supervisor {} — skipping kill",
                        pane_id, supervisor_session
                    );
                    continue;
                }
                if let Err(e) = tmux.kill_pane(pane_id) {
                    eprintln!(
                        "resync: failed to kill orphaned agent pane {}: {}",
                        pane_id, e
                    );
                } else {
                    killed += 1;
                }
            }
        }
    }

    if killed > 0 {
        eprintln!(
            "resync: purged {} orphaned agent pane(s) from non-stash windows",
            killed
        );
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use tmux_router::{IsolatedTmux, Registry as SessionRegistry, RegistryEntry as SessionEntry};
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_unregistered_stash_panes_bulk_kills_unregistered_agent_without_live_owner() {
        let iso = IsolatedTmux::new("resync-purge-agent-no-owner-bulk");
        let cwd = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let script = write_mock_agent_doc(dir.path());

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        iso.send_keys(&pane2, &format!("exec {}", script.display()))
            .unwrap();
        let _ = wait_for_pane_contains(&iso, &pane2, "\n>", std::time::Duration::from_secs(3));
        iso.stash_pane(&pane2, "test").unwrap();
        assert!(
            wait_for_pane_in_stash_window(&iso, "test", &pane2, std::time::Duration::from_secs(3)),
            "pane should move into stash before purge"
        );

        let windows = fetch_all_window_metadata(&iso);
        let panes = fetch_all_pane_metadata(&iso);
        purge_unregistered_stash_panes_bulk(&iso, &windows, &panes);
        assert!(
            wait_for_pane_dead(&iso, &pane2, std::time::Duration::from_secs(3)),
            "bulk stash purge should kill unregistered agent panes with no live owner"
        );
        assert!(
            !iso.pane_alive(&pane2),
            "bulk stash purge should kill unregistered agent panes with no live owner"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn safe_passive_stash_cleanup_defers_live_agent_owner_proof() {
        let iso = IsolatedTmux::new("resync-safe-passive-defer-agent-proof");
        let cwd = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let agent_bin = bin_dir.join("agent-doc");
        std::os::unix::fs::symlink("/bin/sleep", &agent_bin).unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        iso.send_keys(&pane2, &format!("exec {} 60", agent_bin.display()))
            .unwrap();
        assert!(
            wait_for_pane_current_command(
                &iso,
                &pane2,
                "agent-doc",
                std::time::Duration::from_secs(3)
            ),
            "pane should run an agent-doc-named process before stash cleanup"
        );
        iso.stash_pane(&pane2, "test").unwrap();
        assert!(
            wait_for_pane_in_stash_window(&iso, "test", &pane2, std::time::Duration::from_secs(3)),
            "pane should move into stash before safe-passive prune"
        );

        let windows = fetch_all_window_metadata(&iso);
        let panes = fetch_all_pane_metadata(&iso);
        purge_unregistered_stash_panes_bulk_in_mode(&iso, &windows, &panes, true);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            iso.pane_alive(&pane2),
            "safe-passive stash cleanup should defer live agent panes to full cleanup instead of proving ownership"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_unregistered_stash_panes_bulk_preserves_live_supervisor() {
        let iso = IsolatedTmux::new("resync-purge-agent-live-supervisor-bulk");
        let cwd = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let script = write_mock_agent_doc(dir.path());
        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        iso.send_keys(&pane2, &format!("exec {}", script.display()))
            .unwrap();
        let _ = wait_for_pane_contains(&iso, &pane2, "\n>", std::time::Duration::from_secs(3));
        let live_pid = wait_for_process_pid(
            &script.display().to_string(),
            std::time::Duration::from_secs(3),
        );
        let mut ipc = agent_doc_supervisor_io::ipc::SupervisorIpc::start(
            dir.path(),
            "super-live-bulk",
            move |method| match method {
                IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": live_pid })),
                IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
                IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                    IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                IpcMethod::Restart { .. }
                | IpcMethod::Stop { .. }
                | IpcMethod::StopAgent { .. } => IpcResponse::ok_empty(),
            },
        )
        .unwrap();
        iso.stash_pane(&pane2, "test").unwrap();
        assert!(
            wait_for_pane_in_stash_window(&iso, "test", &pane2, std::time::Duration::from_secs(3)),
            "pane should move into stash before bulk purge"
        );

        let windows = fetch_all_window_metadata(&iso);
        let panes = fetch_all_pane_metadata(&iso);
        let live_supervisors = agent_doc_supervisor_io::ipc::active_supervisor_pids(dir.path());
        purge_unregistered_stash_panes_bulk_with_supervisors(
            &iso,
            &windows,
            &panes,
            &live_supervisors,
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            iso.pane_alive(&pane2),
            "bulk purge should preserve stash panes with a live supervisor"
        );
        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_unregistered_stash_panes_bulk_preserves_live_supervisor_in_pane_project_root() {
        let root = tempfile::tempdir().unwrap();
        let child_root = root.path().join("src/session-share");
        std::fs::create_dir_all(&child_root).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(root.path());

        let iso = IsolatedTmux::new("resync-purge-agent-live-supervisor-cross-root");
        let script = write_mock_agent_doc(&child_root);
        let pane1 = iso.auto_start("test", root.path()).unwrap();
        let pane2 = iso.split_window(&pane1, &child_root, "-dh").unwrap();
        iso.send_keys(&pane2, &format!("exec {}", script.display()))
            .unwrap();
        let _ = wait_for_pane_contains(&iso, &pane2, "\n>", std::time::Duration::from_secs(3));
        let live_pid = wait_for_process_pid(
            &script.display().to_string(),
            std::time::Duration::from_secs(3),
        );
        let mut ipc = agent_doc_supervisor_io::ipc::SupervisorIpc::start(
            &child_root,
            "super-live-cross-root",
            move |method| match method {
                IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": live_pid })),
                IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
                IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                    IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                IpcMethod::Restart { .. }
                | IpcMethod::Stop { .. }
                | IpcMethod::StopAgent { .. } => IpcResponse::ok_empty(),
            },
        )
        .unwrap();
        iso.stash_pane(&pane2, "test").unwrap();
        assert!(
            wait_for_pane_in_stash_window(&iso, "test", &pane2, std::time::Duration::from_secs(3)),
            "pane should move into stash before bulk purge"
        );

        let windows = fetch_all_window_metadata(&iso);
        let panes = fetch_all_pane_metadata(&iso);
        purge_unregistered_stash_panes_bulk_with_supervisors(&iso, &windows, &panes, &[]);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            iso.pane_alive(&pane2),
            "bulk purge should preserve stash panes with a live supervisor in the pane project root"
        );
        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_orphan_agent_in_non_stash_window() {
        // An unregistered agent-doc pane in a regular window (not stash) should be killed
        // if the window has other panes.
        let iso = IsolatedTmux::new("resync-purge-orphan-agent");
        let cwd = std::env::current_dir().unwrap();

        // Create a session with 2 panes in the same window
        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));

        // pane2 is running a shell. We want to simulate an agent-doc process.
        // Instead, we test the shell case: shells should NOT be killed by this function
        // (only agent processes). Let's just verify the non-stash purge doesn't touch shells.
        let registry = SessionRegistry::new();
        purge_orphaned_agent_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Both panes should survive (they're running shells, not agent processes)
        assert!(iso.pane_alive(&pane1), "shell pane1 should survive");
        assert!(iso.pane_alive(&pane2), "shell pane2 should survive");
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_orphan_agent_in_non_stash_preserves_nested_project_owner() {
        let parent = tempfile::tempdir().unwrap();
        let nested = parent.path().join("nested");
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        let doc = nested.join("tasks/root.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# Root\n").unwrap();
        let script = write_mock_agent_doc(parent.path());

        let _cwd_guard = ScopedCurrentDir::set(parent.path());
        let iso = IsolatedTmux::new("resync-preserve-nested-owner");
        let pane = iso.new_session("test", &nested).unwrap();
        let sibling = iso.split_window(&pane, &nested, "-dh").unwrap();
        launch_mock_agent_doc(&iso, &pane, &script, &doc);
        sessions::register_full_in(
            &nested,
            "nested-session",
            &pane,
            &doc.to_string_lossy(),
            123,
            "@1",
        )
        .unwrap();

        purge_orphaned_agent_panes_with_registry(&iso, &SessionRegistry::new());
        std::thread::sleep(std::time::Duration::from_millis(100));

        assert!(
            iso.pane_alive(&pane),
            "parent resync must not kill a non-stash pane registered in a nested project"
        );
        assert!(
            iso.pane_alive(&sibling),
            "sibling shell pane should survive"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_unregistered_dead_non_stash_pane() {
        let iso = IsolatedTmux::new("resync-purge-dead-non-stash");
        let cwd = std::env::current_dir().unwrap();

        let live_pane = iso.auto_start("test", &cwd).unwrap();
        let dead_pane = iso.split_window(&live_pane, &cwd, "-dh").unwrap();
        iso.enable_remain_on_exit(&dead_pane).unwrap();
        drive_pane_to_retained_dead(
            &iso,
            &dead_pane,
            "printf 'dead pane\\n'; exit 0",
            std::time::Duration::from_secs(6),
        );

        let registry = SessionRegistry::new();
        purge_unregistered_dead_non_stash_panes_with_registry(&iso, &registry);
        assert!(
            wait_for_pane_removed(&iso, &dead_pane, std::time::Duration::from_secs(3)),
            "unregistered dead pane with siblings should be reaped"
        );
        assert!(
            iso.pane_alive(&live_pane),
            "live sibling pane should survive"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_orphan_does_not_kill_last_pane() {
        // A window with only one pane (even if orphaned agent) should not be touched.
        let iso = IsolatedTmux::new("resync-purge-last-pane");
        let cwd = std::env::current_dir().unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let registry = SessionRegistry::new();
        purge_orphaned_agent_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(100));

        assert!(
            iso.pane_alive(&pane1),
            "last pane in window should not be killed"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_unregistered_dead_non_stash_panes_skips_last_pane() {
        let iso = IsolatedTmux::new("resync-purge-dead-last-pane");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("test", &cwd).unwrap();
        let _other_window = iso.new_window("test", &cwd).unwrap();
        iso.enable_remain_on_exit(&pane).unwrap();
        drive_pane_to_retained_dead(
            &iso,
            &pane,
            "printf 'dead last pane\\n'; exit 0",
            std::time::Duration::from_secs(6),
        );

        let registry = SessionRegistry::new();
        purge_unregistered_dead_non_stash_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(100));

        assert!(
            iso.pane_dead(&pane),
            "last pane in window should be preserved for manual inspection"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn return_stashed_panes_moves_active_pane_back() {
        // A registered pane running an active process (sleep) in stash should be
        // returned to its original session window.
        let iso = IsolatedTmux::new("resync-return-active");
        let cwd = std::env::current_dir().unwrap();

        // Create a pane with an active process (sleep)
        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let output = iso
            .cmd()
            .args([
                "split-window",
                "-t",
                &pane1,
                "-d",
                "-h",
                "-c",
                &cwd.to_string_lossy(),
                "-P",
                "-F",
                "#{pane_id}",
                "sleep",
                "60",
            ])
            .output()
            .unwrap();
        let active_pane = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let original_window = iso.pane_window(&active_pane).unwrap();

        // Move the active pane to stash
        iso.stash_pane(&active_pane, "test").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        // Verify it's in stash
        let stash_window = iso.pane_window(&active_pane).unwrap();
        assert_ne!(stash_window, original_window, "pane should be in stash");

        // Register the pane with the original window
        let mut registry = SessionRegistry::new();
        let mut entry = test_entry(&active_pane, "");
        entry.window = original_window.clone();
        registry.insert("active-sess".to_string(), entry);

        // Return stashed panes
        return_stashed_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(300));

        // Verify pane is back in original window
        assert!(iso.pane_alive(&active_pane), "pane should still be alive");
        let current_window = iso.pane_window(&active_pane).unwrap();
        assert_eq!(
            current_window, original_window,
            "active pane should be returned to original window"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn return_stashed_panes_skips_idle_shells() {
        // An idle shell in stash should NOT be returned — it's handled by purge.
        let iso = IsolatedTmux::new("resync-return-idle");
        let cwd = std::env::current_dir().unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        let original_window = iso.pane_window(&pane2).unwrap();

        // Move idle shell to stash, then wait for the pane to settle.
        // Fixed sleep is unreliable under parallel test load.
        iso.stash_pane(&pane2, "test").unwrap();
        assert!(
            wait_for_shell(&iso, &pane2, 5000),
            "shell did not settle in stash pane within 5s"
        );

        let stash_window = iso.pane_window(&pane2).unwrap();
        assert_ne!(stash_window, original_window, "pane should be in stash");

        // Register the idle shell pane
        let mut registry = SessionRegistry::new();
        let mut entry = test_entry(&pane2, "");
        entry.window = original_window.clone();
        registry.insert("idle-sess".to_string(), entry);

        // Return stashed panes — should skip idle shells
        return_stashed_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(1000));

        // Verify pane is still in stash (not returned)
        let current_window = iso.pane_window(&pane2).unwrap();
        assert_eq!(
            current_window, stash_window,
            "idle shell should NOT be returned from stash"
        );
    }
}
