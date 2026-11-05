//! # Module: lint
//!
//! Read-only CLI adapter for the authoritative session-document lint policy.
//! It deliberately delegates to `agent-doc-lint-io`, so inspection and the
//! write/compact gates cannot drift into separate rule sets.
//!
//! After the gate it prints the first-principle and fact warnings preflight
//! would raise for the document (`#firstprincipleslist`,
//! `#firstprinciplesfactlist`, see `principles_cmd`).
//! They are warnings in every lint mode and never change the exit status.

use std::path::Path;

pub fn run(file: &Path) -> anyhow::Result<()> {
    let gate = agent_doc_lint_io::run_with_logger(file, None, agent_doc_ops_log_io::log_op);
    crate::principles_cmd::emit_lint_warnings(file);
    gate?;
    println!("No blocking lint findings for {}", file.display());
    Ok(())
}
