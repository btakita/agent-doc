//! `agent-doc sync` — 2D layout sync: mirror a columnar editor layout in tmux.
//!
//! Usage: agent-doc sync --col plan.md,corky.md --col agent-doc.md [--window @1] [--focus plan.md]
//!
//! Each `--col` is a comma-separated list of files. Columns arrange left-to-right.
//! Within each column, files stack top-to-bottom.
//!
//! For unresolved files (no session/pane), sync auto-creates a tmux pane and
//! starts `agent-doc start <file>` in it.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::sessions::Tmux;
use crate::{frontmatter, sessions};

/// A column of files (stacked top-to-bottom).
#[derive(Debug, Clone)]
struct Column {
    files: Vec<PathBuf>,
}

/// The 2D layout: columns left-to-right, files stacked within each column.
#[derive(Debug, Clone)]
struct Layout {
    columns: Vec<Column>,
}

/// A file resolved to its tmux pane.
#[derive(Debug)]
struct ResolvedFile {
    path: PathBuf,
    pane_id: String,
}

impl Layout {
    /// Parse `--col` arguments into a Layout.
    /// Each arg is a comma-separated list of file paths.
    fn parse(col_args: &[String]) -> Result<Self> {
        let mut columns = Vec::new();
        for arg in col_args {
            let files: Vec<PathBuf> = arg
                .split(',')
                .map(|s| PathBuf::from(s.trim()))
                .filter(|p| !p.as_os_str().is_empty())
                .collect();
            if files.is_empty() {
                anyhow::bail!("empty --col argument: '{}'", arg);
            }
            columns.push(Column { files });
        }
        if columns.is_empty() {
            anyhow::bail!("at least one --col required");
        }
        Ok(Layout { columns })
    }

    /// All files in the layout, in column-major order.
    fn all_files(&self) -> Vec<&Path> {
        self.columns
            .iter()
            .flat_map(|col| col.files.iter().map(|f| f.as_path()))
            .collect()
    }
}

pub fn run(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
) -> Result<()> {
    run_with_tmux(col_args, window, focus, &Tmux::default_server())
}

