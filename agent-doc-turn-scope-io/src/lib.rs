//! Durable per-document state-ledger I/O for the current turn's [`TurnScope`] manifest
//! (`#nm1x`, `tasks/agent-doc/plan-operation-scoped-drift.md`).
//!
//! ## Spec
//! - `save(doc, scope)`: transactionally persist the JSON-encoded scope in the
//!   project `state.db`, keyed by canonical document hash.
//! - `load(doc)`: return the ledger scope, or `None` when absent/unavailable.
//! - `delete(doc)`: idempotently remove the ledger row.
//!
//! ## Agentic Contracts
//! - Preflight derives the [`TurnScope`] at turn start and persists it here so the
//!   later finalize-path drift gate (a *separate* process invocation) can intersect
//!   incoming document ops against the same scope instead of treating every
//!   non-exchange component change as turn-affecting.
//! - Persistence is best-effort: a write failure must never block a preflight
//!   cycle, and missing state makes the gate fall back to its coarse,
//!   conservative behavior (block on any non-exchange drift).

use agent_doc_turn::turn_scope::TurnScope;
use anyhow::{Context, Result};
use std::path::Path;

const TURN_SCOPE_STATE_KIND: &str = "turn_scope";

fn state_identity(doc: &Path) -> Result<(std::path::PathBuf, String, String)> {
    let canonical = std::fs::canonicalize(doc)
        .with_context(|| format!("failed to canonicalize {}", doc.display()))?;
    let root = agent_doc_fs::find_project_root(&canonical)
        .or_else(|| canonical.parent().map(Path::to_path_buf))
        .context("turn scope document has no project root")?;
    let hash = agent_doc_fs::document_state_hash(&canonical)?;
    Ok((root, hash, canonical.to_string_lossy().into_owned()))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// Persist the current turn's scope manifest in the authoritative ledger.
pub fn save(doc: &Path, scope: &TurnScope) -> Result<()> {
    let encoded = serde_json::to_string(scope).context("failed to encode turn scope")?;
    let (root, document_hash, canonical_path) = state_identity(doc)?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::upsert_document_runtime_state_in_db(
        &conn,
        &agent_doc_sqlite::state_store::DocumentRuntimeStateRecord {
            document_hash,
            state_kind: TURN_SCOPE_STATE_KIND.to_string(),
            canonical_path,
            payload_json: encoded,
            updated_at_ms: now_ms(),
        },
    )
}

/// Load the persisted turn scope, conservatively returning `None` if ledger
/// access or decoding fails.
pub fn load(doc: &Path) -> Option<TurnScope> {
    let result = (|| -> Result<Option<TurnScope>> {
        let (root, document_hash, _) = state_identity(doc)?;
        let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
        let Some(record) = agent_doc_sqlite::state_store::load_document_runtime_state_from_db(
            &conn,
            &document_hash,
            TURN_SCOPE_STATE_KIND,
        )?
        else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_str(&record.payload_json)?))
    })();
    match result {
        Ok(scope) => scope,
        Err(err) => {
            eprintln!(
                "[agent-doc] turn scope ledger read failed for {}: {err:#}",
                doc.display()
            );
            None
        }
    }
}

/// Remove the turn-scope ledger row. Idempotent.
pub fn delete(doc: &Path) -> Result<()> {
    let (root, document_hash, _) = state_identity(doc)?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::clear_document_runtime_state_in_db(
        &conn,
        &document_hash,
        TURN_SCOPE_STATE_KIND,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_turn::turn_scope::Address;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn doc_in(dir: &TempDir) -> PathBuf {
        // A `.agent-doc/` directory makes `find_project_root` resolve here.
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# Plan\n").unwrap();
        doc
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = TempDir::new().unwrap();
        let doc = doc_in(&dir);
        let scope = TurnScope::for_driver(Some(Address::node("queue", 0, "queue:0:driver:0")));
        save(&doc, &scope).unwrap();
        assert_eq!(load(&doc), Some(scope));
    }

    #[test]
    fn load_returns_none_when_absent() {
        let dir = TempDir::new().unwrap();
        let doc = doc_in(&dir);
        assert_eq!(load(&doc), None);
    }

    #[test]
    fn save_never_emits_a_turn_scope_sidecar() {
        let dir = TempDir::new().unwrap();
        let doc = doc_in(&dir);
        save(&doc, &TurnScope::for_driver(None)).unwrap();
        assert!(!dir.path().join(".agent-doc/turn-scope").exists());
    }

    #[test]
    fn delete_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let doc = doc_in(&dir);
        // Absent → ok.
        delete(&doc).unwrap();
        let scope = TurnScope::for_driver(None);
        save(&doc, &scope).unwrap();
        delete(&doc).unwrap();
        assert_eq!(load(&doc), None);
    }
}
