//! Durable queue-continuation and clear/cooldown marker storage.
//!
//! This module owns queue sidecar paths, serialization, and idempotent marker
//! cleanup. Callers own continuation detection, actor-ownership gates, and any
//! higher-level retry/scan orchestration.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use agent_doc_queue::queue_continuation::QueueContinuation;
use agent_doc_queue::queue_preemption::{
    DeferredOperatorClear, deferred_operator_clear_marker, deferred_operator_clear_marker_json,
    parse_deferred_operator_clear_marker_json,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const QUEUE_CONTINUATIONS_DIR: &str = ".agent-doc/queue-continuations";
const QUEUE_COOLDOWNS_DIR: &str = ".agent-doc/queue-cooldowns";
const DEFERRED_CLEARS_DIR: &str = ".agent-doc/deferred-clears";

/// Durable on-disk proof that a closed-out document still owes an auto-queue
/// continuation. Survives missing Codex hook session state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuationMarker {
    pub file: String,
    pub head_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_id: Option<String>,
    pub created_at: u64,
    pub source_command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_head: Option<String>,
    /// The head prompt last surfaced to a Codex Stop hook as a continuation
    /// request. Lets the hook fail closed when a repeated stop sees the same,
    /// non-advancing head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_requested_head: Option<String>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn sidecar_path(file: &Path, dir: &str) -> Result<Option<PathBuf>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    let hash = agent_doc_hash::path_hash(file)
        .with_context(|| format!("canonicalize document path for hash: {}", file.display()))?;
    Ok(Some(root.join(dir).join(format!("{hash}.json"))))
}

pub fn continuation_marker_path(file: &Path) -> Result<Option<PathBuf>> {
    sidecar_path(file, QUEUE_CONTINUATIONS_DIR)
}

fn cooldown_marker_path(file: &Path) -> Result<Option<PathBuf>> {
    sidecar_path(file, QUEUE_COOLDOWNS_DIR)
}

fn deferred_clear_marker_path(file: &Path) -> Result<Option<PathBuf>> {
    sidecar_path(file, DEFERRED_CLEARS_DIR)
}

pub fn write_continuation_marker(
    file: &Path,
    continuation: &QueueContinuation,
    source_command: &str,
) -> Result<()> {
    let Some(path) = continuation_marker_path(file)? else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    // Preserve the last continuation request across reconciles so the Stop-hook
    // non-advancing-head guard still works after a re-detect.
    let last_requested_head =
        load_continuation_marker(file)?.and_then(|marker| marker.last_requested_head);
    let marker = ContinuationMarker {
        file: file.display().to_string(),
        head_prompt: continuation.head_prompt.clone(),
        head_id: continuation.head_id.clone(),
        created_at: now_secs(),
        source_command: source_command.to_string(),
        commit_head: head_oid(file),
        last_requested_head,
    };
    let json = serde_json::to_string_pretty(&marker).context("serialize continuation marker")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn clear_continuation_marker(file: &Path) -> Result<()> {
    let Some(path) = continuation_marker_path(file)? else {
        return Ok(());
    };
    remove_marker_file(&path)
}

pub fn load_continuation_marker(file: &Path) -> Result<Option<ContinuationMarker>> {
    let Some(path) = continuation_marker_path(file)? else {
        return Ok(None);
    };
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(serde_json::from_str(&content).ok()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("read {}", path.display())),
    }
}

