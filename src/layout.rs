//! `agent-doc layout` — Arrange tmux panes to mirror editor split layout.
//!
//! Usage: agent-doc layout <file1.md> <file2.md> [--split h|v]
//!
//! Creates a "mirror window" in tmux where panes are arranged to match the
//! editor's split layout. Uses `join-pane` to move Claude sessions into the
//! mirror window and `break-pane` to disassemble when layout changes.
//!
//! The mirror window is tracked in sessions.json so subsequent layout calls
//! can update it rather than creating duplicates.

use anyhow::{Context, Result};
use std::path::Path;

use crate::sessions::Tmux;
use crate::{frontmatter, sessions};

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

pub fn run_with_tmux(files: &[&Path], split: Split, pane: Option<&str>, window: Option<&str>, tmux: &Tmux) -> Result<()> {
    if files.is_empty() {
        anyhow::bail!("at least one file required");
    }

    if files.len() == 1 {
        // Single file — just focus it, no layout needed.
        return crate::focus::run_with_tmux(files[0], pane, tmux);
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
        let pane = sessions::lookup(&session_id)?;
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
    let registry = sessions::load().unwrap_or_default();
    let session_panes: std::collections::HashSet<String> =
        registry.values().map(|e| e.pane.clone()).collect();

    let window_panes = tmux.list_window_panes(&target_window)?;
    for existing_pane in &window_panes {
        if !wanted.contains(existing_pane.as_str())
            && session_panes.contains(existing_pane)
            && window_panes.len() > 1
        {
            tmux.break_pane(existing_pane)?;
            eprintln!("Broke out pane {} from window {}", existing_pane, target_window);
        }
    }

    // Join remaining panes into the target window with the requested split.
    for (pane_id, file_display) in &pane_files {
        let pane_window = tmux.pane_window(pane_id)?;
        if pane_window == target_window {
            continue;
        }

        tmux.join_pane(pane_id, &anchor_pane, split.tmux_flag())?;
        eprintln!("Joined {} (pane {}) into window {}", file_display, pane_id, target_window);
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
