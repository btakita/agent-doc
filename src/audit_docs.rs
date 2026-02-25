//! Audit instruction files against the codebase.
//!
//! Delegates to the shared `instruction_files` crate with agent-doc configuration.

use anyhow::Result;
use instruction_files::AuditConfig;
use std::path::Path;

pub fn run(root_override: Option<&Path>) -> Result<()> {
    let config = AuditConfig::agent_doc();
    instruction_files::run(&config, root_override)
}