/// Record that the head prompt was surfaced to a Codex Stop hook as a
/// continuation request, so a subsequent stop with the same head can fail
/// closed instead of looping. No-op when no marker exists.
pub fn record_continuation_requested_head(file: &Path, head_prompt: &str) -> Result<()> {
    let Some(mut marker) = load_continuation_marker(file)? else {
        return Ok(());
    };
    marker.last_requested_head = Some(head_prompt.to_string());
    let Some(path) = continuation_marker_path(file)? else {
        return Ok(());
    };
    let json = serde_json::to_string_pretty(&marker).context("serialize continuation marker")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn write_clear_cooldown(file: &Path) -> Result<()> {
    let Some(path) = cooldown_marker_path(file)? else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let payload = serde_json::json!({
        "file": file.to_string_lossy(),
        "written_at": now_secs(),
    });
    let json = serde_json::to_string_pretty(&payload).context("serialize cooldown marker")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn clear_cooldown_marker(file: &Path) -> Result<()> {
    let Some(path) = cooldown_marker_path(file)? else {
        return Ok(());
    };
    remove_marker_file(&path)
}

pub fn clear_cooldown_active(file: &Path) -> Result<bool> {
    let Some(path) = cooldown_marker_path(file)? else {
        return Ok(false);
    };
    match std::fs::read_to_string(&path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("read {}", path.display())),
    }
}

/// Record that a non-interrupting operator clear was deferred while the pane was
/// busy under an active auto-loop. Paired with [`write_clear_cooldown`] (which
/// pauses the loop); the watch delivers `clear_command` once the pane is idle,
/// then clears both markers to resume.
pub fn write_deferred_operator_clear(file: &Path, clear_command: &str) -> Result<()> {
    let Some(path) = deferred_clear_marker_path(file)? else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let payload = deferred_operator_clear_marker(
        file.to_string_lossy().into_owned(),
        clear_command,
        now_secs(),
    );
    let json =
        deferred_operator_clear_marker_json(&payload).context("serialize deferred clear marker")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Read the pending deferred operator clear for `file`, if any.
pub fn read_deferred_operator_clear(file: &Path) -> Result<Option<DeferredOperatorClear>> {
    let Some(path) = deferred_clear_marker_path(file)? else {
        return Ok(None);
    };
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(parse_deferred_operator_clear_marker_json(&content)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("read {}", path.display())),
    }
}

/// Remove the deferred-clear marker after the watch delivers the clear or an
/// explicit interrupt-clear supersedes it.
pub fn clear_deferred_operator_clear_marker(file: &Path) -> Result<()> {
    let Some(path) = deferred_clear_marker_path(file)? else {
        return Ok(());
    };
    remove_marker_file(&path)
}

fn remove_marker_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("remove {}", path.display())),
    }
}

fn head_oid(file: &Path) -> Option<String> {
    let dir = file.parent()?;
    let output = Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let oid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!oid.is_empty()).then_some(oid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_doc(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        let doc = dir.join("task.md");
        std::fs::write(&doc, "body").unwrap();
        doc
    }

    fn continuation() -> QueueContinuation {
        QueueContinuation {
            head_prompt: "do [#a]".to_string(),
            head_id: Some("a".to_string()),
            reason: "test".to_string(),
        }
    }

    #[test]
    fn continuation_marker_roundtrips_and_preserves_requested_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path());

        write_continuation_marker(&doc, &continuation(), "commit").unwrap();
        let marker = load_continuation_marker(&doc).unwrap().unwrap();
        assert_eq!(marker.head_prompt, "do [#a]");
        assert_eq!(marker.head_id.as_deref(), Some("a"));
        assert_eq!(marker.source_command, "commit");

        record_continuation_requested_head(&doc, "do [#a]").unwrap();
        write_continuation_marker(&doc, &continuation(), "commit2").unwrap();
        let marker = load_continuation_marker(&doc).unwrap().unwrap();
        assert_eq!(marker.source_command, "commit2");
        assert_eq!(marker.last_requested_head.as_deref(), Some("do [#a]"));

        clear_continuation_marker(&doc).unwrap();
        assert!(load_continuation_marker(&doc).unwrap().is_none());
    }

    #[test]
    fn cooldown_marker_roundtrips_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path());

        assert!(!clear_cooldown_active(&doc).unwrap());
        write_clear_cooldown(&doc).unwrap();
        assert!(clear_cooldown_active(&doc).unwrap());
        clear_cooldown_marker(&doc).unwrap();
        assert!(!clear_cooldown_active(&doc).unwrap());
    }

    #[test]
    fn deferred_operator_clear_marker_roundtrips_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path());

        assert!(read_deferred_operator_clear(&doc).unwrap().is_none());
        write_deferred_operator_clear(&doc, "/clear").unwrap();
        let marker = read_deferred_operator_clear(&doc).unwrap().unwrap();
        assert_eq!(marker.clear_command, "/clear");
        assert!(marker.file.contains("task.md"));

        clear_deferred_operator_clear_marker(&doc).unwrap();
        assert!(read_deferred_operator_clear(&doc).unwrap().is_none());
    }
}
