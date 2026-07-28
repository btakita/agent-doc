//! High-level write IPC transport adapters.

mod transport;

pub use transport::try_ipc_with_effects;

use agent_doc_element_boundary::boundary::find_boundary_id;
use agent_doc_element_exchange::{
    extract_post_commit_normalization_targets, normalize_exchange_prefixes_for_targets,
};
use agent_doc_ipc_io::editor_target::target_payload_to_editor;
use agent_doc_ipc_protocol::{EditorIntent, is_socket_receipt_timeout_error};
use agent_doc_run_context_io::AgentDocContextExt;
use agent_doc_template::stale_baseline::is_append_mode_component;
use agent_doc_write_converge_io::{clear_ipc_socket_ack_timeouts, record_ipc_socket_ack_timeout};
use anyhow::Result;
use std::path::Path;

pub(crate) fn current_document_content(file: &Path, source: &str) -> Result<String> {
    agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)
}

pub(crate) fn ipc_document_content(file: &Path, source: &str) -> Result<String> {
    current_document_content(file, source)
}

pub(crate) fn projected_cycle_id(file: &Path) -> Option<String> {
    agent_doc_cycle_state_io::load_closeout_projection(file)
        .ok()
        .flatten()
        .and_then(|projection| projection.cycle_id)
        .or_else(|| {
            agent_doc_cycle_state_io::load_with_closeout_projection(file)
                .ok()
                .flatten()
                .map(|state| state.cycle_id)
        })
}

/// Result of an IPC write attempt, including the patch_id used.
///
/// The `patch_id` is returned so callers can report/retry the same logical
/// response. The plugin tracks applied patch_ids and skips duplicates,
/// preventing double-apply when a CP delivery is retried.
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

/// Truthful outcome of a post-commit editor-boundary delivery attempt.
///
/// `RetainedForRetry` is intentionally distinct from `Delivered`: refining an
/// already-retained reconnect target preserves the future delivery obligation;
/// it does not prove that any live editor has consumed the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryRepositionAttempt {
    Delivered,
    RetainedForRetry,
    Unavailable,
}

/// Return `true` when every non-empty incoming response patch body is already
/// present in the document's `HEAD` content.
///
/// This distinguishes "cycle already committed because this response landed"
/// from "cycle committed by an unrelated mid-turn operation; rotate and apply".
/// A heading alone is not response identity: headings are intentionally reused
/// across retries and queue IDs.
pub fn patch_response_bodies_already_in_head(
    file: &Path,
    patches: &[agent_doc_template::PatchBlock],
) -> bool {
    let response_bodies = patches
        .iter()
        .map(|patch| patch.content.trim())
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>();
    if response_bodies.is_empty() {
        return true;
    }
    let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());
    let Some(head) = rc.head_content() else {
        return false;
    };
    response_bodies.iter().all(|body| head.contains(body))
}

