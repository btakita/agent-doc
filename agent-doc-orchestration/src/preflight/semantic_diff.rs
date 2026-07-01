//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn is_zero_usize(n: &usize) -> bool {
    *n == 0
}

/// Persist the cycle's node ops to the durable sqlite op log. Best effort:
/// failures are logged to stderr and never propagate, so the durable substrate
/// can never block a preflight cycle.
pub(crate) fn persist_op_log(
    file: &Path,
    rc: &crate::graph::RunContext,
    origin_session: Option<&str>,
    summary: &agent_doc_diff::semantic::SemanticDiffSummary,
) {
    if summary.node_events.is_empty() {
        return;
    }
    let Some(project_root) = rc.project_root() else {
        return;
    };
    let document_path = file.to_string_lossy().to_string();
    let recorded_at = op_log_timestamp().to_string();
    let ops = agent_doc_turn::op_log::build_ops_from_semantic_diff(
        &document_path,
        origin_session,
        &recorded_at,
        summary,
    );
    if let Err(err) = agent_doc_sqlite::op_log::append_ops(&project_root, &ops) {
        eprintln!("[preflight] op-log persist skipped: {err}");
    }
}

pub(crate) fn op_log_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}
