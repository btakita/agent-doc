//! # Module: reset
//!
//! ## Spec
//! - Resets a session document to a clean state by clearing the agent conversation resume pointer and deleting or rebuilding associated state files.
//! - `run(file, from_current, preserve_session, force_disk)` performs three operations in sequence:
//!   1. Reads YAML frontmatter, sets `resume` to `None` (clears the conversation ID), rewrites the frontmatter while preserving all other fields and the document body.
//!   2. Clears the durable baseline, or with `--from-current` checkpoints the current markdown.
//!   3. With `--from-current`, refreshes the cold CRDT restart projection in `state.db`.
//! - `--from-current --preserve-session` is non-destructive: it leaves the
//!   markdown, resume pointer, cycle state, and captured response payload/state
//!   untouched while refreshing snapshot/CRDT/baseline recovery projections from the visible
//!   file. If a response capture is active, its replay baseline hashes are
//!   explicitly rebased to that operator-approved visible state.
//! - The `session` frontmatter field (routing key) is intentionally preserved; only `resume` (conversation continuity pointer) is cleared.
//! - After reset, the next `agent-doc submit` or `agent-doc stream` starts a fresh agent conversation.
//!
//! ## Agentic Contracts
//! - `run(file, from_current, preserve_session, force_disk)` — returns `Err` if the file is missing or any I/O operation fails; returns `Ok(())` on success with a confirmation message on stderr.
//! - Callers may rely on the baseline being cleared after a default reset.
//! - Callers may rely on the baseline and cold restart projection matching the visible markdown after `--from-current`.
//! - Callers may rely on `--from-current --preserve-session` not rewriting the
//!   document or clearing `resume`.
//! - Session identity (`session` field) is unaffected; document routing continues to work after reset.
//!
//! ## Evals
//! - file_not_found: missing path → Err containing "file not found"
//! - clears_resume: document with `resume: abc` → after reset, frontmatter has no `resume` field
//! - preserves_session: document with `session: xyz` → after reset, `session` field unchanged
//! - baseline_cleared: baseline exists before reset → cleared after successful run
//! - from_current_rebuilds_recovery_projections: `--from-current` saves current markdown to the recovery projections
//! - preserve_session_from_current_keeps_document_and_recovery_projections: `--from-current
//!   --preserve-session` leaves the document/capture/cycle files intact and
//!   refreshes snapshot/CRDT/baseline

use anyhow::Result;
use std::path::Path;

use agent_doc_frontmatter::frontmatter;

