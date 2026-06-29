//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn prune_dead_entries_for_target_in_registry<F>(
    registry: &mut tmux_router::Registry,
    target: &Path,
    mut pane_alive: F,
) -> Vec<(String, tmux_router::RegistryEntry)>
where
    F: FnMut(&str) -> bool,
{
    let removed: Vec<(String, tmux_router::RegistryEntry)> = registry
        .iter()
        .filter(|(key, entry)| {
            (same_document_path(target, &entry.file) || same_document_path(target, key))
                && !pane_alive(&entry.pane)
        })
        .map(|(key, entry)| (key.clone(), entry.clone()))
        .collect();

    for (key, _) in &removed {
        registry.remove(key);
    }

    removed
}

pub(crate) fn prune_targeted_in(
    tmux: &Tmux,
    target: &Path,
    base_dir: &Path,
) -> Result<Vec<(String, tmux_router::RegistryEntry)>> {
    let registry_path = sessions::registry_path_in(base_dir);
    let _lock = tmux_router::RegistryLock::acquire(&registry_path)?;
    let mut registry = sessions::load_in(base_dir)?;
    let removed = prune_dead_entries_for_target_in_registry(&mut registry, target, |pane| {
        tmux.pane_alive(pane)
    });
    if !removed.is_empty() {
        sessions::save_in(base_dir, &registry)?;
    }
    Ok(removed)
}

pub fn apply_targeted_fix_for_route(
    tmux: &Tmux,
    target_file: &Path,
) -> Result<TargetDocumentFixOutcome> {
    let target = resolve_target_file(target_file)?;
    let base_dir = resolve_registry_root(&target);
    let removed = prune_targeted_in(tmux, &target, &base_dir)?;
    let mut outcome = TargetDocumentFixOutcome {
        pruned_dead_entries: removed.len(),
        ..TargetDocumentFixOutcome::default()
    };
    let recovered = recover_target_document_pane_in(tmux, &target, &base_dir)?;
    outcome.reregistered_owner = recovered.reregistered_owner;
    outcome.killed_redundant_stash_panes = recovered.killed_redundant_stash_panes;
    let scoped_registry = filter_registry_for_target(&sessions::load_in(&base_dir)?, &target);
    let issues = detect_issues_in_registry(tmux, &scoped_registry);
    if !issues.is_empty() {
        outcome.fixed_issues =
            apply_fixes_with_base(tmux, &issues, None, Some(&base_dir), Some(&target))?;
    }
    Ok(outcome)
}

/// Quietly prune dead panes and deduplicate entries for the provided tmux server.
/// Returns the number of registry entries removed.
pub fn prune_with_tmux(tmux: &Tmux) -> Result<usize> {
    prune_with_tmux_timed(tmux).map(|(removed, _)| removed)
}

pub fn prune_with_tmux_timed(tmux: &Tmux) -> Result<(usize, Vec<PrunePhaseTiming>)> {
    prune_with_tmux_timed_in_mode(tmux, PruneCleanupMode::Full)
}

