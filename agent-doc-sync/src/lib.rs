//! Sync scope and layout-state planning policy.
//!
//! This crate owns path-list normalization and `.agent-doc` scope-root
//! selection for sync. Orchestration owns tmux calls, repair side effects,
//! document validation, and state-file IO.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const SYNC_FRONTMATTER_STATUS_PREFIX: &str = "[agent-doc sync] malformed frontmatter";
pub const SAFE_PASSIVE_SYNC_LOCK_SKIPPED_MARKER: &str =
    "[sync] safe_passive_sync_lock_contention_retry";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoStartMode {
    Full,
    SafePassive,
}

impl AutoStartMode {
    pub fn log_label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::SafePassive => "safe-passive",
        }
    }
}

/// Trim an optional CLI scope argument and treat empty strings as absent.
pub fn normalize_scope_arg(value: Option<&str>) -> Option<&str> {
    value.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

/// Return focused and column-projected candidate files in sync processing order.
pub fn sync_candidate_files(col_args: &[String], focus: Option<&str>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(focused) = focus.map(str::trim).filter(|path| !path.is_empty()) {
        files.push(PathBuf::from(focused));
    }
    files.extend(
        col_args
            .iter()
            .flat_map(|arg| arg.split(','))
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from),
    );
    files
}

/// Return existing candidate files in canonical form.
pub fn canonical_sync_candidate_files(col_args: &[String], focus: Option<&str>) -> Vec<PathBuf> {
    sync_candidate_files(col_args, focus)
        .into_iter()
        .filter(|path| path.exists())
        .map(|path| path.canonicalize().unwrap_or(path))
        .collect()
}

/// Return the nearest common directory ancestor for a set of paths.
pub fn common_ancestor_dir(paths: &[PathBuf]) -> Option<PathBuf> {
    let mut iter = paths.iter();
    let first = iter.next()?;
    let mut common = if first.is_dir() {
        first.clone()
    } else {
        first.parent()?.to_path_buf()
    };

    for path in iter {
        let other = if path.is_dir() {
            path.clone()
        } else {
            path.parent()?.to_path_buf()
        };
        while !other.starts_with(&common) {
            common = common.parent()?.to_path_buf();
        }
    }

    Some(common)
}

/// Return the `.agent-doc` root shared by the current sync candidate set.
pub fn shared_sync_scope_root(col_args: &[String], focus: Option<&str>) -> Option<PathBuf> {
    let files = canonical_sync_candidate_files(col_args, focus);
    let mut current = common_ancestor_dir(&files)?;
    loop {
        if current.join(".agent-doc").is_dir() {
            return Some(current);
        }
        current = current.parent()?.to_path_buf();
    }
}

/// Resolve the root used for sync layout and prune state.
pub fn sync_scope_root(col_args: &[String], focus: Option<&str>, cwd: &Path) -> Option<PathBuf> {
    shared_sync_scope_root(col_args, focus)
        .or_else(|| {
            focus
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .and_then(|path| agent_doc_fs::find_project_root(Path::new(path)))
        })
        .or_else(|| {
            agent_doc_fs::find_project_root(cwd)
                .or_else(|| cwd.join(".agent-doc").is_dir().then_some(cwd.to_path_buf()))
        })
}

/// Resolve the base directory used for sync layout state.
pub fn layout_state_scope_root(col_args: &[String], focus: Option<&str>, cwd: &Path) -> PathBuf {
    sync_scope_root(col_args, focus, cwd).unwrap_or_else(|| cwd.to_path_buf())
}

/// Resolve `.agent-doc/last_layout.json` for this sync invocation.
pub fn layout_state_path(col_args: &[String], focus: Option<&str>, cwd: &Path) -> PathBuf {
    layout_state_scope_root(col_args, focus, cwd)
        .join(".agent-doc")
        .join("last_layout.json")
}

/// Resolve `.agent-doc/sync-prune-state.json` for this sync invocation.
pub fn sync_prune_state_path(col_args: &[String], focus: Option<&str>, cwd: &Path) -> PathBuf {
    let base = sync_scope_root(col_args, focus, cwd).unwrap_or_else(|| cwd.to_path_buf());
    base.join(".agent-doc").join("sync-prune-state.json")
}

