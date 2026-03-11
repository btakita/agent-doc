//! `agent-doc mode` — Get or set the document mode (append/template).
//!
//! Usage:
//!   agent-doc mode <file>              # Show current mode
//!   agent-doc mode <file> --set append # Set mode to append
//!   agent-doc mode <file> --set template # Set mode to template

use anyhow::{Context, Result};
use std::path::Path;

use crate::frontmatter;

pub fn run(file: &Path, set: Option<&str>) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (fm, _) = frontmatter::parse(&content)?;

    if let Some(mode) = set {
        match mode {
            "append" | "template" | "stream" => {}
            _ => anyhow::bail!("invalid mode: {} (expected 'append', 'template', or 'stream')", mode),
        }
        let updated = frontmatter::set_mode(&content, mode)?;
        std::fs::write(file, &updated)
            .with_context(|| format!("failed to write {}", file.display()))?;
        eprintln!("set agent_doc_mode={} in {}", mode, file.display());
    } else {
        let mode = fm.mode.as_deref().unwrap_or("append");
        println!("{}", mode);
    }

    Ok(())
}
