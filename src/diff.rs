use anyhow::Result;
use similar::{ChangeTag, TextDiff};
use std::path::Path;

use crate::snapshot;

/// Compute a unified diff between the snapshot and the current document.
/// Returns None if there are no changes.
pub fn compute(doc: &Path) -> Result<Option<String>> {
    let current = std::fs::read_to_string(doc)?;
    let previous = snapshot::load(doc)?.unwrap_or_default();

    let diff = TextDiff::from_lines(&previous, &current);
    let has_changes = diff
        .iter_all_changes()
        .any(|c| c.tag() != ChangeTag::Equal);

    if !has_changes {
        return Ok(None);
    }

    let mut output = String::new();
    for change in diff.iter_all_changes() {
        let prefix = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        output.push_str(prefix);
        output.push_str(change.value());
    }
    Ok(Some(output))
}

/// Print the diff to stdout (for the `diff` subcommand).
pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    match compute(file)? {
        Some(diff) => print!("{}", diff),
        None => eprintln!("No changes since last submit."),
    }
    Ok(())
}
