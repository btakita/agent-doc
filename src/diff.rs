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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_format_additions() {
        use similar::{ChangeTag, TextDiff};
        let previous = "line1\n";
        let current = "line1\nline2\n";
        let diff = TextDiff::from_lines(previous, current);
        let has_insert = diff.iter_all_changes().any(|c| c.tag() == ChangeTag::Insert);
        assert!(has_insert);
    }

    #[test]
    fn diff_format_deletions() {
        use similar::{ChangeTag, TextDiff};
        let previous = "line1\nline2\n";
        let current = "line1\n";
        let diff = TextDiff::from_lines(previous, current);
        let has_delete = diff.iter_all_changes().any(|c| c.tag() == ChangeTag::Delete);
        assert!(has_delete);
    }

    #[test]
    fn diff_format_unchanged() {
        use similar::{ChangeTag, TextDiff};
        let content = "line1\nline2\n";
        let diff = TextDiff::from_lines(content, content);
        let all_equal = diff.iter_all_changes().all(|c| c.tag() == ChangeTag::Equal);
        assert!(all_equal);
    }

    #[test]
    fn diff_format_mixed() {
        use similar::{ChangeTag, TextDiff};
        let previous = "line1\nline2\nline3\n";
        let current = "line1\nchanged\nline3\n";
        let diff = TextDiff::from_lines(previous, current);

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
        assert!(output.contains(" line1\n"));
        assert!(output.contains("-line2\n"));
        assert!(output.contains("+changed\n"));
        assert!(output.contains(" line3\n"));
    }

    #[test]
    fn run_file_not_found() {
        let err = run(Path::new("/nonexistent/file.md")).unwrap_err();
        assert!(err.to_string().contains("file not found"));
    }
}
