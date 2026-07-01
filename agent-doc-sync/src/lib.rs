//! Sync scope and layout-state planning policy.
//!
//! This crate owns path-list normalization and `.agent-doc` scope-root
//! selection for sync. Orchestration owns tmux calls, repair side effects,
//! document validation, and state-file IO.

use std::path::{Path, PathBuf};

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
}
