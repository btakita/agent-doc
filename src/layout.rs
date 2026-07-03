//! # Module: layout
//!
//! Arrange tmux panes to mirror the editor's split layout.
//!
//! Usage: `agent-doc layout <file1.md> <file2.md> [--split h|v]`
//!
//! ## Spec
//! - `Split` enum: `Horizontal` (side-by-side, `-h`) or `Vertical` (stacked, `-v`).
//! - `run(files, split, pane, window)`: entry point; delegates to `run_with_tmux` using
//!   the default tmux server.
//! - `run_with_tmux(files, split, pane, window, tmux)`:
//!   - Requires at least one file; errors if `files` is empty.
//!   - With exactly one file: delegates entirely to `agent-doc-focus-io` and returns.
//!   - For each file: reads frontmatter to obtain the session UUID via
//!     `frontmatter::ensure_session`, then looks up the live pane in `sessions.json`.
//!     Files with no registered pane or a dead pane are skipped with a stderr warning.
//!   - When `--window` is supplied, discards any resolved pane that does not belong to
//!     that window (prevents cross-window pane migration).
//!   - If fewer than two live, unique panes remain after filtering, focuses the first
//!     file's pane if it was resolved; returns `Ok(())` without rearranging.
//!   - Deduplicates panes so multiple files sharing a pane count as one; errors if
//!     deduplication leaves fewer than two panes.
//!   - Selects the target window by choosing the window that already contains the most
//!     wanted panes (tiebreaker: most total panes), minimising disruption to any
//!     existing layout.
//!   - Breaks out session-registered panes that are in the target window but not in the
//!     wanted set (`tmux break-pane`); non-session panes (shells, tools) are untouched.
//!   - Joins each wanted pane that is outside the target window into it via `join-pane`
//!     with the `Split` flag.
//!   - Focuses the first file's pane after the layout is complete.
//!
//! ## Agentic Contracts
//! - Only panes that are registered in `sessions.json` are ever broken out of the target
//!   window; unmanaged panes are never touched.
//! - A single-file invocation never modifies tmux window structure; it is a pure focus
//!   operation.
//! - The `--window` filter is a hard boundary: panes outside the specified window are
//!   silently excluded rather than migrated into it.
//! - After a successful multi-pane layout, the first file's pane is always focused.
//! - `run_with_tmux` does not modify `sessions.json`; session registry updates are the
//!   responsibility of `claim.rs` / `route.rs`.
//!
//! ## Evals
//! - `layout_two_files_horizontal` (aspirational): two files each with a live pane in
//!   different windows → both panes joined into one window side-by-side and first pane
//!   focused.
//! - `layout_single_file_delegates_to_focus` (aspirational): one file supplied → focus
//!   is called, no `join-pane` or `break-pane` issued.
//! - `layout_skips_dead_pane` (aspirational): one of two files has a dead pane → dead
//!   pane is skipped with a warning; if only one live pane remains, focus is called
//!   instead of rearranging.
//! - `layout_window_filter_excludes_foreign_panes` (aspirational): `--window` supplied
//!   and one resolved pane is in a different window → that pane is filtered out before
//!   arrangement.
//! - `layout_does_not_break_nonregistered_panes` (aspirational): target window contains
//!   a shell pane not in `sessions.json` alongside a registered pane to be broken out →
//!   only the registered pane is broken out; the shell pane remains.
//! - `layout_empty_files_errors` (aspirational): `files` is empty → `anyhow::bail!`
//!   with "at least one file required".

use anyhow::{Context, Result};
use std::path::Path;

use agent_doc_frontmatter::frontmatter;
use tmux_router::{PaneMoveOp, Tmux};

/// Split direction for the mirror window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Split {
    /// Horizontal split (panes side by side).
    Horizontal,
    /// Vertical split (panes stacked).
    Vertical,
}

impl Split {
    fn tmux_flag(&self) -> &str {
        match self {
            Split::Horizontal => "-h",
            Split::Vertical => "-v",
        }
    }
}

pub fn run(files: &[&Path], split: Split, pane: Option<&str>, window: Option<&str>) -> Result<()> {
    run_with_tmux(files, split, pane, window, &Tmux::default_server())
}

