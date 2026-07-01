//! Loaded-context ledger model for prompt/context planning.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LoadedContextLedger {
    pub entries: Vec<LoadedContextRecord>,
    pub duplicate_expansions_suppressed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LoadedContextRecord {
    pub source_id: String,
    pub source_kind: String,
    pub path: String,
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concept_id: Option<String>,
    pub loaded_at: String,
    pub expansion_reason: String,
}

pub fn loaded_context_record(
    source_id: &str,
    source_kind: &str,
    path: &str,
    content: &str,
    concept_id: Option<&str>,
    loaded_at: &str,
    expansion_reason: &str,
) -> LoadedContextRecord {
    LoadedContextRecord {
        source_id: source_id.to_string(),
        source_kind: source_kind.to_string(),
        path: path.to_string(),
        content_hash: agent_doc_hash::content_hash(content),
        concept_id: concept_id.map(str::to_string),
        loaded_at: loaded_at.to_string(),
        expansion_reason: expansion_reason.to_string(),
    }
}

pub fn build_loaded_context_ledger(records: Vec<LoadedContextRecord>) -> LoadedContextLedger {
    let mut entries_by_key: HashMap<(String, String), LoadedContextRecord> = HashMap::new();
    let mut duplicate_expansions_suppressed = 0;
    for record in records {
        let key = (record.source_id.clone(), record.content_hash.clone());
        if let std::collections::hash_map::Entry::Vacant(entry) = entries_by_key.entry(key) {
            entry.insert(record);
        } else {
            duplicate_expansions_suppressed += 1;
        }
    }
    let mut entries = entries_by_key.into_values().collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        a.source_id
            .cmp(&b.source_id)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.content_hash.cmp(&b.content_hash))
    });
    LoadedContextLedger {
        entries,
        duplicate_expansions_suppressed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loaded_context_ledger_suppresses_duplicate_source_hashes() {
        let first = loaded_context_record(
            "agent-doc.test.source",
            "procedure",
            "runbooks/respond.md",
            "same content",
            None,
            "test",
            "first expansion",
        );
        let duplicate = loaded_context_record(
            "agent-doc.test.source",
            "procedure",
            "runbooks/respond.md",
            "same content",
            None,
            "test",
            "duplicate expansion",
        );
        let distinct = loaded_context_record(
            "agent-doc.test.source",
            "procedure",
            "runbooks/respond.md",
            "changed content",
            None,
            "test",
            "changed source expansion",
        );

        let ledger = build_loaded_context_ledger(vec![first, duplicate, distinct]);

        assert_eq!(ledger.entries.len(), 2);
        assert_eq!(ledger.duplicate_expansions_suppressed, 1);
    }

    #[test]
    fn loaded_context_record_serializes_snake_case_and_skips_missing_concept() {
        let ledger = build_loaded_context_ledger(vec![loaded_context_record(
            "agent-doc.test.source",
            "procedure",
            "runbooks/respond.md",
            "same content",
            None,
            "test",
            "first expansion",
        )]);

        let value = serde_json::to_value(&ledger).unwrap();

        assert_eq!(value["duplicate_expansions_suppressed"], 0);
        assert!(value["entries"][0].get("content_hash").is_some());
        assert!(value["entries"][0].get("concept_id").is_none());
    }
}
