//! # Module: callback
//!
//! Bidirectional IPC callback request/response system.
//!
//! - Binary writes a callback request to `.agent-doc/callbacks/<doc_hash>/request.json`
//! - Claude Code PostToolUse hook fires `agent-doc hook check-callbacks` to detect pending requests
//! - Claude session processes the request and writes `response.json`
//! - Binary polls for `response.json` with a configurable timeout
//! - On timeout, falls back to spawning `claude --print` as a subagent

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::snapshot;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackRequest {
    pub doc_path: String,
    pub doc_hash: String,
    pub operations: Vec<String>,
    pub context: Option<String>,
    pub created_at: u64,
    pub ttl_secs: u64,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Patch {
    pub component: String,
    pub mode: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackResponse {
    pub request_id: String,
    pub status: String, // "success" | "error"
    pub summary: String,
    pub details: Option<String>,
    pub patches: Option<Vec<Patch>>,
    pub completed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingCallback {
    pub doc_path: String,
    pub operations: Vec<String>,
    pub urgency: String,
}

/// Compute the callback directory path for a document.
fn callback_dir_for(doc: &Path) -> Result<PathBuf> {
    let root =
        agent_doc_fs::find_project_root(doc).context("could not find .agent-doc directory")?;
    let hash = snapshot::doc_hash(doc)?;
    Ok(root.join(".agent-doc").join("callbacks").join(hash))
}

/// Create a callback request file for the given document.
pub fn create_request(
    doc: &Path,
    operations: &[&str],
    context: Option<&str>,
    ttl_secs: u64,
) -> Result<CallbackRequest> {
    let doc_path = doc
        .canonicalize()
        .context("could not canonicalize document path")?;
    let hash = snapshot::doc_hash(&doc_path)?;
    let dir = callback_dir_for(&doc_path)?;

    std::fs::create_dir_all(&dir)?;

    let request_id = uuid::Uuid::new_v4().to_string();
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let request = CallbackRequest {
        doc_path: doc_path.to_string_lossy().to_string(),
        doc_hash: hash,
        operations: operations.iter().map(|s| s.to_string()).collect(),
        context: context.map(|s| s.to_string()),
        created_at,
        ttl_secs,
        request_id,
    };

    let json = serde_json::to_string_pretty(&request)?;
    std::fs::write(dir.join("request.json"), json)?;

    Ok(request)
}

/// Read a callback response if one exists and matches the request.
pub fn read_response(doc: &Path) -> Result<Option<CallbackResponse>> {
    let doc_path = doc.canonicalize().ok().unwrap_or_else(|| doc.to_path_buf());
    let dir = callback_dir_for(&doc_path)?;
    let response_path = dir.join("response.json");

    if !response_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&response_path)?;
    let response: CallbackResponse =
        serde_json::from_str(&content).context("failed to parse callback response JSON")?;

    // Verify request_id matches
    let request = read_request(doc)?;
    if let Some(req) = request
        && response.request_id != req.request_id
    {
        return Ok(None); // stale response from a previous request
    }

    Ok(Some(response))
}

/// Read the current callback request if one exists.
pub fn read_request(doc: &Path) -> Result<Option<CallbackRequest>> {
    let doc_path = doc.canonicalize().ok().unwrap_or_else(|| doc.to_path_buf());
    let dir = callback_dir_for(&doc_path)?;
    let request_path = dir.join("request.json");

    if !request_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&request_path)?;
    let request: CallbackRequest =
        serde_json::from_str(&content).context("failed to parse callback request JSON")?;

    Ok(Some(request))
}

/// Write a callback response for a document.
pub fn write_response(
    doc: &Path,
    request_id: &str,
    status: &str,
    summary: &str,
    patches: Option<Vec<Patch>>,
) -> Result<()> {
    let doc_path = doc.canonicalize().ok().unwrap_or_else(|| doc.to_path_buf());
    let dir = callback_dir_for(&doc_path)?;

    // Verify request exists and matches
    let request = read_request(&doc_path)?;
    match &request {
        Some(req) if req.request_id == request_id => {}
        Some(_) => anyhow::bail!("request_id mismatch — stale or wrong request"),
        None => anyhow::bail!("no pending callback request for this document"),
    }

    let completed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let response = CallbackResponse {
        request_id: request_id.to_string(),
        status: status.to_string(),
        summary: summary.to_string(),
        details: None,
        patches,
        completed_at,
    };

    let json = serde_json::to_string_pretty(&response)?;
    std::fs::write(dir.join("response.json"), json)?;

    Ok(())
}

/// Delete a callback response file after it has been processed.
pub fn delete_response(doc: &Path) -> Result<()> {
    let doc_path = doc.canonicalize().ok().unwrap_or_else(|| doc.to_path_buf());
    let dir = callback_dir_for(&doc_path)?;
    let response_path = dir.join("response.json");

    if response_path.exists() {
        std::fs::remove_file(&response_path)?;
    }

    Ok(())
}

/// Clean up expired callback directories.
pub fn cleanup_expired(project_root: &Path, _max_age_secs: u64) -> Result<()> {
    let callbacks_dir = project_root.join(".agent-doc/callbacks");
    if !callbacks_dir.is_dir() {
        return Ok(());
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    for entry in std::fs::read_dir(&callbacks_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let request_path = path.join("request.json");
        if let Ok(content) = std::fs::read_to_string(&request_path)
            && let Ok(request) = serde_json::from_str::<CallbackRequest>(&content)
        {
            let age = now.saturating_sub(request.created_at);
            if age > request.ttl_secs {
                std::fs::remove_dir_all(&path)?;
                eprintln!("[callback] removed expired request: {}", path.display());
            }
        }
    }

    Ok(())
}

/// Scan all callback directories for pending (valid, unresponded) requests.
pub fn scan_pending_callbacks(project_root: Option<&str>) -> Result<Vec<PendingCallback>> {
    let callbacks_dir = if let Some(root) = project_root {
        PathBuf::from(root).join(".agent-doc/callbacks")
    } else {
        let cwd = std::env::current_dir()?;
        let Some(root) = agent_doc_fs::find_project_root(&cwd) else {
            return Ok(Vec::new());
        };
        root.join(".agent-doc/callbacks")
    };

    if !callbacks_dir.is_dir() {
        return Ok(Vec::new());
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let mut pending = Vec::new();

    for entry in std::fs::read_dir(&callbacks_dir)? {
        let entry = entry?;
        let dir_path = entry.path();
        if !dir_path.is_dir() {
            continue;
        }

        let request_path = dir_path.join("request.json");
        let response_path = dir_path.join("response.json");

        // Skip if already responded
        if response_path.exists() {
            continue;
        }

        if let Ok(content) = std::fs::read_to_string(&request_path)
            && let Ok(request) = serde_json::from_str::<CallbackRequest>(&content)
        {
            let age = now.saturating_sub(request.created_at);
            if age > request.ttl_secs {
                continue; // expired
            }

            let elapsed_secs = now.saturating_sub(request.created_at);
            let urgency = if elapsed_secs < 10 {
                "high"
            } else if elapsed_secs < 60 {
                "normal"
            } else {
                "low"
            };

            pending.push(PendingCallback {
                doc_path: request.doc_path,
                operations: request.operations,
                urgency: urgency.to_string(),
            });
        }
    }

    Ok(pending)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_project() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
        tmp
    }

    fn create_test_doc(tmp: &tempfile::TempDir, name: &str) -> PathBuf {
        let doc = tmp.path().join(name);
        fs::write(&doc, "---\nagent_doc_session: test\n---\nHello\n").unwrap();
        doc
    }

    #[test]
    fn create_request_writes_valid_json() {
        let tmp = setup_test_project();
        let doc = create_test_doc(&tmp, "test.md");
        let request = create_request(&doc, &["compact", "prune-pending"], None, 300).unwrap();

        let dir = callback_dir_for(&doc).unwrap();
        let request_json = dir.join("request.json");
        assert!(request_json.exists());

        let content = fs::read_to_string(&request_json).unwrap();
        let parsed: CallbackRequest = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.request_id, request.request_id);
        assert_eq!(parsed.operations, vec!["compact", "prune-pending"]);
        assert_eq!(parsed.doc_hash, request.doc_hash);
        assert_eq!(parsed.ttl_secs, 300);
    }

    #[test]
    fn response_returns_none_when_absent() {
        let tmp = setup_test_project();
        let doc = create_test_doc(&tmp, "test.md");
        create_request(&doc, &["compact"], None, 300).unwrap();

        let response = read_response(&doc).unwrap();
        assert!(response.is_none());
    }

    #[test]
    fn response_round_trip() {
        use std::time::SystemTime;

        let tmp = setup_test_project();
        let doc = create_test_doc(&tmp, "test.md");
        let request = create_request(&doc, &["compact"], None, 300).unwrap();

        let dir = callback_dir_for(&doc).unwrap();
        let response = CallbackResponse {
            request_id: request.request_id.clone(),
            status: "success".to_string(),
            summary: "Compacted 3 exchanges.".to_string(),
            details: None,
            patches: None,
            completed_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        fs::write(
            dir.join("response.json"),
            serde_json::to_string_pretty(&response).unwrap(),
        )
        .unwrap();

        let result = read_response(&doc).unwrap();
        assert!(result.is_some());
        let resp = result.unwrap();
        assert_eq!(resp.request_id, request.request_id);
        assert_eq!(resp.status, "success");
    }

    #[test]
    fn mismatched_request_id_ignored() {
        use std::time::SystemTime;

        let tmp = setup_test_project();
        let doc = create_test_doc(&tmp, "test.md");
        let _request = create_request(&doc, &["compact"], None, 300).unwrap();

        let dir = callback_dir_for(&doc).unwrap();
        let wrong_response = CallbackResponse {
            request_id: "wrong-uuid".to_string(),
            status: "success".to_string(),
            summary: "test".to_string(),
            details: None,
            patches: None,
            completed_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        fs::write(
            dir.join("response.json"),
            serde_json::to_string_pretty(&wrong_response).unwrap(),
        )
        .unwrap();

        let result = read_response(&doc).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn cleanup_expired_removes_old_dirs() {
        use std::time::SystemTime;

        let tmp = setup_test_project();
        let doc = create_test_doc(&tmp, "test.md");
        let request = create_request(&doc, &["compact"], None, 60).unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut expired = request.clone();
        expired.created_at = now - 120;

        let dir = callback_dir_for(&doc).unwrap();
        fs::write(
            dir.join("request.json"),
            serde_json::to_string_pretty(&expired).unwrap(),
        )
        .unwrap();

        cleanup_expired(tmp.path(), 60).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn gc_skips_unexpired() {
        let tmp = setup_test_project();
        let doc = create_test_doc(&tmp, "test.md");
        create_request(&doc, &["compact"], None, 300).unwrap();

        let dir = callback_dir_for(&doc).unwrap();
        cleanup_expired(tmp.path(), 300).unwrap();
        assert!(dir.exists());
        assert!(dir.join("request.json").exists());
    }

    #[test]
    fn scan_pending_finds_unresponded() {
        let tmp = setup_test_project();
        let doc = create_test_doc(&tmp, "test.md");
        create_request(&doc, &["compact"], None, 300).unwrap();

        let pending = scan_pending_callbacks(Some(tmp.path().to_str().unwrap())).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].operations, vec!["compact"]);
    }

    #[test]
    fn scan_pending_skips_responded() {
        use std::time::SystemTime;

        let tmp = setup_test_project();
        let doc = create_test_doc(&tmp, "test.md");
        let request = create_request(&doc, &["compact"], None, 300).unwrap();

        let dir = callback_dir_for(&doc).unwrap();
        let response = CallbackResponse {
            request_id: request.request_id.clone(),
            status: "success".to_string(),
            summary: "done".to_string(),
            details: None,
            patches: None,
            completed_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        fs::write(
            dir.join("response.json"),
            serde_json::to_string_pretty(&response).unwrap(),
        )
        .unwrap();

        let pending = scan_pending_callbacks(Some(tmp.path().to_str().unwrap())).unwrap();
        assert!(pending.is_empty());
    }
}
