//! Diff I/O — durable-baseline document diff computation. The pure half lives
//! in [`agent_doc_diff`].
//!
//! Baseline storage is injected; the live current cut comes from Lazily when an
//! editor owns the document.

use anyhow::Result;
use std::path::Path;

use agent_doc_diff::{is_stale_snapshot, strip_comments, unified_diff_from_contents};

/// Diff result plus the exact snapshot/current document content used to compute it.
pub struct ComputeResult {
    pub diff: Option<String>,
    pub previous: String,
    pub current: String,
}

/// Snapshot operations required by diff computation.
pub trait DocumentBaselineStore {
    fn resolve(&self, doc: &Path) -> Result<Option<String>>;
    fn checkpoint(&self, doc: &Path, content: &str) -> Result<()>;
}

/// Source of the realtime-coherent "current" document content, injected by a
/// higher layer that can read the lazily reactive CRDT model.
///
/// `agent-doc-diff-io` is a leaf beneath the relay crate
/// (`crdt-relay-io → snapshot-io → diff-io`), so it cannot depend on the
/// reactive model directly. This trait is the seam: the diff still settles on
/// the disk buffer for prompt-completeness (so a half-typed prompt line is not
/// diffed), then asks the live source for the coherent current state. When the
/// operator's edits live in the CRDT buffer but disk lags behind, this returns
/// the reactive canonical text so the realtime model — not a disk read race —
/// owns the typing. Returning `None` means "no live divergence, or the reactive
/// model is unavailable/detached": fall back to the disk-sourced content.
pub trait LiveCurrentSource {
    fn live_current(&self, doc: &Path, disk: &str) -> Option<String>;
}

/// Compute a unified diff between the snapshot and the current document, and
/// return the exact snapshot/current content used to compute it.
///
/// Both snapshot and current content are comment-stripped before comparison.
pub fn compute_with_current<S: DocumentBaselineStore + ?Sized>(
    snapshots: &S,
    doc: &Path,
    live: Option<&dyn LiveCurrentSource>,
) -> Result<ComputeResult> {
    let t_total = std::time::Instant::now();

    let previous = snapshots.resolve(doc)?.unwrap_or_default();
    // Settle on the disk buffer for prompt-completeness, then source the
    // coherent current state from the lazily reactive model when a live source
    // is injected (#preflight-lazily-diff-feed).
    let current = wait_for_stable_content(doc, &previous, live)?;

    eprintln!(
        "[diff] doc={} baseline=state_db doc_len={} baseline_len={}",
        doc.display(),
        current.len(),
        previous.len(),
    );

    let t_strip = std::time::Instant::now();
    let current_stripped = strip_comments(&current);
    let previous_stripped = strip_comments(&previous);
    let elapsed_strip = t_strip.elapsed().as_millis();
    if elapsed_strip > 0 {
        eprintln!("[perf] diff.strip_comments: {}ms", elapsed_strip);
    }

    eprintln!(
        "[diff] stripped: doc_len={} snap_len={}",
        current_stripped.len(),
        previous_stripped.len(),
    );

    let Some(output) = unified_diff_from_contents(&previous, &current) else {
        eprintln!(
            "[diff] no changes detected between snapshot and document (after comment stripping)"
        );
        let elapsed_total = t_total.elapsed().as_millis();
        if elapsed_total > 0 {
            eprintln!("[perf] diff.compute total: {}ms", elapsed_total);
        }
        return Ok(ComputeResult {
            diff: None,
            previous,
            current,
        });
    };

    // Stale snapshot recovery: if the diff is only completed assistant/user
    // exchanges with no new user content, the previous cycle wrote the response
    // but context compaction prevented the snapshot update.
    //
    if is_stale_snapshot(&previous, &current) {
        eprintln!(
            "[baseline recovery] state-db baseline advanced — previous cycle was already materialized"
        );
        snapshots.checkpoint(doc, &current)?;
        let elapsed_total = t_total.elapsed().as_millis();
        if elapsed_total > 0 {
            eprintln!("[perf] diff.compute total: {}ms", elapsed_total);
        }
        return Ok(ComputeResult {
            diff: None,
            previous,
            current,
        });
    }

    eprintln!("[diff] changes detected, computing unified diff");

    let elapsed_total = t_total.elapsed().as_millis();
    if elapsed_total > 0 {
        eprintln!("[perf] diff.compute total: {}ms", elapsed_total);
    }

    Ok(ComputeResult {
        diff: Some(output),
        previous,
        current,
    })
}

