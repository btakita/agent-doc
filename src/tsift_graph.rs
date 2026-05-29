//! # Module: tsift_graph
//!
//! ## Spec
//! - Optional integration with a materialized tsift graph database.
//! - For queued `do #id` / `do [#id]` items, collect `tsift graph-db evidence`
//!   and `tsift conflict-matrix` JSON, then expose compact graph handles for
//!   planning and orchestration prompts.
//! - The integration is active only when an ancestor `.tsift/graph.db` exists.
//!   Once active, successful graph reports still enforce their safety
//!   contracts, but callers may treat collection/query/validation failures as
//!   advisory and continue without graph evidence.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TSIFT_BIN_ENV: &str = "AGENT_DOC_TSIFT_BIN";
const TSIFT_GRAPH_TIMEOUT_ENV: &str = "AGENT_DOC_TSIFT_GRAPH_TIMEOUT_SECS";
const DEFAULT_TSIFT_GRAPH_TIMEOUT_SECS: u64 = 30;
const GRAPH_DB_EVIDENCE_CONTRACT_VERSION: &str = "graph-db-evidence-v1";
const CONFLICT_MATRIX_CONTRACT_VERSION: &str = "conflict-matrix-v1";
const WORKER_PROMPT_PACKET_CONTRACT_VERSION: &str = "worker-prompt-packet-v1";
const DISPATCH_TRACE_CONTRACT_VERSION: &str = "dispatch-trace-v1";
const LOWER_AGENT_JOB_PACKET_CONTRACT_VERSION: &str = "agent-doc-lower-agent-job-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftGraphEvidencePlan {
    pub(crate) targets: Vec<String>,
    pub(crate) graph_db_status: TsiftGraphDbStatus,
    pub(crate) prompt_target_handles: Vec<TsiftPromptTargetHandle>,
    pub(crate) conflict_matrix: TsiftConflictMatrixSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) dispatch_trace: Option<TsiftDispatchTraceSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) next_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftGraphDbStatus {
    pub(crate) root: Option<String>,
    pub(crate) graph_db: Option<String>,
    pub(crate) status: String,
    pub(crate) content_hash: Option<String>,
    pub(crate) source_watermark: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftPromptTargetHandle {
    pub(crate) prompt_target: String,
    pub(crate) target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) contract_version: Option<String>,
    pub(crate) evidence_packet_id: String,
    pub(crate) target_node_id: String,
    pub(crate) target_kind: String,
    pub(crate) target_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) projection_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) worker_context_handles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) source_handles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) semantic_handles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) next_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) replay_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) repair_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftConflictMatrixSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) contract_version: Option<String>,
    pub(crate) can_parallel: bool,
    pub(crate) fail_closed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) inputs: Option<TsiftConflictMatrixInputs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) context_pack: Option<TsiftConflictMatrixContextSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) candidates: Vec<TsiftConflictMatrixCandidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) conflicts: Vec<TsiftConflictMatrixConflict>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) evidence_packet_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) decisions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) worker_ownership_blocks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) worker_prompt_packets: Vec<TsiftWorkerPromptPacket>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) next_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftConflictMatrixInputs {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) graph_db_evidence_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) evidence_packets: Vec<TsiftConflictMatrixEvidencePacket>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) context_pack_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) cached_diff_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) impact_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftConflictMatrixEvidencePacket {
    pub(crate) target: String,
    pub(crate) packet_id: String,
    pub(crate) target_node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) projection_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) replay_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftConflictMatrixContextSummary {
    pub(crate) target: String,
    pub(crate) target_kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) prompt_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) touched_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) touched_symbols: Vec<String>,
    pub(crate) files_changed: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) worker_context: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) source_windows: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) status_reminders: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftConflictMatrixCandidate {
    pub(crate) target: String,
    pub(crate) rank: usize,
    pub(crate) risk: String,
    pub(crate) risk_score: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) risk_reasons: Vec<String>,
    pub(crate) evidence_packet_id: String,
    pub(crate) target_node_id: String,
    pub(crate) target_kind: String,
    pub(crate) target_label: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) owned_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) owned_symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) config_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) affected_tests: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) staged_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) staged_symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) staged_tests: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) staged_config_files: Vec<String>,
    pub(crate) semantic_dispatch_score: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) semantic_dispatch_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) worker_feedback: Option<TsiftWorkerFeedbackSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftConflictMatrixConflict {
    pub(crate) left: String,
    pub(crate) right: String,
    pub(crate) risk: String,
    pub(crate) risk_score: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) shared_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) shared_symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) shared_tests: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) shared_config_files: Vec<String>,
    pub(crate) verdict: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftWorkerPromptPacket {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) contract_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) packet_id: Option<String>,
    pub(crate) target: String,
    pub(crate) rank: usize,
    pub(crate) risk: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) projection_hash: Option<String>,
    pub(crate) title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) owned_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) owned_symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) read_only_context: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) forbidden_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) expected_tests: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) expansion_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) token_budget: Option<TsiftWorkerPromptTokenBudget>,
    pub(crate) semantic_dispatch_score: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) semantic_dispatch_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) worker_feedback: Option<TsiftWorkerFeedbackSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct TsiftWorkerPromptTokenBudget {
    pub(crate) prompt_estimated_tokens: usize,
    pub(crate) max_prompt_tokens: usize,
    pub(crate) source_window_count: usize,
    pub(crate) source_window_lines: usize,
    pub(crate) max_context_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct TsiftWorkerFeedbackSummary {
    pub(crate) total: usize,
    pub(crate) completed: usize,
    pub(crate) blocked: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) touched_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) expected_tests: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) follow_up_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) outcome_history: Vec<String>,
    pub(crate) repeated_blockage: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) stale_expected_tests: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) follow_up_debt: Vec<String>,
    pub(crate) closure_rank_score: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) closure_rank_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct TsiftProjectionFreshness {
    pub(crate) status: String,
    pub(crate) fail_closed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) content_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_watermark: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftDispatchTraceSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) contract_version: Option<String>,
    pub(crate) projection_freshness: TsiftProjectionFreshness,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) projection_hashes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) evidence_packet_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) worker_prompt_packets: Vec<TsiftWorkerPromptPacket>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) worker_feedback: Vec<TsiftWorkerFeedbackSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) graph_nodes: Vec<TsiftDispatchTraceGraphNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) graph_edges: Vec<TsiftDispatchTraceGraphEdge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) replay_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) repair_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftDispatchTraceGraphNode {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) label: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TsiftDispatchTraceGraphEdge {
    pub(crate) from_id: String,
    pub(crate) to_id: String,
    pub(crate) kind: String,
}

#[derive(Debug, Serialize)]
struct TaskGraphPromptContext<'a> {
    target: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    contract_version: Option<&'a str>,
    evidence_packet_id: &'a str,
    target_node_id: &'a str,
    target_kind: &'a str,
    target_label: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    projection_hash: Option<&'a str>,
    worker_context_handles: &'a [String],
    source_handles: &'a [String],
    semantic_handles: &'a [String],
    replay_commands: &'a [String],
    repair_commands: &'a [String],
    conflict_matrix: TaskGraphConflictContext<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_prompt_packet: Option<&'a TsiftWorkerPromptPacket>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dispatch_trace: Option<&'a TsiftDispatchTraceSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lower_agent_job_packet: Option<LowerAgentJobPacket<'a>>,
}

#[derive(Debug, Serialize)]
struct TaskGraphConflictContext<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    contract_version: Option<&'a str>,
    can_parallel: bool,
    fail_closed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    inputs: Option<&'a TsiftConflictMatrixInputs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_pack: Option<&'a TsiftConflictMatrixContextSummary>,
    candidates: &'a [TsiftConflictMatrixCandidate],
    conflicts: &'a [TsiftConflictMatrixConflict],
    evidence_packet_ids: &'a [String],
    decisions: &'a [String],
    worker_ownership_blocks: &'a [String],
    warnings: &'a [String],
}

#[derive(Debug, Serialize)]
struct LowerAgentJobPacket<'a> {
    contract_version: &'static str,
    source_contract_version: &'a str,
    packet_id: &'a str,
    evidence_packet_id: &'a str,
    target_node_id: &'a str,
    target: &'a str,
    rank: usize,
    risk: &'a str,
    projection_hash: &'a str,
    title: &'a str,
    owned_files: &'a [String],
    owned_symbols: &'a [String],
    read_only_context: &'a [String],
    forbidden_files: &'a [String],
    expected_tests: &'a [String],
    expansion_commands: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    token_budget: Option<&'a TsiftWorkerPromptTokenBudget>,
    semantic_dispatch_score: i64,
    semantic_dispatch_reasons: &'a [String],
    fail_closed_prompt: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    dispatch_trace: Option<LowerAgentDispatchTraceAudit<'a>>,
}

