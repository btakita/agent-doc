//! # Module: recover
//!
//! ## Spec
//! - Guards against response loss caused by context compaction interrupting the write-back phase (between agent respond and `agent-doc write`).
//! - Pending responses are stored in `.agent-doc/pending/<hash>.md` before the write attempt, making them durable across process restarts.
//! - `run(file)` — canonicalizes the path, checks for a pending file, and applies it if found. Before applying, reads the current document and checks if the response is already present (dedup guard). If already present, removes the pending file without writing (returns `Ok(false)`). Detection of `<!-- patch:` in the pending content selects template-patch write (`write::apply_template_from_string`); otherwise plain append is used (`write::apply_append_from_string`). Removes the pending file on successful write.
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
//! - no_pending_returns_false: document with no pending file → run returns Ok(false)
//! - save_and_clear_pending: save then clear → pending file created then removed
//! - recover_append_response: pending plain text response → applied as Assistant section, file updated, pending file removed, run returns Ok(true)
//! - empty_pending_cleaned_up: pending file with only whitespace → run returns Ok(false), pending file removed
//! - recover_skips_duplicate_apply: pending response already present in document → run returns Ok(false), pending file removed, document unchanged

use anyhow::{Context, Result};
use std::path::Path;

use crate::{snapshot, write};

/// Check for a pending response and apply it if found.
///
/// Returns `true` if a pending response was recovered, `false` otherwise.
pub fn run(file: &Path) -> Result<bool> {
    // Canonicalize first to handle CWD drift (e.g., when CWD is in a submodule)
    let canonical = file.canonicalize().map_err(|_| {
        anyhow::anyhow!("file not found: {}", file.display())
    })?;

    let pending_path = snapshot::pending_path_for(&canonical)?;
    if !pending_path.exists() {
        return Ok(false);
    }

    let response = std::fs::read_to_string(&pending_path)
        .with_context(|| format!("failed to read pending response {}", pending_path.display()))?;

    if response.trim().is_empty() {
        // Empty pending file — just clean up
        let _ = std::fs::remove_file(&pending_path);
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
        if let Err(e) = crate::cycle_state::mark_write_applied(
            file,
            "recover_already_applied",
            Some(&doc_content),
            Some(&doc_content),
        ) {
            eprintln!("[recover] cycle-state update failed: {} (non-fatal)", e);
        }
        std::fs::remove_file(&pending_path)
            .with_context(|| format!("failed to remove pending file {}", pending_path.display()))?;
        return Ok(false);
    }

    eprintln!(
        "[recover] Found orphaned response for {} ({} bytes). Applying...",
        file.display(),
        response.len()
    );

    // Check if response contains template patch blocks
    let is_template = response.contains("<!-- patch:");
    if is_template {
        write::apply_template_from_string(file, &response)?;
    } else {
        write::apply_append_from_string(file, &response)?;
    }

    // Remove the pending file after successful write
    std::fs::remove_file(&pending_path)
        .with_context(|| format!("failed to remove pending file {}", pending_path.display()))?;

    eprintln!("[recover] Response recovered and written to {}", file.display());
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
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n";
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
        assert!(!recovered, "should detect content as already applied despite (HEAD) suffix and blank lines");

        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, content);
    }
}