/// Build the typed editor-intent patch array.
///
/// Reads the document to find boundary IDs, filters frontmatter patches, and
/// synthesizes exchange/output patches for unmatched response content.
pub fn build_ipc_patches_json(
    file: &Path,
    patches: &[agent_doc_template::PatchBlock],
    unmatched: &str,
    normalize_prefix_lines: Option<&[String]>,
    boundary_seed: Option<&str>,
) -> Result<Vec<serde_json::Value>> {
    let raw_doc = ipc_document_content(file, "write_ipc_build_patches_current")?;
    let summary = file.file_stem().and_then(|s| s.to_str());
    // #finalize-visible-buffer-ipc-timeout-race: when a stable seed (the IPC
    // patch_id) is supplied, derive a deterministic boundary so CP delivery
    // retries carry the same boundary and the plugin does not append
    // the same response twice under different node IDs.
    let current_doc = match boundary_seed {
        Some(seed) => {
            let bid = agent_doc_element::id::boundary_id_from_seed_with_summary(seed, summary);
            agent_doc_template::reposition_boundary_to_end_clean_with_summary_and_id(
                &raw_doc,
                Some(&bid),
                summary,
            )
        }
        None => {
            agent_doc_template::reposition_boundary_to_end_clean_with_summary(&raw_doc, summary)
        }
    };

    let mut ipc_patches: Vec<serde_json::Value> = patches
        .iter()
        .filter(|p| p.name != "frontmatter")
        .map(|p| {
            let content = match normalize_prefix_lines {
                Some(prefix_lines)
                    if !prefix_lines.is_empty() && is_append_mode_component(&p.name) =>
                {
                    agent_doc_document_realtime::write_policy::normalize_patch_content(
                        &p.content,
                        prefix_lines,
                    )
                }
                _ => p.content.clone(),
            };
            let mut patch_json = serde_json::json!({
                "component": p.name,
                "content": content,
                "op": if is_append_mode_component(&p.name) {
                    "append"
                } else {
                    "replace"
                },
            });
            if let Some(bid) = find_boundary_id(&current_doc, &p.name) {
                patch_json["boundary_id"] = serde_json::Value::String(bid.clone());
                patch_json["node_id"] = serde_json::Value::String(bid);
            } else if is_append_mode_component(&p.name) {
                patch_json["ensure_boundary"] = serde_json::Value::Bool(true);
            }
            patch_json
        })
        .collect();

    let effective_unmatched = unmatched.trim().to_string();
    if ipc_patches.is_empty() && !effective_unmatched.is_empty() {
        let parsed_comps = agent_doc_element::element::parse(&current_doc).unwrap_or_default();
        for target in &["exchange", "output"] {
            let already_present = parsed_comps.iter().any(|c| {
                c.name == *target && {
                    let body = &current_doc[c.open_end..c.close_start];
                    body.contains(effective_unmatched.as_str())
                }
            });
            if already_present {
                eprintln!(
                    "[write] dedup: content already present in {} - skipping synthesis",
                    target
                );
                break;
            }
            if let Some(bid) = find_boundary_id(&current_doc, target) {
                let synthesized_content = match normalize_prefix_lines {
                    Some(prefix_lines)
                        if !prefix_lines.is_empty() && is_append_mode_component(target) =>
                    {
                        agent_doc_document_realtime::write_policy::normalize_patch_content(
                            &effective_unmatched,
                            prefix_lines,
                        )
                    }
                    _ => effective_unmatched.clone(),
                };
                eprintln!(
                    "[write] synthesizing {} patch for unmatched content (boundary {})",
                    target,
                    &bid[..8.min(bid.len())]
                );
                ipc_patches.push(serde_json::json!({
                    "component": target,
                    "content": synthesized_content,
                    "op": "append",
                    "boundary_id": bid,
                    "node_id": bid,
                }));
                break;
            } else if is_append_mode_component(target) {
                let synthesized_content = match normalize_prefix_lines {
                    Some(prefix_lines) if !prefix_lines.is_empty() => {
                        agent_doc_document_realtime::write_policy::normalize_patch_content(
                            &effective_unmatched,
                            prefix_lines,
                        )
                    }
                    _ => effective_unmatched.clone(),
                };
                eprintln!(
                    "[write] synthesizing {} patch for unmatched content (ensure_boundary)",
                    target
                );
                ipc_patches.push(serde_json::json!({
                    "component": target,
                    "content": synthesized_content,
                    "op": "append",
                    "ensure_boundary": true,
                }));
                break;
            }
        }
    }

    Ok(ipc_patches)
}

