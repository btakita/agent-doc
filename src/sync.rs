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

    // Degenerate: single file → just focus it
    if all_files.len() == 1 {
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
        // Not enough panes to arrange — just focus what we have
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

    // If --window is specified, override the auto-detected window
    let target_window = window.map(|w| w.to_string()).unwrap_or(best_window);

    // The anchor is the first pane of the first column
    let anchor_pane = pane_columns[0][0].clone();

    // --- Phase 5: Break out unwanted session panes from target window ---
    let registry = sessions::load().unwrap_or_default();
    let session_panes: HashSet<String> = registry.values().map(|e| e.pane.clone()).collect();

    let window_panes = tmux.list_window_panes(&target_window).unwrap_or_default();
    for existing_pane in &window_panes {
        if !wanted.contains(existing_pane.as_str())
            && session_panes.contains(existing_pane)
            && window_panes.len() > 1
        {
            tmux.break_pane(existing_pane)?;
            eprintln!(
                "Broke out pane {} from window {}",
                existing_pane, target_window
            );
        }
    }

    // --- Phase 6: Break all wanted panes (except anchor) to separate windows ---
    // This gives us a clean slate for the 2D join algorithm.
    for col in &pane_columns {
        for pane_id in col {
            if *pane_id == anchor_pane {
                continue;
            }
            let pane_win = tmux.pane_window(pane_id)?;
            let win_panes = tmux.list_window_panes(&pane_win)?;
            if win_panes.len() > 1 {
                tmux.break_pane(pane_id)?;
            }
        }
    }

    // --- Phase 7: 2D join algorithm ---
    // Column 0: anchor stays. Join remaining files with -v (vertical stack).
    for pane_id in &pane_columns[0][1..] {
        tmux.join_pane(pane_id, &anchor_pane, "-v")?;
        eprintln!("Joined pane {} below anchor (col 0, stack)", pane_id);
    }

    // Columns 1+: join first file with -h (creates new column right of anchor).
    // Then join remaining files with -v (stack within column).
    for (col_idx, col) in pane_columns[1..].iter().enumerate() {
        // First pane of this column joins horizontally to the anchor
        let col_anchor = &col[0];
        tmux.join_pane(col_anchor, &anchor_pane, "-h")?;
        eprintln!(
            "Joined pane {} to right of anchor (col {})",
            col_anchor,
            col_idx + 1
        );

        // Remaining panes stack vertically within this column
        for pane_id in &col[1..] {
            tmux.join_pane(pane_id, col_anchor, "-v")?;
            eprintln!(
                "Joined pane {} below {} (col {}, stack)",
                pane_id,
                col_anchor,
                col_idx + 1
            );
        }
    }

    // --- Phase 8: Focus ---
    let focus_pane = if let Some(focus_file) = focus {
        let focus_path = PathBuf::from(focus_file);
        file_to_pane
            .get(&focus_path)
            .cloned()
            .unwrap_or_else(|| anchor_pane.clone())
    } else {
        anchor_pane.clone()
    };
    tmux.select_pane(&focus_pane)?;

    let total_panes: usize = pane_columns.iter().map(|c| c.len()).sum();
    eprintln!(
        "Sync: {} panes in {} columns",
        total_panes,
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
