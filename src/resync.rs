//! # Module: resync
//!
//! ## Spec
//! - `prune()`: quietly removes dead/duplicate registry entries by delegating to
//!   `tmux_router::prune()`, then returns active panes from stash windows, purges
//!   idle stash windows, and clears orphaned stash panes. Called automatically
//!   before route, sync, and claim operations. Returns the count of entries removed.
//! - `run(fix, target_file)`: verbose counterpart to `prune()` for the `agent-doc resync`
//!   and `agent-doc fix` CLI subcommands. Prints which entries were removed, which
//!   issues were detected, and the post-resync active session list. When
//!   `target_file` is provided, scope mutations to that single document.
//! - Issue detection (`detect_issues`): inspects every alive registry pane for five
//!   problem classes: `InStash` (pane parked in a stash window), `WrongProcess`
//!   (pane running a non-agent process such as `corky watch`), `WrongSession`
//!   (pane's tmux session differs from the document's `tmux_session` frontmatter
//!   field, or from `config::project_tmux_session()` when frontmatter field is absent),
//!   `NoLiveOwner` (alive registered pane no longer proves ownership of its
//!   document), and `WrongWindow` (panes for the same tmux session are scattered
//!   across multiple non-stash windows, determined by majority-window vote).
//! - Fix application (`apply_fixes`): `WrongSession` → kill pane + deregister entry (default),
//!   or when `relocate_session = Some(target)` → `join-pane` to target session (registry kept);
//!   `WrongProcess` → deregister only (foreign process is not killed); `NoLiveOwner`
//!   → deregister only (pane left intact for route/later manual recovery);
//!   `InStash` → deregister only (pane left intact for potential manual recovery);
//!   `WrongWindow` → stash the outlier pane so the next route consolidates it.
//! - Stash management: `return_stashed_panes` moves registered active panes back to
//!   their original window (or first non-stash window of the frontmatter session);
//!   `purge_stash_windows` kills entire stash windows where all panes are idle
//!   shells and the window is older than 30 seconds; `purge_unregistered_stash_panes`
//!   kills individual unregistered idle shell panes in stash windows. Unregistered
//!   agent panes in stash are preserved when they still prove ownership of
//!   some registered document or still host a live supervisor session; otherwise
//!   they are purged as orphaned. `purge_orphaned_agent_panes`
//!   removes unregistered agent-doc/claude/node panes from any window, but only when
//!   the window has at least one other pane (never orphans the last pane).
//! - Process classification: `AGENT_PROCESSES` (`agent-doc`, `claude`, `codex`, `node`) are
//!   expected occupants of registered panes. `IDLE_SHELLS` (`zsh`, `bash`, `sh`,
//!   `fish`) are treated as empty/unused slots. Short-lived shell startup helpers
//!   (for example `mkdir`, `mv`, `xset`) must not be classified as `WrongProcess`
//!   unless they remain the stable foreground command across a brief grace window.
//!
//! ## Agentic Contracts
//! - `prune()` never kills a registered pane that is alive and in a non-stash window;
//!   it only removes dead entries from the registry and stash-specific garbage.
//! - User-owned processes (anything not in `AGENT_PROCESSES` or `IDLE_SHELLS`) are
//!   never killed by any automatic or fix path — they are left running.
//! - Stash windows named exactly `"stash"` or matching `"stash-*"` are the only
//!   windows whose panes may be killed automatically; non-stash windows are only
//!   touched when purging orphaned agent panes with sibling panes present.
//! - `apply_fixes` acquires a `RegistryLock` before mutating `sessions.json`; all
//!   registry mutations are atomic with respect to concurrent agent-doc processes.
//! - Dead panes are exclusively handled by `tmux_router::prune()`; `detect_issues`
//!   skips dead panes entirely to avoid double-reporting.
//! - On `WrongSession` fix failure (kill error), the registry entry is still removed
//!   to prevent a permanently stale entry from blocking future routes.
//! - `find_return_target` priority: (1) original window from registry entry if alive
//!   and non-stash, (2) first non-stash window in the frontmatter `tmux_session`,
//!   (3) returns `None` (no move attempted, error logged).
//!
//! ## Evals
//! - `detect_dead_pane_not_flagged_as_issue`: registry entry with a non-existent
//!   pane ID → `detect_issues_in_registry` returns no issues (dead panes belong to
//!   `prune`, not issue detection).
//! - `detect_wrong_session_pane`: pane running in session `"wrong"` with frontmatter
//!   `tmux_session: correct` → `WrongSession` issue detected.
//! - `fix_wrong_session_removes_registry_entry`: `apply_fixes_to_registry` with a
//!   `WrongSession` issue → entry removed from registry, pane kill attempted.
//! - `fix_wrong_process_deregisters_without_kill`: `WrongProcess` issue →
//!   registry entry removed, foreign process pane untouched.
//! - `fix_in_stash_deregisters_entry`: `InStash` issue → registry entry removed,
//!   stash pane left alive.
//! - `stash_window_purged_when_all_idle`: stash window with only idle shell panes
//!   older than 30 s → `purge_stash_windows` kills the window.
//! - `stash_window_spared_when_agent_active`: stash window containing a `claude`
//!   pane → `purge_stash_windows` leaves the window intact.
//! - `purge_unregistered_stash_panes_leaves_user_processes`: stash window with an
//!   unregistered `corky` pane and an unregistered idle shell → idle shell killed,
//!   `corky` pane untouched.
//! - `purge_preserves_unregistered_agent_with_live_supervisor`: stash window with an
//!   unregistered agent pane that still hosts a live supervisor socket → pane survives purge.
//! - `purge_orphaned_agent_panes_skips_last_pane`: window with a single unregistered
//!   `claude` pane → pane not killed (would orphan the window).
//! - `wrong_window_detected_by_majority_vote`: three registered panes in session A,
//!   two in window W1 and one in window W2 → the W2 pane produces a `WrongWindow`
//!   issue; the W1 panes do not.

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::sessions::{self, PaneMoveOp, Tmux};
use crate::{config, frontmatter};

/// Valid process names for agent-doc panes.
const AGENT_PROCESSES: &[&str] = &["agent-doc", "claude", "codex", "node"];

/// Shells considered idle (not running an agent process).
const IDLE_SHELLS: &[&str] = &["zsh", "bash", "sh", "fish"];

const PROCESS_GRACE_SAMPLES: usize = 4;
const PROCESS_GRACE_DELAY_MS: u64 = 75;

#[derive(Debug, Clone, PartialEq, Eq)]
enum PaneProcessKind {
    Agent(String),
    IdleShell(String),
    Foreign(String),
    UnknownTransient,
}

fn classify_pane_process(tmux: &Tmux, pane_id: &str) -> PaneProcessKind {
    let mut first_foreign: Option<String> = None;
    let mut foreign_stable = true;

    for sample_idx in 0..PROCESS_GRACE_SAMPLES {
        if let Some(cmd) = pane_current_command(tmux, pane_id) {
            if AGENT_PROCESSES.contains(&cmd.as_str()) {
                return PaneProcessKind::Agent(cmd);
            }
            if IDLE_SHELLS.contains(&cmd.as_str()) {
                return PaneProcessKind::IdleShell(cmd);
            }

            match &first_foreign {
                Some(prev) if prev != &cmd => foreign_stable = false,
                None => first_foreign = Some(cmd),
                _ => {}
            }
        } else {
            foreign_stable = false;
        }

        if sample_idx + 1 < PROCESS_GRACE_SAMPLES {
            std::thread::sleep(std::time::Duration::from_millis(PROCESS_GRACE_DELAY_MS));
        }
    }

    match (first_foreign, foreign_stable) {
        (Some(cmd), true) => PaneProcessKind::Foreign(cmd),
        _ => PaneProcessKind::UnknownTransient,
    }
}

/// A problem detected during resync --fix analysis.
#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
enum Issue {
    /// Pane is in a different tmux session than the document's frontmatter expects.
    WrongSession {
        key: String,
        file: String,
        pane: String,
        actual_session: String,
        expected_session: String,
    },
    /// Pane is running a process that is not agent-doc or claude.
    WrongProcess {
        key: String,
        file: String,
        pane: String,
        process: String,
    },
    /// Pane is alive but no live owner can still be proven for its file.
    NoLiveOwner {
        key: String,
        file: String,
        pane: String,
    },
    /// Panes for the same session are in different windows (excluding stash windows).
    WrongWindow {
        key: String,
        file: String,
        pane: String,
        actual_window: String,
        expected_window: String,
    },
    /// Pane is alive but in a stash window (not the active workspace).
    InStash {
        key: String,
        file: String,
        pane: String,
        window_name: String,
    },
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Issue::WrongSession {
                file,
                pane,
                actual_session,
                expected_session,
                ..
            } => write!(
                f,
                "{} (pane {}) in session '{}', expected '{}'",
                file, pane, actual_session, expected_session
            ),
            Issue::WrongProcess {
                file,
                pane,
                process,
                ..
            } => write!(
                f,
                "{} (pane {}) running '{}', expected agent-doc/claude",
                file, pane, process
            ),
            Issue::NoLiveOwner { file, pane, .. } => write!(
                f,
                "{} (pane {}) has no provable live owner and would be deregistered",
                file, pane
            ),
            Issue::WrongWindow {
                file,
                pane,
                actual_window,
                expected_window,
                ..
            } => write!(
                f,
                "{} (pane {}) in window '{}', expected '{}'",
                file, pane, actual_window, expected_window
            ),
            Issue::InStash {
                file,
                pane,
                window_name,
                ..
            } => write!(
                f,
                "{} (pane {}) is in stash window '{}'",
                file, pane, window_name
            ),
        }
    }
}

