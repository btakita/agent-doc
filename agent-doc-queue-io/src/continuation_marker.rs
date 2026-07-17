//! Durable queue-continuation state.
//!
//! This module stores continuation facts in the project state ledger. Callers
//! own continuation detection, actor-ownership gates, and any
//! higher-level retry/scan orchestration.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use agent_doc_queue::queue_continuation::QueueContinuation;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const QUEUE_CONTINUATION_STATE_KIND: &str = "continuation";

/// Durable ledger proof that a closed-out document still owes an auto-queue
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

/// Caller-owned decision for a parsed continuation marker during a directory
/// scan.
pub enum ContinuationMarkerScanAction {
    /// Return this marker with the supplied live continuation.
    Return(QueueContinuation),
    /// Keep the marker and continue scanning.
    Skip,
    /// Remove the marker as stale and continue scanning.
    RemoveStale,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn state_identity(file: &Path) -> Result<Option<(PathBuf, String, String)>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    let hash = agent_doc_hash::path_hash(file)
        .with_context(|| format!("canonicalize document path for hash: {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    Ok(Some((root, hash, canonical.to_string_lossy().into_owned())))
}

pub fn write_continuation_marker(
    file: &Path,
    continuation: &QueueContinuation,
    source_command: &str,
) -> Result<()> {
    let Some((_, _, _)) = state_identity(file)? else {
        return Ok(());
    };
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
    save_marker(file, &marker)
}

pub fn clear_continuation_marker(file: &Path) -> Result<()> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(());
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::clear_queue_document_state_in_db(
        &conn,
        &document_hash,
        QUEUE_CONTINUATION_STATE_KIND,
    )?;
    Ok(())
}

pub fn load_continuation_marker(file: &Path) -> Result<Option<ContinuationMarker>> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(None);
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    let Some(record) = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        &conn,
        &document_hash,
        QUEUE_CONTINUATION_STATE_KIND,
    )?
    else {
        return Ok(None);
    };
    Ok(serde_json::from_str(&record.payload_json).ok())
}

/// Scan roots for the first marker the caller confirms is still valid.
///
/// Owns ledger scans, invalid-record skips, document-level dedupe, and stale
/// record cleanup. Callers own live document
/// detection and actor/owner policy.
pub fn scan_pending_marker_continuations_for_roots<F>(
    roots: &[PathBuf],
    mut decide: F,
) -> Result<Option<(PathBuf, QueueContinuation, ContinuationMarker)>>
where
    F: FnMut(&Path, &Path, &ContinuationMarker) -> Result<ContinuationMarkerScanAction>,
{
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        let conn = agent_doc_sqlite::state_store::open_state_db(root)?;
        let records = agent_doc_sqlite::state_store::list_queue_document_state_from_db(
            &conn,
            QUEUE_CONTINUATION_STATE_KIND,
        )?;
        for record in records {
            let Ok(marker) = serde_json::from_str::<ContinuationMarker>(&record.payload_json)
            else {
                continue;
            };
            let doc = PathBuf::from(&record.canonical_path);
            if !seen.insert(doc.clone()) {
                continue;
            }
            match decide(root, &doc, &marker)? {
                ContinuationMarkerScanAction::Return(continuation) => {
                    return Ok(Some((doc, continuation, marker)));
                }
                ContinuationMarkerScanAction::Skip => {}
                ContinuationMarkerScanAction::RemoveStale => {
                    let _ = agent_doc_sqlite::state_store::clear_queue_document_state_in_db(
                        &conn,
                        &record.document_hash,
                        QUEUE_CONTINUATION_STATE_KIND,
                    );
                }
            }
        }
    }
    Ok(None)
}

/// Record that the head prompt was surfaced to a Codex Stop hook as a
/// continuation request, so a subsequent stop with the same head can fail
/// closed instead of looping. No-op when no marker exists.
pub fn record_continuation_requested_head(file: &Path, head_prompt: &str) -> Result<()> {
    let Some(mut marker) = load_continuation_marker(file)? else {
        return Ok(());
    };
    marker.last_requested_head = Some(head_prompt.to_string());
    save_marker(file, &marker)
}

fn save_marker(file: &Path, marker: &ContinuationMarker) -> Result<()> {
    let Some((root, document_hash, canonical_path)) = state_identity(file)? else {
        return Ok(());
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
        &conn,
        &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
            document_hash,
            state_kind: QUEUE_CONTINUATION_STATE_KIND.to_string(),
            canonical_path,
            payload_json: serde_json::to_string(marker).context("serialize continuation marker")?,
            updated_at_secs: now_secs(),
        },
    )
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

    fn write_named_doc(dir: &Path, name: &str) -> PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        let doc = dir.join(name);
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
    fn scan_pending_state_returns_valid_marker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let doc = write_doc(&root);
        let continuation = continuation();
        write_continuation_marker(&doc, &continuation, "commit").unwrap();

        let found = scan_pending_marker_continuations_for_roots(&[root], |_, marker_doc, _| {
            assert_eq!(marker_doc, doc.as_path());
            Ok(ContinuationMarkerScanAction::Return(continuation.clone()))
        })
        .unwrap()
        .expect("valid marker returned");

        assert_eq!(found.0, doc);
        assert_eq!(found.1.head_prompt, "do [#a]");
        assert_eq!(found.2.source_command, "commit");
    }

    #[test]
    fn scan_pending_marker_prunes_stale_marker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let doc = write_named_doc(&root, "stale.md");
        write_continuation_marker(&doc, &continuation(), "commit").unwrap();
        assert!(load_continuation_marker(&doc).unwrap().is_some());

        let found = scan_pending_marker_continuations_for_roots(&[root], |_, marker_doc, _| {
            assert_eq!(marker_doc, doc.as_path());
            Ok(ContinuationMarkerScanAction::RemoveStale)
        })
        .unwrap();

        assert!(found.is_none());
        assert!(load_continuation_marker(&doc).unwrap().is_none());
    }
}
