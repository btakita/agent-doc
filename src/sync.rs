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

    /// Which column index contains this file, if any.
    fn column_of(&self, file: &Path) -> Option<usize> {
        self.columns
            .iter()
            .position(|col| col.files.iter().any(|f| f == file))
    }
}

// =========================================================================
// Helpers
// =========================================================================

/// Find a donor pane in the same column as `file` (same-column only, no spiral).
/// Returns the pane_id of a resolved file in the same column.
fn find_column_pane(
    layout: &Layout,
    file: &Path,
    file_to_pane: &std::collections::HashMap<PathBuf, String>,
) -> Option<String> {
    let col_idx = layout.column_of(file)?;
    for f in &layout.columns[col_idx].files {
        if let Some(pane) = file_to_pane.get(f) {
            return Some(pane.clone());
        }
    }
    None
}

/// Find the best window for consolidating wanted panes.
/// Prefers the window (within the target session) that already contains the most wanted panes.
fn find_best_window(
    tmux: &Tmux,
    wanted: &std::collections::HashSet<&str>,
    target_session: Option<&str>,
) -> String {
    let mut best_window = String::new();
    let mut best_wanted = 0usize;
    let mut best_total = 0usize;
    for pane_id in wanted {
        let win = match tmux.pane_window(pane_id) {
            Ok(w) => w,
            Err(_) => continue,
        };
        // Only consider windows in the same tmux session
        if let Some(ts) = target_session
            && let Ok(pane_sess) = tmux.pane_session(pane_id)
                && pane_sess != ts {
                    continue;
                }
        let window_panes = tmux.list_window_panes(&win).unwrap_or_default();
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
    eprintln!("target_window={} (auto-detected, {} wanted panes)", best_window, best_wanted);
    best_window
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

    // Log comprehensive tmux tree at sync start
    if let Ok(tree) = tmux.dump_tmux_tree() {
        eprintln!("{}", tree);
    }

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
    let mut resolved: Vec<ResolvedFile> = Vec::new();
    let mut unresolved_files: Vec<PathBuf> = Vec::new();
    let mut non_agent_doc_files: Vec<PathBuf> = Vec::new();
    let mut doc_tmux_session: Option<String> = None;
    let mut files_needing_tmux_session: Vec<PathBuf> = Vec::new();

    for file in &all_files {
        if !file.exists() {
            eprintln!("warning: file not found: {}, skipping", file.display());
            continue;
        }
        let content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let (fm, _) = frontmatter::parse(&content)?;

        // Collect tmux_session from first doc that has it
        if doc_tmux_session.is_none()
            && let Some(ref ts) = fm.tmux_session {
                doc_tmux_session = Some(ts.clone());
                eprintln!("tmux_session={} (from {})", ts, file.display());
            }
        if fm.tmux_session.is_none() && fm.session.is_some() {
            files_needing_tmux_session.push(file.to_path_buf());
        }

        let session_id = match fm.session {
            Some(id) => id,
            None => {
                // No session UUID — not an agent-doc file. Track and skip.
                non_agent_doc_files.push(file.to_path_buf());
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

    // --- Phase 2: Log unresolved files (no auto-restart) ---
    // Auto-restart was creating orphan tmux windows. Instead, skip dead/missing panes.
    // The user can manually start sessions with `agent-doc claim` or `agent-doc start`.
    for file in &unresolved_files {
        eprintln!(
            "skipping {} — pane dead/missing (use `agent-doc claim` to re-register)",
            file.display()
        );
    }

    // Build a lookup from file path → pane_id (mutable for Phase 1.5 auto-register)
    let mut file_to_pane: std::collections::HashMap<PathBuf, String> = resolved
        .iter()
        .map(|r| (r.path.clone(), r.pane_id.clone()))
        .collect();

    // --- Phase 1.5: In-memory auto-register unclaimed agent-doc files ---
    // For unresolved files (have session UUID but no pane), find a donor pane in the
    // SAME column only. This is ephemeral — not written to sessions.json.
    // Same-column-only prevents cross-column focus jumps.
    for file in &unresolved_files {
        if let Some(donor_pane) = find_column_pane(&layout, file, &file_to_pane) {
            eprintln!(
                "auto-register {} → {} (in-memory, same column)",
                file.display(),
                donor_pane
            );
            file_to_pane.insert(file.clone(), donor_pane.clone());
            resolved.push(ResolvedFile {
                path: file.clone(),
                pane_id: donor_pane,
            });
        }
    }

    if resolved.len() < 2 {
        // Not enough resolved panes for 2D layout — just focus, don't rearrange.
        // Respect --focus: only select a pane if the focus file has a resolved pane.
        // Otherwise, preserve the current tmux selection.
        if let Some(focus_file) = focus {
            let focus_path = PathBuf::from(focus_file);
            if !non_agent_doc_files.contains(&focus_path)
                && let Some(pane) = file_to_pane.get(&focus_path) {
                    tmux.select_pane(pane)?;
                }
            // else: non-agent-doc or no pane → preserve selection
        } else if let Some(r) = resolved.first() {
            tmux.select_pane(&r.pane_id)?;
        }
        return Ok(());
    }

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

    // --- Phase 4: Pick target window (same tmux session only) ---
    // Priority: 1) tmux_session from frontmatter, 2) --window param, 3) first wanted pane
    let target_session = if let Some(ref ts) = doc_tmux_session {
        if tmux.session_alive(ts) {
            Some(ts.clone())
        } else {
            eprintln!(
                "warning: configured tmux_session '{}' is dead, falling back to --window",
                ts
            );
            if let Some(w) = window {
                tmux.pane_session(w).ok()
            } else {
                wanted.iter().find_map(|p| tmux.pane_session(p).ok())
            }
        }
    } else if let Some(w) = window {
        tmux.pane_session(w).ok()
    } else {
        wanted.iter().find_map(|p| tmux.pane_session(p).ok())
    };
    eprintln!("target_session={:?}", target_session);

    // If --window is alive, use it directly (stable, deterministic).
    // Only search for best_window when --window is missing or dead.
    let target_window = if let Some(w) = window {
        if !tmux.list_window_panes(w).unwrap_or_default().is_empty() {
            eprintln!("target_window={} (from --window)", w);
            w.to_string()
        } else {
            eprintln!("warning: --window {} is dead, searching for best window", w);
            find_best_window(tmux, &wanted, target_session.as_deref())
        }
    } else {
        find_best_window(tmux, &wanted, target_session.as_deref())
    };

    // Write tmux_session back to documents that don't have it yet
    if let Some(ref session_name) = target_session {
        for file in &files_needing_tmux_session {
            if let Ok(content) = std::fs::read_to_string(file)
                && let Ok(updated) = frontmatter::set_tmux_session(&content, session_name)
                    && updated != content {
                        if let Err(e) = std::fs::write(file, &updated) {
                            eprintln!("warning: failed to write tmux_session to {}: {}", file.display(), e);
                        } else {
                            eprintln!("set tmux_session={} in {}", session_name, file.display());
                        }
                    }
        }
    }

    let anchor_pane = pane_columns[0][0].clone();

    let desired_ordered: Vec<&str> = pane_columns
        .iter()
        .flat_map(|col| col.iter().map(|s| s.as_str()))
        .collect();

    // Resolve --focus to a pane ID (Option: None preserves current tmux selection)
    let focus_pane: Option<String> = if let Some(focus_file) = focus {
        let focus_path = PathBuf::from(focus_file);
        if non_agent_doc_files.contains(&focus_path) {
            // Non-agent-doc file → preserve tmux selection
            None
        } else if let Some(pane) = file_to_pane.get(&focus_path) {
            // Directly resolved (includes auto-registered)
            Some(pane.clone())
        } else {
            // Column-positional fallback: find first pane in same column
            find_column_pane(&layout, &focus_path, &file_to_pane)
        }
    } else {
        Some(anchor_pane.clone())
    };

    // --- Phase 5: Reconcile (attach-first: attach → select → detach) ---
    let log = reconcile(
        tmux,
        &target_window,
        &pane_columns,
        &desired_ordered,
        target_session.as_deref(),
        focus_pane.as_deref(),
    )?;

    // --- Phase 6: Resize + re-select ---
    // Reconcile already selected focus_pane before detach.
    // Equalize sizes, then re-confirm selection.
    equalize_sizes(tmux, &pane_columns);
    if let Some(ref fp) = focus_pane {
        tmux.select_pane(fp)?;
    }
    let sel = target_session.as_deref().and_then(|s| tmux.active_pane(s)).unwrap_or_default();
    eprintln!("phase6: focus={:?}, selected={}", focus_pane, sel);

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
/// # Algorithm: Attach-first (prevents pane selection flicker)
///
/// ```text
/// SNAPSHOT — query current panes
/// FAST PATH — if already correct, done
/// ATTACH — join missing desired panes into target window (all with -d)
/// SELECT — select the focus pane (now in window after attach)
/// DETACH — stash unwanted panes (focus pane survives, no selection jump)
/// REORDER — if needed, break non-first + rejoin in correct order
/// VERIFY — confirm final layout
/// ```
///
/// By attaching BEFORE detaching, the focus pane is in the window when
/// unwanted panes are stashed, preventing tmux from auto-selecting a
/// different pane.
fn reconcile(
    tmux: &Tmux,
    target_window: &str,
    pane_columns: &[Vec<String>],
    desired_ordered: &[&str],
    session_name: Option<&str>,
    focus_pane: Option<&str>,
) -> Result<SyncLog> {
    let mut log = SyncLog::new();

    let wanted: HashSet<&str> = desired_ordered.iter().copied().collect();
    let first_pane = desired_ordered[0];

    // --- SNAPSHOT ---
    let current = tmux.list_panes_ordered(target_window).unwrap_or_default();
    let current_refs: Vec<&str> = current.iter().map(|s| s.as_str()).collect();
    let sel = session_name.and_then(|s| tmux.active_pane(s)).unwrap_or_default();
    log.log(
        "SNAPSHOT",
        format!(
            "window={}, current={:?}, desired={:?}, selected={}",
            target_window, current_refs, desired_ordered, sel
        ),
    );

    // --- FAST PATH ---
    if current_refs == desired_ordered {
        log.log("FAST_PATH", "layout already correct");
        return Ok(log);
    }

    let current_panes = tmux.list_window_panes(target_window).unwrap_or_default();
    let current_set: HashSet<String> = current_panes.iter().cloned().collect();

    // --- ATTACH missing desired panes ---
    // First, ensure the first desired pane is in the target window.
    if !current_set.contains(first_pane) {
        let existing = tmux.list_window_panes(target_window).unwrap_or_default();
        if let Some(target) = existing.first() {
            match tmux.join_pane(first_pane, target, "-dbh") {
                Ok(()) => {
                    log.log("ATTACH", format!("joined first {} into {}", first_pane, target_window));
                    update_registry(tmux, first_pane, &mut log);
                }
                Err(e) => {
                    log.log_err("ATTACH", format!("failed to join first {}: {}", first_pane, e));
                }
            }
        }
    }

    // Join remaining desired panes in column order
    for (col_idx, column) in pane_columns.iter().enumerate() {
        for (row_idx, pane) in column.iter().enumerate() {
            if pane.as_str() == first_pane {
                continue;
            }
            let in_target = tmux
                .list_window_panes(target_window)
                .unwrap_or_default()
                .contains(pane);
            if in_target {
                continue;
            }
            if tmux.pane_window(pane).is_err() {
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

    let sel = session_name.and_then(|s| tmux.active_pane(s)).unwrap_or_default();
    log.log("ATTACH", format!("done — selected={}", sel));

    // --- SELECT focus pane (before detach, so stash won't change selection) ---
    let select_target = focus_pane.unwrap_or(first_pane);
    let _ = tmux.select_pane(select_target);
    log.log("SELECT", format!("pre-selected {} before detach", select_target));

    // --- DETACH unwanted panes ---
    let refreshed = tmux.list_window_panes(target_window).unwrap_or_default();
    for pane in &refreshed {
        if wanted.contains(pane.as_str()) {
            continue;
        }
        let window_count = tmux.list_window_panes(target_window).unwrap_or_default().len();
        if window_count <= 1 {
            log.log("DETACH", format!("skipped {} — last pane in window", pane));
            continue;
        }
        let (result, verb) = if let Some(sess) = session_name {
            (tmux.stash_pane(pane, sess), "stashed")
        } else {
            (tmux.break_pane(pane), "broke")
        };
        match result {
            Ok(()) => {
                log.log("DETACH", format!("{} {} from {}", verb, pane, target_window));
                update_registry(tmux, pane, &mut log);
            }
            Err(e) => {
                log.log_err("DETACH", format!("failed to detach {}: {}", pane, e));
            }
        }
    }

    // Re-select target window after stash operations
    let _ = tmux.select_window(target_window);
    let sel = session_name.and_then(|s| tmux.active_pane(s)).unwrap_or_default();
    log.log("DETACH", format!("done — selected={}", sel));

    // --- REORDER if needed ---
    let current_order = tmux.list_panes_ordered(target_window).unwrap_or_default();
    let current_order_refs: Vec<&str> = current_order.iter().map(|s| s.as_str()).collect();
    let final_set: HashSet<&str> = current_order_refs.iter().copied().collect();

    if final_set == wanted && current_order_refs != desired_ordered {
        log.log("REORDER", format!("current={:?}, desired={:?}", current_order_refs, desired_ordered));
        for pane in desired_ordered.iter().skip(1) {
            let _ = tmux.break_pane(pane);
            log.log("REORDER", format!("broke {} for reorder", pane));
        }
        for (col_idx, column) in pane_columns.iter().enumerate() {
            for (row_idx, pane) in column.iter().enumerate() {
                if pane.as_str() == first_pane {
                    continue;
                }
                let in_target = tmux
                    .list_window_panes(target_window)
                    .unwrap_or_default()
                    .contains(pane);
                if in_target {
                    continue;
                }
                let (target_pane, flag) = join_target(pane_columns, col_idx, row_idx);
                let _ = tmux.join_pane(pane, &target_pane, flag);
                log.log("REORDER", format!("rejoined {} → {} ({})", pane, target_pane, flag));
            }
        }
        // Re-select focus after reorder
        let _ = tmux.select_pane(select_target);
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
/// All flags include `-d` to prevent changing the active pane during reconcile.
fn join_target(pane_columns: &[Vec<String>], col_idx: usize, row_idx: usize) -> (String, &'static str) {
    if col_idx == 0 {
        // Same column as anchor: stack below previous pane
        (pane_columns[0][row_idx - 1].clone(), "-dv")
    } else if row_idx == 0 {
        // First pane of new column: horizontal split right of previous column's first pane
        (pane_columns[col_idx - 1][0].clone(), "-dh")
    } else {
        // Stack below previous pane in this column
        (pane_columns[col_idx][row_idx - 1].clone(), "-dv")
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
    } else if pane_columns.len() > 2
        && let Ok(win) = tmux.pane_window(&pane_columns[0][0]) {
            let _ = tmux.select_layout(&win, "even-horizontal");
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

    // --- Helpers ---

    /// Full tmux state snapshot for assertions.
    #[derive(Debug)]
    struct TmuxState {
        /// Panes in the target window (ordered).
        target_panes: Vec<String>,
        /// Total windows in the session.
        window_count: usize,
        /// Currently selected window ID.
        active_window: String,
    }

    /// Capture the full tmux state for a session.
    fn snapshot_state(tmux: &IsolatedTmux, session: &str, target_window: &str) -> TmuxState {
        let target_panes = tmux.list_panes_ordered(target_window).unwrap_or_default();
        let window_count = count_windows(tmux, session);
        let active_window = active_window(tmux, session);
        TmuxState {
            target_panes,
            window_count,
            active_window,
        }
    }

    /// Assert the target window contains exactly the expected panes (in order).
    fn assert_target_panes(state: &TmuxState, expected: &[&str], msg: &str) {
        let actual: Vec<&str> = state.target_panes.iter().map(|s| s.as_str()).collect();
        assert_eq!(actual, expected, "{}: target panes mismatch", msg);
    }

    /// Assert the active window is the target window.
    fn assert_active_window(state: &TmuxState, target_window: &str, msg: &str) {
        assert_eq!(
            state.active_window, target_window,
            "{}: active window should be target",
            msg
        );
    }

    /// Assert that all given panes are alive.
    fn assert_all_alive(tmux: &IsolatedTmux, panes: &[String], msg: &str) {
        for pane in panes {
            assert!(tmux.pane_alive(pane), "{}: pane {} should be alive", msg, pane);
        }
    }

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        // Snapshot-based state assertions
        let state = snapshot_state(&t, "test", &target_window);
        assert_target_panes(&state, &desired, "2col happy path");
        assert_active_window(&state, &target_window, "2col happy path");
        // 3 panes started in 3 windows; after joining 2 into target, only 1 window remains
        assert_eq!(state.window_count, 1, "2col happy path: all panes consolidated into 1 window");
        assert_all_alive(&t, &panes, "2col happy path");
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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

        assert!(
            log.entries().iter().any(|e| e.phase == "FAST_PATH"),
            "should take fast path"
        );
        assert_eq!(log.mutation_count(), 0, "should have zero mutations");

        // Snapshot-based state assertions
        let state = snapshot_state(&t, "test", &target_window);
        assert_target_panes(&state, &desired, "already correct");
        assert_active_window(&state, &target_window, "already correct");
        assert_eq!(state.window_count, 1, "already correct: window count unchanged");
        assert_all_alive(&t, &[pane_a, pane_b], "already correct");
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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        // Snapshot-based state assertions
        let state = snapshot_state(&t, "test", &target_window);
        assert_target_panes(&state, &desired, "unwanted evicted");
        assert_active_window(&state, &target_window, "unwanted evicted");
        // Started with 1 window (all panes via split). X evicted to solo window → 2 windows.
        assert_eq!(state.window_count, 2, "unwanted evicted: target + X's solo window");
    }

    #[test]
    fn test_sync_missing_pane_joined() {
        let t = IsolatedTmux::new("sync-test-missing-join");
        let (target_window, panes, _tmp) = setup_panes(&t, 2);

        // panes[0] is in target_window. panes[1] is in another window.
        let pane_columns = vec![vec![panes[0].clone()], vec![panes[1].clone()]];
        let desired: Vec<&str> = vec![panes[0].as_str(), panes[1].as_str()];

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        // Snapshot-based state assertions
        let state = snapshot_state(&t, "test", &target_window);
        assert_target_panes(&state, &desired, "mixed evict and join");
        assert_active_window(&state, &target_window, "mixed evict and join");
        // Started: 2 windows (target with A+X, B solo). After: target (A+B), X solo → 2 windows.
        assert_eq!(state.window_count, 2, "mixed evict and join: target + X's solo window");
        assert_all_alive(&t, &[pane_a, pane_b], "mixed evict and join");
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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        // Snapshot-based state assertions
        let state = snapshot_state(&t, "test", &target_window);
        assert_target_panes(&state, &desired, "3 to 2 evict");
        assert_active_window(&state, &target_window, "3 to 2 evict");
        // Started: 1 window (all splits). C evicted to solo → 2 windows.
        assert_eq!(state.window_count, 2, "3 to 2 evict: target + C's solo window");
        assert_all_alive(&t, &[pane_a, pane_b], "3 to 2 evict");
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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

        let log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

        let final_panes = t.list_window_panes(&target_window).unwrap();
        assert!(final_panes.contains(&pane_a), "A in target");
        assert!(final_panes.contains(&pane_b), "B in target");
        assert!(final_panes.contains(&pane_d), "D in target");
        assert!(!final_panes.contains(&pane_c), "C evicted from target");
        assert!(t.pane_alive(&pane_c), "C still alive");
        assert!(!log.has_errors(), "no errors: {:?}", log.entries());

        // Snapshot-based state assertions
        let state = snapshot_state(&t, "test", &target_window);
        assert_target_panes(&state, &desired, "full real world");
        assert_active_window(&state, &target_window, "full real world");
        // Started: 3 windows (A+C, B, D). After: target (A+B+D), C solo → 2 windows.
        assert_eq!(state.window_count, 2, "full real world: target + C's solo window");
        assert_all_alive(&t, &[pane_a.clone(), pane_b, pane_d], "full real world");
    }

    #[test]
    fn test_reconcile_stable_across_repeated_syncs() {
        // Bug reproduction: switching between two files in the editor
        // caused window cycling (0→1→2→0...) because reconcile would
        // break/join panes each time, creating temporary windows.
        // After fix: repeated reconcile with the same desired layout
        // should be idempotent — no new windows, same target.
        let t = IsolatedTmux::new("sync-test-stable-repeat");
        let _tmp = TempDir::new().unwrap();

        let pane_a = t.new_session("test", _tmp.path()).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-x", "200", "-y", "60"]);
        let pane_b = t.new_window("test", _tmp.path()).unwrap();

        // Start: A in one window, B in another
        let target_window = t.pane_window(&pane_a).unwrap();

        let pane_columns = vec![vec![pane_a.clone(), pane_b.clone()]];
        let desired: Vec<&str> = vec![pane_a.as_str(), pane_b.as_str()];

        // First reconcile: should consolidate A and B into target_window
        let log1 = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();
        let panes1 = t.list_window_panes(&target_window).unwrap();
        assert_eq!(panes1.len(), 2, "first sync: 2 panes in target");
        assert!(panes1.contains(&pane_a), "first sync: A present");
        assert!(panes1.contains(&pane_b), "first sync: B present");
        assert!(!log1.has_errors(), "first sync: no errors");

        // Verify active window after first sync
        let state1 = snapshot_state(&t, "test", &target_window);
        assert_active_window(&state1, &target_window, "first sync");

        // Count total windows in the session
        let windows_after_1 = t
            .raw_cmd(&["list-windows", "-t", "test", "-F", "#{window_id}"])
            .unwrap();
        let win_count_1 = windows_after_1.lines().count();

        // Second reconcile with SAME layout — should be a no-op
        let log2 = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();
        let panes2 = t.list_window_panes(&target_window).unwrap();
        assert_eq!(panes2.len(), 2, "second sync: still 2 panes");
        assert!(panes2.contains(&pane_a), "second sync: A present");
        assert!(panes2.contains(&pane_b), "second sync: B present");
        assert!(!log2.has_errors(), "second sync: no errors");
        assert_eq!(
            log2.mutation_count(),
            0,
            "second sync: no mutations (idempotent)"
        );

        // Verify no new windows were created
        let windows_after_2 = t
            .raw_cmd(&["list-windows", "-t", "test", "-F", "#{window_id}"])
            .unwrap();
        let win_count_2 = windows_after_2.lines().count();
        assert_eq!(
            win_count_1, win_count_2,
            "no new windows from idempotent sync"
        );

        // Verify active window after second sync
        let state2 = snapshot_state(&t, "test", &target_window);
        assert_active_window(&state2, &target_window, "second sync");

        // Third reconcile — still stable
        let log3 = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();
        assert_eq!(
            log3.mutation_count(),
            0,
            "third sync: still no mutations"
        );

        // Verify active window after third sync
        let state3 = snapshot_state(&t, "test", &target_window);
        assert_active_window(&state3, &target_window, "third sync");
    }

    #[test]
    fn test_reconcile_tab_switching_no_window_cycling() {
        // Bug reproduction: switching between two documents in the editor
        // causes window cycling (0→1→2→0...) because each sync evicts
        // the unwanted pane via break_pane into a NEW solo window.
        //
        // Scenario: 3 panes (A=agent-doc, B=plugin, C=dave-franklin).
        // Editor alternates between [A,C] and [B,C] as user switches tabs.
        // With stash window: evicted panes go to stash, no new windows created.
        let t = IsolatedTmux::new("sync-test-tab-cycling");
        let tmp = TempDir::new().unwrap();

        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-x", "200", "-y", "60"]);
        let pane_b = t.new_window("test", tmp.path()).unwrap();
        let pane_c = t.new_window("test", tmp.path()).unwrap();

        // Pick A's window as canonical
        let target_window = t.pane_window(&pane_a).unwrap();

        // --- Sync 1: layout = [A, C] ---
        let cols1 = vec![vec![pane_a.clone(), pane_c.clone()]];
        let desired1: Vec<&str> = vec![pane_a.as_str(), pane_c.as_str()];
        let log1 = reconcile(&t, &target_window, &cols1, &desired1, Some("test"), None).unwrap();
        assert!(!log1.has_errors(), "sync1 errors: {:?}", log1.entries());
        let panes1 = t.list_window_panes(&target_window).unwrap();
        assert!(panes1.contains(&pane_a), "sync1: A in target");
        assert!(panes1.contains(&pane_c), "sync1: C in target");
        assert!(!panes1.contains(&pane_b), "sync1: B not in target");

        // Verify active window after sync 1
        let state1 = snapshot_state(&t, "test", &target_window);
        assert_active_window(&state1, &target_window, "sync1");

        // Count windows after sync 1
        let win_count_1 = count_windows(&t, "test");

        // --- Sync 2: layout = [B, C] (user switched left tab) ---
        let cols2 = vec![vec![pane_b.clone(), pane_c.clone()]];
        let desired2: Vec<&str> = vec![pane_b.as_str(), pane_c.as_str()];
        let log2 = reconcile(&t, &target_window, &cols2, &desired2, Some("test"), None).unwrap();
        assert!(!log2.has_errors(), "sync2 errors: {:?}", log2.entries());
        let panes2 = t.list_window_panes(&target_window).unwrap();
        assert!(panes2.contains(&pane_b), "sync2: B in target");
        assert!(panes2.contains(&pane_c), "sync2: C in target");
        assert!(!panes2.contains(&pane_a), "sync2: A not in target");

        // Verify active window after sync 2
        let state2 = snapshot_state(&t, "test", &target_window);
        assert_active_window(&state2, &target_window, "sync2");

        // A should be in the stash, NOT in a new solo window
        let win_count_2 = count_windows(&t, "test");
        // Allow at most +1 window (the stash). No growing.
        assert!(
            win_count_2 <= win_count_1 + 1,
            "sync2: windows should not grow unbounded ({} → {})",
            win_count_1,
            win_count_2
        );

        // --- Sync 3: back to [A, C] (user switched back) ---
        let log3 = reconcile(&t, &target_window, &cols1, &desired1, Some("test"), None).unwrap();
        assert!(!log3.has_errors(), "sync3 errors: {:?}", log3.entries());
        let panes3 = t.list_window_panes(&target_window).unwrap();
        assert!(panes3.contains(&pane_a), "sync3: A in target");
        assert!(panes3.contains(&pane_c), "sync3: C in target");

        // Verify active window after sync 3
        let state3 = snapshot_state(&t, "test", &target_window);
        assert_active_window(&state3, &target_window, "sync3");

        let win_count_3 = count_windows(&t, "test");
        // Window count must NOT keep growing with each switch
        assert_eq!(
            win_count_2, win_count_3,
            "sync3: no new windows from switching back ({} → {})",
            win_count_2, win_count_3
        );

        // --- Sync 4: [B, C] again ---
        let log4 = reconcile(&t, &target_window, &cols2, &desired2, Some("test"), None).unwrap();
        assert!(!log4.has_errors(), "sync4 errors: {:?}", log4.entries());

        // Verify active window after sync 4
        let state4 = snapshot_state(&t, "test", &target_window);
        assert_active_window(&state4, &target_window, "sync4");

        let win_count_4 = count_windows(&t, "test");
        assert_eq!(
            win_count_3, win_count_4,
            "sync4: window count stable ({} → {})",
            win_count_3, win_count_4
        );

        // All panes still alive (no process killed)
        assert!(t.pane_alive(&pane_a), "A still alive");
        assert!(t.pane_alive(&pane_b), "B still alive");
        assert!(t.pane_alive(&pane_c), "C still alive");

        // Verify the TARGET WINDOW stays selected throughout all syncs
        // (the window selection should never cycle to stash or other windows)
        let active_win = active_window(&t, "test");
        assert_eq!(
            active_win, target_window,
            "target window should be selected after all syncs"
        );
    }

    /// Count windows in a tmux session.
    fn count_windows(tmux: &IsolatedTmux, session: &str) -> usize {
        tmux.raw_cmd(&["list-windows", "-t", session, "-F", "#{window_id}"])
            .unwrap_or_default()
            .lines()
            .count()
    }

    /// Get the active window ID in a tmux session.
    fn active_window(tmux: &IsolatedTmux, session: &str) -> String {
        tmux.raw_cmd(&[
            "display-message",
            "-t",
            session,
            "-p",
            "#{window_id}",
        ])
        .unwrap_or_default()
        .trim()
        .to_string()
    }

    /// Get the active pane ID in a tmux session.
    fn active_pane(tmux: &IsolatedTmux, session: &str) -> String {
        tmux.raw_cmd(&[
            "display-message",
            "-t",
            session,
            "-p",
            "#{pane_id}",
        ])
        .unwrap_or_default()
        .trim()
        .to_string()
    }

    #[test]
    fn test_reconcile_2col_tab_switch_selects_left_pane() {
        // Bug reproduction: 2-column editor layout. User switches between
        // agent-doc.md and plugin.md on the LEFT side. dave-franklin.md
        // stays on the RIGHT. After each switch, the LEFT pane should be selected.
        //
        // Layout 1: [A] [C]  (agent-doc left, dave-franklin right)
        // Layout 2: [B] [C]  (plugin left, dave-franklin right)
        // In both cases, the focus file is the left-column file.
        let t = IsolatedTmux::new("sync-test-2col-focus");
        let tmp = TempDir::new().unwrap();

        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-x", "200", "-y", "60"]);
        let pane_b = t.new_window("test", tmp.path()).unwrap();
        let pane_c = t.new_window("test", tmp.path()).unwrap();

        let target_window = t.pane_window(&pane_a).unwrap();

        // --- Sync 1: layout = [[A], [C]], focus = A ---
        let cols1 = vec![vec![pane_a.clone()], vec![pane_c.clone()]];
        let desired1: Vec<&str> = vec![pane_a.as_str(), pane_c.as_str()];
        let log1 = reconcile(&t, &target_window, &cols1, &desired1, Some("test"), Some(&pane_a)).unwrap();
        assert!(!log1.has_errors(), "sync1 errors: {:?}", log1.entries());
        let sel1 = active_pane(&t, "test");
        assert_eq!(sel1, pane_a, "sync1: A (left) should be selected after reconcile");

        // --- Sync 2: layout = [[B], [C]], focus = B ---
        let cols2 = vec![vec![pane_b.clone()], vec![pane_c.clone()]];
        let desired2: Vec<&str> = vec![pane_b.as_str(), pane_c.as_str()];
        let log2 = reconcile(&t, &target_window, &cols2, &desired2, Some("test"), Some(&pane_b)).unwrap();
        assert!(!log2.has_errors(), "sync2 errors: {:?}", log2.entries());
        // After reconcile: focus pane (B) should already be selected (attach→select→detach)
        let sel2 = active_pane(&t, "test");
        assert_eq!(sel2, pane_b, "sync2: B (left) should be selected after reconcile");

        // Verify B is actually on the left (first in pane order)
        let ordered = t.list_panes_ordered(&target_window).unwrap();
        assert_eq!(ordered[0], pane_b, "sync2: B should be leftmost pane");
        assert_eq!(ordered[1], pane_c, "sync2: C should be rightmost pane");

        // --- Sync 3: back to [[A], [C]], focus = A ---
        let log3 = reconcile(&t, &target_window, &cols1, &desired1, Some("test"), Some(&pane_a)).unwrap();
        assert!(!log3.has_errors(), "sync3 errors: {:?}", log3.entries());
        let sel3 = active_pane(&t, "test");
        assert_eq!(sel3, pane_a, "sync3: A (left) should be selected after reconcile");

        let ordered3 = t.list_panes_ordered(&target_window).unwrap();
        assert_eq!(ordered3[0], pane_a, "sync3: A should be leftmost pane");
        assert_eq!(ordered3[1], pane_c, "sync3: C should be rightmost pane");

        // --- Sync 4: [[B], [C]] again ---
        let log4 = reconcile(&t, &target_window, &cols2, &desired2, Some("test"), Some(&pane_b)).unwrap();
        assert!(!log4.has_errors(), "sync4 errors: {:?}", log4.entries());
        let sel4 = active_pane(&t, "test");
        assert_eq!(sel4, pane_b, "sync4: B (left) should be selected after reconcile");

        let ordered4 = t.list_panes_ordered(&target_window).unwrap();
        assert_eq!(ordered4[0], pane_b, "sync4: B should be leftmost pane");
        assert_eq!(ordered4[1], pane_c, "sync4: C should be rightmost pane");

        // All alive
        assert_all_alive(&t, &[pane_a.clone(), pane_b, pane_c], "end");

        // Window count stable
        let win_count = count_windows(&t, "test");
        // Expect: target + stash (2), possibly + dead B/A window shells
        assert!(win_count <= 3, "at most 3 windows: target + stash + 1 shell");
    }

    #[test]
    fn test_full_flow_2col_tab_switch_pane_selection() {
        // Bug reproduction: full sync flow (reconcile + equalize_sizes + select_pane).
        // When switching between 2 left-column files, the LEFT pane should remain
        // selected after the full flow — including equalize_sizes which might
        // interfere with pane selection.
        let t = IsolatedTmux::new("sync-full-flow");
        let tmp = TempDir::new().unwrap();

        let pane_a = t.new_session("test", tmp.path()).unwrap();
        let _ = t.raw_cmd(&["resize-window", "-x", "200", "-y", "60"]);
        let pane_b = t.new_window("test", tmp.path()).unwrap();
        let pane_c = t.new_window("test", tmp.path()).unwrap();

        let target_window = t.pane_window(&pane_a).unwrap();

        // Helper: full sync flow (reconcile with focus + equalize_sizes + select_pane)
        let full_sync = |cols: &[Vec<String>], desired: &[&str], focus: &str, label: &str| {
            // reconcile now includes SELECT before DETACH, so focus pane
            // should already be selected after reconcile returns.
            let log = reconcile(&t, &target_window, cols, desired, Some("test"), Some(focus)).unwrap();
            assert!(!log.has_errors(), "{}: reconcile errors: {:?}", label, log.entries());

            let sel_after_reconcile = active_pane(&t, "test");
            eprintln!("{}: after reconcile, selected={}", label, sel_after_reconcile);
            // Key assertion: reconcile should have already selected the focus pane
            assert_eq!(sel_after_reconcile, focus,
                "{}: reconcile should pre-select focus pane (attach→select→detach)", label);

            equalize_sizes(&t, cols);

            let sel_after_equalize = active_pane(&t, "test");
            eprintln!("{}: after equalize_sizes, selected={}", label, sel_after_equalize);
            // equalize_sizes should NOT change the selected pane
            assert_eq!(sel_after_equalize, focus,
                "{}: equalize_sizes should not change selected pane", label);

            // Final select_pane (as sync_layout does)
            t.select_pane(focus).unwrap();
            let sel_final = active_pane(&t, "test");
            assert_eq!(sel_final, focus, "{}: final selected pane", label);

            // Verify the focus pane is in the target window
            let ordered = t.list_panes_ordered(&target_window).unwrap();
            assert!(ordered.contains(&focus.to_string()),
                "{}: focus pane {} not in target window {:?}", label, focus, ordered);
        };

        // --- Sync 1: [[A], [C]], focus = A (left) ---
        let cols1 = vec![vec![pane_a.clone()], vec![pane_c.clone()]];
        let desired1: Vec<&str> = vec![pane_a.as_str(), pane_c.as_str()];
        full_sync(&cols1, &desired1, &pane_a, "sync1");

        // Verify A is leftmost
        let ordered1 = t.list_panes_ordered(&target_window).unwrap();
        assert_eq!(ordered1[0], pane_a, "sync1: A should be leftmost");

        // --- Sync 2: [[B], [C]], focus = B (left) ---
        let cols2 = vec![vec![pane_b.clone()], vec![pane_c.clone()]];
        let desired2: Vec<&str> = vec![pane_b.as_str(), pane_c.as_str()];
        full_sync(&cols2, &desired2, &pane_b, "sync2");

        let ordered2 = t.list_panes_ordered(&target_window).unwrap();
        assert_eq!(ordered2[0], pane_b, "sync2: B should be leftmost");

        // --- Sync 3: back to [[A], [C]], focus = A ---
        full_sync(&cols1, &desired1, &pane_a, "sync3");

        let ordered3 = t.list_panes_ordered(&target_window).unwrap();
        assert_eq!(ordered3[0], pane_a, "sync3: A should be leftmost");

        // --- Sync 4: [[B], [C]] again ---
        full_sync(&cols2, &desired2, &pane_b, "sync4");

        let ordered4 = t.list_panes_ordered(&target_window).unwrap();
        assert_eq!(ordered4[0], pane_b, "sync4: B should be leftmost");

        // All panes alive
        assert_all_alive(&t, &[pane_a, pane_b, pane_c], "end");
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

                let _log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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
                let _log1 = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

                // Second reconcile — should be fast path (zero mutations)
                let log2 = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

                let _log = reconcile(&t, &target_window, &pane_columns, &desired, None, None).unwrap();

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

    // --- Auto-register and focus resolution tests ---

    #[test]
    fn test_auto_register_shares_column_pane() {
        // Layout: col0=[a.md, b.md], col1=[c.md]
        // a.md resolved to %1, b.md unresolved, c.md resolved to %2
        // After find_column_pane, b.md should map to %1 (same column as a.md)
        let layout = Layout {
            columns: vec![
                Column {
                    files: vec![PathBuf::from("a.md"), PathBuf::from("b.md")],
                },
                Column {
                    files: vec![PathBuf::from("c.md")],
                },
            ],
        };

        let mut file_to_pane = std::collections::HashMap::new();
        file_to_pane.insert(PathBuf::from("a.md"), "%1".to_string());
        file_to_pane.insert(PathBuf::from("c.md"), "%2".to_string());

        let donor = find_column_pane(&layout, Path::new("b.md"), &file_to_pane);
        assert_eq!(donor, Some("%1".to_string()), "b.md should get a.md's pane (same column)");
    }

    #[test]
    fn test_no_cross_column_donor() {
        // Layout: col0=[a.md], col1=[b.md]
        // b.md unresolved, a.md resolved to %1. No resolved pane in col1.
        // Should NOT spiral to col0 — same-column only to prevent focus jumps.
        let layout = Layout {
            columns: vec![
                Column {
                    files: vec![PathBuf::from("a.md")],
                },
                Column {
                    files: vec![PathBuf::from("b.md")],
                },
            ],
        };

        let mut file_to_pane = std::collections::HashMap::new();
        file_to_pane.insert(PathBuf::from("a.md"), "%1".to_string());

        let donor = find_column_pane(&layout, Path::new("b.md"), &file_to_pane);
        assert_eq!(donor, None, "should NOT fall back to adjacent column");
    }

    #[test]
    fn test_focus_non_agent_doc_preserves_selection() {
        // find_column_pane returns None for a file not in layout
        let layout = Layout {
            columns: vec![Column {
                files: vec![PathBuf::from("a.md")],
            }],
        };
        let file_to_pane = std::collections::HashMap::new();
        let result = find_column_pane(&layout, Path::new("readme.txt"), &file_to_pane);
        assert_eq!(result, None, "non-layout file should return None");
    }

    #[test]
    fn test_focus_column_positional_fallback() {
        // Layout: col0=[a.md, b.md], col1=[c.md]
        // a.md resolved to %1, b.md NOT in file_to_pane
        // find_column_pane for b.md should return %1 (first resolved pane in same column)
        let layout = Layout {
            columns: vec![
                Column {
                    files: vec![PathBuf::from("a.md"), PathBuf::from("b.md")],
                },
                Column {
                    files: vec![PathBuf::from("c.md")],
                },
            ],
        };

        let mut file_to_pane = std::collections::HashMap::new();
        file_to_pane.insert(PathBuf::from("a.md"), "%1".to_string());
        file_to_pane.insert(PathBuf::from("c.md"), "%2".to_string());

        let result = find_column_pane(&layout, Path::new("b.md"), &file_to_pane);
        assert_eq!(result, Some("%1".to_string()), "should fall back to a.md's pane in same column");
    }

    #[test]
    fn test_unclaimed_left_col_preserves_selection() {
        // Exact user scenario:
        // - 2-column layout: col0=[plugin.md], col1=[dave-franklin.md]
        // - plugin.md has agent_doc_session but no registered pane (unclaimed)
        // - dave-franklin.md has a registered pane (right column)
        // - Focus: plugin.md
        // Expected: tmux selection stays on the LEFT pane (preserve selection)
        //
        // This is a unit test of the focus resolution + early return logic.
        // When plugin.md has no same-column donor, resolved.len() < 2 triggers
        // early return. The early return should preserve selection, NOT select
        // dave-franklin's pane.
        let layout = Layout {
            columns: vec![
                Column {
                    files: vec![PathBuf::from("plugin.md")],
                },
                Column {
                    files: vec![PathBuf::from("dave-franklin.md")],
                },
            ],
        };

        let mut file_to_pane = std::collections::HashMap::new();
        file_to_pane.insert(PathBuf::from("dave-franklin.md"), "%65".to_string());

        // Phase 1.5: find_column_pane for plugin.md in col0 — no donor
        let donor = find_column_pane(&layout, Path::new("plugin.md"), &file_to_pane);
        assert_eq!(donor, None, "no donor in same column → no auto-register");

        // Focus resolution: plugin.md not in file_to_pane → column fallback
        let focus_pane = file_to_pane
            .get(&PathBuf::from("plugin.md"))
            .cloned()
            .or_else(|| find_column_pane(&layout, Path::new("plugin.md"), &file_to_pane));
        assert_eq!(focus_pane, None, "focus should be None → preserve tmux selection");
    }

    #[test]
    fn test_claimed_left_col_selects_left_pane() {
        // Counterpart: when agent-doc.md IS claimed (col0), focus should select it.
        let _layout = Layout {
            columns: vec![
                Column {
                    files: vec![PathBuf::from("agent-doc.md")],
                },
                Column {
                    files: vec![PathBuf::from("dave-franklin.md")],
                },
            ],
        };

        let mut file_to_pane = std::collections::HashMap::new();
        file_to_pane.insert(PathBuf::from("agent-doc.md"), "%39".to_string());
        file_to_pane.insert(PathBuf::from("dave-franklin.md"), "%65".to_string());

        let focus_pane = file_to_pane.get(&PathBuf::from("agent-doc.md")).cloned();
        assert_eq!(focus_pane, Some("%39".to_string()), "claimed left file → select left pane");
    }

    #[test]
    fn test_column_of() {
        let layout = Layout {
            columns: vec![
                Column {
                    files: vec![PathBuf::from("a.md"), PathBuf::from("b.md")],
                },
                Column {
                    files: vec![PathBuf::from("c.md")],
                },
            ],
        };
        assert_eq!(layout.column_of(Path::new("a.md")), Some(0));
        assert_eq!(layout.column_of(Path::new("b.md")), Some(0));
        assert_eq!(layout.column_of(Path::new("c.md")), Some(1));
        assert_eq!(layout.column_of(Path::new("d.md")), None);
    }
}
