//! Test-only write convergence coverage relocated from orchestration.

use super::*;
use agent_doc_document_realtime::write_policy::{
    live_prompt_drift_auto_recovery_safe, live_prompt_drift_recovery_target,
    normalize_visible_recovery_compare, snapshot_contains_dropped_prompt,
};

struct TestWriteConvergenceEffects;

static TEST_WRITE_CONVERGENCE_EFFECTS: TestWriteConvergenceEffects = TestWriteConvergenceEffects;

impl EditorConvergenceEffects for TestWriteConvergenceEffects {
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        super::atomic_write(file, content)
    }

    fn guard_visible_write_idle_and_current(
        &self,
        file: &Path,
        source: &str,
        expected_current: &str,
    ) -> Result<()> {
        let current = std::fs::read_to_string(file).unwrap_or_default();
        if current != expected_current {
            anyhow::bail!("refusing {source} write: document changed before write");
        }
        Ok(())
    }

    fn atomic_write_if_current(
        &self,
        file: &Path,
        content: &str,
        expected_current: &str,
        source: &str,
    ) -> Result<()> {
        self.guard_visible_write_idle_and_current(file, source, expected_current)?;
        super::atomic_write(file, content)
    }

    fn cycle_already_committed(&self, _file: &Path) -> Option<String> {
        None
    }

    fn log_file_ipc_already_committed(&self, _file: &Path, _cycle_id: &str) {}

    fn cleanup_fallback_patch_files(&self, _file: &Path) {}

    fn file_ipc_patch_rejected(&self, _file: &Path, _patch_id: &str) -> Option<String> {
        None
    }

    fn log_file_ipc_proof_failure(
        &self,
        file: &Path,
        patch_id: Option<&str>,
        invariant: &str,
        recovery: &str,
        detail: &str,
    ) {
        super::log_ipc_proof_failure_with_recycle(
            file, "file_ipc", patch_id, invariant, recovery, detail,
        );
    }
}

#[cfg(test)]
pub(crate) fn try_auto_recover_live_prompt_drift(
    file: &Path,
    snapshot: &str,
    file_content: &str,
) -> Result<Option<String>> {
    super::try_auto_recover_live_prompt_drift(
        &TEST_WRITE_CONVERGENCE_EFFECTS,
        file,
        snapshot,
        file_content,
    )
}

#[cfg(test)]
pub(crate) fn try_editor_converge(
    file: &Path,
    target: &str,
    current_content: &str,
    source: &str,
) -> Result<bool> {
    super::try_editor_converge(
        &TEST_WRITE_CONVERGENCE_EFFECTS,
        file,
        target,
        current_content,
        source,
    )
}

#[cfg(test)]
pub(crate) fn converge_document_or_disk(
    file: &Path,
    target: &str,
    current: &str,
    source: &str,
) -> Result<()> {
    super::converge_document_or_disk(
        &TEST_WRITE_CONVERGENCE_EFFECTS,
        file,
        target,
        current,
        source,
    )
}

#[cfg(test)]
pub(crate) fn converge_or_disk_write(
    file: &Path,
    current: &str,
    target: &str,
    source: &str,
) -> Result<()> {
    super::converge_or_disk_write(
        &TEST_WRITE_CONVERGENCE_EFFECTS,
        file,
        current,
        target,
        source,
    )
}

#[cfg(test)]
pub(crate) fn live_buffer_delivery_missing_operator_text_authority_after_refresh(
    file: &Path,
    content: &str,
    source: &str,
) -> Option<agent_doc_debounce::LiveBufferSnapshot> {
    super::live_buffer_delivery_missing_operator_text_authority_after_refresh(file, content, source)
}

#[cfg(test)]
pub(crate) fn editor_convergence_payload(
    canonical_file: &Path,
    target: &str,
    current_content: &str,
    source: &str,
    patch_id: &str,
) -> Result<Option<serde_json::Value>> {
    super::editor_convergence_payload(canonical_file, target, current_content, source, patch_id)
}

#[cfg(test)]
fn live_prompt_drift_response_patches(
    file_content: &str,
    snapshot: &str,
) -> Result<Vec<serde_json::Value>> {
    super::live_prompt_drift_response_patches(file_content, snapshot)
}

#[cfg(test)]
mod core_tests {
    #![allow(unused_imports)]
    use super::*;
    use fs2::FileExt;
    use std::fs;
    use std::fs::OpenOptions;
    use std::time::Duration;
    use tempfile::TempDir;

    fn doc_with_queue_and_exchange(queue_body: &str, response: &str) -> String {
        format!(
            "---\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange -->\n{response}\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n{queue_body}\n<!-- /agent:queue -->\n"
        )
    }

