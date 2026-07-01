//! Durable per-document sidecar I/O for the current turn's [`TurnScope`] manifest
//! (`#nm1x`, `tasks/agent-doc/plan-operation-scoped-drift.md`).
//!
//! ## Spec
//! - `agent_doc_fs::turn_scope_path_for(doc)`: compute
//!   `<project_root>/.agent-doc/turn-scope/<hash>.json`, keyed by the same
//!   per-document state hash as every other per-doc state file so the scope
//!   colocates with the document's snapshot/baseline tree.
//! - `save(doc, scope)`: atomically persist the JSON-encoded scope (tempfile +
//!   rename), creating the parent directory on demand.
//! - `load(doc)`: return the persisted scope, or `None` when absent / unreadable
//!   / malformed.
//! - `delete(doc)`: idempotently remove the sidecar.
//!
//! ## Agentic Contracts
//! - Preflight derives the [`TurnScope`] at turn start and persists it here so the
//!   later finalize-path drift gate (a *separate* process invocation) can intersect
//!   incoming document ops against the same scope instead of treating every
//!   non-exchange component change as turn-affecting.
//! - Persistence is best-effort: a write failure must never block a preflight
//!   cycle, and a missing sidecar makes the gate fall back to its coarse,
//!   conservative behavior (block on any non-exchange drift).

use agent_doc_turn::turn_scope::TurnScope;
use anyhow::{Context, Result};
use std::path::Path;

/// Persist the current turn's scope manifest. Atomic (tempfile + rename).
pub fn save(doc: &Path, scope: &TurnScope) -> Result<()> {
    let path = agent_doc_fs::turn_scope_path_for(doc)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let encoded = serde_json::to_string(scope).context("failed to encode turn scope")?;
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    std::io::Write::write_all(&mut tmp, encoded.as_bytes())
        .with_context(|| "failed to write turn scope temp file")?;
    tmp.persist(&path)
        .with_context(|| format!("failed to rename temp file to {}", path.display()))?;
    Ok(())
}

/// Load the persisted turn scope for a document, or `None` when the sidecar is
/// absent, unreadable, or malformed. A malformed sidecar is treated as absent so
/// the gate falls back to its conservative coarse behavior rather than failing.
pub fn load(doc: &Path) -> Option<TurnScope> {
    let path = agent_doc_fs::turn_scope_path_for(doc).ok()?;
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Remove the turn-scope sidecar for a document. Idempotent.
pub fn delete(doc: &Path) -> Result<()> {
    let path = agent_doc_fs::turn_scope_path_for(doc)?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
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
    fn malformed_sidecar_loads_as_none() {
        let dir = TempDir::new().unwrap();
        let doc = doc_in(&dir);
        let path = agent_doc_fs::turn_scope_path_for(&doc).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{not valid json").unwrap();
        assert_eq!(load(&doc), None);
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
