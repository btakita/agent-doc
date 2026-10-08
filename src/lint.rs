//! # Module: lint
//!
//! Read-only CLI adapter for the authoritative session-document lint policy.
//! It deliberately delegates to `agent-doc-lint-io`, so inspection and the
//! write/compact gates cannot drift into separate rule sets.

use std::path::Path;

pub fn run(file: &Path) -> anyhow::Result<()> {
    agent_doc_lint_io::run_with_logger(file, None, agent_doc_ops_log_io::log_op)?;
    println!("No blocking lint findings for {}", file.display());
    Ok(())
}
