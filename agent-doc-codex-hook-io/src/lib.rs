//! Codex hook sidecar persistence.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionState {
    pub session_id: String,
    pub doc_path: String,
    pub last_turn_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_auto_queue_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_context_clear_at: Option<u64>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSessionState {
    pub session_id: String,
    pub doc_path: String,
    pub last_turn_id: String,
    pub last_prompt: String,
    pub updated_at: u64,
}

pub fn load_state_any(
    roots: &[PathBuf],
    session_id: &str,
) -> Result<Option<(PathBuf, SessionState)>> {
    for root in roots {
        if let Some(state) = load_state(root, session_id)? {
            return Ok(Some((root.clone(), state)));
        }
    }
    Ok(None)
}

pub fn clear_state_across_roots(
    roots: &[PathBuf],
    loaded_root: &Path,
    session_id: &str,
) -> Result<()> {
    let mut all_roots = roots.to_vec();
    push_unique_root(&mut all_roots, loaded_root.to_path_buf());
    for root in all_roots {
        clear_state(&root, session_id)?;
    }
    Ok(())
}

pub fn save_state_across_roots(
    roots: &[PathBuf],
    loaded_root: &Path,
    state: &SessionState,
) -> Result<()> {
    let mut all_roots = roots.to_vec();
    push_unique_root(&mut all_roots, loaded_root.to_path_buf());
    for root in all_roots {
        save_state(&root, state)?;
    }
    Ok(())
}

pub fn load_state(root: &Path, session_id: &str) -> Result<Option<SessionState>> {
    let path = state_path(root, session_id);
    if !path.exists() {
        return Ok(None);
    }
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let state =
        serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(state))
}

pub fn load_prompt_for_current_session(file: &Path) -> Result<Option<String>> {
    let Some(state) = load_active_session_for_current_file(file)? else {
        return Ok(None);
    };
    if state.last_prompt.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(state.last_prompt))
}

pub fn load_latest_prompt_for_file(file: &Path) -> Result<Option<String>> {
    let Some(state) = load_latest_state_for_file(file)? else {
        return Ok(None);
    };
    if state.last_prompt.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(state.last_prompt))
}

pub fn load_latest_prompt_state_for_file(file: &Path) -> Result<Option<ActiveSessionState>> {
    load_latest_state_for_file(file)
}

pub fn prompt_requests_clear(prompt: &str) -> bool {
    matches!(prompt.trim(), "/clear" | "/new")
}

pub fn record_external_prompt_for_file(file: &Path, session_id: &str, prompt: &str) -> Result<()> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let state = SessionState {
        session_id: session_id.to_string(),
        doc_path: canonical.display().to_string(),
        last_turn_id: String::new(),
        last_prompt: prompt.to_string(),
        last_auto_queue_head: None,
        last_context_clear_at: prompt_requests_clear(prompt).then(now_secs),
        updated_at: now_secs(),
    };
    for root in project_roots_for(&canonical) {
        save_state(&root, &state)?;
    }
    Ok(())
}

pub fn load_active_session_for_current_file(file: &Path) -> Result<Option<ActiveSessionState>> {
    let Some(session_id) = current_session_id() else {
        return Ok(None);
    };
    let roots = project_roots_for(file);
    let Some((_, state)) = load_state_any(&roots, &session_id)? else {
        return Ok(None);
    };
    let state_file = PathBuf::from(&state.doc_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&state.doc_path));
    let current_file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if state_file != current_file {
        return Ok(None);
    }
    Ok(Some(active_session_state(state)))
}

pub fn save_state(root: &Path, state: &SessionState) -> Result<()> {
    let path = state_path(root, &state.session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn clear_state(root: &Path, session_id: &str) -> Result<()> {
    let path = state_path(root, session_id);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err).with_context(|| format!("remove {}", path.display())),
    }
    Ok(())
}

pub fn state_path(root: &Path, session_id: &str) -> PathBuf {
    let hash = agent_doc_hash::content_hash(session_id);
    root.join(".agent-doc/codex-hooks/sessions")
        .join(format!("{hash}.json"))
}

pub fn current_session_id() -> Option<String> {
    std::env::var("CODEX_THREAD_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("CODEX_SESSION_ID")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

fn load_latest_state_for_file(file: &Path) -> Result<Option<ActiveSessionState>> {
    let current_file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let mut latest: Option<SessionState> = None;

    for root in project_roots_for(file) {
        let dir = root.join(".agent-doc/codex-hooks/sessions");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(state) = serde_json::from_str::<SessionState>(&content) else {
                continue;
            };
            let state_file = PathBuf::from(&state.doc_path)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&state.doc_path));
            if state_file != current_file {
                continue;
            }
            let is_newer = latest
                .as_ref()
                .is_none_or(|best| state.updated_at >= best.updated_at);
            if is_newer {
                latest = Some(state);
            }
        }
    }

    Ok(latest.map(active_session_state))
}

fn active_session_state(state: SessionState) -> ActiveSessionState {
    ActiveSessionState {
        session_id: state.session_id,
        doc_path: state.doc_path,
        last_turn_id: state.last_turn_id,
        last_prompt: state.last_prompt,
        updated_at: state.updated_at,
    }
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
