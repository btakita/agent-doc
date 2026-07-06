//! Response materialization IO diagnostics for write/IPC paths.

use anyhow::Result;
use std::path::Path;

use agent_doc_template::response_materialization::strip_partial_response_materialization_from_exchange;
use agent_doc_turn::response_replay::response_materialized_in_content;

pub fn ipc_response_materialized_or_fallback_with_recycle(
    file: &Path,
    source: &str,
    response: &str,
    content: &str,
    schedule_stale_supervisor_recycle: impl FnOnce(&Path, &str),
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
    log_ipc_proof_failure_with_recycle(
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
        schedule_stale_supervisor_recycle,
    );
    false
}

pub fn log_ipc_proof_failure(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    invariant: &str,
    recovery: &str,
    detail: &str,
) {
    log_ipc_proof_failure_with_recycle(
        file,
        source,
        patch_id,
        invariant,
        recovery,
        detail,
        |_, _| {},
    );
}

pub fn log_ipc_proof_failure_with_recycle(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    invariant: &str,
    recovery: &str,
    detail: &str,
    schedule_stale_supervisor_recycle: impl FnOnce(&Path, &str),
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
        schedule_stale_supervisor_recycle(file, source);
    }
}

pub fn log_partial_response_materialization_for_retry(
    file: &Path,
    source: &str,
    response: &str,
) -> Result<()> {
    let Ok(current) = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "log_partial_response_materialization_for_retry",
    ) else {
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
