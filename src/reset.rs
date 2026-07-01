//! # Module: reset
//!
//! ## Spec
//! - Resets a session document to a clean state by clearing the agent conversation resume pointer and deleting or rebuilding associated state files.
//! - `run(file, from_current, preserve_session, force_disk)` performs three operations in sequence:
//!   1. Reads YAML frontmatter, sets `resume` to `None` (clears the conversation ID), rewrites the frontmatter while preserving all other fields and the document body.
//!   2. Deletes the snapshot file via `snapshot::delete`, or with `--from-current` saves the current markdown as the snapshot.
//!   3. Deletes the CRDT state file via `snapshot::delete_crdt`, or with `--from-current` rebuilds it from the current markdown.
//! - `--from-current --preserve-session` is non-destructive: it leaves the
//!   markdown, resume pointer, cycle state, and capture chain untouched while
//!   refreshing snapshot/CRDT/baseline sidecars from the visible file.
//! - The `session` frontmatter field (routing key) is intentionally preserved; only `resume` (conversation continuity pointer) is cleared.
//! - After reset, the next `agent-doc submit` or `agent-doc stream` starts a fresh agent conversation.
//!
//! ## Agentic Contracts
//! - `run(file, from_current, preserve_session, force_disk)` — returns `Err` if the file is missing or any I/O operation fails; returns `Ok(())` on success with a confirmation message on stderr.
//! - Callers may rely on snapshot and CRDT state being absent after a default reset.
//! - Callers may rely on snapshot and CRDT state matching the visible markdown after `--from-current`.
//! - Callers may rely on `--from-current --preserve-session` not rewriting the
//!   document or clearing `resume`.
//! - Session identity (`session` field) is unaffected; document routing continues to work after reset.
//!
//! ## Evals
//! - file_not_found: missing path → Err containing "file not found"
//! - clears_resume: document with `resume: abc` → after reset, frontmatter has no `resume` field
//! - preserves_session: document with `session: xyz` → after reset, `session` field unchanged
//! - snapshot_deleted: snapshot exists before reset → absent after successful run
//! - crdt_deleted: CRDT state exists before reset → absent after successful run
//! - from_current_rebuilds_snapshot_and_crdt: `--from-current` saves current markdown to both state sidecars
//! - preserve_session_from_current_keeps_document_and_sidecars: `--from-current
//!   --preserve-session` leaves the document/capture/cycle files intact and
//!   refreshes snapshot/CRDT/baseline

use anyhow::Result;
use std::io::Write;
use std::path::Path;