/// Send a reposition-only IPC signal to the plugin.
///
/// Used by `commit()` to keep the boundary at end-of-exchange. A successful
/// CRDT path must also materialize the acknowledged canonical target to disk;
/// otherwise Git/editor can commit the new singleton boundary while the
/// working tree retains the superseded placement.
pub fn try_ipc_reposition_boundary(file: &Path) -> BoundaryRepositionAttempt {
    let canonical = match file.canonicalize() {
        Ok(c) => c,
        Err(_) => return BoundaryRepositionAttempt::Unavailable,
    };
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
    let snapshot_doc = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten();
    let working_doc = ipc_document_content(file, "write_ipc_reposition_working_doc").ok();
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

    if let Some(working) = working_doc.as_deref()
        && let Some(target) =
            post_commit_reposition_target(working, boundary_id.as_deref(), &normalize_prefix_lines)
    {
        if let Some(pending) = agent_doc_document_realtime_io::pending_document_write(file)
            .or_else(|| agent_doc_document_realtime_io::pending_external_disk_candidate(file))
        {
            // A deferred reconnect target is canonical state, not an editor-only
            // transient projection. Retaining `(HEAD)` here would reintroduce a
            // stale working-tree diff when the JetBrains replica reconnects.
            let target = agent_doc_document::transient_markers::strip_head_markers(&target);
            let force_disk_projection = pending.source.is_force_disk();
            let (transport, result) = if force_disk_projection {
                (
                    "force_disk_projection",
                    agent_doc_document_realtime_io::atomic_write_force_disk_through_authority(
                        file, &target,
                    )
                    .map(|_| pending.intent_id.clone()),
                )
            } else {
                (
                    "lazily_deferred",
                    agent_doc_document_realtime_io::refine_deferred_document_write_target(
                        file,
                        working,
                        &target,
                        "post_commit_reposition",
                            agent_doc_document_realtime_io::DocumentWriteDeferredReason::ExtendPendingEditorReconnectTarget,
                    ),
                )
            };
            let delivery = if force_disk_projection && result.is_ok() {
                BoundaryRepositionAttempt::Delivered
            } else {
                BoundaryRepositionAttempt::RetainedForRetry
            };
            match result {
                Ok(intent_id) => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "post_commit_reposition_deferred file={} transport={} prior_intent_id={} resulting_intent_id={} target_hash={} recovery=editor_reconnect",
                            file.display(),
                            transport,
                            pending.intent_id,
                            intent_id,
                            agent_doc_hash::content_hash(&target),
                        ),
                    );
                }
                Err(err) => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "post_commit_reposition_deferred_failed file={} transport={} retained_intent_id={} error={} recovery=retain_existing_target_no_relay_spin",
                            file.display(),
                            transport,
                            pending.intent_id,
                            err,
                        ),
                    );
                }
            }
            return delivery;
        }
        match agent_doc_document_realtime_io::atomic_write_if_current_through_authority(
            file,
            &target,
            working,
            "post_commit_reposition",
        ) {
            Ok(()) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "post_commit_reposition_materialized file={} content_hash={} authority_and_disk_converged=true",
                        file.display(),
                        agent_doc_hash::content_hash(&target),
                    ),
                );
                eprintln!("[commit] CRDT boundary refresh materialized");
                return BoundaryRepositionAttempt::Delivered;
            }
            Err(err) => {
                eprintln!(
                    "[commit] authority/disk boundary refresh failed (falling back to IPC): {err}"
                );
            }
        }
    }

    let Some(registration) =
        agent_doc_controller_io::project_controller::live_editor_registration_for_file(file)
            .ok()
            .flatten()
    else {
        return BoundaryRepositionAttempt::Unavailable;
    };
    if !agent_doc_ipc_io::is_listener_active_for_pid(&project_root, registration.pid) {
        return BoundaryRepositionAttempt::Unavailable;
    }

    let result = if normalize_prefix_lines.is_empty() {
        let mut message = serde_json::json!({
            "type": EditorIntent::Reposition.as_str(),
            "file": canonical.to_string_lossy(),
            "preserve_head": true,
        });
        if let Some(boundary_id) = boundary_id.as_deref() {
            message["boundary_id"] = serde_json::Value::String(boundary_id.to_string());
        }
        target_payload_to_editor(
            file,
            &mut message,
            "socket_reposition",
            &registration.editor_id,
            registration.pid,
        );
        agent_doc_ipc_io::send_message_to_pid(&project_root, registration.pid, &message)
            .map(|_| true)
    } else {
        let mut message = serde_json::json!({
            "type": agent_doc_ipc_protocol::EditorIntent::ApplyCanonical.as_str(),
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
        target_payload_to_editor(
            file,
            &mut message,
            "socket_reposition_patch",
            &registration.editor_id,
            registration.pid,
        );
        agent_doc_ipc_io::send_message_to_pid(&project_root, registration.pid, &message)
            .map(|_| true)
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
            BoundaryRepositionAttempt::Delivered
        }
        Ok(false) => {
            eprintln!("[commit] IPC reposition: no receipt (non-fatal)");
            BoundaryRepositionAttempt::Unavailable
        }
        Err(e) => {
            eprintln!("[commit] IPC reposition failed (non-fatal): {}", e);
            if is_socket_receipt_timeout_error(e.to_string()) {
                match record_ipc_socket_ack_timeout(&project_root, file, None, "reposition") {
                    Ok(true) => {
                        eprintln!(
                            "[commit] IPC listener degraded for {} after repeated reposition receipt timeouts",
                            file.display()
                        );
                        return BoundaryRepositionAttempt::Unavailable;
                    }
                    Ok(false) => {}
                    Err(record_err) => eprintln!(
                        "[commit] IPC reposition timeout record failed (non-fatal): {}",
                        record_err
                    ),
                }
            }
            BoundaryRepositionAttempt::Unavailable
        }
    }
}

