//! # Module: recover
//!
//! ## Spec
//! - Guards against response loss caused by context compaction interrupting the write-back phase (between agent respond and `agent-doc write`).
//! - Pending responses are stored in `.agent-doc/pending/<hash>.md` before the write attempt, and
//!   the same response is also captured in `.agent-doc/captures/<doc-hash>/<cycle-id>.json`.
//! - `run(file)` — canonicalizes the path, checks for a pending file or active durable capture, and
//!   applies it if found. Before applying, reads the current document and checks if the response is
//!   already present (dedup guard). If already present, removes the pending file without writing
//!   (returns `Ok(false)`). When replaying from a durable capture, requires the current document and
//!   snapshot hashes to still match the captured baseline; otherwise fails closed.
//!   Template/CRDT documents always replay through the template write path
//!   (`write::apply_template_from_string`) even when the captured response is raw text
//!   without `<!-- patch:... -->` fences (for example `compact exchange` closeouts).
//!   Non-template documents use plain append (`write::apply_append_from_string`).
//!   Removes the pending file on successful write.
//! - Empty pending files are cleaned up without triggering a write; `run` returns `false`.
//! - `save_pending(file, response)` — writes the response to the pending store, creating parent directories as needed.
//! - `clear_pending(file)` — removes the pending file; no-op if it does not exist.
//! - `fingerprint_lines(response)` — extracts the first 3 non-empty, non-marker lines from a response for dedup checking.
//!
//! ## Agentic Contracts
//! - `run(file)` — returns `Ok(false)` when no pending file exists, the pending file is empty, or the response is already present in the document; returns `Ok(true)` after a successful recovery write; returns `Err` on I/O failure or if the write-back itself fails.
//! - Pending file is removed only after a fully successful write (or dedup detection); a failed write leaves the pending file intact for retry.
//! - `save_pending` and `clear_pending` are idempotent with respect to directory creation and missing files respectively.
//! - Callers (e.g., `preflight`) invoke `run` at session start to surface any orphaned responses before proceeding.
//!
//! ## Evals
//! - no_pending_returns_false: document with no pending file or capture → run returns Ok(false)
//! - save_and_clear_pending: save then clear → pending file created then removed
//! - recover_append_response: pending plain text response → applied as Assistant section, file updated, pending file removed, run returns Ok(true)
//! - empty_pending_cleaned_up: pending file with only whitespace → run returns Ok(false), pending file removed
//! - recover_skips_duplicate_apply: pending response already present in document → run returns Ok(false), pending file removed, document unchanged
//! - recover_replays_capture_without_pending: durable capture with no pending file → run returns Ok(true)
//! - recover_fails_closed_on_capture_hash_mismatch: durable capture baseline mismatch → run returns Err

use anyhow::{Context, Result};
use std::path::Path;

use crate::{frontmatter, snapshot, write};

fn repair_stale_preflight_started_cycle(file: &Path) -> Result<bool> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(false);
    };
    if state.phase != crate::cycle_state::CyclePhase::PreflightStarted {
        return Ok(false);
    }

    let file_content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} for stale preflight repair",
            file.display()
        )
    })?;
    let snapshot_content = snapshot::load(file)?;
    let current_file_hash = crate::ops_log::content_hash(&file_content);
    let current_snapshot_hash = snapshot_content
        .as_deref()
        .map(crate::ops_log::content_hash);

    if state.file_hash.as_deref() != Some(current_file_hash.as_str())
        || state.snapshot_hash != current_snapshot_hash
    {
        return Ok(false);
    }

    crate::cycle_state::mark_committed(
        file,
        "recover_preflight_stale_lock",
        snapshot_content.as_deref(),
        Some(&file_content),
    )?;
    crate::ops_log::log_op(
        file,
        &format!(
            "recover_preflight_stale_lock file={} cycle_id={}",
            file.display(),
            state.cycle_id
        ),
    );
    eprintln!(
        "[recover] repaired stale preflight_started cycle {} for {}",
        state.cycle_id,
        file.display()
    );
    Ok(true)
}

fn repair_template_tail_if_needed(file: &Path, doc_content: &str) -> Result<String> {
    match crate::template::repair_conversation_tail_outside_exchange(doc_content)? {
        Some(repaired) => {
            write::atomic_write_pub(file, &repaired)?;
            snapshot::save(file, &repaired)?;
            crate::ops_log::log_op(
                file,
                &format!("recover_repair_exchange_tail file={}", file.display()),
            );
            eprintln!(
                "[recover] repaired escaped conversation tail in {}",
                file.display()
            );
            Ok(repaired)
        }
        None => Ok(doc_content.to_string()),
    }
}

