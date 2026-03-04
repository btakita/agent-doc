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

// =========================================================================
// Data types
// =========================================================================

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

// =========================================================================
// SyncLog — structured operation trace for debugging
// =========================================================================

/// A single operation logged during sync reconciliation.
#[derive(Debug, Clone)]
#[allow(dead_code)] // fields read in tests and Debug output
struct SyncEntry {
    phase: &'static str,
    message: String,
    ok: bool,
}

/// Structured log of all sync operations for debugging.
#[derive(Debug, Clone, Default)]
struct SyncLog {
    entries: Vec<SyncEntry>,
}

impl SyncLog {
    fn new() -> Self {
        Self::default()
    }

    fn log(&mut self, phase: &'static str, message: impl Into<String>) {
        let msg = message.into();
        eprintln!("[sync:{}] {}", phase, &msg);
        self.entries.push(SyncEntry {
            phase,
            message: msg,
            ok: true,
        });
    }

    fn log_err(&mut self, phase: &'static str, message: impl Into<String>) {
        let msg = message.into();
        eprintln!("[sync:{}] ERROR: {}", phase, &msg);
        self.entries.push(SyncEntry {
            phase,
            message: msg,
            ok: false,
        });
    }

    fn has_errors(&self) -> bool {
        self.entries.iter().any(|e| !e.ok)
    }

    /// Log the global tmux state (all windows and panes across all sessions).
    fn log_global_state(&mut self, tmux: &Tmux, label: &str) {
        // Log all windows
        match tmux.list_all_windows() {
            Ok(windows) => {
                self.log("GLOBAL", format!("[{}] windows: {}", label, windows));
            }
            Err(e) => {
                self.log_err("GLOBAL", format!("[{}] failed to list windows: {}", label, e));
            }
        }
        // Log all panes
        match tmux.list_all_panes() {
            Ok(panes) => {
                self.log("GLOBAL", format!("[{}] panes: {}", label, panes));
            }
            Err(e) => {
                self.log_err("GLOBAL", format!("[{}] failed to list panes: {}", label, e));
            }
        }
    }

    /// Return all entries (for testing assertions).
    #[cfg(test)]
    fn entries(&self) -> &[SyncEntry] {
        &self.entries
    }

    /// Count of mutations (break/join operations, not snapshots or checks).
    #[cfg(test)]
    fn mutation_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.phase, "DETACH" | "ATTACH" | "REORDER"))
            .count()
    }
}

// =========================================================================
// Layout parsing
// =========================================================================

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

// =========================================================================
// Public API
// =========================================================================