pub fn latency_budget_status(elapsed: Duration, budget: Duration) -> &'static str {
    if elapsed >= budget {
        "over_budget"
    } else {
        "ok"
    }
}

pub fn sync_latency_message(
    phase: &str,
    elapsed: Duration,
    budget: Duration,
    mode_label: &str,
) -> String {
    format!(
        "sync_latency phase={} elapsed_ms={} budget_ms={} status={} mode={}",
        phase,
        elapsed.as_millis(),
        budget.as_millis(),
        latency_budget_status(elapsed, budget),
        mode_label
    )
}

pub fn sync_frontmatter_status_message(phase: &str, err: &anyhow::Error) -> String {
    format!(
        "{} during {}.\n\n{}",
        SYNC_FRONTMATTER_STATUS_PREFIX, phase, err
    )
}

pub fn safe_passive_lock_contention_message(elapsed: Duration, budget: Duration) -> String {
    format!(
        "{} phase=sync_lock_wait elapsed_ms={} budget_ms={} status=over_budget coalesced=skipped_stale action=retry",
        SAFE_PASSIVE_SYNC_LOCK_SKIPPED_MARKER,
        elapsed.as_millis(),
        budget.as_millis()
    )
}

pub fn safe_passive_prune_cleanup_throttle() -> Duration {
    Duration::from_secs(2)
}

pub fn sanitize_stamp_component(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn sync_repair_stamp_filename(server_socket: Option<&str>, session_name: &str) -> String {
    let socket = sanitize_stamp_component(server_socket.unwrap_or("default"));
    let session = sanitize_stamp_component(session_name);
    format!("sync-repair-{socket}-{session}.stamp")
}

/// Resolve the destructive repair throttle stamp path for a server+session.
///
/// Missing `.agent-doc/` means there is no durable sync state directory for this
/// process root, so callers should skip throttling instead of inventing a path.
pub fn sync_repair_stamp_path(
    cwd: &Path,
    server_socket: Option<&str>,
    session_name: &str,
) -> Option<PathBuf> {
    let dir = cwd.join(".agent-doc");
    if !dir.is_dir() {
        return None;
    }
    Some(dir.join(sync_repair_stamp_filename(server_socket, session_name)))
}

pub fn rename_debounce_expired(age: Duration, ttl: Duration) -> bool {
    age >= ttl
}

pub fn auto_started_panes_summary(auto_started_panes: &[(String, String)]) -> Option<String> {
    if auto_started_panes.len() <= 1 {
        return None;
    }
    let summary = auto_started_panes
        .iter()
        .map(|(pane, file)| format!("{pane}→{file}"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "auto-started {} panes: {}",
        auto_started_panes.len(),
        summary
    ))
}

pub fn sanitize_excerpt(text: &str) -> Option<String> {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    let mut excerpt = collapsed;
    if excerpt.len() > 200 {
        excerpt.truncate(200);
        excerpt.push_str("...");
    }
    Some(excerpt)
}

pub fn last_visible_excerpt(capture: &str) -> Option<String> {
    capture
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("Pane is dead"))
        .and_then(sanitize_excerpt)
}

pub fn registry_relative_file_path(project_root: &Path, canonical_file: &Path) -> String {
    canonical_file
        .strip_prefix(project_root)
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| canonical_file.to_string_lossy().to_string())
}

