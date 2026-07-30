use anyhow::Result;
use std::path::Path;

pub use agent_doc_turn::closeout_recovery::CloseoutRecoveryState;

pub use agent_doc_flow_io::closeout::{
    CloseoutBinaryFreshnessEvidence, CloseoutCaptureEvidence, CloseoutCycleEvidence,
    CloseoutEditorIpcEvidence, CloseoutQueueOnlyDriftEvidence, CloseoutRecoveryEvidence,
    CloseoutResponseBodyEvidence, RecoveryApplication, closeout_recovery_command_for_file,
    reconcile_compacted_committed_capture, stuck_captured_cycle,
};

pub fn complete_required_closeout(file: &Path) -> Result<bool> {
    agent_doc_flow_io::closeout::complete_required_closeout(
        file,
        &agent_doc_closeout_runtime_io::closeout_effects(),
    )
}

struct LocalCrdtBarrierCloseoutEffects;

impl agent_doc_flow_io::closeout::CloseoutEffects for LocalCrdtBarrierCloseoutEffects {
    fn commit(&self, file: &Path) -> Result<bool> {
        agent_doc_closeout_runtime_io::closeout_effects().commit(file)
    }

    fn commit_for_authority(&self, file: &Path, force_disk: bool) -> Result<bool> {
        agent_doc_closeout_runtime_io::closeout_effects().commit_for_authority(file, force_disk)
    }

    fn crdt_commit_barrier(&self, file: &Path) -> Result<bool> {
        Ok(agent_doc_crdt_relay_io::commit_barrier_for_file(file))
    }

    fn run_pending_maintenance(
        &self,
        file: &Path,
        force_disk: bool,
    ) -> Result<agent_doc_preflight_io::PendingMaintenanceReport> {
        agent_doc_closeout_runtime_io::closeout_effects().run_pending_maintenance(file, force_disk)
    }

    fn enforce_clean_closeout(&self, file: &Path) -> Result<()> {
        agent_doc_closeout_runtime_io::closeout_effects().enforce_clean_closeout(file)
    }

    fn enforce_clean_closeout_for_authority(&self, file: &Path, force_disk: bool) -> Result<()> {
        agent_doc_closeout_runtime_io::closeout_effects()
            .enforce_clean_closeout_for_authority(file, force_disk)
    }

    fn cancel_preflight_cycle(&self, file: &Path) -> Result<()> {
        agent_doc_closeout_runtime_io::closeout_effects().cancel_preflight_cycle(file)
    }

    fn detect_jb_cache_conflict_cancel_recoverable(&self, file: &Path) -> Result<bool> {
        agent_doc_closeout_runtime_io::closeout_effects()
            .detect_jb_cache_conflict_cancel_recoverable(file)
    }

    fn detect_bypassed_response_write(&self, file: &Path) -> Result<Option<String>> {
        agent_doc_closeout_runtime_io::closeout_effects().detect_bypassed_response_write(file)
    }

    fn resolve_current_document(
        &self,
        file: &Path,
        source: &str,
    ) -> Result<agent_doc_document_realtime::CurrentDocument> {
        agent_doc_closeout_runtime_io::closeout_effects().resolve_current_document(file, source)
    }

    fn resolve_current_document_for_authority(
        &self,
        file: &Path,
        source: &str,
        force_disk: bool,
    ) -> Result<agent_doc_document_realtime::CurrentDocument> {
        agent_doc_closeout_runtime_io::closeout_effects()
            .resolve_current_document_for_authority(file, source, force_disk)
    }

    fn write_current_document(
        &self,
        doc: &agent_doc_document_realtime::CurrentDocument,
        content: &str,
        source: &str,
    ) -> Result<()> {
        agent_doc_closeout_runtime_io::closeout_effects()
            .write_current_document(doc, content, source)
    }

    fn mark_committed_frontmatter(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<agent_doc_cycle_state_io::CycleState> {
        agent_doc_closeout_runtime_io::closeout_effects().mark_committed_frontmatter(
            file,
            event,
            snapshot_content,
            file_content,
        )
    }

    fn mark_abandoned_frontmatter(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<agent_doc_cycle_state_io::CycleState> {
        agent_doc_closeout_runtime_io::closeout_effects().mark_abandoned_frontmatter(
            file,
            event,
            snapshot_content,
            file_content,
        )
    }
}

fn complete_required_closeout_with_local_crdt_barrier(file: &Path) -> Result<bool> {
    agent_doc_flow_io::closeout::complete_required_closeout(file, &LocalCrdtBarrierCloseoutEffects)
}

pub fn apply_closeout_recovery(file: &Path) -> Result<RecoveryApplication> {
    agent_doc_flow_io::closeout::apply_closeout_recovery(
        file,
        &agent_doc_closeout_runtime_io::closeout_effects(),
    )
}

pub fn gather_closeout_recovery_evidence(file: &Path) -> Result<CloseoutRecoveryEvidence> {
    agent_doc_flow_io::closeout::gather_closeout_recovery_evidence(
        file,
        &agent_doc_closeout_runtime_io::closeout_effects(),
    )
}

pub fn observe_closeout_recovery_evidence(file: &Path) -> Result<CloseoutRecoveryEvidence> {
    agent_doc_flow_io::closeout::observe_closeout_recovery_evidence(
        file,
        &agent_doc_closeout_runtime_io::closeout_effects(),
    )
}

pub fn load_current_observed_closeout_recovery_evidence(
    file: &Path,
) -> Result<Option<CloseoutRecoveryEvidence>> {
    agent_doc_flow_io::closeout::load_current_observed_closeout_recovery_evidence(
        file,
        &agent_doc_closeout_runtime_io::closeout_effects(),
    )
}

pub fn classify_closeout_recovery_state_for_file(
    file: &Path,
) -> agent_doc_turn::closeout_recovery::CloseoutRecoveryState {
    agent_doc_flow_io::closeout::classify_closeout_recovery_state_for_file(
        file,
        &agent_doc_closeout_runtime_io::closeout_effects(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    struct TestPipelineFrontmatterEffects;

    static TEST_PIPELINE_FRONTMATTER_EFFECTS: TestPipelineFrontmatterEffects =
        TestPipelineFrontmatterEffects;

    impl agent_doc_cycle_state_io::pipeline_frontmatter::PipelineFrontmatterEffects
        for TestPipelineFrontmatterEffects
    {
        fn read_current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            Ok(std::fs::read_to_string(file)?)
        }

        fn converge_or_disk_write(
            &self,
            file: &Path,
            current_content: &str,
            target_content: &str,
            reason: &str,
        ) -> Result<()> {
            let observed = std::fs::read_to_string(file)?;
            anyhow::ensure!(
                observed == current_content,
                "pipeline frontmatter test write saw stale content for reason {reason}"
            );
            std::fs::write(file, target_content)?;
            Ok(())
        }

        fn log_op(&self, file: &Path, message: &str) {
            agent_doc_ops_log_io::log_op(file, message);
        }
    }

    fn seed_live_reliable_sync_open(file: &Path) {
        let document_hash = agent_doc_hash::document_id_for_path(file);
        let pid = std::process::id();
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid: pid.into(),
                tag: format!("test-editor-{pid}:{}", file.display()),
            }]);
    }