pub fn run(
    file: &Path,
    from_current: bool,
    preserve_session: bool,
    force_disk: bool,
) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    if preserve_session && !from_current {
        anyhow::bail!("--preserve-session requires --from-current");
    }

    let mut content = if force_disk {
        agent_doc_document_realtime_io::resolve_disk_current_document_content(
            file,
            "reset_command_document",
        )?
    } else {
        agent_doc_document_realtime_io::try_resolve_current_document_content(
            file,
            "reset_command_document",
        )?
    };
    if force_disk {
        // An explicit disk-authority reset supersedes every retained ordinary
        // document-write lineage for this file. Leave external disk/editor
        // decision state to the force-disk write path, but prevent an older
        // whole-document payload from replaying over the selected disk cut.
        agent_doc_document_realtime_io::clear_all_deferred_document_write_intents(
            file,
            "reset_force_disk_override",
        )?;
    }
    if preserve_session {
        if !force_disk {
            let disk_content =
                agent_doc_document_realtime_io::resolve_disk_current_document_content(
                    file,
                    "reset_command_disk_compare",
                )?;
            if disk_content != content {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "reset_preserve_session_authority_disk_divergence file={} authority_hash={} disk_hash={} recovery=automatic_native_editor_save operator_action=none",
                        file.display(),
                        agent_doc_hash::content_hash(&content),
                        agent_doc_hash::content_hash(&disk_content),
                    ),
                );
                if !agent_doc_document_realtime_io::settle_live_editor_projection_through_authority(
                    file,
                    "reset_preserve_session_editor_authority_convergence",
                )? {
                    anyhow::bail!(
                        "reset --from-current --preserve-session is waiting for automatic editor-authority convergence for {} (authority_hash={}, disk_hash={}); the exact editor revision remains authoritative and retained, and no operator save, reload, or retry is required",
                        file.display(),
                        agent_doc_hash::content_hash(&content),
                        agent_doc_hash::content_hash(&disk_content),
                    );
                }
                let settled_authority =
                    agent_doc_document_realtime_io::try_resolve_current_document_content(
                        file,
                        "reset_preserve_session_settled_authority",
                    )?;
                let settled_disk =
                    agent_doc_document_realtime_io::resolve_disk_current_document_content(
                        file,
                        "reset_preserve_session_settled_disk",
                    )?;
                anyhow::ensure!(
                    settled_authority == settled_disk,
                    "reset --from-current --preserve-session automatic editor save for {} returned without exact authority/disk convergence (authority_hash={}, disk_hash={}); the convergence effect remains controller-owned and no operator save, reload, or retry is required",
                    file.display(),
                    agent_doc_hash::content_hash(&settled_authority),
                    agent_doc_hash::content_hash(&settled_disk),
                );
                content = settled_authority;
            }
        }
        rebuild_recovery_projections_from_current(file, &content)?;
        rebase_active_capture_after_preserve_session_reset(file, &content)?;
        if force_disk
            && let Some(outcome) =
                agent_doc_crdt_relay_io::apply_disk_change_for_file(file, &content)?
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "reset_preserve_session_force_disk_reconciled_canonical file={} outcome={outcome:?}",
                    file.display(),
                ),
            );
        }
        eprintln!(
            "Reset recovery projections for {} from current file while preserving session state",
            file.display()
        );
        return Ok(());
    }

    // Clear agent conversation ID (resume) — keep session (routing key)
    let (mut fm, body) = frontmatter::parse(&content)?;
    fm.resume = None;
    let updated = frontmatter::write(&fm, body)?;
    if force_disk {
        agent_doc_document_realtime_io::atomic_write_force_disk_through_authority(file, &updated)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "reset_resume_clear_writeback file={} transport=disk_force reason=force_disk len={} hash={}",
                file.display(),
                updated.len(),
                agent_doc_hash::content_hash(&updated)
            ),
        );
    } else {
        // `#evmh`: route the resume-clear write through the listener-guarded
        // converge seam so a live JB editor buffer stays in sync with the change
        // instead of diverging from a bare disk write and raising a File Cache
        // Conflict. Without `--force-disk`, no-listener reset fails closed.
        agent_doc_write_converge_io::converge_or_disk_write(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            &content,
            &updated,
            "reset_resume_clear",
        )?;
    }
    // Use the intended content directly: an editor convergence may apply
    // asynchronously, so re-reading disk could race the buffer. `updated` is the
    // authoritative post-reset document either way.
    let updated_content = updated;

    if from_current {
        rebuild_recovery_projections_from_current(file, &updated_content)?;
        eprintln!(
            "Reset session for {} and rebuilt document baseline from current file",
            file.display()
        );
    } else {
        // Delete snapshot
        agent_doc_snapshot_io::delete_recovery_projection_and_clear_baseline(file)?;

        eprintln!("Reset session for {}", file.display());
    }
    Ok(())
}

fn rebuild_recovery_projections_from_current(file: &Path, content: &str) -> Result<()> {
    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        content,
        agent_doc_ops_log_io::log_op,
    )?;
    Ok(())
}

