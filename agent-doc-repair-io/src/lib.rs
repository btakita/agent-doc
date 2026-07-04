//! Repair sidecar I/O.

pub mod pending;

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub use pending::{clear_pending, save_pending};

#[derive(Debug, Serialize)]
struct BlockedRepairPayloadRecord<'a> {
    captured_at: u64,
    file: String,
    reason: &'a str,
    payload_sha256: String,
    response_body: &'a str,
}

/// Persist a blocked repair replay payload under `.agent-doc/repair-blocked`.
pub fn save_blocked_repair_payload(file: &Path, response: &str, reason: &str) -> Result<PathBuf> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = agent_doc_project_root_io::project_root_containing(&canonical)
        .or_else(|| canonical.parent().map(Path::to_path_buf))
        .context("resolve project root for blocked repair payload")?;
    let dir = root.join(".agent-doc/repair-blocked");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create blocked repair dir {}", dir.display()))?;
    let filename = format!(
        "{}-{}.json",
        agent_doc_hash::content_hash(canonical.to_string_lossy().as_ref()),
        now_millis()
    );
    let path = dir.join(filename);
    let record = BlockedRepairPayloadRecord {
        captured_at: now_secs(),
        file: canonical.display().to_string(),
        reason,
        payload_sha256: agent_doc_hash::content_hash(response),
        response_body: response,
    };
    let json = serde_json::to_string_pretty(&record)?;
    std::fs::write(&path, json)
        .with_context(|| format!("write blocked repair payload {}", path.display()))?;
    Ok(path)
}

pub fn repair_committed_historical_snapshot_drift(file: &Path) -> Result<Option<&'static str>> {
    let Some(snapshot_doc) = agent_doc_snapshot_io::load(file)? else {
        return Ok(None);
    };
    let current_doc = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    if current_doc == snapshot_doc {
        return Ok(None);
    }

    let Some(head_doc) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(None);
    };
    let historical_mutation =
        agent_doc_document_realtime::write_policy::classify_committed_historical_agent_doc_mutation(
            &snapshot_doc,
            &head_doc,
        );
    // #nm1x: intersect the drift against the current turn scope so independent
    // out-of-scope edits (e.g. a queue item added beside the running one) do not
    // block the historical snapshot repair.
    let turn_scope = agent_doc_turn_scope_io::load(file);
    let non_exchange_component_drift = agent_doc_git::has_blocking_non_exchange_component_drift(
        &snapshot_doc,
        &head_doc,
        turn_scope.as_ref(),
    );
    let historical_response_marker =
        agent_doc_turn::document_drift::detect_bypassed_response_write_between(
            &snapshot_doc,
            &head_doc,
        );
    let historical_prompt_prefix_artifact = snapshot_doc != head_doc
        && !non_exchange_component_drift
        && agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(
            &snapshot_doc,
        ) == agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(
            &head_doc,
        );
    let Some(reason) = (match historical_mutation {
        Some("exchange") => Some("exchange"),
        None if !non_exchange_component_drift && historical_response_marker.is_some() => {
            Some("exchange")
        }
        None if historical_prompt_prefix_artifact => Some("exchange"),
        _ => None,
    }) else {
        return Ok(None);
    };

    if agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(
        &current_doc,
    ) == agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(
        &head_doc,
    ) {
        agent_doc_snapshot_io::save(file, &current_doc, agent_doc_ops_log_io::log_op)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "snapshot_repair file={} reason={} basis=head",
                file.display(),
                reason
            ),
        );
        return Ok(Some(reason));
    }

    if agent_doc_turn::document_drift::detect_bypassed_response_write_between(
        &head_doc,
        &current_doc,
    )
    .is_none()
    {
        if agent_doc_write_converge_io::guard_no_stale_snapshot_reset_drift(
            file,
            Some(&head_doc),
            &current_doc,
            "historical snapshot repair",
        )? {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "snapshot_repair file={} reason={} basis=visible_rebase_guard",
                    file.display(),
                    reason
                ),
            );
            return Ok(Some(reason));
        }
        let basis = if agent_doc_git::is_safe_user_only_follow_up_after_committed_head(
            &head_doc,
            &current_doc,
        ) {
            "head_follow_up"
        } else {
            "head_local_drift"
        };
        agent_doc_snapshot_io::save(file, &head_doc, agent_doc_ops_log_io::log_op)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "snapshot_repair file={} reason={} basis={}",
                file.display(),
                reason,
                basis
            ),
        );
        return Ok(Some(reason));
    }

    Ok(None)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir_without_agent_doc_ancestor() -> tempfile::TempDir {
        for base in [
            Path::new("/var/tmp"),
            Path::new("/dev/shm"),
            Path::new("/tmp"),
        ] {
            if !base.is_dir() || agent_doc_project_root_io::project_root_containing(base).is_some()
            {
                continue;
            }
            if let Ok(dir) = tempfile::Builder::new()
                .prefix("agent-doc-repair-io-")
                .tempdir_in(base)
                && agent_doc_project_root_io::project_root_containing(dir.path()).is_none()
            {
                return dir;
            }
        }
        panic!("no writable temp base without a .agent-doc ancestor");
    }

    #[test]
    fn saves_blocked_repair_payload_under_project_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("task.md");
        std::fs::write(&doc, "---\n---\n").unwrap();

        let path = save_blocked_repair_payload(&doc, "response body", "agent markers").unwrap();

        assert!(path.starts_with(root.join(".agent-doc/repair-blocked")));
        let json = std::fs::read_to_string(path).unwrap();
        assert!(json.contains("\"reason\": \"agent markers\""));
        assert!(json.contains("\"response_body\": \"response body\""));
        assert!(json.contains("\"payload_sha256\""));
    }

    #[test]
    fn falls_back_to_file_parent_without_project_root() {
        let dir = tempdir_without_agent_doc_ancestor();
        let doc = dir.path().join("task.md");
        std::fs::write(&doc, "---\n---\n").unwrap();

        let path = save_blocked_repair_payload(&doc, "response body", "blocked").unwrap();

        assert!(path.starts_with(dir.path().join(".agent-doc/repair-blocked")));
    }
}