    fn setup_git_project_with_doc(base: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        run_git(dir.path(), &["init"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test User"]);
        run_git(dir.path(), &["add", "doc.md"]);
        run_git(dir.path(), &["commit", "-m", "initial", "--no-verify"]);
        (dir, doc)
    }

    fn run_git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed with {status}");
    }

    #[test]
    fn complete_required_closeout_reaps_lingering_completed_item() {
        // #exit75-done-reap-not-atomic: the exit-75 / file-IPC fallback commits a
        // `[x]` item without reaping it, then reaches complete_required_closeout.
        // The closeout must reap the lingering completed item in the same pass so
        // session-check passes without a separate recovery preflight.
        let base = concat!(
            "---\nagent_doc_format: template\nsession: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: close the loop — gpt-5\n\nImplemented and verified.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#donelinger] Close the loop\n",
            "- [ ] [#keep] Keep tracking follow-up\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n<!-- /agent:done -->\n",
        );
        let (_dir, doc) = setup_git_project_with_doc(base);

        // Committed response cycle (capture present), with the `[x]` already on
        // disk + in HEAD — exactly the exit-75 residual shape.
        let state =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let response = "<!-- patch:exchange -->\n### Re: close the loop — gpt-5\n\nImplemented and verified.\n<!-- /patch:exchange -->";
        agent_doc_capture_io::capture_response(&doc, response).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(base),
            Some(base),
        )
        .unwrap();

        complete_required_closeout(&doc).expect("closeout must reap the lingering completed item");

        let content = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !content.contains("- [x] [#donelinger]"),
            "closeout must reap the lingering completed item:\n{content}"
        );
        assert!(
            content.contains("- [ ] [#keep] Keep tracking follow-up"),
            "live follow-up must remain:\n{content}"
        );
        assert!(
            content.contains("<!-- agent:done -->") && content.contains("[#donelinger]"),
            "reaped item must be archived to agent:done:\n{content}"
        );