fn post_commit_reposition_target(
    working_doc: &str,
    boundary_id: Option<&str>,
    normalize_prefix_lines: &[String],
) -> Option<String> {
    let prefix_repaired = if normalize_prefix_lines.is_empty() {
        working_doc.to_string()
    } else {
        normalize_exchange_prefixes_for_targets(working_doc, normalize_prefix_lines)
    };
    let repositioned = agent_doc_template::reposition_boundary_to_end_preserve_head_with_id(
        &prefix_repaired,
        boundary_id,
    );
    (repositioned != working_doc).then_some(repositioned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn build_ipc_patches_json_seeded_boundary_is_stable_across_rebuilds() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("agent-doc-bugs2.md");
        let doc_content =
            "<!-- agent:exchange patch=append -->\nPrior response.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, doc_content).unwrap();

        let patches = vec![agent_doc_template::PatchBlock::new(
            "exchange",
            "### Re: fix\n\nNew response body.",
        )];
        let seed = "2ffa57c0-24e8-441c-aca9-46e6aa6f1c2a";

        let build_a = build_ipc_patches_json(&doc, &patches, "", None, Some(seed)).unwrap();
        let build_b = build_ipc_patches_json(&doc, &patches, "", None, Some(seed)).unwrap();
        let bid_a = build_a[0]["boundary_id"].as_str();
        let bid_b = build_b[0]["boundary_id"].as_str();
        assert!(
            bid_a.is_some(),
            "patch should carry a boundary_id: {build_a:?}"
        );
        assert_eq!(build_a[0]["op"].as_str(), Some("append"));
        assert_eq!(build_a[0]["node_id"].as_str(), bid_a);
        assert_eq!(
            bid_a, bid_b,
            "same patch_id seed must yield the same boundary across rebuilds"
        );
        assert_eq!(bid_a, Some("2ffa57c0:agent-doc-bugs2"));

        let other_seed = "99887766-1111-2222-3333-444455556666";
        let build_c = build_ipc_patches_json(&doc, &patches, "", None, Some(other_seed)).unwrap();
        assert_ne!(build_c[0]["boundary_id"].as_str(), bid_a);
    }

    #[test]
    fn synthesis_dedup_skips_when_content_already_present() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let existing = "This is the agent response.";
        let doc_content = format!(
            "<!-- agent:exchange patch=append -->\n{}\n<!-- /agent:exchange -->\n",
            existing
        );
        fs::write(&doc, &doc_content).unwrap();

        let patches: Vec<agent_doc_template::PatchBlock> = vec![];
        let result = build_ipc_patches_json(&doc, &patches, existing, None, None).unwrap();

        assert!(
            result.is_empty(),
            "synthesis should be skipped when content already exists in target component, got {result:?}"
        );
    }

    #[test]
    fn synthesis_proceeds_when_content_is_new() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let doc_content =
            "<!-- agent:exchange patch=append -->\nExisting content.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, doc_content).unwrap();

        let patches: Vec<agent_doc_template::PatchBlock> = vec![];
        let new_content = "Completely new agent response.";
        let result = build_ipc_patches_json(&doc, &patches, new_content, None, None).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["component"].as_str().unwrap(), "exchange");
        assert_eq!(result[0]["content"].as_str().unwrap(), new_content);
    }

    #[test]
    fn build_ipc_patches_json_preserves_leading_code_fence_content() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let prompt_prefix = "\u{276f}";
        let doc_content = format!(
            "<!-- agent:exchange patch=append -->\n{prompt_prefix} show fenced prompt\n```\nprompt body\n```\n<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, doc_content).unwrap();

        let patches = vec![agent_doc_template::PatchBlock::new(
            "exchange",
            "```\nresponse body\n```\n",
        )];
        let result = build_ipc_patches_json(&doc, &patches, "", None, None).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["component"].as_str().unwrap(), "exchange");
        assert_eq!(result[0]["op"].as_str().unwrap(), "append");
        assert_eq!(
            result[0]["content"].as_str().unwrap(),
            "```\nresponse body\n```\n"
        );
    }

    #[test]
    fn synthesis_normalizes_prefix_lines_for_unmatched_exchange_content() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let doc_content =
            "<!-- agent:exchange patch=append -->\nPrevious response.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, doc_content).unwrap();

        let patches: Vec<agent_doc_template::PatchBlock> = vec![];
        let unmatched = "do #expatch. spec-test-build-install-commit-push\n### Re: #expatch - gpt-5\n\nImplemented.\n";
        let prefix_lines = vec!["do #expatch. spec-test-build-install-commit-push".to_string()];
        let result = build_ipc_patches_json(
            &doc,
            &patches,
            unmatched,
            Some(prefix_lines.as_slice()),
            None,
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["component"].as_str().unwrap(), "exchange");
        assert_eq!(result[0]["op"].as_str().unwrap(), "append");
        let content = result[0]["content"].as_str().unwrap();
        assert!(content.contains("\u{276f} do #expatch. spec-test-build-install-commit-push"));
        assert!(content.contains("### Re: #expatch - gpt-5"));
    }

    #[test]
    fn patch_response_bodies_already_in_head_true_when_no_patches() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, "doc body\n").unwrap();
        assert!(patch_response_bodies_already_in_head(&doc, &[]));
    }

    #[test]
    fn patch_response_bodies_already_in_head_true_when_heading_in_head() {
        let dir = TempDir::new().unwrap();
        let doc = agent_doc_test_support::init_repo_with_doc(
            dir.path(),
            "session.md",
            "## Exchange\n\n### Re: shipped - opus-4-7\n\nbody line one\n",
        );
        let patch = agent_doc_test_support::patch_with_heading("### Re: shipped - opus-4-7");
        assert!(patch_response_bodies_already_in_head(&doc, &[patch]));
    }

    #[test]
    fn patch_response_heading_match_is_not_response_identity() {
        let dir = TempDir::new().unwrap();
        let doc = agent_doc_test_support::init_repo_with_doc(
            dir.path(),
            "session.md",
            "## Exchange\n\n### Re: repeated - opus-4-7\n\nold response body\n",
        );
        let patch = agent_doc_template::PatchBlock::new(
            "exchange",
            "### Re: repeated - opus-4-7\n\nnew retained response body\n",
        );
        assert!(
            !patch_response_bodies_already_in_head(&doc, &[patch]),
            "a committed cycle must rotate when only the heading, not the full response, is in HEAD"
        );
    }

    #[test]
    fn patch_response_bodies_already_in_head_false_when_heading_missing_from_head() {
        let dir = TempDir::new().unwrap();
        let doc = agent_doc_test_support::init_repo_with_doc(
            dir.path(),
            "session.md",
            "## Exchange\n\n### Re: prior cycle - opus-4-7\n\nold\n",
        );
        let patch = agent_doc_test_support::patch_with_heading("### Re: new response - opus-4-7");
        assert!(
            !patch_response_bodies_already_in_head(&doc, &[patch]),
            "mid-turn rotation must allow the patch when response is not in HEAD"
        );
    }

    #[test]
    fn patch_response_bodies_already_in_head_false_when_any_heading_missing() {
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
            !patch_response_bodies_already_in_head(&doc, &patches),
            "all headings must be in HEAD for the gate to skip"
        );
    }

    #[test]
    fn patch_response_bodies_already_in_head_false_when_file_not_in_git() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, "no git\n").unwrap();
        let patch = agent_doc_test_support::patch_with_heading("### Re: something - opus-4-7");
        assert!(!patch_response_bodies_already_in_head(&doc, &[patch]));
    }
    #[test]
    fn post_commit_reposition_target_adds_committed_boundary_without_socket_wait() {
        let working = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "Compacted summary.\n",
            "<!-- /agent:exchange -->\n",
        );

        let target = post_commit_reposition_target(working, Some("committed-id"), &[])
            .expect("compacted exchange without a boundary needs a refresh target");

        assert!(target.contains("<!-- agent:boundary:committed-id -->"));
        assert!(target.contains("Compacted summary."));
        assert_ne!(target, working);
    }

    #[test]
    fn post_commit_reposition_target_returns_none_when_current() {
        let working = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "Compacted summary.\n",
            "<!-- agent:boundary:committed-id -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert_eq!(
            post_commit_reposition_target(working, Some("committed-id"), &[]),
            None
        );
    }

    #[test]
    fn post_commit_reposition_target_repairs_prefixes_before_boundary_refresh() {
        let working = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "do #spfxnorm. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        let targets = vec!["do #spfxnorm. spec-test-build-install-commit-push".to_string()];

        let target = post_commit_reposition_target(working, Some("committed-id"), &targets)
            .expect("prefix repair should produce a refresh target");

        assert!(target.contains("\u{276f} do #spfxnorm. spec-test-build-install-commit-push"));
        assert!(!target.contains("\n<!-- agent:exchange -->\ndo #spfxnorm"));
        assert!(target.contains("<!-- agent:boundary:committed-id -->"));
    }

    #[test]
    fn post_commit_reposition_materializes_acknowledged_crdt_target_to_disk() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let working = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- agent:boundary:committed-id -->\n",
            "### Re: complete - gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, working).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            working,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_test_support::publish_editor_text_via_crdt_relay(
            &doc,
            "intellij:post-commit-reposition",
            working,
        );

        let target = post_commit_reposition_target(working, Some("committed-id"), &[])
            .expect("boundary before the response must move to the exchange tail");
        assert_eq!(
            try_ipc_reposition_boundary(&doc),
            BoundaryRepositionAttempt::Delivered,
        );

        assert_eq!(fs::read_to_string(&doc).unwrap(), target);
        assert_eq!(
            agent_doc_document_realtime_io::try_resolve_current_document_content(
                &doc,
                "post_commit_reposition_materialization_test",
            )
            .unwrap(),
            target,
        );
    }
}
