//! # Module: audit_docs
//!
//! ## Spec
//! - Audits instruction files (CLAUDE.md, AGENTS.md, etc.) present in the codebase against
//!   a known-correct reference, using the shared `instruction_files` crate.
//! - Accepts an optional `root_override` path to run the audit from a directory other than CWD.
//! - Uses `AuditConfig::agent_doc()` to select agent-doc–specific configuration (file patterns,
//!   ignore rules, etc.).
//!
//! ## Agentic Contracts
//! - `run(root_override)` — performs the audit and returns `Ok(())` on success or a descriptive
//!   `anyhow::Error` on failure; never silently swallows errors.
//! - All configuration and output formatting is owned by `instruction_files`; this module is a
//!   thin adapter.
//!
//! ## Evals
//! - valid_project: project with correct instruction files → `Ok(())`, no output
//! - missing_file: required instruction file absent → `Err` with file path in message
//! - root_override: alternate root provided → audit runs relative to that path, not CWD

use anyhow::Result;
use instruction_files::AuditConfig;
use std::path::Path;

pub fn run(root_override: Option<&Path>) -> Result<()> {
    let config = AuditConfig::agent_doc();
    instruction_files::run(&config, root_override)
}