pub fn run_with_tmux(
    files: &[&Path],
    split: Split,
    pane: Option<&str>,
    window: Option<&str>,
    tmux: &Tmux,
) -> Result<()> {
    tracing::debug!(file_count = files.len(), split = ?split, window, "layout::run start");
    if files.is_empty() {
        anyhow::bail!("at least one file required");
    }

    if files.len() == 1 {
        // Single file — just focus it, no layout needed.
        return agent_doc_focus_io::run_with_tmux(
            &crate::focus_effects::FOCUS_EFFECTS,
            files[0],
            pane,
            tmux,
        );
    }

    // Resolve each file to its session pane.
    let mut pane_files: Vec<(String, String)> = Vec::new(); // (pane_id, file_display)
    for file in files {
        if !file.exists() {
            anyhow::bail!("file not found: {}", file.display());
        }
        let content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let (_updated, session_id) = frontmatter::ensure_session(&content)?;
        let pane = agent_doc_session_registry_io::lookup(&session_id)?;
        match pane {
            Some(pane_id) if tmux.pane_alive(&pane_id) => {
                pane_files.push((pane_id, file.display().to_string()));
            }
            Some(pane_id) => {
                eprintln!(
                    "warning: pane {} is dead for {}, skipping",
                    pane_id,
                    file.display()
                );
            }
            None => {
                eprintln!(
                    "warning: no pane registered for {}, skipping",
                    file.display()
                );
            }
        }
    }

    // If --window is specified, filter to only panes in that window.
    // This prevents layout from pulling panes from other windows.
    if let Some(win) = window {
        let window_panes_list = tmux.list_window_panes(win).unwrap_or_default();
        let window_pane_set: std::collections::HashSet<&str> =
            window_panes_list.iter().map(|s| s.as_str()).collect();
        let before = pane_files.len();
        pane_files.retain(|(pane_id, _)| window_pane_set.contains(pane_id.as_str()));
        if pane_files.len() < before {
            eprintln!(
                "Filtered {} panes outside window {}",
                before - pane_files.len(),
                win
            );
        }
    }

    if pane_files.len() < 2 {
        // Only focus the most recently selected file's pane (files[0]).
        // If that file has no pane, don't change focus at all — the user
        // selected an unclaimed file, so switching to a different pane
        // would be confusing.
        if let Some(first_file) = files.first() {
            let first_display = first_file.display().to_string();
            for (pane_id, display) in &pane_files {
                if *display == first_display {
                    tmux.select_pane(pane_id)?;
                    break;
                }
            }
        }
        return Ok(());
    }

    // Deduplicate panes (multiple files might share a pane).
    let mut seen = std::collections::HashSet::new();
    pane_files.retain(|(pane_id, _)| seen.insert(pane_id.clone()));

    if pane_files.len() < 2 {
        anyhow::bail!("all files share the same pane — nothing to arrange");
    }

    // Collect the set of wanted pane IDs.
    let wanted: std::collections::HashSet<&str> =
        pane_files.iter().map(|(id, _)| id.as_str()).collect();

    // Pick the target window — the one containing the most wanted panes.
    // Tiebreaker: prefer the window with the most total panes (the existing
    // layout window). This keeps the current layout in place and swaps panes
    // in/out, rather than moving everything to a solo pane's window.
    let mut best_window = String::new();
    let mut best_wanted = 0usize;
    let mut best_total = 0usize;
    let mut anchor_pane = pane_files[0].0.clone(); // fallback
    for (pane_id, _) in &pane_files {
        let window = tmux.pane_window(pane_id)?;
        let window_panes = tmux.list_window_panes(&window)?;
        let wanted_count = window_panes
            .iter()
            .filter(|p| wanted.contains(p.as_str()))
            .count();
        let total = window_panes.len();
        if wanted_count > best_wanted || (wanted_count == best_wanted && total > best_total) {
            best_wanted = wanted_count;
            best_total = total;
            best_window = window;
            anchor_pane = pane_id.clone();
        }
    }
    let target_window = best_window;

    // Break out unwanted panes, but only if they are registered sessions.
    // Non-session panes (shells, tools, etc.) are left in place — the user
    // didn't ask us to manage them.
    let registry = agent_doc_session_registry_io::load().unwrap_or_default();
    let session_panes: std::collections::HashSet<String> =
        registry.values().map(|e| e.pane.clone()).collect();

    let window_panes = tmux.list_window_panes(&target_window)?;
    for existing_pane in &window_panes {
        if !wanted.contains(existing_pane.as_str())
            && session_panes.contains(existing_pane)
            && window_panes.len() > 1
        {
            // Skip busy panes (running agent-doc/claude sessions)
            if is_pane_busy(tmux, existing_pane) {
                eprintln!(
                    "Skipped busy pane {} in window {}",
                    existing_pane, target_window
                );
                continue;
            }
            tmux.break_pane(existing_pane)?;
            eprintln!(
                "Broke out pane {} from window {}",
                existing_pane, target_window
            );
        }
    }

    // Join remaining panes into the target window with the requested split.
    for (pane_id, file_display) in &pane_files {
        let pane_window = tmux.pane_window(pane_id)?;
        if pane_window == target_window {
            continue;
        }

        PaneMoveOp::new(tmux, pane_id, &anchor_pane).join(split.tmux_flag())?;
        eprintln!(
            "Joined {} (pane {}) into window {}",
            file_display, pane_id, target_window
        );
    }

    // Focus the first file's pane (the most recently selected file from the plugin).
    let (focus_pane, _) = &pane_files[0];
    tmux.select_pane(focus_pane)?;

    eprintln!(
        "Layout: {} panes arranged {}",
        pane_files.len(),
        match split {
            Split::Horizontal => "side-by-side",
            Split::Vertical => "stacked",
        }
    );
    Ok(())
}

/// Check if a tmux pane is running an active agent-doc or claude session.
fn is_pane_busy(tmux: &Tmux, pane_id: &str) -> bool {
    let output = tmux
        .cmd()
        .args(["display-message", "-t", pane_id, "-p", "#{pane_pid}"])
        .output();
    let pid_str = match output {
        Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => return false,
    };
    if pid_str.is_empty() {
        return false;
    }
    if pid_is_agent_session(&pid_str) {
        return true;
    }
    // Check child processes (pane PID is usually a shell)
    if let Ok(children) = std::process::Command::new("pgrep")
        .args(["-P", &pid_str])
        .output()
    {
        for child_pid in String::from_utf8_lossy(&children.stdout).lines() {
            let child_pid = child_pid.trim();
            if !child_pid.is_empty() && pid_is_agent_session(child_pid) {
                return true;
            }
        }
    }
    false
}

fn pid_is_agent_session(pid: &str) -> bool {
    let output = match std::process::Command::new("ps")
        .args(["-p", pid, "-o", "command="])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };
    let cmdline = String::from_utf8_lossy(&output.stdout);
    cmdline.contains("agent-doc") || cmdline.contains("claude")
}