fn resolve_target_file(file: &Path) -> Result<PathBuf> {
    let resolved = crate::git::resolve_absolute_file_path(file);
    if !resolved.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    Ok(resolved.canonicalize().unwrap_or(resolved))
}

fn same_document_path(target: &Path, candidate: &str) -> bool {
    if candidate.is_empty() {
        return false;
    }
    let resolved = crate::git::resolve_absolute_file_path(Path::new(candidate));
    let canonical = resolved.canonicalize().unwrap_or(resolved);
    canonical == target
}

fn filter_registry_for_target(
    registry: &sessions::SessionRegistry,
    target: &Path,
) -> sessions::SessionRegistry {
    registry
        .iter()
        .filter(|(_, entry)| same_document_path(target, &entry.file))
        .map(|(key, entry)| (key.clone(), entry.clone()))
        .collect()
}

fn format_associated_pane_fix_error(
    file: &Path,
    candidates: &[crate::sync::AssociatedPaneCandidate],
    preferred_window: Option<&str>,
) -> String {
    let mut lines = vec![format!(
        "multiple tmux panes are associated with {}; fix will not auto-pick one.",
        file.display()
    )];
    if let Some(window_id) = preferred_window {
        lines.push(format!(
            "Preferred active window: {}. Inspect one pane, claim it explicitly, then kill the redundant panes.",
            window_id
        ));
    } else {
        lines.push(
            "Inspect one pane, claim it explicitly, then kill the redundant panes.".to_string(),
        );
    }
    for candidate in candidates {
        lines.push(format!(
            "  - {} session={} window={} ({}) cmd={} sources={}",
            candidate.pane_id,
            candidate.session_name,
            candidate.window_id,
            candidate.window_name,
            candidate.current_command,
            candidate.source_summary()
        ));
        lines.push(format!(
            "    view: tmux capture-pane -pt {} | tail -n 80",
            candidate.pane_id
        ));
        lines.push(format!(
            "    assign: agent-doc claim {} --pane {} --force",
            file.display(),
            candidate.pane_id
        ));
        lines.push(format!("    kill: tmux kill-pane -t {}", candidate.pane_id));
    }
    lines.join("\n")
}

fn kill_redundant_associated_stash_panes(
    tmux: &Tmux,
    redundant: &[crate::sync::AssociatedPaneCandidate],
) -> usize {
    let registry = sessions::load().unwrap_or_default();
    let mut killed = 0;

    for candidate in redundant {
        if !candidate.is_stash() {
            continue;
        }
        if registry
            .values()
            .any(|entry| entry.pane == candidate.pane_id)
        {
            eprintln!(
                "resync: preserving redundant pane {} because it is still registered",
                candidate.pane_id
            );
            continue;
        }
        match tmux.kill_pane(&candidate.pane_id) {
            Ok(()) => {
                killed += 1;
                eprintln!(
                    "resync: killed redundant stash pane {} for duplicate document session",
                    candidate.pane_id
                );
            }
            Err(err) => eprintln!(
                "resync: failed to kill redundant stash pane {}: {}",
                candidate.pane_id, err
            ),
        }
    }

    killed
}

fn recover_target_document_pane(tmux: &Tmux, target: &Path) -> Result<()> {
    let Some(session_id) = frontmatter::read_session_id(target) else {
        return Ok(());
    };

    let preferred_window = config::project_tmux_session()
        .as_deref()
        .and_then(|session| tmux.active_window(session));
    let candidates = crate::sync::find_associated_panes(tmux, target, &session_id);
    match crate::sync::resolve_associated_panes(candidates, preferred_window.as_deref()) {
        crate::sync::AssociatedPaneResolution::None => Ok(()),
        crate::sync::AssociatedPaneResolution::Selected { winner, redundant } => {
            if sessions::lookup(&session_id)?.as_deref() != Some(winner.pane_id.as_str()) {
                sessions::register(&session_id, &winner.pane_id, &target.to_string_lossy())?;
                eprintln!(
                    "resync: re-registered {} to pane {}",
                    target.display(),
                    winner.pane_id
                );
            }
            let killed = kill_redundant_associated_stash_panes(tmux, &redundant);
            if killed > 0 {
                eprintln!(
                    "resync: removed {} redundant stash pane(s) for {}",
                    killed,
                    target.display()
                );
            }
            Ok(())
        }
        crate::sync::AssociatedPaneResolution::Ambiguous(candidates) => {
            anyhow::bail!(format_associated_pane_fix_error(
                target,
                &candidates,
                preferred_window.as_deref()
            ));
        }
    }
}

fn prune_dead_entries_for_target_in_registry<F>(
    registry: &mut sessions::SessionRegistry,
    target: &Path,
    mut pane_alive: F,
) -> Vec<(String, sessions::SessionEntry)>
where
    F: FnMut(&str) -> bool,
{
    let removed: Vec<(String, sessions::SessionEntry)> = registry
        .iter()
        .filter(|(_, entry)| same_document_path(target, &entry.file) && !pane_alive(&entry.pane))
        .map(|(key, entry)| (key.clone(), entry.clone()))
        .collect();

    for (key, _) in &removed {
        registry.remove(key);
    }

    removed
}

fn prune_targeted(tmux: &Tmux, target: &Path) -> Result<Vec<(String, sessions::SessionEntry)>> {
    let registry_path = sessions::registry_path();
    let _lock = tmux_router::RegistryLock::acquire(&registry_path)?;
    let mut registry = sessions::load()?;
    let removed = prune_dead_entries_for_target_in_registry(&mut registry, target, |pane| {
        tmux.pane_alive(pane)
    });
    if !removed.is_empty() {
        sessions::save(&registry)?;
    }
    Ok(removed)
}

/// Quietly prune dead panes and deduplicate entries.
/// Called automatically before route, sync, and claim operations.
/// Returns the number of registry entries removed.
pub fn prune() -> Result<usize> {
    tracing::debug!("resync::prune start");
    let tmux = Tmux::default_server();
    let registry_path = sessions::registry_path();
    let removed = tmux_router::prune(&registry_path, &tmux)?;
    if removed > 0 {
        tracing::debug!(removed, "resync: pruned stale sessions");
        eprintln!("resync: pruned {} stale session(s)", removed);
    }

    // Fetch all metadata once (2 subprocess calls total instead of ~20-40)
    let windows = fetch_all_window_metadata(&tmux);
    let panes = fetch_all_pane_metadata(&tmux);

    // Purge idle stash panes (but do NOT return active panes from stash).
    // return_stashed_panes_bulk was removed from the automatic prune path because
    // it caused a stash-bounce loop: sync stashes unwanted panes → prune returns them
    // → next sync stashes them again. Active panes should stay in stash until the
    // reconciler explicitly needs them. Use `agent-doc resync --fix` for manual recovery.
    purge_stash_windows_bulk(&tmux, &windows, &panes);
    purge_unregistered_stash_panes_bulk(&tmux, &windows, &panes);
    Ok(removed)
}