pub fn prune_with_tmux_timed_in_mode(
    tmux: &Tmux,
    cleanup_mode: PruneCleanupMode,
) -> Result<(usize, Vec<PrunePhaseTiming>)> {
    tracing::debug!("resync::prune start");
    let mut timings = Vec::new();
    let registry_path = sessions::registry_path();
    let removed = record_prune_phase(&mut timings, "prune_registry", || {
        tmux_router::prune(&registry_path, tmux)
    })?;
    if removed > 0 {
        tracing::debug!(removed, "resync: pruned stale sessions");
        eprintln!("resync: pruned {} stale session(s)", removed);
    }
    let skip_expensive_stash_cleanup = cleanup_mode == PruneCleanupMode::SkipExpensiveStashCleanup;
    let preserve_live_agent_stash_panes =
        cleanup_mode == PruneCleanupMode::PreserveLiveAgentStashPanes && removed == 0;

    // Fetch metadata once. Repeated safe-passive no-op syncs can skip window
    // metadata and stash cleanup while still pruning the registry and retained
    // dead non-stash panes.
    let windows = if skip_expensive_stash_cleanup {
        record_prune_phase(&mut timings, "prune_fetch_windows", WindowMeta::new)
    } else {
        record_prune_phase(&mut timings, "prune_fetch_windows", || {
            fetch_all_window_metadata(tmux)
        })
    };
    let panes = record_prune_phase(&mut timings, "prune_fetch_panes", || {
        fetch_all_pane_metadata(tmux)
    });

    // Purge idle stash panes (but do NOT return active panes from stash).
    // return_stashed_panes_bulk was removed from the automatic prune path because
    // it caused a stash-bounce loop: sync stashes unwanted panes → prune returns them
    // → next sync stashes them again. Active panes should stay in stash until the
    // reconciler explicitly needs them. Use `agent-doc resync --fix` for manual recovery.
    if skip_expensive_stash_cleanup {
        record_prune_phase(&mut timings, "prune_stash_windows", || {});
        record_prune_phase(&mut timings, "prune_stash_panes", || {});
    } else {
        record_prune_phase(&mut timings, "prune_stash_windows", || {
            purge_stash_windows_bulk(tmux, &windows, &panes)
        });
        record_prune_phase(&mut timings, "prune_stash_panes", || {
            if preserve_live_agent_stash_panes {
                purge_unregistered_stash_panes_bulk_in_mode(tmux, &windows, &panes, true)
            } else {
                purge_unregistered_stash_panes_bulk(tmux, &windows, &panes)
            }
        });
    }
    record_prune_phase(&mut timings, "prune_dead_non_stash", || {
        purge_unregistered_dead_non_stash_panes_bulk(tmux, &panes)
    });
    Ok((removed, timings))
}

/// Quietly prune dead panes and deduplicate entries.
/// Called automatically before route, sync, and claim operations.
/// Returns the number of registry entries removed.
pub fn prune() -> Result<usize> {
    let tmux = Tmux::default_server();
    prune_with_tmux(&tmux)
}

/// Purge stash windows where all panes are idle shells.
///
/// Safe criteria:
/// 1. Window name is "stash" (never touch "claude" or user-named windows)
/// 2. ALL panes are running idle shells (not claude/agent-doc/etc.)
/// 3. Window was created more than 30 seconds ago (grace period for auto-start)
pub(crate) fn purge_stash_windows(tmux: &Tmux) {
    let output = tmux
        .cmd()
        .args([
            "list-windows",
            "-a",
            "-F",
            "#{window_id}\t#{window_name}\t#{window_activity}",
        ])
        .output();
    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return,
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let (window_id, window_name, activity_str) = (parts[0], parts[1], parts[2]);

        // Only target "stash" windows
        if window_name != "stash" {
            continue;
        }

        // Grace period: skip if last activity was within 30 seconds
        if let Ok(activity) = activity_str.parse::<u64>()
            && now.saturating_sub(activity) < 30
        {
            continue;
        }

        // Check that ALL panes are idle shells
        let pane_output = tmux
            .cmd()
            .args([
                "list-panes",
                "-t",
                window_id,
                "-F",
                "#{pane_current_command}",
            ])
            .output();
        let pane_output = match pane_output {
            Ok(o) if o.status.success() => o,
            _ => continue,
        };

        let all_idle = String::from_utf8_lossy(&pane_output.stdout)
            .lines()
            .all(|cmd| IDLE_SHELLS.contains(&cmd));

        if all_idle {
            if let Err(e) = tmux.cmd().args(["kill-window", "-t", window_id]).output() {
                eprintln!("resync: failed to purge stash window {}: {}", window_id, e);
            } else {
                eprintln!("resync: purged stash window {} (all panes idle)", window_id);
            }
        }
    }
}

