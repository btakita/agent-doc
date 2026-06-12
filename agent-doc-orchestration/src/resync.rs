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
//!   they are purged as orphaned. Automatic prune also reaps unregistered
//!   retained-dead panes in non-stash windows when another pane remains in that
//!   window, so dead-pane diagnostics do not linger forever after the registry
//!   forgets them. `purge_orphaned_agent_panes` removes unregistered
//!   agent-doc/claude/node panes from any window, but only when the window has at
//!   least one other pane (never orphans the last pane).
//! - Process classification: `AGENT_PROCESSES` (`agent-doc`, `claude`, `codex`, `node`) are
//!   expected occupants of registered panes. `IDLE_SHELLS` (`zsh`, `bash`, `sh`,
//!   `fish`) are treated as empty/unused slots. Short-lived shell startup helpers
//!   (for example `mkdir`, `mv`, `xset`) must not be classified as `WrongProcess`
//!   unless they remain the stable foreground command across a brief grace window.
//!
//! ## Agentic Contracts
//! - `prune()` never kills a registered pane that is alive and in a non-stash window;
//!   it only removes dead entries from the registry, stash-specific garbage, and
//!   unregistered retained-dead panes that still have sibling panes.
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
//! - `purge_unregistered_dead_non_stash_panes_skips_last_pane`: window with a single
//!   retained dead pane → pane not killed automatically.
//! - `wrong_window_detected_by_majority_vote`: three registered panes in session A,
//!   two in window W1 and one in window W2 → the W2 pane produces a `WrongWindow`
//!   issue; the W1 panes do not.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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

