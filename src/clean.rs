//! # Module: clean
//!
//! ## Spec
//! - Squashes the git history for a session document file, collapsing incremental session commits
//!   into a single commit to keep the repository history readable.
//! - `--archive` creates a tag `archive/pre-squash-<timestamp>` before squashing, preserving
//!   the full history for later inspection.
//! - Delegates all git logic to `git::squash_session(file)`.
//! - Fails fast with a clear error if the target file does not exist on disk.
//!
//! ## Agentic Contracts
//! - `run(file, archive) -> Result<()>` — errors if `file` does not exist; optionally tags
//!   pre-squash state, then delegates to `git::squash_session`.
//!
//! ## Evals
//! - file_not_found: non-existent path → `Err` containing "file not found"
//! - existing_file: valid session document with git history → `Ok(())`, history squashed
//! - archive_flag: creates tag before squash

use anyhow::Result;
use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

use crate::git;

pub fn run(file: &Path, archive: bool) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    if archive {
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let tag = format!("archive/pre-squash-{}", ts);
        let status = Command::new("git")
            .args(["tag", &tag])
            .status()?;
        if status.success() {
            eprintln!("[clean] Archived pre-squash state as tag: {}", tag);
        } else {
            anyhow::bail!("failed to create archive tag: {}", tag);
        }
    }
    git::squash_session(file)
}
