//! Codex hook sidecar persistence.

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Resolve the nearest agent-doc project root for `path`.
pub fn project_root_for(path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    agent_doc_project_root_io::project_root_containing(&canonical)
}

/// Roots that should receive Codex hook state for `path`.
///
/// The nearest `.agent-doc` root is always first. When the path lives inside a
/// git worktree whose root also has `.agent-doc`, that git root is included as a
/// second state location so hooks remain visible from both nested and workspace
/// scopes.
pub fn project_roots_for(path: &Path) -> Vec<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let Some(nearest_root) = project_root_for(&canonical) else {
        return Vec::new();
    };
    let mut roots = vec![nearest_root.clone()];
    let Some(git_root) = find_git_root(&canonical) else {
        return roots;
    };
    if !nearest_root.starts_with(&git_root) {
        return roots;
    }
    if git_root.join(".agent-doc").is_dir() {
        push_unique_root(&mut roots, git_root);
    }
    roots
}

/// Roots that should be consulted or updated for a hook payload with a cwd and
/// optional document target.
pub fn tracking_roots(cwd: &Path, doc_path: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = project_roots_for(cwd);
    if let Some(doc_path) = doc_path {
        for root in project_roots_for(doc_path) {
            push_unique_root(&mut roots, root);
        }
    }
    roots
}

/// Append `root` once, preserving first-seen order.
pub fn push_unique_root(roots: &mut Vec<PathBuf>, root: PathBuf) {
    if !roots.iter().any(|existing| existing == &root) {
        roots.push(root);
    }
}

fn find_git_root(path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut current = if canonical.is_file() {
        canonical.parent()
    } else {
        Some(canonical.as_path())
    };
    while let Some(path) = current {
        if path.join(".git").exists() {
            return Some(path.to_path_buf());
        }
        current = path.parent();
    }
    None
}

#[derive(Debug, Serialize)]
struct BlockedStopPayloadRecord<'a> {
    captured_at: u64,
    file: String,
    kind: &'a str,
    reason: &'a str,
    payload_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_assistant_message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_prompt: Option<&'a str>,
}

pub fn save_blocked_stop_payload(
    file: &Path,
    payload: &str,
    reason: &str,
    kind: &str,
    last_prompt: Option<&str>,
) -> Result<PathBuf> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = agent_doc_project_root_io::project_root_containing(&canonical)
        .or_else(|| canonical.parent().map(Path::to_path_buf))
        .context("resolve project root for blocked stop payload")?;
    let dir = root.join(".agent-doc/codex-hooks/blocked-stop");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create blocked-stop dir {}", dir.display()))?;
    let filename = format!(
        "{}-{}.json",
        agent_doc_hash::content_hash(canonical.to_string_lossy().as_ref()),
        now_millis()
    );
    let path = dir.join(filename);
    let record = BlockedStopPayloadRecord {
        captured_at: now_secs(),
        file: canonical.display().to_string(),
        kind,
        reason,
        payload_sha256: agent_doc_hash::content_hash(payload),
        last_assistant_message: (!payload.trim().is_empty()).then_some(payload),
        last_prompt: last_prompt.filter(|prompt| !prompt.trim().is_empty()),
    };
    let json = serde_json::to_string_pretty(&record)?;
    std::fs::write(&path, json)
        .with_context(|| format!("write blocked stop payload {}", path.display()))?;
    Ok(path)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir_without_agent_doc_ancestor() -> tempfile::TempDir {
        for base in [
            Path::new("/var/tmp"),
            Path::new("/dev/shm"),
            Path::new("/tmp"),
        ] {
            if !base.is_dir() || agent_doc_project_root_io::project_root_containing(base).is_some()
            {
                continue;
            }
            if let Ok(dir) = tempfile::Builder::new()
                .prefix("agent-doc-codex-hook-io-")
                .tempdir_in(base)
                && agent_doc_project_root_io::project_root_containing(dir.path()).is_none()
            {
                return dir;
            }
        }
        panic!("no writable temp base without a .agent-doc ancestor");
    }

    #[test]
    fn saves_blocked_stop_payload_under_project_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("task.md");
        std::fs::write(&doc, "---\n---\n").unwrap();

        let path = save_blocked_stop_payload(
            &doc,
            "assistant response",
            "component dump",
            "blocked_replay_payload",
            Some("agent-doc task.md"),
        )
        .unwrap();

        assert!(path.starts_with(root.join(".agent-doc/codex-hooks/blocked-stop")));
        let json = std::fs::read_to_string(path).unwrap();
        assert!(json.contains("\"kind\": \"blocked_replay_payload\""));
        assert!(json.contains("\"reason\": \"component dump\""));
        assert!(json.contains("\"last_assistant_message\": \"assistant response\""));
        assert!(json.contains("\"last_prompt\": \"agent-doc task.md\""));
    }

    #[test]
    fn falls_back_to_file_parent_without_project_root() {
        let dir = tempdir_without_agent_doc_ancestor();
        let doc = dir.path().join("task.md");
        std::fs::write(&doc, "---\n---\n").unwrap();

        let path =
            save_blocked_stop_payload(&doc, "", "missing response", "missing", None).unwrap();

        assert!(path.starts_with(dir.path().join(".agent-doc/codex-hooks/blocked-stop")));
        let json = std::fs::read_to_string(path).unwrap();
        assert!(json.contains("\"kind\": \"missing\""));
        assert!(!json.contains("last_assistant_message"));
        assert!(!json.contains("last_prompt"));
    }

    #[test]
    fn project_roots_for_returns_nearest_then_git_root_when_both_have_agent_doc() {
        let dir = tempfile::tempdir().unwrap();
        let git_root = dir.path();
        std::fs::create_dir_all(git_root.join(".git")).unwrap();
        std::fs::create_dir_all(git_root.join(".agent-doc")).unwrap();
        let nested = git_root.join("nested");
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        let doc = nested.join("task.md");
        std::fs::write(&doc, "---\n---\n").unwrap();

        assert_eq!(
            project_roots_for(&doc),
            vec![nested, git_root.to_path_buf()]
        );
    }

    #[test]
    fn tracking_roots_merges_cwd_and_document_roots_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("tasks/task.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\n---\n").unwrap();

        assert_eq!(tracking_roots(root, Some(&doc)), vec![root.to_path_buf()]);
    }
}