#[derive(Debug, Serialize)]
struct LowerAgentDispatchTraceAudit<'a> {
    contract_version: &'a str,
    projection_freshness: &'a TsiftProjectionFreshness,
    projection_hashes: &'a [String],
    evidence_packet_ids: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_feedback: Option<&'a TsiftWorkerFeedbackSummary>,
    graph_nodes: &'a [TsiftDispatchTraceGraphNode],
    graph_edges: &'a [TsiftDispatchTraceGraphEdge],
    replay_commands: &'a [String],
    repair_commands: &'a [String],
    warnings: &'a [String],
}

pub(crate) fn collect_for_do_items(
    file: &Path,
    prompt_targets: &[String],
) -> Result<Option<TsiftGraphEvidencePlan>> {
    let do_items = collect_do_items(prompt_targets);
    if do_items.is_empty() || find_materialized_graph_db(file).is_none() {
        return Ok(None);
    }

    let file_arg = file.display().to_string();
    let status_json =
        run_tsift_json(&["graph-db", "--path", file_arg.as_str(), "--json", "status"])
            .context("running tsift graph-db status")?;
    if fail_closed_freshness(&status_json) {
        anyhow::bail!(
            "tsift graph.db is not current for {}: {}",
            file.display(),
            freshness_diagnostics(&status_json).join("; ")
        );
    }

    let refresh_json =
        run_tsift_json(&["graph-db", "--path", file_arg.as_str(), "--json", "refresh"])
            .context("refreshing tsift graph-db projection before evidence collection")?;
    let graph_db_status = parse_graph_db_status(&refresh_json);
    if fail_closed_freshness(&refresh_json) || graph_db_status.status != "current" {
        anyhow::bail!(
            "tsift graph.db refresh is not current for {}: {}",
            file.display(),
            freshness_diagnostics(&refresh_json).join("; ")
        );
    }

    let mut handles = Vec::new();
    for item in &do_items {
        let evidence = run_tsift_json(&[
            "graph-db",
            "--path",
            file_arg.as_str(),
            "--json",
            "evidence",
            &item.target,
            "--depth",
            "3",
            "--limit",
            "8",
        ])
        .with_context(|| format!("collecting tsift graph-db evidence for #{}", item.target))?;
        handles.push(parse_evidence_handle(
            &item.prompt_target,
            &item.target,
            &evidence,
        )?);
    }

    let mut conflict_args = vec![
        "conflict-matrix".to_string(),
        "--path".to_string(),
        file.display().to_string(),
        "--json".to_string(),
    ];
    conflict_args.extend(do_items.iter().map(|item| item.target.clone()));
    let conflict_refs = conflict_args.iter().map(String::as_str).collect::<Vec<_>>();
    let conflict_json = run_tsift_json(&conflict_refs).context("running tsift conflict-matrix")?;
    let conflict_matrix = parse_conflict_matrix(&conflict_json);

    let mut next_commands = BTreeSet::new();
    for command in string_array(status_json.pointer("/next_commands")) {
        next_commands.insert(command);
    }
    for command in string_array(refresh_json.pointer("/next_commands")) {
        next_commands.insert(command);
    }
    for handle in &handles {
        for command in &handle.next_commands {
            next_commands.insert(command.clone());
        }
    }
    for command in &conflict_matrix.next_commands {
        next_commands.insert(command.clone());
    }

    let mut plan = TsiftGraphEvidencePlan {
        targets: do_items.into_iter().map(|item| item.target).collect(),
        graph_db_status,
        prompt_target_handles: handles,
        conflict_matrix,
        dispatch_trace: None,
        next_commands: next_commands.into_iter().collect(),
    };
    validate_graph_contracts(&plan)?;

    let dispatch_trace = collect_dispatch_trace(file, &plan.targets)?;
    let mut next_commands = plan.next_commands.iter().cloned().collect::<BTreeSet<_>>();
    for command in &dispatch_trace.replay_commands {
        next_commands.insert(command.clone());
    }
    for command in &dispatch_trace.repair_commands {
        next_commands.insert(command.clone());
    }
    plan.dispatch_trace = Some(dispatch_trace);
    plan.next_commands = next_commands.into_iter().collect();
    validate_dispatch_trace_contract(&plan)?;
    Ok(Some(plan))
}

pub(crate) fn graph_unavailable_warning(error: &anyhow::Error) -> String {
    format!("tsift graph evidence unavailable; continuing without tsift graph evidence: {error:#}")
}

pub(crate) fn manual_packet_fallback_warning(error: &anyhow::Error) -> String {
    format!(
        "{}; manual_packet_only=true",
        graph_unavailable_warning(error)
    )
}

#[cfg(test)]
fn is_recoverable_graph_db_access_error(error: &anyhow::Error) -> bool {
    is_recoverable_graph_db_access_message(&format!("{error:#}"))
}

#[cfg(test)]
fn is_recoverable_graph_db_access_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("database file is locked")
        || lower.contains("error code 5")
        || lower.contains("hot-journal")
        || lower.contains("hot journal")
        || lower.contains("read-only")
        || lower.contains("read only")
        || lower.contains("readonly database")
}

fn collect_dispatch_trace(file: &Path, targets: &[String]) -> Result<TsiftDispatchTraceSummary> {
    let mut trace_args = vec![
        "dispatch-trace".to_string(),
        "--path".to_string(),
        file.display().to_string(),
        "--json".to_string(),
        "--depth".to_string(),
        "3".to_string(),
        "--limit".to_string(),
        "8".to_string(),
    ];
    trace_args.extend(targets.iter().cloned());
    let trace_refs = trace_args.iter().map(String::as_str).collect::<Vec<_>>();
    let trace_json = run_tsift_json(&trace_refs).context("running tsift dispatch-trace")?;
    Ok(parse_dispatch_trace(&trace_json))
}

pub(crate) fn extract_do_target(text: &str) -> Option<String> {
    extract_do_targets(text).into_iter().next()
}

pub(crate) fn extract_do_targets(text: &str) -> Vec<String> {
    let mut normalized = text.trim().trim_start_matches('❯').trim();
    if normalized.starts_with('[')
        && let Some(closing) = normalized.find(']')
    {
        normalized = normalized[closing + 1..].trim();
    }
    let lower = normalized.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("do ") else {
        return Vec::new();
    };
    extract_hash_ids(rest)
}

fn extract_hash_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let chars = text.chars().collect::<Vec<_>>();
    let mut idx = 0usize;
    while idx < chars.len() {
        if chars[idx] != '#' {
            idx += 1;
            continue;
        }
        idx += 1;
        let start = idx;
        while idx < chars.len()
            && (chars[idx].is_ascii_alphanumeric() || chars[idx] == '-' || chars[idx] == '_')
        {
            idx += 1;
        }
        if start == idx {
            continue;
        }
        let id = chars[start..idx].iter().collect::<String>();
        if !ids.iter().any(|existing| existing == &id) {
            ids.push(id);
        }
    }
    ids
}

impl TsiftGraphEvidencePlan {
    pub(crate) fn prompt_context_for_task(&self, task_label: &str) -> Result<Option<String>> {
        let Some(target) = extract_do_target(task_label) else {
            return Ok(None);
        };
        let Some(handle) = self
            .prompt_target_handles
            .iter()
            .find(|handle| handle.target.eq_ignore_ascii_case(&target))
        else {
            return Ok(None);
        };
        let worker_prompt_packet = self
            .dispatch_trace
            .as_ref()
            .and_then(|trace| trace.worker_prompt_packet_for_target(&target))
            .or_else(|| {
                self.conflict_matrix
                    .worker_prompt_packets
                    .iter()
                    .find(|packet| packet.target.eq_ignore_ascii_case(&target))
            });
        let context = TaskGraphPromptContext {
            target: &handle.target,
            contract_version: handle.contract_version.as_deref(),
            evidence_packet_id: &handle.evidence_packet_id,
            target_node_id: &handle.target_node_id,
            target_kind: &handle.target_kind,
            target_label: &handle.target_label,
            projection_hash: handle.projection_hash.as_deref(),
            worker_context_handles: &handle.worker_context_handles,
            source_handles: &handle.source_handles,
            semantic_handles: &handle.semantic_handles,
            replay_commands: &handle.replay_commands,
            repair_commands: &handle.repair_commands,
            conflict_matrix: TaskGraphConflictContext {
                contract_version: self.conflict_matrix.contract_version.as_deref(),
                can_parallel: self.conflict_matrix.can_parallel,
                fail_closed: self.conflict_matrix.fail_closed,
                inputs: self.conflict_matrix.inputs.as_ref(),
                context_pack: self.conflict_matrix.context_pack.as_ref(),
                candidates: &self.conflict_matrix.candidates,
                conflicts: &self.conflict_matrix.conflicts,
                evidence_packet_ids: &self.conflict_matrix.evidence_packet_ids,
                decisions: &self.conflict_matrix.decisions,
                worker_ownership_blocks: &self.conflict_matrix.worker_ownership_blocks,
                warnings: &self.conflict_matrix.warnings,
            },
            worker_prompt_packet,
            dispatch_trace: self.dispatch_trace.as_ref(),
            lower_agent_job_packet: worker_prompt_packet.and_then(|packet| {
                packet.as_lower_agent_job_packet(handle, self.dispatch_trace.as_ref())
            }),
        };
        Ok(Some(format!(
            "<tsift_graph_evidence>\n{}\n</tsift_graph_evidence>",
            serde_json::to_string_pretty(&context)
                .context("serializing tsift graph prompt context")?
        )))
    }

