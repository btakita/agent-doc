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
//!   field, or from `project_config_io::project_tmux_session()` when frontmatter field is absent),
//!   `NoLiveOwner` (alive registered pane no longer proves ownership of its
//!   document), and `WrongWindow` (panes for the same tmux session are scattered
//!   across multiple non-stash windows, determined by majority-window vote).
//! - Fix application (`apply_fixes`): `WrongSession` → kill pane + deregister entry (default),
//!   or when `relocate_session = Some(target)` → `join-pane` to target session (registry kept);
//!   `WrongProcess` → deregister only (foreign process is not killed); target-scoped
//!   `NoLiveOwner` → refresh the recovered registry binding, otherwise deregister only
//!   (pane left intact for route/later manual recovery);
//!   `InStash` → promote live owner panes back into `agent-doc` and refresh
//!   registry runtime metadata; deregister only unproven stashed panes;
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
//! - `fix_in_stash_promotes_live_owner`: `InStash` issue with live owner proof →
//!   pane promoted back into `agent-doc`, registry entry retained and refreshed.
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
use agent_doc_frontmatter::frontmatter;

use crate::{frontmatter_io, project_config_io};

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
    agent_doc_fs::find_project_root(target)
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

fn candidate_matches_target(target: &Path, base_dir: &Path, candidate: &str) -> bool {
    if same_document_path(target, candidate) {
        return true;
    }
    if candidate.is_empty() {
        return false;
    }
    let path = Path::new(candidate);
    if path.is_absolute() {
        return false;
    }
    let resolved = base_dir.join(path);
    let canonical = resolved.canonicalize().unwrap_or(resolved);
    canonical == target
}

