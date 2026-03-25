//! # Module: clean
//!
//! ## Spec
//! - Squashes the git history for a session document file, collapsing incremental session commits
//!   into a single commit to keep the repository history readable.
//! - Delegates all git logic to `git::squash_session(file)`.
//! - Fails fast with a clear error if the target file does not exist on disk.
//!
//! ## Agentic Contracts
//! - `run(file) -> Result<()>` — errors if `file` does not exist; otherwise delegates to
//!   `git::squash_session` and propagates any git errors unchanged.
//!
//! ## Evals
//! - file_not_found: non-existent path → `Err` containing "file not found"
//! - existing_file: valid session document with git history → `Ok(())`, history squashed

use anyhow::Result;
use std::path::Path;

use crate::git;

pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    git::squash_session(file)
}