    fn start_ack_mismatch_then_refresh_listener(
        project_root: &Path,
        visible_write_content: String,
    ) -> std::thread::JoinHandle<()> {
        let listener_root = project_root.to_path_buf();
        std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let msg_type = v.get("type").and_then(|value| value.as_str()).unwrap_or("");
                let patch_id = v
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                let content = if msg_type == "refresh_content" {
                    v.get("content")
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                        .to_string()
                } else {
                    visible_write_content.clone()
                };
                if let Some(file_path) = v.get("file").and_then(|value| value.as_str()) {
                    let _ = std::fs::write(file_path, &content);
                    let _ =
                        agent_doc_controller_io::project_controller::record_visible_write_commit_candidate_for_file(
                            Path::new(file_path),
                            patch_id,
                            &content,
                            "test_visible_write_listener",
                        );
                }
                Some(
                    serde_json::json!({"type": "receipt", "status": "applied", "id": patch_id})
                        .to_string(),
                )
            });
        })
    }

    #[test]
    fn live_prompt_drift_auto_recovery_safe_accepts_benign_wedge() {
        // Snapshot owns the response the fragmented disk file lost; no disk-only
        // user prompt → safe to auto-recover.
        let snapshot = agent_doc_test_support::drift_content_ours();
        let fragmented = agent_doc_test_support::drift_baseline();
        assert!(
            live_prompt_drift_auto_recovery_safe(
                &snapshot,
                &fragmented,
                normalize_visible_recovery_compare,
            ),
            "benign live-prompt-drift wedge should be recoverable"
        );
    }
    #[test]
    fn live_prompt_drift_response_patches_ignore_operator_owned_components() {
        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- existing backlog text\n<!-- /agent:backlog -->\n",
            agent_doc_test_support::drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- existing backlog text with operator word\n<!-- /agent:backlog -->\n",
            agent_doc_test_support::drift_baseline()
        );

        let generic = agent_doc_document::component_patches::component_replace_patches(
            &fragmented,
            &snapshot,
        )
        .unwrap();
        let generic_components: Vec<&str> = generic
            .iter()
            .filter_map(|patch| patch.get("component").and_then(|value| value.as_str()))
            .collect();
        assert!(
            generic_components.contains(&"exchange") && generic_components.contains(&"backlog"),
            "generic convergence should notice both component deltas: {generic:?}"
        );

        let response_only = live_prompt_drift_response_patches(&fragmented, &snapshot).unwrap();
        assert_eq!(
            response_only.len(),
            1,
            "live drift recovery only owns exchange"
        );
        assert_eq!(response_only[0]["component"], "exchange");
    }

    #[test]
    fn try_compact_editor_converge_writes_detached_disk_without_listener() {
        // Detached realtime: with no live editor listener and no live editor
        // sidecar, the current file is authoritative and the converger may use a
        // guarded direct disk write. This is not a snapshot fallback.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let current = agent_doc_test_support::drift_baseline();
        let compacted = agent_doc_test_support::drift_content_ours();
        std::fs::write(&doc, &current).unwrap();

        let converged = try_editor_converge(&doc, &compacted, &current, "compact").unwrap();
        assert!(converged, "detached compact should write the target");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            compacted,
            "no-listener compact convergence should write the compacted target"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("compact_writeback")
                && log.contains("transport=disk_detached")
                && log.contains("reason=no_listener"),
            "no-listener compact must record a detached disk writeback:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "no-listener compact must not advertise disk fallback:\n{log}"
        );
    }
    #[test]
    fn try_compact_editor_converge_converges_via_editor_ipc_with_listener() {
        // `#jbcompactcrdt`/`#w42v`: with a live JB IPC listener, compaction must
        // converge the compacted document through the editor (`transport=editor_ipc`)
        // instead of a direct disk write that diverges from the open buffer and
        // raises a `File Cache Conflict`.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("visible-write")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::compact_convergence_source();
        let compacted = agent_doc_test_support::compact_convergence_compacted();
        fs::write(&doc, &source).unwrap();

        // The fake editor acks with the compacted content, mirroring a JB plugin
        // that applied the exchange `op:replace` and converged its buffer.
        let _listener = agent_doc_test_support::start_live_prompt_drift_receipt_listener(
            dir.path(),
            compacted.clone(),
        );
        agent_doc_test_support::wait_for_live_prompt_drift_listener(dir.path());

        let converged = try_editor_converge(&doc, &compacted, &source, "compact").unwrap();
        assert!(
            converged,
            "an active JB IPC listener that converges the buffer must report editor_ipc transport"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("compact_editor_convergence_attempt"),
            "compact convergence attempt should be observable in ops.log:\n{log}"
        );
        assert!(
            log.contains("compact_writeback") && log.contains("transport=editor_ipc"),
            "successful compaction must record the editor_ipc writeback transport:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "a converged compaction must not also take the disk fallback:\n{log}"
        );
    }
    /// Pre-consume document with a `go` queue head an operator could be concurrently
    /// editing while the queue is struck.
    /// Post-consume document: only the `queue` head is struck; every other
    /// component is byte-identical (queue consume never touches the exchange).
    #[test]
    fn queue_consume_writeback_converges_via_editor_ipc_with_listener() {
        // `#fcc0`: the queue-consume write must route through the shared
        // converger so an active JB listener converges the struck queue through
        // the editor (`transport=editor_ipc`, `queue_consume`-labelled) instead of
        // a direct disk write that raises a `File Cache Conflict`.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("visible-write")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let _listener = agent_doc_test_support::start_live_prompt_drift_receipt_listener(
            dir.path(),
            target.clone(),
        );
        agent_doc_test_support::wait_for_live_prompt_drift_listener(dir.path());

        let converged = try_editor_converge(&doc, &target, &source, "queue_consume").unwrap();
        assert!(
            converged,
            "an active JB IPC listener that converges the buffer must report editor_ipc transport"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_editor_convergence_attempt"),
            "queue-consume convergence attempt should be source-labelled in ops.log:\n{log}"
        );
        assert!(
            log.contains("queue_consume_writeback") && log.contains("transport=editor_ipc"),
            "a converged queue consume must record the editor_ipc writeback transport:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "a converged queue consume must not also take the disk fallback:\n{log}"
        );
    }

    #[test]
    fn queue_consume_socket_status_error_falls_back_to_proven_file_ipc() {
        // A live editor socket can accept a patch, emit the early accepted
        // receipt, then reject the terminal apply (`status:rejected`) because
        // the editor is busy or the socket-side apply path lost its generation
        // race. That must not authorize a raw disk write, but it should try the
        // plugin-owned file-IPC queue in the same cycle and accept it only with
        // visible-write.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("visible-write")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            &source,
            "jetbrains-test-editor",
            "jetbrains",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let listener_root = dir.path().to_path_buf();
        let _listener = std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = v
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                Some(
                    serde_json::json!({
                        "type": "receipt",
                        "id": patch_id,
                        "status": "rejected",
                        "reason": "socket_apply_failed"
                    })
                    .to_string(),
                )
            });
        });
        agent_doc_test_support::wait_for_live_prompt_drift_listener(dir.path());

        let watcher_dir = agent_doc_dir.join("patches");
        let watcher_doc = doc.clone();
        let watcher_doc_str = doc_str.clone();
        let watcher_target = target.clone();
        let watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_none_or(|e| e != "json") {
                        continue;
                    }
                    let payload_text = fs::read_to_string(&path).unwrap();
                    let payload: serde_json::Value = serde_json::from_str(&payload_text).unwrap();
                    let patch_id = payload
                        .get("patch_id")
                        .and_then(|value| value.as_str())
                        .unwrap()
                        .to_string();
                    fs::write(&watcher_doc, &watcher_target).unwrap();
                    agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
                        &watcher_doc_str,
                        &watcher_target,
                        "jetbrains-test-editor",
                        "jetbrains",
                        "test",
                        &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
                    )
                    .unwrap();
                    agent_doc_controller_io::project_controller::record_visible_write_commit_candidate_for_file(
                        &watcher_doc,
                        &patch_id,
                        &watcher_target,
                        "test_file_ipc_watcher",
                    )
                    .unwrap();
                    fs::remove_file(path).unwrap();
                    return true;
                }
            }
            false
        });

        let converged = try_editor_converge(&doc, &target, &source, "queue_consume").unwrap();
        assert!(
            converged,
            "socket status:rejected should retry through proven file IPC before failing closed"
        );
        assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("send_failed")
                && log.contains("IPC receipt rejected"),
            "socket receipt rejection should remain auditable:\n{log}"
        );
        assert!(
            log.contains("queue_consume_file_ipc_convergence_attempt")
                && log.contains("degraded_cause=socket_status_error")
                && log.contains("transport=file_ipc"),
            "socket receipt rejection should fall back to proven file IPC:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "socket receipt-rejection fallback must not raw-write behind the plugin:\n{log}"
        );
        assert_eq!(fs::read_to_string(&doc).unwrap(), target);
    }

    #[test]
    fn queue_consume_ack_mismatch_refreshes_editor_back_to_preconsume() {
        // `#fcc0-ack-mismatch`: when the editor acks with content that does not
        // match the target, the disk write must still fail closed. The previous
        // behavior left that untrusted visible-write content in the live editor buffer, so a
        // later flush could persist a stale queue strike. Refresh it back to the
        // pre-consume document using the visible-write content as the stale hash guard.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("visible-write")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        let stale_ack = target.replace(
            "<!-- /agent:exchange -->",
            "> **Queue prompt:** stale leftover from failed queue consume\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, &source).unwrap();

        let root = dir.path().to_path_buf();
        let _listener = start_ack_mismatch_then_refresh_listener(&root, stale_ack);
        agent_doc_test_support::wait_for_live_prompt_drift_listener(&root);
        agent_doc_test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("ack_mismatch"),
            "ACK mismatch should still fail closed: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "untrusted visible-write content should be refreshed back to the pre-consume editor buffer"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=blocked")
                && log.contains("ack_mismatch"),
            "ACK mismatch must remain a blocked writeback:\n{log}"
        );
        assert!(
            log.contains("queue_consume_ack_mismatch_editor_refresh")
                && log.contains("action=revert_untrusted_visible_write"),
            "ACK mismatch should refresh the editor back to the pre-consume buffer:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "ACK mismatch must not take the disk fallback:\n{log}"
        );
    }

    #[test]
    fn queue_consume_ack_accepts_node_patch_with_editor_owned_queue_addition() {
        // `#qpcwcmerge`: queue consume owns the exact node-keyed strike, not the
        // whole live queue component. If the editor ACK proves that strike landed
        // while also carrying a concurrent operator queue addition, accept the
        // ACK-visible buffer instead of replaying or rejecting it.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("visible-write")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        let recovered = target.replace(
            "<!-- /agent:queue -->",
            "- do [#qftlossdelta]\n<!-- /agent:queue -->",
        );
        fs::write(&doc, &source).unwrap();

        let root = dir.path().to_path_buf();
        let _listener = start_ack_mismatch_then_refresh_listener(&root, recovered.clone());
        agent_doc_test_support::wait_for_live_prompt_drift_listener(&root);
        agent_doc_test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .expect("queue consume should accept proven node patch plus editor-owned queue edits");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            recovered,
            "the ACK-visible live buffer should remain authoritative"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=editor_ipc")
                && log.contains("resolution=editor_wins_outside_touched_components"),
            "queue consume should accept the editor-owned queue addition:\n{log}"
        );
        assert!(
            !log.contains("ack_mismatch")
                && !log.contains("editor_convergence_required")
                && !log.contains("transport=disk_fallback"),
            "proven editor-owned queue drift must not be treated as a failed convergence:\n{log}"
        );
    }

    #[test]
    fn pending_write_shorter_ack_replays_missing_agent_response() {
        // `#ack-shorter-replay`: a plugin ACK that proves every non-exchange
        // component but is missing the newly materialized `### Re:` block is not
        // user drift. Refresh the editor to the target response and treat the
        // write as converged instead of leaving the cycle interrupted.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("visible-write")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = doc_with_queue_and_exchange("- do [#head]\n", "");
        let target = doc_with_queue_and_exchange(
            "- do [#head]\n",
            "### Re: do [#head]\n\nAnswered from the agent.\n",
        );
        let shorter_ack = source.clone();
        assert!(
            shorter_ack.len() < target.len(),
            "test setup should model the shorter recovered ack"
        );
        fs::write(&doc, &source).unwrap();

        let root = dir.path().to_path_buf();
        let _listener = start_ack_mismatch_then_refresh_listener(&root, shorter_ack);
        agent_doc_test_support::wait_for_live_prompt_drift_listener(&root);
        agent_doc_test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        converge_document_or_disk(&doc, &target, &source, "pending_write")
            .expect("safe shorter ack should replay the target response through the editor");

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            target,
            "safe shorter ack should leave the editor/disk at the target response"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("pending_write_ack_mismatch_editor_refresh")
                && log.contains("action=replay_missing_agent_response"),
            "shorter ack should refresh the editor to the target response:\n{log}"
        );
        assert!(
            log.contains("pending_write_writeback")
                && log.contains("transport=editor_ipc")
                && log.contains("recovery=ack_mismatch_replayed_target"),
            "shorter ack recovery should be recorded as successful editor convergence:\n{log}"
        );
        assert!(
            !log.contains("action=refuse_external_disk_write"),
            "safe shorter ack must not be recorded as a refused external disk write:\n{log}"
        );
    }

    #[test]
    fn queue_consume_ack_mismatch_does_not_refresh_user_prompt_drift() {
        // If the visible-write content carries a genuine concurrent editor prompt, the
        // binary must still refuse the disk write but must not refresh the editor
        // back to the pre-consume document, because that would drop user work.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("visible-write")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        let user_ack = target.replace(
            "<!-- /agent:exchange -->",
            "❯ do [#followup] preserve this concurrent prompt\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, &source).unwrap();

        let root = dir.path().to_path_buf();
        let _listener = start_ack_mismatch_then_refresh_listener(&root, user_ack.clone());
        agent_doc_test_support::wait_for_live_prompt_drift_listener(&root);
        agent_doc_test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("ack_mismatch"),
            "ACK mismatch should still fail closed: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            user_ack,
            "user prompt drift must remain editor-owned instead of being refreshed away"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_visible_write_mismatch_editor_refresh")
                && log.contains("untrusted_visible_write_contains_user_drift")
                && log.contains("action=leave_editor_owned_visible_write"),
            "user drift should block the refresh path:\n{log}"
        );
        assert!(
            !log.contains("action=revert_untrusted_visible_write"),
            "user drift must not be reverted:\n{log}"
        );
    }

    #[test]
    fn queue_consume_editor_convergence_payload_is_node_keyed_and_fenced() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");
        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let payload = editor_convergence_payload(
            &doc.canonicalize().unwrap(),
            &target,
            &source,
            "queue_consume",
            "patch-queue-consume",
        )
        .unwrap()
        .expect("queue consume should produce an editor convergence payload");

        assert_eq!(
            payload["baseline_hash"].as_str(),
            Some(agent_doc_hash::content_hash(&source).as_str()),
            "socket convergence payloads must carry the raw generation fence"
        );
        assert_eq!(
            payload["baseline_normalized_hash"].as_str(),
            Some(
                agent_doc_hash::content_hash(
                    &agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                        &source
                    )
                )
                .as_str()
            ),
            "socket convergence payloads must also carry the transient-marker-normalized fence"
        );
        assert!(
            payload["patches"]
                .as_array()
                .unwrap()
                .iter()
                .all(|patch| patch["component"] != "queue"),
            "queue consume must not send a broad legacy queue component replace: {payload:?}"
        );
        let node_patches = payload["node_patches"].as_array().unwrap();
        assert!(
            node_patches
                .iter()
                .any(|patch| { patch["component"] == "queue" && patch["op"] == "strike" }),
            "queue consume must be expressed as an exact node-keyed strike: {payload:?}"
        );
    }
    #[test]
    fn try_editor_converge_treats_active_listener_already_current_as_noop() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        fs::write(&doc, &source).unwrap();

        let _listener = agent_doc_test_support::start_receipt_without_content_listener(dir.path());
        agent_doc_test_support::wait_for_live_prompt_drift_listener(dir.path());

        let converged = try_editor_converge(&doc, &source, &source, "pending_write").unwrap();
        assert!(
            converged,
            "already-current active-listener converge should be a no-op success"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "already-current converge must not mutate the document"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("pending_write_writeback") && log.contains("transport=already_current"),
            "already-current converge should be observable without disk fallback:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback") && !log.contains("transport=blocked"),
            "already-current converge must not fall back or block:\n{log}"
        );
    }
    #[test]
    fn converge_document_or_disk_writes_detached_disk_without_listener() {
        // Detached realtime: with no live editor listener and no live editor
        // sidecar, the current file is authoritative and the shared converger
        // may use a guarded direct disk write.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .expect("detached queue consume should write the target");

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            target,
            "with no listener the converger should write the target to disk"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=disk_detached")
                && log.contains("reason=no_listener"),
            "a no-listener queue consume must record the source-labelled detached writeback:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "no-listener queue consume must not record disk fallback:\n{log}"
        );
    }

    #[test]
    fn converge_document_or_disk_blocks_diverged_under_capable_live_buffer_before_ipc() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &doc_str,
            &format!("{source}\noperator typed text\n"),
            Some("jetbrains-old"),
        )
        .unwrap();

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("operator_text_authority_v1"),
            "under-capable editor sidecar must block with the missing capability: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "under-capable editor sidecar must not let the converger mutate disk"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("reason=editor_capability_missing")
                && log.contains("capability=operator_text_authority_v1")
                && log.contains("editor_id=jetbrains-old")
                && !log.contains("queue_consume_editor_convergence_attempt"),
            "capability guard must fire before IPC attempt:\n{log}"
        );
    }

    #[test]
    fn converge_document_or_disk_blocks_matching_under_capable_live_buffer_before_ipc() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &doc_str,
            &source,
            Some("jetbrains-old"),
        )
        .unwrap();

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("operator_text_authority_v1"),
            "matching under-capable editor sidecar must block delivery too: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "matching under-capable editor sidecar must not let the converger mutate disk"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("reason=editor_capability_missing")
                && log.contains("capability=operator_text_authority_v1")
                && log.contains("editor_id=jetbrains-old")
                && !log.contains("queue_consume_editor_convergence_attempt"),
            "delivery capability guard must fire before IPC attempt:\n{log}"
        );
    }

    #[test]
    fn capability_guard_refreshes_live_buffer_sidecar_over_editor_ipc() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &doc_str,
            &source,
            Some("jetbrains-old"),
        )
        .unwrap();

        let captured = std::sync::Arc::new(std::sync::Mutex::new(None::<serde_json::Value>));
        let captured_clone = captured.clone();
        let listener_root = dir.path().to_path_buf();
        let doc_for_listener = doc_str.clone();
        let source_for_listener = source.clone();
        let server = std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                *captured_clone.lock().unwrap() = Some(v.clone());
                if v.get("type").and_then(|value| value.as_str()) == Some("publish_live_buffer") {
                    let published =
                        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
                            &doc_for_listener,
                            &source_for_listener,
                            "jetbrains-old",
                            "jetbrains",
                            "test",
                            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
                        );
                    published.ok()?;
                }
                Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
            });
        });
        std::thread::sleep(Duration::from_millis(120));

        let missing = live_buffer_delivery_missing_operator_text_authority_after_refresh(
            &doc,
            &source,
            "queue_consume",
        );
        assert!(
            missing.is_none(),
            "a capable editor refresh should clear the stale missing-authority sidecar"
        );
        let msg = captured
            .lock()
            .unwrap()
            .clone()
            .expect("listener saw publish_live_buffer");
        assert_eq!(msg["type"], "publish_live_buffer");
        assert_eq!(msg["file"], doc_str);

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_editor_authority_refresh")
                && log.contains("transport=editor_ipc")
                && log.contains("action=publish_live_buffer"),
            "authority refresh should be logged as read-only editor IPC:\n{log}"
        );

        let _ = std::fs::remove_file(agent_doc_ipc_io::socket_path(dir.path()));
        drop(server);
    }

    #[test]
    fn capability_guard_refreshes_live_buffer_sidecar_over_file_signal() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &doc_str,
            &source,
            Some("vscode-old"),
        )
        .unwrap();

        let captured = std::sync::Arc::new(std::sync::Mutex::new(None::<serde_json::Value>));
        let captured_clone = captured.clone();
        let signal_root = dir.path().to_path_buf();
        let doc_for_signal = doc_str.clone();
        let source_for_signal = source.clone();
        let signal_thread = std::thread::spawn(move || {
            let signal = signal_root
                .join(".agent-doc")
                .join("patches")
                .join("publish-live-buffer.signal");
            for _ in 0..100 {
                if let Ok(raw) = fs::read_to_string(&signal) {
                    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
                    *captured_clone.lock().unwrap() = Some(v.clone());
                    if v.get("type").and_then(|value| value.as_str()) == Some("publish_live_buffer")
                    {
                        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
                            &doc_for_signal,
                            &source_for_signal,
                            "vscode-old",
                            "vscode",
                            "test",
                            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
                        )
                        .unwrap();
                    }
                    let _ = fs::remove_file(&signal);
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("publish-live-buffer file signal was not written");
        });

        let missing = live_buffer_delivery_missing_operator_text_authority_after_refresh(
            &doc,
            &source,
            "queue_consume",
        );
        signal_thread.join().unwrap();
        assert!(
            missing.is_none(),
            "a capable file-signal refresh should clear the stale missing-authority sidecar"
        );
        let msg = captured
            .lock()
            .unwrap()
            .clone()
            .expect("file signal was captured");
        assert_eq!(msg["type"], "publish_live_buffer");
        assert_eq!(msg["file"], doc_str);
        assert!(
            msg.get("content").is_none() && msg.get("patches").is_none(),
            "publish-live-buffer signal must be read-only: {msg}"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_editor_authority_refresh")
                && log.contains("transport=file_signal")
                && log.contains("action=publish_live_buffer"),
            "authority refresh should be logged as read-only file signal IPC:\n{log}"
        );
    }

    #[test]
    fn converge_document_or_disk_ignores_projection_only_live_buffer_for_detached_disk() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            &format!("{source}\noperator typed text\n"),
            "jetbrains-new",
            "jetbrains",
            "0.2.197",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        assert_eq!(
            converge_document_or_disk(&doc, &target, &source, "queue_consume").unwrap(),
            (),
            "projection-only live-buffer sidecars must not block detached disk authority"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            target,
            "sidecar projection must not be treated as live editor authority"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            !log.contains("reason=editor_capability_missing"),
            "capable sidecar must not trip the capability guard:\n{log}"
        );
        assert!(
            log.contains("transport=disk_detached"),
            "projection-only sidecar should allow detached disk write:\n{log}"
        );
    }

    #[test]
    fn converge_document_or_disk_route_source_writes_detached_disk_without_listener() {
        // `#fccroute`: the three route/dispatch session-document write sites
        // (`route_session_id`, `route_dedup_scrub`, `route_queue_activation`) now
        // route their disk writes through `converge_document_or_disk` so a live JB
        // editor converges them instead of hitting the File Cache Conflict dialog.
        // With no listener or live editor sidecar, detached realtime writes the
        // current file through the guarded disk path. Cover each route label so a
        // future regression on any one of them is caught.
        for source_label in [
            "route_session_id",
            "route_dedup_scrub",
            "route_queue_activation",
        ] {
            let dir = TempDir::new().unwrap();
            let agent_doc_dir = dir.path().join(".agent-doc");
            fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
            let doc = dir.path().join("plan.md");

            let source = agent_doc_test_support::queue_consume_convergence_source();
            let target = agent_doc_test_support::queue_consume_convergence_target();
            fs::write(&doc, &source).unwrap();

            converge_document_or_disk(&doc, &target, &source, source_label)
                .unwrap_or_else(|err| panic!("{source_label}: detached write failed: {err}"));

            assert_eq!(
                fs::read_to_string(&doc).unwrap(),
                target,
                "{source_label}: with no listener the converger must write the target"
            );
            let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
            assert!(
                log.contains(&format!("{source_label}_writeback"))
                    && log.contains("transport=disk_detached")
                    && log.contains("reason=no_listener"),
                "{source_label}: no-listener route write must record a source-labelled detached writeback:\n{log}"
            );
            assert!(
                !log.contains("transport=disk_fallback"),
                "{source_label}: no-listener route write must not record disk fallback:\n{log}"
            );
        }
    }
    #[test]
    fn converge_document_or_disk_blocks_disk_fallback_with_active_listener_without_visible_write_receipt()
     {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let _listener = agent_doc_test_support::start_receipt_without_content_listener(dir.path());
        agent_doc_test_support::wait_for_live_prompt_drift_listener(dir.path());
        // `#6b5h`: a real editor is attached — seed a live plugin-owner lease so
        // the guard fails closed (protects the buffer) rather than treating the
        // ack-without-content listener as the editor-less CLI-only case.
        agent_doc_test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("refused direct disk write"),
            "active listener without visible-write receipt should block disk fallback: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "an unproven editor IPC apply must not be followed by an external disk write"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=blocked")
                && log.contains("reason=no_visible_write_receipt"),
            "active listener failure must be logged as a blocked disk write:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "active listener failure must not be logged as a disk fallback:\n{log}"
        );
    }
    #[test]
    fn converge_or_disk_write_writes_detached_disk_without_listener() {
        // Detached realtime: the converge-or-disk gate used by pending/review,
        // dedupe, preflight-maintenance, and pipeline-mirror write sites may
        // write disk directly only when no editor endpoint or live sidecar owns
        // the document.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        converge_or_disk_write(&doc, &source, &target, "pending_write")
            .expect("detached pending write should write the target");

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            target,
            "with no listener the converger must write the target to disk"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("pending_write_writeback")
                && log.contains("transport=disk_detached")
                && log.contains("reason=no_listener"),
            "a no-listener plain converge must record the source-labelled detached writeback:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "a no-listener plain converge must not record disk fallback:\n{log}"
        );
    }
    #[test]
    fn converge_or_disk_write_blocks_plain_disk_fallback_with_active_listener_without_visible_write_receipt()
     {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let _listener = agent_doc_test_support::start_receipt_without_content_listener(dir.path());
        agent_doc_test_support::wait_for_live_prompt_drift_listener(dir.path());
        // `#6b5h`: a real editor is attached — seed a live plugin-owner lease so
        // the guard fails closed on unproven delivery.
        agent_doc_test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        let err = converge_or_disk_write(&doc, &source, &target, "pending_write")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("refused direct disk write"),
            "active listener without visible-write receipt should block plain disk fallback: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "plain component maintenance must not write behind a running editor plugin"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("pending_write_writeback")
                && log.contains("transport=blocked")
                && log.contains("reason=no_visible_write_receipt"),
            "active listener failure must be logged as a blocked plain disk write:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "active listener failure must not be logged as a disk fallback:\n{log}"
        );
    }
    #[test]
    fn converge_document_or_disk_editorless_socket_blocks_without_ack_proof() {
        // `#6b5h` cutover: a pure-CLI session may see a connectable
        // controller-hosted socket with NO plugin editor behind it. An
        // ack-without-content listener still does not prove editor convergence, so
        // the realtime path fails closed instead of routing the write to disk.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let _listener = agent_doc_test_support::start_receipt_without_content_listener(dir.path());
        agent_doc_test_support::wait_for_live_prompt_drift_listener(dir.path());
        // No plugin-owner lease seeded → no live editor endpoint, but the
        // connectable socket still requires convergence proof.

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("editor convergence is unproven"),
            "editorless socket without visible-write proof should fail closed: {err}"
        );

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "unproven editor convergence must not be followed by a disk write"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=blocked")
                && log.contains("reason=no_visible_write_receipt")
                && log.contains("editor_endpoint=absent")
                && log.contains("action=editor_convergence_required"),
            "editorless socket must record a fail-closed convergence requirement:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "editorless socket must not route missing ACK proof to disk fallback:\n{log}"
        );
    }
    #[test]
    fn live_prompt_drift_auto_recovery_safe_rejects_no_wedge() {
        // Snapshot == file: no wedge, nothing to recover, must not fire.
        let snapshot = agent_doc_test_support::drift_content_ours();
        assert!(
            !live_prompt_drift_auto_recovery_safe(
                &snapshot,
                &snapshot,
                normalize_visible_recovery_compare,
            ),
            "no drift means no auto-recovery"
        );
    }
    #[test]
    fn live_prompt_drift_auto_recovery_safe_rejects_disk_only_exchange_prompt() {
        // The visible file carries a NEW user prompt the snapshot never saw —
        // adopting content_ours would silently drop it. Fail closed.
        let snapshot = agent_doc_test_support::drift_content_ours();
        let mut fragmented = agent_doc_test_support::drift_baseline();
        fragmented = fragmented.replace(
            "❯ do #fix\n<!-- /agent:exchange -->",
            "❯ do #fix\n❯ do #brand-new-user-prompt-typed-after-preflight\n<!-- /agent:exchange -->",
        );
        assert!(
            !live_prompt_drift_auto_recovery_safe(
                &snapshot,
                &fragmented,
                normalize_visible_recovery_compare,
            ),
            "a disk-only user prompt must block auto-recovery"
        );
    }
    #[test]
    fn live_prompt_drift_auto_recovery_preserves_disk_only_queue_item() {
        // A user-added `do [#id]` queue line is disjoint realtime state: the
        // response can land while the queue edit remains in the merged target.
        let snapshot = agent_doc_test_support::drift_content_ours();
        let fragmented = agent_doc_test_support::drift_baseline().replace(
            "- do [#fix]\n<!-- /agent:queue -->",
            "- do [#fix]\n- do [#user-added-queue-item]\n<!-- /agent:queue -->",
        );
        let target = live_prompt_drift_recovery_target(
            &snapshot,
            &fragmented,
            normalize_visible_recovery_compare,
        )
        .expect("queue edits should be preserved while the response lands");
        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- do [#user-added-queue-item]"));
    }

    #[test]
    fn live_prompt_drift_auto_recovery_preserves_partial_exchange_word() {
        // A raw word typed into the exchange after preflight is operator-visible
        // document text even when it is not yet a complete prompt. Recovery may
        // append the missing agent response, but it must not reset the exchange
        // back to the pre-typing snapshot.
        let snapshot = agent_doc_test_support::drift_content_ours();
        let fragmented = agent_doc_test_support::drift_baseline().replace(
            "❯ do #fix\n<!-- /agent:exchange -->",
            "❯ do #fix\noperator-partial-wo\n<!-- /agent:exchange -->",
        );

        let target = live_prompt_drift_recovery_target(
            &snapshot,
            &fragmented,
            normalize_visible_recovery_compare,
        )
        .expect("partial exchange text should be preserved while the response lands");
        assert!(target.contains("### Re: do #fix"));
        assert!(
            target.contains("operator-partial-wo"),
            "operator-typed partial word must survive recovery:\n{target}"
        );
    }

    #[test]
    fn live_prompt_drift_auto_recovery_preserves_disk_only_backlog_text() {
        // Ordinary operator text is just as authoritative as prompt-shaped text:
        // realtime recovery keeps it and adds only the missing response.
        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- existing backlog text\n<!-- /agent:backlog -->\n",
            agent_doc_test_support::drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- existing backlog text with operator word\n<!-- /agent:backlog -->\n",
            agent_doc_test_support::drift_baseline()
        );

        let target = live_prompt_drift_recovery_target(
            &snapshot,
            &fragmented,
            normalize_visible_recovery_compare,
        )
        .expect("backlog edits should be preserved while the response lands");
        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- existing backlog text with operator word"));
        assert!(!target.contains("- existing backlog text\n<!-- /agent:backlog -->"));
    }

    #[test]
    fn live_prompt_drift_auto_recovery_preserves_operator_deleted_backlog_text() {
        // Operator deletions are also authoritative. Recovery must not resurrect
        // a deleted backlog line while restoring the agent response.
        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- keep this\n- operator deleted this\n<!-- /agent:backlog -->\n",
            agent_doc_test_support::drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- keep this\n<!-- /agent:backlog -->\n",
            agent_doc_test_support::drift_baseline()
        );

        let target = live_prompt_drift_recovery_target(
            &snapshot,
            &fragmented,
            normalize_visible_recovery_compare,
        )
        .expect("backlog deletions should be preserved while the response lands");
        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- keep this"));
        assert!(!target.contains("operator deleted this"));
    }

    #[test]
    fn live_prompt_drift_auto_recovery_preserves_operator_edited_backlog_text() {
        // Same for edits/replacements: the file line is not a prompt, but the
        // operator-visible value must win over the older snapshot value.
        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- original backlog wording\n<!-- /agent:backlog -->\n",
            agent_doc_test_support::drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- edited backlog wording\n<!-- /agent:backlog -->\n",
            agent_doc_test_support::drift_baseline()
        );

        let target = live_prompt_drift_recovery_target(
            &snapshot,
            &fragmented,
            normalize_visible_recovery_compare,
        )
        .expect("backlog edits should be preserved while the response lands");
        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- edited backlog wording"));
        assert!(!target.contains("- original backlog wording"));
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_rebases_onto_post_preflight_response_block_deletion() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        let historical =
            "### Re: do #old — gpt-5\n\nHistorical answer the operator deleted after preflight.\n";
        let preflight = agent_doc_test_support::drift_baseline().replace(
            "❯ do #fix\n",
            &format!("❯ do #old\n{historical}❯ do #fix\n"),
        );
        let snapshot = agent_doc_test_support::drift_content_ours().replace(
            "❯ do #fix\n",
            &format!("❯ do #old\n{historical}❯ do #fix\n"),
        );
        let current = preflight.replace(historical, "");
        fs::write(&doc, &current).unwrap();
        agent_doc_snapshot_io::save(&doc, &snapshot, agent_doc_ops_log_io::log_op).unwrap();
        // Preflight observed the historical response. The operator deleted it
        // before auto-recovery ran, so recovery must not resurrect it while
        // trying to restore the new response.
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&snapshot), Some(&preflight)).unwrap();
        agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &current).unwrap();
        assert!(
            recovered.as_deref().is_some_and(|content| {
                content.contains("### Re: do #fix")
                    && !content.contains("Historical answer the operator deleted")
            }),
            "post-preflight response-block deletion should be preserved while the new response lands"
        );
        let disk = fs::read_to_string(&doc).unwrap();
        assert!(disk.contains("### Re: do #fix"));
        assert!(!disk.contains("Historical answer the operator deleted"));
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_advances_snapshot_to_operator_preserving_merge() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- original backlog wording\n<!-- /agent:backlog -->\n",
            agent_doc_test_support::drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- edited backlog wording\n<!-- /agent:backlog -->\n",
            agent_doc_test_support::drift_baseline()
        );
        fs::write(&doc, &fragmented).unwrap();
        agent_doc_snapshot_io::save(&doc, &snapshot, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&snapshot), Some(&fragmented))
            .unwrap();
        agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented)
            .unwrap()
            .expect("response should merge onto edited backlog");

        assert!(recovered.contains("### Re: do #fix"));
        assert!(recovered.contains("- edited backlog wording"));
        assert!(!recovered.contains("- original backlog wording"));
        assert_eq!(fs::read_to_string(&doc).unwrap(), recovered);
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap().as_deref(),
            Some(recovered.as_str()),
            "snapshot must advance to the operator-preserving merged document"
        );
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_writes_realtime_merge_when_blocked_and_safe() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = agent_doc_test_support::drift_content_ours();
        let fragmented = agent_doc_test_support::drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        agent_doc_snapshot_io::save(&doc, &snapshot, agent_doc_ops_log_io::log_op).unwrap();
        // The drift guard fired this cycle and adopted content_ours.
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&snapshot), Some(&fragmented))
            .unwrap();
        agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert_eq!(
            recovered.as_deref(),
            Some(snapshot.as_str()),
            "the no-operator-drift merge equals the candidate response snapshot"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            snapshot,
            "the working-tree file should now carry the full response"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("live_prompt_drift_auto_recovered"),
            "auto-recovery must leave an observable ops.log marker:\n{log}"
        );
    }
    #[test]
    fn try_auto_recover_live_prompt_drift_prefers_editor_ipc_when_listener_active() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("visible-write")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = agent_doc_test_support::drift_content_ours();
        let fragmented = agent_doc_test_support::drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        agent_doc_snapshot_io::save(&doc, &snapshot, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&snapshot), Some(&fragmented))
            .unwrap();
        agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let _listener = agent_doc_test_support::start_live_prompt_drift_receipt_listener(
            dir.path(),
            snapshot.clone(),
        );
        agent_doc_test_support::wait_for_live_prompt_drift_listener(dir.path());

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert_eq!(
            recovered.as_deref(),
            Some(snapshot.as_str()),
            "recovery should accept the editor-applied snapshot"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            snapshot,
            "the fake editor listener should converge the working tree through IPC"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("[jbstalecache] editor_convergence_attempt")
                && log.contains("[jbstalecache] editor_convergence_succeeded"),
            "active listener recovery should be observable as editor convergence:\n{log}"
        );
        assert!(
            log.contains("live_prompt_drift_auto_recovered")
                && log.contains("transport=editor_ipc")
                && log.contains("ipc_listener_active=true"),
            "recovery marker should name the editor transport:\n{log}"
        );
        assert!(
            !log.contains("auto_recovery_disk_write_during_ipc_listener")
                && !log.contains("transport=disk_fallback"),
            "successful editor convergence must not take the stale-cache disk fallback:\n{log}"
        );
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_editor_ipc_preserves_partial_exchange_word() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("visible-write")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = agent_doc_test_support::drift_content_ours();
        let fragmented = agent_doc_test_support::drift_baseline().replace(
            "❯ do #fix\n<!-- /agent:exchange -->",
            "❯ do #fix\noperator-partial-wo\n<!-- /agent:exchange -->",
        );
        let recovery_target = live_prompt_drift_recovery_target(
            &snapshot,
            &fragmented,
            normalize_visible_recovery_compare,
        )
        .expect("partial exchange text should be preserved in the target");
        fs::write(&doc, &fragmented).unwrap();
        agent_doc_snapshot_io::save(&doc, &snapshot, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&snapshot), Some(&fragmented))
            .unwrap();
        agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let _listener = agent_doc_test_support::start_live_prompt_drift_receipt_listener(
            dir.path(),
            recovery_target.clone(),
        );
        agent_doc_test_support::wait_for_live_prompt_drift_listener(dir.path());

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert_eq!(
            recovered.as_deref(),
            Some(recovery_target.as_str()),
            "editor IPC recovery should accept the operator-preserving target"
        );
        let visible = fs::read_to_string(&doc).unwrap();
        assert!(
            visible.contains("operator-partial-wo") && visible.contains("### Re: do #fix"),
            "the fake editor listener must retain the partial word and land the response:\n{visible}"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("transport=editor_ipc")
                && log.contains("ipc_listener_active=true")
                && !log.contains("transport=disk_fallback"),
            "partial-word recovery must go through editor IPC without disk fallback:\n{log}"
        );
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_blocks_disk_fallback_with_active_listener_without_visible_write_receipt()
     {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("visible-write")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = agent_doc_test_support::drift_content_ours();
        let fragmented = agent_doc_test_support::drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        agent_doc_snapshot_io::save(&doc, &snapshot, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&snapshot), Some(&fragmented))
            .unwrap();
        agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let _listener = agent_doc_test_support::start_receipt_without_content_listener(dir.path());
        agent_doc_test_support::wait_for_live_prompt_drift_listener(dir.path());

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert!(
            recovered.is_none(),
            "active listener without visible-write receipt must block binary-owned disk recovery"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            fragmented,
            "auto-recovery must not write the merged target behind the editor"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("[jbstalecache] editor_convergence_no_visible_write_receipt")
                && log.contains("action=block_external_disk_write"),
            "unproven editor convergence must be logged as a blocked write:\n{log}"
        );
        assert!(
            log.contains("[jbstalecache] auto_recovery_disk_write_blocked")
                && log.contains("reason=editor_ipc_unconfirmed"),
            "auto-recovery must record that it refused the disk fallback:\n{log}"
        );
        assert!(
            !log.contains("auto_recovery_disk_write_during_ipc_listener")
                && !log.contains("transport=disk_fallback"),
            "active listener recovery must not take or advertise the disk fallback:\n{log}"
        );
    }
    #[test]
    fn try_auto_recover_live_prompt_drift_skips_without_blocked_flag() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = agent_doc_test_support::drift_content_ours();
        let fragmented = agent_doc_test_support::drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        agent_doc_snapshot_io::save(&doc, &snapshot, agent_doc_ops_log_io::log_op).unwrap();
        // A cycle exists but the drift guard never fired (flag stays false) →
        // not the wedge we own.
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&snapshot), Some(&fragmented))
            .unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert!(
            recovered.is_none(),
            "without the drift flag this is not the auto-recovery case"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            fragmented,
            "the working tree must be untouched when recovery does not apply"
        );
    }
    #[test]
    fn try_auto_recover_live_prompt_drift_skips_when_dropped_prompts_recorded() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = agent_doc_test_support::drift_content_ours();
        let fragmented = agent_doc_test_support::drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        agent_doc_snapshot_io::save(&doc, &snapshot, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&snapshot), Some(&fragmented))
            .unwrap();
        agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(&doc).unwrap();
        // A genuine dropped user prompt was recorded this cycle → session-check
        // owns the fail-closed; auto-recovery must NOT paper over it.
        agent_doc_cycle_state_io::record_dropped_exchange_prompts(
            &doc,
            &["do #dropped".to_string()],
        )
        .unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert!(
            recovered.is_none(),
            "recorded dropped prompts must block auto-recovery (fail closed)"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            fragmented,
            "the working tree must be untouched when a dropped prompt was recorded"
        );
    }
    #[test]
    fn snapshot_contains_dropped_prompt_matches_consumed_and_active() {
        let snapshot = concat!(
            "<!-- agent:queue go -->\n",
            "- ~~do [#consumed]~~\n",
            "- do [#active]\n",
            "<!-- /agent:queue -->\n",
        );
        // Consumed (struck) item still present → not lost.
        assert!(snapshot_contains_dropped_prompt(snapshot, "do [#consumed]"));
        // Active item present → not lost.
        assert!(snapshot_contains_dropped_prompt(snapshot, "do [#active]"));
        // Genuinely absent → real loss.
        assert!(!snapshot_contains_dropped_prompt(snapshot, "do [#gone]"));
    }
    #[test]
    fn try_auto_recover_live_prompt_drift_fires_when_dropped_prompt_is_consumed_in_snapshot() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        // Snapshot consumed the queued `do [#fix]` (struck) and carries the full
        // `### Re:` response; the fragmented disk file also struck it but lost the
        // response body → wedge shape.
        let snapshot = agent_doc_test_support::drift_content_ours()
            .replace("- do [#fix]\n", "- ~~do [#fix]~~\n");
        let fragmented =
            agent_doc_test_support::drift_baseline().replace("- do [#fix]\n", "- ~~do [#fix]~~\n");
        fs::write(&doc, &fragmented).unwrap();
        agent_doc_snapshot_io::save(&doc, &snapshot, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&snapshot), Some(&fragmented))
            .unwrap();
        agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(&doc).unwrap();
        // The drift heuristic recorded the consumed item as a dropped queue prompt.
        agent_doc_cycle_state_io::record_dropped_queue_prompts(&doc, &["do [#fix]".to_string()])
            .unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert!(
            recovered.is_some(),
            "a dropped prompt that survives (struck) in the snapshot must not block recovery"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            snapshot,
            "auto-recovery must write the realtime merge target to disk"
        );

        // `#jbstalecache`: the recovery write records the IPC-listener state so the
        // operator can correlate a stale-cache dialog with this disk write. No live
        // listener exists in the test env, so the canonical marker reports
        // `ipc_listener_active=false` and the dedicated stale-cache-risk line stays
        // silent (it only fires when a listener is genuinely active).
        let ops_log =
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("live_prompt_drift_auto_recovered")
                && ops_log.contains("ipc_listener_active=false"),
            "recovery marker must record the IPC-listener state:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("[jbstalecache]"),
            "the stale-cache-risk marker must stay silent without an active listener:\n{ops_log}"
        );
    }
}
