//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_template::response_materialization::strip_partial_response_materialization_from_exchange;

pub(crate) fn ipc_response_materialized_or_fallback(
    file: &Path,
    source: &str,
    response: &str,
    content: &str,
) -> bool {
    if response_materialized_in_content(response, content) {
        return true;
    }
    let response_hash = agent_doc_hash::content_hash(response);
    let content_hash = agent_doc_hash::content_hash(content);
    eprintln!(
        "[write] IPC {} consumed a patch for {}, but the materialized content is missing the captured response body — retry required before snapshot/commit",
        source,
        file.display()
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_materialization_missing_response file={} source={} response_sha256={} content_len={} content_hash={}",
            file.display(),
            source,
            response_hash,
            content.len(),
            content_hash
        ),
    );
    log_ipc_proof_failure(
        file,
        source,
        None,
        "missing_response_probe",
        "retry_without_disk_write",
        &format!(
            "response_sha256={} content_len={} content_hash={}",
            response_hash,
            content.len(),
            content_hash
        ),
    );
    false
}

pub(crate) fn log_ipc_proof_failure(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    invariant: &str,
    recovery: &str,
    detail: &str,
) {
    eprintln!(
        "[write] IPC proof insufficient for {}: source={} patch_id={} invariant={} recovery={}{}{}",
        file.display(),
        source,
        patch_id.unwrap_or("-"),
        invariant,
        recovery,
        if detail.is_empty() { "" } else { " " },
        detail
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_proof_insufficient file={} source={} patch_id={} invariant={} recovery={}{}{}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            invariant,
            recovery,
            if detail.is_empty() { "" } else { " " },
            detail
        ),
    );
    // `#turnsaferecycle` Goal 2 — a `retry_without_disk_write` proof failure against a
    // STALE supervisor is a doomed IPC write; schedule an immediate forced PCP recycle
    // (fail-open, gated on proven staleness inside the helper) rather than let the
    // caller keep thrashing the buffer. Only the retry-without-disk recovery class is a
    // candidate; genuine disk-fallback failures are not stale-supervisor drift.
    if recovery.contains("retry_without_disk_write") {
        super::converge::schedule_stale_supervisor_pcp_recycle(file, source);
    }
}

pub(crate) fn log_partial_response_materialization_for_retry(
    file: &Path,
    source: &str,
    response: &str,
) -> Result<()> {
    let Ok(current) = std::fs::read_to_string(file) else {
        return Ok(());
    };
    if response_materialized_in_content(response, &current) {
        return Ok(());
    }
    let Some(repaired) = strip_partial_response_materialization_from_exchange(&current, response)
    else {
        return Ok(());
    };
    eprintln!(
        "[write] IPC {} partial response materialization left in editor buffer for retry for {}",
        source,
        file.display()
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_partial_materialization_retained_for_retry file={} source={} response_sha256={} current_len={} stripped_len={}",
            file.display(),
            source,
            agent_doc_hash::content_hash(response),
            current.len(),
            repaired.len()
        ),
    );
    Ok(())
}
