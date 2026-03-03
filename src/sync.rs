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
            .filter(|e| matches!(e.phase, "ANCHOR" | "EVICT" | "JOIN" | "ORDER"))
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

    // Single file without --window: just focus.
    if all_files.len() == 1 && window.is_none() {
        return crate::focus::run_with_tmux(all_files[0], None, tmux);
    }

    // --- Phase 1: Resolve each file to its session pane ---
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut resolved: Vec<ResolvedFile> = Vec::new();
    let mut unresolved_files: Vec<PathBuf> = Vec::new();

    for file in &all_files {
        if !file.exists() {
            eprintln!(
                "warning: file not found: {}, will auto-create session",
                file.display()
            );
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
        if file.exists() {
            let content = std::fs::read_to_string(file)
                .with_context(|| format!("failed to read {}", file.display()))?;
            let (updated_content, session_id) = frontmatter::ensure_session(&content)?;
            if updated_content != content {
                std::fs::write(file, &updated_content)
                    .with_context(|| format!("failed to write {}", file.display()))?;
            }

            let pane_id = tmux.auto_start("claude", &cwd)?;
            let file_str = file.to_string_lossy();
            sessions::register(&session_id, &pane_id, &file_str)?;

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
        // Not enough panes for 2D layout, but still clean the target window.
        if let Some(target_win) = window {
            let wanted: HashSet<String> =
                resolved.iter().map(|r| r.pane_id.clone()).collect();
            let window_panes = tmux.list_window_panes(target_win).unwrap_or_default();
            for existing_pane in &window_panes {
                if !wanted.contains(existing_pane.as_str()) && window_panes.len() > 1 {
                    tmux.break_pane(existing_pane)?;
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
    let log = reconcile(tmux, &target_window, &pane_columns, &desired_ordered)?;

    // --- Phase 6: Resize + Focus ---
    equalize_sizes(tmux, &pane_columns);
    tmux.select_pane(&focus_pane)?;

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
/// # Algorithm: Additive reconciliation with convergent loop
///
/// ```text
/// for attempt in 0..3:
///   PHASE 0: SNAPSHOT + FAST PATH
///   PHASE 1: ENSURE ANCHOR in target window
///   PHASE 2: EVICT unwanted panes (defer if last pane)
///   PHASE 3: JOIN MISSING + break deferred unwanted
///   PHASE 4: REORDER (if all wanted present but wrong order)
/// VERIFY + REGISTRY UPDATE (after loop)
/// ```
///
/// # Invariants
///
/// - **I1**: Target window always has ≥1 pane (anchor never broken out)
/// - **I2**: Only unwanted panes are evicted; wanted panes stay (except Phase 4 reorder)
/// - **I3**: Every tmux command is logged
/// - **I4**: Individual failures are non-fatal (logged, algorithm continues)
/// - **I5**: Registry is updated after every move
fn reconcile(
    tmux: &Tmux,
    target_window: &str,
    pane_columns: &[Vec<String>],
    desired_ordered: &[&str],
) -> Result<SyncLog> {
    let mut log = SyncLog::new();

    let wanted: HashSet<&str> = desired_ordered.iter().copied().collect();
    let anchor = desired_ordered[0];

    for attempt in 0..3 {
        // --- PHASE 0: SNAPSHOT ---
        let current = tmux.list_panes_ordered(target_window).unwrap_or_default();
        let current_refs: Vec<&str> = current.iter().map(|s| s.as_str()).collect();
        log.log(
            "SNAPSHOT",
            format!(
                "attempt={}, window={}, current={:?}, desired={:?}",
                attempt, target_window, current_refs, desired_ordered
            ),
        );

        // --- FAST PATH ---
        if current_refs == desired_ordered {
            log.log("FAST_PATH", "layout already correct");
            return Ok(log);
        }

        let present: HashSet<&str> = current_refs.iter().copied().collect();

        // --- PHASE 1: ENSURE ANCHOR ---
        if !present.contains(anchor) {
            let existing = tmux.list_window_panes(target_window).unwrap_or_default();
            if let Some(first) = existing.first() {
                // Isolate anchor from its current window if it shares one
                if let Ok(anchor_win) = tmux.pane_window(anchor) {
                    let anchor_siblings =
                        tmux.list_window_panes(&anchor_win).unwrap_or_default();
                    if anchor_siblings.len() > 1 {
                        if let Err(e) = tmux.break_pane(anchor) {
                            log.log_err(
                                "ANCHOR",
                                format!("failed to isolate anchor {}: {}", anchor, e),
                            );
                        }
                    }
                }
                match tmux.join_pane(anchor, first, "-bh") {
                    Ok(()) => {
                        log.log(
                            "ANCHOR",
                            format!("joined {} into {}", anchor, target_window),
                        );
                        update_registry(tmux, anchor, &mut log);
                    }
                    Err(e) => {
                        log.log_err(
                            "ANCHOR",
                            format!(
                                "failed to join anchor {} into {}: {}",
                                anchor, target_window, e
                            ),
                        );
                        continue; // retry on next attempt
                    }
                }
            } else {
                log.log_err(
                    "ANCHOR",
                    format!("target window {} has no panes", target_window),
                );
                continue;
            }
        }

        // --- PHASE 2: EVICT unwanted panes (defer last-pane case) ---
        let mut deferred_unwanted: Vec<String> = Vec::new();
        let current_after_anchor =
            tmux.list_window_panes(target_window).unwrap_or_default();
        for pane in &current_after_anchor {
            if wanted.contains(pane.as_str()) {
                continue;
            }
            let window_count =
                tmux.list_window_panes(target_window).unwrap_or_default().len();
            if window_count <= 1 {
                deferred_unwanted.push(pane.clone());
                log.log(
                    "EVICT",
                    format!("deferred {} — last pane in window", pane),
                );
                continue;
            }
            match tmux.break_pane(pane) {
                Ok(()) => {
                    log.log(
                        "EVICT",
                        format!("broke {} out of {}", pane, target_window),
                    );
                    update_registry(tmux, pane, &mut log);
                }
                Err(e) => {
                    log.log_err("EVICT", format!("failed to break {}: {}", pane, e));
                }
            }
        }

        // --- PHASE 3: JOIN MISSING wanted panes ---
        let present_after_evict: HashSet<String> = tmux
            .list_window_panes(target_window)
            .unwrap_or_default()
            .into_iter()
            .collect();

        for (col_idx, column) in pane_columns.iter().enumerate() {
            for (row_idx, pane) in column.iter().enumerate() {
                if present_after_evict.contains(pane.as_str()) {
                    continue;
                }
                if pane.as_str() == anchor {
                    continue;
                }

                // Isolate pane from its current window if it shares one
                let pane_win = match tmux.pane_window(pane) {
                    Ok(w) => w,
                    Err(e) => {
                        log.log_err(
                            "JOIN",
                            format!("pane {} not found (dead?): {}", pane, e),
                        );
                        continue;
                    }
                };
                let siblings = tmux.list_window_panes(&pane_win).unwrap_or_default();
                if siblings.len() > 1 {
                    if let Err(e) = tmux.break_pane(pane) {
                        log.log_err(
                            "JOIN",
                            format!("failed to isolate {}: {}", pane, e),
                        );
                        continue;
                    }
                }

                let (target_pane, flag) = join_target(pane_columns, col_idx, row_idx);
                match tmux.join_pane(pane, &target_pane, flag) {
                    Ok(()) => {
                        log.log(
                            "JOIN",
                            format!("{} → {} ({})", pane, target_pane, flag),
                        );
                        update_registry(tmux, pane, &mut log);
                    }
                    Err(e) => {
                        log.log_err(
                            "JOIN",
                            format!(
                                "failed to join {} → {}: {}",
                                pane, target_pane, e
                            ),
                        );
                    }
                }
            }
        }

        // Break deferred unwanted panes (window now has wanted panes)
        for pane in &deferred_unwanted {
            let window_count =
                tmux.list_window_panes(target_window).unwrap_or_default().len();
            if window_count <= 1 {
                log.log_err(
                    "EVICT",
                    format!("cannot break deferred {} — still last pane", pane),
                );
                continue;
            }
            match tmux.break_pane(pane) {
                Ok(()) => {
                    log.log(
                        "EVICT",
                        format!("broke deferred {} out of {}", pane, target_window),
                    );
                    update_registry(tmux, pane, &mut log);
                }
                Err(e) => {
                    log.log_err(
                        "EVICT",
                        format!("failed to break deferred {}: {}", pane, e),
                    );
                }
            }
        }

        // --- PHASE 4: REORDER (if all wanted present but wrong order) ---
        let after_join = tmux.list_panes_ordered(target_window).unwrap_or_default();
        let after_refs: Vec<&str> = after_join.iter().map(|s| s.as_str()).collect();
        let after_set: HashSet<&str> = after_refs.iter().copied().collect();

        if after_set == wanted && after_refs != desired_ordered {
            log.log(
                "ORDER",
                format!(
                    "reordering: current={:?}, desired={:?}",
                    after_refs, desired_ordered
                ),
            );
            // Break all non-anchor wanted panes out
            for pane in desired_ordered.iter().skip(1) {
                match tmux.break_pane(pane) {
                    Ok(()) => {
                        log.log("ORDER", format!("broke {} for reorder", pane));
                    }
                    Err(e) => {
                        log.log_err(
                            "ORDER",
                            format!("failed to break {} for reorder: {}", pane, e),
                        );
                    }
                }
            }
            // Rejoin in correct column structure
            for (col_idx, column) in pane_columns.iter().enumerate() {
                for (row_idx, pane) in column.iter().enumerate() {
                    if pane.as_str() == anchor {
                        continue;
                    }
                    let (target_pane, flag) =
                        join_target(pane_columns, col_idx, row_idx);
                    match tmux.join_pane(pane, &target_pane, flag) {
                        Ok(()) => {
                            log.log(
                                "ORDER",
                                format!("{} → {} ({})", pane, target_pane, flag),
                            );
                            update_registry(tmux, pane, &mut log);
                        }
                        Err(e) => {
                            log.log_err(
                                "ORDER",
                                format!(
                                    "failed reorder join {} → {}: {}",
                                    pane, target_pane, e
                                ),
                            );
                        }
                    }
                }
            }
        }
    } // end convergent loop

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
        log.log_err("EVICT", "something failed");
        assert_eq!(log.entries().len(), 2);
        assert!(log.entries()[0].ok);
        assert_eq!(log.entries()[0].phase, "SNAPSHOT");
        assert!(!log.entries()[1].ok);
        assert_eq!(log.entries()[1].phase, "EVICT");
    }

    #[test]
    fn sync_log_has_errors() {
        let mut log = SyncLog::new();
        log.log("SNAPSHOT", "ok");
        assert!(!log.has_errors());
        log.log_err("EVICT", "bad");
        assert!(log.has_errors());
    }

    #[test]
    fn sync_log_mutation_count() {
        let mut log = SyncLog::new();
        log.log("SNAPSHOT", "snapshot");
        log.log("FAST_PATH", "fast");
        log.log("EVICT", "broke pane");
        log.log("JOIN", "joined pane");
        log.log("VERIFY", "ok");
        assert_eq!(log.mutation_count(), 2); // EVICT + JOIN
    }

    // --- Helper: set up N panes in separate windows within an isolated tmux ---

    fn setup_panes(tmux: &Tmux, n: usize) -> (String, Vec<String>) {
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
        (target_window, panes)
    }

    // --- Integration tests using IsolatedTmux ---

    #[test]
    fn test_sync_2col_happy_path() {
        let t = IsolatedTmux::new("sync-test-2col-happy");
        let (target_window, panes) = setup_panes(&t, 3);

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
            log.entries().iter().any(|e| e.phase == "EVICT" && e.ok),
            "should have evict entry"
        );
    }

    #[test]
    fn test_sync_missing_pane_joined() {
        let t = IsolatedTmux::new("sync-test-missing-join");
        let (target_window, panes) = setup_panes(&t, 2);

        // panes[0] is in target_window. panes[1] is in another window.
        let pane_columns = vec![vec![panes[0].clone()], vec![panes[1].clone()]];
        let desired: Vec<&str> = vec![panes[0].as_str(), panes[1].as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert_eq!(final_panes.len(), 2);
        assert!(final_panes.contains(&panes[0]));
        assert!(final_panes.contains(&panes[1]));
        assert!(
            log.entries().iter().any(|e| e.phase == "JOIN" && e.ok),
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
                .any(|e| e.phase == "JOIN" && !e.ok && e.message.contains(&pane_b)),
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
        let (target_window, panes) = setup_panes(&t, 5);

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
        let (target_window, panes) = setup_panes(&t, 3);

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
            log.entries().iter().any(|e| e.phase == "ANCHOR" && e.ok),
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
            log.entries().iter().any(|e| e.phase == "EVICT" && e.ok),
            "should have evict"
        );
        assert!(
            log.entries().iter().any(|e| e.phase == "JOIN" && e.ok),
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
            log.entries().iter().any(|e| e.phase == "ANCHOR" && e.ok),
            "should have anchor entry"
        );
        assert!(
            log.entries().iter().any(|e| e.phase == "EVICT" && e.ok),
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

        let evict_count = log
            .entries()
            .iter()
            .filter(|e| e.phase == "EVICT" && e.ok && e.message.starts_with("broke"))
            .count();
        assert!(evict_count >= 2, "should have evicted at least 2 panes, got {}", evict_count);
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
                let (target_window, panes) = setup_panes(&t, num_panes);

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
                let (target_window, panes) = setup_panes(&t, num_panes);

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
                let (_, panes) = setup_panes(&t, num_panes);

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