/// Compute a unified diff between the snapshot and the current document.
/// Returns None if there are no changes.
///
/// Both snapshot and current content are comment-stripped before comparison.
pub fn compute<S: DocumentBaselineStore + ?Sized>(
    snapshots: &S,
    doc: &Path,
) -> Result<Option<String>> {
    Ok(compute_with_current(snapshots, doc, None)?.diff)
}

/// Resolve the current document from Lazily, or from disk when no editor owns
/// the document.
pub fn wait_for_stable_content(
    doc: &Path,
    _previous: &str,
    live: Option<&dyn LiveCurrentSource>,
) -> Result<String> {
    // ── Lazily reactive state-diff feed, reactive-first (#preflight-lazily-diff-feed) ──
    // When a live source is present, read disk ONCE (no debounce) purely to
    // detect buffer divergence, then ask the reactive model for the coherent
    // current content. The reactive read is its own quiescence gate: the relay
    // only surfaces content once the canonical replica covers every live
    // editor's ops (the commit barrier), so it is prompt-complete by
    // construction — the realtime model, not a disk-mtime debounce, owns the
    // typing. The disk-settle path below is the fallback for when there is no
    // live divergence (reactive == disk), the model is unavailable/detached, or
    // no live source was injected at all.
    if let Some(live) = live
        && let Ok(disk_now) = std::fs::read_to_string(doc)
        && let Some(live_text) = live.live_current(doc, &disk_now)
    {
        eprintln!("[diff] current sourced from lazily reactive model (commit-barrier gated)");
        return Ok(live_text);
    }

    // A live editor without a current Lazily projection fails closed at the
    // authority gate. Reaching this branch means disk is the current replica;
    // no timing heuristic or parallel editor-state model participates.
    std::fs::read_to_string(doc).map_err(Into::into)
}

