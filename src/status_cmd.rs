//! CLI-driven status component mutations for `agent-doc write --status`.
//!
//! Replaces the content of `<!-- agent:status -->` with the provided text.

use anyhow::{Context, Result};
use std::path::Path;

use crate::component;

fn find_status_component(file: &Path) -> Result<(String, component::Component)> {
    let content = std::fs::read_to_string(file).context("failed to read document")?;
    let components = component::parse(&content).context("failed to parse components")?;
    let comp = components
        .into_iter()
        .find(|c| c.name == "status")
        .context("document has no status component")?;
    Ok((content, comp))
}

/// Replace the status component content with the provided text.
pub fn set(file: &Path, text: &str) -> Result<()> {
    let (full_content, comp) = find_status_component(file)?;
    let new_content = format!("\n{}\n", text);
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    Ok(())
}