    pub(crate) fn worker_result_line_for_task(
        &self,
        task_label: &str,
        response_text: &str,
    ) -> Option<String> {
        let target = extract_do_target(task_label)?;
        let packet = self
            .conflict_matrix
            .worker_prompt_packets
            .iter()
            .find(|packet| packet.target.eq_ignore_ascii_case(&target))
            .or_else(|| {
                self.dispatch_trace
                    .as_ref()
                    .and_then(|trace| trace.worker_prompt_packet_for_target(&target))
            });
        Some(render_worker_result_line(
            &target,
            response_text,
            packet
                .map(|packet| packet.owned_files.as_slice())
                .unwrap_or(&[]),
            packet
                .map(|packet| packet.expected_tests.as_slice())
                .unwrap_or(&[]),
        ))
    }

    pub(crate) fn closeout_audit_proof_for_task(&self, task_label: &str) -> Option<String> {
        let target = extract_do_target(task_label)?;
        let handle = self
            .prompt_target_handles
            .iter()
            .find(|handle| handle.target.eq_ignore_ascii_case(&target))?;
        let mut parts = vec![
            "finalize_session_check".to_string(),
            format!("evidence_packet_id={}", handle.evidence_packet_id),
            format!("target_node_id={}", handle.target_node_id),
        ];
        if let Some(hash) = handle.projection_hash.as_deref() {
            parts.push(format!("projection_hash={hash}"));
        }
        if let Some(trace) = &self.dispatch_trace {
            if let Some(version) = trace.contract_version.as_deref() {
                parts.push(format!("dispatch_trace={version}"));
            }
            if !trace.projection_hashes.is_empty() {
                parts.push(format!(
                    "trace_projection_hashes={}",
                    trace.projection_hashes.join(",")
                ));
            }
            if let Some(packet) = trace.worker_prompt_packet_for_target(&target)
                && let Some(feedback) = &packet.worker_feedback
            {
                parts.push(format!(
                    "worker_feedback=completed:{}_blocked:{}_follow_up:{}",
                    feedback.completed,
                    feedback.blocked,
                    feedback.follow_up_ids.join(",")
                ));
            }
            if !trace.replay_commands.is_empty() {
                parts.push(format!("replay_commands={}", trace.replay_commands.len()));
            }
            if !trace.repair_commands.is_empty() {
                parts.push(format!("repair_commands={}", trace.repair_commands.len()));
            }
        }
        Some(parts.join(" "))
    }
}

impl TsiftWorkerPromptPacket {
    fn as_lower_agent_job_packet<'a>(
        &'a self,
        handle: &'a TsiftPromptTargetHandle,
        dispatch_trace: Option<&'a TsiftDispatchTraceSummary>,
    ) -> Option<LowerAgentJobPacket<'a>> {
        Some(LowerAgentJobPacket {
            contract_version: LOWER_AGENT_JOB_PACKET_CONTRACT_VERSION,
            source_contract_version: self.contract_version.as_deref()?,
            packet_id: self.packet_id.as_deref()?,
            evidence_packet_id: &handle.evidence_packet_id,
            target_node_id: &handle.target_node_id,
            target: &self.target,
            rank: self.rank,
            risk: &self.risk,
            projection_hash: self.projection_hash.as_deref()?,
            title: &self.title,
            owned_files: &self.owned_files,
            owned_symbols: &self.owned_symbols,
            read_only_context: &self.read_only_context,
            forbidden_files: &self.forbidden_files,
            expected_tests: &self.expected_tests,
            expansion_commands: &self.expansion_commands,
            token_budget: self.token_budget.as_ref(),
            semantic_dispatch_score: self.semantic_dispatch_score,
            semantic_dispatch_reasons: &self.semantic_dispatch_reasons,
            fail_closed_prompt: self.prompt.as_deref()?,
            dispatch_trace: dispatch_trace.and_then(|trace| {
                Some(LowerAgentDispatchTraceAudit {
                    contract_version: trace.contract_version.as_deref()?,
                    projection_freshness: &trace.projection_freshness,
                    projection_hashes: &trace.projection_hashes,
                    evidence_packet_ids: &trace.evidence_packet_ids,
                    worker_feedback: self.worker_feedback.as_ref(),
                    graph_nodes: &trace.graph_nodes,
                    graph_edges: &trace.graph_edges,
                    replay_commands: &trace.replay_commands,
                    repair_commands: &trace.repair_commands,
                    warnings: &trace.warnings,
                })
            }),
        })
    }
}

impl TsiftDispatchTraceSummary {
    fn worker_prompt_packet_for_target(&self, target: &str) -> Option<&TsiftWorkerPromptPacket> {
        self.worker_prompt_packets
            .iter()
            .find(|packet| packet.target.eq_ignore_ascii_case(target))
    }
}

impl TsiftConflictMatrixSummary {
    pub(crate) fn parallel_dispatch_blocker(&self) -> Option<String> {
        if self.can_parallel && !self.fail_closed {
            return None;
        }

        let mut reasons = Vec::new();
        if self.fail_closed {
            reasons.push("conflict-matrix fail_closed=true".to_string());
        }
        if !self.can_parallel {
            reasons.push("conflict-matrix can_parallel=false".to_string());
        }
        reasons.extend(self.decisions.iter().cloned());
        reasons.extend(
            self.conflicts
                .iter()
                .filter(|conflict| conflict.risk != "low")
                .map(|conflict| conflict.describe()),
        );
        if reasons.is_empty() {
            reasons.push("conflict-matrix did not approve parallel dispatch".to_string());
        }
        Some(reasons.join("; "))
    }
}

impl TsiftConflictMatrixConflict {
    fn describe(&self) -> String {
        let mut parts = vec![format!(
            "pair {}<->{} risk={} verdict={}",
            self.left, self.right, self.risk, self.verdict
        )];
        if !self.shared_files.is_empty() {
            parts.push(format!("shared_files={}", self.shared_files.join(",")));
        }
        if !self.shared_symbols.is_empty() {
            parts.push(format!("shared_symbols={}", self.shared_symbols.join(",")));
        }
        if !self.shared_tests.is_empty() {
            parts.push(format!("shared_tests={}", self.shared_tests.join(",")));
        }
        if !self.shared_config_files.is_empty() {
            parts.push(format!(
                "shared_config_files={}",
                self.shared_config_files.join(",")
            ));
        }
        parts.join(" ")
    }
}

#[derive(Debug, Clone)]
struct DoItem {
    prompt_target: String,
    target: String,
}

fn collect_do_items(prompt_targets: &[String]) -> Vec<DoItem> {
    let mut seen = BTreeSet::new();
    let mut items = Vec::new();
    for prompt_target in prompt_targets {
        let Some(target) = extract_do_target(prompt_target) else {
            continue;
        };
        if seen.insert(target.to_ascii_lowercase()) {
            items.push(DoItem {
                prompt_target: prompt_target.clone(),
                target,
            });
        }
    }
    items
}