/// Check for a pending response and apply it if found.
///
/// Returns `true` if a pending response was recovered, `false` otherwise.
pub fn run(file: &Path) -> Result<bool> {
    // Canonicalize first to handle CWD drift (e.g., when CWD is in a submodule)
    let canonical = file
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("file not found: {}", file.display()))?;

    let pending_path = snapshot::pending_path_for(&canonical)?;
    let capture = crate::capture::load_active(&canonical)?;
    if !pending_path.exists() && capture.is_none() {
        if repair_stale_preflight_started_cycle(file)? {
            return Ok(true);
        }
        return Ok(false);
    }

    let pending_response = if pending_path.exists() {
        Some(std::fs::read_to_string(&pending_path).with_context(|| {
            format!("failed to read pending response {}", pending_path.display())
        })?)
    } else {
        None
    };
    let response = capture
        .as_ref()
        .map(|r| r.response_body.clone())
        .or(pending_response.clone())
        .unwrap_or_default();

    if response.trim().is_empty() {
        // Empty pending file — just clean up
        let _ = std::fs::remove_file(&pending_path);
        let _ = crate::capture::mark_discarded(&canonical);
        return Ok(false);
    }

    // Dedup guard: check if the response content is already present in the document.
    // This prevents double-apply when the pending file was left behind after a successful
    // IPC write (e.g., IPC timeout path exits with code 75 without calling clear_pending,
    // but the plugin already applied the content via the IPC patch file).
    let doc_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read document for dedup check {}", file.display()))?;
    if is_already_applied(&doc_content, &response) {
        eprintln!(
            "[recover] Response already present in document — skipping apply, cleaning up pending file"
        );
        let repaired_doc = repair_template_tail_if_needed(file, &doc_content)?;
        if let Err(e) = crate::cycle_state::mark_write_applied(
            file,
            "recover_already_applied",
            Some(&repaired_doc),
            Some(&repaired_doc),
        ) {
            eprintln!("[recover] cycle-state update failed: {} (non-fatal)", e);
        }
        clear_pending(&canonical)?;
        return Ok(false);
    }

    if let Some(ref capture) = capture {
        crate::capture::validate_replay(&canonical, capture)?;
    }

    eprintln!(
        "[recover] Found orphaned response for {} ({} bytes). Applying...",
        file.display(),
        response.len()
    );

    let (fm, _) = frontmatter::parse(&doc_content)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;
    let use_template_write = fm.resolve_mode().is_template() || response.contains("<!-- patch:");
    if use_template_write {
        write::apply_template_from_string(file, &response)?;
    } else {
        write::apply_append_from_string(file, &response)?;
    }

    // Remove the pending file after successful write
    clear_pending(&canonical)?;

    eprintln!(
        "[recover] Response recovered and written to {}",
        file.display()
    );
    let final_doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read recovered document {}", file.display()))?;
    if let Err(e) = crate::cycle_state::mark_write_applied(
        file,
        "recover_applied",
        Some(&final_doc),
        Some(&final_doc),
    ) {
        eprintln!("[recover] cycle-state update failed: {} (non-fatal)", e);
    }
    if let Err(e) = crate::capture::mark_replayed(&canonical) {
        eprintln!("[recover] capture-state update failed: {} (non-fatal)", e);
    }
    Ok(true)
}

/// Returns true if the pending response content appears to already be applied to the document.
///
/// Checks each fingerprint line individually against the document. This handles
/// blank-line separation (document paragraphs have blank lines between them but
/// fingerprint skips blanks) and boundary marker suffixes like `(HEAD)`.
fn is_already_applied(doc: &str, response: &str) -> bool {
    let lines = fingerprint_lines(response);
    if lines.is_empty() {
        return false;
    }
    lines.iter().all(|line| doc.contains(line.as_str()))
}

fn fingerprint_lines(response: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("<!-- patch:")
            || trimmed.starts_with("<!-- /patch:")
            || trimmed.starts_with("<!-- agent:")
            || trimmed.starts_with("<!-- /agent:")
        {
            continue;
        }
        lines.push(line.to_string());
        if lines.len() >= 3 {
            break;
        }
    }
    lines
}

/// Save a response to the pending store before attempting write-back.
/// This makes the response durable across context compaction.
pub fn save_pending(file: &Path, response: &str) -> Result<()> {
    crate::capture::capture_response(file, response)?;
    let pending_path = snapshot::pending_path_for(file)?;
    if let Some(parent) = pending_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pending_path, response)
        .with_context(|| format!("failed to save pending response {}", pending_path.display()))?;
    Ok(())
}