pub fn run_with_tmux(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    tmux: &Tmux,
) -> Result<()> {
    let layout = Layout::parse(col_args)?;
    let all_files = layout.all_files();

    // Single file: resolve, break out unwanted panes from window, then focus.
    // Don't short-circuit — the window may have extra panes that need cleanup.
    if all_files.len() == 1 && window.is_none() {
        // Without --window, we can't do window cleanup; just focus.
        return crate::focus::run_with_tmux(all_files[0], None, tmux);
    }

    // --- Phase 1: Resolve each file to its session pane ---
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut resolved: Vec<ResolvedFile> = Vec::new();
    let mut unresolved_files: Vec<PathBuf> = Vec::new();

    for file in &all_files {
        if !file.exists() {
            eprintln!("warning: file not found: {}, will auto-create session", file.display());
            unresolved_files.push(file.to_path_buf());
            continue;
        }
        let content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let (_updated, session_id) = frontmatter::ensure_session(&content)?;
        match sessions::lookup(&session_id)? {
            Some(pane_id) if tmux.pane_alive(&pane_id) => {
                resolved.push(ResolvedFile {
                    path: file.to_path_buf(),
                    pane_id,
                });
            }
            Some(pane_id) => {
                eprintln!(
                    "warning: pane {} is dead for {}, will auto-create session",
                    pane_id,
                    file.display()
                );
                unresolved_files.push(file.to_path_buf());
            }
            None => {
                unresolved_files.push(file.to_path_buf());
            }
        }
    }

    // --- Phase 2: Auto-create sessions for unresolved files ---
    for file in &unresolved_files {
        // Ensure session UUID exists in frontmatter
        if file.exists() {
            let content = std::fs::read_to_string(file)
                .with_context(|| format!("failed to read {}", file.display()))?;
            let (updated_content, session_id) = frontmatter::ensure_session(&content)?;
            if updated_content != content {
                std::fs::write(file, &updated_content)
                    .with_context(|| format!("failed to write {}", file.display()))?;
            }

            // Create a new tmux pane
            let pane_id = tmux.auto_start("claude", &cwd)?;

            // Register session → pane
            let file_str = file.to_string_lossy();
            sessions::register(&session_id, &pane_id, &file_str)?;

            // Start agent-doc in the new pane
            let start_cmd = format!("agent-doc start {}", file.display());
            tmux.send_keys(&pane_id, &start_cmd)?;

            eprintln!(
                "Auto-created session for {} → pane {}",
                file.display(),
                pane_id
            );
            resolved.push(ResolvedFile {
                path: file.to_path_buf(),
                pane_id,
            });
        } else {
            eprintln!("warning: skipping {} — file not found", file.display());
        }
    }

    if resolved.len() < 2 {
        // Not enough panes for 2D layout, but still break out unwanted
        // panes from the target window so layout stays clean.
        if let Some(target_win) = window {
            let wanted: HashSet<String> =
                resolved.iter().map(|r| r.pane_id.clone()).collect();
            let window_panes = tmux.list_window_panes(target_win).unwrap_or_default();
            for existing_pane in &window_panes {
                if !wanted.contains(existing_pane.as_str())
                    && window_panes.len() > 1
                {
                    tmux.break_pane(existing_pane)?;
                    eprintln!(
                        "Broke out pane {} from window {}",
                        existing_pane, target_win
                    );
                }
            }
        }
        if let Some(r) = resolved.first() {
            tmux.select_pane(&r.pane_id)?;
        }
        return Ok(());
    }

    // Build a lookup from file path → pane_id
    let file_to_pane: std::collections::HashMap<PathBuf, String> = resolved
        .iter()
        .map(|r| (r.path.clone(), r.pane_id.clone()))
        .collect();

    // --- Phase 3: Build the 2D column structure with resolved panes ---
    // Each column becomes a Vec of pane_ids (maintaining order).
    let mut pane_columns: Vec<Vec<String>> = Vec::new();
    for col in &layout.columns {
        let mut panes = Vec::new();
        for file in &col.files {
            if let Some(pane_id) = file_to_pane.get(file) {
                panes.push(pane_id.clone());
            }
        }
        if !panes.is_empty() {
            pane_columns.push(panes);
        }
    }

    // Deduplicate panes across all columns
    let mut seen = HashSet::new();
    for col in &mut pane_columns {
        col.retain(|p| seen.insert(p.clone()));
    }
    pane_columns.retain(|col| !col.is_empty());

    if pane_columns.is_empty() {
        anyhow::bail!("no resolved panes to arrange");
    }
    if pane_columns.len() == 1 && pane_columns[0].len() == 1 {
        tmux.select_pane(&pane_columns[0][0])?;
        return Ok(());
    }

    // Collect the full set of wanted pane IDs
    let wanted: HashSet<&str> = pane_columns
        .iter()
        .flat_map(|col| col.iter().map(|s| s.as_str()))
        .collect();

    // --- Phase 4: Pick target window ---
    let mut best_window = String::new();
    let mut best_wanted = 0usize;
    let mut best_total = 0usize;
    for pane_id in &wanted {
        let win = tmux.pane_window(pane_id)?;
        let window_panes = tmux.list_window_panes(&win)?;
        let wanted_count = window_panes
            .iter()
            .filter(|p| wanted.contains(p.as_str()))
            .count();
        let total = window_panes.len();
        if wanted_count > best_wanted || (wanted_count == best_wanted && total > best_total) {
            best_wanted = wanted_count;
            best_total = total;
            best_window = win;
        }
    }

    // If --window is specified, override the auto-detected window — but only if alive
    let target_window = if let Some(w) = window {
        if tmux.list_window_panes(w).unwrap_or_default().is_empty() {
            eprintln!("warning: --window {} is dead, using auto-detected window", w);
            best_window
        } else {
            w.to_string()
        }
    } else {
        best_window
    };

    // The anchor is the first pane of the first column
    let anchor_pane = pane_columns[0][0].clone();

    // Flatten desired layout into ordered list for comparison
    let desired_ordered: Vec<&str> = pane_columns
        .iter()
        .flat_map(|col| col.iter().map(|s| s.as_str()))
        .collect();

    // Helper: resolve --focus to a pane ID
    let resolve_focus = |anchor: &str| -> String {
        if let Some(focus_file) = focus {
            let focus_path = PathBuf::from(focus_file);
            file_to_pane
                .get(&focus_path)
                .cloned()
                .unwrap_or_else(|| anchor.to_string())
        } else {
            anchor.to_string()
        }
    };

    // --- Convergent reconciliation loop ---
    // Re-queries tmux state each iteration. Max 3 attempts.

    for attempt in 0..3 {
        // Snapshot current window state
        let current_ordered = tmux.list_panes_ordered(&target_window).unwrap_or_default();
        let current_refs: Vec<&str> = current_ordered.iter().map(|s| s.as_str()).collect();

        // Check if layout already matches
        if current_refs == desired_ordered {
            if attempt > 0 {
                eprintln!("Layout converged on attempt {}", attempt + 1);
            }
            let focus_pane = resolve_focus(&anchor_pane);
            tmux.select_pane(&focus_pane)?;
            eprintln!("Focus: {} (layout unchanged)", focus_pane);
            return Ok(());
        }

        eprintln!(
            "Reconcile attempt {} — desired {:?}, actual {:?}",
            attempt + 1,
            desired_ordered,
            current_refs
        );

        // Step A: Break out ALL unwanted panes from target window
        // Any pane not in the desired layout gets moved out (non-destructive).
        let window_panes = tmux.list_window_panes(&target_window).unwrap_or_default();
        let mut remaining = window_panes.len();
        for pane in &window_panes {
            if !wanted.contains(pane.as_str()) && remaining > 1 {
                tmux.break_pane(pane)?;
                remaining -= 1;
                eprintln!("Broke out {} from {}", pane, target_window);
            }
        }

        // Step B: Ensure anchor is in the target window
        let anchor_win = tmux.pane_window(&anchor_pane)?;
        if anchor_win != target_window {
            let anchor_siblings = tmux.list_window_panes(&anchor_win)?;
            if anchor_siblings.len() > 1 {
                tmux.break_pane(&anchor_pane)?;
            }
            let target_panes = tmux.list_window_panes(&target_window)?;
            if let Some(existing) = target_panes.first() {
                tmux.join_pane(&anchor_pane, existing, "-bh")?;
                eprintln!("Moved anchor {} into {}", anchor_pane, target_window);
            }
        }

        // Step C: Join missing wanted panes (re-query window each time)
        // Column 0: stack below anchor
        for pane_id in &pane_columns[0][1..] {
            let in_win: HashSet<String> =
                tmux.list_window_panes(&target_window).unwrap_or_default().into_iter().collect();
            if !in_win.contains(pane_id.as_str()) {
                let pane_win = tmux.pane_window(pane_id)?;
                let siblings = tmux.list_window_panes(&pane_win)?;
                if siblings.len() > 1 {
                    tmux.break_pane(pane_id)?;
                }
                tmux.join_pane(pane_id, &anchor_pane, "-v")?;
            }
        }
        // Columns 1+: horizontal split, then vertical stacks
        for col in &pane_columns[1..] {
            let col_anchor = &col[0];
            let in_win: HashSet<String> =
                tmux.list_window_panes(&target_window).unwrap_or_default().into_iter().collect();
            if !in_win.contains(col_anchor.as_str()) {
                let pane_win = tmux.pane_window(col_anchor)?;
                let siblings = tmux.list_window_panes(&pane_win)?;
                if siblings.len() > 1 {
                    tmux.break_pane(col_anchor)?;
                }
                tmux.join_pane(col_anchor, &anchor_pane, "-h")?;
            }
            for pane_id in &col[1..] {
                let in_win: HashSet<String> =
                    tmux.list_window_panes(&target_window).unwrap_or_default().into_iter().collect();
                if !in_win.contains(pane_id.as_str()) {
                    let pane_win = tmux.pane_window(pane_id)?;
                    let siblings = tmux.list_window_panes(&pane_win)?;
                    if siblings.len() > 1 {
                        tmux.break_pane(pane_id)?;
                    }
                    tmux.join_pane(pane_id, col_anchor, "-v")?;
                }
            }
        }

        // Step D: Break out any remaining unwanted panes
        let window_panes = tmux.list_window_panes(&target_window).unwrap_or_default();
        for pane in &window_panes {
            if !wanted.contains(pane.as_str())
                && window_panes.len() > 1
            {
                tmux.break_pane(pane)?;
                eprintln!("Broke out {} from {}", pane, target_window);
            }
        }
    }

    // Final verification after all attempts
    let final_state = tmux.list_panes_ordered(&target_window).unwrap_or_default();
    let final_refs: Vec<&str> = final_state.iter().map(|s| s.as_str()).collect();
    if final_refs != desired_ordered {
        eprintln!(
            "error: layout did not converge after 3 attempts — desired {:?}, actual {:?}",
            desired_ordered, final_refs
        );
    }

    // --- Step 4: Equalize + Focus ---
    let anchor_window = tmux.pane_window(&anchor_pane)?;
    if pane_columns.len() == 2 {
        let _ = tmux.resize_pane(&pane_columns[0][0], "-x", 50);
    } else if pane_columns.len() > 2 {
        let _ = tmux.select_layout(&anchor_window, "even-horizontal");
    }
    for col in &pane_columns {
        if col.len() > 1 {
            let pct = 100 / col.len() as u32;
            let _ = tmux.resize_pane(&col[0], "-y", pct);
        }
    }

    let focus_pane = resolve_focus(&anchor_pane);
    tmux.select_pane(&focus_pane)?;

    eprintln!(
        "Sync: {} panes in {} columns",
        desired_ordered.len(),
        pane_columns.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_col() {
        let args = vec!["plan.md,corky.md".to_string()];
        let layout = Layout::parse(&args).unwrap();
        assert_eq!(layout.columns.len(), 1);
        assert_eq!(layout.columns[0].files.len(), 2);
        assert_eq!(layout.columns[0].files[0], PathBuf::from("plan.md"));
        assert_eq!(layout.columns[0].files[1], PathBuf::from("corky.md"));
    }

    #[test]
    fn parse_multiple_cols() {
        let args = vec![
            "plan.md,corky.md".to_string(),
            "agent-doc.md".to_string(),
        ];
        let layout = Layout::parse(&args).unwrap();
        assert_eq!(layout.columns.len(), 2);
        assert_eq!(layout.columns[0].files.len(), 2);
        assert_eq!(layout.columns[1].files.len(), 1);
    }

    #[test]
    fn parse_empty_col_fails() {
        let args = vec!["".to_string()];
        assert!(Layout::parse(&args).is_err());
    }

    #[test]
    fn parse_no_cols_fails() {
        let args: Vec<String> = vec![];
        assert!(Layout::parse(&args).is_err());
    }

    #[test]
    fn all_files_preserves_order() {
        let args = vec![
            "a.md,b.md".to_string(),
            "c.md".to_string(),
        ];
        let layout = Layout::parse(&args).unwrap();
        let files = layout.all_files();
        assert_eq!(files.len(), 3);
        assert_eq!(files[0], Path::new("a.md"));
        assert_eq!(files[1], Path::new("b.md"));
        assert_eq!(files[2], Path::new("c.md"));
    }

    #[test]
    fn parse_trims_whitespace() {
        let args = vec!["plan.md , corky.md".to_string()];
        let layout = Layout::parse(&args).unwrap();
        assert_eq!(layout.columns[0].files[0], PathBuf::from("plan.md"));
        assert_eq!(layout.columns[0].files[1], PathBuf::from("corky.md"));
    }
}
