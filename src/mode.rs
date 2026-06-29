//! # Module: mode
//!
//! ## Spec
//! - `run(file, set)`: reads the document's frontmatter and either reports the current mode or
//!   updates it.
//!   - **Get** (`set = None`): resolves `(agent_doc_format, agent_doc_write)` via
//!     `frontmatter::parse` + `fm.resolve_mode()` and prints `format: <value>` and
//!     `write: <value>` to stdout.  If the legacy `agent_doc_mode` field is present, emits a
//!     deprecation warning to stderr.
//!   - **Set** (`set = Some(mode)`): maps the mode string to `(AgentDocFormat, AgentDocWrite)`:
//!     `"append"` → `(Append, Crdt)`, `"template"` → `(Template, Crdt)`, `"stream"` →
//!     `(Template, Crdt)`.  Any other value returns `Err`.  Calls
//!     `frontmatter::set_format_and_write` and writes the updated content back to the file.
//!
//! ## Agentic Contracts
//! - Returns `Err` if the file does not exist or is unreadable.
//! - Returns `Err` for unrecognised mode strings.
//! - The set path writes the full document back atomically via `std::fs::write`; the rest of the
//!   document content is preserved exactly.
//! - Get output goes to stdout; all diagnostic messages go to stderr.
//!
//! ## Evals
//! - run_get_reports_format: document with `agent_doc_format: template` → stdout contains "format: template"
//! - run_get_legacy_deprecation: document with `agent_doc_mode: stream` → stderr contains deprecation note
//! - run_set_append: `set="append"` → frontmatter updated to `agent_doc_format: append`
//! - run_set_invalid: `set="unknown"` → returns Err with message listing valid values
//! - run_file_not_found: non-existent file → returns Err

use anyhow::{Context, Result};
use std::path::Path;

use agent_doc_core::frontmatter::{self, AgentDocFormat, AgentDocWrite};

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
            _ => anyhow::bail!(
                "invalid mode: {} (expected 'append', 'template', or 'stream')",
                mode
            ),
        };
        let updated = frontmatter::set_format_and_write(&content, format, write)?;
        std::fs::write(file, &updated)
            .with_context(|| format!("failed to write {}", file.display()))?;
        eprintln!(
            "set agent_doc_format={}, agent_doc_write={} in {}",
            format,
            write,
            file.display()
        );
    } else {
        let resolved = fm.resolve_mode();
        // Show new fields
        println!("format: {}", resolved.format);
        println!("write: {}", resolved.write);
        // Show deprecation note if legacy mode field is present
        if let Some(ref legacy) = fm.mode {
            eprintln!(
                "note: deprecated agent_doc_mode={} is present; migrate to agent_doc_format + agent_doc_write",
                legacy
            );
        }
    }

    Ok(())
}
