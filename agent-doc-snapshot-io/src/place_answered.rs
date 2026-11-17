//! Latest answer recorded for each `agent:place` id.
//!
//! The row is runtime bookkeeping in `state.db` (`document_runtime_state`,
//! kind [`STATE_KIND`]), keyed by the document path hash. It is not a ledger
//! fact. A missing row means the place has not been answered or seeded.

use std::collections::BTreeMap;
use std::path::Path;

use agent_doc_document::place::PlaceAnswer;
use agent_doc_sqlite::state_store::{self, DocumentRuntimeStateRecord};
use anyhow::{Context, Result};

pub const STATE_KIND: &str = "place_answered";
pub const CHECKPOINT_KIND: &str = "place_checkpoint";

fn project_root(doc: &Path) -> Result<std::path::PathBuf> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    agent_doc_project_root_io::project_root_or_file_parent(&canonical)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

fn document_hash(doc: &Path) -> Result<String> {
    agent_doc_fs::document_state_hash(doc)
}

fn canonical_path(doc: &Path) -> String {
    doc.canonicalize()
        .unwrap_or_else(|_| doc.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Answers recorded for `doc`. Missing state is an empty map.
pub fn load_place_answers(doc: &Path) -> Result<BTreeMap<String, PlaceAnswer>> {
    let Some(raw) = load_payload(doc, STATE_KIND)? else {
        return Ok(BTreeMap::new());
    };
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
    let Some(answers) = value.get("answers").and_then(|answers| answers.as_object()) else {
        return Ok(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    for (id, entry) in answers {
        let Some(prompt_hash) = entry.get("prompt_hash").and_then(|v| v.as_str()) else {
            continue;
        };
        let output = entry.get("output").and_then(|v| v.as_str()).unwrap_or("");
        out.insert(
            id.clone(),
            PlaceAnswer {
                prompt_hash: prompt_hash.to_string(),
                output: output.to_string(),
            },
        );
    }
    Ok(out)
}

/// Merge `updates` into the recorded answers. An empty update writes nothing.
pub fn record_place_answers(doc: &Path, updates: &BTreeMap<String, PlaceAnswer>) -> Result<()> {
    if updates.is_empty() {
        return Ok(());
    }
    let mut answers = load_place_answers(doc)?;
    answers.extend(
        updates
            .iter()
            .map(|(id, answer)| (id.clone(), answer.clone())),
    );
    store_payload(
        doc,
        STATE_KIND,
        &serde_json::json!({ "answers": answers }).to_string(),
    )
}

pub fn record_place_answer(doc: &Path, id: &str, answer: &PlaceAnswer) -> Result<()> {
    let mut updates = BTreeMap::new();
    updates.insert(id.to_string(), answer.clone());
    record_place_answers(doc, &updates)
}

/// Last live checkpoint fingerprint and the time it was surfaced.
pub fn load_place_checkpoint(doc: &Path) -> Result<Option<(String, u64)>> {
    let Some(raw) = load_payload(doc, CHECKPOINT_KIND)? else {
        return Ok(None);
    };
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
    let fingerprint = value
        .get("fingerprint")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let at_ms = value.get("at_ms").and_then(|v| v.as_u64()).unwrap_or(0);
    Ok(Some((fingerprint, at_ms)))
}

pub fn record_place_checkpoint(doc: &Path, fingerprint: &str, at_ms: u64) -> Result<()> {
    store_payload(
        doc,
        CHECKPOINT_KIND,
        &serde_json::json!({ "fingerprint": fingerprint, "at_ms": at_ms }).to_string(),
    )
}

fn load_payload(doc: &Path, kind: &str) -> Result<Option<String>> {
    let root = project_root(doc)?;
    if !state_store::state_db_path(&root).exists() {
        return Ok(None);
    }
    let conn = state_store::open_state_db(&root)?;
    let row = state_store::load_document_runtime_state_from_db(&conn, &document_hash(doc)?, kind)
        .with_context(|| format!("load {kind}"))?;
    Ok(row.map(|row| row.payload_json))
}

fn store_payload(doc: &Path, kind: &str, payload: &str) -> Result<()> {
    let root = project_root(doc)?;
    let conn = state_store::open_state_db(&root)?;
    state_store::upsert_document_runtime_state_in_db(
        &conn,
        &DocumentRuntimeStateRecord {
            document_hash: document_hash(doc)?,
            state_kind: kind.to_string(),
            canonical_path: canonical_path(doc),
            payload_json: payload.to_string(),
            updated_at_ms: now_ms(),
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_round_trip_and_merge_by_id() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("note.md");
        std::fs::write(&doc, "note\n").unwrap();
        assert!(load_place_answers(&doc).unwrap().is_empty());
        record_place_answer(
            &doc,
            "step-1",
            &PlaceAnswer {
                prompt_hash: "abc".to_string(),
                output: "done".to_string(),
            },
        )
        .unwrap();
        record_place_answer(
            &doc,
            "step-2",
            &PlaceAnswer {
                prompt_hash: "def".to_string(),
                output: String::new(),
            },
        )
        .unwrap();
        let loaded = load_place_answers(&doc).unwrap();
        assert_eq!(loaded["step-1"].output, "done");
        assert_eq!(loaded["step-2"].prompt_hash, "def");
        record_place_checkpoint(&doc, "fp", 10).unwrap();
        assert_eq!(
            load_place_checkpoint(&doc)
                .unwrap()
                .as_ref()
                .map(|(fp, at)| (fp.as_str(), *at)),
            Some(("fp", 10))
        );
    }
}
