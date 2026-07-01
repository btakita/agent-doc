//! Sync scope and layout-state planning policy.
//!
//! This crate owns path-list normalization and `.agent-doc` scope-root
//! selection for sync. Orchestration owns tmux calls, repair side effects,
//! document validation, and state-file IO.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn sanitize_stamp_component_replaces_path_separators() {
        assert_eq!(
            sanitize_stamp_component("/tmp/socket:name"),
            "_tmp_socket_name"
        );
        assert_eq!(sanitize_stamp_component("safe-name_1"), "safe-name_1");
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
}
