//! Append-only proof ledger I/O for workflow operations.
//!
//! This module is a narrow substrate for controller cutover work: runtime paths
//! can record a durable proof row for every queue head, response capture, patch
//! write, actor generation, and terminal proof without making the row mutable.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofOperationKind {
    QueueHead,
    ResponseCapture,
    PatchWrite,
    ActorGeneration,
    TerminalProof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofOutcome {
    Consumed,
    Deferred,
    Retried,
    Superseded,
    Recorded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofEvidenceKind {
    ActorGenerationObserved,
    CaptureRecord,
    DispatchReceipt,
    PatchContentHash,
    QueueHeadIdentity,
    ResponseBodyHash,
    SupersessionProof,
    TerminalStateObserved,
    WriteResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationProofRecord {
    pub schema_version: u8,
    pub operation_id: String,
    pub operation_kind: ProofOperationKind,
    pub outcome: ProofOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<String>,
    pub content_hash: String,
    pub proof_kind: ProofEvidenceKind,
    pub proof: String,
    pub recorded_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationProofInput {
    pub operation_id: String,
    pub operation_kind: ProofOperationKind,
    pub outcome: ProofOutcome,
    pub subject_id: Option<String>,
    pub content_hash: String,
    pub proof_kind: ProofEvidenceKind,
    pub proof: String,
    pub recorded_at_ms: u64,
}

impl OperationProofRecord {
    pub const SCHEMA_VERSION: u8 = 1;

    pub fn new(input: OperationProofInput) -> Result<Self> {
        let record = Self {
            schema_version: Self::SCHEMA_VERSION,
            operation_id: input.operation_id,
            operation_kind: input.operation_kind,
            outcome: input.outcome,
            subject_id: input.subject_id,
            content_hash: input.content_hash,
            proof_kind: input.proof_kind,
            proof: input.proof,
            recorded_at_ms: input.recorded_at_ms,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn operation_key(&self) -> String {
        format!("{}:{}", self.operation_id, self.content_hash)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != Self::SCHEMA_VERSION {
            bail!(
                "unsupported operation proof ledger schema version {}",
                self.schema_version
            );
        }
        if self.operation_id.trim().is_empty() {
            bail!("operation proof ledger record missing operation_id");
        }
        if self.content_hash.trim().is_empty() {
            bail!("operation proof ledger record missing content_hash");
        }
        if self.proof.trim().is_empty() {
            bail!("operation proof ledger record missing proof");
        }
        Ok(())
    }
}

pub fn proof_ledger_path(project_root: &Path, document_path: &Path) -> PathBuf {
    let document_key = proof_ledger_document_key(project_root, document_path);
    let document_hash = agent_doc_hash::content_hash(&document_key);
    project_root
        .join(".agent-doc")
        .join("proof-ledger")
        .join(format!("{document_hash}.jsonl"))
}

pub fn proof_ledger_document_key(project_root: &Path, document_path: &Path) -> String {
    document_path
        .strip_prefix(project_root)
        .unwrap_or(document_path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn append_operation_proof(
    project_root: &Path,
    document_path: &Path,
    record: &OperationProofRecord,
) -> Result<PathBuf> {
    record.validate()?;
    let ledger_path = proof_ledger_path(project_root, document_path);
    if let Some(parent) = ledger_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "create operation proof ledger directory {}",
                parent.display()
            )
        })?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&ledger_path)
        .with_context(|| format!("open operation proof ledger {}", ledger_path.display()))?;
    serde_json::to_writer(&mut file, record).context("serialize operation proof ledger record")?;
    file.write_all(b"\n")
        .with_context(|| format!("append operation proof ledger {}", ledger_path.display()))?;
    file.flush()
        .with_context(|| format!("flush operation proof ledger {}", ledger_path.display()))?;
    Ok(ledger_path)
}

pub fn read_operation_proofs(ledger_path: &Path) -> Result<Vec<OperationProofRecord>> {
    if !ledger_path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(ledger_path)
        .with_context(|| format!("open operation proof ledger {}", ledger_path.display()))?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    for (line_index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| {
            format!(
                "read operation proof ledger {} line {}",
                ledger_path.display(),
                line_index + 1
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let record: OperationProofRecord = serde_json::from_str(&line).with_context(|| {
            format!(
                "parse operation proof ledger {} line {}",
                ledger_path.display(),
                line_index + 1
            )
        })?;
        record.validate()?;
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(
        operation_id: &str,
        operation_kind: ProofOperationKind,
        outcome: ProofOutcome,
        content: &str,
    ) -> OperationProofRecord {
        OperationProofRecord::new(OperationProofInput {
            operation_id: operation_id.to_string(),
            operation_kind,
            outcome,
            subject_id: Some("subject-1".to_string()),
            content_hash: agent_doc_hash::content_hash(content),
            proof_kind: ProofEvidenceKind::QueueHeadIdentity,
            proof: format!("proof-for-{operation_id}"),
            recorded_at_ms: 42,
        })
        .unwrap()
    }

    #[test]
    fn proof_record_rejects_missing_identity_hash_or_proof() {
        assert!(
            OperationProofRecord::new(OperationProofInput {
                operation_id: String::new(),
                operation_kind: ProofOperationKind::QueueHead,
                outcome: ProofOutcome::Consumed,
                subject_id: None,
                content_hash: agent_doc_hash::content_hash("head"),
                proof_kind: ProofEvidenceKind::QueueHeadIdentity,
                proof: "proof".to_string(),
                recorded_at_ms: 1,
            })
            .is_err()
        );
        assert!(
            OperationProofRecord::new(OperationProofInput {
                operation_id: "op-1".to_string(),
                operation_kind: ProofOperationKind::QueueHead,
                outcome: ProofOutcome::Consumed,
                subject_id: None,
                content_hash: String::new(),
                proof_kind: ProofEvidenceKind::QueueHeadIdentity,
                proof: "proof".to_string(),
                recorded_at_ms: 1,
            })
            .is_err()
        );
        assert!(
            OperationProofRecord::new(OperationProofInput {
                operation_id: "op-1".to_string(),
                operation_kind: ProofOperationKind::QueueHead,
                outcome: ProofOutcome::Consumed,
                subject_id: None,
                content_hash: agent_doc_hash::content_hash("head"),
                proof_kind: ProofEvidenceKind::QueueHeadIdentity,
                proof: String::new(),
                recorded_at_ms: 1,
            })
            .is_err()
        );
    }

    #[test]
    fn proof_ledger_path_is_stable_and_document_scoped() {
        let tmp = tempfile::tempdir().unwrap();
        let absolute_doc = tmp.path().join("tasks/agent-doc/session.md");
        let relative_doc = Path::new("tasks/agent-doc/session.md");

        assert_eq!(
            proof_ledger_document_key(tmp.path(), &absolute_doc),
            "tasks/agent-doc/session.md"
        );
        assert_eq!(
            proof_ledger_path(tmp.path(), &absolute_doc),
            proof_ledger_path(tmp.path(), relative_doc)
        );
        assert_eq!(
            proof_ledger_path(tmp.path(), &absolute_doc)
                .parent()
                .unwrap(),
            tmp.path().join(".agent-doc").join("proof-ledger")
        );
    }

    #[test]
    fn append_operation_proof_preserves_history() {
        let tmp = tempfile::tempdir().unwrap();
        let document = tmp.path().join("tasks/session.md");
        let first = record(
            "queue:alpha",
            ProofOperationKind::QueueHead,
            ProofOutcome::Consumed,
            "alpha head",
        );
        let second = record(
            "queue:alpha",
            ProofOperationKind::QueueHead,
            ProofOutcome::Superseded,
            "alpha head superseded",
        );

        let path = append_operation_proof(tmp.path(), &document, &first).unwrap();
        let second_path = append_operation_proof(tmp.path(), &document, &second).unwrap();

        assert_eq!(path, second_path);
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.lines().count(), 2);
        let records = read_operation_proofs(&path).unwrap();
        assert_eq!(records, vec![first, second]);
    }

    #[test]
    fn proof_ledger_covers_required_operation_kinds_and_outcomes() {
        let records = [
            record(
                "queue:1",
                ProofOperationKind::QueueHead,
                ProofOutcome::Consumed,
                "queue",
            ),
            record(
                "capture:1",
                ProofOperationKind::ResponseCapture,
                ProofOutcome::Retried,
                "response",
            ),
            record(
                "patch:1",
                ProofOperationKind::PatchWrite,
                ProofOutcome::Recorded,
                "patch",
            ),
            record(
                "actor:1",
                ProofOperationKind::ActorGeneration,
                ProofOutcome::Deferred,
                "actor",
            ),
            record(
                "terminal:1",
                ProofOperationKind::TerminalProof,
                ProofOutcome::Superseded,
                "terminal",
            ),
        ];

        let json = serde_json::to_string(&records).unwrap();
        for expected in [
            "queue_head",
            "response_capture",
            "patch_write",
            "actor_generation",
            "terminal_proof",
            "consumed",
            "deferred",
            "retried",
            "superseded",
            "recorded",
        ] {
            assert!(json.contains(expected), "{json}");
        }
        assert!(records[0].operation_key().starts_with("queue:1:"));
    }
}