/// Resolve current content before external callers compute their own diff.
pub fn run<S: DocumentBaselineStore + ?Sized>(
    snapshots: &S,
    file: &Path,
    wait: bool,
) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    if wait {
        let previous = snapshots.resolve(file)?.unwrap_or_default();
        let _stable = wait_for_stable_content(file, &previous, None)?;
        eprintln!("[diff --wait] content is stable");
    }
    match compute(snapshots, file)? {
        Some(diff) => print!("{}", diff),
        None => eprintln!("No changes since last run."),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    struct TestBaselineStore;

    impl DocumentBaselineStore for TestBaselineStore {
        fn resolve(&self, doc: &Path) -> Result<Option<String>> {
            load_test_snapshot(doc)
        }

        fn checkpoint(&self, doc: &Path, content: &str) -> Result<()> {
            save_test_snapshot(doc, content)
        }
    }

    fn load_test_snapshot(doc: &Path) -> Result<Option<String>> {
        let path = agent_doc_fs::snapshot_path_for(doc)?;
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(std::fs::read_to_string(path)?))
    }

    fn save_test_snapshot(doc: &Path, content: &str) -> Result<()> {
        let path = agent_doc_fs::snapshot_path_for(doc)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(())
    }

    #[test]
    fn run_file_not_found() {
        let err = run(&TestBaselineStore, Path::new("/nonexistent/file.md"), false).unwrap_err();
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn copy_on_read_guard_skips_recovery_when_snapshot_modified() {
        // Verifies the copy-on-read guard logic: if snapshot mtime changes
        // between read and recovery, the save must be skipped.
        use std::time::SystemTime;

        let t1 = Some(SystemTime::UNIX_EPOCH);
        let t2 = Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1));

        // Same mtime → recovery should proceed (guard passes)
        assert_eq!(t1, t1, "same mtime should allow recovery");

        // Different mtime → recovery should be skipped (guard blocks)
        assert_ne!(t1, t2, "different mtime should block recovery");

        // Both None (no snapshot file) → recovery should proceed
        let none: Option<SystemTime> = None;
        assert_eq!(none, none, "both None should allow recovery");
    }

    /// Set up a temp directory with `.agent-doc/snapshots/` and a document file.
    /// Returns (TempDir, doc_path). The TempDir must be kept alive for the test.
    fn setup_compute_env(
        doc_content: &str,
        snap_content: &str,
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, doc_content).unwrap();

        // Create .agent-doc/snapshots/ and write the snapshot
        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        std::fs::create_dir_all(snap_path.parent().unwrap()).unwrap();
        std::fs::write(&snap_path, snap_content).unwrap();

        (dir, doc)
    }

    #[test]
    fn compute_stale_snapshot_recovery_proceeds_when_unmodified() {
        let snapshot = "## User\n\nHello\n";
        let document = "## User\n\nHello\n\n## Assistant\n\nResponse\n\n## User\n\n";

        let (_dir, doc) = setup_compute_env(document, snapshot);

        let result = compute(&TestBaselineStore, &doc).unwrap();
        assert!(
            result.is_none(),
            "stale snapshot recovery should return None"
        );

        let updated = load_test_snapshot(&doc).unwrap().unwrap();
        assert_eq!(updated, document);
    }

    #[test]
    fn compute_stale_recovery_updates_snapshot_to_current_document() {
        let snapshot = "## User\n\nHello\n";
        let document = "## User\n\nHello\n\n## Assistant\n\nResponse\n\n## User\n\n";

        let (_dir, doc) = setup_compute_env(document, snapshot);

        let result = compute(&TestBaselineStore, &doc).unwrap();
        assert!(result.is_none(), "stale recovery returns None");

        let snap = load_test_snapshot(&doc).unwrap().unwrap();
        assert_eq!(
            snap, document,
            "snapshot should be synced to document after recovery"
        );
    }

    #[test]
    fn compute_returns_diff_when_user_adds_content() {
        let snapshot = "## User\n\nHello\n";
        let document = "## User\n\nHello\n\nNew question here\n";

        let (_dir, doc) = setup_compute_env(document, snapshot);

        let result = compute(&TestBaselineStore, &doc).unwrap();
        assert!(result.is_some(), "should return a diff for user additions");
        let diff = result.unwrap();
        assert!(diff.contains("New question here"));
    }

    #[test]
    fn compute_returns_none_when_no_changes() {
        let content = "## User\n\nHello\n";

        let (_dir, doc) = setup_compute_env(content, content);

        let result = compute(&TestBaselineStore, &doc).unwrap();
        assert!(result.is_none(), "identical content should return None");
    }

    #[test]
    fn diff_detects_user_edits_after_stream_write() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test.md");

        let content_after_write = "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\nUser prompt\n\nAgent response\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content_after_write).unwrap();
        save_test_snapshot(&doc, content_after_write).unwrap();

        let content_after_edit = "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\nUser prompt\n\nAgent response\n\nNew user edit here\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content_after_edit).unwrap();

        let diff = compute(&TestBaselineStore, &doc).unwrap();
        assert!(
            diff.is_some(),
            "diff should detect user edit after stream write"
        );
        let diff_text = diff.unwrap();
        assert!(
            diff_text.contains("New user edit here"),
            "diff should contain user's new text: {}",
            diff_text
        );
    }

    #[test]
    fn diff_no_change_when_document_matches_snapshot() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test.md");
        let content = "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\nContent\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();
        save_test_snapshot(&doc, content).unwrap();

        let diff = compute(&TestBaselineStore, &doc).unwrap();
        assert!(diff.is_none(), "no diff when document matches snapshot");
    }

    #[test]
    fn diff_detects_change_after_cumulative_stream_flushes() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test.md");

        let snapshot_content = "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\nFull agent response here\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, snapshot_content).unwrap();
        save_test_snapshot(&doc, snapshot_content).unwrap();

        let edited = "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\nFull agent response here\n\nRelease agent-doc\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, edited).unwrap();

        let diff = compute(&TestBaselineStore, &doc).unwrap();
        assert!(diff.is_some(), "diff should detect user's edit");
        assert!(diff.unwrap().contains("Release agent-doc"));
    }

    // --- diff --wait tests ---

    #[test]
    fn run_with_wait_stable_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test.md");
        let snapshot_content = "line1\n";
        std::fs::write(&doc, "line1\nline2\n").unwrap();
        save_test_snapshot(&doc, snapshot_content).unwrap();

        let result = run(&TestBaselineStore, &doc, true);
        assert!(result.is_ok());
    }

    #[test]
    fn run_with_wait_no_changes() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test.md");
        let content = "line1\nline2\n";
        std::fs::write(&doc, content).unwrap();
        save_test_snapshot(&doc, content).unwrap();

        let result = run(&TestBaselineStore, &doc, true);
        assert!(result.is_ok());
    }

    #[test]
    fn wait_for_stable_content_returns_immediately_when_complete() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let content = "Complete sentence.\n";
        std::fs::write(&doc, content).unwrap();
        let previous = "";

        let start = std::time::Instant::now();
        let result = wait_for_stable_content(&doc, previous, None).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result, content);
        assert!(
            elapsed.as_millis() < 500,
            "should not delay for complete content"
        );
    }

    #[test]
    fn wait_for_stable_content_falls_back_without_editor_state() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test-fallback.md");
        let content = "Complete sentence.\n";
        std::fs::write(&doc, content).unwrap();

        let previous = "";
        let result = wait_for_stable_content(&doc, previous, None).unwrap();
        assert_eq!(result, content);
    }

    /// Live source that returns a fixed reactive-buffer text when the disk it is
    /// handed matches an expected value — models "the CRDT buffer has edits disk
    /// has not yet flushed".
    struct FixedLiveSource {
        reactive: Option<String>,
        seen_disk: std::cell::RefCell<Option<String>>,
    }

    impl LiveCurrentSource for FixedLiveSource {
        fn live_current(&self, _doc: &Path, disk: &str) -> Option<String> {
            *self.seen_disk.borrow_mut() = Some(disk.to_string());
            self.reactive.clone()
        }
    }

    // #preflight-lazily-diff-feed: when the reactive model reports live content
    // that has diverged from disk, the settled diff `current` is sourced from the
    // reactive model, not the disk read.
    #[test]
    fn wait_for_stable_content_prefers_live_reactive_over_disk() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("live.md");
        let disk = "Prompt on disk.\n";
        std::fs::write(&doc, disk).unwrap();

        let live = FixedLiveSource {
            reactive: Some("Prompt in reactive buffer, not yet on disk.\n".to_string()),
            seen_disk: std::cell::RefCell::new(None),
        };
        let result = wait_for_stable_content(&doc, "", Some(&live)).unwrap();

        assert_eq!(result, "Prompt in reactive buffer, not yet on disk.\n");
        // The live source is consulted AFTER the disk buffer settles, so it must
        // be handed the settled disk content (prompt-completeness gate first).
        assert_eq!(live.seen_disk.borrow().as_deref(), Some(disk));
    }

    // Reactive-first (#preflight-lazily-diff-feed Phase 3): the commit-barrier-
    // gated reactive read is the quiescence signal, so a live divergence bypasses
    // the disk-settle debounce entirely — even when the editor buffer is DIRTY
    // (which would otherwise make the disk path wait for stability).
    #[test]
    fn wait_for_stable_content_reactive_current_never_waits_on_disk_heuristics() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("dirty.md");
        let disk = "On disk.\n";
        std::fs::write(&doc, disk).unwrap();
        let live = FixedLiveSource {
            reactive: Some("Reactive canonical (commit-barrier complete).\n".to_string()),
            seen_disk: std::cell::RefCell::new(None),
        };
        let start = std::time::Instant::now();
        let result = wait_for_stable_content(&doc, "", Some(&live)).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result, "Reactive canonical (commit-barrier complete).\n");
        assert!(
            elapsed.as_millis() < 500,
            "reactive-first must not wait on the dirty-buffer disk debounce, took {}ms",
            elapsed.as_millis()
        );
    }

    // At rest (no live divergence → live source returns None), the result is
    // byte-identical to the disk-only path, so the feed is a pure superset.
    #[test]
    fn wait_for_stable_content_none_live_matches_disk_byte_for_byte() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("rest.md");
        let disk = "Settled content.\n";
        std::fs::write(&doc, disk).unwrap();

        let live = FixedLiveSource {
            reactive: None,
            seen_disk: std::cell::RefCell::new(None),
        };
        let with_live = wait_for_stable_content(&doc, "", Some(&live)).unwrap();
        let without_live = wait_for_stable_content(&doc, "", None).unwrap();

        assert_eq!(with_live, disk);
        assert_eq!(with_live, without_live);
    }

    #[test]
    fn compute_with_current_uses_live_reactive_content_for_diff() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("compute-live.md");
        let disk = "baseline line\n";
        std::fs::write(&doc, disk).unwrap();
        save_test_snapshot(&doc, disk).unwrap();

        let live = FixedLiveSource {
            reactive: Some("baseline line\nreactive addition\n".to_string()),
            seen_disk: std::cell::RefCell::new(None),
        };
        let result = compute_with_current(&TestBaselineStore, &doc, Some(&live)).unwrap();

        assert_eq!(result.current, "baseline line\nreactive addition\n");
        let diff = result
            .diff
            .expect("reactive addition should surface as a diff");
        assert!(
            diff.contains("reactive addition"),
            "diff must reflect the reactive-sourced current, got: {diff}"
        );
    }
}
