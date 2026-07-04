//! High-level write IPC transport adapters.

use agent_doc_element_boundary::boundary::find_boundary_id;
use agent_doc_element_exchange::extract_post_commit_normalization_targets;
use agent_doc_ipc_io::editor_target::target_payload_to_live_editor;
use agent_doc_ipc_protocol::{existing_patch_is_reposition_only, is_socket_ack_timeout_error};
use agent_doc_template::response_materialization::extract_response_headings_from_patches;
use agent_doc_write_converge_io::{
    cleanup_legacy_ipc_degraded, clear_ipc_socket_ack_timeouts, ipc_direct_disk_degraded,
    log_ipc_dewedge_direct_disk_skip, record_ipc_socket_ack_timeout,
};
use anyhow::Result;
use std::path::Path;

/// Result of an IPC write attempt, including the patch_id used.
///
/// The `patch_id` is returned so callers can report/retry the same logical
/// response. The plugin tracks applied patch_ids and skips duplicates,
/// preventing double-apply when both socket and file IPC fire.
#[derive(Debug)]
pub struct IpcResult {
    /// Whether the plugin successfully consumed the patch.
    pub success: bool,
    /// The patch_id used for this write attempt. Reuse in fallback writes
    /// so the plugin can deduplicate.
    pub patch_id: String,
    /// True when IPC was intentionally skipped because the current cycle has
    /// already reached the terminal committed state.
    pub skipped_committed_cycle: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIpcRepositionResult {
    Queued,
    DeferredExistingPatch,
    Unavailable,
}

/// Return `true` when every response heading carried in the incoming patches is
/// already present in the document's `HEAD` content.
///
/// This distinguishes "cycle already committed because this response landed"
/// from "cycle committed by an unrelated mid-turn operation; rotate and apply".
pub fn patch_response_headings_already_in_head(
    file: &Path,
    patches: &[agent_doc_template::PatchBlock],
) -> bool {
    let headings = extract_response_headings_from_patches(patches);
    if headings.is_empty() {
        return true;
    }
    let rc = agent_doc_run_context_io::RunContext::new(file.to_path_buf());
    let Some(head) = rc.head_content() else {
        return false;
    };
    headings.iter().all(|h| head.contains(h.as_str()))
}

pub fn queue_file_ipc_reposition_boundary(
    file: &Path,
    boundary_id: Option<&str>,
    normalize_prefix_lines: &[String],
) -> Result<FileIpcRepositionResult> {
    let canonical = file.canonicalize()?;
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
    let patches_dir = project_root.join(".agent-doc/patches");
    if !patches_dir.exists() {
        return Ok(FileIpcRepositionResult::Unavailable);
    }

    let hash = agent_doc_fs::document_state_hash(file)?;
    let patch_file = patches_dir.join(format!("{hash}.json"));
    if patch_file.exists() {
        let existing = std::fs::read_to_string(&patch_file).unwrap_or_default();
        match serde_json::from_str::<serde_json::Value>(&existing) {
            Ok(payload) if existing_patch_is_reposition_only(&payload) => {}
            Ok(_) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "file_ipc_reposition_deferred_existing_patch file={} patch_file={}",
                        file.display(),
                        patch_file.display()
                    ),
                );
                return Ok(FileIpcRepositionResult::DeferredExistingPatch);
            }
            Err(e) => {
                eprintln!(
                    "[commit] replacing unreadable file IPC reposition patch {}: {}",
                    patch_file.display(),
                    e
                );
            }
        }
    }

    let patch_id = uuid::Uuid::new_v4().to_string();
    let mut payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": [],
        "unmatched": "",
        "baseline": "",
        "patch_id": patch_id,
        "reposition_boundary": true,
        "preserve_head": true,
    });
    if let Some(boundary_id) = boundary_id {
        payload["reposition_boundary_id"] = serde_json::Value::String(boundary_id.to_string());
    }
    if !normalize_prefix_lines.is_empty() {
        payload["normalize_prefix_lines"] = serde_json::Value::Array(
            normalize_prefix_lines
                .iter()
                .map(|line| serde_json::Value::String(line.clone()))
                .collect(),
        );
    }

    // #late-ipc-patch-duplicate-stall: tag the queued file patch with the cycle
    // id + a baseline content hash of the doc it targets so a late applier can
    // fence a superseded patch instead of blindly re-applying it.
    if let Ok(Some(cs)) = agent_doc_cycle_state_io::load(file) {
        payload["cycle_id"] = serde_json::Value::String(cs.cycle_id);
    }
    if let Ok(live) = std::fs::read_to_string(file) {
        payload["baseline_hash"] = serde_json::Value::String(agent_doc_hash::content_hash(&live));
    }
    target_payload_to_live_editor(file, &mut payload, "file_reposition");

    atomic_write(&patch_file, &serde_json::to_string_pretty(&payload)?)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "file_ipc_reposition_queued file={} patch_file={} patch_id={}",
            file.display(),
            patch_file.display(),
            payload
                .get("patch_id")
                .and_then(|value| value.as_str())
                .unwrap_or("")
        ),
    );
    eprintln!(
        "[commit] file IPC reposition patch queued: {}",
        patch_file.display()
    );
    Ok(FileIpcRepositionResult::Queued)
}