        // HEAD reflects the reap, and session-check accepts the closeout.
        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            !head.contains("- [x] [#donelinger]"),
            "HEAD must not strand the completed item:\n{head}"
        );
        matches!(
            agent_doc_session_check_io::inspect(
                &doc,
                &agent_doc_closeout_runtime_io::session_check_effects()
            )
            .unwrap(),
            agent_doc_session_check_io::SessionCheckStatus::Ok(_)
        )
        .then_some(())
        .expect("session-check must accept the atomic-reap closeout");

        let terminal = agent_doc_cycle_state_io::load_latest_terminal_closeout_proof(&doc)
            .unwrap()
            .expect("closeout must record a typed terminal proof projection");
        assert_eq!(terminal.cycle_id, state.cycle_id);
        assert!(terminal.last_event.contains("commit"));
        assert!(terminal.did_commit);
        assert_eq!(terminal.file_hash, terminal.snapshot_hash);
        assert_eq!(terminal.snapshot_hash, terminal.head_hash);
        assert_eq!(terminal.agreement.as_str(), "file_snapshot_head");
    }

    #[test]
    fn terminal_proof_preserves_newer_queue_prompts_after_committed_response() {
        let committed = concat!(
            "---\nagent_doc_format: template\nsession: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: draft complete — gpt-5\n\nDrafted and autosaved.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n",
        );
        let (_dir, doc) = setup_git_project_with_doc(committed);
        let state =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(committed))
                .unwrap();
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: draft complete — gpt-5\n\nDrafted and autosaved.\n",
            "<!-- /patch:exchange -->",
        );
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        let visible = committed.replace(
            "<!-- agent:queue -->\n<!-- /agent:queue -->",
            "<!-- agent:queue -->\n- remove unnecessary hours disclosure\n<!-- /agent:queue -->",
        );
        std::fs::write(&doc, &visible).unwrap();

        agent_doc_flow_io::closeout::record_terminal_closeout_proof(
            &doc,
            true,
            &agent_doc_closeout_runtime_io::closeout_effects(),
            agent_doc_flow_io::closeout::CompleteRequiredCloseoutOptions::default(),
        )
        .expect("newer queue-only visible state must not invalidate committed response proof");

        let terminal = agent_doc_cycle_state_io::load_latest_terminal_closeout_proof(&doc)
            .unwrap()
            .expect("terminal proof");
        assert_eq!(terminal.cycle_id, state.cycle_id);
        assert_eq!(
            terminal.capture_id.as_deref(),
            Some(capture.capture_id.as_str())
        );
        assert_eq!(
            terminal.response_sha256.as_deref(),
            Some(capture.response_sha256.as_str())
        );
        assert_eq!(
            terminal.agreement,
            agent_doc_state_backbone::TerminalCloseoutAgreement::SnapshotHeadVisibleDrift
        );
        assert!(terminal.response_materialized_in_head);
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), visible);
    }

    #[test]
    fn complete_required_closeout_records_terminal_proof_after_abandoned_preflight_cycle() {
        // An interrupted/stale preflight may be force-closed as `abandoned` by the
        // supervisor recycle path. A later visible repair/response closeout that
        // successfully commits must not inherit that old abandoned terminal state;
        // terminal proof should attach to the new committed closeout.
        let base = concat!(
            "---\nagent_doc_format: template\nsession: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior - gpt-5\n\nDone.\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let (_dir, doc) = setup_git_project_with_doc(base);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let abandoned = agent_doc_cycle_state_io::pipeline_frontmatter::mark_abandoned(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "suprecyclespin_stalled_cycle_resolved",
            Some(base),
            Some(base),
        )
        .unwrap();
        assert_eq!(abandoned.phase, agent_doc_turn::CyclePhase::Abandoned);

        let closed = base.replace(
            "<!-- agent:boundary:base -->",
            "### Re: repair closeout - gpt-5\n\nRecovered.\n<!-- agent:boundary:repair -->",
        );
        std::fs::write(&doc, &closed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &closed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        complete_required_closeout(&doc)
            .expect("abandoned stale cycle must not block terminal closeout proof");

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_ne!(
            state.cycle_id, abandoned.cycle_id,
            "new committed closeout should not reuse the stale abandoned cycle id"
        );
        let terminal = agent_doc_cycle_state_io::load_latest_terminal_closeout_proof(&doc)
            .unwrap()
            .expect("terminal proof should project for the new committed cycle");
        assert_eq!(terminal.cycle_id, state.cycle_id);
        assert_eq!(terminal.agreement.as_str(), "file_snapshot_head");
    }

    #[test]
    fn complete_required_closeout_blocks_until_live_replica_delivery_is_acked() {
        use agent_doc_merge::crdt_sync::ReplicaState;

        let base = concat!(
            "---\nagent_doc_format: template\nsession: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: base — gpt-5\n\nBase response.\n",
            "<!-- /agent:exchange -->\n",
        );
        let (_dir, doc) = setup_git_project_with_doc(base);
        // Make the document editor-attached (MultiReplica): a reliable-sync Open
        // fact for the current test process takes the real editor-attached path.
        seed_live_reliable_sync_open(&doc);
        assert!(agent_doc_crdt_relay_io::crdt_authority_for_file(&doc).editor_attached());

        let (a_id, a_bootstrap) =
            agent_doc_crdt_relay_io::register_replica_for_file(&doc, "vscode:a")
                .unwrap()
                .expect("replica A should register");
        agent_doc_crdt_relay_io::register_replica_for_file(&doc, "vscode:b")
            .unwrap()
            .expect("replica B should register");

        let a = ReplicaState::from_encoded(a_id, &a_bootstrap).unwrap();
        a.apply_local_edit(0, 0, "typed before closeout\n");
        agent_doc_crdt_relay_io::relay_replica_update_for_file(&doc, "vscode:a", &a.encode_state())
            .unwrap()
            .expect("replica A update should relay");

        let err = complete_required_closeout_with_local_crdt_barrier(&doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("CRDT relay convergence is still pending"),
            "closeout must wait for target ACK before commit: {err}"
        );

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            !head.contains("typed before closeout"),
            "pending replica delivery must not be materialized in HEAD before ACK:\n{head}"
        );
    }

    #[test]
    fn stuck_captured_cycle_reports_committed_projection_missing_response_in_head() {
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        let response = "### Re: hello — gpt-5\n\nCaptured but never committed.\n";
        let (_dir, doc) = setup_git_project_with_doc(base);

        let state =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(base),
            Some(base),
        )
        .unwrap();

        let info = stuck_captured_cycle(&doc).expect("missing HEAD response should be detected");
        assert_eq!(info.cycle_id, state.cycle_id);
        assert_eq!(info.capture_id, capture.capture_id);
        assert_eq!(info.response_body_len, response.len());
        assert_eq!(info.capture_state, "committed");
    }

    #[test]
    fn stuck_captured_cycle_uses_projection_when_capture_sidecar_is_missing() {
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        let response = "### Re: hello — gpt-5\n\nCaptured but never committed.\n";
        let (_dir, doc) = setup_git_project_with_doc(base);

        let state =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(base),
            Some(base),
        )
        .unwrap();
        assert!(!capture.capture_id.is_empty());

        let info =
            stuck_captured_cycle(&doc).expect("projected missing HEAD response should be detected");
        assert_eq!(info.cycle_id, state.cycle_id);
        assert_eq!(info.capture_id, capture.capture_id);
        assert_eq!(info.response_body_len, response.len());
        assert_eq!(info.capture_state, "committed");
    }

    #[test]
    fn stuck_captured_cycle_ignores_queue_prompt_echo_inserted_in_head() {
        // #stuck-capture-queue-echo-false-positive: when a queue head is consumed,
        // the binary inserts a `> **Queue prompt:**` echo blockquote between the
        // response heading and body. The captured response is the raw heading+body,
        // so the materialized HEAD differs by that echo. stuck_captured_cycle must
        // still treat the response as present in HEAD (no false-positive warning).
        let base = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\nOlder response.\n",
            "<!-- /agent:exchange -->\n",
        );
        let response = "### Re: do [#thing] — gpt-5\n\nShipped the fix.\n";
        let (dir, doc) = setup_git_project_with_doc(base);

        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        agent_doc_capture_io::capture_response(&doc, response).unwrap();

        // Materialize the response into HEAD with the queue-prompt echo inserted
        // between the heading and body, exactly as queue consumption writes it.
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\nOlder response.\n",
            "### Re: do [#thing] — gpt-5\n\n",
            "> **Queue prompt:**\n>\n> do [#thing]\n\n",
            "Shipped the fix.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        run_git(dir.path(), &["add", "doc.md"]);
        run_git(dir.path(), &["commit", "-m", "response", "--no-verify"]);
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        assert!(
            stuck_captured_cycle(&doc).is_none(),
            "queue-prompt echo inserted between heading and body must not flag the cycle stuck"
        );
    }

    #[test]
    fn stuck_captured_cycle_ignores_committed_cycle_when_response_is_in_head() {
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        let response = "### Re: hello — gpt-5\n\nCommitted response.\n";
        let full_doc = format!("{base}\n{response}");
        let (dir, doc) = setup_git_project_with_doc(base);

        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        agent_doc_capture_io::capture_response(&doc, response).unwrap();
        std::fs::write(&doc, &full_doc).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &full_doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        run_git(dir.path(), &["add", "doc.md"]);
        run_git(dir.path(), &["commit", "-m", "response", "--no-verify"]);
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&full_doc),
            Some(&full_doc),
        )
        .unwrap();

        assert!(stuck_captured_cycle(&doc).is_none());
    }

    #[test]
    fn stuck_captured_cycle_ignores_committed_cycle_when_only_guard_marker_stripped() {
        // #8j86: the captured response body carries an ephemeral
        // `<!-- no-pending-done-guard -->` guard marker that `agent_doc_document::transient_markers::strip_guard_markers`
        // removes from the committed blob. The materialization probe must mirror
        // that strip, otherwise stuck_captured_cycle false-alarms on a response
        // that IS in HEAD (seen live 2026-06-10 on agent-doc-bugs2.md capture
        // cycle-1781112407668 — no compact archive involved, body already in HEAD).
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        // Capture stores the raw patch-wrapped body including the guard marker.
        let captured = "<!-- patch:exchange -->\n<!-- no-pending-done-guard -->\n### Re: hello — gpt-5\n\nCommitted response.\n<!-- /patch:exchange -->\n";
        // Committed HEAD has the guard marker stripped (as `git::commit` does).
        let committed_response = "### Re: hello — gpt-5\n\nCommitted response.\n";
        let full_doc = format!("{base}\n{committed_response}");
        let (dir, doc) = setup_git_project_with_doc(base);

        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        agent_doc_capture_io::capture_response(&doc, captured).unwrap();
        std::fs::write(&doc, &full_doc).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &full_doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        run_git(dir.path(), &["add", "doc.md"]);
        run_git(dir.path(), &["commit", "-m", "response", "--no-verify"]);
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&full_doc),
            Some(&full_doc),
        )
        .unwrap();

        assert!(
            stuck_captured_cycle(&doc).is_none(),
            "a committed response whose only HEAD difference is the stripped guard marker must not be flagged stuck"
        );
    }

    #[test]
    fn open_cycle_recovery_command_names_durable_checkpoint() {
        let base = "---\nsession: test\n---\n\nHi\n";
        let (_dir, doc) = setup_git_project_with_doc(base);
        let started =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        agent_doc_cycle_state_io::record_turn_checkpoint(
            &doc,
            &[":pushpin: do [#durablerecycle]".to_string()],
            Some("#durablerecycle"),
            Some("#durablerecycle"),
        )
        .unwrap();
        agent_doc_cycle_state_io::record_pending_done_ids(&doc, &["#durablerecycle".to_string()])
            .unwrap();
        agent_doc_cycle_state_io::mark_pending_mutations(&doc).unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(base),
            Some(base),
            "response-sha",
            Some(&started.cycle_id),
        )
        .unwrap();

        let cmd =
            closeout_recovery_command_for_file(&doc, CloseoutRecoveryState::OpenCycle).unwrap();

        assert!(cmd.contains("resume durable checkpoint"), "{cmd}");
        assert!(cmd.contains("phase=response_captured"), "{cmd}");
        assert!(cmd.contains("target=\"#durablerecycle\""), "{cmd}");
        assert!(cmd.contains("pending_mutations=true"), "{cmd}");
        assert!(
            cmd.contains(&format!("capture_id={}", started.cycle_id)),
            "{cmd}"
        );
        assert!(!cmd.contains("--baseline-file"), "{cmd}");
        assert!(!cmd.contains("agent-doc finalize"), "{cmd}");
        assert!(!cmd.contains("agent-doc write"), "{cmd}");
    }

    #[test]
    fn recovery_evidence_gathers_hash_cycle_capture_and_fresh_editor_state() {
        let base = "---\nsession: test\n---\n\n## Exchange\n\nUser prompt\n";
        let response = "### Re: user prompt — gpt-5\n\nDone.\n";
        let (_dir, doc) = setup_git_project_with_doc(base);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
        let visible = format!("{base}\n{response}");
        std::fs::write(&doc, &visible).unwrap();
        agent_doc_crdt_relay_io::seed_embedded_relay_for_file(&doc).unwrap();
        let canonical = doc.canonicalize().unwrap();
        let canonical_key = canonical.to_string_lossy();
        seed_live_reliable_sync_open(&canonical);
        assert!(
            agent_doc_crdt_relay_io::register_replica_for_file(
                &canonical,
                &format!("flow-test-editor:{canonical_key}"),
            )
            .unwrap()
            .is_some(),
            "fresh editor evidence must come from a live CRDT relay replica"
        );

        let evidence = gather_closeout_recovery_evidence(&doc).unwrap();
        assert_eq!(
            evidence.visible_markdown_hash,
            agent_doc_capture_io::replay_file_hash(&visible)
        );
        assert_eq!(
            evidence.snapshot_hash.as_deref(),
            Some(agent_doc_hash::content_hash(base).as_str())
        );
        assert_eq!(
            evidence.active_cycle,
            Some(CloseoutCycleEvidence {
                cycle_id: capture.cycle_id.clone(),
                phase: agent_doc_turn::CyclePhase::ResponseCaptured,
            })
        );
        assert_eq!(
            evidence.active_capture,
            Some(CloseoutCaptureEvidence {
                capture_id: capture.capture_id.clone(),
                cycle_id: capture.cycle_id.clone(),
                state: agent_doc_workflow::capture::CaptureState::Captured,
                response_sha256: capture.response_sha256.clone(),
            })
        );
        assert_eq!(
            evidence.response_body,
            CloseoutResponseBodyEvidence::PresentInVisible {
                capture_id: capture.capture_id.clone(),
            }
        );
        assert_eq!(
            evidence.editor_ipc,
            CloseoutEditorIpcEvidence::FreshLiveBuffer {
                live_buffer_count: 1,
                socket_degraded: false,
            }
        );
        assert_eq!(
            evidence.binary_freshness,
            CloseoutBinaryFreshnessEvidence::NoStaleWarning
        );
    }

    #[test]
    fn recovery_evidence_uses_captured_response_projection_without_capture_sidecar() {
        let base = "---\nsession: test\n---\n\n## Exchange\n\nUser prompt\n";
        let response = "### Re: user prompt — gpt-5\n\nCaptured but not visible.\n";
        let (_dir, doc) = setup_git_project_with_doc(base);
        let state =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
        assert!(!capture.capture_id.is_empty());

        let evidence = gather_closeout_recovery_evidence(&doc).unwrap();
        assert_eq!(
            evidence.active_capture,
            Some(CloseoutCaptureEvidence {
                capture_id: capture.capture_id.clone(),
                cycle_id: state.cycle_id,
                state: agent_doc_workflow::capture::CaptureState::Captured,
                response_sha256: capture.response_sha256.clone(),
            })
        );
        assert_eq!(
            evidence.response_body,
            CloseoutResponseBodyEvidence::MissingFromVisible {
                capture_id: capture.capture_id,
            }
        );
        assert_eq!(evidence.queue_only_drift, None);
    }

    #[test]
    fn recovery_evidence_proves_queue_only_drift() {
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "user prompt\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- first head\n",
            "<!-- /agent:queue -->\n",
        );
        let (_dir, doc) = setup_git_project_with_doc(base);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let capture = agent_doc_capture_io::capture_response(
            &doc,
            "<!-- patch:exchange -->\n### Re: first head — gpt-5\n\nDone.\n<!-- /patch:exchange -->\n",
        )
        .unwrap();
        assert!(!capture.capture_id.is_empty());
        let current = base.replace(
            "- first head\n",
            "- first head\n- user typed a new queue note during closeout\n",
        );
        std::fs::write(&doc, current).unwrap();

        let evidence = gather_closeout_recovery_evidence(&doc).unwrap();
        assert_eq!(
            evidence.queue_only_drift,
            Some(CloseoutQueueOnlyDriftEvidence {
                file_hash_mismatch: true,
                snapshot_hash_mismatch: false,
                proven_queue_only: true,
            })
        );
    }

    #[test]
    fn recovery_evidence_reports_superseded_capture_heading() {
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: earlier — gpt-5\n\nOlder.\n",
            "<!-- /agent:exchange -->\n",
        );
        let (_dir, doc) = setup_git_project_with_doc(base);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let capture = agent_doc_capture_io::capture_response(
            &doc,
            "### Re: repeated prompt — gpt-5\n\nCaptured but stale.\n",
        )
        .unwrap();
        let visible = base.replace(
            "<!-- /agent:exchange -->",
            "### Re: repeated prompt — gpt-5\n\nA later answer already landed.\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, visible).unwrap();

        let evidence = gather_closeout_recovery_evidence(&doc).unwrap();
        match &evidence.response_body {
            CloseoutResponseBodyEvidence::SupersededByVisibleExchange { capture_id, proof } => {
                assert_eq!(capture_id, &capture.capture_id);
                assert!(
                    proof.contains("repeated prompt"),
                    "proof should name the answered heading: {proof}"
                );
            }
            other => panic!("expected supersession proof, got {other:?}"),
        }
        assert!(
            evidence.stale_capture_supersession_proof().is_some(),
            "decision input should be able to borrow the supersession proof"
        );
    }

    #[test]
    fn closeout_recovery_drains_superseded_capture_stack_in_one_pass() {
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: earlier — gpt-5\n\nOlder.\n",
            "<!-- /agent:exchange -->\n",
        );
        let (_dir, doc) = setup_git_project_with_doc(base);
        for attempt in 0..4 {
            agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
            agent_doc_capture_io::capture_response(
                &doc,
                &format!("### Re: repeated prompt — gpt-5\n\nRejected capture {attempt}.\n"),
            )
            .unwrap();
        }
        let visible = base.replace(
            "<!-- /agent:exchange -->",
            "### Re: repeated prompt — gpt-5\n\nA later answer already landed.\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, &visible).unwrap();

        let recovery = apply_closeout_recovery(&doc).unwrap();
        match recovery {
            RecoveryApplication::Applied { action, .. } => {
                assert!(action.contains("in one metadata-only pass"), "{action}");
            }
            other => panic!("expected fixed-point stale-capture recovery, got {other:?}"),
        }
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            visible,
            "stale-capture drainage must not rewrite visible document content",
        );
        let second = apply_closeout_recovery(&doc).unwrap();
        assert!(
            !matches!(
                second,
                RecoveryApplication::Applied { ref action, .. }
                    if action.contains("superseded captured-only")
            ),
            "one recovery invocation must leave no second stale-capture retirement pass: {second:?}",
        );
    }

    #[test]
    fn observed_recovery_evidence_loads_from_projection_without_capture_sidecar() {
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: earlier — gpt-5\n\nOlder.\n",
            "<!-- /agent:exchange -->\n",
        );
        let (_dir, doc) = setup_git_project_with_doc(base);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let capture = agent_doc_capture_io::capture_response(
            &doc,
            "### Re: repeated prompt — gpt-5\n\nCaptured but stale.\n",
        )
        .unwrap();
        let visible = base.replace(
            "<!-- /agent:exchange -->",
            "### Re: repeated prompt — gpt-5\n\nA later answer already landed.\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, visible).unwrap();

        observe_closeout_recovery_evidence(&doc).unwrap();
        assert!(!capture.capture_id.is_empty());

        let observed = load_current_observed_closeout_recovery_evidence(&doc)
            .unwrap()
            .expect("visible-hash-matching recovery evidence projection");
        assert_eq!(
            observed
                .active_capture
                .as_ref()
                .map(|capture| capture.capture_id.as_str()),
            Some(capture.capture_id.as_str())
        );
        match &observed.response_body {
            CloseoutResponseBodyEvidence::SupersededByVisibleExchange { capture_id, proof } => {
                assert_eq!(capture_id, &capture.capture_id);
                assert!(proof.contains("repeated prompt"));
            }
            other => panic!("expected projected supersession proof, got {other:?}"),
        }
    }

    #[test]
    fn observed_recovery_evidence_ignores_stale_snapshot_when_visible_hash_matches() {
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        let (_dir, doc) = setup_git_project_with_doc(base);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();

        let recorded = observe_closeout_recovery_evidence(&doc).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "obsolete snapshot sidecar baseline",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let loaded = load_current_observed_closeout_recovery_evidence(&doc)
            .unwrap()
            .expect("snapshot sidecar drift must not veto current typed recovery evidence");
        assert_eq!(loaded.visible_markdown_hash, recorded.visible_markdown_hash);
        assert_eq!(
            loaded.snapshot_hash, recorded.snapshot_hash,
            "the typed observation retains its recorded snapshot as advisory evidence"
        );
    }

    #[test]
    fn classify_recovery_missing_response_body_uses_observed_projection_without_capture_sidecar() {
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        let response = "### Re: hello — gpt-5\n\nCaptured but never committed.\n";
        let (_dir, doc) = setup_git_project_with_doc(base);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(base),
            Some(base),
        )
        .unwrap();

        observe_closeout_recovery_evidence(&doc).unwrap();
        assert!(!capture.capture_id.is_empty());

        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::MissingResponseBody
        );
    }

    #[test]
    fn observed_recovery_evidence_requires_current_visible_hash() {
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "user prompt\n",
            "<!-- /agent:exchange -->\n",
        );
        let (_dir, doc) = setup_git_project_with_doc(base);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        agent_doc_capture_io::capture_response(&doc, "### Re: user prompt — gpt-5\n\nCaptured.\n")
            .unwrap();
        observe_closeout_recovery_evidence(&doc).unwrap();

        std::fs::write(&doc, format!("{base}\noperator changed visible text\n")).unwrap();

        assert!(
            load_current_observed_closeout_recovery_evidence(&doc)
                .unwrap()
                .is_none(),
            "stale visible hash must reject projected recovery evidence"
        );
    }

    #[test]
    fn classify_recovery_clean_without_cycle_state() {
        let (_dir, doc) = setup_git_project_with_doc("---\nsession: test\n---\n\nHi\n");
        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::Clean
        );
    }

    #[test]
    fn classify_recovery_open_empty_preflight_when_nothing_followed() {
        // `#recursive-repair-recovery-states`: a bare preflight_started cycle with
        // no capture / response / pending mutation is an abandonable probe.
        let base = "---\nsession: test\n---\n\nHi\n";
        let (_dir, doc) = setup_git_project_with_doc(base);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::OpenEmptyPreflight
        );
        let cmd =
            closeout_recovery_command_for_file(&doc, CloseoutRecoveryState::OpenEmptyPreflight)
                .unwrap();
        assert!(cmd.contains("agent-doc cancel"), "{cmd}");
    }

    #[test]
    fn classify_recovery_open_cycle_when_preflight_has_pending_mutations() {
        // A preflight_started cycle that already did work (pending mutation) is a
        // real open cycle to finish, not an abandonable empty probe.
        let base = "---\nsession: test\n---\n\nHi\n";
        let (_dir, doc) = setup_git_project_with_doc(base);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        agent_doc_cycle_state_io::mark_pending_mutations(&doc).unwrap();
        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::OpenCycle
        );
    }

    #[test]
    fn classify_recovery_queue_metadata_drift_when_only_queue_differs() {
        // `#recursive-repair-recovery-states`: snapshot differs from HEAD only by
        // queue lines (a `queue` sync regeneration); exchange content identical.
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: x — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let snapshot = head.replace("- do [#a]\n", "- do [#a]\n- do [#b]\n");
        let (_dir, doc) = setup_git_project_with_doc(head);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(head), Some(head)).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&snapshot),
            Some(&snapshot),
        )
        .unwrap();
        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::QueueMetadataDrift
        );
    }

    #[test]
    fn classify_recovery_queue_metadata_drift_uses_observed_projection_without_snapshot_sidecar() {
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: x — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let snapshot = head.replace("- do [#a]\n", "- do [#a]\n- do [#b]\n");
        let (_dir, doc) = setup_git_project_with_doc(head);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(head), Some(head)).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&snapshot),
            Some(&snapshot),
        )
        .unwrap();

        let evidence = observe_closeout_recovery_evidence(&doc).unwrap();
        assert_eq!(
            evidence.snapshot_head_drift,
            Some(agent_doc_turn::closeout_recovery::CloseoutRecoveryDrift::MetadataOnly)
        );
        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::QueueMetadataDrift
        );
    }

    #[test]
    fn classify_recovery_unsafe_user_content_drift_when_exchange_differs() {
        // Real user/response content differs from HEAD → must not auto-commit.
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: x — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n",
        );
        let snapshot = head.replace("Done.\n", "Done.\n\nReal unreviewed user content.\n");
        let (_dir, doc) = setup_git_project_with_doc(head);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(head), Some(head)).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&snapshot),
            Some(&snapshot),
        )
        .unwrap();
        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::UnsafeUserContentDrift
        );
    }

    #[test]
    fn classify_recovery_direct_response_patchback_when_visible_response_uncommitted() {
        // `#closeout-recovery-state-machine`: a `### Re:` response was patched
        // directly into the working file outside the binary write path (snapshot
        // and HEAD are clean, the working file gained the response). Classified as
        // DirectResponsePatchback → recover with `write --commit`, NOT the generic
        // UnsafeUserContentDrift / RecoveryProjectionVisibleDrift.
        let base = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n❯ a question\n<!-- /agent:exchange -->\n",
        );
        let (_dir, doc) = setup_git_project_with_doc(base);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(base),
            Some(base),
        )
        .unwrap();
        // Patch a visible response directly into the working file (bypassing write).
        let with_response = base.replace(
            "❯ a question\n",
            "❯ a question\n### Re: a question — gpt-5\n\nDirect answer.\n",
        );
        std::fs::write(&doc, &with_response).unwrap();
        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::DirectResponsePatchback
        );
        let cmd = closeout_recovery_command_for_file(
            &doc,
            CloseoutRecoveryState::DirectResponsePatchback,
        )
        .unwrap();
        assert!(cmd.contains("write --commit"), "{cmd}");
    }

    #[test]
    fn classify_recovery_nested_parent_pointer_stale_when_submodule_ahead_of_parent() {
        // `#closeout-recovery-state-machine`: the document is clean (snapshot ==
        // HEAD == working) but its submodule HEAD is ahead of the parent repo's
        // recorded pointer (a reaped item left the parent pointer un-bumped).
        // Classified as NestedParentPointerStale → recover with `agent-doc commit`.
        let root = tempfile::TempDir::new().unwrap();
        let sub_origin = root.path().join("sub_origin");
        let sup = root.path().join("super");
        let init_repo = |p: &Path| {
            std::fs::create_dir_all(p).unwrap();
            run_git(p, &["init"]);
            run_git(p, &["config", "user.email", "test@example.com"]);
            run_git(p, &["config", "user.name", "Test User"]);
        };
        // Submodule origin with an initial commit (S1).
        init_repo(&sub_origin);
        std::fs::write(sub_origin.join("doc.md"), "---\nsession: t\n---\n\nv1\n").unwrap();
        run_git(&sub_origin, &["add", "."]);
        run_git(&sub_origin, &["commit", "-m", "s1", "--no-verify"]);
        // Super repo records the submodule pointer at S1.
        init_repo(&sup);
        run_git(
            &sup,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                sub_origin.to_str().unwrap(),
                "sub",
            ],
        );
        run_git(&sup, &["commit", "-m", "add sub", "--no-verify"]);
        // Advance the submodule HEAD (S2) WITHOUT bumping the parent pointer.
        let subwt = sup.join("sub");
        // The checked-out submodule's git repo (`super/.git/modules/sub`) may not
        // inherit the parent's local identity, and a clean CI sandbox has no global
        // identity, so pass identity inline on this commit command.
        let content = "---\nsession: t\nagent_doc_format: template\n---\n\nv2\n";
        std::fs::write(subwt.join("doc.md"), content).unwrap();
        run_git(&subwt, &["add", "doc.md"]);
        run_git(
            &subwt,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test User",
                "commit",
                "-m",
                "s2",
                "--no-verify",
            ],
        );
        // Document itself is clean: snapshot == HEAD == working.
        let doc = subwt.join("doc.md");
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::NestedParentPointerStale
        );
        let cmd = closeout_recovery_command_for_file(
            &doc,
            CloseoutRecoveryState::NestedParentPointerStale,
        )
        .unwrap();
        assert!(cmd.contains("agent-doc commit"), "{cmd}");
    }

    #[test]
    fn apply_recovery_clean_is_nothing_to_do() {
        let (_dir, doc) = setup_git_project_with_doc("---\nsession: test\n---\n\nHi\n");
        assert_eq!(
            apply_closeout_recovery(&doc).unwrap(),
            RecoveryApplication::NothingToDo
        );
    }

    #[test]
    fn apply_recovery_cancels_open_empty_preflight() {
        // `#recursive-repair-apply`: the safe action for an empty probe cycle is to
        // abandon it — exactly the churn the diagnostic-preflight bug produces.
        let base = "---\nsession: test\n---\n\nHi\n";
        let (_dir, doc) = setup_git_project_with_doc(base);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        match apply_closeout_recovery(&doc).unwrap() {
            RecoveryApplication::Applied { state, .. } => {
                assert_eq!(state, CloseoutRecoveryState::OpenEmptyPreflight);
            }
            other => panic!("expected Applied for empty preflight, got {other:?}"),
        }
        // The cycle is now abandoned, so re-classification is Clean.
        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::Clean
        );
    }

    #[test]
    fn apply_recovery_commits_queue_metadata_drift_when_no_live_continuation() {
        // `#recovery-drift-authoritative-side`: with no live HEAD continuation at
        // risk, queue metadata drift is now auto-committed (local authoritative).
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: x — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let snapshot = head.replace("- do [#a]\n", "- do [#a]\n- do [#b]\n");
        let (dir, doc) = setup_git_project_with_doc(head);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(head), Some(head)).unwrap();
        // The snapshot AND the visible file carry the drift; only HEAD is behind.
        std::fs::write(&doc, &snapshot).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&snapshot),
            Some(&snapshot),
        )
        .unwrap();
        match apply_closeout_recovery(&doc).unwrap() {
            RecoveryApplication::Applied { state, .. } => {
                assert_eq!(state, CloseoutRecoveryState::QueueMetadataDrift);
            }
            other => panic!("expected Applied for queue metadata drift, got {other:?}"),
        }
        // HEAD now carries the committed queue item, and re-classification is Clean.
        assert!(
            agent_doc_git_io::revision::show_head(&doc)
                .unwrap()
                .unwrap()
                .contains("- do [#b]")
        );
        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::Clean
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("closeout_recovery_mutation")
                && log.contains("reason=commit_queue_metadata_drift"),
            "queue metadata commit must go through the shared recovery mutation primitive:\n{log}"
        );
    }

    #[test]
    fn apply_recovery_restores_from_head_when_local_drops_live_continuation() {
        // The live bug: HEAD has an active go-mode `queue_active` continuation; a
        // spurious local snapshot drift flipped it to `false`. Auto-apply must
        // restore from HEAD (not commit the snapshot), preserving the live queue.
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: x — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n",
        );
        let snapshot = head.replace("queue_active: true", "queue_active: false");
        let (dir, doc) = setup_git_project_with_doc(head);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(head), Some(head)).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&snapshot),
            Some(&snapshot),
        )
        .unwrap();
        match apply_closeout_recovery(&doc).unwrap() {
            RecoveryApplication::Applied { state, action } => {
                assert_eq!(state, CloseoutRecoveryState::QueueMetadataDrift);
                assert!(action.contains("restored"), "{action}");
            }
            other => panic!("expected Applied (restore) for dropped continuation, got {other:?}"),
        }
        // The visible file + sidecars are restored to HEAD's live queue, so the
        // continuation survives and re-classification is Clean.
        let restored = std::fs::read_to_string(&doc).unwrap();
        assert!(restored.contains("queue_active: true"), "{restored}");
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .unwrap(),
            restored
        );
        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::Clean
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("closeout_recovery_mutation")
                && log.contains("reason=restore_head_metadata"),
            "restore-from-HEAD recovery must go through the shared recovery mutation primitive:\n{log}"
        );
    }

    #[test]
    fn apply_recovery_commits_recovery_projection_visible_drift_through_mutation() {
        // `#smrecoverymutate`: reset-from-visible rebuilds snapshot/CRDT through
        // the shared mutation primitive before committing accepted metadata drift.
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: x — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let visible = head.replace("- do [#a]\n", "- do [#a]\n- do [#b]\n");
        let (dir, doc) = setup_git_project_with_doc(head);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(head), Some(head)).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            head,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(head),
            Some(head),
        )
        .unwrap();
        std::fs::write(&doc, &visible).unwrap();
        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::RecoveryProjectionVisibleDrift
        );

        match apply_closeout_recovery(&doc).unwrap() {
            RecoveryApplication::Applied { state, .. } => {
                assert_eq!(state, CloseoutRecoveryState::RecoveryProjectionVisibleDrift);
            }
            other => panic!("expected Applied for sidecar-visible drift, got {other:?}"),
        }

        let working = std::fs::read_to_string(&doc).unwrap();
        assert!(working.contains("- do [#b]"), "{working}");
        let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(snapshot.contains("- do [#b]"), "{snapshot}");
        assert!(
            agent_doc_git_io::revision::show_head(&doc)
                .unwrap()
                .unwrap()
                .contains("- do [#b]")
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("closeout_recovery_mutation") && log.contains("reason=reset_from_visible"),
            "reset-from-visible recovery must go through the shared recovery mutation primitive:\n{log}"
        );
    }

    #[test]
    fn apply_recovery_withholds_queue_metadata_drift_when_live_heads_diverge() {
        // Both sides carry distinct live continuation heads with no consuming
        // response → the direction is genuinely ambiguous → fail closed.
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: x — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let snapshot = head.replace("- do [#a]", "- do [#z]");
        let (_dir, doc) = setup_git_project_with_doc(head);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(head), Some(head)).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&snapshot),
            Some(&snapshot),
        )
        .unwrap();
        match apply_closeout_recovery(&doc).unwrap() {
            RecoveryApplication::NotApplied {
                state, recommended, ..
            } => {
                assert_eq!(state, CloseoutRecoveryState::QueueMetadataDrift);
                assert!(recommended.contains("agent-doc commit"), "{recommended}");
            }
            other => panic!("expected NotApplied for ambiguous queue drift, got {other:?}"),
        }
    }

    #[test]
    fn classify_recovery_missing_response_body_for_stuck_cycle() {
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        let response = "### Re: hello — gpt-5\n\nCaptured but never committed.\n";
        let (_dir, doc) = setup_git_project_with_doc(base);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        agent_doc_capture_io::capture_response(&doc, response).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(base),
            Some(base),
        )
        .unwrap();
        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::MissingResponseBody
        );
    }

    #[test]
    fn classify_recovery_clean_when_response_committed_in_head() {
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        let response = "### Re: hello — gpt-5\n\nCommitted response.\n";
        let full_doc = format!("{base}\n{response}");
        let (dir, doc) = setup_git_project_with_doc(base);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        agent_doc_capture_io::capture_response(&doc, response).unwrap();
        std::fs::write(&doc, &full_doc).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &full_doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        run_git(dir.path(), &["add", "doc.md"]);
        run_git(dir.path(), &["commit", "-m", "response", "--no-verify"]);
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&full_doc),
            Some(&full_doc),
        )
        .unwrap();
        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::Clean
        );
    }

    #[test]
    fn classify_recovery_boundary_only_drift_for_answered_prompt_prefix() {
        // `#recursive-repair-state-drift`: snapshot differs from HEAD only by an
        // answered-prompt-prefix (`❯ do …` vs bare `do …` above a real `### Re:`).
        // `verify_snapshot_committed` normalizes only transient markers, so this
        // still trips the snapshot-vs-HEAD guard, but the fuller artifact
        // normalization proves it is safe metadata-only drift → BoundaryOnlyDrift
        // → single `agent-doc commit` recovery (never `write --commit`).
        let head = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Please rerun the deploy check.\n",
            "### Re: deploy check — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n",
        );
        let snapshot = head.replace("Please rerun the", "❯ Please rerun the");
        // `setup_git_project_with_doc` already commits `head` to HEAD.
        let (_dir, doc) = setup_git_project_with_doc(head);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(head), Some(head)).unwrap();
        // Snapshot carries the un-canonicalized prompt prefix; HEAD has the bare
        // form. Artifact normalization makes them equal; transient does not.
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&snapshot),
            Some(&snapshot),
        )
        .unwrap();
        assert_ne!(
            agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(&snapshot),
            agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(head),
            "test precondition: transient normalization must still differ"
        );
        assert_eq!(
            classify_closeout_recovery_state_for_file(&doc),
            CloseoutRecoveryState::BoundaryOnlyDrift
        );
        let cmd =
            closeout_recovery_command_for_file(&doc, CloseoutRecoveryState::BoundaryOnlyDrift)
                .unwrap();
        assert!(cmd.contains("agent-doc commit"), "{cmd}");
        assert!(!cmd.contains("write --commit"), "{cmd}");
    }

    #[test]
    fn stuck_captured_cycle_ignores_committed_template_patch_body_in_head() {
        let base = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Hello\n",
            "<!-- /agent:exchange -->\n",
        );
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: hello — gpt-5\n\n",
            "Committed through template patching.\n",
            "<!-- /patch:exchange -->\n",
        );
        let full_doc = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Hello\n",
            "### Re: hello — gpt-5\n\n",
            "Committed through template patching.\n",
            "<!-- /agent:exchange -->\n",
        );
        let (dir, doc) = setup_git_project_with_doc(base);

        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        agent_doc_capture_io::capture_response(&doc, response).unwrap();
        std::fs::write(&doc, full_doc).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            full_doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        run_git(dir.path(), &["add", "doc.md"]);
        run_git(dir.path(), &["commit", "-m", "response", "--no-verify"]);
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(full_doc),
            Some(full_doc),
        )
        .unwrap();

        assert!(
            stuck_captured_cycle(&doc).is_none(),
            "template patch wrappers are not expected in HEAD after materialization"
        );
    }

    #[test]
    fn stuck_captured_cycle_ignores_committed_cycle_when_response_is_in_compact_archive() {
        let base = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Older response.\n",
            "<!-- /agent:exchange -->\n",
        );
        let response = "### Re: compacted — gpt-5\n\nArchived response body.\n";
        let (dir, doc) = setup_git_project_with_doc(base);
        let archive_dir = dir.path().join(".agent-doc/archives");
        std::fs::create_dir_all(&archive_dir).unwrap();
        let archive_path = archive_dir.join("doc-20260527-000000.md");
        std::fs::write(
            &archive_path,
            format!(
                "---\narchived_from: compact\ncomponent: exchange\ndocument: doc.md\n---\n\n{base}\n{response}"
            ),
        )
        .unwrap();
        let compacted = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n*Compacted. Content archived to `{}`*\n<!-- /agent:exchange -->\n",
            archive_path.display()
        );

        let state =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        agent_doc_capture_io::capture_response(&doc, response).unwrap();
        std::fs::write(&doc, &compacted).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &compacted,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        run_git(dir.path(), &["add", "doc.md"]);
        run_git(dir.path(), &["commit", "-m", "compact", "--no-verify"]);
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&compacted),
            Some(&compacted),
        )
        .unwrap();

        assert!(
            stuck_captured_cycle(&doc).is_none(),
            "cycle {} should not warn when the captured response is materialized in the compact archive",
            state.cycle_id
        );
    }

    #[test]
    fn reconcile_compacted_committed_capture_discards_and_survives_archive_gc() {
        // #stuck-capture-compact-false-positive: reconciliation marks the capture
        // Discarded once the response is proven in the compact archive, so the
        // false-positive stuck warning cannot resurface even if the archive is
        // later garbage-collected.
        let base = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Older response.\n",
            "<!-- /agent:exchange -->\n",
        );
        let response = "### Re: compacted — gpt-5\n\nArchived response body.\n";
        let (dir, doc) = setup_git_project_with_doc(base);
        let archive_dir = dir.path().join(".agent-doc/archives");
        std::fs::create_dir_all(&archive_dir).unwrap();
        let archive_path = archive_dir.join("doc-20260527-000000.md");
        std::fs::write(
            &archive_path,
            format!(
                "---\narchived_from: compact\ncomponent: exchange\ndocument: doc.md\n---\n\n{base}\n{response}"
            ),
        )
        .unwrap();
        let compacted = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n*Compacted. Content archived to `{}`*\n<!-- /agent:exchange -->\n",
            archive_path.display()
        );

        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        agent_doc_capture_io::capture_response(&doc, response).unwrap();
        std::fs::write(&doc, &compacted).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &compacted,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        run_git(dir.path(), &["add", "doc.md"]);
        run_git(dir.path(), &["commit", "-m", "compact", "--no-verify"]);
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&compacted),
            Some(&compacted),
        )
        .unwrap();

        // Reconcile once: the terminal capture projection is retired from the
        // state backbone rather than kept as an active compatibility record.
        assert!(
            reconcile_compacted_committed_capture(&doc).unwrap(),
            "expected reconciliation to settle the compacted committed capture"
        );
        assert!(
            agent_doc_capture_io::load_active(&doc).unwrap().is_none(),
            "a reconciled terminal capture must not remain active"
        );

        // A second pass is a no-op (already discarded).
        assert!(
            !reconcile_compacted_committed_capture(&doc).unwrap(),
            "reconciliation should be idempotent once the capture is discarded"
        );

        // Durability: even after the archive is GC'd, the discarded capture must
        // not resurface as a stuck-capture false positive.
        std::fs::remove_file(&archive_path).unwrap();
        assert!(
            stuck_captured_cycle(&doc).is_none(),
            "a reconciled (discarded) capture must not flag stuck after the archive is removed"
        );
    }
}
