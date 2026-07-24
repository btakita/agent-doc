//! Owner-pane wedge state-ledger I/O.
//!
//! `#recguard-wedge-escape`: persist a small counter for a *wedged*
//! owner-pane self-invocation loop and let orchestration break it.
//!
//! The recursive-direct-invocation guard refuses to dispatch when
//! `agent-doc <FILE>` runs inside the Codex pane that already owns the document
//! because re-entering the pane would deadlock. The Stop-hook redirect
//! (`#codex-self-reinvoke-prevent`, Option B) keeps the *end-of-turn*
//! continuation in-pane, but a busy agent that re-invokes the CLI **mid-turn**
//! still trips the guard. In a self-driving `agent:queue auto` loop with no
//! operator watching, the same active queue head can trip the guard every cycle
//! because the loop re-invokes and fails without ever advancing.
//!
//! This module tracks the count of *consecutive* self-invocation guard fires for
//! the same head. A single transient self-invoke is normal; the same head
//! tripping the focused owner-pane wedge threshold with no advance is a proven
//! dead-loop. At that point the caller halts the runaway auto-queue
//! (`queue: stop`) so the loop stops re-firing and the operator gets one clear
//! recovery action instead of an unbounded retry storm.
//!
//! A different head (the queue advanced) resets the counter, so a healthy loop
//! that occasionally self-invokes never escalates.

use anyhow::{Context, Result};
use std::path::Path;
use std::time::Duration;

#[cfg(test)]
use agent_doc_turn::owner_pane_recursion::owner_pane_wedge_threshold_reached;
use agent_doc_turn::owner_pane_recursion::{OwnerPaneWedgeRecord, record_owner_pane_wedge_fire};

const OWNER_PANE_WEDGE_STATE_KIND: &str = "owner_pane_wedge";
/// The controller command name for the wedge RMW (mirrored in the controller
/// dispatch table).
const RECORD_CMD: &str = "record_owner_pane_wedge";
const CLEAR_CMD: &str = "clear_owner_pane_wedge";

fn state_identity(file: &Path) -> Result<Option<(std::path::PathBuf, String, String)>> {
    let canonical = std::fs::canonicalize(file)?;
    let Some(root) = agent_doc_fs::find_project_root(&canonical) else {
        return Ok(None);
    };
    let hash = agent_doc_fs::document_state_hash(&canonical)?;
    Ok(Some((root, hash, canonical.to_string_lossy().into_owned())))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// A controller-envelope response (`{ ok, data | error }`).
#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct ControllerEnvelope {
    ok: bool,
    data: Option<serde_json::Value>,
    error: Option<String>,
}

/// Send a controller command, returning the parsed envelope. Returns `None`
/// when no controller socket exists (actorless/bootstrap boundary) so the
/// caller falls back to the direct SQLite path.
fn try_controller_envelope(
    root: &Path,
    command: &str,
    canonical_path: &str,
    document_hash: &str,
    head: Option<&str>,
) -> Result<Option<ControllerEnvelope>> {
    let socket = agent_doc_controller::paths::socket_path(root);
    if !socket.exists() {
        return Ok(None);
    }
    let payload = serde_json::json!({
        "document_hash": document_hash,
        "head": head.unwrap_or(""),
    });
    let request = serde_json::json!({
        "command": command,
        "file": canonical_path,
        "diagnostic_payload": serde_json::to_string(&payload)?,
    });
    let raw = agent_doc_state_wire::send_ndjson_request_to_actor(
        &socket,
        &request,
        Duration::from_secs(5),
    )
    .context("submit owner-pane wedge command through the Lazily controller")?;
    let envelope: ControllerEnvelope = serde_json::from_str(&raw)
        .context("decode owner-pane wedge controller response")?;
    Ok(Some(envelope))
}

/// Record one owner-pane self-invocation guard fire for `head` and return the
/// new consecutive count. A new head resets the count to 1. Best-effort: if the
/// project root or ledger is unavailable the call still reports `1` so the
/// caller falls through to the normal fail-closed diagnostic.
///
/// `#lazily-hot-path`: the read-modify-write is arbitrated by the live
/// controller (the SQLite authority) when one is running; the controller does
/// the RMW server-side over its own `state.db`. The direct SQLite path runs only
/// for the actorless/bootstrap boundary (no controller socket), mirroring the
/// durable-sink command-plane split.
pub fn record(file: &Path, head: &str) -> Result<u32> {
    let Some((root, document_hash, canonical_path)) = state_identity(file)? else {
        return Ok(1);
    };
    if let Some(envelope) = try_controller_envelope(
        &root,
        RECORD_CMD,
        &canonical_path,
        &document_hash,
        Some(head),
    )? && envelope.ok
    {
        let count = envelope
            .data
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;
        return Ok(count);
    }
    // Controller absent or rejected — fall through to the direct SQLite path so
    // the actorless boundary and a transient controller error never disable the
    // wedge backstop.
    record_via_sqlite(&root, &document_hash, &canonical_path, head)
}