/// Send a reposition-only IPC signal to the plugin.
///
/// No content changes - just tells the plugin to move the boundary marker to the
/// end of the exchange component. Used by `commit()` to keep the boundary at
/// end-of-exchange without writing to the working tree.
pub fn try_ipc_reposition_boundary(file: &Path) -> bool {
    let canonical = match file.canonicalize() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
    cleanup_legacy_ipc_degraded(&project_root);
    match ipc_direct_disk_degraded(&project_root, file) {
        Ok(true) => {
            eprintln!(
                "[commit] IPC reposition skipped for {}: listener degraded for this session",
                file.display()
            );
            log_ipc_dewedge_direct_disk_skip(file, "reposition");
            return false;
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!(
                "[commit] IPC reposition degradation check failed (non-fatal): {}",
                e
            );
        }
    }
    let snapshot_doc = agent_doc_snapshot_io::load(file).ok().flatten();
    let working_doc = std::fs::read_to_string(file).ok();
    let boundary_id = snapshot_doc
        .as_deref()
        .and_then(|doc| find_boundary_id(doc, "exchange"))
        .or_else(|| {
            working_doc
                .as_deref()
                .and_then(|doc| find_boundary_id(doc, "exchange"))
        });
    let normalize_prefix_lines = match (snapshot_doc.as_deref(), working_doc.as_deref()) {
        (Some(committed), Some(working)) => {
            extract_post_commit_normalization_targets(committed, working)
        }
        _ => vec![],
    };

    if !agent_doc_ipc_io::is_listener_active(&project_root) {
        return match queue_file_ipc_reposition_boundary(
            file,
            boundary_id.as_deref(),
            &normalize_prefix_lines,
        ) {
            Ok(FileIpcRepositionResult::Queued) => true,
            Ok(FileIpcRepositionResult::DeferredExistingPatch) => true,
            Ok(FileIpcRepositionResult::Unavailable) => false,
            Err(e) => {
                eprintln!("[commit] file IPC reposition queue failed (non-fatal): {e}");
                false
            }
        };
    }

    let result = if normalize_prefix_lines.is_empty() {
        let mut message = serde_json::json!({
            "type": "reposition",
            "file": canonical.to_string_lossy(),
            "preserve_head": true,
        });
        if let Some(boundary_id) = boundary_id.as_deref() {
            message["boundary_id"] = serde_json::Value::String(boundary_id.to_string());
        }
        target_payload_to_live_editor(file, &mut message, "socket_reposition");
        agent_doc_ipc_io::send_message(&project_root, &message).map(|_| true)
    } else {
        let mut message = serde_json::json!({
            "type": "patch",
            "file": canonical.to_string_lossy(),
            "patches": [],
            "unmatched": "",
            "reposition_boundary": true,
            "preserve_head": true,
            "normalize_prefix_lines": normalize_prefix_lines.clone(),
        });
        if let Some(boundary_id) = boundary_id.as_deref() {
            message["reposition_boundary_id"] = serde_json::Value::String(boundary_id.to_string());
        }
        target_payload_to_live_editor(file, &mut message, "socket_reposition_patch");
        agent_doc_ipc_io::send_message(&project_root, &message).map(|_| true)
    };

    match result {
        Ok(true) => {
            if let Err(e) =
                clear_ipc_socket_ack_timeouts(&project_root, file, "reposition_socket_ack")
            {
                eprintln!(
                    "[commit] IPC reposition timeout clear failed (non-fatal): {}",
                    e
                );
            }
            if normalize_prefix_lines.is_empty() {
                eprintln!("[commit] IPC reposition boundary signal sent");
            } else {
                eprintln!(
                    "[commit] IPC prefix repair + boundary signal sent ({} lines)",
                    normalize_prefix_lines.len()
                );
            }
            true
        }
        Ok(false) => {
            eprintln!("[commit] IPC reposition: no ack (non-fatal)");
            match queue_file_ipc_reposition_boundary(
                file,
                boundary_id.as_deref(),
                &normalize_prefix_lines,
            ) {
                Ok(FileIpcRepositionResult::Queued) => true,
                Ok(FileIpcRepositionResult::DeferredExistingPatch) => true,
                Ok(FileIpcRepositionResult::Unavailable) => false,
                Err(e) => {
                    eprintln!("[commit] file IPC reposition queue failed (non-fatal): {e}");
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("[commit] IPC reposition failed (non-fatal): {}", e);
            if is_socket_ack_timeout_error(e.to_string()) {
                match record_ipc_socket_ack_timeout(&project_root, file, None, "reposition") {
                    Ok(true) => {
                        eprintln!(
                            "[commit] IPC listener degraded for {} after repeated reposition ack timeouts",
                            file.display()
                        );
                        log_ipc_dewedge_direct_disk_skip(file, "reposition_timeout");
                        agent_doc_flow_io::closeout::cleanup_fallback_patch_files(file);
                        return false;
                    }
                    Ok(false) => {}
                    Err(record_err) => eprintln!(
                        "[commit] IPC reposition timeout record failed (non-fatal): {}",
                        record_err
                    ),
                }
            }
            match queue_file_ipc_reposition_boundary(
                file,
                boundary_id.as_deref(),
                &normalize_prefix_lines,
            ) {
                Ok(FileIpcRepositionResult::Queued) => true,
                Ok(FileIpcRepositionResult::DeferredExistingPatch) => true,
                Ok(FileIpcRepositionResult::Unavailable) => false,
                Err(e) => {
                    eprintln!("[commit] file IPC reposition queue failed (non-fatal): {e}");
                    false
                }
            }
        }
    }
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn patch_response_headings_already_in_head_true_when_no_patches() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, "doc body\n").unwrap();
        assert!(patch_response_headings_already_in_head(&doc, &[]));
    }

    #[test]
    fn patch_response_headings_already_in_head_true_when_heading_in_head() {
        let dir = TempDir::new().unwrap();
        let doc = agent_doc_test_support::init_repo_with_doc(
            dir.path(),
            "session.md",
            "## Exchange\n\n### Re: shipped - opus-4-7\n\nbody\n",
        );
        let patch = agent_doc_test_support::patch_with_heading("### Re: shipped - opus-4-7");
        assert!(patch_response_headings_already_in_head(&doc, &[patch]));
    }

    #[test]
    fn patch_response_headings_already_in_head_false_when_heading_missing_from_head() {
        let dir = TempDir::new().unwrap();
        let doc = agent_doc_test_support::init_repo_with_doc(
            dir.path(),
            "session.md",
            "## Exchange\n\n### Re: prior cycle - opus-4-7\n\nold\n",
        );
        let patch = agent_doc_test_support::patch_with_heading("### Re: new response - opus-4-7");
        assert!(
            !patch_response_headings_already_in_head(&doc, &[patch]),
            "mid-turn rotation must allow the patch when response is not in HEAD"
        );
    }

    #[test]
    fn patch_response_headings_already_in_head_false_when_any_heading_missing() {
        let dir = TempDir::new().unwrap();
        let doc = agent_doc_test_support::init_repo_with_doc(
            dir.path(),
            "session.md",
            "## Exchange\n\n### Re: first - opus-4-7\n\nbody\n",
        );
        let patches = vec![
            agent_doc_test_support::patch_with_heading("### Re: first - opus-4-7"),
            agent_doc_test_support::patch_with_heading("### Re: second - opus-4-7"),
        ];
        assert!(
            !patch_response_headings_already_in_head(&doc, &patches),
            "all headings must be in HEAD for the gate to skip"
        );
    }

    #[test]
    fn patch_response_headings_already_in_head_false_when_file_not_in_git() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, "no git\n").unwrap();
        let patch = agent_doc_test_support::patch_with_heading("### Re: something - opus-4-7");
        assert!(!patch_response_headings_already_in_head(&doc, &[patch]));
    }

    #[test]
    fn queued_file_reposition_patch_carries_generation_token() {
        // #late-ipc-patch-duplicate-stall: the durable file reposition patch must
        // carry the cycle id + a baseline content hash so a late applier can
        // fence a superseded patch.
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();
        let doc = root.join("plan.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior - opus\nDone.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, content).unwrap();
        let cs =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let result = queue_file_ipc_reposition_boundary(&doc, Some("abc123"), &[]).unwrap();
        assert!(matches!(result, FileIpcRepositionResult::Queued));

        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let patch_file = root.join(".agent-doc/patches").join(format!("{hash}.json"));
        let payload: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&patch_file).unwrap()).unwrap();

        assert_eq!(
            payload["cycle_id"].as_str(),
            Some(cs.cycle_id.as_str()),
            "queued reposition patch must tag the originating cycle id"
        );
        assert_eq!(
            payload["baseline_hash"].as_str(),
            Some(agent_doc_hash::content_hash(content).as_str()),
            "queued reposition patch must tag the baseline content hash it targets"
        );
        assert_eq!(payload["patches"], serde_json::json!([]));
        assert_eq!(payload["unmatched"], serde_json::json!(""));
        assert_eq!(payload["reposition_boundary"], serde_json::json!(true));
        assert_eq!(payload["preserve_head"], serde_json::json!(true));
        assert_eq!(
            payload["reposition_boundary_id"],
            serde_json::json!("abc123")
        );
    }
}