fn pane_process_kind_from_current_command(cmd: &str) -> PaneProcessKind {
    if AGENT_PROCESSES.contains(&cmd) {
        PaneProcessKind::Agent(cmd.to_string())
    } else if IDLE_SHELLS.contains(&cmd) {
        PaneProcessKind::IdleShell(cmd.to_string())
    } else if cmd.is_empty() {
        PaneProcessKind::UnknownTransient
    } else {
        PaneProcessKind::Foreign(cmd.to_string())
    }
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

/// Resolve the `.agent-doc` project root for a target file.
/// Falls back to the current working directory when no `.agent-doc/` ancestor is found.
fn resolve_registry_root(target: &Path) -> PathBuf {
    crate::snapshot::find_project_root(target)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
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
        .filter(|(key, entry)| {
            same_document_path(target, &entry.file) || same_document_path(target, key)
        })
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TargetDocumentFixOutcome {
    pub pruned_dead_entries: usize,
    pub reregistered_owner: bool,
    pub killed_redundant_stash_panes: usize,
    pub fixed_issues: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrunePhaseTiming {
    pub phase: &'static str,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneCleanupMode {
    Full,
    PreserveLiveAgentStashPanes,
    SkipExpensiveStashCleanup,
}

fn record_prune_phase<T>(
    timings: &mut Vec<PrunePhaseTiming>,
    phase: &'static str,
    f: impl FnOnce() -> T,
) -> T {
    let start = Instant::now();
    let value = f();
    timings.push(PrunePhaseTiming {
        phase,
        elapsed: start.elapsed(),
    });
    value
}

impl TargetDocumentFixOutcome {
    pub fn made_changes(self) -> bool {
        self.pruned_dead_entries > 0
            || self.reregistered_owner
            || self.killed_redundant_stash_panes > 0
            || self.fixed_issues > 0
    }
}

fn recover_target_document_pane_in(
    tmux: &Tmux,
    target: &Path,
    base_dir: &Path,
) -> Result<TargetDocumentFixOutcome> {
    let Some(session_id) = frontmatter::read_session_id(target) else {
        return Ok(TargetDocumentFixOutcome::default());
    };

    let preferred_window = config::project_tmux_session()
        .as_deref()
        .and_then(|session| tmux.active_window(session));
    let candidates = crate::sync::find_associated_panes(tmux, target, &session_id);
    match crate::sync::resolve_associated_panes(candidates, preferred_window.as_deref()) {
        crate::sync::AssociatedPaneResolution::None => Ok(TargetDocumentFixOutcome::default()),
        crate::sync::AssociatedPaneResolution::Selected { winner, redundant } => {
            let mut outcome = TargetDocumentFixOutcome::default();
            if sessions::lookup_in(base_dir, &session_id)?.as_deref()
                != Some(winner.pane_id.as_str())
            {
                crate::sync::reregister_recovered_owner(
                    tmux,
                    target,
                    &session_id,
                    &winner.pane_id,
                )?;
                outcome.reregistered_owner = true;
                eprintln!(
                    "resync: re-registered {} to pane {}",
                    target.display(),
                    winner.pane_id
                );
            }
            let killed = kill_redundant_associated_stash_panes(tmux, &redundant);
            outcome.killed_redundant_stash_panes = killed;
            if killed > 0 {
                eprintln!(
                    "resync: removed {} redundant stash pane(s) for {}",
                    killed,
                    target.display()
                );
            }
            Ok(outcome)
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

fn prune_targeted_in(
    tmux: &Tmux,
    target: &Path,
    base_dir: &Path,
) -> Result<Vec<(String, sessions::SessionEntry)>> {
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
        outcome.fixed_issues = apply_fixes_with_base(tmux, &issues, None, Some(&base_dir))?;
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

#[derive(Default)]
struct PaneProjectContextCache {
    registries: std::collections::HashMap<PathBuf, sessions::SessionRegistry>,
    live_supervisors: std::collections::HashMap<PathBuf, Vec<(String, u32)>>,
}

fn pane_project_root(tmux: &Tmux, pane_id: &str) -> Option<PathBuf> {
    let output = tmux
        .cmd()
        .args([
            "display-message",
            "-t",
            pane_id,
            "-p",
            "#{pane_current_path}",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let current_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if current_path.is_empty() {
        return None;
    }
    let path = PathBuf::from(current_path);
    crate::snapshot::find_project_root(&path).or(Some(path))
}

fn registry_for_project_root<'a>(
    cache: &'a mut PaneProjectContextCache,
    project_root: &Path,
) -> &'a sessions::SessionRegistry {
    cache
        .registries
        .entry(project_root.to_path_buf())
        .or_insert_with(|| {
            let actor = crate::graph::ActorContext::for_project_root(project_root.to_path_buf());
            (*actor.context().session_registry()).clone()
        })
}

fn live_supervisors_for_project_root<'a>(
    cache: &'a mut PaneProjectContextCache,
    project_root: &Path,
) -> &'a Vec<(String, u32)> {
    cache
        .live_supervisors
        .entry(project_root.to_path_buf())
        .or_insert_with(|| crate::supervisor::ipc::active_supervisor_pids(project_root))
}

fn live_owned_registered_file_for_pane(
    tmux: &Tmux,
    pane_id: &str,
    registry: &sessions::SessionRegistry,
) -> Option<String> {
    registry.values().find_map(|entry| {
        if entry.file.is_empty() {
            return None;
        }
        let file = std::path::Path::new(&entry.file);
        if !file.exists() {
            return None;
        }
        (crate::sync::find_live_owner_pane(tmux, file, &entry.session_id).as_deref()
            == Some(pane_id))
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

fn purge_unregistered_stash_panes_bulk_in_mode(
    tmux: &Tmux,
    windows: &WindowMeta,
    panes: &PaneMeta,
    preserve_live_agent_stash_panes: bool,
) {
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let live_supervisors = crate::supervisor::ipc::active_supervisor_pids(&project_root);
    purge_unregistered_stash_panes_bulk_with_supervisors_in_mode(
        tmux,
        windows,
        panes,
        &live_supervisors,
        preserve_live_agent_stash_panes,
    );
}

fn purge_unregistered_stash_panes_bulk_with_supervisors(
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

fn purge_unregistered_stash_panes_bulk_with_supervisors_in_mode(
    tmux: &Tmux,
    windows: &WindowMeta,
    panes: &PaneMeta,
    live_supervisors: &[(String, u32)],
    preserve_live_agent_stash_panes: bool,
) {
    let current_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut pane_context_cache = PaneProjectContextCache::default();
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
    for (pane_id, (window_id, _window_name, cmd)) in panes {
        if !stash_windows.contains(window_id.as_str()) {
            continue;
        }
        if registered_panes.contains(pane_id.as_str()) {
            continue;
        }
        let process_kind = pane_process_kind_from_current_command(cmd);
        if preserve_live_agent_stash_panes && matches!(process_kind, PaneProcessKind::Agent(_)) {
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
            crate::ops_log::log_op(
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

fn sync_log_safe_passive_stash_skip(pane_id: &str) {
    eprintln!(
        "resync: safe-passive stash cleanup deferred live agent pane {} ownership proof",
        pane_id
    );
}

fn purge_unregistered_dead_non_stash_panes(tmux: &Tmux) {
    let registry = sessions::load().unwrap_or_default();
    purge_unregistered_dead_non_stash_panes_with_registry(tmux, &registry);
}

fn purge_unregistered_dead_non_stash_panes_with_registry(
    tmux: &Tmux,
    registry: &sessions::SessionRegistry,
) {
    let output = tmux
        .cmd()
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{pane_id}\t#{window_id}\t#{window_name}",
        ])
        .output();
    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return,
    };

    let panes: PaneMeta = String::from_utf8_lossy(&output.stdout)
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

fn purge_unregistered_dead_non_stash_panes_bulk(tmux: &Tmux, panes: &PaneMeta) {
    let registry = sessions::load().unwrap_or_default();
    purge_unregistered_dead_non_stash_panes_bulk_with_registry(tmux, panes, &registry);
}

fn purge_unregistered_dead_non_stash_panes_bulk_with_registry(
    tmux: &Tmux,
    panes: &PaneMeta,
    registry: &sessions::SessionRegistry,
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
    let current_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let live_supervisors = crate::supervisor::ipc::active_supervisor_pids(&current_root);
    let mut pane_context_cache = PaneProjectContextCache::default();
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
                    crate::ops_log::log_op(
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

/// Close a tmux session that has been superseded by a newly-canonical session.
///
/// Contract (`agent-doc session set <new>`): once a new tmux session becomes
/// canonical, a leftover *other* session should be closed and removed from the
/// model. Call this only after the agent-doc / stash windows have been migrated
/// to the new canonical session.
///
/// Safety — close `old_session` only when it is a pure agent-doc orphan:
/// - if tmux already auto-destroyed it (no windows remained), report it gone;
/// - otherwise close it only when every remaining window is agent-doc-managed
///   (`agent-doc`, `stash`, `stash-*`) **and** no pane runs a live agent process.
///
/// A session that still holds any unmanaged user window or a live agent is
/// preserved (and logged), so superseding a canonical session never destroys
/// unrelated work.
///
/// Returns `Ok(true)` when the session was closed (or was already gone), and
/// `Ok(false)` when it was deliberately preserved.
pub fn close_superseded_session(tmux: &Tmux, old_session: &str) -> Result<bool> {
    if !tmux.session_alive(old_session) {
        eprintln!(
            "[session] superseded session '{}' already closed (no windows remained)",
            old_session
        );
        return Ok(true);
    }

    let windows = tmux.list_window_names(old_session);
    let all_managed = !windows.is_empty()
        && windows
            .iter()
            .all(|w| w == "agent-doc" || is_stash_window_name(w));
    if !all_managed {
        eprintln!(
            "[session] preserving superseded session '{}': it still holds unmanaged window(s): [{}]",
            old_session,
            windows.join(", ")
        );
        return Ok(false);
    }

    for pane in tmux.list_session_panes(old_session) {
        if let PaneProcessKind::Agent(cmd) = classify_pane_process(tmux, &pane) {
            eprintln!(
                "[session] preserving superseded session '{}': pane {} still runs live agent '{}'",
                old_session, pane, cmd
            );
            return Ok(false);
        }
    }

    tmux.kill_session(old_session)
        .with_context(|| format!("closing superseded tmux session '{}'", old_session))?;
    eprintln!(
        "[session] closed superseded tmux session '{}' (only managed windows remained: [{}])",
        old_session,
        windows.join(", ")
    );
    Ok(true)
}

/// Query the tmux session name hosting a pane, if it is alive.
fn pane_session_name(tmux: &Tmux, pane_id: &str) -> Option<String> {
    let output = tmux
        .cmd()
        .args(["display-message", "-t", pane_id, "-p", "#{session_name}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Distinct tmux sessions hosting alive registered panes — the auto-resync
/// drift candidate set. Order is first-seen for determinism.
pub fn registered_pane_sessions(tmux: &Tmux, registry: &sessions::SessionRegistry) -> Vec<String> {
    let mut sessions = Vec::new();
    for entry in registry.values() {
        if let Some(session) = pane_session_name(tmux, &entry.pane)
            && !sessions.contains(&session)
        {
            sessions.push(session);
        }
    }
    sessions
}

/// Resolve the canonical tmux session for a document: the session holding the
/// live agent-doc supervisor registered for `file`
/// (`#canonical-session-close-autodetect`, canonical rule = active agent-doc
/// window session). Returns `None` when no registered pane for the document
/// currently runs a live agent-doc process.
pub fn canonical_session_for_document(
    tmux: &Tmux,
    registry: &sessions::SessionRegistry,
    file: &Path,
) -> Option<String> {
    let target = file.canonicalize().ok();
    registry.values().find_map(|entry| {
        if entry.file.is_empty() {
            return None;
        }
        let entry_path = Path::new(&entry.file);
        let matches = match (&target, entry_path.canonicalize().ok()) {
            (Some(target), Some(entry_canonical)) => *target == entry_canonical,
            _ => entry_path == file,
        };
        if !matches {
            return None;
        }
        // Only a pane running a live agent-doc supervisor is canonical.
        match classify_pane_process(tmux, &entry.pane) {
            PaneProcessKind::Agent(_) => pane_session_name(tmux, &entry.pane),
            _ => None,
        }
    })
}

/// The drift sessions that should be closed: every candidate except the
/// canonical one, deduped, order-stable. Pure — the destructive close is
/// delegated to `close_superseded_session`.
pub fn superseded_candidates(canonical: &str, drift_sessions: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for session in drift_sessions {
        if session == canonical || out.iter().any(|s| s == session) {
            continue;
        }
        out.push(session.clone());
    }
    out
}

/// Close superseded tmux sessions on the auto-resync drift path: every session
/// in `drift_sessions` other than `canonical` gets the safe
/// `close_superseded_session` treatment (which still preserves any session with
/// a live agent or an unmanaged user window). Returns the number closed.
pub fn close_superseded_drift_sessions(
    tmux: &Tmux,
    canonical: &str,
    drift_sessions: &[String],
) -> Result<usize> {
    let mut closed = 0;
    for session in superseded_candidates(canonical, drift_sessions) {
        if close_superseded_session(tmux, &session)? {
            closed += 1;
        }
    }
    Ok(closed)
}

/// Apply fixes for detected issues: kill wrong-session panes, deregister wrong-process panes.
fn apply_fixes(tmux: &Tmux, issues: &[Issue], relocate_session: Option<&str>) -> Result<usize> {
    apply_fixes_with_base(tmux, issues, relocate_session, None)
}

fn apply_fixes_with_base(
    tmux: &Tmux,
    issues: &[Issue],
    relocate_session: Option<&str>,
    base_dir: Option<&Path>,
) -> Result<usize> {
    if issues.is_empty() {
        return Ok(0);
    }
    tracing::debug!(issue_count = issues.len(), "resync::apply_fixes");

    let cwd;
    let effective_base = match base_dir {
        Some(dir) => dir,
        None => {
            cwd = std::env::current_dir()?;
            &cwd
        }
    };
    let registry_path = sessions::registry_path_in(effective_base);
    let _lock = tmux_router::RegistryLock::acquire(&registry_path)?;
    let mut registry = sessions::load_in(effective_base)?;
    let fixed = apply_fixes_to_registry(tmux, issues, &mut registry, relocate_session);

    if fixed > 0 {
        sessions::save_in(effective_base, &registry)?;
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
    // #jb-fix-document-finish-turn: when fixing a specific document, first finish
    // any unfinished agent-doc turn (the deterministic repair path) so `agent-doc
    // fix <FILE>` — and the JB `Fix Document` action that wraps it — recovers a
    // stranded response / reap before reconciling tmux routing.
    if let Some(file) = target_file {
        finish_unfinished_turn(file)?;
    }
    run(true, relocate_session, target_file)
}

/// Finish any unfinished agent-doc turn on `file` before routing reconciliation
/// (`#jb-fix-document-finish-turn`). Runs the deterministic repair path until
/// `session-check` is clean or no further progress is made — recovery can need a
/// second pass (commit the orphaned response, then persist the reap). Best-effort:
/// a document that stays interrupted falls through to the routing fix with a
/// warning rather than aborting the whole `fix`.
fn finish_unfinished_turn(file: &Path) -> Result<()> {
    use crate::session_check::SessionCheckStatus;
    if !file.exists() {
        return Ok(());
    }
    for _ in 0..4 {
        let before = std::fs::read_to_string(file)
            .ok()
            .map(|c| crate::ops_log::content_hash(&c));
        // Always attempt repair: it is a no-op on a clean document, and a stranded
        // pending response does not necessarily register as a session-check
        // interruption (no open cycle yet). `repair` bails on an interruption that
        // the *next* pass resolves (response commit → reap), so log and keep going
        // rather than aborting `fix` — never swallow the diagnostic.
        if let Err(e) = crate::repair::repair(file) {
            eprintln!("[fix] finish-turn pass interrupted (continuing): {e}");
        }
        if matches!(
            crate::session_check::inspect(file)?,
            SessionCheckStatus::Ok(_)
        ) {
            return Ok(());
        }
        let after = std::fs::read_to_string(file)
            .ok()
            .map(|c| crate::ops_log::content_hash(&c));
        if before == after {
            break; // no progress this pass and still not clean — stop looping
        }
    }
    if let SessionCheckStatus::Interrupted(msg) = crate::session_check::inspect(file)? {
        eprintln!("[fix] document still has an unfinished turn after finish-turn passes: {msg}");
    }
    Ok(())
}

/// `target_file`: when `Some(file)`, scope detection and mutations to that single
/// document instead of mutating the whole registry.
pub fn run(fix: bool, relocate_session: Option<&str>, target_file: Option<&Path>) -> Result<()> {
    let tmux = Tmux::default_server();

    if let Some(file) = target_file {
        let target = resolve_target_file(file)?;
        let base_dir = resolve_registry_root(&target);
        let removed = prune_targeted_in(&tmux, &target, &base_dir)?;

        if removed.is_empty() {
            let scoped = filter_registry_for_target(&sessions::load_in(&base_dir)?, &target);
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
            let _ = recover_target_document_pane_in(&tmux, &target, &base_dir)?;
        }

        let scoped_registry = filter_registry_for_target(&sessions::load_in(&base_dir)?, &target);
        let issues = detect_issues_in_registry(&tmux, &scoped_registry);
        if !issues.is_empty() {
            if fix {
                eprintln!(
                    "\nFixing {} issue(s) for {}:",
                    issues.len(),
                    target.display()
                );
                let fixed =
                    apply_fixes_with_base(&tmux, &issues, relocate_session, Some(&base_dir))?;
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

        let scoped_registry = filter_registry_for_target(&sessions::load_in(&base_dir)?, &target);
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
        purge_unregistered_dead_non_stash_panes(&tmux);
        purge_orphaned_agent_panes(&tmux);
        apply_stash_ttl_prune(&tmux);
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

/// `#stash-session-ttl-prune`: query live tmux state, build candidates, and
/// either log report-only or kill idle stash panes that exceed the configured TTL.
fn apply_stash_ttl_prune(tmux: &Tmux) {
    let config = crate::project_config::load_project();
    let ttl_secs = config.stash_session_ttl_secs;
    if ttl_secs == 0 {
        return;
    }

    let registry = sessions::load().unwrap_or_default();
    let registered_panes: std::collections::HashSet<&str> =
        registry.values().map(|e| e.pane.as_str()).collect();

    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let live_supervisors = crate::supervisor::ipc::active_supervisor_pids(&project_root);

    let window_output = tmux
        .cmd()
        .args([
            "list-windows",
            "-a",
            "-F",
            "#{window_id}\t#{window_name}\t#{window_activity}",
        ])
        .output();
    let window_output = match window_output {
        Ok(o) if o.status.success() => o,
        _ => return,
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut candidates: Vec<StashTtlCandidate> = Vec::new();

    for line in String::from_utf8_lossy(&window_output.stdout).lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let (window_id, window_name, activity_str) = (parts[0], parts[1], parts[2]);

        if !is_stash_window_name(window_name) {
            continue;
        }

        let window_activity = activity_str.parse::<u64>().unwrap_or(0);
        let idle_secs = now.saturating_sub(window_activity);

        let panes = tmux.list_window_panes(window_id).unwrap_or_default();
        for pane_id in panes {
            let is_active = false;
            let is_agent_doc = registered_panes.contains(pane_id.as_str())
                || live_supervisors.iter().any(|(_session_id, _pid)| {
                    pane_hosts_live_supervisor_session(tmux, &pane_id, &live_supervisors).is_some()
                });

            candidates.push(StashTtlCandidate {
                pane_id: pane_id.clone(),
                idle_secs,
                is_active_pane: is_active,
                is_agent_doc_stash_pane: is_agent_doc,
            });
        }
    }

    let targets = stash_ttl_prune_targets(&candidates, ttl_secs);

    if targets.is_empty() {
        return;
    }

    let current_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    for pane_id in &targets {
        crate::ops_log::log_op(
            &current_root,
            &format!(
                "stash_ttl_prune_candidate pane={} idle_secs={} ttl={} kill_enabled={}",
                pane_id,
                candidates
                    .iter()
                    .find(|c| c.pane_id == *pane_id)
                    .map(|c| c.idle_secs)
                    .unwrap_or(0),
                ttl_secs,
                config.stash_session_ttl_prune_enabled,
            ),
        );

        if config.stash_session_ttl_prune_enabled {
            let _ = tmux.cmd().args(["kill-pane", "-t", pane_id]).output();
            eprintln!(
                "stash-ttl-prune: killed idle stash pane {} (exceeded {}s TTL)",
                pane_id, ttl_secs
            );
        } else {
            eprintln!(
                "stash-ttl-prune: pane {} exceeded {}s TTL (report-only; enable stash_session_ttl_prune_enabled to kill)",
                pane_id, ttl_secs
            );
        }
    }
}

/// `#stash-session-ttl-prune`: one stash pane's candidacy for opt-in TTL reaping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashTtlCandidate {
    pub pane_id: String,
    /// Seconds since the pane's window last saw activity (`#{window_activity}`).
    pub idle_secs: u64,
    /// The currently active/visible pane is never reaped, regardless of idle time.
    pub is_active_pane: bool,
    /// Only an agent-doc pane parked in the `stash` window is eligible.
    pub is_agent_doc_stash_pane: bool,
}

/// `#stash-session-ttl-prune`: pure, **non-destructive** decision for whether a
/// stash-parked agent-doc pane is eligible for opt-in TTL reaping. Conservative
/// by construction (see `tasks/agent-doc/plan-stash-session-ttl-prune.md`): the
/// actual `kill-pane` wiring, the idle-signal query, the config knob, and live
/// verification are all gated — this is only the reusable decision core.
///
/// Returns true only when ALL hold: TTL is enabled (`ttl_secs > 0`; `0`/unset
/// disables), the pane is an agent-doc pane parked in the stash window, it is not
/// the active/visible pane, and it has been idle strictly longer than the TTL.
pub fn stash_ttl_prune_candidate(
    idle_secs: u64,
    ttl_secs: u64,
    is_active_pane: bool,
    is_agent_doc_stash_pane: bool,
) -> bool {
    ttl_secs > 0 && is_agent_doc_stash_pane && !is_active_pane && idle_secs > ttl_secs
}

/// Filter a candidate list to the pane ids eligible for TTL reaping. With
/// `ttl_secs == 0` (disabled / unset) this always returns empty, so the feature
/// is inert until explicitly configured.
pub fn stash_ttl_prune_targets(candidates: &[StashTtlCandidate], ttl_secs: u64) -> Vec<String> {
    candidates
        .iter()
        .filter(|c| {
            stash_ttl_prune_candidate(
                c.idle_secs,
                ttl_secs,
                c.is_active_pane,
                c.is_agent_doc_stash_pane,
            )
        })
        .map(|c| c.pane_id.clone())
        .collect()
}

#[cfg(test)]
mod stash_ttl_prune_tests;
#[cfg(test)]
mod tests;
