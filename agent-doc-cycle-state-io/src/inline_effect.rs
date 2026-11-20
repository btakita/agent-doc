//! Durable receipts for effects executed inline by the active harness.
//!
//! The binary cannot infer an arbitrary external effect from prompt prose. The
//! harness therefore reserves a named, operation-specific observation contract
//! before it performs the effect, then records the exact native result. A
//! reservation is bound to the open cycle and its admitted prompt checkpoint.
//! It is insert-once, and an unresolved reservation prevents both closeout and
//! replay of another prompt into the document.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use agent_doc_sqlite::state_store;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const STATE_PREFIX: &str = "inline_effect_receipt_v1:";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum InlineEffectState {
    Reserved,
    Applied {
        observation: Value,
    },
    Failed {
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        observation: Option<Value>,
    },
}

impl InlineEffectState {
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Reserved)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InlineEffectReceipt {
    pub version: u8,
    pub receipt_id: String,
    pub cycle_id: String,
    /// Hash of the complete admitted prompt checkpoint, not harness prose.
    pub admission_fingerprint: String,
    /// Stable id chosen by the harness for this one operation in this cycle.
    pub operation_id: String,
    /// Adapter which owns the meaning of `contract` and `observation`.
    pub adapter: String,
    /// Operation-specific before/expected-state contract.
    pub contract: Value,
    pub state: InlineEffectState,
    pub updated_at_secs: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn admission_fingerprint(cycle: &crate::CycleState) -> Result<String> {
    let admitted = serde_json::json!({
        "prompt_targets": cycle.prompt_targets,
        "queue_task_id": cycle.queue_task_id,
        "turn_id": cycle.turn_id,
        "selected_free_text_queue_heads": cycle.selected_free_text_queue_heads,
        "chat_prompts": cycle.chat_prompts,
        "absorbed_steering_prompts": cycle.absorbed_steering_prompts,
    });
    Ok(agent_doc_hash::content_hash(&serde_json::to_string(
        &admitted,
    )?))
}

fn validate_name(label: &str, value: &str) -> Result<String> {
    let value = value.trim();
    anyhow::ensure!(!value.is_empty(), "inline effect {label} must not be empty");
    anyhow::ensure!(
        value.len() <= 256,
        "inline effect {label} exceeds 256 bytes"
    );
    Ok(value.to_string())
}

fn validate_contract(contract: &Value) -> Result<()> {
    anyhow::ensure!(
        contract.is_object(),
        "inline effect contract must be an operation-specific JSON object"
    );
    anyhow::ensure!(
        !contract.as_object().is_some_and(serde_json::Map::is_empty),
        "inline effect contract must name observable before/expected state"
    );
    Ok(())
}

fn state_kind(receipt_id: &str) -> String {
    format!("{STATE_PREFIX}{receipt_id}")
}

fn project(file: &Path) -> Result<(std::path::PathBuf, String, String)> {
    let canonical = file
        .canonicalize()
        .with_context(|| format!("canonicalize inline-effect document {}", file.display()))?;
    let root = agent_doc_fs::find_project_root(&canonical).context("no agent-doc project root")?;
    let hash = agent_doc_hash::path_hash(&canonical)?;
    Ok((root, hash, canonical.to_string_lossy().into_owned()))
}

fn decode(record: state_store::QueueDocumentStateRecord) -> Result<InlineEffectReceipt> {
    serde_json::from_str(&record.payload_json).with_context(|| {
        format!(
            "decode inline-effect receipt {}",
            record.state_kind.trim_start_matches(STATE_PREFIX)
        )
    })
}

fn list_in(
    conn: &state_store::Connection,
    document_hash: &str,
) -> Result<Vec<InlineEffectReceipt>> {
    let mut statement = conn.prepare(
        "SELECT document_hash, state_kind, canonical_path, payload_json, updated_at_secs
         FROM queue_document_state
         WHERE document_hash = ?1 AND substr(state_kind, 1, length(?2)) = ?2
         ORDER BY updated_at_secs, state_kind",
    )?;
    let rows = statement.query_map((document_hash, STATE_PREFIX), |row| {
        let updated_at_secs: i64 = row.get(4)?;
        Ok(state_store::QueueDocumentStateRecord {
            document_hash: row.get(0)?,
            state_kind: row.get(1)?,
            canonical_path: row.get(2)?,
            payload_json: row.get(3)?,
            updated_at_secs: u64::try_from(updated_at_secs).unwrap_or_default(),
        })
    })?;
    rows.map(|row| decode(row?)).collect()
}

pub fn list(file: &Path) -> Result<Vec<InlineEffectReceipt>> {
    let (root, document_hash, _) = project(file)?;
    if !state_store::state_db_path(&root).exists() {
        return Ok(Vec::new());
    }
    let conn = state_store::open_state_db_read_only(&root)?;
    if !state_store::state_table_exists(&conn, "queue_document_state")? {
        return Ok(Vec::new());
    }
    list_in(&conn, &document_hash)
}

/// Reserve one operation before the active harness performs it.
pub fn reserve(
    file: &Path,
    operation_id: &str,
    adapter: &str,
    contract: Value,
) -> Result<InlineEffectReceipt> {
    let operation_id = validate_name("operation_id", operation_id)?;
    let adapter = validate_name("adapter", adapter)?;
    validate_contract(&contract)?;
    let cycle = crate::load(file)?
        .filter(crate::CycleState::is_open)
        .context("inline effect reservation requires an admitted open cycle")?;
    let admission_fingerprint = admission_fingerprint(&cycle)?;
    let (root, document_hash, canonical_path) = project(file)?;
    let identity = serde_json::to_string(&(
        &canonical_path,
        &cycle.cycle_id,
        &admission_fingerprint,
        &operation_id,
        &adapter,
        &contract,
    ))?;
    let receipt_id = agent_doc_hash::content_hash(&identity);
    let receipt = InlineEffectReceipt {
        version: 1,
        receipt_id: receipt_id.clone(),
        cycle_id: cycle.cycle_id,
        admission_fingerprint,
        operation_id,
        adapter,
        contract,
        state: InlineEffectState::Reserved,
        updated_at_secs: now_secs(),
    };

    let mut conn = state_store::open_state_db(&root)?;
    let tx = agent_doc_sqlite::immediate_transaction(&mut conn)?;
    let existing = list_in(&tx, &document_hash)?;
    if let Some(existing) = existing.iter().find(|existing| {
        existing.cycle_id == receipt.cycle_id && existing.operation_id == receipt.operation_id
    }) {
        anyhow::ensure!(
            existing.receipt_id == receipt.receipt_id,
            "inline effect operation {:?} in cycle {} already has a different observation contract (receipt {})",
            receipt.operation_id,
            receipt.cycle_id,
            existing.receipt_id
        );
        return Ok(existing.clone());
    }
    state_store::upsert_queue_document_state_in_db(
        &tx,
        &state_store::QueueDocumentStateRecord {
            document_hash,
            state_kind: state_kind(&receipt_id),
            canonical_path,
            payload_json: serde_json::to_string(&receipt)?,
            updated_at_secs: receipt.updated_at_secs,
        },
    )?;
    tx.commit()?;
    Ok(receipt)
}

/// Record the exact harness result for a reserved operation.
pub fn settle(
    file: &Path,
    receipt_id: &str,
    state: InlineEffectState,
) -> Result<InlineEffectReceipt> {
    anyhow::ensure!(
        receipt_id.len() == 64 && receipt_id.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid inline effect receipt_id"
    );
    anyhow::ensure!(
        state.is_terminal(),
        "inline effect settlement must be terminal"
    );
    match &state {
        InlineEffectState::Applied { observation } => {
            anyhow::ensure!(
                observation.is_object()
                    && !observation
                        .as_object()
                        .is_some_and(serde_json::Map::is_empty),
                "applied inline effect requires an operation-specific observation object"
            );
        }
        InlineEffectState::Failed { reason, .. } => {
            anyhow::ensure!(
                !reason.trim().is_empty(),
                "failed inline effect requires a reason"
            );
        }
        InlineEffectState::Reserved => unreachable!(),
    }

    let (root, document_hash, canonical_path) = project(file)?;
    let mut conn = state_store::open_state_db(&root)?;
    let tx = agent_doc_sqlite::immediate_transaction(&mut conn)?;
    let record = state_store::load_queue_document_state_from_db(
        &tx,
        &document_hash,
        &state_kind(receipt_id),
    )?
    .with_context(|| format!("unknown inline effect receipt {receipt_id}"))?;
    let mut receipt = decode(record)?;
    if receipt.state.is_terminal() {
        anyhow::ensure!(
            receipt.state == state,
            "inline effect receipt {receipt_id} is already terminal with a different result"
        );
        return Ok(receipt);
    }
    receipt.state = state;
    receipt.updated_at_secs = now_secs();
    state_store::upsert_queue_document_state_in_db(
        &tx,
        &state_store::QueueDocumentStateRecord {
            document_hash,
            state_kind: state_kind(receipt_id),
            canonical_path,
            payload_json: serde_json::to_string(&receipt)?,
            updated_at_secs: receipt.updated_at_secs,
        },
    )?;
    tx.commit()?;
    Ok(receipt)
}

pub fn unresolved(file: &Path) -> Result<Vec<InlineEffectReceipt>> {
    Ok(list(file)?
        .into_iter()
        .filter(|receipt| !receipt.state.is_terminal())
        .collect())
}

pub fn ensure_no_unresolved_before_prompt(file: &Path) -> Result<()> {
    let pending = unresolved(file)?;
    if pending.is_empty() {
        return Ok(());
    }
    let receipts = pending
        .iter()
        .map(|receipt| format!("{}:{}", receipt.receipt_id, receipt.operation_id))
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!(
        "inline effect result is unresolved for {} ({receipts}). Recover the original harness result and settle that exact receipt before admitting another prompt revision (#promptconservation)",
        file.display()
    )
}

pub fn ensure_open_cycle_terminal(file: &Path) -> Result<()> {
    let Some(cycle) = crate::load(file)?.filter(crate::CycleState::is_open) else {
        return Ok(());
    };
    let pending = unresolved(file)?
        .into_iter()
        .filter(|receipt| receipt.cycle_id == cycle.cycle_id)
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return Ok(());
    }
    let receipts = pending
        .iter()
        .map(|receipt| format!("{}:{}", receipt.receipt_id, receipt.operation_id))
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!(
        "[finalize] inline effect receipt pending for cycle {} ({receipts}); record the original harness result before closeout (#promptconservation)",
        cycle.cycle_id
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_cycle() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        std::fs::write(&file, "# Task\n").unwrap();
        crate::start_preflight(&file, Some("# Task\n"), Some("# Task\n")).unwrap();
        crate::record_turn_checkpoint(&file, &["do [#publish]".to_string()], None, None).unwrap();
        (dir, file)
    }

    #[test]
    fn reservation_is_insert_once_and_requires_exact_terminal_observation() {
        let (_dir, file) = open_cycle();
        let contract = serde_json::json!({"ref": "refs/heads/main", "before": "abc"});
        let first = reserve(&file, "publish-main", "git-ref", contract.clone()).unwrap();
        let second = reserve(&file, "publish-main", "git-ref", contract).unwrap();
        assert_eq!(first, second);
        assert!(ensure_open_cycle_terminal(&file).is_err());
        assert!(ensure_no_unresolved_before_prompt(&file).is_err());

        let applied = InlineEffectState::Applied {
            observation: serde_json::json!({"after": "def", "pushed": true}),
        };
        let settled = settle(&file, &first.receipt_id, applied.clone()).unwrap();
        assert_eq!(settled.state, applied);
        assert_eq!(
            settle(&file, &first.receipt_id, settled.state.clone()).unwrap(),
            settled
        );
        ensure_open_cycle_terminal(&file).unwrap();
        ensure_no_unresolved_before_prompt(&file).unwrap();
    }

    #[test]
    fn operation_id_cannot_change_contract_and_terminal_result_cannot_flip() {
        let (_dir, file) = open_cycle();
        let receipt = reserve(
            &file,
            "notify",
            "message-api",
            serde_json::json!({"channel": "release", "message_hash": "one"}),
        )
        .unwrap();
        let conflict = reserve(
            &file,
            "notify",
            "message-api",
            serde_json::json!({"channel": "release", "message_hash": "two"}),
        )
        .unwrap_err();
        assert!(
            conflict
                .to_string()
                .contains("different observation contract")
        );

        let failed = InlineEffectState::Failed {
            reason: "service unavailable".to_string(),
            observation: Some(serde_json::json!({"status": 503})),
        };
        settle(&file, &receipt.receipt_id, failed).unwrap();
        let conflict = settle(
            &file,
            &receipt.receipt_id,
            InlineEffectState::Applied {
                observation: serde_json::json!({"status": 200}),
            },
        )
        .unwrap_err();
        assert!(conflict.to_string().contains("different result"));
    }
}
