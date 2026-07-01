//! # Module: dedupe
//!
//! ## Spec
//! - `run(file)` detects and removes duplicate consecutive response blocks
//!   within the exchange component of a template document.
//! - A duplicate is defined as two consecutive `### Re:` sections with identical
//!   headings and content (after trimming whitespace).
//! - When a duplicate is found, the second occurrence is removed.
//! - The snapshot is updated after deduplication.
//! - Any stale `.agent-doc/patches/<hash>.json` file is deleted after dedup to prevent
//!   the plugin from re-applying the removed content on next startup.
//! - Reports what was removed to stderr.
//!
//! ## Agentic Contracts
//! - Dedupe is idempotent: running it twice produces the same result.
//! - Only removes exact consecutive duplicates — non-consecutive identical
//!   blocks are preserved (they may be intentional).
//! - Never modifies user content — only agent response blocks (### Re:).

use agent_doc_turn::response_replay::dedupe_responses;
use anyhow::{Context, Result};
use std::path::Path;

pub fn run(file: &Path) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let result = dedupe_responses(&content);

    if result == content {
        eprintln!("[dedupe] no duplicates found in {}", file.display());
        return Ok(());
    }

    let removed = content.len() - result.len();
    // `#fcc0`: converge through the editor IPC when a live JB listener is active so
    // dedup never raises a `File Cache Conflict`; falls back to the same plain disk
    // write otherwise.
    crate::write::converge_or_disk_write(file, &content, &result, "dedupe")?;

    // Update snapshot to match
    crate::snapshot::save(file, &result)?;

    // Clean up any stale patch file so the plugin doesn't re-apply the removed content.
    // Without this, processPendingPatches() on plugin restart would re-duplicate.
    if let Ok(hash) = crate::snapshot::doc_hash(file)
        && let Some(project_root) = agent_doc_fs::find_project_root(file)
    {
        let patch_file = project_root
            .join(".agent-doc/patches")
            .join(format!("{}.json", hash));
        if patch_file.exists() {
            eprintln!(
                "[dedupe] cleaning stale patch file: {}",
                patch_file.display()
            );
            if let Err(e) = std::fs::remove_file(&patch_file) {
                eprintln!("[dedupe] WARNING: failed to remove stale patch file: {}", e);
            }
        }
    }

    eprintln!(
        "[dedupe] removed {} bytes of duplicate content from {}",
        removed,
        file.display()
    );

    Ok(())
}