use agent_doc_frontmatter::frontmatter;
use agent_doc_orchestration::snapshot;

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

    let content = std::fs::read_to_string(file)?;
    if preserve_session {
        rebuild_sidecars_from_current(file, &content, true)?;
        eprintln!(
            "Reset sidecars for {} from current file while preserving session state",
            file.display()
        );
        return Ok(());
    }

    // Clear agent conversation ID (resume) — keep session (routing key)
    let (mut fm, body) = frontmatter::parse(&content)?;
    fm.resume = None;
    let updated = frontmatter::write(&fm, body)?;
    if force_disk {
        agent_doc_orchestration::write::atomic_write_pub(file, &updated)?;
        agent_doc_orchestration::ops_log::log_op(
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
        agent_doc_orchestration::write::converge_or_disk_write(
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
        rebuild_sidecars_from_current(file, &updated_content, false)?;
        eprintln!(
            "Reset session for {} and rebuilt snapshot/CRDT from current file",
            file.display()
        );
    } else {
        // Delete snapshot
        snapshot::delete(file)?;

        // Delete CRDT state (stream mode)
        snapshot::delete_crdt(file)?;

        eprintln!("Reset session for {}", file.display());
    }
    Ok(())
}

fn rebuild_sidecars_from_current(file: &Path, content: &str, save_baseline: bool) -> Result<()> {
    snapshot::save(file, content)?;
    let crdt = agent_doc_merge::crdt::CrdtDoc::from_text(content).encode_state();
    snapshot::save_document_crdt(file, &crdt, content)?;
    if save_baseline {
        save_baseline_from_current(file, content)?;
    }
    // Clear the crash-durability queue journal (mirrors the commit-time clear in
    // `git::commit`). The rebuilt snapshot/baseline IS the new durable queue
    // baseline for the current file, so any pre-reset journal window (queue heads
    // recorded while older prompts were live) is superseded. Without this, a
    // compaction or reset that removed answered/compacted heads would leave those
    // heads in the journal, and the next `start` would call
    // `queue_journal::replay_missing` and resurrect them over the current queue.
    agent_doc_orchestration::queue_journal::clear(file);
    Ok(())
}

fn save_baseline_from_current(file: &Path, content: &str) -> Result<()> {
    let baseline_path = agent_doc_fs::baseline_path_for(file)?;
    if let Some(parent) = baseline_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp =
        tempfile::NamedTempFile::new_in(baseline_path.parent().unwrap_or_else(|| Path::new(".")))?;
    tmp.write_all(content.as_bytes())?;
    tmp.persist(&baseline_path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn assert_overlay_projects_to(doc: &Path, expected: &str) {
        let overlay = snapshot::load_overlay_crdt(doc)
            .unwrap()
            .expect("overlay sidecar present");
        let projected = agent_doc_markdown_ast::crdt::OverlayCrdtDoc::decode_state(&overlay)
            .unwrap()
            .to_markdown()
            .unwrap();
        assert_eq!(
            projected, expected,
            "overlay sidecar must project the current visible document"
        );
    }

    #[test]
    fn from_current_rebuilds_snapshot_and_crdt() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/crdt")).unwrap();
        let doc = dir.path().join("session.md");
        let current = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nresume: old\n---\n\nBody\n";
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, "stale snapshot").unwrap();
        snapshot::save_crdt(&doc, b"stale crdt").unwrap();

        run(&doc, true, false, true).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(!updated.contains("resume: old"));
        assert_eq!(snapshot::load(&doc).unwrap().unwrap(), updated);
        let crdt_state = snapshot::load_crdt(&doc).unwrap().unwrap();
        let crdt_text = agent_doc_merge::crdt::CrdtDoc::decode_state(&crdt_state)
            .unwrap()
            .to_text();
        assert_eq!(crdt_text, updated);
        assert_overlay_projects_to(&doc, &updated);
    }

    #[test]
    fn from_current_force_disk_records_audited_no_listener_write() {
        // `#evmh` / realtime cutover: no-listener reset writes must be explicit.
        // `--force-disk` preserves the headless recovery path while recording the
        // audited bypass in ops.log.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/crdt")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("session.md");
        let current = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nresume: old\n---\n\nBody\n";
        std::fs::write(&doc, current).unwrap();

        run(&doc, true, false, true).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("resume: old"),
            "resume must be cleared on disk after reset"
        );
        assert_eq!(
            snapshot::load(&doc).unwrap().unwrap(),
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
    }

    #[test]
    fn preserve_session_from_current_keeps_document_and_sidecars() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/crdt")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/baselines")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/state/cycles")).unwrap();
        let doc = dir.path().join("session.md");
        let current = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nresume: keep-me\n---\n\nBody\n";
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, "stale snapshot").unwrap();
        snapshot::save_crdt(&doc, b"stale crdt").unwrap();
        std::fs::write(
            agent_doc_fs::baseline_path_for(&doc).unwrap(),
            "stale baseline",
        )
        .unwrap();

        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let cycle_path = dir
            .path()
            .join(".agent-doc/state/cycles")
            .join(format!("{hash}.json"));
        let cycle_state = r#"{"cycle_id":"cycle-keep","phase":"preflight_started"}"#;
        std::fs::write(&cycle_path, cycle_state).unwrap();
        let capture_dir = dir.path().join(".agent-doc/captures").join(&hash);
        std::fs::create_dir_all(&capture_dir).unwrap();
        let capture_path = capture_dir.join("capture-keep.json");
        let capture_state = r#"{"capture_id":"capture-keep","state":"committed"}"#;
        std::fs::write(&capture_path, capture_state).unwrap();

        run(&doc, true, true, false).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(updated, current);
        assert!(updated.contains("resume: keep-me"));
        assert_eq!(snapshot::load(&doc).unwrap().unwrap(), current);
        assert_eq!(
            std::fs::read_to_string(agent_doc_fs::baseline_path_for(&doc).unwrap()).unwrap(),
            current
        );
        let crdt_state = snapshot::load_crdt(&doc).unwrap().unwrap();
        let crdt_text = agent_doc_merge::crdt::CrdtDoc::decode_state(&crdt_state)
            .unwrap()
            .to_text();
        assert_eq!(crdt_text, current);
        assert_overlay_projects_to(&doc, current);
        assert_eq!(std::fs::read_to_string(&cycle_path).unwrap(), cycle_state);
        assert_eq!(
            std::fs::read_to_string(&capture_path).unwrap(),
            capture_state
        );
    }

    #[test]
    fn preserve_session_clears_stale_queue_journal_so_compacted_heads_do_not_resurface() {
        // The crash-durability queue journal records live heads while they are
        // pending. A compaction/answer removes heads B,C from the queue, then
        // `reset --from-current --preserve-session` rebuilds the sidecars. Without
        // clearing the journal, the next `start` would call
        // `queue_journal::replay_missing` and resurrect B,C over the current file.
        use agent_doc_orchestration::queue_journal;

        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/crdt")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/baselines")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/state/cycles")).unwrap();
        let doc = dir.path().join("session.md");

        // Earlier state: queue had heads A, B, C live — recorded in the journal.
        let earlier = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nresume: keep-me\n---\n\n## Queue\n\n<!-- agent:queue auto -->\n- do [#a]\n- do [#b]\n- do [#c]\n<!-- /agent:queue -->\n";
        std::fs::write(&doc, earlier).unwrap();
        queue_journal::record(&doc, earlier).unwrap();
        // Sanity: B,C are journaled and would replay if absent.
        let only_a = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nresume: keep-me\n---\n\n## Queue\n\n<!-- agent:queue auto -->\n- do [#a]\n<!-- /agent:queue -->\n";
        let pre_fix_missing = queue_journal::replay_missing(&doc, only_a);
        assert_eq!(
            pre_fix_missing.len(),
            2,
            "before the reset, B,C are journaled and would resurface: {pre_fix_missing:?}"
        );

        // Current file (post-answer/compaction): queue has ONLY head A.
        std::fs::write(&doc, only_a).unwrap();
        snapshot::save(&doc, "stale snapshot").unwrap();
        snapshot::save_crdt(&doc, b"stale crdt").unwrap();
        std::fs::write(
            agent_doc_fs::baseline_path_for(&doc).unwrap(),
            "stale baseline",
        )
        .unwrap();

        // Preserved continuity: cycle state + capture.
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let cycle_path = dir
            .path()
            .join(".agent-doc/state/cycles")
            .join(format!("{hash}.json"));
        let cycle_state = r#"{"cycle_id":"cycle-keep","phase":"preflight_started"}"#;
        std::fs::write(&cycle_path, cycle_state).unwrap();
        let capture_dir = dir.path().join(".agent-doc/captures").join(&hash);
        std::fs::create_dir_all(&capture_dir).unwrap();
        let capture_path = capture_dir.join("capture-keep.json");
        let capture_state = r#"{"capture_id":"capture-keep","state":"committed"}"#;
        std::fs::write(&capture_path, capture_state).unwrap();

        run(&doc, true, true, false).unwrap();

        // The journal is cleared: B,C no longer replay over the current file.
        let missing = queue_journal::replay_missing(&doc, only_a);
        assert!(
            missing.is_empty(),
            "reset --from-current must clear the journal so compacted heads do not resurface: {missing:?}"
        );

        // Preserved continuity is unchanged.
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), only_a);
        assert!(
            std::fs::read_to_string(&doc)
                .unwrap()
                .contains("resume: keep-me")
        );
        assert_eq!(snapshot::load(&doc).unwrap().unwrap(), only_a);
        assert_eq!(
            std::fs::read_to_string(agent_doc_fs::baseline_path_for(&doc).unwrap()).unwrap(),
            only_a
        );
        assert_eq!(std::fs::read_to_string(&cycle_path).unwrap(), cycle_state);
        assert_eq!(
            std::fs::read_to_string(&capture_path).unwrap(),
            capture_state
        );
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