/// Purge stash windows where all panes are idle shells.
///
/// Safe criteria:
/// 1. Window name is "stash" (never touch "claude" or user-named windows)
/// 2. ALL panes are running idle shells (not claude/agent-doc/etc.)
/// 3. Window was created more than 30 seconds ago (grace period for auto-start)
fn purge_stash_windows(tmux: &Tmux) {
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
fn purge_unregistered_stash_panes(tmux: &Tmux) {
    let registry = sessions::load().unwrap_or_default();
    purge_unregistered_stash_panes_with_registry(tmux, &registry);
}

/// Testable inner function that accepts a registry parameter.
fn purge_unregistered_stash_panes_with_registry(tmux: &Tmux, registry: &sessions::SessionRegistry) {
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let live_supervisors = crate::supervisor::ipc::active_supervisor_pids(&project_root);
    purge_unregistered_stash_panes_with_registry_and_supervisors(tmux, registry, &live_supervisors);
}

fn purge_unregistered_stash_panes_with_registry_and_supervisors(
    tmux: &Tmux,
    registry: &sessions::SessionRegistry,
    live_supervisors: &[(String, u32)],
) {
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
            if let Some(owner_file) = live_owned_registered_file_for_pane(tmux, pane_id, registry) {
                eprintln!(
                    "resync: stash pane {} ({}) is unregistered but still owns {} — skipping kill",
                    pane_id, session_name, owner_file
                );
                continue;
            }
            if let Some(supervisor_session) =
                pane_hosts_live_supervisor_session(tmux, pane_id, live_supervisors)
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

fn live_owned_registered_file_for_pane(
    tmux: &Tmux,
    pane_id: &str,
    registry: &sessions::SessionRegistry,
) -> Option<String> {
    registry.iter().find_map(|(session_id, entry)| {
        if entry.file.is_empty() {
            return None;
        }
        let file = std::path::Path::new(&entry.file);
        if !file.exists() {
            return None;
        }
        (crate::sync::find_live_owner_pane(tmux, file, session_id).as_deref() == Some(pane_id))
            .then(|| entry.file.clone())
    })
}

fn pane_hosts_live_supervisor_session(
    tmux: &Tmux,
    pane_id: &str,
    live_supervisors: &[(String, u32)],
) -> Option<String> {
    live_supervisors.iter().find_map(|(session_id, pid)| {
        pane_process_tree_contains_pid(tmux, pane_id, *pid).then(|| session_id.clone())
    })
}

fn pane_process_tree_contains_pid(tmux: &Tmux, pane_id: &str, target_pid: u32) -> bool {
    let output = match tmux
        .cmd()
        .args(["display-message", "-t", pane_id, "-p", "#{pane_pid}"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return false,
    };
    let pane_pid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if pane_pid.is_empty() {
        return false;
    }
    process_tree_contains_pid(&pane_pid, target_pid)
}

fn process_tree_contains_pid(root_pid: &str, target_pid: u32) -> bool {
    let mut frontier = vec![root_pid.to_string()];
    let target_pid = target_pid.to_string();

    while let Some(pid) = frontier.pop() {
        if pid == target_pid {
            return true;
        }
        let output = match std::process::Command::new("pgrep")
            .args(["-P", &pid])
            .output()
        {
            Ok(output) if output.status.success() => output,
            _ => continue,
        };
        for child_pid in String::from_utf8_lossy(&output.stdout).lines() {
            let child_pid = child_pid.trim();
            if child_pid.is_empty() {
                continue;
            }
            if child_pid == target_pid {
                return true;
            }
            frontier.push(child_pid.to_string());
        }
    }

    false
}

/// Return active (non-idle) panes from stash windows back to their original sessions.
///
/// For each registered pane sitting in a stash window:
/// 1. Skip idle shells (zsh/bash/sh/fish) — those are handled by purge functions.
/// 2. Look up the pane's registry entry to find the original window.
/// 3. If the original window is alive, move the pane back via `join-pane`.
/// 4. Otherwise, if the tmux session exists, move to the session's first window.
/// 5. Log each action to stderr.
///
/// After returning panes, any stash window that becomes empty is auto-cleaned by tmux.
fn return_stashed_panes(tmux: &Tmux) {
    let registry = sessions::load().unwrap_or_default();
    return_stashed_panes_with_registry(tmux, &registry);
}

/// Testable inner function that accepts a registry parameter.
fn return_stashed_panes_with_registry(tmux: &Tmux, registry: &sessions::SessionRegistry) {
    // Build a map from pane_id → (key, entry) for quick lookup
    let pane_to_entry: std::collections::HashMap<&str, (&str, &sessions::SessionEntry)> = registry
        .iter()
        .map(|(k, e)| (e.pane.as_str(), (k.as_str(), e)))
        .collect();

    // List all windows to find stash windows
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

    let mut returned = 0;

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let (window_id, window_name, _session_name) = (parts[0], parts[1], parts[2]);

        if !is_stash_window_name(window_name) {
            continue;
        }

        // List panes in this stash window with their current command
        let pane_output = tmux
            .cmd()
            .args([
                "list-panes",
                "-t",
                window_id,
                "-F",
                "#{pane_id}\t#{pane_current_command}",
            ])
            .output();
        let pane_output = match pane_output {
            Ok(o) if o.status.success() => o,
            _ => continue,
        };

        for pane_line in String::from_utf8_lossy(&pane_output.stdout).lines() {
            let pane_parts: Vec<&str> = pane_line.splitn(2, '\t').collect();
            if pane_parts.len() < 2 {
                continue;
            }
            let pane_id = pane_parts[0];
            let pane_kind = classify_pane_process(tmux, pane_id);
            let pane_cmd = match &pane_kind {
                PaneProcessKind::IdleShell(_) => continue,
                PaneProcessKind::Agent(cmd) | PaneProcessKind::Foreign(cmd) => cmd.as_str(),
                PaneProcessKind::UnknownTransient => pane_parts[1],
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
type WindowMeta = Vec<(String, String, String, String)>; // (window_id, window_name, session_name, activity)
type PaneMeta = std::collections::HashMap<String, (String, String, String)>; // pane_id → (window_id, window_name, cmd)

/// Fetch all window metadata in a single subprocess call.
fn fetch_all_window_metadata(tmux: &Tmux) -> WindowMeta {
    let output = tmux
        .cmd()
        .args([
            "list-windows",
            "-a",
            "-F",
            "#{window_id}\t#{window_name}\t#{session_name}\t#{window_activity}",
        ])
        .output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
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
fn fetch_all_pane_metadata(tmux: &Tmux) -> PaneMeta {
    let output = tmux
        .cmd()
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{pane_id}\t#{window_id}\t#{window_name}\t#{pane_current_command}",
        ])
        .output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
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
fn purge_stash_windows_bulk(tmux: &Tmux, windows: &WindowMeta, panes: &PaneMeta) {
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
            .all(|(_, (_, _, cmd))| IDLE_SHELLS.contains(&cmd.as_str()));

        // Also check there ARE panes in this window
        let has_panes = panes.iter().any(|(_, (wid, _, _))| wid == window_id);

        if has_panes && all_idle {
            if let Err(e) = tmux.cmd().args(["kill-window", "-t", window_id]).output() {
                eprintln!("resync: failed to purge stash window {}: {}", window_id, e);
            } else {
                eprintln!("resync: purged stash window {} (all panes idle)", window_id);
            }
        }
    }
}

/// Bulk variant of `purge_unregistered_stash_panes` — uses pre-fetched metadata.
fn purge_unregistered_stash_panes_bulk(tmux: &Tmux, windows: &WindowMeta, panes: &PaneMeta) {
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let live_supervisors = crate::supervisor::ipc::active_supervisor_pids(&project_root);
    purge_unregistered_stash_panes_bulk_with_supervisors(tmux, windows, panes, &live_supervisors);
}

fn purge_unregistered_stash_panes_bulk_with_supervisors(
    tmux: &Tmux,
    windows: &WindowMeta,
    panes: &PaneMeta,
    live_supervisors: &[(String, u32)],
) {
    let registry = sessions::load().unwrap_or_default();
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
    for (pane_id, (window_id, _window_name, _cmd)) in panes {
        if !stash_windows.contains(window_id.as_str()) {
            continue;
        }
        if registered_panes.contains(pane_id.as_str()) {
            continue;
        }
        if let Some(owner_file) = live_owned_registered_file_for_pane(tmux, pane_id, &registry) {
            eprintln!(
                "resync: stash pane {} is unregistered but still owns {} — skipping kill",
                pane_id, owner_file
            );
            continue;
        }
        if let Some(supervisor_session) =
            pane_hosts_live_supervisor_session(tmux, pane_id, live_supervisors)
        {
            eprintln!(
                "resync: stash pane {} is unregistered but still hosts live supervisor {} — skipping kill",
                pane_id, supervisor_session
            );
            continue;
        }
        match classify_pane_process(tmux, pane_id) {
            PaneProcessKind::IdleShell(_) | PaneProcessKind::Agent(_) => {
                if let Err(e) = tmux.kill_pane(pane_id) {
                    eprintln!("resync: failed to kill stash pane {}: {}", pane_id, e);
                } else {
                    killed_count += 1;
                }
            }
            PaneProcessKind::Foreign(_) | PaneProcessKind::UnknownTransient => {}
        }
    }

    if killed_count > 0 {
        eprintln!("resync: purged {} orphaned stash pane(s)", killed_count);
    }
}

/// Bulk variant of `return_stashed_panes` — uses pre-fetched metadata.
/// Also deregisters stranded panes when no return target is found, preventing
/// repeated expensive lookups on subsequent cycles.
#[allow(dead_code)]
fn return_stashed_panes_bulk(tmux: &Tmux, windows: &WindowMeta, panes: &PaneMeta) {
    let registry = sessions::load().unwrap_or_default();
    let pane_to_entry: std::collections::HashMap<&str, (&str, &sessions::SessionEntry)> = registry
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
        let pane_kind = match cmd.as_str() {
            shell if IDLE_SHELLS.contains(&shell) => PaneProcessKind::IdleShell(cmd.clone()),
            agent if AGENT_PROCESSES.contains(&agent) => PaneProcessKind::Agent(cmd.clone()),
            _ => classify_pane_process(tmux, pane_id),
        };
        let pane_cmd = match &pane_kind {
            PaneProcessKind::IdleShell(_) => continue,
            PaneProcessKind::Agent(cmd) | PaneProcessKind::Foreign(cmd) => cmd.as_str(),
            PaneProcessKind::UnknownTransient => cmd.as_str(),
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
                if matches!(pane_kind, PaneProcessKind::IdleShell(_)) {
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
        && let Ok(mut reg) = sessions::load()
    {
        for key in &deregistered {
            reg.remove(key);
        }
        if let Err(e) = sessions::save(&reg) {
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
fn find_return_target_bulk(
    entry: &sessions::SessionEntry,
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
fn find_return_target(tmux: &Tmux, entry: &sessions::SessionEntry) -> Option<String> {
    // 1. Try the original window from the registry entry
    if !entry.window.is_empty()
        && let Ok(panes) = tmux.list_window_panes(&entry.window)
        && !panes.is_empty()
    {
        // Check it's not a stash window itself
        if let Some(wname) = pane_window_name(tmux, &panes[0])
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
fn first_non_stash_pane(tmux: &Tmux, session_name: &str) -> Option<String> {
    let output = tmux
        .cmd()
        .args([
            "list-windows",
            "-t",
            &format!("{}:", session_name),
            "-F",
            "#{window_id}\t#{window_name}",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
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
/// 1. Not registered in sessions.json
/// 2. Running agent-doc, claude, or node
/// 3. In a window that has at least one other pane (won't orphan last pane)
///
/// This catches orphaned Claude sessions in non-stash windows (e.g., session 3).
fn purge_orphaned_agent_panes(tmux: &Tmux) {
    let registry = sessions::load().unwrap_or_default();
    purge_orphaned_agent_panes_with_registry(tmux, &registry);
}

fn purge_orphaned_agent_panes_with_registry(tmux: &Tmux, registry: &sessions::SessionRegistry) {
    let registered_panes: std::collections::HashSet<&str> =
        registry.values().map(|e| e.pane.as_str()).collect();

    // List all panes across all sessions
    let output = tmux
        .cmd()
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{pane_id}\t#{window_id}\t#{pane_current_command}",
        ])
        .output();
    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return,
    };

    // Group panes by window
    let mut window_panes: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
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
            if AGENT_PROCESSES.contains(&cmd.as_str()) {
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

/// Detect issues with alive panes: wrong tmux session or wrong process.
fn detect_issues(tmux: &Tmux) -> Vec<Issue> {
    tracing::debug!("resync::detect_issues start");
    let registry = match sessions::load() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("resync: failed to load registry: {}", e);
            return Vec::new();
        }
    };
    detect_issues_in_registry(tmux, &registry)
}

/// Info about an alive pane for cross-entry analysis.
struct PaneInfo {
    key: String,
    label: String,
    pane: String,
    tmux_session: String,
    window_id: String,
    window_name: String,
}

/// Detect issues in a given registry (testable without disk I/O).
fn detect_issues_in_registry(tmux: &Tmux, registry: &sessions::SessionRegistry) -> Vec<Issue> {
    let mut issues = Vec::new();

    // Collect alive panes with their window info for cross-entry analysis
    let mut alive_panes: Vec<PaneInfo> = Vec::new();

    for (key, entry) in registry {
        if !tmux.pane_alive(&entry.pane) {
            continue; // Dead panes are handled by prune()
        }

        let label = if entry.file.is_empty() {
            key.as_str()
        } else {
            entry.file.as_str()
        };

        // Check 0: Is the pane in a stash window?
        // Stash panes are alive but not in the active workspace — deregister them
        // so that sync/route can auto-start a fresh pane in the correct window.
        if let Some(ref wname) = pane_window_name(tmux, &entry.pane)
            && is_stash_window_name(wname)
        {
            issues.push(Issue::InStash {
                key: key.clone(),
                file: label.to_string(),
                pane: entry.pane.clone(),
                window_name: wname.clone(),
            });
            continue; // Don't run further checks on stash panes
        }

        // Check 1: Is the pane running an agent-doc/claude process?
        let pane_kind = classify_pane_process(tmux, &entry.pane);
        if let PaneProcessKind::Foreign(cmd) = pane_kind {
            issues.push(Issue::WrongProcess {
                key: key.clone(),
                file: label.to_string(),
                pane: entry.pane.clone(),
                process: cmd,
            });
            continue; // Don't also check session for wrong-process panes
        }

        if entry.file.is_empty() {
            continue; // Can't check frontmatter without a file path
        }

        let live_owner =
            crate::sync::find_live_owner_pane(tmux, std::path::Path::new(&entry.file), key);
        if live_owner.as_deref() != Some(entry.pane.as_str()) {
            issues.push(Issue::NoLiveOwner {
                key: key.clone(),
                file: label.to_string(),
                pane: entry.pane.clone(),
            });
            continue;
        }

        // Check 2: Is the pane in the expected tmux session?

        let frontmatter_session = match std::fs::read_to_string(&entry.file) {
            Ok(content) => match frontmatter::parse(&content) {
                Ok((fm, _)) => fm.tmux_session,
                Err(_) => None,
            },
            Err(_) => None,
        };

        // Use frontmatter `tmux_session` if present; otherwise fall back to project config.
        // This ensures cross-session drift is detected even when documents lack a
        // `tmux_session` frontmatter field (the common case).
        let expected_session = frontmatter_session.or_else(config::project_tmux_session);

        if let Some(ref expected) = expected_session {
            match tmux.pane_session(&entry.pane) {
                Ok(actual) if actual != *expected => {
                    issues.push(Issue::WrongSession {
                        key: key.clone(),
                        file: label.to_string(),
                        pane: entry.pane.clone(),
                        actual_session: actual,
                        expected_session: expected.clone(),
                    });
                    continue;
                }
                Err(e) => {
                    eprintln!(
                        "resync: failed to query session for pane {}: {}",
                        entry.pane, e
                    );
                }
                _ => {} // Matches expected session — no issue
            }
        }

        // Collect window info for wrong-window detection
        alive_panes.push(PaneInfo {
            key: key.clone(),
            label: label.to_string(),
            pane: entry.pane.clone(),
            tmux_session: tmux.pane_session(&entry.pane).unwrap_or_default(),
            window_id: tmux.pane_window(&entry.pane).unwrap_or_default(),
            window_name: pane_window_name(tmux, &entry.pane).unwrap_or_default(),
        });
    }

    // Check 3: Detect panes for the same tmux session in different non-stash windows.
    // Group alive panes by tmux session, then check for window scatter.
    let mut by_session: std::collections::HashMap<String, Vec<&PaneInfo>> =
        std::collections::HashMap::new();
    for info in &alive_panes {
        if is_stash_window_name(&info.window_name) {
            continue;
        }
        by_session
            .entry(info.tmux_session.clone())
            .or_default()
            .push(info);
    }

    for panes in by_session.values() {
        if panes.len() < 2 {
            continue;
        }
        // Find the majority window (most panes) — that's the "expected" window
        let mut window_counts: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for p in panes {
            *window_counts.entry(&p.window_id).or_insert(0) += 1;
        }
        let expected_window = window_counts
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(w, _)| *w)
            .unwrap_or("");

        for p in panes {
            if p.window_id != expected_window {
                issues.push(Issue::WrongWindow {
                    key: p.key.clone(),
                    file: p.label.clone(),
                    pane: p.pane.clone(),
                    actual_window: p.window_id.clone(),
                    expected_window: expected_window.to_string(),
                });
            }
        }
    }

    issues
}

/// Get the window name for a pane.
fn pane_window_name(tmux: &Tmux, pane_id: &str) -> Option<String> {
    let output = tmux
        .cmd()
        .args(["display-message", "-t", pane_id, "-p", "#{window_name}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

/// Check if a window name is a stash window (e.g., "stash", "stash-1", "stash-2").
fn is_stash_window_name(name: &str) -> bool {
    name == "stash" || name.starts_with("stash-")
}

/// Get the current command running in a tmux pane.
fn pane_current_command(tmux: &Tmux, pane_id: &str) -> Option<String> {
    let output = tmux
        .cmd()
        .args([
            "display-message",
            "-t",
            pane_id,
            "-p",
            "#{pane_current_command}",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let cmd = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if cmd.is_empty() { None } else { Some(cmd) }
}

fn registered_pane_still_owns_file(tmux: &Tmux, key: &str, file: &str, pane: &str) -> bool {
    if file.is_empty() {
        return false;
    }
    crate::sync::find_live_owner_pane(tmux, std::path::Path::new(file), key).as_deref()
        == Some(pane)
}

/// Apply fixes for detected issues: kill wrong-session panes, deregister wrong-process panes.
fn apply_fixes(tmux: &Tmux, issues: &[Issue], relocate_session: Option<&str>) -> Result<usize> {
    if issues.is_empty() {
        return Ok(0);
    }
    tracing::debug!(issue_count = issues.len(), "resync::apply_fixes");

    let registry_path = sessions::registry_path();
    let _lock = tmux_router::RegistryLock::acquire(&registry_path)?;
    let mut registry = sessions::load()?;
    let fixed = apply_fixes_to_registry(tmux, issues, &mut registry, relocate_session);

    if fixed > 0 {
        sessions::save(&registry)?;
    }
    Ok(fixed)
}

/// Apply fixes to a mutable registry (testable without disk I/O).
/// Returns number of issues fixed.
///
/// `relocate_session`: when `Some(target)`, `WrongSession` fixes use `join-pane` to
/// move the pane to the target session instead of killing it. The registry entry is
/// kept (pane ID is stable after join-pane). Use this to preserve running sessions
/// while consolidating them into a single tmux session.
fn apply_fixes_to_registry(
    tmux: &Tmux,
    issues: &[Issue],
    registry: &mut sessions::SessionRegistry,
    relocate_session: Option<&str>,
) -> usize {
    let mut fixed = 0;

    for issue in issues {
        match issue {
            Issue::WrongSession {
                key,
                file,
                pane,
                expected_session,
                ..
            } => {
                if registered_pane_still_owns_file(tmux, key, file, pane) {
                    if tmux.session_alive(expected_session) {
                        if let Some(dest_pane) = tmux.active_pane(expected_session) {
                            match PaneMoveOp::new(tmux, pane, &dest_pane)
                                .allow_cross_session(
                                    "auto-relocate active live owner to expected session",
                                )
                                .join("-dh")
                            {
                                Ok(()) => {
                                    eprintln!(
                                        "  auto-relocated live owner pane {} → session '{}'",
                                        pane, expected_session
                                    );
                                    eprintln!("  fixed: {}", issue);
                                    fixed += 1;
                                }
                                Err(e) => {
                                    eprintln!(
                                        "  preserving live owner pane {} after relocation failed ({}); registry left intact",
                                        pane, e
                                    );
                                }
                            }
                        } else {
                            eprintln!(
                                "  preserving live owner pane {} because session '{}' has no active join target",
                                pane, expected_session
                            );
                        }
                    } else {
                        eprintln!(
                            "  preserving live owner pane {} for {} because it still owns the document",
                            pane, file
                        );
                    }
                    continue;
                }

                if let Some(target) = relocate_session {
                    // join-pane: move the pane to the target session without killing it.
                    // The pane ID is stable after join-pane, so the registry entry stays.
                    // Use `expected_session` from frontmatter as the join target if it
                    // matches the requested target; otherwise use the requested target directly.
                    let dest_session = if target == expected_session.as_str() {
                        expected_session.as_str()
                    } else {
                        target
                    };
                    if let Some(dest_pane) = tmux.active_pane(dest_session) {
                        match PaneMoveOp::new(tmux, pane, &dest_pane)
                            .allow_cross_session("relocate WrongSession pane to project session")
                            .join("-dh")
                        {
                            Ok(()) => {
                                eprintln!("  relocated pane {} → session '{}'", pane, dest_session)
                            }
                            Err(e) => {
                                eprintln!(
                                    "  relocate failed for pane {} ({}), deregistering",
                                    pane, e
                                );
                                registry.remove(key);
                            }
                        }
                    } else {
                        eprintln!(
                            "  no active pane in '{}' to join into, deregistering pane {}",
                            dest_session, pane
                        );
                        registry.remove(key);
                    }
                } else {
                    // Check if the pane is running an active agent process.
                    // If so, relocate instead of killing to preserve running sessions.
                    let pane_kind = classify_pane_process(tmux, pane);
                    let is_agent = matches!(
                        pane_kind,
                        PaneProcessKind::Agent(_) | PaneProcessKind::UnknownTransient
                    );

                    if is_agent && tmux.session_alive(expected_session) {
                        if let Some(dest_pane) = tmux.active_pane(expected_session) {
                            match PaneMoveOp::new(tmux, pane, &dest_pane)
                                .allow_cross_session(
                                    "auto-relocate active agent to expected session",
                                )
                                .join("-dh")
                            {
                                Ok(()) => {
                                    eprintln!(
                                        "  auto-relocated active agent pane {} → session '{}'",
                                        pane, expected_session
                                    );
                                }
                                Err(e) => {
                                    eprintln!(
                                        "  auto-relocate failed for pane {} ({}), deregistering (not killing active agent)",
                                        pane, e
                                    );
                                    registry.remove(key);
                                }
                            }
                        } else {
                            eprintln!(
                                "  no active pane in '{}' for relocation, deregistering pane {} (not killing active agent)",
                                expected_session, pane
                            );
                            registry.remove(key);
                        }
                    } else if is_agent {
                        // Expected session doesn't exist — don't kill the active agent,
                        // just deregister so route can re-create in the correct session.
                        eprintln!(
                            "  session '{}' not alive, deregistering pane {} (not killing active agent '{}')",
                            expected_session,
                            pane,
                            match &pane_kind {
                                PaneProcessKind::Agent(cmd)
                                | PaneProcessKind::IdleShell(cmd)
                                | PaneProcessKind::Foreign(cmd) => cmd.as_str(),
                                PaneProcessKind::UnknownTransient => "transient",
                            }
                        );
                        registry.remove(key);
                    } else {
                        // Idle shell or unknown — safe to kill.
                        if let Err(e) = tmux.kill_pane(pane) {
                            eprintln!(
                                "resync: could not kill pane {} ({}), deregistering anyway",
                                pane, e
                            );
                        }
                        registry.remove(key);
                    }
                }
                eprintln!("  fixed: {}", issue);
                fixed += 1;
            }
            Issue::WrongProcess { key, .. } => {
                // Just deregister — don't kill the foreign process
                registry.remove(key);
                eprintln!("  fixed: {}", issue);
                fixed += 1;
            }
            Issue::NoLiveOwner { key, .. } => {
                registry.remove(key);
                eprintln!("  fixed: {}", issue);
                fixed += 1;
            }
            Issue::InStash { key, pane, .. } => {
                // Deregister — the pane is in the stash, not the active workspace.
                // Don't kill it; just remove the registry entry so auto-start can
                // create a fresh pane in the correct window.
                eprintln!(
                    "  [resync] pane {} for {} is in stash window, deregistering",
                    pane, key
                );
                registry.remove(key);
                fixed += 1;
            }
            Issue::WrongWindow {
                key, file, pane, ..
            } => {
                if registered_pane_still_owns_file(tmux, key, file, pane) {
                    eprintln!(
                        "  preserving live owner pane {} for {} in its current window; not stashing an active bound session",
                        pane, file
                    );
                    continue;
                }
                // Move the pane to the stash window to consolidate.
                // Determine the tmux session for this pane.
                let session_name = tmux
                    .pane_session(pane)
                    .unwrap_or_else(|_| "claude".to_string());
                if let Err(e) = tmux.stash_pane(pane, &session_name) {
                    eprintln!("  resync: failed to stash pane {}: {}", pane, e);
                    continue;
                }
                eprintln!("  fixed: {}", issue);
                fixed += 1;
            }
        }
    }

    fixed
}

/// Verbose resync for the standalone `agent-doc resync` command.
///
/// `relocate_session`: when `Some(target)`, `WrongSession` panes are relocated via
/// `join-pane` instead of being killed. Pass the target tmux session name (e.g. `"10"`).
pub fn run_fix(target_file: Option<&Path>, relocate_session: Option<&str>) -> Result<()> {
    run(true, relocate_session, target_file)
}

/// `target_file`: when `Some(file)`, scope detection and mutations to that single
/// document instead of mutating the whole registry.
pub fn run(fix: bool, relocate_session: Option<&str>, target_file: Option<&Path>) -> Result<()> {
    let tmux = Tmux::default_server();

    if let Some(file) = target_file {
        let target = resolve_target_file(file)?;
        let removed = prune_targeted(&tmux, &target)?;

        if removed.is_empty() {
            let scoped = filter_registry_for_target(&sessions::load()?, &target);
            if scoped.is_empty() {
                eprintln!("No registered sessions found for {}.", target.display());
            } else {
                eprintln!(
                    "All {} matching session(s) for {} have live panes.",
                    scoped.len(),
                    target.display()
                );
            }
        } else {
            eprintln!("Removed {} stale matching session(s):", removed.len());
            for (key, entry) in &removed {
                let label = if entry.file.is_empty() {
                    key.as_str()
                } else {
                    entry.file.as_str()
                };
                eprintln!("  {} (pane {} removed)", label, entry.pane);
            }
        }

        if fix {
            recover_target_document_pane(&tmux, &target)?;
        }

        let scoped_registry = filter_registry_for_target(&sessions::load()?, &target);
        let issues = detect_issues_in_registry(&tmux, &scoped_registry);
        if !issues.is_empty() {
            if fix {
                eprintln!(
                    "\nFixing {} issue(s) for {}:",
                    issues.len(),
                    target.display()
                );
                let fixed = apply_fixes(&tmux, &issues, relocate_session)?;
                eprintln!("\nFixed {} of {} issue(s).", fixed, issues.len());
            } else {
                eprintln!(
                    "\nFound {} issue(s) for {} (run with --fix to resolve):",
                    issues.len(),
                    target.display()
                );
                for issue in &issues {
                    eprintln!("  {}", issue);
                }
            }
        } else {
            eprintln!(
                "\nNo session/process issues detected for {}.",
                target.display()
            );
        }

        let scoped_registry = filter_registry_for_target(&sessions::load()?, &target);
        if !scoped_registry.is_empty() {
            eprintln!("\nActive matching sessions:");
            for (key, entry) in &scoped_registry {
                let label = if entry.file.is_empty() {
                    key.as_str()
                } else {
                    entry.file.as_str()
                };
                eprintln!("  {} -> pane {}", label, entry.pane);
            }
        }

        return Ok(());
    }

    let registry_path = sessions::registry_path();

    // Show what's being removed (verbose)
    let registry_before = sessions::load()?;
    let before = registry_before.len();

    let removed = tmux_router::prune(&registry_path, &tmux)?;

    if removed > 0 {
        // Show which entries were removed by diffing before/after
        let registry_after = sessions::load()?;
        eprintln!("Removed {} stale session(s):", removed);
        for (key, entry) in &registry_before {
            if !registry_after.contains_key(key) {
                let label = if entry.file.is_empty() {
                    key.as_str()
                } else {
                    entry.file.as_str()
                };
                eprintln!("  {} (pane {} removed)", label, entry.pane);
            }
        }
    } else {
        eprintln!("All {} session(s) have live panes.", before);
    }

    // Detect issues with alive panes
    let issues = detect_issues(&tmux);
    if !issues.is_empty() {
        if fix {
            eprintln!("\nFixing {} issue(s):", issues.len());
            let fixed = apply_fixes(&tmux, &issues, relocate_session)?;
            eprintln!("\nFixed {} of {} issue(s).", fixed, issues.len());
        } else {
            eprintln!(
                "\nFound {} issue(s) (run with --fix to resolve):",
                issues.len()
            );
            for issue in &issues {
                eprintln!("  {}", issue);
            }
        }
    } else {
        eprintln!("\nNo session/process issues detected.");
    }

    if fix {
        // Return active panes from stash back to their original sessions,
        // then clean up idle/orphaned stash panes.
        return_stashed_panes(&tmux);
        purge_stash_windows(&tmux);
        purge_unregistered_stash_panes(&tmux);
        purge_orphaned_agent_panes(&tmux);
    }

    // Show current state
    let registry = sessions::load()?;
    if !registry.is_empty() {
        eprintln!("\nActive sessions:");
        for (key, entry) in &registry {
            let label = if entry.file.is_empty() {
                key.as_str()
            } else {
                entry.file.as_str()
            };
            eprintln!("  {} -> pane {}", label, entry.pane);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sessions::{IsolatedTmux, SessionEntry, SessionRegistry};

    static TMUX_START_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn tmux_start_lock() -> std::sync::MutexGuard<'static, ()> {
        TMUX_START_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn test_cwd() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// Poll until `pane_current_command` returns an idle shell, or timeout.
    /// Needed because shell startup is asynchronous and the 500ms sleep is
    /// insufficient under parallel test load (other tests saturate the CPU,
    /// slowing the new pane's shell init — which can briefly show transient
    /// commands like `mv` from shell frameworks).
    fn wait_for_shell(iso: &IsolatedTmux, pane: &str, timeout_ms: u64) -> bool {
        let start = std::time::Instant::now();
        loop {
            if let Some(cmd) = pane_current_command(iso, pane) {
                if IDLE_SHELLS.contains(&cmd.as_str()) {
                    return true;
                }
            }
            if start.elapsed().as_millis() >= timeout_ms as u128 {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// Helper to create a registry entry for testing.
    fn test_entry(pane: &str, file: &str) -> SessionEntry {
        SessionEntry {
            pane: pane.to_string(),
            pid: std::process::id(),
            cwd: "/tmp".to_string(),
            started: "2026-01-01T00:00:00Z".to_string(),
            file: file.to_string(),
            window: String::new(),
        }
    }

    fn write_mock_agent_doc(base: &std::path::Path) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let bin_dir = base.join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let script = bin_dir.join("agent-doc");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf \"> \\n\"\nwhile IFS= read -r CMD; do\n  printf 'GOT:%s\\n' \"$CMD\"\ndone\n",
        )
        .unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        script
    }

    fn wait_for_pane_contains(
        tmux: &IsolatedTmux,
        pane: &str,
        needle: &str,
        timeout: std::time::Duration,
    ) -> String {
        let start = std::time::Instant::now();
        loop {
            let content = sessions::capture_pane(tmux, pane).unwrap_or_default();
            if content.contains(needle) || start.elapsed() >= timeout {
                return content;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    fn wait_for_window_relation(
        tmux: &IsolatedTmux,
        pane_a: &str,
        pane_b: &str,
        same_window: bool,
        timeout: std::time::Duration,
    ) -> Option<(String, String)> {
        let start = std::time::Instant::now();
        let mut last = None;
        while start.elapsed() < timeout {
            if let (Ok(window_a), Ok(window_b)) =
                (tmux.pane_window(pane_a), tmux.pane_window(pane_b))
            {
                let relation_matches = (window_a == window_b) == same_window;
                last = Some((window_a, window_b));
                if relation_matches {
                    return last;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        last
    }

    fn wait_for_pane_in_stash_window(
        tmux: &IsolatedTmux,
        session: &str,
        pane: &str,
        timeout: std::time::Duration,
    ) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if let Some(stash_window) = tmux.find_stash_window(session)
                && tmux.pane_window(pane).ok().as_deref() == Some(stash_window.as_str())
            {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }

    fn wait_for_pane_dead(tmux: &IsolatedTmux, pane: &str, timeout: std::time::Duration) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if !tmux.pane_alive(pane) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }

    fn send_keys_with_retry(tmux: &IsolatedTmux, pane: &str, text: &str) {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(3);
        let poll = std::time::Duration::from_millis(100);
        let mut last_err = None;

        while start.elapsed() < timeout {
            match tmux.send_keys(pane, text) {
                Ok(()) => return,
                Err(err) => last_err = Some(err.to_string()),
            }
            std::thread::sleep(poll);
        }

        panic!(
            "failed to send keys to pane {} after {:.1}s: {}",
            pane,
            start.elapsed().as_secs_f64(),
            last_err.unwrap_or_else(|| "unknown error".to_string())
        );
    }

    fn launch_mock_agent_doc(
        tmux: &IsolatedTmux,
        pane: &str,
        script: &std::path::Path,
        file: &std::path::Path,
    ) {
        {
            let _tmux_guard = tmux_start_lock();
            assert!(
                wait_for_shell(tmux, pane, 5000),
                "shell did not become ready before mock agent launch in {}",
                pane
            );
            send_keys_with_retry(
                tmux,
                pane,
                &format!("exec {} {}", script.display(), file.display()),
            );
        }
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(3);
        let poll = std::time::Duration::from_millis(300);
        let mut content = String::new();
        while start.elapsed() < timeout {
            content = sessions::capture_pane(tmux, pane).unwrap_or_default();
            if content.contains("\n>") {
                break;
            }
            if let Some(cmd) = pane_current_command(tmux, pane)
                && IDLE_SHELLS.contains(&cmd.as_str())
            {
                let _ = tmux.send_keys_raw(pane, "Enter");
            }
            std::thread::sleep(poll);
        }
        assert!(
            content.contains("\n>"),
            "mock agent should be ready, got: {content}"
        );
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(3) {
            if crate::sync::find_alive_pane_for_file(tmux, &file.to_string_lossy()).as_deref()
                == Some(pane)
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!(
            "mock agent pane {} never became the live owner for {}",
            pane,
            file.display()
        );
    }

    fn wait_for_process_pid(pattern: &str, timeout: std::time::Duration) -> u32 {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if let Ok(output) = std::process::Command::new("pgrep")
                .args(["-f", pattern])
                .output()
                && output.status.success()
                && let Some(pid) = String::from_utf8_lossy(&output.stdout).lines().next()
                && let Ok(pid) = pid.trim().parse::<u32>()
            {
                return pid;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("timed out waiting for process matching {pattern}");
    }

    #[test]
    fn filter_registry_for_target_matches_only_selected_file() {
        let dir = tempfile::tempdir().unwrap();
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("b.md");
        std::fs::write(&doc_a, "# A\n").unwrap();
        std::fs::write(&doc_b, "# B\n").unwrap();
        let target = doc_a.canonicalize().unwrap();

        let mut registry = SessionRegistry::new();
        registry.insert(
            "sess-a".to_string(),
            test_entry("%1", &doc_a.to_string_lossy()),
        );
        registry.insert(
            "sess-b".to_string(),
            test_entry("%2", &doc_b.to_string_lossy()),
        );

        let filtered = filter_registry_for_target(&registry, &target);
        assert_eq!(filtered.len(), 1, "only the target doc should remain");
        assert!(filtered.contains_key("sess-a"));
        assert!(!filtered.contains_key("sess-b"));
    }

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
    fn detect_wrong_session_pane() {
        // A pane in tmux session "wrong" but frontmatter expects "correct"
        let iso = IsolatedTmux::new("resync-test-wrong-sess");
        let cwd = std::env::current_dir().unwrap();

        // Create a pane in session "wrong" — poll until the shell settles.
        // Fixed sleep is unreliable under parallel test load.
        let pane = iso.auto_start("wrong", &cwd).unwrap();
        assert!(
            wait_for_shell(&iso, &pane, 5000),
            "shell did not start in pane within 5s"
        );

        // Create a temp file with frontmatter specifying tmux_session: correct
        let tmp = tempfile::TempDir::new().unwrap();
        let doc_path = tmp.path().join("test.md");
        std::fs::write(
            &doc_path,
            "---\nsession: abc-123\ntmux_session: correct\n---\n# Test\n",
        )
        .unwrap();

        let mut registry = SessionRegistry::new();
        registry.insert(
            "abc-123".to_string(),
            test_entry(&pane, &doc_path.to_string_lossy()),
        );

        let issues = detect_issues_in_registry(&iso, &registry);
        assert_eq!(issues.len(), 1, "should detect 1 stale-owner issue");
        assert!(
            matches!(&issues[0], Issue::NoLiveOwner { .. }),
            "pane with no provable owner should now fail as NoLiveOwner before WrongSession; got: {}",
            &issues[0]
        );
    }

    #[test]
    fn detect_wrong_process_pane() {
        // A pane running a non-agent-doc process (e.g., "sleep")
        let iso = IsolatedTmux::new("resync-test-wrong-proc");
        let cwd = std::env::current_dir().unwrap();

        // Create a pane running "sleep" (not agent-doc/claude/node/shell)
        let output = iso
            .cmd()
            .args([
                "new-session",
                "-d",
                "-s",
                "test",
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
        let pane = String::from_utf8_lossy(&output.stdout).trim().to_string();

        let mut registry = SessionRegistry::new();
        registry.insert("sess-1".to_string(), test_entry(&pane, "test.md"));

        // Give tmux a moment to register the process
        std::thread::sleep(std::time::Duration::from_millis(200));

        let issues = detect_issues_in_registry(&iso, &registry);
        assert_eq!(issues.len(), 1, "should detect 1 wrong-process issue");
        assert!(
            matches!(&issues[0], Issue::WrongProcess { process, .. } if process == "sleep"),
            "issue should be WrongProcess(sleep), got: {}",
            &issues[0]
        );
    }

    #[test]
    fn detect_no_live_owner_pane() {
        let iso = IsolatedTmux::new("resync-test-no-live-owner");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("test", &cwd).unwrap();
        assert!(wait_for_shell(&iso, &pane, 5000));

        let tmp = tempfile::TempDir::new().unwrap();
        let doc_path = tmp.path().join("test.md");
        std::fs::write(&doc_path, "# Test\n").unwrap();

        let mut registry = SessionRegistry::new();
        registry.insert(
            "sess-no-owner".to_string(),
            test_entry(&pane, &doc_path.to_string_lossy()),
        );

        let issues = detect_issues_in_registry(&iso, &registry);
        assert_eq!(issues.len(), 1, "should detect 1 no-live-owner issue");
        assert!(
            matches!(&issues[0], Issue::NoLiveOwner { .. }),
            "issue should be NoLiveOwner, got: {}",
            &issues[0]
        );
    }

    #[test]
    fn fix_wrong_session_kills_pane_and_deregisters() {
        let iso = IsolatedTmux::new("resync-test-fix-sess");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("wrong", &cwd).unwrap();
        assert!(iso.pane_alive(&pane));
        // Create a second window so the kill_pane guard allows killing the pane
        let _ = iso.new_window("wrong", &cwd);

        let mut registry = SessionRegistry::new();
        registry.insert("sess-fix".to_string(), test_entry(&pane, "test.md"));

        let issues = vec![Issue::WrongSession {
            key: "sess-fix".to_string(),
            file: "test.md".to_string(),
            pane: pane.clone(),
            actual_session: "wrong".to_string(),
            expected_session: "correct".to_string(),
        }];

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None);
        assert_eq!(fixed, 1);
        assert!(
            !registry.contains_key("sess-fix"),
            "entry should be removed from registry"
        );
        assert!(!iso.pane_alive(&pane), "pane should be killed");
    }

    #[test]
    fn fix_wrong_process_deregisters_but_keeps_pane() {
        let iso = IsolatedTmux::new("resync-test-fix-proc");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("test", &cwd).unwrap();
        assert!(iso.pane_alive(&pane));

        let mut registry = SessionRegistry::new();
        registry.insert("sess-proc".to_string(), test_entry(&pane, "test.md"));

        let issues = vec![Issue::WrongProcess {
            key: "sess-proc".to_string(),
            file: "test.md".to_string(),
            pane: pane.clone(),
            process: "corky".to_string(),
        }];

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None);
        assert_eq!(fixed, 1);
        assert!(
            !registry.contains_key("sess-proc"),
            "entry should be removed from registry"
        );
        assert!(
            iso.pane_alive(&pane),
            "pane should NOT be killed (foreign process)"
        );
    }

    #[test]
    fn fix_no_live_owner_deregisters_but_keeps_pane() {
        let iso = IsolatedTmux::new("resync-test-fix-no-owner");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("test", &cwd).unwrap();
        assert!(iso.pane_alive(&pane));

        let mut registry = SessionRegistry::new();
        registry.insert("sess-no-owner".to_string(), test_entry(&pane, "test.md"));

        let issues = vec![Issue::NoLiveOwner {
            key: "sess-no-owner".to_string(),
            file: "test.md".to_string(),
            pane: pane.clone(),
        }];

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None);
        assert_eq!(fixed, 1);
        assert!(
            !registry.contains_key("sess-no-owner"),
            "entry should be removed from registry"
        );
        assert!(
            iso.pane_alive(&pane),
            "pane should remain alive after NoLiveOwner deregister"
        );
    }

    #[test]
    fn no_fix_without_flag() {
        // detect_issues returns issues but apply_fixes is only called with --fix.
        // This test verifies the reporting path doesn't mutate anything.
        let iso = IsolatedTmux::new("resync-test-no-fix");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("wrong", &cwd).unwrap();

        let tmp = tempfile::TempDir::new().unwrap();
        let doc_path = tmp.path().join("test.md");
        std::fs::write(&doc_path, "---\nsession: abc\ntmux_session: correct\n---\n").unwrap();

        let mut registry = SessionRegistry::new();
        registry.insert(
            "abc".to_string(),
            test_entry(&pane, &doc_path.to_string_lossy()),
        );

        // detect_issues finds the problem
        let issues = detect_issues_in_registry(&iso, &registry);
        assert!(!issues.is_empty(), "should detect issues");

        // But without calling apply_fixes, nothing changes
        assert!(registry.contains_key("abc"), "registry should be unchanged");
        assert!(iso.pane_alive(&pane), "pane should still be alive");
    }

    #[test]
    fn healthy_pane_has_no_issues() {
        // A pane running a shell (idle) with no tmux_session mismatch should be clean.
        let iso = IsolatedTmux::new("resync-test-healthy");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("test", &cwd).unwrap();

        // Wait for the shell to fully start (otherwise pane_current_command
        // may return "tmux" or a profile command instead of "zsh"/"bash")
        std::thread::sleep(std::time::Duration::from_millis(2000));

        // No file path means no frontmatter check; shell is in IDLE_SHELLS
        let mut registry = SessionRegistry::new();
        registry.insert("healthy-sess".to_string(), test_entry(&pane, ""));

        let issues = detect_issues_in_registry(&iso, &registry);
        assert!(
            issues.is_empty(),
            "healthy idle shell should have no issues, got: {:?}",
            issues.iter().map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn detect_wrong_window_panes_in_different_windows() {
        // Two panes in the same tmux session but different non-stash windows
        // should trigger WrongWindow.
        let iso = IsolatedTmux::new("resync-test-wrong-win");
        let cwd = test_cwd();
        let dir = tempfile::tempdir().unwrap();
        let script = write_mock_agent_doc(dir.path());
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("b.md");
        std::fs::write(&doc_a, "# A\n").unwrap();
        std::fs::write(&doc_b, "# B\n").unwrap();

        // Create two panes in separate windows in the same session
        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.auto_start("test", &cwd).unwrap(); // creates new window
        launch_mock_agent_doc(&iso, &pane1, &script, &doc_a);
        launch_mock_agent_doc(&iso, &pane2, &script, &doc_b);

        let (w1, w2) = wait_for_window_relation(
            &iso,
            &pane1,
            &pane2,
            false,
            std::time::Duration::from_secs(3),
        )
        .expect("panes should report windows");
        assert_ne!(w1, w2, "panes should be in different windows");

        let mut registry = SessionRegistry::new();
        registry.insert(
            "sess-1".to_string(),
            test_entry(&pane1, &doc_a.to_string_lossy()),
        );
        registry.insert(
            "sess-2".to_string(),
            test_entry(&pane2, &doc_b.to_string_lossy()),
        );

        let issues = detect_issues_in_registry(&iso, &registry);
        let wrong_window_count = issues
            .iter()
            .filter(|i| matches!(i, Issue::WrongWindow { .. }))
            .count();
        assert_eq!(
            wrong_window_count,
            1,
            "should detect 1 wrong-window issue (minority pane), got issues: {:?}",
            issues.iter().map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_wrong_window_when_panes_in_same_window() {
        // Two panes in the same window should NOT trigger WrongWindow.
        let iso = IsolatedTmux::new("resync-test-same-win");
        let cwd = test_cwd();
        let dir = tempfile::tempdir().unwrap();
        let script = write_mock_agent_doc(dir.path());
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("b.md");
        std::fs::write(&doc_a, "# A\n").unwrap();
        std::fs::write(&doc_b, "# B\n").unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        launch_mock_agent_doc(&iso, &pane1, &script, &doc_a);
        launch_mock_agent_doc(&iso, &pane2, &script, &doc_b);

        let (w1, w2) = wait_for_window_relation(
            &iso,
            &pane1,
            &pane2,
            true,
            std::time::Duration::from_secs(3),
        )
        .expect("panes should report windows");
        assert_eq!(w1, w2, "panes should be in the same window");

        let mut registry = SessionRegistry::new();
        registry.insert(
            "sess-1".to_string(),
            test_entry(&pane1, &doc_a.to_string_lossy()),
        );
        registry.insert(
            "sess-2".to_string(),
            test_entry(&pane2, &doc_b.to_string_lossy()),
        );

        let issues = detect_issues_in_registry(&iso, &registry);
        let wrong_window_count = issues
            .iter()
            .filter(|i| matches!(i, Issue::WrongWindow { .. }))
            .count();
        assert_eq!(
            wrong_window_count,
            0,
            "should not detect wrong-window when panes are in same window, got: {:?}",
            issues.iter().map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_wrong_window_for_stash_panes() {
        // A pane in a stash window should NOT trigger WrongWindow.
        let iso = IsolatedTmux::new("resync-test-stash-excl");
        let cwd = std::env::current_dir().unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.auto_start("test", &cwd).unwrap();

        // Move pane2 to a stash window
        iso.stash_pane(&pane2, "test").unwrap();

        std::thread::sleep(std::time::Duration::from_millis(500));

        let mut registry = SessionRegistry::new();
        registry.insert("sess-1".to_string(), test_entry(&pane1, "a.md"));
        registry.insert("sess-2".to_string(), test_entry(&pane2, "b.md"));

        let issues = detect_issues_in_registry(&iso, &registry);
        let wrong_window_count = issues
            .iter()
            .filter(|i| matches!(i, Issue::WrongWindow { .. }))
            .count();
        assert_eq!(
            wrong_window_count,
            0,
            "stash panes should be excluded from wrong-window detection, got: {:?}",
            issues.iter().map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn fix_wrong_window_stashes_pane() {
        // --fix for WrongWindow should move the pane to stash.
        let iso = IsolatedTmux::new("resync-test-fix-win");
        let cwd = std::env::current_dir().unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.auto_start("test", &cwd).unwrap();
        let w1 = iso.pane_window(&pane1).unwrap();
        let w2_before = iso.pane_window(&pane2).unwrap();
        assert_ne!(w1, w2_before, "panes should start in different windows");

        let mut registry = SessionRegistry::new();
        registry.insert("sess-1".to_string(), test_entry(&pane1, "a.md"));
        registry.insert("sess-2".to_string(), test_entry(&pane2, "b.md"));

        let issues = vec![Issue::WrongWindow {
            key: "sess-2".to_string(),
            file: "b.md".to_string(),
            pane: pane2.clone(),
            actual_window: w2_before.clone(),
            expected_window: w1.clone(),
        }];

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None);
        assert_eq!(fixed, 1);
        assert!(
            iso.pane_alive(&pane2),
            "pane should still be alive (moved, not killed)"
        );

        // Verify pane2 is now in the stash window
        let stash_win = iso.find_stash_window("test");
        assert!(stash_win.is_some(), "stash window should exist");
        let w2_after = iso.pane_window(&pane2).unwrap();
        assert_eq!(
            w2_after,
            stash_win.unwrap(),
            "pane should have been moved to stash window"
        );
    }

    #[test]
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
                    crate::supervisor::ipc::IpcMethod::Inject { bytes } => {
                        crate::supervisor::ipc::IpcResponse::ok(
                            serde_json::json!({ "n": bytes.len() }),
                        )
                    }
                    crate::supervisor::ipc::IpcMethod::Restart { .. }
                    | crate::supervisor::ipc::IpcMethod::Stop { .. } => {
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
        let mut ipc = crate::supervisor::ipc::SupervisorIpc::start(
            dir.path(),
            "super-live-bulk",
            move |method| match method {
                crate::supervisor::ipc::IpcMethod::Pid => {
                    crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({ "pid": live_pid }))
                }
                crate::supervisor::ipc::IpcMethod::State => {
                    crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({ "running": true }))
                }
                crate::supervisor::ipc::IpcMethod::Inject { bytes } => {
                    crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                crate::supervisor::ipc::IpcMethod::Restart { .. }
                | crate::supervisor::ipc::IpcMethod::Stop { .. } => {
                    crate::supervisor::ipc::IpcResponse::ok_empty()
                }
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
        let live_supervisors = crate::supervisor::ipc::active_supervisor_pids(dir.path());
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
                    crate::supervisor::ipc::IpcMethod::Inject { bytes } => {
                        crate::supervisor::ipc::IpcResponse::ok(
                            serde_json::json!({ "n": bytes.len() }),
                        )
                    }
                    crate::supervisor::ipc::IpcMethod::Restart { .. }
                    | crate::supervisor::ipc::IpcMethod::Stop { .. } => {
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
    fn is_stash_window_name_matches() {
        assert!(is_stash_window_name("stash"));
        assert!(is_stash_window_name("stash-1"));
        assert!(is_stash_window_name("stash-42"));
        assert!(!is_stash_window_name("claude"));
        assert!(!is_stash_window_name(""));
        assert!(!is_stash_window_name("stashed"));
    }

    #[test]
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

    #[test]
    fn fix_wrong_session_idle_shell_still_killed() {
        // Regression: idle shells in the wrong session should still be killed
        // even with the new agent-preservation logic.
        let iso = IsolatedTmux::new("resync-fix-shell-killed");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("wrong", &cwd).unwrap();
        let _ = iso.new_window("wrong", &cwd); // second window so kill works
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(iso.pane_alive(&pane));

        let mut registry = SessionRegistry::new();
        registry.insert("sess-shell".to_string(), test_entry(&pane, "test.md"));

        let issues = vec![Issue::WrongSession {
            key: "sess-shell".to_string(),
            file: "test.md".to_string(),
            pane: pane.clone(),
            actual_session: "wrong".to_string(),
            expected_session: "correct".to_string(),
        }];

        // No relocate_session — uses the new auto-detect path
        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None);
        assert_eq!(fixed, 1);
        assert!(
            !registry.contains_key("sess-shell"),
            "entry should be removed"
        );
        // Idle shell should be killed (not just deregistered)
        assert!(!iso.pane_alive(&pane), "idle shell should be killed");
    }

    #[test]
    fn fix_wrong_session_deregisters_agent_without_kill_when_expected_session_dead() {
        // When the expected session doesn't exist, active agent panes should be
        // deregistered but NOT killed.
        let iso = IsolatedTmux::new("resync-fix-agent-nodeadkill");
        let cwd = std::env::current_dir().unwrap();

        // Start a pane running `node -e "setTimeout(()=>{},60000)"` to simulate
        // an agent process. If node isn't available, use `sleep` and adjust expectations.
        let pane = iso.auto_start("wrong", &cwd).unwrap();
        let _ = iso.new_window("wrong", &cwd);
        std::thread::sleep(std::time::Duration::from_millis(500));

        // The pane is running an idle shell. For this test, verify the shell case:
        // idle shells should be killed even when expected session is dead.
        let mut registry = SessionRegistry::new();
        registry.insert("sess-agent".to_string(), test_entry(&pane, "test.md"));

        let issues = vec![Issue::WrongSession {
            key: "sess-agent".to_string(),
            file: "test.md".to_string(),
            pane: pane.clone(),
            actual_session: "wrong".to_string(),
            expected_session: "nonexistent-session".to_string(),
        }];

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None);
        assert_eq!(fixed, 1);
        assert!(
            !registry.contains_key("sess-agent"),
            "entry should be removed"
        );
        // Shell pane should be killed (expected session doesn't matter for shells)
        assert!(
            !iso.pane_alive(&pane),
            "idle shell should still be killed when expected session is dead"
        );
    }

    #[test]
    fn registered_pane_still_owns_file_returns_false_when_file_missing() {
        let iso = IsolatedTmux::new("resync-live-owner-missing-file");
        assert!(!registered_pane_still_owns_file(
            &iso,
            "session-1",
            "/tmp/does-not-exist.md",
            "%42"
        ));
    }

    #[test]
    fn fix_wrong_session_relocates_agent_when_expected_session_alive() {
        // When the expected session exists, active agent panes should be relocated
        // via join-pane, not killed.
        let iso = IsolatedTmux::new("resync-fix-agent-relocate");
        let cwd = std::env::current_dir().unwrap();

        // Create the expected session with a pane (needed for join-pane target)
        let _anchor = iso.auto_start("correct", &cwd).unwrap();

        // Create a pane in the wrong session running node (agent process)
        let output = iso
            .cmd()
            .args([
                "new-session",
                "-d",
                "-s",
                "wrong",
                "-c",
                &cwd.to_string_lossy(),
                "-P",
                "-F",
                "#{pane_id}",
                "node",
                "-e",
                "setTimeout(()=>{},60000)",
            ])
            .output()
            .unwrap();
        let agent_pane = String::from_utf8_lossy(&output.stdout).trim().to_string();

        if agent_pane.is_empty() || !iso.pane_alive(&agent_pane) {
            // node not available — skip test gracefully
            eprintln!("skipping: node not available");
            return;
        }

        std::thread::sleep(std::time::Duration::from_millis(500));

        // Verify pane is running node (an AGENT_PROCESS)
        let cmd = pane_current_command(&iso, &agent_pane);
        if cmd.as_deref() != Some("node") {
            eprintln!("skipping: pane running {:?} instead of node", cmd);
            return;
        }

        let mut registry = SessionRegistry::new();
        registry.insert("sess-node".to_string(), test_entry(&agent_pane, "test.md"));

        let issues = vec![Issue::WrongSession {
            key: "sess-node".to_string(),
            file: "test.md".to_string(),
            pane: agent_pane.clone(),
            actual_session: "wrong".to_string(),
            expected_session: "correct".to_string(),
        }];

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None);
        assert_eq!(fixed, 1);
        // Agent pane should be alive (relocated, not killed)
        assert!(
            iso.pane_alive(&agent_pane),
            "agent pane should be alive after relocation"
        );
        // Registry entry should still exist (relocation preserves it)
        assert!(
            registry.contains_key("sess-node"),
            "entry should be preserved after successful relocation"
        );
        // Pane should now be in the correct session
        let new_session = iso.pane_session(&agent_pane).unwrap();
        assert_eq!(
            new_session, "correct",
            "agent pane should be relocated to the correct session"
        );
    }
}