fn run_tsift_json(args: &[&str]) -> Result<Value> {
    let bin = std::env::var(TSIFT_BIN_ENV).unwrap_or_else(|_| "tsift".to_string());
    let mut child = Command::new(&bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to launch {bin}"))?;
    let command = std::iter::once(bin.as_str())
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ");
    let timeout = tsift_graph_timeout();
    let start = Instant::now();
    loop {
        if child
            .try_wait()
            .with_context(|| format!("failed to poll `{command}`"))?
            .is_some()
        {
            break;
        }
        if start.elapsed() >= timeout {
            if let Err(err) = child.kill() {
                eprintln!("[tsift_graph] warning: failed to kill timed out `{command}`: {err}");
            }
            let output = child
                .wait_with_output()
                .with_context(|| format!("failed to collect timed out `{command}`"))?;
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            anyhow::bail!(
                "`{}` timed out after {}s: {}{}{}",
                command,
                timeout.as_secs(),
                stderr.trim(),
                if stderr.trim().is_empty() || stdout.trim().is_empty() {
                    ""
                } else {
                    "\n"
                },
                stdout.trim()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = child
        .wait_with_output()
        .with_context(|| format!("failed to collect `{command}`"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "`{}` exited with status {}: {}{}{}",
            command,
            output.status,
            stderr.trim(),
            if stderr.trim().is_empty() || stdout.trim().is_empty() {
                ""
            } else {
                "\n"
            },
            stdout.trim()
        );
    }
    serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "failed to parse tsift JSON from `{}`",
            args.to_vec().join(" ")
        )
    })
}

fn tsift_graph_timeout() -> Duration {
    let secs = std::env::var(TSIFT_GRAPH_TIMEOUT_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_TSIFT_GRAPH_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

fn find_materialized_graph_db(file: &Path) -> Option<PathBuf> {
    let canonical = file.canonicalize().ok()?;
    let mut cursor = if canonical.is_dir() {
        canonical.as_path()
    } else {
        canonical.parent()?
    };
    loop {
        let graph_db = cursor.join(".tsift/graph.db");
        if graph_db.exists() {
            return Some(graph_db);
        }
        cursor = cursor.parent()?;
    }
}

fn parse_graph_db_status(value: &Value) -> TsiftGraphDbStatus {
    TsiftGraphDbStatus {
        root: string_at(value, "/root"),
        graph_db: string_at(value, "/graph_db"),
        status: string_at(value, "/freshness/status")
            .or_else(|| string_at(value, "/status"))
            .unwrap_or_else(|| "unknown".to_string()),
        content_hash: string_at(value, "/freshness/content_hash"),
        source_watermark: string_at(value, "/freshness/source_watermark"),
        diagnostics: freshness_diagnostics(value),
    }
}

fn fail_closed_freshness(value: &Value) -> bool {
    value
        .pointer("/freshness/fail_closed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn freshness_diagnostics(value: &Value) -> Vec<String> {
    string_array(value.pointer("/freshness/diagnostics"))
}

fn parse_evidence_handle(
    prompt_target: &str,
    target: &str,
    value: &Value,
) -> Result<TsiftPromptTargetHandle> {
    if fail_closed_freshness(value) {
        anyhow::bail!(
            "tsift graph-db evidence for #{} failed closed: {}",
            target,
            freshness_diagnostics(value).join("; ")
        );
    }
    let target_node_id = string_at(value, "/target_node/id")
        .with_context(|| format!("tsift graph-db evidence target not found: {target}"))?;
    let target_kind = string_at(value, "/target_node/kind").unwrap_or_default();
    let target_label = string_at(value, "/target_node/label").unwrap_or_default();
    Ok(TsiftPromptTargetHandle {
        prompt_target: prompt_target.to_string(),
        target: target.to_string(),
        contract_version: string_at(value, "/contract_version"),
        evidence_packet_id: string_at(value, "/packet_id").unwrap_or_default(),
        target_node_id,
        target_kind,
        target_label,
        projection_hash: string_at(value, "/projection_hash"),
        worker_context_handles: node_ids(value.pointer("/worker_context")),
        source_handles: node_ids(value.pointer("/source_handles")),
        semantic_handles: node_ids(value.pointer("/semantic_related")),
        next_commands: string_array(value.pointer("/next_commands")),
        replay_commands: string_array(value.pointer("/replay_commands")),
        repair_commands: string_array(value.pointer("/repair_commands")),
    })
}

fn validate_graph_contracts(plan: &TsiftGraphEvidencePlan) -> Result<()> {
    let mut errors = Vec::new();

    if plan.graph_db_status.status != "current" {
        errors.push(format!(
            "graph-db status freshness must be current, got {}",
            plan.graph_db_status.status
        ));
    }

    for handle in &plan.prompt_target_handles {
        require_version(
            &mut errors,
            &format!("graph-db evidence {}", handle.target),
            "contract_version",
            handle.contract_version.as_deref(),
            GRAPH_DB_EVIDENCE_CONTRACT_VERSION,
        );
        require_nonempty(
            &mut errors,
            &format!("graph-db evidence {}", handle.target),
            "packet_id",
            Some(handle.evidence_packet_id.as_str()),
        );
        require_nonempty(
            &mut errors,
            &format!("graph-db evidence {}", handle.target),
            "projection_hash",
            handle.projection_hash.as_deref(),
        );
        require_nonempty_list(
            &mut errors,
            &format!("graph-db evidence {}", handle.target),
            "replay_commands",
            &handle.replay_commands,
        );
        require_nonempty_list(
            &mut errors,
            &format!("graph-db evidence {}", handle.target),
            "repair_commands",
            &handle.repair_commands,
        );
    }

    require_version(
        &mut errors,
        "conflict-matrix",
        "contract_version",
        plan.conflict_matrix.contract_version.as_deref(),
        CONFLICT_MATRIX_CONTRACT_VERSION,
    );

    for handle in &plan.prompt_target_handles {
        if !handle.evidence_packet_id.is_empty()
            && !plan
                .conflict_matrix
                .evidence_packet_ids
                .iter()
                .any(|id| id == &handle.evidence_packet_id)
        {
            errors.push(format!(
                "conflict-matrix missing evidence_packet_id {} for target {}",
                handle.evidence_packet_id, handle.target
            ));
        }

        let Some(packet) = plan
            .conflict_matrix
            .worker_prompt_packets
            .iter()
            .find(|packet| packet.target.eq_ignore_ascii_case(&handle.target))
        else {
            errors.push(format!(
                "conflict-matrix missing worker_prompt_packet for target {}",
                handle.target
            ));
            continue;
        };

        let packet_label = format!("worker_prompt_packet {}", handle.target);
        require_version(
            &mut errors,
            &packet_label,
            "contract_version",
            packet.contract_version.as_deref(),
            WORKER_PROMPT_PACKET_CONTRACT_VERSION,
        );
        require_nonempty(
            &mut errors,
            &packet_label,
            "packet_id",
            packet.packet_id.as_deref(),
        );
        require_nonempty(
            &mut errors,
            &packet_label,
            "projection_hash",
            packet.projection_hash.as_deref(),
        );
        require_nonempty(
            &mut errors,
            &packet_label,
            "prompt",
            packet.prompt.as_deref(),
        );
        if let Some(prompt) = packet.prompt.as_deref()
            && !prompt.to_ascii_lowercase().contains("fail closed")
        {
            errors.push(format!(
                "{packet_label} prompt missing fail-closed instructions"
            ));
        }
        if packet.token_budget.is_none() {
            errors.push(format!("{packet_label} missing token_budget"));
        }
        if let Some(feedback) = &packet.worker_feedback {
            validate_worker_feedback_contract(&mut errors, &packet_label, feedback);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "tsift graph orchestration contract invalid: {}",
            errors.join("; ")
        )
    }
}

fn validate_dispatch_trace_contract(plan: &TsiftGraphEvidencePlan) -> Result<()> {
    let mut errors = Vec::new();
    let Some(trace) = &plan.dispatch_trace else {
        errors.push("dispatch-trace missing report".to_string());
        anyhow::bail!(
            "tsift graph orchestration contract invalid: {}",
            errors.join("; ")
        );
    };

    require_version(
        &mut errors,
        "dispatch-trace",
        "contract_version",
        trace.contract_version.as_deref(),
        DISPATCH_TRACE_CONTRACT_VERSION,
    );
    if trace.projection_freshness.fail_closed {
        errors.push(format!(
            "dispatch-trace projection freshness failed closed: {}",
            trace.projection_freshness.diagnostics.join("; ")
        ));
    }
    if trace.projection_freshness.status != "current" {
        errors.push(format!(
            "dispatch-trace projection freshness must be current, got {}",
            trace.projection_freshness.status
        ));
    }
    require_nonempty_list(
        &mut errors,
        "dispatch-trace",
        "projection_hashes",
        &trace.projection_hashes,
    );
    require_nonempty_list(
        &mut errors,
        "dispatch-trace",
        "replay_commands",
        &trace.replay_commands,
    );
    require_nonempty_list(
        &mut errors,
        "dispatch-trace",
        "repair_commands",
        &trace.repair_commands,
    );

    for handle in &plan.prompt_target_handles {
        if !trace
            .evidence_packet_ids
            .iter()
            .any(|id| id == &handle.evidence_packet_id)
        {
            errors.push(format!(
                "dispatch-trace missing evidence_packet_id {} for target {}",
                handle.evidence_packet_id, handle.target
            ));
        }
        let Some(packet) = trace.worker_prompt_packet_for_target(&handle.target) else {
            errors.push(format!(
                "dispatch-trace missing worker_prompt_packet for target {}",
                handle.target
            ));
            continue;
        };
        let packet_label = format!("dispatch-trace worker_prompt_packet {}", handle.target);
        require_version(
            &mut errors,
            &packet_label,
            "contract_version",
            packet.contract_version.as_deref(),
            WORKER_PROMPT_PACKET_CONTRACT_VERSION,
        );
        require_nonempty(
            &mut errors,
            &packet_label,
            "packet_id",
            packet.packet_id.as_deref(),
        );
        require_nonempty(
            &mut errors,
            &packet_label,
            "projection_hash",
            packet.projection_hash.as_deref(),
        );
        require_nonempty_list(
            &mut errors,
            &packet_label,
            "expansion_commands",
            &packet.expansion_commands,
        );
        if packet.token_budget.is_none() {
            errors.push(format!("{packet_label} missing token_budget"));
        }
        if packet.worker_feedback.is_none() {
            errors.push(format!("{packet_label} missing worker_feedback"));
        } else if let Some(feedback) = &packet.worker_feedback {
            validate_worker_feedback_contract(&mut errors, &packet_label, feedback);
        }
    }

    for (idx, feedback) in trace.worker_feedback.iter().enumerate() {
        validate_worker_feedback_contract(
            &mut errors,
            &format!("dispatch-trace worker_feedback {}", idx + 1),
            feedback,
        );
    }

    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "tsift graph orchestration contract invalid: {}",
            errors.join("; ")
        )
    }
}

fn require_version(
    errors: &mut Vec<String>,
    subject: &str,
    field: &str,
    actual: Option<&str>,
    expected: &str,
) {
    match actual.filter(|value| !value.trim().is_empty()) {
        Some(value) if value == expected => {}
        Some(value) => errors.push(format!(
            "{subject} unsupported {field}={value}, expected {expected}"
        )),
        None => errors.push(format!("{subject} missing {field}")),
    }
}

fn require_nonempty(errors: &mut Vec<String>, subject: &str, field: &str, actual: Option<&str>) {
    if actual.is_none_or(|value| value.trim().is_empty()) {
        errors.push(format!("{subject} missing {field}"));
    }
}

fn require_nonempty_list(errors: &mut Vec<String>, subject: &str, field: &str, actual: &[String]) {
    if actual.is_empty() {
        errors.push(format!("{subject} missing {field}"));
    }
}

fn validate_worker_feedback_contract(
    errors: &mut Vec<String>,
    subject: &str,
    feedback: &TsiftWorkerFeedbackSummary,
) {
    if feedback.total == 0 {
        return;
    }
    for warning in &feedback.warnings {
        if warning.starts_with("missing worker_result field ") {
            errors.push(format!("{subject} {warning}"));
        }
    }
}

fn parse_conflict_matrix(value: &Value) -> TsiftConflictMatrixSummary {
    TsiftConflictMatrixSummary {
        contract_version: string_at(value, "/contract_version"),
        can_parallel: value
            .pointer("/can_parallel")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        fail_closed: value
            .pointer("/fail_closed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        inputs: parse_conflict_matrix_inputs(value.pointer("/inputs")),
        context_pack: parse_conflict_matrix_context(value.pointer("/context_pack")),
        candidates: parse_conflict_matrix_candidates(value.pointer("/candidates")),
        conflicts: parse_conflict_matrix_conflicts(value.pointer("/conflicts")),
        evidence_packet_ids: string_array(value.pointer("/orchestration/evidence_packet_ids")),
        decisions: string_array(value.pointer("/orchestration/conflict_matrix_decisions")),
        worker_ownership_blocks: string_array(
            value.pointer("/orchestration/worker_ownership_blocks"),
        ),
        worker_prompt_packets: parse_worker_prompt_packets(value.pointer("/worker_prompt_packets")),
        next_commands: string_array(value.pointer("/next_commands")),
        warnings: string_array(value.pointer("/warnings")),
    }
}

fn parse_dispatch_trace(value: &Value) -> TsiftDispatchTraceSummary {
    TsiftDispatchTraceSummary {
        contract_version: string_at(value, "/contract_version"),
        projection_freshness: parse_projection_freshness(value.pointer("/projection_freshness")),
        projection_hashes: string_array(value.pointer("/projection_hashes")),
        evidence_packet_ids: string_array(value.pointer("/evidence_packet_ids")),
        worker_prompt_packets: parse_worker_prompt_packets(value.pointer("/worker_prompt_packets")),
        worker_feedback: value
            .pointer("/worker_feedback")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(parse_worker_feedback_summary)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        graph_nodes: parse_dispatch_trace_nodes(value.pointer("/nodes")),
        graph_edges: parse_dispatch_trace_edges(value.pointer("/edges")),
        replay_commands: string_array(value.pointer("/replay_commands")),
        repair_commands: string_array(value.pointer("/repair_commands")),
        warnings: string_array(value.pointer("/warnings")),
    }
}

fn parse_projection_freshness(value: Option<&Value>) -> TsiftProjectionFreshness {
    let Some(value) = value else {
        return TsiftProjectionFreshness::default();
    };
    TsiftProjectionFreshness {
        status: string_at(value, "/status").unwrap_or_else(|| "unknown".to_string()),
        fail_closed: value
            .pointer("/fail_closed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        content_hash: string_at(value, "/content_hash"),
        source_watermark: string_at(value, "/source_watermark"),
        diagnostics: string_array(value.pointer("/diagnostics")),
    }
}

fn parse_worker_prompt_packets(value: Option<&Value>) -> Vec<TsiftWorkerPromptPacket> {
    value
        .and_then(Value::as_array)
        .map(|packets| {
            packets
                .iter()
                .map(|packet| TsiftWorkerPromptPacket {
                    contract_version: string_at(packet, "/contract_version"),
                    packet_id: string_at(packet, "/packet_id"),
                    target: string_at(packet, "/target").unwrap_or_default(),
                    rank: packet.pointer("/rank").and_then(Value::as_u64).unwrap_or(0) as usize,
                    risk: string_at(packet, "/risk").unwrap_or_default(),
                    projection_hash: string_at(packet, "/projection_hash"),
                    title: string_at(packet, "/title").unwrap_or_default(),
                    owned_files: string_array(packet.pointer("/owned_files")),
                    owned_symbols: string_array(packet.pointer("/owned_symbols")),
                    read_only_context: string_array(packet.pointer("/read_only_context")),
                    forbidden_files: string_array(packet.pointer("/forbidden_files")),
                    expected_tests: string_array(packet.pointer("/expected_tests")),
                    expansion_commands: string_array(packet.pointer("/expansion_commands")),
                    token_budget: parse_worker_prompt_token_budget(packet.pointer("/token_budget")),
                    semantic_dispatch_score: i64_at(packet, "/semantic_dispatch_score"),
                    semantic_dispatch_reasons: string_array(
                        packet.pointer("/semantic_dispatch_reasons"),
                    ),
                    worker_feedback: packet
                        .pointer("/worker_feedback")
                        .map(parse_worker_feedback_summary),
                    prompt: string_at(packet, "/prompt"),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_worker_feedback_summary(value: &Value) -> TsiftWorkerFeedbackSummary {
    let mut warnings = string_array(value.pointer("/warnings"));
    for field in ["touched_files", "expected_tests", "follow_up_ids"] {
        if value.pointer(&format!("/{field}")).is_none() {
            warnings.push(format!("missing worker_result field {field}"));
        }
    }
    TsiftWorkerFeedbackSummary {
        total: usize_at(value, "/total"),
        completed: usize_at(value, "/completed"),
        blocked: usize_at(value, "/blocked"),
        touched_files: string_array(value.pointer("/touched_files")),
        expected_tests: string_array(value.pointer("/expected_tests")),
        follow_up_ids: string_array(value.pointer("/follow_up_ids")),
        outcome_history: string_array(value.pointer("/outcome_history")),
        repeated_blockage: value
            .pointer("/repeated_blockage")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        stale_expected_tests: string_array(value.pointer("/stale_expected_tests")),
        follow_up_debt: string_array(value.pointer("/follow_up_debt")),
        closure_rank_score: usize_at(value, "/closure_rank_score"),
        closure_rank_reasons: string_array(value.pointer("/closure_rank_reasons")),
        warnings,
    }
}

fn parse_dispatch_trace_nodes(value: Option<&Value>) -> Vec<TsiftDispatchTraceGraphNode> {
    value
        .and_then(Value::as_array)
        .map(|nodes| {
            nodes
                .iter()
                .map(|node| TsiftDispatchTraceGraphNode {
                    id: string_at(node, "/id").unwrap_or_default(),
                    kind: string_at(node, "/kind").unwrap_or_default(),
                    label: string_at(node, "/label").unwrap_or_default(),
                    properties: string_map(node.pointer("/properties")),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_dispatch_trace_edges(value: Option<&Value>) -> Vec<TsiftDispatchTraceGraphEdge> {
    value
        .and_then(Value::as_array)
        .map(|edges| {
            edges
                .iter()
                .map(|edge| TsiftDispatchTraceGraphEdge {
                    from_id: string_at(edge, "/from_id").unwrap_or_default(),
                    to_id: string_at(edge, "/to_id").unwrap_or_default(),
                    kind: string_at(edge, "/kind").unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_conflict_matrix_inputs(value: Option<&Value>) -> Option<TsiftConflictMatrixInputs> {
    let value = value?;
    Some(TsiftConflictMatrixInputs {
        graph_db_evidence_targets: string_array(value.pointer("/graph_db_evidence_targets")),
        evidence_packets: value
            .pointer("/evidence_packets")
            .and_then(Value::as_array)
            .map(|packets| {
                packets
                    .iter()
                    .map(|packet| TsiftConflictMatrixEvidencePacket {
                        target: string_at(packet, "/target").unwrap_or_default(),
                        packet_id: string_at(packet, "/packet_id").unwrap_or_default(),
                        target_node_id: string_at(packet, "/target_node_id").unwrap_or_default(),
                        projection_hash: string_at(packet, "/projection_hash"),
                        replay_command: string_at(packet, "/replay_command"),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        context_pack_command: string_at(value, "/context_pack_command"),
        cached_diff_command: string_at(value, "/cached_diff_command"),
        impact_command: string_at(value, "/impact_command"),
    })
}

fn parse_conflict_matrix_context(
    value: Option<&Value>,
) -> Option<TsiftConflictMatrixContextSummary> {
    let value = value?;
    Some(TsiftConflictMatrixContextSummary {
        target: string_at(value, "/target").unwrap_or_default(),
        target_kind: string_at(value, "/target_kind").unwrap_or_default(),
        prompt_targets: string_array(value.pointer("/prompt_targets")),
        touched_files: string_array(value.pointer("/touched_files")),
        touched_symbols: string_array(value.pointer("/touched_symbols")),
        files_changed: usize_at(value, "/files_changed"),
        worker_context: string_array(value.pointer("/worker_context")),
        source_windows: string_array(value.pointer("/source_windows")),
        status_reminders: string_array(value.pointer("/status_reminders")),
    })
}

fn parse_conflict_matrix_candidates(value: Option<&Value>) -> Vec<TsiftConflictMatrixCandidate> {
    value
        .and_then(Value::as_array)
        .map(|candidates| {
            candidates
                .iter()
                .map(|candidate| TsiftConflictMatrixCandidate {
                    target: string_at(candidate, "/target").unwrap_or_default(),
                    rank: usize_at(candidate, "/rank"),
                    risk: string_at(candidate, "/risk").unwrap_or_default(),
                    risk_score: usize_at(candidate, "/risk_score"),
                    risk_reasons: string_array(candidate.pointer("/risk_reasons")),
                    evidence_packet_id: string_at(candidate, "/evidence_packet_id")
                        .unwrap_or_default(),
                    target_node_id: string_at(candidate, "/target_node_id").unwrap_or_default(),
                    target_kind: string_at(candidate, "/target_kind").unwrap_or_default(),
                    target_label: string_at(candidate, "/target_label").unwrap_or_default(),
                    owned_files: string_array(candidate.pointer("/owned_files")),
                    owned_symbols: string_array(candidate.pointer("/owned_symbols")),
                    config_files: string_array(candidate.pointer("/config_files")),
                    affected_tests: string_array(candidate.pointer("/affected_tests")),
                    staged_files: string_array(candidate.pointer("/staged_overlap/files")),
                    staged_symbols: string_array(candidate.pointer("/staged_overlap/symbols")),
                    staged_tests: string_array(candidate.pointer("/staged_overlap/tests")),
                    staged_config_files: string_array(
                        candidate.pointer("/staged_overlap/config_files"),
                    ),
                    semantic_dispatch_score: i64_at(candidate, "/semantic_dispatch_score"),
                    semantic_dispatch_reasons: string_array(
                        candidate.pointer("/semantic_dispatch_reasons"),
                    ),
                    worker_feedback: candidate
                        .pointer("/worker_feedback")
                        .map(parse_worker_feedback_summary),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_conflict_matrix_conflicts(value: Option<&Value>) -> Vec<TsiftConflictMatrixConflict> {
    value
        .and_then(Value::as_array)
        .map(|conflicts| {
            conflicts
                .iter()
                .map(|conflict| TsiftConflictMatrixConflict {
                    left: string_at(conflict, "/left").unwrap_or_default(),
                    right: string_at(conflict, "/right").unwrap_or_default(),
                    risk: string_at(conflict, "/risk").unwrap_or_default(),
                    risk_score: usize_at(conflict, "/risk_score"),
                    shared_files: string_array(conflict.pointer("/shared_files")),
                    shared_symbols: string_array(conflict.pointer("/shared_symbols")),
                    shared_tests: string_array(conflict.pointer("/shared_tests")),
                    shared_config_files: string_array(conflict.pointer("/shared_config_files")),
                    verdict: string_at(conflict, "/verdict").unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_worker_prompt_token_budget(value: Option<&Value>) -> Option<TsiftWorkerPromptTokenBudget> {
    let value = value?;
    Some(TsiftWorkerPromptTokenBudget {
        prompt_estimated_tokens: usize_at(value, "/prompt_estimated_tokens"),
        max_prompt_tokens: usize_at(value, "/max_prompt_tokens"),
        source_window_count: usize_at(value, "/source_window_count"),
        source_window_lines: usize_at(value, "/source_window_lines"),
        max_context_bytes: usize_at(value, "/max_context_bytes"),
    })
}

fn node_ids(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|node| string_at(node, "/id"))
                .collect()
        })
        .unwrap_or_default()
}

fn string_at(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn usize_at(value: &Value, pointer: &str) -> usize {
    value.pointer(pointer).and_then(Value::as_u64).unwrap_or(0) as usize
}

fn i64_at(value: &Value, pointer: &str) -> i64 {
    value.pointer(pointer).and_then(Value::as_i64).unwrap_or(0)
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn string_map(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.into())))
                .collect()
        })
        .unwrap_or_default()
}

fn render_worker_result_line(
    target: &str,
    response_text: &str,
    touched_files: &[String],
    expected_tests: &[String],
) -> String {
    let status = infer_worker_result_status(response_text);
    let mut line = format!("<!-- worker_result: {status} #{target}");
    if !touched_files.is_empty() {
        line.push_str(" touched files: ");
        line.push_str(&touched_files.join(", "));
    }
    if !expected_tests.is_empty() {
        line.push_str(" tests: ");
        line.push_str(
            &expected_tests
                .iter()
                .map(|test| format!("`{test}`"))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    let follow_up_ids = extract_follow_up_ids(response_text, target);
    line.push_str(" follow-up: ");
    if follow_up_ids.is_empty() {
        line.push_str("none");
    } else {
        line.push_str(
            &follow_up_ids
                .iter()
                .map(|id| format!("#{id}"))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    line.push_str(" -->");
    line
}

fn infer_worker_result_status(response_text: &str) -> &'static str {
    let lower = response_text.to_ascii_lowercase();
    if lower.contains("externally blocked") || lower.contains("blocked") {
        "blocked"
    } else {
        "completed"
    }
}

fn extract_follow_up_ids(response_text: &str, target: &str) -> Vec<String> {
    let target = target.to_ascii_lowercase();
    let mut ids = Vec::new();
    let chars = response_text.chars().collect::<Vec<_>>();
    let mut idx = 0usize;
    while idx < chars.len() {
        if chars[idx] != '#' {
            idx += 1;
            continue;
        }
        idx += 1;
        let start = idx;
        while idx < chars.len() && chars[idx].is_ascii_alphanumeric() {
            idx += 1;
        }
        if start == idx {
            continue;
        }
        let id = chars[start..idx]
            .iter()
            .collect::<String>()
            .to_ascii_lowercase();
        if id != target && !ids.iter().any(|existing| existing == &id) {
            ids.push(id);
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::test_support::TEST_ENV_LOCK.lock().unwrap();
            let prev = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                prev,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.prev {
                unsafe { std::env::set_var(self.key, value) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    fn setup_doc() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".tsift")).unwrap();
        std::fs::write(dir.path().join(".tsift/graph.db"), "fake").unwrap();
        let doc = dir.path().join("tasks.md");
        std::fs::write(&doc, "# Tasks\n").unwrap();
        (dir, doc)
    }

    #[cfg(unix)]
    fn fake_tsift(dir: &Path, log: &Path, stale: bool) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = dir.join("fake-tsift.sh");
        let status = if stale {
            r#"{"root":"/tmp/repo","graph_db":"/tmp/repo/.tsift/graph.db","freshness":{"status":"stale","fail_closed":true,"content_hash":"old","source_watermark":"old","diagnostics":["graph.db is stale"]},"next_commands":["tsift graph-db --path /tmp/repo refresh --json"]}"#
        } else {
            r#"{"root":"/tmp/repo","graph_db":"/tmp/repo/.tsift/graph.db","freshness":{"status":"current","fail_closed":false,"content_hash":"abc","source_watermark":"abc","diagnostics":[]},"next_commands":["tsift graph-db --path /tmp/repo status --json"]}"#
        };
        let script_body = format!(
            r##"#!/bin/sh
printf '%s\n' "$*" >> "{}"
case "$*" in
  *"graph-db"*"--json status"*)
    cat <<'JSON'
{}
JSON
    ;;
  *"graph-db"*"--json refresh"*)
    cat <<'JSON'
{{"root":"/tmp/repo","graph_db":"/tmp/repo/.tsift/graph.db","operation":"refresh","status":"current","freshness":{{"status":"current","fail_closed":false,"content_hash":"abc","source_watermark":"abc","diagnostics":[]}},"next_commands":["tsift graph-db --path /tmp/repo doctor --json"]}}
JSON
    ;;
  *"graph-db"*"--json evidence agbr"*)
    if grep -q -- '--json refresh' "{}"; then
      cat <<'JSON'
{{"contract_version":"graph-db-evidence-v1","root":"/tmp/repo","backend":"sqlite","target":"agbr","packet_id":"gevd-agbr","projection_hash":"abc","freshness":{{"status":"current","fail_closed":false,"diagnostics":[]}},"target_node":{{"id":"gbak-agbr","kind":"backlog","label":"#agbr"}},"worker_context":[{{"id":"wctx-agbr"}}],"source_handles":[{{"id":"src-agbr"}}],"semantic_related":[{{"id":"sem-agbr"}}],"next_commands":["tsift graph-db --path /tmp/repo evidence agbr --json"],"replay_commands":["tsift graph-db --path /tmp/repo evidence agbr --json"],"repair_commands":["tsift graph-db --path /tmp/repo refresh --json"]}}
JSON
    else
      cat <<'JSON'
{{"contract_version":"graph-db-evidence-v1","root":"/tmp/repo","backend":"sqlite","target":"agbr","packet_id":"gevd-old","projection_hash":"old","freshness":{{"status":"current","fail_closed":false,"diagnostics":[]}},"target_node":{{"id":"gbak-agbr","kind":"backlog","label":"#agbr"}},"worker_context":[{{"id":"wctx-agbr"}}],"source_handles":[{{"id":"src-agbr"}}],"semantic_related":[{{"id":"sem-agbr"}}],"next_commands":["tsift graph-db --path /tmp/repo evidence agbr --json"],"replay_commands":["tsift graph-db --path /tmp/repo evidence agbr --json"],"repair_commands":["tsift graph-db --path /tmp/repo refresh --json"]}}
JSON
    fi
    ;;
  *"conflict-matrix"*)
    cat <<'JSON'
{{"contract_version":"conflict-matrix-v1","targets":["agbr"],"can_parallel":true,"fail_closed":false,"inputs":{{"graph_db_evidence_targets":["agbr"],"evidence_packets":[{{"target":"agbr","packet_id":"gevd-agbr","target_node_id":"gbak-agbr","projection_hash":"abc","replay_command":"tsift graph-db evidence agbr --json"}}],"context_pack_command":"tsift --envelope context-pack tasks.md --budget normal","cached_diff_command":"tsift diff-digest --cached /tmp/repo --json","impact_command":"tsift impact /tmp/repo --cached --limit 20 --json"}},"context_pack":{{"target":"tasks.md","target_kind":"agent_doc_session","prompt_targets":["do #agbr"],"touched_files":["tasks.md"],"touched_symbols":["Exchange"],"files_changed":1,"worker_context":["summary"],"source_windows":["tasks.md:1-20"],"status_reminders":[]}},"candidates":[{{"target":"agbr","rank":1,"risk":"low","risk_score":0,"risk_reasons":[],"evidence_packet_id":"gevd-agbr","target_node_id":"gbak-agbr","target_kind":"backlog","target_label":"#agbr","owned_files":["tasks.md"],"owned_symbols":["Exchange"],"config_files":[],"affected_tests":["cargo test"],"staged_overlap":{{"files":[],"symbols":[],"tests":[],"config_files":[]}},"semantic_dispatch_score":3,"semantic_dispatch_reasons":["semantic match"],"worker_feedback":{{"total":1,"completed":1,"blocked":0,"touched_files":["tasks.md"],"expected_tests":["cargo test"],"follow_up_ids":["next1"],"outcome_history":["completed #agbr"],"repeated_blockage":false,"stale_expected_tests":[],"follow_up_debt":[],"closure_rank_score":0,"closure_rank_reasons":[]}}}}],"conflicts":[],"orchestration":{{"evidence_packet_ids":["gevd-agbr"],"conflict_matrix_decisions":["candidate #1 agbr risk=low"],"worker_ownership_blocks":["Worker 1 owns agbr (#agbr)"],"projection_hashes":["abc"],"projection_freshness":{{"status":"current","fail_closed":false,"content_hash":"abc","source_watermark":"abc","diagnostics":[]}}}},"worker_prompt_packets":[{{"contract_version":"worker-prompt-packet-v1","packet_id":"wpp-agbr","target":"agbr","rank":1,"risk":"low","projection_hash":"abc","title":"Worker 1 owns agbr (#agbr)","owned_files":["tasks.md"],"owned_symbols":["Exchange"],"read_only_context":["src-agbr","semantic_rank: semantic match"],"forbidden_files":[],"expected_tests":["cargo test"],"expansion_commands":["tsift graph-db evidence agbr --json"],"token_budget":{{"prompt_estimated_tokens":20,"max_prompt_tokens":200,"source_window_count":1,"source_window_lines":20,"max_context_bytes":2400}},"semantic_dispatch_score":3,"semantic_dispatch_reasons":["semantic match"],"worker_feedback":{{"total":1,"completed":1,"blocked":0,"touched_files":["tasks.md"],"expected_tests":["cargo test"],"follow_up_ids":["next1"],"outcome_history":["completed #agbr"],"repeated_blockage":false,"stale_expected_tests":[],"follow_up_debt":[],"closure_rank_score":0,"closure_rank_reasons":[]}},"prompt":"Worker 1 owns agbr (#agbr)\n\nFail closed if the task requires a forbidden/shared file."}}],"next_commands":["tsift conflict-matrix --path /tmp/repo agbr --json"],"warnings":[]}}
JSON
    ;;
  *"dispatch-trace"*)
    cat <<'JSON'
{{"contract_version":"dispatch-trace-v1","root":"/tmp/repo","targets":["agbr"],"projection_freshness":{{"status":"current","fail_closed":false,"content_hash":"abc","source_watermark":"abc","diagnostics":[]}},"projection_hashes":["abc"],"evidence_packet_ids":["gevd-agbr"],"worker_prompt_packets":[{{"contract_version":"worker-prompt-packet-v1","packet_id":"wpp-agbr","target":"agbr","rank":1,"risk":"low","projection_hash":"abc","title":"Worker 1 owns agbr (#agbr)","owned_files":["tasks.md"],"owned_symbols":["Exchange"],"read_only_context":["src-agbr","semantic_rank: semantic match"],"forbidden_files":[],"expected_tests":["cargo test"],"expansion_commands":["tsift graph-db evidence agbr --json"],"token_budget":{{"prompt_estimated_tokens":20,"max_prompt_tokens":200,"source_window_count":1,"source_window_lines":20,"max_context_bytes":2400}},"semantic_dispatch_score":3,"semantic_dispatch_reasons":["semantic match"],"worker_feedback":{{"total":1,"completed":1,"blocked":0,"touched_files":["tasks.md"],"expected_tests":["cargo test"],"follow_up_ids":["next1"],"outcome_history":["completed #agbr"],"repeated_blockage":false,"stale_expected_tests":[],"follow_up_debt":[],"closure_rank_score":0,"closure_rank_reasons":[]}},"prompt":"Worker 1 owns agbr (#agbr)\n\nFail closed if the task requires a forbidden/shared file."}}],"worker_feedback":[{{"total":1,"completed":1,"blocked":0,"touched_files":["tasks.md"],"expected_tests":["cargo test"],"follow_up_ids":["next1"],"outcome_history":["completed #agbr"],"repeated_blockage":false,"stale_expected_tests":[],"follow_up_debt":[],"closure_rank_score":0,"closure_rank_reasons":[]}}],"summary":{{"backlog":1,"job_packet":1,"worker_result":1,"worker_context":1,"source_handle":1,"semantic_rows":0}},"nodes":[{{"id":"gbak-agbr","kind":"backlog","label":"#agbr","properties":{{"ref_id":"agbr"}}}},{{"id":"job-agbr","kind":"job_packet","label":"do #agbr","properties":{{"ref_id":"agbr"}}}},{{"id":"wres-agbr","kind":"worker_result","label":"completed #agbr","properties":{{"status":"completed","touched_files":"tasks.md","expected_tests":"cargo test","follow_up_ids":"next1"}}}}],"edges":[{{"from_id":"job-agbr","to_id":"gbak-agbr","kind":"targets","properties":{{}}}},{{"from_id":"gbak-agbr","to_id":"wres-agbr","kind":"has_result","properties":{{}}}}],"conflict_matrix_decisions":["candidate #1 agbr risk=low"],"replay_commands":["tsift conflict-matrix --path /tmp/repo agbr --json"],"repair_commands":["tsift graph-db --path /tmp/repo refresh --json"],"truncated":false,"warnings":[]}}
JSON
    ;;
  *)
    echo "unexpected fake tsift args: $*" >&2
    exit 2
    ;;
esac
"##,
            log.display(),
            status,
            log.display()
        );
        std::fs::write(&script, script_body).unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        script
    }

    #[test]
    fn extracts_do_targets_from_common_task_shapes() {
        assert_eq!(
            extract_do_target("do #agbr. spec"),
            Some("agbr".to_string())
        );
        assert_eq!(
            extract_do_target("do [#agbr]. spec"),
            Some("agbr".to_string())
        );
        assert_eq!(
            extract_do_target("[prep] do #agbr"),
            Some("agbr".to_string())
        );
        assert_eq!(extract_do_target("run tests"), None);
        assert_eq!(
            extract_do_targets("do [#x63e] [#v4v0]. spec-test"),
            vec!["x63e".to_string(), "v4v0".to_string()]
        );
        assert_eq!(
            extract_do_target("do #inline-done-signal. spec-test"),
            Some("inline-done-signal".to_string())
        );
    }

    #[test]
    fn classifies_recoverable_graph_db_access_errors() {
        let locked = anyhow::anyhow!(
            "`tsift graph-db status` exited: Error code 5: The database file is locked"
        );
        assert!(is_recoverable_graph_db_access_error(&locked));

        let hot_journal =
            anyhow::anyhow!("sqlite hot-journal/read-only recovery prevented graph-db status");
        assert!(is_recoverable_graph_db_access_error(&hot_journal));

        let stale = anyhow::anyhow!("tsift graph.db is not current: graph.db is stale");
        assert!(!is_recoverable_graph_db_access_error(&stale));
    }

    #[cfg(unix)]
    #[test]
    fn collect_for_do_items_attaches_graph_handles() {
        let (dir, doc) = setup_doc();
        let log = dir.path().join("calls.log");
        let fake = fake_tsift(dir.path(), &log, false);
        let _env = EnvGuard::set(TSIFT_BIN_ENV, fake.to_str().unwrap());

        let plan = collect_for_do_items(&doc, &["do [#agbr]. spec-test".to_string()])
            .unwrap()
            .unwrap();

        assert_eq!(plan.targets, vec!["agbr"]);
        assert_eq!(plan.graph_db_status.status, "current");
        assert_eq!(
            plan.prompt_target_handles[0].evidence_packet_id,
            "gevd-agbr"
        );
        assert_eq!(plan.conflict_matrix.evidence_packet_ids, vec!["gevd-agbr"]);
        assert_eq!(
            plan.conflict_matrix.contract_version.as_deref(),
            Some("conflict-matrix-v1")
        );
        assert_eq!(
            plan.conflict_matrix.candidates[0].owned_files,
            vec!["tasks.md"]
        );
        assert_eq!(
            plan.conflict_matrix
                .context_pack
                .as_ref()
                .unwrap()
                .touched_files,
            vec!["tasks.md"]
        );
        assert_eq!(
            plan.conflict_matrix.worker_prompt_packets[0]
                .token_budget
                .as_ref()
                .unwrap()
                .source_window_count,
            1
        );
        let context = plan
            .prompt_context_for_task("do #agbr")
            .unwrap()
            .expect("expected prompt context");
        assert!(context.contains("<tsift_graph_evidence>"));
        assert!(context.contains("\"source_handles\": ["));
        assert!(context.contains("\"Worker 1 owns agbr (#agbr)\""));
        assert!(context.contains("\"context_pack\""));
        assert!(context.contains("\"candidates\""));
        assert!(context.contains("Fail closed if the task requires a forbidden/shared file"));
        assert!(context.contains("\"lower_agent_job_packet\""));
        assert!(context.contains("\"dispatch_trace\""));
        assert!(context.contains("\"contract_version\": \"dispatch-trace-v1\""));
        assert!(context.contains("\"worker_feedback\""));
        assert!(context.contains("\"follow_up_ids\": ["));
        assert!(context.contains("\"graph_nodes\": ["));
        assert!(context.contains("\"graph_edges\": ["));
        assert!(context.contains("\"replay_commands\": ["));
        assert!(context.contains("\"repair_commands\": ["));

        let calls = std::fs::read_to_string(log).unwrap();
        assert!(calls.contains("graph-db"));
        assert!(calls.contains("refresh"));
        assert!(calls.contains("evidence agbr"));
        assert!(calls.contains("conflict-matrix"));
        assert!(calls.contains("dispatch-trace"));
    }

    #[cfg(unix)]
    #[test]
    fn collect_for_do_items_fails_closed_on_stale_graph_db() {
        let (dir, doc) = setup_doc();
        let log = dir.path().join("calls.log");
        let fake = fake_tsift(dir.path(), &log, true);
        let _env = EnvGuard::set(TSIFT_BIN_ENV, fake.to_str().unwrap());

        let err = collect_for_do_items(&doc, &["do #agbr".to_string()])
            .unwrap_err()
            .to_string();

        assert!(err.contains("not current"));
        let calls = std::fs::read_to_string(log).unwrap();
        assert!(calls.contains("status"));
        assert!(!calls.contains("evidence agbr"));
        assert!(!calls.contains("conflict-matrix"));
    }

    #[cfg(unix)]
    #[test]
    fn collect_for_do_items_fails_closed_on_missing_graph_contract_fields() {
        use std::os::unix::fs::PermissionsExt;

        let (dir, doc) = setup_doc();
        let log = dir.path().join("calls.log");
        let script = dir.path().join("fake-tsift-missing-contracts.sh");
        let script_body = format!(
            r##"#!/bin/sh
printf '%s\n' "$*" >> "{}"
case "$*" in
  *"graph-db"*"--json status"*)
    cat <<'JSON'
{{"root":"/tmp/repo","graph_db":"/tmp/repo/.tsift/graph.db","freshness":{{"status":"current","fail_closed":false,"content_hash":"abc","source_watermark":"abc","diagnostics":[]}}}}
JSON
    ;;
  *"graph-db"*"--json refresh"*)
    cat <<'JSON'
{{"root":"/tmp/repo","graph_db":"/tmp/repo/.tsift/graph.db","operation":"refresh","status":"current","freshness":{{"status":"current","fail_closed":false,"content_hash":"abc","source_watermark":"abc","diagnostics":[]}}}}
JSON
    ;;
  *"graph-db"*"--json evidence agbr"*)
    cat <<'JSON'
{{"root":"/tmp/repo","backend":"sqlite","target":"agbr","freshness":{{"status":"current","fail_closed":false,"diagnostics":[]}},"target_node":{{"id":"gbak-agbr","kind":"backlog","label":"#agbr"}},"worker_context":[],"source_handles":[]}}
JSON
    ;;
  *"conflict-matrix"*)
    cat <<'JSON'
{{"targets":["agbr"],"can_parallel":true,"fail_closed":false,"worker_prompt_packets":[]}}
JSON
    ;;
  *)
    echo "unexpected fake tsift args: $*" >&2
    exit 2
    ;;
esac
"##,
            log.display()
        );
        std::fs::write(&script, script_body).unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        let _env = EnvGuard::set(TSIFT_BIN_ENV, script.to_str().unwrap());

        let err = collect_for_do_items(&doc, &["do #agbr".to_string()])
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("graph-db evidence agbr missing contract_version"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn conflict_matrix_blocks_parallel_when_report_does_not_approve_dispatch() {
        let summary = TsiftConflictMatrixSummary {
            contract_version: Some("conflict-matrix-v1".to_string()),
            can_parallel: false,
            fail_closed: false,
            inputs: None,
            context_pack: None,
            candidates: Vec::new(),
            conflicts: vec![TsiftConflictMatrixConflict {
                left: "left".to_string(),
                right: "right".to_string(),
                risk: "high".to_string(),
                risk_score: 40,
                shared_files: Vec::new(),
                shared_symbols: vec!["shared_symbol".to_string()],
                shared_tests: Vec::new(),
                shared_config_files: Vec::new(),
                verdict: "split by file or serialize".to_string(),
            }],
            evidence_packet_ids: Vec::new(),
            decisions: vec!["pair left<->right risk=high".to_string()],
            worker_ownership_blocks: Vec::new(),
            worker_prompt_packets: Vec::new(),
            next_commands: Vec::new(),
            warnings: Vec::new(),
        };

        let blocker = summary.parallel_dispatch_blocker().unwrap();

        assert!(blocker.contains("can_parallel=false"));
        assert!(blocker.contains("shared_symbols=shared_symbol"));
    }
}