fn rebase_active_capture_after_preserve_session_reset(file: &Path, content: &str) -> Result<()> {
    let Some(capture) = agent_doc_capture_io::load_active(file)? else {
        return Ok(());
    };
    let file_hash = agent_doc_capture_io::replay_file_hash(content);
    let snapshot_hash = agent_doc_hash::content_hash(content);
    agent_doc_capture_io::refresh_replay_baseline_for_recovery(
        file,
        &capture,
        &file_hash,
        Some(&snapshot_hash),
        Some(content),
        "capture_replay_baseline_rebased_after_preserve_session_reset",
        "explicit preserve-session reset accepted the current visible document",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn assert_legacy_recovery_projection_unchanged(doc: &Path, expected: &str) {
        let projection = agent_doc_snapshot_io::load_crdt_recovery_projection(doc)
            .unwrap()
            .expect("cold recovery projection present");
        let projected = agent_doc_merge::crdt::CrdtDoc::decode_state(&projection.projection)
            .unwrap()
            .to_text();
        assert_eq!(
            projected, expected,
            "reset must not rewrite the retired CRDT recovery sidecar"
        );
    }

    #[test]
    fn from_current_does_not_rewrite_legacy_recovery_projection() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let current = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nresume: old\n---\n\nBody\n";
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "stale snapshot",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let stale = agent_doc_merge::crdt::CrdtDoc::from_text("stale crdt").encode_state();
        agent_doc_snapshot_io::checkpoint_crdt_recovery_projection(&doc, &stale, "test:stale")
            .unwrap();

        run(&doc, true, false, true).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(!updated.contains("resume: old"));
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .unwrap(),
            updated
        );
        assert_legacy_recovery_projection_unchanged(&doc, "stale crdt");
    }

    #[test]
    fn from_current_force_disk_records_audited_no_listener_write() {
        // `#evmh` / realtime cutover: no-listener reset writes must be explicit.
        // `--force-disk` preserves the headless recovery path while recording the
        // audited bypass in ops.log.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("session.md");
        let current = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nresume: old\n---\n\nBody\n";
        std::fs::write(&doc, current).unwrap();
        agent_doc_document_realtime_io::retain_deferred_document_write_target(
            &doc,
            current,
            current,
            "legacy_delivery_failed_to_all",
            agent_doc_state_backbone::DocumentWriteDeferredReason::EditorOwnerWithoutRegisteredReplica,
        )
        .unwrap();
        assert!(!agent_doc_document_realtime_io::pending_document_write_journal(&doc).is_empty());

        run(&doc, true, false, true).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("resume: old"),
            "resume must be cleared on disk after reset"
        );
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .unwrap(),
            updated,
            "snapshot must match the resume-cleared document"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("reset_resume_clear_writeback")
                && ops_log.contains("transport=disk_force")
                && ops_log.contains("reason=force_disk"),
            "reset must record the explicit no-listener force-disk write, got:\n{ops_log}"
        );
        assert!(
            agent_doc_document_realtime_io::pending_document_write_journal(&doc).is_empty(),
            "explicit force-disk reset must retire the superseded retained lineage",
        );
    }

    #[test]
    fn preserve_session_from_current_keeps_document_and_refreshes_ledger() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let current = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nresume: keep-me\n---\n\nBody\n";
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "stale snapshot",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let stale = agent_doc_merge::crdt::CrdtDoc::from_text("stale crdt").encode_state();
        agent_doc_snapshot_io::checkpoint_crdt_recovery_projection(&doc, &stale, "test:stale")
            .unwrap();

        run(&doc, true, true, false).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(updated, current);
        assert!(updated.contains("resume: keep-me"));
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .unwrap(),
            current
        );
        assert_legacy_recovery_projection_unchanged(&doc, "stale crdt");
    }

    #[test]
    fn preserve_session_from_current_rebases_active_capture_for_replay() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let captured_baseline = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n<!-- agent:exchange patch=append -->\n❯ original prompt\n<!-- /agent:exchange -->\n";
        let accepted_current = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n<!-- agent:exchange patch=append -->\n❯ original prompt\n\n❯ later operator prompt\n<!-- /agent:exchange -->\n";
        let response = "### Re: original prompt — gpt-5\n\nRetained response.";
        std::fs::write(&doc, captured_baseline).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            captured_baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(
            &doc,
            Some(captured_baseline),
            Some(captured_baseline),
        )
        .unwrap();
        let captured = agent_doc_capture_io::capture_response_with_current_content(
            &doc,
            response,
            captured_baseline,
        )
        .unwrap();

        std::fs::write(&doc, accepted_current).unwrap();
        run(&doc, true, true, false).unwrap();

        let rebased = agent_doc_capture_io::load_active(&doc)
            .unwrap()
            .expect("active capture remains available");
        assert_eq!(rebased.capture_id, captured.capture_id);
        assert_eq!(rebased.response_body, captured.response_body);
        assert_eq!(rebased.state, captured.state);
        assert_eq!(
            rebased.file_hash.as_deref(),
            Some(agent_doc_capture_io::replay_file_hash(accepted_current).as_str())
        );
        assert_eq!(
            rebased.snapshot_hash.as_deref(),
            Some(agent_doc_hash::content_hash(accepted_current).as_str())
        );
        agent_doc_capture_io::validate_replay(&doc, &rebased)
            .expect("the printed preserve-session recovery must make replay admissible");
    }

    #[test]
    fn preserve_session_from_current_retains_stale_live_authority_for_automatic_save() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("session.md");
        let expanded = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "resume: keep-me\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: old topic - gpt-5\n\n",
            "Old response that was compacted.\n",
            "<!-- /agent:exchange -->\n",
        );
        let compacted = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "resume: keep-me\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "*Compacted. Content archived to `.agent-doc/archives/session.md`*\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, expanded).unwrap();
        let pid = std::process::id();
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid: pid.into(),
                tag: format!("test-editor-{pid}:{}", doc.display()),
            }]);
        agent_doc_crdt_relay_io::register_replica_for_file(&doc, "intellij:reset-stale")
            .unwrap()
            .expect("test relay should attach");

        std::fs::write(&doc, compacted).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            expanded,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let err = run(&doc, true, true, false).unwrap_err();
        let message = format!("{err:#}");
        assert!(
            message.contains("waiting for automatic editor-authority convergence"),
            "plain preserve reset must retain stale live authority for its save effect: {err:#}"
        );
        assert!(
            message.contains("no operator save, reload, or retry is required"),
            "recovery guidance must never delegate persistence to the operator: {err:#}"
        );
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .unwrap(),
            expanded,
            "failed reset must not adopt the stale live projection or compacted disk"
        );

        run(&doc, true, true, true).unwrap();
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .unwrap(),
            compacted
        );
        let current = agent_doc_crdt_relay_io::current_text_for_file(&doc).unwrap();
        match current {
            agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => {
                assert_eq!(text, compacted);
                assert!(
                    !text.contains("### Re: old topic"),
                    "force-disk reset must remove compacted cells from live canonical text"
                );
            }
            other => {
                panic!("expected live relay current text after force-disk reset, got {other:?}")
            }
        }
    }

    #[test]
    fn default_reset_clears_baseline_without_touching_legacy_recovery_projection() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("session.md");
        let current = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nresume: old\n---\n\nBody\n";
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "stale snapshot",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let stale = agent_doc_merge::crdt::CrdtDoc::from_text("stale crdt").encode_state();
        agent_doc_snapshot_io::checkpoint_crdt_recovery_projection(&doc, &stale, "test:stale")
            .unwrap();

        run(&doc, false, false, true).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(!updated.contains("resume: old"));
        assert!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .is_none()
        );
        assert_legacy_recovery_projection_unchanged(&doc, "stale crdt");
    }

    #[test]
    fn preserve_session_requires_from_current() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "---\nagent_doc_session: test\n---\n\nBody\n").unwrap();

        let err = run(&doc, false, true, false).unwrap_err();

        assert!(
            err.to_string()
                .contains("--preserve-session requires --from-current")
        );
    }
}
