//! Bidirectional callback requests and responses stored in the controller's
//! single `.agent-doc/state.db` state-machine ledger.
//!
//! The Claude hook and the CLI observe the same typed document-runtime record;
//! no request/response filesystem transport or polling sidecar participates.

use agent_doc_ipc_protocol::{
    CallbackPatch, CallbackRequest, CallbackResponse, PendingCallback, callback_request,
    callback_request_is_expired, callback_response, callback_response_matches_request,
    pending_callback_from_request,
};
use agent_doc_sqlite::state_store::{
    DocumentRuntimeStateRecord, clear_document_runtime_state_in_db,
    list_document_runtime_state_kind_from_db, load_document_runtime_state_from_db, open_state_db,
    state_db_path, upsert_document_runtime_state_in_db,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CALLBACK_STATE_KIND: &str = "callback_exchange";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CallbackState {
    request: CallbackRequest,
    response: Option<CallbackResponse>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn canonical_doc(doc: &Path) -> PathBuf {
    doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf())
}

fn project_root_for(doc: &Path) -> Result<PathBuf> {
    agent_doc_fs::find_project_root(doc).context("could not find .agent-doc directory")
}

fn document_hash(doc: &Path) -> Result<String> {
    agent_doc_hash::path_hash(doc)
        .with_context(|| format!("canonicalize document path for hash: {}", doc.display()))
}

fn load_state(doc: &Path) -> Result<Option<(PathBuf, String, CallbackState)>> {
    let doc = canonical_doc(doc);
    let root = project_root_for(&doc)?;
    if !state_db_path(&root).exists() {
        return Ok(None);
    }
    let hash = document_hash(&doc)?;
    let connection = open_state_db(&root)?;
    let Some(record) =
        load_document_runtime_state_from_db(&connection, &hash, CALLBACK_STATE_KIND)?
    else {
        return Ok(None);
    };
    let state = serde_json::from_str(&record.payload_json)
        .context("failed to parse callback state from state.db")?;
    Ok(Some((root, hash, state)))
}

fn save_state(root: &Path, doc: &Path, hash: &str, state: &CallbackState) -> Result<()> {
    let connection = open_state_db(root)?;
    upsert_document_runtime_state_in_db(
        &connection,
        &DocumentRuntimeStateRecord {
            document_hash: hash.to_string(),
            state_kind: CALLBACK_STATE_KIND.to_string(),
            canonical_path: doc.to_string_lossy().into_owned(),
            payload_json: serde_json::to_string(state)?,
            updated_at_ms: now_millis(),
        },
    )
}

pub fn create_request(
    doc: &Path,
    operations: &[&str],
    context: Option<&str>,
    ttl_secs: u64,
) -> Result<CallbackRequest> {
    let doc = doc
        .canonicalize()
        .context("could not canonicalize document path")?;
    let root = project_root_for(&doc)?;
    let hash = document_hash(&doc)?;
    let request = callback_request(
        doc.to_string_lossy(),
        hash.clone(),
        operations.iter().copied(),
        context,
        now_secs(),
        ttl_secs,
        uuid::Uuid::new_v4().to_string(),
    );
    save_state(
        &root,
        &doc,
        &hash,
        &CallbackState {
            request: request.clone(),
            response: None,
        },
    )?;
    Ok(request)
}

pub fn read_response(doc: &Path) -> Result<Option<CallbackResponse>> {
    let Some((_root, _hash, state)) = load_state(doc)? else {
        return Ok(None);
    };
    Ok(state
        .response
        .filter(|response| callback_response_matches_request(response, Some(&state.request))))
}

pub fn read_request(doc: &Path) -> Result<Option<CallbackRequest>> {
    Ok(load_state(doc)?.map(|(_, _, state)| state.request))
}

pub fn write_response(
    doc: &Path,
    request_id: &str,
    status: &str,
    summary: &str,
    patches: Option<Vec<CallbackPatch>>,
) -> Result<()> {
    let doc = canonical_doc(doc);
    let Some((root, hash, mut state)) = load_state(&doc)? else {
        anyhow::bail!("no pending callback request for this document");
    };
    if state.request.request_id != request_id {
        anyhow::bail!("request_id mismatch — stale or wrong request");
    }
    state.response = Some(callback_response(
        request_id,
        status,
        summary,
        None::<String>,
        patches,
        now_secs(),
    ));
    save_state(&root, &doc, &hash, &state)
}

pub fn delete_response(doc: &Path) -> Result<()> {
    let doc = canonical_doc(doc);
    let Some((root, hash, mut state)) = load_state(&doc)? else {
        return Ok(());
    };
    state.response = None;
    save_state(&root, &doc, &hash, &state)
}

pub fn cleanup_expired(project_root: &Path, _max_age_secs: u64) -> Result<()> {
    if !state_db_path(project_root).exists() {
        return Ok(());
    }
    let connection = open_state_db(project_root)?;
    let now = now_secs();
    for record in list_document_runtime_state_kind_from_db(&connection, CALLBACK_STATE_KIND)? {
        let state: CallbackState = match serde_json::from_str(&record.payload_json) {
            Ok(state) => state,
            Err(error) => {
                eprintln!(
                    "[callback] ignored malformed state for {}: {error}",
                    record.canonical_path
                );
                continue;
            }
        };
        if callback_request_is_expired(&state.request, now) {
            clear_document_runtime_state_in_db(
                &connection,
                &record.document_hash,
                CALLBACK_STATE_KIND,
            )?;
            eprintln!(
                "[callback] removed expired request: {}",
                record.canonical_path
            );
        }
    }
    Ok(())
}

pub fn scan_pending_callbacks(project_root: Option<&str>) -> Result<Vec<PendingCallback>> {
    let root = if let Some(root) = project_root {
        PathBuf::from(root)
    } else {
        let cwd = std::env::current_dir()?;
        let Some(root) = agent_doc_fs::find_project_root(&cwd) else {
            return Ok(Vec::new());
        };
        root
    };
    if !state_db_path(&root).exists() {
        return Ok(Vec::new());
    }
    let connection = open_state_db(&root)?;
    let now = now_secs();
    let mut pending = Vec::new();
    for record in list_document_runtime_state_kind_from_db(&connection, CALLBACK_STATE_KIND)? {
        let Ok(state) = serde_json::from_str::<CallbackState>(&record.payload_json) else {
            continue;
        };
        if state.response.is_none() && !callback_request_is_expired(&state.request, now) {
            pending.push(pending_callback_from_request(state.request, now));
        }
    }
    Ok(pending)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("test.md");
        fs::write(&doc, "---\nagent_doc_session: test\n---\nHello\n").unwrap();
        (tmp, doc)
    }

    #[test]
    fn request_and_response_round_trip_through_state_db() {
        let (tmp, doc) = setup();
        let request = create_request(&doc, &["compact", "prune-pending"], None, 300).unwrap();
        assert_eq!(
            read_request(&doc).unwrap().unwrap().request_id,
            request.request_id
        );
        assert!(!tmp.path().join(".agent-doc/callbacks").exists());
        write_response(&doc, &request.request_id, "success", "done", None).unwrap();
        assert_eq!(read_response(&doc).unwrap().unwrap().status, "success");
        delete_response(&doc).unwrap();
        assert!(read_response(&doc).unwrap().is_none());
    }

    #[test]
    fn mismatched_request_id_is_rejected() {
        let (_tmp, doc) = setup();
        create_request(&doc, &["compact"], None, 300).unwrap();
        assert!(write_response(&doc, "wrong", "success", "bad", None).is_err());
        assert!(read_response(&doc).unwrap().is_none());
    }

    #[test]
    fn cleanup_and_pending_scan_share_the_state_machine() {
        let (tmp, doc) = setup();
        let request = create_request(&doc, &["compact"], None, 300).unwrap();
        let pending = scan_pending_callbacks(Some(tmp.path().to_str().unwrap())).unwrap();
        assert_eq!(pending.len(), 1);
        write_response(&doc, &request.request_id, "success", "done", None).unwrap();
        assert!(
            scan_pending_callbacks(Some(tmp.path().to_str().unwrap()))
                .unwrap()
                .is_empty()
        );

        let expired = create_request(&doc, &["compact"], None, 0).unwrap();
        assert_eq!(
            read_request(&doc).unwrap().unwrap().request_id,
            expired.request_id
        );
        std::thread::sleep(std::time::Duration::from_millis(1_100));
        cleanup_expired(tmp.path(), 0).unwrap();
        assert!(read_request(&doc).unwrap().is_none());
    }
}
