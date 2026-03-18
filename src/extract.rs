//! `agent-doc extract` — Move content between session documents.
//!
//! Supports extracting the last exchange pair, specific components,
//! or line ranges from one document to another.

use anyhow::{Context, Result};
use std::path::Path;

use crate::{component, snapshot, write};

/// Extract the last exchange entry from source and append to target.
///
/// For template documents: extracts the last `### Re:` block from `agent:exchange`.
/// For inline documents: extracts the last `## User` + `## Assistant` pair.
pub fn run(source: &Path, target: &Path, component_name: Option<&str>) -> Result<()> {
    if !source.exists() {
        anyhow::bail!("source file not found: {}", source.display());
    }
    if !target.exists() {
        anyhow::bail!("target file not found: {}", target.display());
    }

    let source_content = std::fs::read_to_string(source)
        .with_context(|| format!("failed to read {}", source.display()))?;
    let target_content = std::fs::read_to_string(target)
        .with_context(|| format!("failed to read {}", target.display()))?;

    let comp_name = component_name.unwrap_or("exchange");

    // Find the exchange component in source
    let components = component::parse(&source_content)
        .context("failed to parse components in source")?;

    let exchange = components.iter().find(|c| c.name == comp_name);
    let Some(exchange) = exchange else {
        anyhow::bail!("component '{}' not found in {}", comp_name, source.display());
    };

    let exchange_content = exchange.content(&source_content);
    if exchange_content.trim().is_empty() {
        anyhow::bail!("component '{}' is empty in {}", comp_name, source.display());
    }

    // Extract the last exchange entry (### Re: block)
    let (extracted, remaining) = split_last_entry(exchange_content);

    if extracted.trim().is_empty() {
        anyhow::bail!("no exchange entry found to extract");
    }

    // Update source: replace exchange content with remaining
    let new_source = exchange.replace_content(&source_content, &remaining);
    write::atomic_write_pub(source, &new_source)?;
    snapshot::save(source, &new_source)?;

    // Append extracted content to target's exchange component
    let target_components = component::parse(&target_content)
        .context("failed to parse components in target")?;

    let target_exchange = target_components.iter().find(|c| c.name == comp_name);
    let new_target = if let Some(tc) = target_exchange {
        let existing = tc.content(&target_content);
        let appended = format!("{}{}", existing.trim_end(), if existing.trim().is_empty() { "\n" } else { "\n\n" });
        tc.replace_content(&target_content, &format!("{}{}\n", appended.trim_end(), extracted.trim_end()))
    } else {
        // No matching component in target — append at end
        format!("{}\n{}\n", target_content.trim_end(), extracted.trim_end())
    };

    write::atomic_write_pub(target, &new_target)?;
    snapshot::save(target, &new_target)?;

    eprintln!(
        "[extract] Moved last entry from {}:{} → {}:{}",
        source.display(), comp_name, target.display(), comp_name
    );

    Ok(())
}

/// Split content into (last_entry, remaining).
/// Looks for the last `### Re:` header as the split point.
fn split_last_entry(content: &str) -> (String, String) {
    // Find the last ### Re: header
    let mut last_header_pos = None;
    for (i, _) in content.match_indices("### Re:") {
        last_header_pos = Some(i);
    }

    match last_header_pos {
        Some(pos) => {
            let remaining = &content[..pos];
            let extracted = &content[pos..];
            (extracted.to_string(), remaining.to_string())
        }
        None => {
            // No ### Re: headers — extract everything
            (content.to_string(), String::new())
        }
    }
}

/// Transfer content between documents by component name.
/// Moves the entire component content from source to target.
pub fn transfer(source: &Path, target: &Path, component_name: &str) -> Result<()> {
    if !source.exists() {
        anyhow::bail!("source file not found: {}", source.display());
    }
    if !target.exists() {
        anyhow::bail!("target file not found: {}", target.display());
    }

    let source_content = std::fs::read_to_string(source)
        .with_context(|| format!("failed to read {}", source.display()))?;
    let target_content = std::fs::read_to_string(target)
        .with_context(|| format!("failed to read {}", target.display()))?;

    let components = component::parse(&source_content)
        .context("failed to parse components in source")?;

    let comp = components.iter().find(|c| c.name == component_name);
    let Some(comp) = comp else {
        anyhow::bail!("component '{}' not found in {}", component_name, source.display());
    };

    let content = comp.content(&source_content);
    if content.trim().is_empty() {
        anyhow::bail!("component '{}' is empty in {}", component_name, source.display());
    }

    // Clear source component
    let new_source = comp.replace_content(&source_content, "\n");
    write::atomic_write_pub(source, &new_source)?;
    snapshot::save(source, &new_source)?;

    // Append to target component (or end of file)
    let target_components = component::parse(&target_content)
        .context("failed to parse components in target")?;

    let target_comp = target_components.iter().find(|c| c.name == component_name);
    let new_target = if let Some(tc) = target_comp {
        let existing = tc.content(&target_content);
        tc.replace_content(&target_content, &format!("{}{}\n", existing, content.trim_end()))
    } else {
        format!("{}\n{}\n", target_content.trim_end(), content.trim_end())
    };

    write::atomic_write_pub(target, &new_target)?;
    snapshot::save(target, &new_target)?;

    eprintln!(
        "[transfer] Moved component '{}' from {} → {}",
        component_name, source.display(), target.display()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_last_entry_single_block() {
        let content = "### Re: Question\n\nAnswer here.\n";
        let (extracted, remaining) = split_last_entry(content);
        assert_eq!(extracted, "### Re: Question\n\nAnswer here.\n");
        assert_eq!(remaining, "");
    }

    #[test]
    fn split_last_entry_multiple_blocks() {
        let content = "### Re: First\n\nFirst answer.\n\n### Re: Second\n\nSecond answer.\n";
        let (extracted, remaining) = split_last_entry(content);
        assert_eq!(extracted, "### Re: Second\n\nSecond answer.\n");
        assert_eq!(remaining, "### Re: First\n\nFirst answer.\n\n");
    }

    #[test]
    fn split_last_entry_no_headers() {
        let content = "Just some text without headers.\n";
        let (extracted, remaining) = split_last_entry(content);
        assert_eq!(extracted, "Just some text without headers.\n");
        assert_eq!(remaining, "");
    }
}