pub fn sync_prune_fingerprint(col_args: &[String], window: Option<&str>) -> String {
    serde_json::json!({
        "window": window.unwrap_or(""),
        "columns": col_args,
    })
    .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct SyncPruneState {
    pub fingerprint: String,
    pub last_full_cleanup_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPruneStateUpdate {
    pub state: SyncPruneState,
    pub should_write: bool,
}

pub fn sync_prune_state_update(
    raw_state: Option<&str>,
    col_args: &[String],
    window: Option<&str>,
    now_ms: u64,
    throttle_ms: u64,
) -> SyncPruneStateUpdate {
    let fingerprint = sync_prune_fingerprint(col_args, window);
    let parsed = raw_state.and_then(|raw| serde_json::from_str::<SyncPruneState>(raw).ok());
    let fresh_unchanged = parsed.as_ref().is_some_and(|state| {
        state.fingerprint == fingerprint
            && now_ms.saturating_sub(state.last_full_cleanup_ms) < throttle_ms
    });

    if fresh_unchanged {
        SyncPruneStateUpdate {
            state: parsed.expect("fresh_unchanged requires parsed state"),
            should_write: false,
        }
    } else {
        SyncPruneStateUpdate {
            state: SyncPruneState {
                fingerprint,
                last_full_cleanup_ms: now_ms,
            },
            should_write: true,
        }
    }
}

pub fn planned_stash_window_indices(
    windows: &[(String, String, String)],
    is_stash_window_name: fn(&str) -> bool,
) -> Vec<(String, usize)> {
    windows
        .iter()
        .filter(|(_, _, name)| is_stash_window_name(name))
        .enumerate()
        .map(|(offset, (_, id, _))| (id.clone(), offset + 1))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowIndexNormalizationPlan {
    Missing,
    AlreadyAtIndex,
    Move {
        current_index: String,
        desired_index: String,
        current_name: String,
    },
    Swap {
        current_index: String,
        desired_index: String,
        current_name: String,
        occupant_id: String,
        occupant_name: String,
    },
}

pub fn plan_window_index_normalization(
    windows: &[(String, String, String)],
    window_id: &str,
    desired_index: usize,
) -> WindowIndexNormalizationPlan {
    let desired_index = desired_index.to_string();
    let Some((current_index, _, current_name)) =
        windows.iter().find(|(_, id, _)| id == window_id).cloned()
    else {
        return WindowIndexNormalizationPlan::Missing;
    };
    if current_index == desired_index {
        return WindowIndexNormalizationPlan::AlreadyAtIndex;
    }

    if let Some((_, occupant_id, occupant_name)) = windows
        .iter()
        .find(|(index, _, _)| index == &desired_index)
        .cloned()
    {
        if occupant_id == window_id {
            return WindowIndexNormalizationPlan::AlreadyAtIndex;
        }
        WindowIndexNormalizationPlan::Swap {
            current_index,
            desired_index,
            current_name,
            occupant_id,
            occupant_name,
        }
    } else {
        WindowIndexNormalizationPlan::Move {
            current_index,
            desired_index,
            current_name,
        }
    }
}

pub fn effective_sync_columns(
    col_args: &[String],
    saved_layout: &[String],
    layout_state_path: &Path,
) -> anyhow::Result<Vec<String>> {
    if !col_args.is_empty() {
        return Ok(col_args.to_vec());
    }

    if saved_layout.iter().all(|col| col.trim().is_empty()) {
        anyhow::bail!(
            "no sync columns provided and no recorded layout exists at {}",
            layout_state_path.display()
        );
    }

    Ok(saved_layout
        .iter()
        .map(|col| col.trim().to_string())
        .collect())
}

/// Detect whether a file has been renamed: the registered path differs from
/// the current path and the old path no longer exists on disk.
pub fn is_file_rename(registered_path: &str, current_path: &str) -> bool {
    registered_path != current_path && !Path::new(registered_path).exists()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResyncTargetMatcher {
    target: PathBuf,
    base_dir: PathBuf,
}

impl ResyncTargetMatcher {
    pub fn new(target: impl AsRef<Path>, base_dir: impl AsRef<Path>) -> Self {
        Self {
            target: target.as_ref().to_path_buf(),
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub fn same_document_path(&self, candidate: &str) -> bool {
        if candidate.is_empty() {
            return false;
        }
        let resolved = resolve_absolute_file_path(Path::new(candidate));
        let canonical = resolved.canonicalize().unwrap_or(resolved);
        canonical == self.target
    }

    pub fn candidate_matches_target(&self, candidate: &str) -> bool {
        if self.same_document_path(candidate) {
            return true;
        }
        if candidate.is_empty() {
            return false;
        }
        let path = Path::new(candidate);
        if path.is_absolute() {
            return false;
        }
        let resolved = self.base_dir.join(path);
        let canonical = resolved.canonicalize().unwrap_or(resolved);
        canonical == self.target
    }

    pub fn registry_file_for_target(&self) -> String {
        self.target
            .strip_prefix(&self.base_dir)
            .unwrap_or(&self.target)
            .to_string_lossy()
            .to_string()
    }
}

fn resolve_absolute_file_path(file: &Path) -> PathBuf {
    if file.is_absolute() {
        return file.to_path_buf();
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let candidate = cwd.join(file);
    if candidate.exists() {
        candidate.canonicalize().unwrap_or(candidate)
    } else {
        file.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_start_mode_reports_stable_log_labels() {
        assert_eq!(AutoStartMode::Full.log_label(), "full");
        assert_eq!(AutoStartMode::SafePassive.log_label(), "safe-passive");
    }

    #[test]
    fn normalize_scope_arg_trims_empty_values() {
        assert_eq!(normalize_scope_arg(None), None);
        assert_eq!(normalize_scope_arg(Some("")), None);
        assert_eq!(normalize_scope_arg(Some("   ")), None);
        assert_eq!(normalize_scope_arg(Some("@12")), Some("@12"));
        assert_eq!(normalize_scope_arg(Some("  @12  ")), Some("@12"));
    }

    #[test]
    fn sync_candidate_files_preserves_focus_then_columns() {
        let col_args = vec![
            "left.md, right.md".to_string(),
            "".to_string(),
            "  tail.md  ".to_string(),
        ];
        assert_eq!(
            sync_candidate_files(&col_args, Some(" focus.md ")),
            vec![
                PathBuf::from("focus.md"),
                PathBuf::from("left.md"),
                PathBuf::from("right.md"),
                PathBuf::from("tail.md"),
            ]
        );
    }

    #[test]
    fn layout_state_path_uses_shared_sync_scope_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let child = root.join("src/sample-app");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(child.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(child.join("tasks")).unwrap();

        let root_doc = root.join("tasks/root.md");
        let child_doc = child.join("tasks/child.md");
        std::fs::write(&root_doc, "---\nagent_doc_session: root\n---\n").unwrap();
        std::fs::write(&child_doc, "---\nagent_doc_session: child\n---\n").unwrap();

        let layout_path = layout_state_path(
            &[format!("{},{}", root_doc.display(), child_doc.display())],
            None,
            root,
        );
        assert_eq!(layout_path, root.join(".agent-doc/last_layout.json"));
    }

    #[test]
    fn effective_sync_columns_fall_back_to_recorded_layout() {
        let saved_layout = vec!["left.md".to_string(), "right.md".to_string()];
        let cols =
            effective_sync_columns(&[], &saved_layout, Path::new(".agent-doc/last_layout.json"))
                .expect("recorded layout should satisfy a no-col sync");
        assert_eq!(cols, saved_layout);
    }

    #[test]
    fn sync_latency_message_marks_budget_status() {
        let ok = sync_latency_message(
            "tmux_router",
            Duration::from_millis(999),
            Duration::from_secs(1),
            "safe-passive",
        );
        assert!(ok.contains("status=ok"), "{ok}");
        assert!(ok.contains("mode=safe-passive"), "{ok}");

        let slow = sync_latency_message(
            "safe_passive_total",
            Duration::from_secs(1),
            Duration::from_secs(1),
            "safe-passive",
        );
        assert!(slow.contains("status=over_budget"), "{slow}");
        assert!(slow.contains("elapsed_ms=1000"), "{slow}");

        let controller = sync_latency_message(
            "controller_actor_lookup",
            Duration::from_millis(251),
            Duration::from_millis(250),
            "safe-passive",
        );
        assert!(
            controller.contains("phase=controller_actor_lookup"),
            "{controller}"
        );
    }

    #[test]
    fn sync_frontmatter_status_message_includes_prefix_phase_and_error() {
        let err = anyhow::anyhow!("invalid YAML frontmatter in tasks/bad.md");
        let message = sync_frontmatter_status_message("auto-start", &err);

        assert!(
            message.starts_with(SYNC_FRONTMATTER_STATUS_PREFIX),
            "{message}"
        );
        assert!(
            message.contains("during auto-start.\n\ninvalid YAML frontmatter in tasks/bad.md"),
            "{message}"
        );
    }

    #[test]
    fn safe_passive_lock_contention_message_is_retryable_and_visible() {
        let message = safe_passive_lock_contention_message(
            Duration::from_millis(125),
            Duration::from_millis(100),
        );

        assert!(
            message.contains(SAFE_PASSIVE_SYNC_LOCK_SKIPPED_MARKER),
            "{message}"
        );
        assert!(message.contains("phase=sync_lock_wait"), "{message}");
        assert!(message.contains("elapsed_ms=125"), "{message}");
        assert!(message.contains("budget_ms=100"), "{message}");
        assert!(message.contains("status=over_budget"), "{message}");
        assert!(message.contains("coalesced=skipped_stale"), "{message}");
        assert!(message.contains("action=retry"), "{message}");
    }

    #[test]
    fn sanitize_stamp_component_replaces_path_separators() {
        assert_eq!(
            sanitize_stamp_component("/tmp/socket:name"),
            "_tmp_socket_name"
        );
        assert_eq!(sanitize_stamp_component("safe-name_1"), "safe-name_1");
    }

    #[test]
    fn sync_repair_stamp_filename_includes_sanitized_socket_and_session() {
        assert_eq!(
            sync_repair_stamp_filename(Some("/tmp/socket:name"), "agent doc/session"),
            "sync-repair-_tmp_socket_name-agent_doc_session.stamp"
        );
        assert_eq!(
            sync_repair_stamp_filename(None, "agent-doc"),
            "sync-repair-default-agent-doc.stamp"
        );
    }

    #[test]
    fn sync_repair_stamp_path_uses_default_socket_under_agent_doc_dir() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join(".agent-doc")).unwrap();

        assert_eq!(
            sync_repair_stamp_path(temp.path(), None, "agent-doc"),
            Some(
                temp.path()
                    .join(".agent-doc")
                    .join("sync-repair-default-agent-doc.stamp")
            )
        );
    }

    #[test]
    fn sync_repair_stamp_path_sanitizes_custom_socket_and_session() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join(".agent-doc")).unwrap();

        assert_eq!(
            sync_repair_stamp_path(temp.path(), Some("/tmp/socket:name"), "agent doc/session"),
            Some(
                temp.path()
                    .join(".agent-doc")
                    .join("sync-repair-_tmp_socket_name-agent_doc_session.stamp")
            )
        );
    }

    #[test]
    fn sync_repair_stamp_path_returns_none_without_agent_doc_dir() {
        let temp = tempfile::tempdir().unwrap();

        assert_eq!(sync_repair_stamp_path(temp.path(), None, "agent-doc"), None);
    }

    #[test]
    fn rename_debounce_expired_uses_ttl_boundary() {
        let ttl = Duration::from_secs(5);
        assert!(!rename_debounce_expired(Duration::from_secs(4), ttl));
        assert!(rename_debounce_expired(Duration::from_secs(5), ttl));
        assert!(rename_debounce_expired(Duration::from_secs(6), ttl));
    }

    #[test]
    fn auto_started_panes_summary_formats_multiple_panes() {
        let auto_started_panes = vec![
            ("%80".to_string(), "tasks/cursor.md".to_string()),
            ("%81".to_string(), "tasks/feat.md".to_string()),
            ("%82".to_string(), "tasks/agent-loop.md".to_string()),
        ];

        let summary = auto_started_panes_summary(&auto_started_panes).unwrap();
        assert_eq!(
            summary,
            "auto-started 3 panes: %80→tasks/cursor.md, %81→tasks/feat.md, %82→tasks/agent-loop.md"
        );
    }

    #[test]
    fn auto_started_panes_summary_skips_single_pane() {
        let auto_started_panes = vec![("%84".to_string(), "tasks/file.md".to_string())];
        assert_eq!(auto_started_panes_summary(&auto_started_panes), None);
    }

    #[test]
    fn sanitize_excerpt_collapses_whitespace_and_skips_empty() {
        assert_eq!(sanitize_excerpt(" \n\t "), None);
        assert_eq!(
            sanitize_excerpt(" first\n\nsecond\tthird "),
            Some("first second third".to_string())
        );
    }

    #[test]
    fn sanitize_excerpt_truncates_long_text() {
        let input = "x".repeat(205);
        let excerpt = sanitize_excerpt(&input).unwrap();
        assert_eq!(excerpt.len(), 203);
        assert!(excerpt.ends_with("..."));
    }

    #[test]
    fn last_visible_excerpt_ignores_dead_pane_marker() {
        let capture = "\nPane is dead\n  visible line  \nPane is dead";
        assert_eq!(
            last_visible_excerpt(capture),
            Some("visible line".to_string())
        );
    }

    #[test]
    fn registry_relative_file_path_prefers_project_relative_path() {
        assert_eq!(
            registry_relative_file_path(Path::new("/repo"), Path::new("/repo/tasks/doc.md")),
            "tasks/doc.md"
        );
        assert_eq!(
            registry_relative_file_path(Path::new("/repo"), Path::new("/other/doc.md")),
            "/other/doc.md"
        );
    }

    #[test]
    fn sync_prune_fingerprint_includes_window_and_columns() {
        let cols = vec!["left.md".to_string(), "right.md".to_string()];
        let fingerprint = sync_prune_fingerprint(&cols, Some("@1"));
        assert!(fingerprint.contains("@1"), "{fingerprint}");
        assert!(fingerprint.contains("left.md"), "{fingerprint}");
        assert!(fingerprint.contains("right.md"), "{fingerprint}");
    }

    #[test]
    fn sync_prune_state_update_skips_write_for_recent_same_layout() {
        let cols = vec!["left.md".to_string(), "right.md".to_string()];
        let initial = sync_prune_state_update(None, &cols, Some("@1"), 1_000, 2_000);
        assert!(initial.should_write);
        let raw = serde_json::to_string(&initial.state).unwrap();

        let next = sync_prune_state_update(Some(&raw), &cols, Some("@1"), 1_500, 2_000);
        assert!(!next.should_write);
        assert_eq!(next.state, initial.state);
    }

    #[test]
    fn sync_prune_state_update_rewrites_on_layout_change_or_expiry() {
        let cols = vec!["left.md".to_string(), "right.md".to_string()];
        let initial = sync_prune_state_update(None, &cols, Some("@1"), 1_000, 2_000);
        let raw = serde_json::to_string(&initial.state).unwrap();

        let changed_cols = vec!["left.md".to_string()];
        let changed = sync_prune_state_update(Some(&raw), &changed_cols, Some("@1"), 1_500, 2_000);
        assert!(changed.should_write);
        assert_eq!(changed.state.last_full_cleanup_ms, 1_500);

        let expired = sync_prune_state_update(Some(&raw), &cols, Some("@1"), 3_000, 2_000);
        assert!(expired.should_write);
        assert_eq!(expired.state.last_full_cleanup_ms, 3_000);
    }

    #[test]
    fn planned_stash_window_indices_packs_stash_windows_after_agent_doc() {
        let windows = vec![
            ("0".to_string(), "@10".to_string(), "agent-doc".to_string()),
            ("3".to_string(), "@11".to_string(), "stash".to_string()),
            ("7".to_string(), "@12".to_string(), "stash-2".to_string()),
            ("8".to_string(), "@13".to_string(), "work".to_string()),
        ];

        assert_eq!(
            planned_stash_window_indices(&windows, |name| name == "stash"
                || name.starts_with("stash-")),
            vec![("@11".to_string(), 1), ("@12".to_string(), 2)]
        );
    }

    #[test]
    fn plan_window_index_normalization_moves_when_target_index_is_free() {
        let windows = vec![
            ("1".to_string(), "@10".to_string(), "work".to_string()),
            ("2".to_string(), "@11".to_string(), "agent-doc".to_string()),
        ];

        assert_eq!(
            plan_window_index_normalization(&windows, "@11", 0),
            WindowIndexNormalizationPlan::Move {
                current_index: "2".to_string(),
                desired_index: "0".to_string(),
                current_name: "agent-doc".to_string(),
            }
        );
    }

    #[test]
    fn plan_window_index_normalization_swaps_when_target_index_is_occupied() {
        let windows = vec![
            ("0".to_string(), "@10".to_string(), "work".to_string()),
            ("2".to_string(), "@11".to_string(), "agent-doc".to_string()),
        ];

        assert_eq!(
            plan_window_index_normalization(&windows, "@11", 0),
            WindowIndexNormalizationPlan::Swap {
                current_index: "2".to_string(),
                desired_index: "0".to_string(),
                current_name: "agent-doc".to_string(),
                occupant_id: "@10".to_string(),
                occupant_name: "work".to_string(),
            }
        );
    }

    #[test]
    fn plan_window_index_normalization_noops_for_missing_or_current_index() {
        let windows = vec![("0".to_string(), "@10".to_string(), "agent-doc".to_string())];

        assert_eq!(
            plan_window_index_normalization(&windows, "@10", 0),
            WindowIndexNormalizationPlan::AlreadyAtIndex
        );
        assert_eq!(
            plan_window_index_normalization(&windows, "@99", 0),
            WindowIndexNormalizationPlan::Missing
        );
    }

    #[test]
    fn is_file_rename_detects_missing_old_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let old_path = tmp.path().join("old.md");
        let current_path = tmp.path().join("new.md").to_string_lossy().to_string();
        assert!(is_file_rename(&old_path.to_string_lossy(), &current_path));
    }

    #[test]
    fn is_file_rename_rejects_same_or_existing_old_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let old_path = tmp.path().join("old.md");
        let new_path = tmp.path().join("new.md");
        std::fs::write(&old_path, "content").unwrap();
        std::fs::write(&new_path, "content").unwrap();
        let old = old_path.to_string_lossy();
        assert!(!is_file_rename(&old, &old));
        assert!(!is_file_rename(&old, &new_path.to_string_lossy()));
    }

    #[test]
    fn resync_target_matcher_matches_absolute_candidate_path() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "# Test\n").unwrap();
        let target = doc.canonicalize().unwrap();
        let matcher = ResyncTargetMatcher::new(&target, dir.path());

        assert!(matcher.same_document_path(&doc.to_string_lossy()));
    }

    #[test]
    fn resync_target_matcher_matches_relative_base_dir_path() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("tasks").join("test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# Test\n").unwrap();
        let target = doc.canonicalize().unwrap();
        let matcher = ResyncTargetMatcher::new(&target, dir.path());

        assert!(matcher.candidate_matches_target("tasks/test.md"));
    }

    #[test]
    fn resync_target_matcher_rejects_empty_and_foreign_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("target.md");
        let other = dir.path().join("other.md");
        std::fs::write(&doc, "# Target\n").unwrap();
        std::fs::write(&other, "# Other\n").unwrap();
        let target = doc.canonicalize().unwrap();
        let matcher = ResyncTargetMatcher::new(&target, dir.path());

        assert!(!matcher.candidate_matches_target(""));
        assert!(!matcher.candidate_matches_target("other.md"));
        assert!(!matcher.same_document_path(&other.to_string_lossy()));
    }

    #[test]
    fn resync_target_matcher_formats_registry_file_relative_to_base() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("tasks").join("test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# Test\n").unwrap();
        let target = doc.canonicalize().unwrap();
        let matcher = ResyncTargetMatcher::new(&target, dir.path());

        assert_eq!(matcher.registry_file_for_target(), "tasks/test.md");
    }
}