fn record_via_sqlite(
    root: &Path,
    document_hash: &str,
    canonical_path: &str,
    head: &str,
) -> Result<u32> {
    let mut conn = agent_doc_sqlite::state_store::open_state_db(root)?;
    let tx = conn.transaction()?;
    let prior = agent_doc_sqlite::state_store::load_document_runtime_state_from_db(
        &tx,
        document_hash,
        OWNER_PANE_WEDGE_STATE_KIND,
    )?
    .and_then(|state| serde_json::from_str::<OwnerPaneWedgeRecord>(&state.payload_json).ok());
    let record = record_owner_pane_wedge_fire(prior.as_ref(), head);
    let count = record.count;
    agent_doc_sqlite::state_store::upsert_document_runtime_state_in_db(
        &tx,
        &agent_doc_sqlite::state_store::DocumentRuntimeStateRecord {
            document_hash: document_hash.to_string(),
            state_kind: OWNER_PANE_WEDGE_STATE_KIND.to_string(),
            canonical_path: canonical_path.to_string(),
            payload_json: serde_json::to_string(&record)?,
            updated_at_ms: now_ms(),
        },
    )?;
    tx.commit()?;
    Ok(count)
}

/// Clear the wedge counter (after a halt, or once the head advances).
///
/// Routes through the controller when live (same `#lazily-hot-path` split as
/// [`record`]); falls back to direct SQLite for the actorless boundary.
pub fn clear(file: &Path) -> Result<()> {
    let Some((root, document_hash, canonical_path)) = state_identity(file)? else {
        return Ok(());
    };
    if let Some(envelope) =
        try_controller_envelope(&root, CLEAR_CMD, &canonical_path, &document_hash, None)?
        && envelope.ok
    {
        return Ok(());
    }
    clear_via_sqlite(&root, &document_hash)
}

fn clear_via_sqlite(root: &Path, document_hash: &str) -> Result<()> {
    let conn = agent_doc_sqlite::state_store::open_state_db(root)?;
    agent_doc_sqlite::state_store::clear_document_runtime_state_in_db(
        &conn,
        document_hash,
        OWNER_PANE_WEDGE_STATE_KIND,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        dir
    }

    #[test]
    fn record_counts_consecutive_same_head_and_halts_at_threshold() {
        let dir = setup();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# doc\n").unwrap();

        assert_eq!(record(&doc, "do [#alpha]").unwrap(), 1);
        assert!(!owner_pane_wedge_threshold_reached(1));
        assert_eq!(record(&doc, "do [#alpha]").unwrap(), 2);
        assert!(!owner_pane_wedge_threshold_reached(2));
        let third = record(&doc, "do [#alpha]").unwrap();
        assert_eq!(third, 3);
        assert!(
            owner_pane_wedge_threshold_reached(third),
            "third consecutive same-head fire is a wedge"
        );
    }

    #[test]
    fn record_resets_count_when_head_advances() {
        let dir = setup();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# doc\n").unwrap();

        assert_eq!(record(&doc, "do [#alpha]").unwrap(), 1);
        assert_eq!(record(&doc, "do [#alpha]").unwrap(), 2);
        // The queue advanced to a new head — the loop is healthy, not wedged.
        assert_eq!(
            record(&doc, "do [#beta]").unwrap(),
            1,
            "a new head resets the consecutive counter"
        );
        assert!(!owner_pane_wedge_threshold_reached(1));
    }

    #[test]
    fn clear_removes_the_counter() {
        let dir = setup();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# doc\n").unwrap();

        assert_eq!(record(&doc, "do [#alpha]").unwrap(), 1);
        assert_eq!(record(&doc, "do [#alpha]").unwrap(), 2);
        clear(&doc).unwrap();
        // After a clear, counting starts over.
        assert_eq!(record(&doc, "do [#alpha]").unwrap(), 1);
        // Clearing a non-existent counter is a no-op success.
        clear(&doc).unwrap();
        clear(&doc).unwrap();
    }
}