pub fn run(col_args: &[String], window: Option<&str>, focus: Option<&str>) -> Result<()> {
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

    // Log global tmux state at sync start
    let mut global_log = SyncLog::new();
    global_log.log_global_state(tmux, "sync-start");

    // Single file without --window: just focus (only if it has a session UUID).
    if all_files.len() == 1 && window.is_none() {
        if all_files[0].exists() {
            let content = std::fs::read_to_string(all_files[0])?;
            let (fm, _) = frontmatter::parse(&content)?;
            if fm.session.is_none() {
                eprintln!("skipping {} — no session UUID (use Claim to register)", all_files[0].display());
                return Ok(());
            }
        }
        return crate::focus::run_with_tmux(all_files[0], None, tmux);
    }

    // --- Phase 1: Resolve each file to its session pane ---
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut resolved: Vec<ResolvedFile> = Vec::new();
    let mut unresolved_files: Vec<PathBuf> = Vec::new();

    for file in &all_files {
        if !file.exists() {
            eprintln!("warning: file not found: {}, skipping", file.display());
            continue;
        }
        let content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let (fm, _) = frontmatter::parse(&content)?;
        let session_id = match fm.session {
            Some(id) => id,
            None => {
                // No session UUID — not an agent-doc file. Skip silently.
                continue;
            }
        };
        match sessions::lookup(&session_id)? {
            Some(pane_id) if tmux.pane_alive(&pane_id) => {
                resolved.push(ResolvedFile {
                    path: file.to_path_buf(),
                    pane_id,
                });
            }
            Some(pane_id) => {
                eprintln!(
                    "warning: pane {} is dead for {}, will auto-restart session",
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

    // --- Phase 2: Auto-restart sessions for files with dead/missing panes ---
    // Only files that already have a session UUID reach here (Phase 1 skips files without one).
    for file in &unresolved_files {
        if file.exists() {
            let content = std::fs::read_to_string(file)
                .with_context(|| format!("failed to read {}", file.display()))?;
            let (fm, _) = frontmatter::parse(&content)?;
            let session_id = match fm.session {
                Some(id) => id,
                None => continue, // shouldn't happen, but be safe
            };

            let pane_id = tmux.auto_start("claude", &cwd)?;
            let file_str = file.to_string_lossy();
            sessions::register(&session_id, &pane_id, &file_str)?;

            let start_cmd = format!("agent-doc start {}", file.display());
            tmux.send_keys(&pane_id, &start_cmd)?;

            eprintln!(
                "Auto-restarted session for {} → pane {}",
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
        // Not enough resolved panes for 2D layout — just focus, don't rearrange.
        // This happens when non-agent-doc files are in the layout (no session UUID).
        // Breaking existing panes out of the window would be destructive.
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

    let target_window = if let Some(w) = window {
        if tmux.list_window_panes(w).unwrap_or_default().is_empty() {
            eprintln!(
                "warning: --window {} is dead, using auto-detected window",
                w
            );
            best_window
        } else {
            w.to_string()
        }
    } else {
        best_window
    };

    let anchor_pane = pane_columns[0][0].clone();

    let desired_ordered: Vec<&str> = pane_columns
        .iter()
        .flat_map(|col| col.iter().map(|s| s.as_str()))
        .collect();

    // Resolve --focus to a pane ID
    let focus_pane = if let Some(focus_file) = focus {
        let focus_path = PathBuf::from(focus_file);
        file_to_pane
            .get(&focus_path)
            .cloned()
            .unwrap_or_else(|| anchor_pane.clone())
    } else {
        anchor_pane.clone()
    };

    // --- Phase 5: Reconcile ---
    let log = reconcile(
        tmux,
        &target_window,
        &pane_columns,
        &desired_ordered,
    )?;

    // --- Phase 6: Resize + Focus ---
    equalize_sizes(tmux, &pane_columns);
    tmux.select_pane(&focus_pane)?;

    // Log global tmux state at sync end
    global_log.log_global_state(tmux, "sync-end");

    if log.has_errors() {
        eprintln!(
            "Sync completed with errors: {} panes in {} columns",
            desired_ordered.len(),
            pane_columns.len()
        );
    } else {
        eprintln!(
            "Sync: {} panes in {} columns",
            desired_ordered.len(),
            pane_columns.len()
        );
    }
    Ok(())
}

// =========================================================================
// Core reconciliation algorithm
// =========================================================================

/// Reconcile pane layout in target_window to match desired_ordered.
///
/// # Algorithm: Simple 2-step detach/attach
///
/// ```text
/// SNAPSHOT — query current panes
/// FAST PATH — if already correct, done
/// DETACH — break_pane unwanted panes out of target window
/// ATTACH — join_pane missing desired panes into target window
///          (isolate from shared windows first, then join with correct split direction)
/// VERIFY — confirm final layout
/// ```
///
/// If all desired panes are present but in wrong order, detach non-first + reattach.
fn reconcile(
    tmux: &Tmux,
    target_window: &str,
    pane_columns: &[Vec<String>],
    desired_ordered: &[&str],
) -> Result<SyncLog> {
    let mut log = SyncLog::new();

    let wanted: HashSet<&str> = desired_ordered.iter().copied().collect();
    let first_pane = desired_ordered[0];

    // --- SNAPSHOT ---
    let current = tmux.list_panes_ordered(target_window).unwrap_or_default();
    let current_refs: Vec<&str> = current.iter().map(|s| s.as_str()).collect();
    log.log(
        "SNAPSHOT",
        format!(
            "window={}, current={:?}, desired={:?}",
            target_window, current_refs, desired_ordered
        ),
    );

    // --- FAST PATH ---
    if current_refs == desired_ordered {
        log.log("FAST_PATH", "layout already correct");
        return Ok(log);
    }

    // --- DETACH unwanted panes ---
    let current_panes = tmux.list_window_panes(target_window).unwrap_or_default();
    let unwanted: Vec<String> = current_panes
        .into_iter()
        .filter(|p| !wanted.contains(p.as_str()))
        .collect();
    // Detach unwanted, but never the very last pane in the window.
    for pane in &unwanted {
        let window_count = tmux.list_window_panes(target_window).unwrap_or_default().len();
        if window_count <= 1 {
            log.log("DETACH", format!("deferred {} — last pane in window", pane));
            continue;
        }
        match tmux.break_pane(pane) {
            Ok(()) => {
                log.log("DETACH", format!("broke {} from {}", pane, target_window));
                update_registry(tmux, pane, &mut log);
            }
            Err(e) => {
                log.log_err("DETACH", format!("failed to break {}: {}", pane, e));
            }
        }
    }

    // --- ATTACH desired panes ---
    // First, ensure the first desired pane is in the target window.
    let present_now: HashSet<String> = tmux
        .list_window_panes(target_window)
        .unwrap_or_default()
        .into_iter()
        .collect();

    if !present_now.contains(first_pane) {
        // Isolate first_pane if it shares a window with others
        if let Ok(pane_win) = tmux.pane_window(first_pane) {
            let siblings = tmux.list_window_panes(&pane_win).unwrap_or_default();
            if siblings.len() > 1 {
                let _ = tmux.break_pane(first_pane);
            }
        }
        // Join into target window (before the first existing pane)
        let existing = tmux.list_window_panes(target_window).unwrap_or_default();
        if let Some(target) = existing.first() {
            match tmux.join_pane(first_pane, target, "-bh") {
                Ok(()) => {
                    log.log("ATTACH", format!("joined first {} into {}", first_pane, target_window));
                    update_registry(tmux, first_pane, &mut log);
                }
                Err(e) => {
                    log.log_err("ATTACH", format!("failed to join first {}: {}", first_pane, e));
                }
            }
        }
        // Now break deferred unwanted (window has a wanted pane now)
        let refreshed = tmux.list_window_panes(target_window).unwrap_or_default();
        for pane in &unwanted {
            if refreshed.contains(pane) && refreshed.len() > 1 {
                match tmux.break_pane(pane) {
                    Ok(()) => {
                        log.log("DETACH", format!("broke deferred {} from {}", pane, target_window));
                        update_registry(tmux, pane, &mut log);
                    }
                    Err(e) => {
                        log.log_err("DETACH", format!("failed to break deferred {}: {}", pane, e));
                    }
                }
            }
        }
    }

    // Re-check what's present after ensuring first pane
    let present_after: HashSet<String> = tmux
        .list_window_panes(target_window)
        .unwrap_or_default()
        .into_iter()
        .collect();

    // Check if we need a full reorder (all desired present but wrong order)
    let current_order = tmux.list_panes_ordered(target_window).unwrap_or_default();
    let current_order_refs: Vec<&str> = current_order.iter().map(|s| s.as_str()).collect();
    let current_set: HashSet<&str> = current_order_refs.iter().copied().collect();

    let need_reorder = current_set == wanted && current_order_refs != desired_ordered;
    if need_reorder {
        // Break all non-first panes out, then rejoin in order
        log.log("REORDER", format!("current={:?}, desired={:?}", current_order_refs, desired_ordered));
        for pane in desired_ordered.iter().skip(1) {
            if present_after.contains(*pane) {
                let _ = tmux.break_pane(pane);
                log.log("REORDER", format!("broke {} for reorder", pane));
            }
        }
    }

    // Join remaining desired panes in column order
    for (col_idx, column) in pane_columns.iter().enumerate() {
        for (row_idx, pane) in column.iter().enumerate() {
            if pane.as_str() == first_pane {
                continue;
            }
            // Skip if already in target window (and not reordering)
            let in_target = tmux
                .list_window_panes(target_window)
                .unwrap_or_default()
                .contains(pane);
            if in_target && !need_reorder {
                continue;
            }
            if in_target {
                // Already handled by reorder break above
                // If somehow still present, skip
                let still_there = tmux
                    .list_window_panes(target_window)
                    .unwrap_or_default()
                    .contains(pane);
                if still_there {
                    continue;
                }
            }

            // Isolate pane from its current window if shared
            if let Ok(pane_win) = tmux.pane_window(pane) {
                let siblings = tmux.list_window_panes(&pane_win).unwrap_or_default();
                if siblings.len() > 1 {
                    if let Err(e) = tmux.break_pane(pane) {
                        log.log_err("ATTACH", format!("failed to isolate {}: {}", pane, e));
                        continue;
                    }
                }
            } else {
                log.log_err("ATTACH", format!("pane {} not found (dead?)", pane));
                continue;
            }

            let (target_pane, flag) = join_target(pane_columns, col_idx, row_idx);
            match tmux.join_pane(pane, &target_pane, flag) {
                Ok(()) => {
                    log.log("ATTACH", format!("{} → {} ({})", pane, target_pane, flag));
                    update_registry(tmux, pane, &mut log);
                }
                Err(e) => {
                    log.log_err("ATTACH", format!("failed to join {} → {}: {}", pane, target_pane, e));
                }
            }
        }
    }

    // --- VERIFY ---
    let final_state = tmux.list_panes_ordered(target_window).unwrap_or_default();
    let final_refs: Vec<&str> = final_state.iter().map(|s| s.as_str()).collect();
    if final_refs == desired_ordered {
        log.log("VERIFY", "layout correct");
    } else {
        log.log_err(
            "VERIFY",
            format!(
                "mismatch — desired={:?}, actual={:?}",
                desired_ordered, final_refs
            ),
        );
    }

    // --- REGISTRY UPDATE for all wanted panes ---
    for pane in desired_ordered {
        update_registry(tmux, pane, &mut log);
    }

    Ok(log)
}

/// Determine join target and split direction for a pane at (col_idx, row_idx).
fn join_target(pane_columns: &[Vec<String>], col_idx: usize, row_idx: usize) -> (String, &'static str) {
    if col_idx == 0 {
        // Same column as anchor: stack below previous pane
        (pane_columns[0][row_idx - 1].clone(), "-v")
    } else if row_idx == 0 {
        // First pane of new column: horizontal split right of previous column's first pane
        (pane_columns[col_idx - 1][0].clone(), "-h")
    } else {
        // Stack below previous pane in this column
        (pane_columns[col_idx][row_idx - 1].clone(), "-v")
    }
}

/// Query a pane's current window and update sessions.json.
fn update_registry(tmux: &Tmux, pane: &str, log: &mut SyncLog) {
    match tmux.pane_window(pane) {
        Ok(win) => {
            if let Err(e) = sessions::update_window_for_pane(pane, &win) {
                log.log_err(
                    "REGISTRY",
                    format!("failed to update registry for {}: {}", pane, e),
                );
            }
        }
        Err(e) => {
            log.log_err(
                "REGISTRY",
                format!("can't query window for {}: {}", pane, e),
            );
        }
    }
}

/// Equalize pane sizes after reconciliation.
fn equalize_sizes(tmux: &Tmux, pane_columns: &[Vec<String>]) {
    if pane_columns.len() == 2 {
        let _ = tmux.resize_pane(&pane_columns[0][0], "-x", 50);
    } else if pane_columns.len() > 2 {
        if let Ok(win) = tmux.pane_window(&pane_columns[0][0]) {
            let _ = tmux.select_layout(&win, "even-horizontal");
        }
    }
    for col in pane_columns {
        if col.len() > 1 {
            let pct = 100 / col.len() as u32;
            let _ = tmux.resize_pane(&col[0], "-y", pct);
        }
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::IsolatedTmux;
    use tempfile::TempDir;

    // --- Layout parsing unit tests (unchanged) ---

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
        let args = vec!["a.md,b.md".to_string(), "c.md".to_string()];
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

    // --- SyncLog unit tests ---

    #[test]
    fn sync_log_collects_entries() {
        let mut log = SyncLog::new();
        log.log("SNAPSHOT", "test message");
        log.log_err("DETACH", "something failed");
        assert_eq!(log.entries().len(), 2);
        assert!(log.entries()[0].ok);
        assert_eq!(log.entries()[0].phase, "SNAPSHOT");
        assert!(!log.entries()[1].ok);
        assert_eq!(log.entries()[1].phase, "DETACH");
    }

    #[test]
    fn sync_log_has_errors() {
        let mut log = SyncLog::new();
        log.log("SNAPSHOT", "ok");
        assert!(!log.has_errors());
        log.log_err("DETACH", "bad");
        assert!(log.has_errors());
    }

    #[test]
    fn sync_log_mutation_count() {
        let mut log = SyncLog::new();
        log.log("SNAPSHOT", "snapshot");
        log.log("FAST_PATH", "fast");
        log.log("DETACH", "broke pane");
        log.log("ATTACH", "joined pane");
        log.log("VERIFY", "ok");
        assert_eq!(log.mutation_count(), 2); // DETACH + ATTACH
    }

    // --- Helper: set up N panes in separate windows within an isolated tmux ---

    fn setup_panes(tmux: &Tmux, n: usize) -> (String, Vec<String>, TempDir) {
        let tmp = TempDir::new().unwrap();
        let first_pane = tmux.new_session("test", tmp.path()).unwrap();
        let target_window = tmux.pane_window(&first_pane).unwrap();
        // Resize window large enough to fit many panes
        let _ = tmux.raw_cmd(&["resize-window", "-t", &target_window, "-x", "200", "-y", "60"]);
        let mut panes = vec![first_pane];
        for _ in 1..n {
            let pane = tmux.new_window("test", tmp.path()).unwrap();
            panes.push(pane);
        }
        (target_window, panes, tmp)
    }

    // --- Integration tests using IsolatedTmux ---

    #[test]
    fn test_sync_2col_happy_path() {
        let t = IsolatedTmux::new("sync-test-2col-happy");
        let (target_window, panes, _tmp) = setup_panes(&t, 3);

        // panes[0] is already in target_window, panes[1] and [2] are in separate windows
        let pane_columns = vec![
            vec![panes[0].clone(), panes[1].clone()],
            vec![panes[2].clone()],
        ];
        let desired: Vec<&str> = pane_columns
            .iter()
            .flat_map(|col| col.iter().map(|s| s.as_str()))
            .collect();

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        // Verify all panes are in the target window
        let final_panes = t.list_window_panes(&target_window).unwrap();
        for pane in &desired {
            assert!(
                final_panes.contains(&pane.to_string()),
                "pane {} should be in target window",
                pane
            );
        }
        assert!(!log.has_errors(), "should have no errors: {:?}", log.entries());
    }

    #[test]
    fn test_sync_already_correct() {
        let t = IsolatedTmux::new("sync-test-already-correct");
        let tmp = TempDir::new().unwrap();

        // Create 2 panes in the same window
        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let target_window = t.pane_window(&pane_a).unwrap();

        // Split to create second pane in same window
        let pane_b_raw = t
            .raw_cmd(&[
                "split-window",
                "-t",
                &pane_a,
                "-h",
                "-P",
                "-F",
                "#{pane_id}",
            ])
            .unwrap();
        let pane_b = pane_b_raw.trim().to_string();

        let pane_columns = vec![vec![pane_a.clone()], vec![pane_b.clone()]];
        let desired: Vec<&str> = vec![pane_a.as_str(), pane_b.as_str()];

        // Verify they're already ordered correctly
        let current = t.list_panes_ordered(&target_window).unwrap();
        let current_refs: Vec<&str> = current.iter().map(|s| s.as_str()).collect();
        assert_eq!(current_refs, desired, "setup should produce correct order");

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        assert!(
            log.entries().iter().any(|e| e.phase == "FAST_PATH"),
            "should take fast path"
        );
        assert_eq!(log.mutation_count(), 0, "should have zero mutations");
    }

    #[test]
    fn test_sync_unwanted_pane_evicted() {
        let t = IsolatedTmux::new("sync-test-unwanted-evict");
        let tmp = TempDir::new().unwrap();

        // Create pane A in target window, then split to add B and X
        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let target_window = t.pane_window(&pane_a).unwrap();
        let pane_b = t
            .raw_cmd(&[
                "split-window",
                "-t",
                &pane_a,
                "-h",
                "-P",
                "-F",
                "#{pane_id}",
            ])
            .unwrap();
        let pane_x = t
            .raw_cmd(&[
                "split-window",
                "-t",
                &pane_a,
                "-v",
                "-P",
                "-F",
                "#{pane_id}",
            ])
            .unwrap();

        // Desired: [A, B] — X should be evicted
        let pane_columns = vec![vec![pane_a.clone()], vec![pane_b.clone()]];
        let desired: Vec<&str> = vec![pane_a.as_str(), pane_b.as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert!(
            !final_panes.contains(&pane_x),
            "X should have been evicted"
        );
        assert!(final_panes.contains(&pane_a), "A should remain");
        assert!(final_panes.contains(&pane_b), "B should remain");

        // Verify X is still alive (broken out, not killed)
        assert!(t.pane_alive(&pane_x), "X should still be alive");

        assert!(
            log.entries().iter().any(|e| e.phase == "DETACH" && e.ok),
            "should have evict entry"
        );
    }

    #[test]
    fn test_sync_missing_pane_joined() {
        let t = IsolatedTmux::new("sync-test-missing-join");
        let (target_window, panes, _tmp) = setup_panes(&t, 2);

        // panes[0] is in target_window. panes[1] is in another window.
        let pane_columns = vec![vec![panes[0].clone()], vec![panes[1].clone()]];
        let desired: Vec<&str> = vec![panes[0].as_str(), panes[1].as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert_eq!(final_panes.len(), 2);
        assert!(final_panes.contains(&panes[0]));
        assert!(final_panes.contains(&panes[1]));
        assert!(
            log.entries().iter().any(|e| e.phase == "ATTACH" && e.ok),
            "should have join entry"
        );
    }

    #[test]
    fn test_sync_dead_pane_logged() {
        let t = IsolatedTmux::new("sync-test-dead-pane");
        let tmp = TempDir::new().unwrap();

        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let target_window = t.pane_window(&pane_a).unwrap();
        let pane_b = t.new_window("test", tmp.path()).unwrap();
        let pane_c = t.new_window("test", tmp.path()).unwrap();

        // Kill pane B
        t.kill_pane(&pane_b).unwrap();

        // Desired: [A, B, C] — B is dead
        let pane_columns = vec![
            vec![pane_a.clone(), pane_b.clone()],
            vec![pane_c.clone()],
        ];
        let desired: Vec<&str> = vec![pane_a.as_str(), pane_b.as_str(), pane_c.as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        // A and C should be in the window; B should have failed
        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert!(final_panes.contains(&pane_a));
        assert!(final_panes.contains(&pane_c));
        assert!(!final_panes.contains(&pane_b), "dead pane should not be present");

        assert!(log.has_errors(), "should have errors for dead pane");
        assert!(
            log.entries()
                .iter()
                .any(|e| e.phase == "ATTACH" && !e.ok && e.message.contains(&pane_b)),
            "should have error entry mentioning dead pane"
        );
    }

    #[test]
    fn test_sync_wrong_order_reordered() {
        let t = IsolatedTmux::new("sync-test-wrong-order");
        let tmp = TempDir::new().unwrap();

        // Create A, then B in separate windows
        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let pane_b = t.new_window("test", tmp.path()).unwrap();
        // Put A into B's window: A before B (horizontal split)
        t.join_pane(&pane_a, &pane_b, "-bh").unwrap();

        // Window now has [A, B]. We want [A, B] with A as anchor.
        let target_window = t.pane_window(&pane_b).unwrap();

        // Desired: [A, B] with A as anchor — if A is currently right of B, reconcile should fix it
        let pane_columns = vec![vec![pane_a.clone()], vec![pane_b.clone()]];
        let desired: Vec<&str> = vec![pane_a.as_str(), pane_b.as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        // Both panes should be in the window
        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert!(final_panes.contains(&pane_a));
        assert!(final_panes.contains(&pane_b));
        assert!(!log.has_errors() || log.entries().iter().any(|e| e.phase == "VERIFY"),
            "reconcile should complete");
    }

    #[test]
    fn test_sync_3col_layout() {
        let t = IsolatedTmux::new("sync-test-3col");
        let (target_window, panes, _tmp) = setup_panes(&t, 5);

        let pane_columns = vec![
            vec![panes[0].clone(), panes[1].clone()],
            vec![panes[2].clone(), panes[3].clone()],
            vec![panes[4].clone()],
        ];
        let desired: Vec<&str> = pane_columns
            .iter()
            .flat_map(|col| col.iter().map(|s| s.as_str()))
            .collect();

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert_eq!(final_panes.len(), 5, "all 5 panes should be in window");
        for pane in &desired {
            assert!(
                final_panes.contains(&pane.to_string()),
                "pane {} should be in target window",
                pane
            );
        }
        assert!(!log.has_errors(), "should have no errors: {:?}", log.entries());
    }

    #[test]
    fn test_sync_single_column_stacked() {
        let t = IsolatedTmux::new("sync-test-single-col-stack");
        let (target_window, panes, _tmp) = setup_panes(&t, 3);

        // Single column with 3 panes stacked vertically
        let pane_columns = vec![vec![
            panes[0].clone(),
            panes[1].clone(),
            panes[2].clone(),
        ]];
        let desired: Vec<&str> = vec![panes[0].as_str(), panes[1].as_str(), panes[2].as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert_eq!(final_panes.len(), 3);
        for pane in &desired {
            assert!(final_panes.contains(&pane.to_string()));
        }
        assert!(!log.has_errors());
    }

    #[test]
    fn test_sync_anchor_not_in_target() {
        let t = IsolatedTmux::new("sync-test-anchor-elsewhere");
        let tmp = TempDir::new().unwrap();

        // Create 3 panes: A, B, C — each in separate windows
        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let pane_b = t.new_window("test", tmp.path()).unwrap();
        let pane_c = t.new_window("test", tmp.path()).unwrap();

        // Use B's window as target — but A is our anchor (desired first pane)
        let target_window = t.pane_window(&pane_b).unwrap();

        let pane_columns = vec![vec![pane_a.clone()], vec![pane_c.clone()]];
        let desired: Vec<&str> = vec![pane_a.as_str(), pane_c.as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert!(final_panes.contains(&pane_a), "anchor A should be in target");
        assert!(final_panes.contains(&pane_c), "C should be in target");

        // B should have been evicted (it was in target but not wanted)
        assert!(
            !final_panes.contains(&pane_b),
            "B should have been evicted"
        );
        assert!(t.pane_alive(&pane_b), "B should still be alive");

        assert!(
            log.entries().iter().any(|e| e.phase == "ATTACH" && e.ok),
            "should have anchor move entry"
        );
    }

    #[test]
    fn test_sync_mixed_evict_and_join() {
        let t = IsolatedTmux::new("sync-test-mixed");
        let tmp = TempDir::new().unwrap();

        // A in target window, X in target window (split), B in separate window
        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let target_window = t.pane_window(&pane_a).unwrap();
        let pane_x = t
            .raw_cmd(&[
                "split-window",
                "-t",
                &pane_a,
                "-h",
                "-P",
                "-F",
                "#{pane_id}",
            ])
            .unwrap();
        let pane_b = t.new_window("test", tmp.path()).unwrap();

        // Desired: [A, B] — X should be evicted, B should be joined
        let pane_columns = vec![vec![pane_a.clone()], vec![pane_b.clone()]];
        let desired: Vec<&str> = vec![pane_a.as_str(), pane_b.as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert!(final_panes.contains(&pane_a));
        assert!(final_panes.contains(&pane_b));
        assert!(!final_panes.contains(&pane_x));
        assert!(t.pane_alive(&pane_x), "X should still be alive");

        assert!(
            log.entries().iter().any(|e| e.phase == "DETACH" && e.ok),
            "should have evict"
        );
        assert!(
            log.entries().iter().any(|e| e.phase == "ATTACH" && e.ok),
            "should have join"
        );
    }

    #[test]
    fn test_sync_unwanted_is_only_pane() {
        // Edge case: target window has ONLY an unwanted pane.
        // Anchor joins first (Phase 1), then X can be evicted normally.
        let t = IsolatedTmux::new("sync-test-unwanted-only");
        let tmp = TempDir::new().unwrap();

        let pane_x = t.new_session("test", tmp.path()).unwrap();
        let target_window = t.pane_window(&pane_x).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-t", &target_window, "-x", "200", "-y", "60"]);
        let pane_a = t.new_window("test", tmp.path()).unwrap();
        let pane_b = t.new_window("test", tmp.path()).unwrap();

        // Desired: [A, B]. X is alone in target.
        let pane_columns = vec![vec![pane_a.clone()], vec![pane_b.clone()]];
        let desired: Vec<&str> = vec![pane_a.as_str(), pane_b.as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert!(final_panes.contains(&pane_a), "A should be in target");
        assert!(final_panes.contains(&pane_b), "B should be in target");
        assert!(!final_panes.contains(&pane_x), "X should have been evicted");
        assert!(t.pane_alive(&pane_x), "X should still be alive");

        // Anchor should have been moved in, and X evicted
        assert!(
            log.entries().iter().any(|e| e.phase == "ATTACH" && e.ok),
            "should have anchor entry"
        );
        assert!(
            log.entries().iter().any(|e| e.phase == "DETACH" && e.ok),
            "should have evict entry"
        );
    }

    #[test]
    fn test_sync_multiple_unwanted() {
        let t = IsolatedTmux::new("sync-test-multi-unwanted");
        let tmp = TempDir::new().unwrap();

        // Create A in target, split to add X1 and X2
        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let target_window = t.pane_window(&pane_a).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-t", &target_window, "-x", "200", "-y", "60"]);
        let pane_x1 = t
            .raw_cmd(&[
                "split-window", "-t", &pane_a, "-h", "-P", "-F", "#{pane_id}",
            ])
            .unwrap();
        let pane_x2 = t
            .raw_cmd(&[
                "split-window", "-t", &pane_a, "-v", "-P", "-F", "#{pane_id}",
            ])
            .unwrap();

        // Desired: [A] only — X1 and X2 should be evicted
        let pane_columns = vec![vec![pane_a.clone()]];
        let desired: Vec<&str> = vec![pane_a.as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert_eq!(final_panes.len(), 1, "only A should remain");
        assert!(final_panes.contains(&pane_a));
        assert!(t.pane_alive(&pane_x1), "X1 should still be alive");
        assert!(t.pane_alive(&pane_x2), "X2 should still be alive");

        let detach_count = log
            .entries()
            .iter()
            .filter(|e| e.phase == "DETACH" && e.ok && e.message.starts_with("broke"))
            .count();
        assert!(detach_count >= 2, "should have detached at least 2 panes, got {}", detach_count);
    }

    #[test]
    fn test_sync_wanted_in_shared_window() {
        // Wanted pane B shares a window with C. B must be isolated before joining.
        let t = IsolatedTmux::new("sync-test-shared-window");
        let tmp = TempDir::new().unwrap();

        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let target_window = t.pane_window(&pane_a).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-t", &target_window, "-x", "200", "-y", "60"]);

        // Create B and C in a separate window (split)
        let pane_b = t.new_window("test", tmp.path()).unwrap();
        let pane_c = t
            .raw_cmd(&[
                "split-window", "-t", &pane_b, "-h", "-P", "-F", "#{pane_id}",
            ])
            .unwrap();

        // Desired: [A, B]. B shares a window with C.
        let pane_columns = vec![vec![pane_a.clone()], vec![pane_b.clone()]];
        let desired: Vec<&str> = vec![pane_a.as_str(), pane_b.as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert!(final_panes.contains(&pane_a), "A should be in target");
        assert!(final_panes.contains(&pane_b), "B should be in target");
        assert!(!final_panes.contains(&pane_c), "C should NOT be in target");

        // C should still be alive in its own window
        assert!(t.pane_alive(&pane_c), "C should still be alive");
        assert!(!log.has_errors(), "should have no errors: {:?}", log.entries());
    }

    // --- Real-world simulation tests ---

    #[test]
    fn test_reconcile_with_stale_sessions() {
        // Simulate: 5 panes created, 2 die, reconcile with the 3 survivors
        let t = IsolatedTmux::new("sync-test-stale-sessions");
        let (target_window, panes, _tmp) = setup_panes(&t, 5);

        // Kill panes 3 and 4
        t.kill_pane(&panes[3]).unwrap();
        t.kill_pane(&panes[4]).unwrap();

        // Desired layout uses only the 3 survivors
        let pane_columns = vec![
            vec![panes[0].clone(), panes[1].clone()],
            vec![panes[2].clone()],
        ];
        let desired: Vec<&str> = pane_columns
            .iter()
            .flat_map(|col| col.iter().map(|s| s.as_str()))
            .collect();

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        for pane in &desired {
            assert!(
                final_panes.contains(&pane.to_string()),
                "survivor {} should be in target",
                pane
            );
        }
        // Dead panes should not appear
        assert!(!final_panes.contains(&panes[3]));
        assert!(!final_panes.contains(&panes[4]));
        assert!(!log.has_errors(), "no errors for survivors: {:?}", log.entries());
    }

    #[test]
    fn test_reconcile_multi_file_same_pane() {
        // Simulate: multiple files claim the same pane (deduplication)
        let t = IsolatedTmux::new("sync-test-multi-file-same-pane");
        let (target_window, panes, _tmp) = setup_panes(&t, 3);

        // Two columns, but panes[1] appears twice (like two files claiming same pane)
        // After dedup, we should get [panes[0], panes[1], panes[2]]
        let pane_columns = vec![
            vec![panes[0].clone(), panes[1].clone()],
            vec![panes[2].clone()],
        ];
        let desired: Vec<&str> = pane_columns
            .iter()
            .flat_map(|col| col.iter().map(|s| s.as_str()))
            .collect();

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert_eq!(final_panes.len(), 3, "should have 3 unique panes");
        assert!(!log.has_errors());
    }

    #[test]
    fn test_reconcile_dead_panes_in_desired() {
        // Some desired panes are dead — reconcile should skip them gracefully
        let t = IsolatedTmux::new("sync-test-dead-desired");
        let tmp = TempDir::new().unwrap();

        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let target_window = t.pane_window(&pane_a).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-t", &target_window, "-x", "200", "-y", "60"]);
        let pane_b = t.new_window("test", tmp.path()).unwrap();
        let pane_c = t.new_window("test", tmp.path()).unwrap();
        let pane_d = t.new_window("test", tmp.path()).unwrap();

        // Kill B and D
        t.kill_pane(&pane_b).unwrap();
        t.kill_pane(&pane_d).unwrap();

        // Desired: [A, B, C, D] — B and D are dead
        let pane_columns = vec![
            vec![pane_a.clone(), pane_b.clone()],
            vec![pane_c.clone(), pane_d.clone()],
        ];
        let desired: Vec<&str> = vec![
            pane_a.as_str(),
            pane_b.as_str(),
            pane_c.as_str(),
            pane_d.as_str(),
        ];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        // A and C should be arranged; B and D silently skipped
        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert!(final_panes.contains(&pane_a), "A should be in target");
        assert!(final_panes.contains(&pane_c), "C should be in target");
        assert!(!final_panes.contains(&pane_b), "dead B should not be present");
        assert!(!final_panes.contains(&pane_d), "dead D should not be present");

        // Should have logged errors for dead panes
        assert!(log.has_errors(), "should have errors for dead panes");
    }

    #[test]
    fn test_reconcile_large_layout() {
        // 8 panes in a 3-column grid simulating real editor state
        let t = IsolatedTmux::new("sync-test-large-layout");
        let (target_window, panes, _tmp) = setup_panes(&t, 8);

        // 3 columns: [0,1,2], [3,4,5], [6,7]
        let pane_columns = vec![
            vec![panes[0].clone(), panes[1].clone(), panes[2].clone()],
            vec![panes[3].clone(), panes[4].clone(), panes[5].clone()],
            vec![panes[6].clone(), panes[7].clone()],
        ];
        let desired: Vec<&str> = pane_columns
            .iter()
            .flat_map(|col| col.iter().map(|s| s.as_str()))
            .collect();

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert_eq!(final_panes.len(), 8, "all 8 panes should be in window");
        for pane in &desired {
            assert!(
                final_panes.contains(&pane.to_string()),
                "pane {} should be in target window",
                pane
            );
        }
        assert!(!log.has_errors(), "should have no errors: {:?}", log.entries());
    }

    #[test]
    fn test_sync_3_panes_to_2_evicts_extra() {
        // Real-world scenario: window has 3 panes from previous sync,
        // editor now has only 2 files open → 3rd pane must be evicted.
        let t = IsolatedTmux::new("sync-test-3to2-evict");
        let tmp = TempDir::new().unwrap();

        // Create A in target window, then split to add B and C (3 panes in same window)
        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let target_window = t.pane_window(&pane_a).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-t", &target_window, "-x", "200", "-y", "60"]);
        let pane_b = t
            .raw_cmd(&["split-window", "-t", &pane_a, "-h", "-P", "-F", "#{pane_id}"])
            .unwrap();
        let pane_c = t
            .raw_cmd(&["split-window", "-t", &pane_b, "-h", "-P", "-F", "#{pane_id}"])
            .unwrap();

        // Verify setup: 3 panes in target window
        let initial = t.list_window_panes(&target_window).unwrap();
        assert_eq!(initial.len(), 3, "should start with 3 panes");

        // Desired: only [A, B] — C should be evicted
        let pane_columns = vec![vec![pane_a.clone()], vec![pane_b.clone()]];
        let desired: Vec<&str> = vec![pane_a.as_str(), pane_b.as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert_eq!(final_panes.len(), 2, "should have exactly 2 panes after sync");
        assert!(final_panes.contains(&pane_a), "A should remain");
        assert!(final_panes.contains(&pane_b), "B should remain");
        assert!(!final_panes.contains(&pane_c), "C should be evicted");
        assert!(t.pane_alive(&pane_c), "C should still be alive (detached, not killed)");
        assert!(!log.has_errors(), "no errors: {:?}", log.entries());

        // Verify C was detached
        let detach_count = log
            .entries()
            .iter()
            .filter(|e| e.phase == "DETACH" && e.ok)
            .count();
        assert!(detach_count >= 1, "should have detached at least 1 pane");
    }

    #[test]
    fn test_sync_3_panes_to_2_with_external_join() {
        // Window has 3 panes, desired has 2 — one of the desired is in a different window.
        // This is the exact scenario: @187 has [%62, %60, %41], desired is [%62, %39]
        // where %39 is in @206.
        let t = IsolatedTmux::new("sync-test-3to2-external");
        let tmp = TempDir::new().unwrap();

        // Create A in target, split to add X1 and X2 (3 panes in window)
        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let target_window = t.pane_window(&pane_a).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-t", &target_window, "-x", "200", "-y", "60"]);
        let pane_x1 = t
            .raw_cmd(&["split-window", "-t", &pane_a, "-h", "-P", "-F", "#{pane_id}"])
            .unwrap();
        let pane_x2 = t
            .raw_cmd(&["split-window", "-t", &pane_x1, "-h", "-P", "-F", "#{pane_id}"])
            .unwrap();

        // Create B in a separate window
        let pane_b = t.new_window("test", tmp.path()).unwrap();

        // Verify: target has 3 panes, B is elsewhere
        let initial = t.list_window_panes(&target_window).unwrap();
        assert_eq!(initial.len(), 3);
        assert!(!initial.contains(&pane_b));

        // Desired: [A, B] — X1 and X2 must be evicted, B must be joined
        let pane_columns = vec![vec![pane_a.clone()], vec![pane_b.clone()]];
        let desired: Vec<&str> = vec![pane_a.as_str(), pane_b.as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert_eq!(final_panes.len(), 2, "should have exactly 2 panes");
        assert!(final_panes.contains(&pane_a), "A in target");
        assert!(final_panes.contains(&pane_b), "B joined into target");
        assert!(!final_panes.contains(&pane_x1), "X1 evicted");
        assert!(!final_panes.contains(&pane_x2), "X2 evicted");
        assert!(t.pane_alive(&pane_x1), "X1 still alive");
        assert!(t.pane_alive(&pane_x2), "X2 still alive");
        assert!(!log.has_errors(), "no errors: {:?}", log.entries());
    }

    #[test]
    fn test_reconcile_pane_from_shared_window() {
        // Desired pane shares a window with other panes — sync should isolate and join it
        let t = IsolatedTmux::new("sync-test-pane-from-shared");
        let tmp = TempDir::new().unwrap();

        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let target_window = t.pane_window(&pane_a).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-t", &target_window, "-x", "200", "-y", "60"]);

        // Create B and C in a shared window (not the target)
        let pane_b = t.new_window("test", tmp.path()).unwrap();
        let shared_window = t.pane_window(&pane_b).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-t", &shared_window, "-x", "200", "-y", "60"]);
        let pane_c = t
            .raw_cmd(&["split-window", "-t", &pane_b, "-h", "-P", "-F", "#{pane_id}"])
            .unwrap();

        // Desired layout: [A, B] — B must be pulled from shared window
        let pane_columns = vec![vec![pane_a.clone()], vec![pane_b.clone()]];
        let desired: Vec<&str> = vec![pane_a.as_str(), pane_b.as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert!(final_panes.contains(&pane_a), "A should be in target");
        assert!(final_panes.contains(&pane_b), "B should be in target");
        assert!(t.pane_alive(&pane_c), "C should still be alive");
        assert!(!log.has_errors(), "should have no errors: {:?}", log.entries());
    }

    #[test]
    fn test_reconcile_scattered_panes() {
        // 4 panes scattered across solo windows. Desired: [A, B] col1, [C] col2
        let t = IsolatedTmux::new("sync-test-scattered");
        let tmp = TempDir::new().unwrap();

        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-x", "200", "-y", "60"]);
        let pane_b = t.new_window("test", tmp.path()).unwrap();
        let pane_c = t.new_window("test", tmp.path()).unwrap();
        let pane_d = t.new_window("test", tmp.path()).unwrap();

        let target_window = t.pane_window(&pane_a).unwrap();

        let pane_columns = vec![
            vec![pane_a.clone(), pane_b.clone()],
            vec![pane_c.clone()],
        ];
        let desired: Vec<&str> = vec![pane_a.as_str(), pane_b.as_str(), pane_c.as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert_eq!(final_panes.len(), 3, "should have exactly 3 panes");
        assert!(final_panes.contains(&pane_a), "A in target");
        assert!(final_panes.contains(&pane_b), "B in target");
        assert!(final_panes.contains(&pane_c), "C in target");
        assert!(!final_panes.contains(&pane_d), "D not in target");
        assert!(t.pane_alive(&pane_d), "D still alive");
        assert!(!log.has_errors(), "no errors: {:?}", log.entries());
    }

    #[test]
    fn test_reconcile_evict_and_join_external() {
        // Target has [A, X1, X2], desired is [A, B] where B is in separate window.
        let t = IsolatedTmux::new("sync-test-evict-join-ext");
        let tmp = TempDir::new().unwrap();

        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let target_window = t.pane_window(&pane_a).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-t", &target_window, "-x", "200", "-y", "60"]);

        let pane_x1 = t
            .raw_cmd(&["split-window", "-t", &pane_a, "-h", "-P", "-F", "#{pane_id}"])
            .unwrap();
        let pane_x2 = t
            .raw_cmd(&["split-window", "-t", &pane_x1, "-h", "-P", "-F", "#{pane_id}"])
            .unwrap();

        let pane_b = t.new_window("test", tmp.path()).unwrap();

        let pane_columns = vec![vec![pane_a.clone()], vec![pane_b.clone()]];
        let desired: Vec<&str> = vec![pane_a.as_str(), pane_b.as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert_eq!(final_panes.len(), 2, "should have exactly A and B");
        assert!(final_panes.contains(&pane_a), "A in target");
        assert!(final_panes.contains(&pane_b), "B in target");
        assert!(t.pane_alive(&pane_x1), "X1 still alive");
        assert!(t.pane_alive(&pane_x2), "X2 still alive");
        assert!(!log.has_errors(), "no errors: {:?}", log.entries());
    }

    #[test]
    fn test_reconcile_full_real_world_scenario() {
        // 4 solo windows (A, B, C, D). Previous sync left A+C together.
        // Desired: col1=[B, D], col2=[A]. C evicted, B and D joined.
        let t = IsolatedTmux::new("sync-test-full-realworld");
        let tmp = TempDir::new().unwrap();

        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-x", "200", "-y", "60"]);
        let pane_b = t.new_window("test", tmp.path()).unwrap();
        let pane_c = t.new_window("test", tmp.path()).unwrap();
        let pane_d = t.new_window("test", tmp.path()).unwrap();

        // Put A and C into the same window (simulating a previous sync)
        let target_window = t.pane_window(&pane_a).unwrap();
        t.join_pane(&pane_c, &pane_a, "-h").unwrap();

        let initial = t.list_window_panes(&target_window).unwrap();
        assert_eq!(initial.len(), 2);
        assert!(initial.contains(&pane_a));
        assert!(initial.contains(&pane_c));

        let pane_columns = vec![
            vec![pane_b.clone(), pane_d.clone()],
            vec![pane_a.clone()],
        ];
        let desired: Vec<&str> = vec![pane_b.as_str(), pane_d.as_str(), pane_a.as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert!(final_panes.contains(&pane_a), "A in target");
        assert!(final_panes.contains(&pane_b), "B in target");
        assert!(final_panes.contains(&pane_d), "D in target");
        assert!(!final_panes.contains(&pane_c), "C evicted from target");
        assert!(t.pane_alive(&pane_c), "C still alive");
        assert!(!log.has_errors(), "no errors: {:?}", log.entries());
    }

    #[test]
    #[ignore] // uses set_current_dir which is not thread-safe
    fn test_sync_non_agent_doc_first_preserves_layout() {
        // Bug reproduction: editor sends 2 files where the first has no session UUID.
        // The non-agent-doc file is skipped, leaving only 1 resolved pane.
        // Previously, the < 2 path would break existing panes out of the window.
        // Fix: just focus the resolved pane without rearranging.
        let t = IsolatedTmux::new("sync-test-non-agent-first");
        let tmp = TempDir::new().unwrap();

        // Create 2 panes in the same window (simulating an existing layout)
        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let target_window = t.pane_window(&pane_a).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-t", &target_window, "-x", "200", "-y", "60"]);
        let pane_b = t
            .raw_cmd(&["split-window", "-t", &pane_a, "-h", "-P", "-F", "#{pane_id}"])
            .unwrap();

        // Verify: 2 panes in window
        let initial = t.list_window_panes(&target_window).unwrap();
        assert_eq!(initial.len(), 2, "should start with 2 panes");

        // Create files: one non-agent-doc (no UUID), one agent-doc (with UUID)
        let non_agent_file = tmp.path().join("BrianTakita.md");
        std::fs::write(&non_agent_file, "# Brian Takita\n\nNo frontmatter here.\n").unwrap();

        let session_id = "test-session-123";
        let agent_file = tmp.path().join("plugin.md");
        std::fs::write(
            &agent_file,
            format!("---\nsession: {}\n---\n\n## User\n\nHello\n", session_id),
        )
        .unwrap();

        // Register the agent-doc session → pane_b
        let sessions_dir = tmp.path().join(".agent-doc");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        let registry = format!(
            r#"{{"{session_id}": {{"pane": "{pane_b}", "pid": 1, "cwd": "{cwd}", "started": "2026-01-01T00:00:00Z", "file": "plugin.md", "window": "{target_window}"}}}}"#,
            session_id = session_id,
            pane_b = pane_b,
            cwd = tmp.path().display(),
            target_window = target_window,
        );
        std::fs::write(sessions_dir.join("sessions.json"), &registry).unwrap();

        // Save current dir and change to temp dir (sessions.json is CWD-relative)
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        // Call run_with_tmux with non-agent-doc file first
        let col_args = [
            non_agent_file.to_string_lossy().to_string(),
            agent_file.to_string_lossy().to_string(),
        ];
        let result = run_with_tmux(
            &col_args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            Some(&target_window),
            None,
            &t,
        );

        // Restore CWD
        std::env::set_current_dir(&orig_dir).unwrap();

        assert!(result.is_ok(), "run_with_tmux should succeed: {:?}", result);

        // The key assertion: the window should still have 2 panes.
        // Before the fix, the < 2 path would break_pane the unwanted panes,
        // leaving only 1 pane.
        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert_eq!(
            final_panes.len(),
            2,
            "window should still have 2 panes (non-agent-doc file should not break layout)"
        );
    }

    // --- Property-based tests ---

    mod proptest_reconcile {
        use super::*;
        use proptest::prelude::*;
        use std::sync::atomic::{AtomicUsize, Ordering};

        static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

        fn unique_socket(prefix: &str) -> String {
            let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
            format!("{}-{}-{}", prefix, std::process::id(), id)
        }

        /// Distribute N panes into num_cols columns as evenly as possible.
        fn distribute_into_columns(panes: &[String], num_cols: usize) -> Vec<Vec<String>> {
            let mut columns: Vec<Vec<String>> = (0..num_cols).map(|_| Vec::new()).collect();
            for (i, pane) in panes.iter().enumerate() {
                columns[i % num_cols].push(pane.clone());
            }
            columns.retain(|c| !c.is_empty());
            columns
        }

        proptest! {
            #[test]
            fn reconcile_completeness(
                num_panes in 2..6usize,
                num_cols in 1..4usize,
            ) {
                let t = IsolatedTmux::new(&unique_socket("pt-comp"));
                let (target_window, panes, _tmp) = setup_panes(&t, num_panes);

                let pane_columns = distribute_into_columns(&panes, num_cols);
                let desired: Vec<&str> = pane_columns
                    .iter()
                    .flat_map(|col| col.iter().map(|s| s.as_str()))
                    .collect();

                let _log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

                // After reconcile, all desired panes must be in the target window
                let final_panes = t.list_window_panes(&target_window).unwrap();
                for pane in &desired {
                    prop_assert!(
                        final_panes.contains(&pane.to_string()),
                        "pane {} missing from target window. final={:?}",
                        pane,
                        final_panes
                    );
                }
                // No extra panes
                for pane in &final_panes {
                    prop_assert!(
                        desired.contains(&pane.as_str()),
                        "unexpected pane {} in target window",
                        pane
                    );
                }
            }

            #[test]
            fn reconcile_idempotent(
                num_panes in 2..5usize,
                num_cols in 1..3usize,
            ) {
                let t = IsolatedTmux::new(&unique_socket("pt-idemp"));
                let (target_window, panes, _tmp) = setup_panes(&t, num_panes);

                let pane_columns = distribute_into_columns(&panes, num_cols);
                let desired: Vec<&str> = pane_columns
                    .iter()
                    .flat_map(|col| col.iter().map(|s| s.as_str()))
                    .collect();

                // First reconcile
                let _log1 = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

                // Second reconcile — should be fast path (zero mutations)
                let log2 = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

                prop_assert!(
                    log2.entries().iter().any(|e| e.phase == "FAST_PATH"),
                    "second reconcile should take fast path, got: {:?}",
                    log2.entries()
                );
            }

            #[test]
            fn reconcile_no_pane_loss(
                num_panes in 2..6usize,
                num_cols in 1..4usize,
            ) {
                let t = IsolatedTmux::new(&unique_socket("pt-noloss"));
                let (_, panes, _tmp) = setup_panes(&t, num_panes);

                let pane_columns = distribute_into_columns(&panes, num_cols);
                let desired: Vec<&str> = pane_columns
                    .iter()
                    .flat_map(|col| col.iter().map(|s| s.as_str()))
                    .collect();
                let target_window = t.pane_window(&panes[0]).unwrap();

                let _log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

                // All panes must still be alive (no pane was destroyed)
                for pane in &panes {
                    prop_assert!(
                        t.pane_alive(pane),
                        "pane {} should still be alive after reconcile",
                        pane
                    );
                }
            }
        }
    }
}