/// Remove the pending file after a successful write-back.
pub fn clear_pending(file: &Path) -> Result<()> {
    let pending_path = snapshot::pending_path_for(file)?;
    if pending_path.exists() {
        std::fs::remove_file(&pending_path)?;
    }
    // Also clean up the pre-response snapshot (saved before write for undo support).
    // Without this, pre-response files accumulate indefinitely after successful writes.
    if let Err(e) = snapshot::delete_pre_response(file) {
        eprintln!("[recover] warning: failed to delete pre-response: {}", e);
    }
    if let Err(e) = crate::capture::mark_write_applied(file) {
        eprintln!("[recover] warning: failed to update capture state: {}", e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/pending")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
        dir
    }

    #[test]
    fn no_pending_returns_false() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "# Doc\n\n## User\n\nHello\n").unwrap();
        assert!(!run(&doc).unwrap());
    }

    #[test]
    fn save_and_clear_pending() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();

        save_pending(&doc, "response text").unwrap();
        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(pending.exists());

        clear_pending(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn recover_append_response() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\nagent_doc_mode: append\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();

        // Save a pending response
        save_pending(&doc, "This is the recovered response.").unwrap();

        // Recover it
        let recovered = run(&doc).unwrap();
        assert!(recovered);

        // Verify the response was written
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("This is the recovered response."));
        assert!(result.contains("## Assistant"));

        // Pending file should be cleaned up
        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn recover_plain_response_uses_template_path_for_template_docs() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "## User\n",
            "compact exchange\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending patch=replace -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        save_pending(&doc, "Exchange compacted. No new work was run in this turn.").unwrap();

        let recovered = run(&doc).unwrap();
        assert!(recovered);

        let result = std::fs::read_to_string(&doc).unwrap();
        let exchange_close = result.find("<!-- /agent:exchange -->").unwrap();
        let summary = result
            .find("Exchange compacted. No new work was run in this turn.")
            .unwrap();
        assert!(
            summary < exchange_close,
            "plain recovery for template docs should stay inside exchange:\n{result}"
        );
        assert!(
            !result[exchange_close..].contains("## Assistant"),
            "template recovery must not append inline assistant blocks after exchange:\n{result}"
        );
    }

    #[test]
    fn empty_pending_cleaned_up() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();

        save_pending(&doc, "").unwrap();
        let recovered = run(&doc).unwrap();
        assert!(!recovered);

        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn recover_skips_duplicate_apply() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        // Document already contains the response content (as if IPC applied it)
        let response = "This is the response that was already applied.\nSecond line.\nThird line.";
        let content = format!(
            "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\n{}\n\n## User\n\n",
            response
        );
        std::fs::write(&doc, &content).unwrap();

        // Pending file still exists (clear_pending was never called after IPC write)
        save_pending(&doc, response).unwrap();

        // run should detect the content is already present and skip
        let recovered = run(&doc).unwrap();
        assert!(!recovered);

        // Document should be unchanged
        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, content);

        // Pending file should be cleaned up
        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn recover_replays_capture_without_pending() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        save_pending(&doc, "Recovered from capture.").unwrap();
        clear_pending(&doc).unwrap();
        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
        // Re-arm capture as if the write never happened.
        crate::capture::capture_response(&doc, "Recovered from capture.").unwrap();

        let recovered = run(&doc).unwrap();
        assert!(recovered);
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("Recovered from capture."));
    }

    #[test]
    fn recover_fails_closed_on_capture_hash_mismatch() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        save_pending(&doc, "Recovered from capture.").unwrap();
        let pending = snapshot::pending_path_for(&doc).unwrap();
        std::fs::remove_file(&pending).unwrap();
        std::fs::write(&doc, "---\nsession: test\n---\n\n## User\n\nHello again\n").unwrap();

        let err = run(&doc).unwrap_err();
        assert!(
            err.to_string().contains("baseline no longer matches"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn recover_dedup_with_blank_lines_and_boundary() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        // Response has template patch with content lines
        let response = "<!-- patch:exchange -->\n### Re: topic — opus-4-6\n\n**Details:**\n- Item one\n<!-- /patch:exchange -->";
        // Document has the content with blank lines and (HEAD) boundary suffix
        let content = "---\nsession: test\n---\n\n<!-- agent:exchange -->\n### Re: topic — opus-4-6 (HEAD)\n\n**Details:**\n- Item one\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();

        save_pending(&doc, response).unwrap();

        let recovered = run(&doc).unwrap();
        assert!(
            !recovered,
            "should detect content as already applied despite (HEAD) suffix and blank lines"
        );

        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn recover_repairs_stale_preflight_started_cycle_when_hashes_match() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\nbody\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let repaired = run(&doc).unwrap();
        assert!(repaired, "stale preflight lock should be repaired");
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
        assert_eq!(state.last_event, "recover_preflight_stale_lock");
    }

    #[test]
    fn recover_repairs_escaped_exchange_tail_when_response_already_present() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending patch=replace -->\n",
            "- [ ] keep\n",
            "<!-- /agent:pending -->\n\n",
            "## Assistant\n\n",
            "Recovered answer.\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        save_pending(&doc, "Recovered answer.").unwrap();
        let recovered = run(&doc).unwrap();
        assert!(!recovered, "dedup path should skip replay");

        let repaired = std::fs::read_to_string(&doc).unwrap();
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let assistant = repaired.find("## Assistant").unwrap();
        assert!(
            assistant < exchange_close,
            "escaped assistant block should move back inside exchange:\n{repaired}"
        );
    }
}