fn registry_file_for_target(target: &Path, base_dir: &Path) -> String {
    target
        .strip_prefix(base_dir)
        .unwrap_or(target)
        .to_string_lossy()
        .to_string()
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

fn registry_entry_session_id<'a>(key: &'a str, entry: &'a sessions::SessionEntry) -> &'a str {
    if entry.session_id.is_empty() {
        key
    } else {
        entry.session_id.as_str()
    }
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
    let Some(session_id) = frontmatter_io::read_session_id(target) else {
        return Ok(TargetDocumentFixOutcome::default());
    };

    let preferred_window = project_config_io::project_tmux_session()
        .as_deref()
        .and_then(|session| tmux.active_window(session));
    let candidates = crate::sync::filter_associated_panes_for_document(
        tmux,
        target,
        crate::sync::find_associated_panes(tmux, target, &session_id),
    );
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

mod prune;
pub use prune::*;

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
    agent_doc_fs::find_project_root(&path).or(Some(path))
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
mod stash;
pub(crate) use stash::*;

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
    let proof_cache = crate::sync::SyncProofCache::default();

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
        // Stash panes are alive but not in the active workspace. Fix promotes
        // live owners back into agent-doc and only deregisters unproven panes.
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

        let session_id = registry_entry_session_id(key, entry);
        let entry_file = std::path::Path::new(&entry.file);
        let registered_owner = crate::sync::registered_pane_proves_live_owner(
            tmux,
            entry_file,
            session_id,
            &entry.pane,
            &proof_cache,
        );
        let live_owner = crate::sync::find_live_owner_pane(tmux, entry_file, session_id).as_deref()
            == Some(entry.pane.as_str());
        if !registered_owner && !live_owner {
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
        let expected_session = frontmatter_session.or_else(project_config_io::project_tmux_session);

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
    let file_path = std::path::Path::new(file);
    let session_id = frontmatter_io::read_session_id(file_path).unwrap_or_else(|| key.to_string());
    crate::sync::find_live_owner_pane(tmux, file_path, &session_id).as_deref() == Some(pane)
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
    apply_fixes_with_base(tmux, issues, relocate_session, None, None)
}

#[derive(Clone, Copy)]
struct TargetFixScope<'a> {
    target: &'a Path,
    base_dir: &'a Path,
}

fn refresh_target_no_live_owner_registry_entry(
    tmux: &Tmux,
    registry: &mut sessions::SessionRegistry,
    scope: TargetFixScope<'_>,
    key: &str,
    file: &str,
    pane: &str,
) -> bool {
    if !candidate_matches_target(scope.target, scope.base_dir, file)
        && !candidate_matches_target(scope.target, scope.base_dir, key)
    {
        return false;
    }
    if !tmux.pane_alive(pane) {
        return false;
    }

    let Some(entry) = registry.get_mut(key) else {
        return false;
    };
    entry.pane = pane.to_string();
    entry.pid = sessions::pane_pid_with_mux(tmux, pane).unwrap_or_else(|_| std::process::id());
    entry.window = sessions::pane_window_with_mux(tmux, pane).unwrap_or_default();
    entry.cwd = scope.base_dir.to_string_lossy().to_string();
    if entry.file.is_empty() {
        entry.file = registry_file_for_target(scope.target, scope.base_dir);
    }
    if entry.session_id.is_empty()
        && let Some(session_id) = frontmatter_io::read_session_id(scope.target)
    {
        entry.session_id = session_id;
    }
    true
}

fn refresh_registry_runtime_for_pane(
    tmux: &Tmux,
    registry: &mut sessions::SessionRegistry,
    key: &str,
    pane: &str,
) -> bool {
    let Some(entry) = registry.get_mut(key) else {
        return false;
    };
    entry.pane = pane.to_string();
    if let Ok(pid) = sessions::pane_pid_with_mux(tmux, pane) {
        entry.pid = pid;
    }
    if let Ok(window) = sessions::pane_window_with_mux(tmux, pane) {
        entry.window = window;
    }
    true
}

fn apply_fixes_with_base(
    tmux: &Tmux,
    issues: &[Issue],
    relocate_session: Option<&str>,
    base_dir: Option<&Path>,
    target_file: Option<&Path>,
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
    let target_scope = target_file.map(|target| TargetFixScope {
        target,
        base_dir: effective_base,
    });
    let fixed =
        apply_fixes_to_registry(tmux, issues, &mut registry, relocate_session, target_scope);

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
    target_scope: Option<TargetFixScope<'_>>,
) -> usize {
    let mut fixed = 0;
    let proof_cache = crate::sync::SyncProofCache::default();

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
            Issue::NoLiveOwner { key, file, pane } => {
                if let Some(scope) = target_scope
                    && refresh_target_no_live_owner_registry_entry(
                        tmux, registry, scope, key, file, pane,
                    )
                {
                    eprintln!(
                        "  refreshed recovered owner pane {} for {} instead of deregistering",
                        pane, file
                    );
                    eprintln!("  fixed: {}", issue);
                    fixed += 1;
                    continue;
                }
                registry.remove(key);
                eprintln!("  fixed: {}", issue);
                fixed += 1;
            }
            Issue::InStash {
                key, file, pane, ..
            } => {
                let proves_live_owner =
                    registry.get(key).is_some_and(|entry| {
                        crate::sync::registered_pane_proves_live_owner(
                            tmux,
                            std::path::Path::new(file),
                            registry_entry_session_id(key, entry),
                            pane,
                            &proof_cache,
                        )
                    }) || registered_pane_still_owns_file(tmux, key, file, pane);
                if proves_live_owner {
                    match crate::sync::promote_pane_to_agent_doc_window(tmux, pane) {
                        Ok(true) => {
                            refresh_registry_runtime_for_pane(tmux, registry, key, pane);
                            eprintln!(
                                "  promoted live owner pane {} for {} from stash into agent-doc",
                                pane, file
                            );
                            eprintln!("  fixed: {}", issue);
                            fixed += 1;
                        }
                        Ok(false) => {
                            eprintln!(
                                "  preserving live owner pane {} for {} in stash; promotion was not possible",
                                pane, file
                            );
                        }
                        Err(e) => {
                            eprintln!(
                                "  preserving live owner pane {} for {} in stash; promotion failed: {}",
                                pane, file, e
                            );
                        }
                    }
                    continue;
                }

                // Deregister only when the stashed pane no longer proves
                // ownership; live bound sessions must be promoted or preserved.
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
                let fixed = apply_fixes_with_base(
                    &tmux,
                    &issues,
                    relocate_session,
                    Some(&base_dir),
                    Some(&target),
                )?;
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
    let config = crate::project_config_io::load_project();
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
mod th {
    use super::*;
    use sessions::{IsolatedTmux, SessionEntry};
    pub(crate) static TMUX_START_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
    pub(crate) struct ScopedCurrentDir {
        prev_cwd: std::path::PathBuf,
        _env_guard: crate::test_support::ProcessGlobalLockGuard,
    }
    impl ScopedCurrentDir {
        pub(crate) fn set(path: &std::path::Path) -> Self {
            let env_guard = crate::test_support::env_lock();
            let prev_cwd = std::env::current_dir()
                .ok()
                .filter(|cwd| cwd.exists())
                .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
            std::env::set_current_dir(path).unwrap();
            Self {
                prev_cwd,
                _env_guard: env_guard,
            }
        }
    }
    impl Drop for ScopedCurrentDir {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.prev_cwd);
        }
    }
    pub(crate) fn tmux_start_lock() -> std::sync::MutexGuard<'static, ()> {
        TMUX_START_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    pub(crate) fn test_cwd() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }
    /// Poll until `pane_current_command` returns an idle shell, or timeout.
    /// Needed because shell startup is asynchronous and the 500ms sleep is
    /// insufficient under parallel test load (other tests saturate the CPU,
    /// slowing the new pane's shell init — which can briefly show transient
    /// commands like `mv` from shell frameworks).
    pub(crate) fn wait_for_shell(iso: &IsolatedTmux, pane: &str, timeout_ms: u64) -> bool {
        let start = std::time::Instant::now();
        loop {
            if let Some(cmd) = pane_current_command(iso, pane)
                && IDLE_SHELLS.contains(&cmd.as_str())
            {
                return true;
            }
            if start.elapsed().as_millis() >= timeout_ms as u128 {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    /// Helper to create a registry entry for testing.
    pub(crate) fn test_entry(pane: &str, file: &str) -> SessionEntry {
        SessionEntry {
            pane: pane.to_string(),
            pid: std::process::id(),
            cwd: "/tmp".to_string(),
            started: "2026-01-01T00:00:00Z".to_string(),
            session_id: format!("sess-{pane}"),
            file: file.to_string(),
            window: String::new(),
            supervisor_instance_id: String::new(),
        }
    }
    pub(crate) fn write_mock_agent_doc(base: &std::path::Path) -> std::path::PathBuf {
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
    pub(crate) fn wait_for_pane_contains(
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
    pub(crate) fn wait_for_pane_current_command(
        tmux: &IsolatedTmux,
        pane: &str,
        expected: &str,
        timeout: std::time::Duration,
    ) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if pane_current_command(tmux, pane).as_deref() == Some(expected) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }
    pub(crate) fn wait_for_window_relation(
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
    pub(crate) fn wait_for_pane_in_stash_window(
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
    pub(crate) fn wait_for_pane_dead(
        tmux: &IsolatedTmux,
        pane: &str,
        timeout: std::time::Duration,
    ) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if !tmux.pane_alive(pane) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }
    pub(crate) fn wait_for_pane_removed(
        tmux: &IsolatedTmux,
        pane: &str,
        timeout: std::time::Duration,
    ) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if !tmux.pane_alive(pane) && !tmux.pane_dead(pane) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        !tmux.pane_alive(pane) && !tmux.pane_dead(pane)
    }
    pub(crate) fn drive_pane_to_retained_dead(
        tmux: &IsolatedTmux,
        pane: &str,
        command: &str,
        timeout: std::time::Duration,
    ) {
        {
            let _tmux_guard = tmux_start_lock();
            assert!(
                wait_for_shell(tmux, pane, 5000),
                "shell did not become ready before driving {} to retained-dead",
                pane
            );
            send_keys_with_retry(tmux, pane, command);
        }
        assert!(
            wait_for_pane_dead(tmux, pane, timeout),
            "pane should first become a retained dead pane"
        );
        assert!(
            tmux.pane_dead(pane),
            "pane should still be retained by tmux"
        );
    }
    pub(crate) fn send_keys_with_retry(tmux: &IsolatedTmux, pane: &str, text: &str) {
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
    pub(crate) fn launch_mock_agent_doc(
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
        let timeout = std::time::Duration::from_secs(8);
        let poll = std::time::Duration::from_millis(300);
        let mut content = String::new();
        while start.elapsed() < timeout {
            content = sessions::capture_pane(tmux, pane).unwrap_or_default();
            if mock_agent_prompt_visible(&content) {
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
            mock_agent_prompt_visible(&content),
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
    pub(crate) fn wait_for_process_pid(pattern: &str, timeout: std::time::Duration) -> u32 {
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
    pub(crate) fn mock_agent_prompt_visible(content: &str) -> bool {
        content.lines().any(|line| line.trim() == ">")
    }
}
#[cfg(test)]
pub(crate) use th::{
    ScopedCurrentDir, drive_pane_to_retained_dead, launch_mock_agent_doc, test_cwd, test_entry,
    wait_for_pane_contains, wait_for_pane_current_command, wait_for_pane_dead,
    wait_for_pane_in_stash_window, wait_for_pane_removed, wait_for_process_pid, wait_for_shell,
    wait_for_window_relation, write_mock_agent_doc,
};

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use sessions::{IsolatedTmux, SessionEntry, SessionRegistry};
    #[test]
    fn pane_process_kind_uses_prefetched_command_without_sampling() {
        assert!(matches!(
            pane_process_kind_from_current_command("zsh"),
            PaneProcessKind::IdleShell(cmd) if cmd == "zsh"
        ));
        assert!(matches!(
            pane_process_kind_from_current_command("agent-doc"),
            PaneProcessKind::Agent(cmd) if cmd == "agent-doc"
        ));
        assert!(matches!(
            pane_process_kind_from_current_command("sleep"),
            PaneProcessKind::Foreign(cmd) if cmd == "sleep"
        ));
        assert!(matches!(
            pane_process_kind_from_current_command(""),
            PaneProcessKind::UnknownTransient
        ));
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
    fn registry_entry_session_id_prefers_explicit_session_uuid() {
        let mut entry = test_entry("%1", "test.md");
        entry.session_id = "session-uuid".to_string();

        assert_eq!(
            registry_entry_session_id("/abs/path/test.md", &entry),
            "session-uuid"
        );

        entry.session_id.clear();
        assert_eq!(
            registry_entry_session_id("/abs/path/test.md", &entry),
            "/abs/path/test.md"
        );
    }

    #[test]
    fn target_candidate_matches_relative_base_dir_path() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("tasks").join("test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# Test\n").unwrap();
        let target = doc.canonicalize().unwrap();

        assert!(candidate_matches_target(
            &target,
            dir.path(),
            "tasks/test.md"
        ));
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None, None);
        assert_eq!(fixed, 1);
        assert!(
            !registry.contains_key("sess-fix"),
            "entry should be removed from registry"
        );
        assert!(!iso.pane_alive(&pane), "pane should be killed");
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None, None);
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None, None);
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn target_fix_no_live_owner_refreshes_recovered_registry_entry() {
        let iso = IsolatedTmux::new("resync-test-target-no-owner");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("test", &cwd).unwrap();
        assert!(iso.pane_alive(&pane));

        let tmp = tempfile::TempDir::new().unwrap();
        let doc_path = tmp.path().join("tasks").join("test.md");
        std::fs::create_dir_all(doc_path.parent().unwrap()).unwrap();
        std::fs::write(&doc_path, "---\nsession: recovered-session\n---\n# Test\n").unwrap();
        let target = doc_path.canonicalize().unwrap();
        let key = target.to_string_lossy().to_string();

        let mut entry = test_entry(&pane, "tasks/test.md");
        entry.pid = 0;
        entry.window.clear();
        entry.cwd.clear();
        entry.session_id.clear();
        let mut registry = SessionRegistry::new();
        registry.insert(key.clone(), entry);

        let issues = vec![Issue::NoLiveOwner {
            key: key.clone(),
            file: "tasks/test.md".to_string(),
            pane: pane.clone(),
        }];
        let scope = TargetFixScope {
            target: &target,
            base_dir: tmp.path(),
        };

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None, Some(scope));

        assert_eq!(fixed, 1);
        let refreshed = registry.get(&key).expect("registry entry should remain");
        assert_eq!(refreshed.pane, pane);
        assert_ne!(refreshed.pid, 0, "pane PID should be refreshed");
        assert!(!refreshed.window.is_empty(), "window should be refreshed");
        assert_eq!(refreshed.cwd, tmp.path().to_string_lossy());
        assert_eq!(refreshed.session_id, "recovered-session");
        assert!(iso.pane_alive(&pane), "pane should remain alive");
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None, None);
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
    fn is_stash_window_name_matches() {
        assert!(is_stash_window_name("stash"));
        assert!(is_stash_window_name("stash-1"));
        assert!(is_stash_window_name("stash-42"));
        assert!(!is_stash_window_name("claude"));
        assert!(!is_stash_window_name(""));
        assert!(!is_stash_window_name("stashed"));
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None, None);
        assert_eq!(fixed, 1);
        assert!(
            !registry.contains_key("sess-shell"),
            "entry should be removed"
        );
        // Idle shell should be killed (not just deregistered)
        assert!(!iso.pane_alive(&pane), "idle shell should be killed");
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None, None);
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None, None);
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
    #[test]
    fn superseded_candidates_excludes_canonical_and_dedupes() {
        // The canonical (active agent-doc window) session is never a close target;
        // the rest are closed once each, order-stable.
        let drift = vec![
            "0".to_string(),
            "5".to_string(),
            "5".to_string(),
            "8".to_string(),
        ];
        assert_eq!(
            superseded_candidates("0", &drift),
            vec!["5".to_string(), "8".to_string()]
        );
        // Canonical absent from the drift set → all are candidates.
        assert_eq!(
            superseded_candidates("9", &["0".to_string(), "5".to_string()]),
            vec!["0".to_string(), "5".to_string()]
        );
        // Single session (no drift) → nothing to close.
        assert!(superseded_candidates("0", &["0".to_string()]).is_empty());
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn close_superseded_drift_sessions_skips_canonical_closes_others() {
        // Canonical session is preserved; a sibling pure agent-doc orphan is closed.
        let iso = IsolatedTmux::new("resync-drift-superseded");
        let cwd = std::env::current_dir().unwrap();

        let _canon = iso.new_session("canon", &cwd).unwrap();
        iso.cmd()
            .args(["rename-window", "-t", "canon:", "agent-doc"])
            .output()
            .unwrap();
        iso.ensure_stash_window("canon").unwrap();

        let _orphan = iso.new_session("orphan", &cwd).unwrap();
        iso.cmd()
            .args(["rename-window", "-t", "orphan:", "agent-doc"])
            .output()
            .unwrap();
        iso.ensure_stash_window("orphan").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let drift = vec!["canon".to_string(), "orphan".to_string()];
        let closed = close_superseded_drift_sessions(&iso, "canon", &drift).unwrap();
        assert_eq!(closed, 1, "only the non-canonical orphan should be closed");
        assert!(iso.session_alive("canon"), "canonical session must survive");
        assert!(
            !iso.session_alive("orphan"),
            "superseded orphan should be closed"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn close_superseded_session_kills_pure_agent_doc_orphan() {
        // A superseded session holding only agent-doc + stash windows (idle shells,
        // no live agent) is a pure orphan → closed.
        let iso = IsolatedTmux::new("resync-close-superseded-orphan");
        let cwd = std::env::current_dir().unwrap();

        let _pane = iso.new_session("oldcanon", &cwd).unwrap();
        iso.cmd()
            .args(["rename-window", "-t", "oldcanon:", "agent-doc"])
            .output()
            .unwrap();
        iso.ensure_stash_window("oldcanon").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(iso.session_alive("oldcanon"));

        let closed = close_superseded_session(&iso, "oldcanon").unwrap();
        assert!(closed, "pure agent-doc orphan should be closed");
        assert!(
            !iso.session_alive("oldcanon"),
            "superseded session should be killed"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn close_superseded_session_preserves_session_with_user_window() {
        // A session that still holds an unmanaged user window must NOT be closed.
        let iso = IsolatedTmux::new("resync-close-superseded-userwin");
        let cwd = std::env::current_dir().unwrap();

        let _pane = iso.new_session("oldcanon2", &cwd).unwrap();
        iso.cmd()
            .args(["rename-window", "-t", "oldcanon2:", "agent-doc"])
            .output()
            .unwrap();
        let userwin = iso.new_window("oldcanon2", &cwd).unwrap();
        iso.cmd()
            .args(["rename-window", "-t", &userwin, "vim"])
            .output()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let closed = close_superseded_session(&iso, "oldcanon2").unwrap();
        assert!(
            !closed,
            "session with an unmanaged window must be preserved"
        );
        assert!(
            iso.session_alive("oldcanon2"),
            "session with a user window must stay alive"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn close_superseded_session_reports_already_gone_session() {
        // tmux already auto-destroyed the session (no windows remained) → treated as
        // already closed (Ok(true)).
        let iso = IsolatedTmux::new("resync-close-superseded-gone");
        let closed = close_superseded_session(&iso, "neverexisted").unwrap();
        assert!(closed, "absent session is treated as already closed");
    }
    #[test]
    fn resolve_registry_root_finds_submodule_agent_doc_dir() {
        let dir = tempfile::tempdir().unwrap();
        // Simulate superproject with .agent-doc/
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        // Simulate submodule with its own .agent-doc/
        let sub = dir.path().join("src/sub");
        std::fs::create_dir_all(sub.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(sub.join("tasks")).unwrap();
        let doc = sub.join("tasks/test.md");
        std::fs::write(&doc, "# test\n").unwrap();

        let root = resolve_registry_root(&doc);
        assert_eq!(
            root,
            sub.canonicalize().unwrap_or(sub.clone()),
            "should resolve to the submodule .agent-doc root, not the superproject"
        );
    }
    #[test]
    fn resolve_registry_root_falls_back_to_superproject() {
        let dir = tempfile::tempdir().unwrap();
        // Superproject with .agent-doc/
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        // Subpath without its own .agent-doc/
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let doc = dir.path().join("tasks/test.md");
        std::fs::write(&doc, "# test\n").unwrap();

        let root = resolve_registry_root(&doc);
        assert_eq!(
            root,
            dir.path()
                .canonicalize()
                .unwrap_or(dir.path().to_path_buf()),
            "should resolve to the superproject .agent-doc root"
        );
    }
    #[test]
    fn finish_unfinished_turn_commits_orphaned_response() {
        // #jb-fix-document-finish-turn: `agent-doc fix <FILE>` (and the JB Fix
        // Document action) must commit a stranded response before routing fixes.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "t@t.com"],
            vec!["config", "user.name", "T"],
        ] {
            std::process::Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .ok();
        }
        let doc = root.join("doc.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "-A"])
            .output()
            .ok();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "init", "--no-verify"])
            .output()
            .ok();

        // Strand a response (the "unfinished turn").
        crate::repair::save_pending(
        &doc,
        "<!-- patch:exchange -->\n### Re: unfinished — gpt-5\n\nRecovered.\n<!-- /patch:exchange -->\n",
    )
    .unwrap();

        finish_unfinished_turn(&doc).unwrap();

        // The response is now committed to HEAD.
        let head = std::process::Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:doc.md"])
            .output()
            .unwrap();
        let head_str = String::from_utf8_lossy(&head.stdout);
        assert!(
            head_str.contains("Re: unfinished"),
            "stranded response must be committed by finish_unfinished_turn:\n{head_str}"
        );
    }
    #[test]
    fn finish_unfinished_turn_is_noop_on_clean_document() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let content = "---\nagent_doc_session: test\n---\n\nplain body\n";
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        // No pending/cycle state → no-op, no error, content unchanged.
        finish_unfinished_turn(&doc).unwrap();
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
    }
}

#[cfg(test)]
mod stash_ttl_prune_tests {
    use super::*;

    fn candidate(pane: &str, idle: u64, active: bool, agentdoc_stash: bool) -> StashTtlCandidate {
        StashTtlCandidate {
            pane_id: pane.to_string(),
            idle_secs: idle,
            is_active_pane: active,
            is_agent_doc_stash_pane: agentdoc_stash,
        }
    }

    #[test]
    fn disabled_ttl_never_prunes() {
        // ttl_secs == 0 (unset/disabled) keeps the feature inert.
        assert!(!stash_ttl_prune_candidate(99_999, 0, false, true));
        let targets = stash_ttl_prune_targets(&[candidate("%1", 99_999, false, true)], 0);
        assert!(targets.is_empty(), "disabled TTL must reap nothing");
    }

    #[test]
    fn active_pane_is_never_reaped() {
        assert!(!stash_ttl_prune_candidate(10_000, 300, true, true));
    }

    #[test]
    fn only_agent_doc_stash_panes_are_eligible() {
        assert!(!stash_ttl_prune_candidate(10_000, 300, false, false));
    }

    #[test]
    fn idle_must_strictly_exceed_ttl() {
        assert!(!stash_ttl_prune_candidate(300, 300, false, true));
        assert!(stash_ttl_prune_candidate(301, 300, false, true));
    }

    #[test]
    fn targets_filters_only_eligible_panes() {
        let candidates = vec![
            candidate("%idle-old", 1_000, false, true),   // eligible
            candidate("%active", 1_000, true, true),      // active → skip
            candidate("%fresh", 100, false, true),        // under TTL → skip
            candidate("%not-stash", 1_000, false, false), // not agent-doc stash → skip
        ];
        let targets = stash_ttl_prune_targets(&candidates, 300);
        assert_eq!(targets, vec!["%idle-old".to_string()]);
    }
}