/// Purge unregistered panes in stash windows.
///
/// Kills individual panes in stash windows that are:
/// 1. Not registered in sessions.json (orphaned)
/// 2. Running idle shells OR agent-doc/claude/node processes
/// 3. Leaves other user processes (corky, vim, etc.) alive
///
/// After purging panes, kills any stash window that becomes empty.
pub(crate) fn purge_unregistered_stash_panes(tmux: &Tmux) {
    let registry = sessions::load().unwrap_or_default();
    purge_unregistered_stash_panes_with_registry(tmux, &registry);
}

/// Testable inner function that accepts a registry parameter.
pub(crate) fn purge_unregistered_stash_panes_with_registry(
    tmux: &Tmux,
    registry: &tmux_router::Registry,
) {
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let live_supervisors = crate::supervisor::ipc::active_supervisor_pids(&project_root);
    purge_unregistered_stash_panes_with_registry_and_supervisors(tmux, registry, &live_supervisors);
}

pub(crate) fn purge_unregistered_stash_panes_with_registry_and_supervisors(
    tmux: &Tmux,
    registry: &tmux_router::Registry,
    live_supervisors: &[(String, u32)],
) {
    let current_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut pane_context_cache = PaneProjectContextCache::default();
    let registered_panes: std::collections::HashSet<&str> =
        registry.values().map(|e| e.pane.as_str()).collect();

    let output = tmux
        .cmd()
        .args([
            "list-windows",
            "-a",
            "-F",
            "#{window_id}\t#{window_name}\t#{session_name}",
        ])
        .output();
    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return,
    };

    let mut killed_count = 0;

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let (window_id, window_name, session_name) = (parts[0], parts[1], parts[2]);

        if !is_stash_window_name(window_name) {
            continue;
        }

        let panes = tmux.list_window_panes(window_id).unwrap_or_default();
        if panes.is_empty() {
            continue;
        }

        // Check each pane individually.
        // Only kill idle shells — never kill agent processes (agent-doc, claude, node)
        // even if unregistered, because the registry can go stale and an active Claude
        // session should never be killed automatically.
        let mut panes_to_kill = Vec::new();
        for pane_id in &panes {
            if registered_panes.contains(pane_id.as_str()) {
                continue; // Registered — leave it
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
                // #cross-project-stash-pane-condition: this is correct cross-project
                // preservation, not an error. Route it to ops_log telemetry instead of
                // stderr so it does not pollute the IDE/route error surface (the JB
                // plugin renders resync stderr as an error). Keep the skip behavior.
                crate::ops_log::log_op(
                    &current_root,
                    &format!(
                        "resync: stash pane {} ({}) is registered in its own project root — skipping kill",
                        pane_id, session_name
                    ),
                );
                continue;
            }
            if tmux.pane_dead(pane_id) {
                panes_to_kill.push(pane_id.clone());
                continue;
            }
            if let Some(owner_file) = live_owned_registered_file_for_pane(tmux, pane_id, registry)
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
                    "resync: stash pane {} ({}) is unregistered but still owns {} — skipping kill",
                    pane_id, session_name, owner_file
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
                    "resync: stash pane {} ({}) is unregistered but still hosts live supervisor {} — skipping kill",
                    pane_id, session_name, supervisor_session
                );
                continue;
            }
            match classify_pane_process(tmux, pane_id) {
                PaneProcessKind::IdleShell(_) => panes_to_kill.push(pane_id.clone()),
                PaneProcessKind::Agent(_) => panes_to_kill.push(pane_id.clone()),
                PaneProcessKind::Foreign(_) | PaneProcessKind::UnknownTransient => {}
            }
        }

        for pane_id in &panes_to_kill {
            if let Err(e) = tmux.kill_pane(pane_id) {
                eprintln!("resync: failed to kill stash pane {}: {}", pane_id, e);
            } else {
                killed_count += 1;
            }
        }

        // If we killed all panes in this stash window, tmux auto-removes it.
        // If some survived (user processes), log them.
        let remaining = panes.len() - panes_to_kill.len();
        if remaining > 0 && !panes_to_kill.is_empty() {
            eprintln!(
                "resync: purged {} of {} panes from stash {} in session '{}' ({} user-process panes remain)",
                panes_to_kill.len(),
                panes.len(),
                window_id,
                session_name,
                remaining
            );
        }
    }

    if killed_count > 0 {
        eprintln!("resync: purged {} orphaned stash pane(s)", killed_count);
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use tmux_router::{IsolatedTmux, Registry as SessionRegistry, RegistryEntry as SessionEntry};
    #[test]
    fn prune_dead_entries_for_target_only_removes_matching_file() {
        let dir = tempfile::tempdir().unwrap();
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("b.md");
        std::fs::write(&doc_a, "# A\n").unwrap();
        std::fs::write(&doc_b, "# B\n").unwrap();
        let target = doc_a.canonicalize().unwrap();

        let mut registry = SessionRegistry::new();
        registry.insert(
            "sess-a".to_string(),
            test_entry("%dead-a", &doc_a.to_string_lossy()),
        );
        registry.insert(
            "sess-b".to_string(),
            test_entry("%dead-b", &doc_b.to_string_lossy()),
        );

        let removed =
            prune_dead_entries_for_target_in_registry(&mut registry, &target, |_pane| false);
        assert_eq!(removed.len(), 1, "only the target doc should be pruned");
        assert_eq!(removed[0].0, "sess-a");
        assert!(!registry.contains_key("sess-a"));
        assert!(registry.contains_key("sess-b"));
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn detect_dead_pane_not_flagged_as_issue() {
        // Dead panes are handled by prune(), not detect_issues.
        // detect_issues should skip dead panes entirely.
        let iso = IsolatedTmux::new("resync-test-dead");

        let mut registry = SessionRegistry::new();
        registry.insert("dead-session".to_string(), test_entry("%99999", "test.md"));

        let issues = detect_issues_in_registry(&iso, &registry);
        assert!(
            issues.is_empty(),
            "dead panes should not generate issues (handled by prune), got: {:?}",
            issues.iter().map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_kills_unregistered_shell_in_stash() {
        // An unregistered idle shell in the stash should be killed.
        let iso = IsolatedTmux::new("resync-purge-shell");
        let cwd = std::env::current_dir().unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();

        // Move pane2 to stash (it will be running a shell)
        iso.stash_pane(&pane2, "test").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        assert!(iso.pane_alive(&pane2), "pane2 should be alive in stash");

        // Empty registry — pane2 is not registered
        let registry = SessionRegistry::new();
        purge_unregistered_stash_panes_with_registry(&iso, &registry);

        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            !iso.pane_alive(&pane2),
            "unregistered shell in stash should be killed"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_preserves_registered_pane_in_stash() {
        // A registered pane in stash should NOT be killed.
        let iso = IsolatedTmux::new("resync-purge-registered");
        let cwd = std::env::current_dir().unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        iso.stash_pane(&pane2, "test").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        // Registry with pane2 registered
        let mut registry = SessionRegistry::new();
        registry.insert("registered-sess".to_string(), test_entry(&pane2, "test.md"));

        purge_unregistered_stash_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            iso.pane_alive(&pane2),
            "registered pane in stash should survive purge"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_preserves_user_process_in_stash() {
        // A pane running a user process (not shell/agent) should NOT be killed.
        let iso = IsolatedTmux::new("resync-purge-userproc");
        let cwd = std::env::current_dir().unwrap();

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
        let pane2 = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Move to stash
        iso.stash_pane(&pane2, "test").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let registry = SessionRegistry::new();
        purge_unregistered_stash_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            iso.pane_alive(&pane2),
            "user process (sleep) in stash should survive purge"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_kills_unregistered_agent_in_stash_without_live_owner() {
        let iso = IsolatedTmux::new("resync-purge-agent-no-owner");
        let cwd = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let script = write_mock_agent_doc(dir.path());

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        iso.send_keys(&pane2, &format!("exec {}", script.display()))
            .unwrap();
        let _ = wait_for_pane_contains(&iso, &pane2, "\n>", std::time::Duration::from_secs(3));
        iso.stash_pane(&pane2, "test").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let registry = SessionRegistry::new();
        purge_unregistered_stash_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            !iso.pane_alive(&pane2),
            "unregistered agent pane with no live owner should be killed"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_unregistered_stash_panes_kills_retained_dead_stash_pane() {
        let iso = IsolatedTmux::new("resync-purge-dead-stash");
        let cwd = std::env::current_dir().unwrap();
        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        iso.enable_remain_on_exit(&pane2).unwrap();
        drive_pane_to_retained_dead(
            &iso,
            &pane2,
            "printf 'dead stash\\n'; exit 11",
            std::time::Duration::from_secs(6),
        );
        iso.stash_pane(&pane2, "test").unwrap();
        assert!(
            wait_for_pane_in_stash_window(&iso, "test", &pane2, std::time::Duration::from_secs(3)),
            "dead pane should move into stash before purge"
        );

        purge_unregistered_stash_panes_with_registry(&iso, &SessionRegistry::new());
        assert!(
            wait_for_pane_removed(&iso, &pane2, std::time::Duration::from_secs(3)),
            "retained dead stash pane should be removed during purge"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_preserves_unregistered_agent_in_stash_with_live_supervisor() {
        let iso = IsolatedTmux::new("resync-purge-agent-live-supervisor");
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
        let mut ipc =
            crate::supervisor::ipc::SupervisorIpc::start(dir.path(), "super-live", move |method| {
                match method {
                    crate::supervisor::ipc::IpcMethod::Pid => {
                        crate::supervisor::ipc::IpcResponse::ok(
                            serde_json::json!({ "pid": live_pid }),
                        )
                    }
                    crate::supervisor::ipc::IpcMethod::State => {
                        crate::supervisor::ipc::IpcResponse::ok(
                            serde_json::json!({ "running": true }),
                        )
                    }
                    crate::supervisor::ipc::IpcMethod::Inject { bytes }
                    | crate::supervisor::ipc::IpcMethod::Clear { bytes } => {
                        crate::supervisor::ipc::IpcResponse::ok(
                            serde_json::json!({ "n": bytes.len() }),
                        )
                    }
                    crate::supervisor::ipc::IpcMethod::Restart { .. }
                    | crate::supervisor::ipc::IpcMethod::Stop { .. }
                    | crate::supervisor::ipc::IpcMethod::StopAgent { .. }
                    | crate::supervisor::ipc::IpcMethod::ReplicaRegister { .. }
                    | crate::supervisor::ipc::IpcMethod::ReplicaDeregister { .. }
                    | crate::supervisor::ipc::IpcMethod::ReplicaUpdate { .. }
                    | crate::supervisor::ipc::IpcMethod::ReplicaPull { .. }
                    | crate::supervisor::ipc::IpcMethod::ReplicaAck { .. }
                    | crate::supervisor::ipc::IpcMethod::ReplicaAwareness { .. } => {
                        crate::supervisor::ipc::IpcResponse::ok_empty()
                    }
                }
            })
            .unwrap();
        iso.stash_pane(&pane2, "test").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let live_supervisors = crate::supervisor::ipc::active_supervisor_pids(dir.path());
        purge_unregistered_stash_panes_with_registry_and_supervisors(
            &iso,
            &SessionRegistry::new(),
            &live_supervisors,
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            iso.pane_alive(&pane2),
            "unregistered stash pane with a live supervisor should survive purge"
        );
        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_preserves_unregistered_agent_in_stash_with_live_owner() {
        let iso = IsolatedTmux::new("resync-purge-agent-live-owner");
        let cwd = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let script = write_mock_agent_doc(dir.path());
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "# Test\n").unwrap();
        let session_id = "sess-live-owner";

        let stale_pane = iso.auto_start("test", &cwd).unwrap();
        let live_pane = iso.split_window(&stale_pane, &cwd, "-dh").unwrap();
        launch_mock_agent_doc(&iso, &live_pane, &script, &doc);
        let live_pid = wait_for_process_pid(
            &script.display().to_string(),
            std::time::Duration::from_secs(3),
        );
        let mut ipc =
            crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
                match method {
                    crate::supervisor::ipc::IpcMethod::Pid => {
                        crate::supervisor::ipc::IpcResponse::ok(
                            serde_json::json!({ "pid": live_pid }),
                        )
                    }
                    crate::supervisor::ipc::IpcMethod::State => {
                        crate::supervisor::ipc::IpcResponse::ok(
                            serde_json::json!({ "running": true }),
                        )
                    }
                    crate::supervisor::ipc::IpcMethod::Inject { bytes }
                    | crate::supervisor::ipc::IpcMethod::Clear { bytes } => {
                        crate::supervisor::ipc::IpcResponse::ok(
                            serde_json::json!({ "n": bytes.len() }),
                        )
                    }
                    crate::supervisor::ipc::IpcMethod::Restart { .. }
                    | crate::supervisor::ipc::IpcMethod::Stop { .. }
                    | crate::supervisor::ipc::IpcMethod::StopAgent { .. }
                    | crate::supervisor::ipc::IpcMethod::ReplicaRegister { .. }
                    | crate::supervisor::ipc::IpcMethod::ReplicaDeregister { .. }
                    | crate::supervisor::ipc::IpcMethod::ReplicaUpdate { .. }
                    | crate::supervisor::ipc::IpcMethod::ReplicaPull { .. }
                    | crate::supervisor::ipc::IpcMethod::ReplicaAck { .. }
                    | crate::supervisor::ipc::IpcMethod::ReplicaAwareness { .. } => {
                        crate::supervisor::ipc::IpcResponse::ok_empty()
                    }
                }
            })
            .unwrap();
        iso.stash_pane(&live_pane, "test").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let mut registry = SessionRegistry::new();
        registry.insert(
            session_id.to_string(),
            test_entry(&stale_pane, &doc.to_string_lossy()),
        );

        purge_unregistered_stash_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            iso.pane_alive(&live_pane),
            "unregistered agent pane that still owns a registered file should survive purge"
        );
        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn prune_targeted_in_uses_submodule_registry() {
        let iso = IsolatedTmux::new("resync-test-submod-prune");

        let dir = tempfile::tempdir().unwrap();
        // Superproject with .agent-doc/
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        // Submodule with its own .agent-doc/ and registry
        let sub = dir.path().join("src/sub");
        std::fs::create_dir_all(sub.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(sub.join("tasks")).unwrap();
        let doc = sub.join("tasks/test.md");
        std::fs::write(&doc, "# test\n").unwrap();

        // Register in the submodule's sessions.json with a dead pane
        let mut registry = SessionRegistry::new();
        let canonical = doc.canonicalize().unwrap_or(doc.clone());
        registry.insert(
            canonical.to_string_lossy().to_string(),
            test_entry("%99998", &canonical.to_string_lossy()),
        );
        sessions::save_in(&sub, &registry).unwrap();

        // Superproject registry should be empty
        sessions::save_in(dir.path(), &SessionRegistry::new()).unwrap();

        let target = canonical;
        let removed = prune_targeted_in(&iso, &target, &sub).unwrap();
        assert_eq!(
            removed.len(),
            1,
            "should find and prune the dead entry from the submodule registry"
        );

        // Verify the submodule registry is now empty
        let after = sessions::load_in(&sub).unwrap();
        assert!(
            after.is_empty(),
            "submodule registry should be empty after prune"
        );
    }
}
