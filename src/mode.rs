//! `agent-doc mode` — Get or set the document mode (append/template).
//!
//! Usage:
//!   agent-doc mode <file>              # Show current mode
//!   agent-doc mode <file> --set append # Set mode to append
//!   agent-doc mode <file> --set template # Set mode to template

use anyhow::{Context, Result};
use std::path::Path;

use crate::frontmatter::{self, AgentDocFormat, AgentDocWrite};

pub fn run(file: &Path, set: Option<&str>) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (fm, _) = frontmatter::parse(&content)?;

    if let Some(mode) = set {
        // Accept legacy mode strings and map to new fields
        let (format, write) = match mode {
            "append" => (AgentDocFormat::Append, AgentDocWrite::Crdt),
            "template" => (AgentDocFormat::Template, AgentDocWrite::Crdt),
            "stream" => (AgentDocFormat::Template, AgentDocWrite::Crdt),
            _ => anyhow::bail!("invalid mode: {} (expected 'append', 'template', or 'stream')", mode),
        };
        let updated = frontmatter::set_format_and_write(&content, format, write)?;
        std::fs::write(file, &updated)
            .with_context(|| format!("failed to write {}", file.display()))?;
        eprintln!("set agent_doc_format={}, agent_doc_write={} in {}", format, write, file.display());
    } else {
        let resolved = fm.resolve_mode();
        // Show new fields
        println!("format: {}", resolved.format);
        println!("write: {}", resolved.write);
        // Show deprecation note if legacy mode field is present
        if let Some(ref legacy) = fm.mode {
            eprintln!("note: deprecated agent_doc_mode={} is present; migrate to agent_doc_format + agent_doc_write", legacy);
        }
    }

    Ok(())
}
