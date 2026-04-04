//! # Module: notify
//!
//! ## Spec
//! - Appends a blockquote notification to a template document's exchange component.
//! - Notification format is a `> **[NOTIFY from <source>]** (<timestamp>)` blockquote.
//! - Appends before the boundary marker if one exists, otherwise at the end of the
//!   exchange component content.
//! - After appending, writes the document atomically, updates the snapshot, and
//!   optionally commits.
//!
//! ## Agentic Contracts
//! - `run(file, message, source, affects, commit)` — returns `Err` if the file is missing,
//!   the exchange component is not found, or any write fails.
//! - Snapshot is always updated after a successful notification write.
//! - When `commit` is true, calls `git::commit` after writing.
//!
//! ## Evals
//! - basic_notify: exchange component + message → blockquote appended with timestamp
//! - notify_with_source: `--source` provided → `[NOTIFY from <source>]` header
//! - notify_without_source: no source → `[NOTIFY]` header
//! - notify_with_affects: `--affects` provided → `Re-evaluate:` line present
//! - notify_without_affects: no affects → no `Re-evaluate:` line
//! - notify_before_boundary: boundary marker present → notification inserted before it
//! - multiline_message: message with newlines → each line prefixed with `> `

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use crate::{component, snapshot};

/// Format an ISO-8601 timestamp using the system `date` command.
fn iso_timestamp() -> String {
    let output = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok();
    match output {
        Some(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        None => "unknown".to_string(),
    }
}

/// Format the notification blockquote.
fn format_notification(message: &str, source: Option<&str>, affects: Option<&str>) -> String {
    let timestamp = iso_timestamp();
    let header = match source {
        Some(s) => format!("> **[NOTIFY from {}]** ({})", s, timestamp),
        None => format!("> **[NOTIFY]** ({})", timestamp),
    };

    let mut lines = vec![String::new(), header];

    // Message lines, each prefixed with >
    for line in message.lines() {
        if line.is_empty() {
            lines.push(">".to_string());
        } else {
            lines.push(format!("> {}", line));
        }
    }

    if let Some(affects_text) = affects {
        lines.push(">".to_string());
        lines.push(format!("> **Re-evaluate:** {}", affects_text));
    }

    lines.push(String::new());
    lines.join("\n")
}

/// Find the project root by walking up from a file path looking for `.agent-doc/`.
fn find_project_root(file: &Path) -> Option<std::path::PathBuf> {
    let canonical = file.canonicalize().ok()?;
    let mut dir = canonical.parent()?;
    loop {
        if dir.join(".agent-doc").is_dir() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

/// Append a notification to a document's exchange component, update the snapshot,
/// and optionally commit.
pub fn run(
    file: &Path,
    message: &str,
    source: Option<&str>,
    affects: Option<&str>,
    commit: bool,
) -> Result<()> {
    if !file.exists() {
        bail!("file not found: {}", file.display());
    }

    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let components = component::parse(&doc)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;

    let exchange = components
        .iter()
        .find(|c| c.name == "exchange")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "component 'exchange' not found in {}",
                file.display()
            )
        })?;

    let notification = format_notification(message, source, affects);

    // Find the boundary marker if one exists inside the exchange component
    let content_region = &doc[exchange.open_end..exchange.close_start];
    let boundary_pos = find_boundary_position(content_region);

    let new_doc = if let Some(rel_pos) = boundary_pos {
        // Insert before the boundary marker line
        let abs_pos = exchange.open_end + rel_pos;
        // Find start of the line containing the boundary
        let line_start = doc[..abs_pos]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(exchange.open_end)
            .max(exchange.open_end);

        let mut result = String::with_capacity(doc.len() + notification.len());
        result.push_str(&doc[..line_start]);
        result.push_str(&notification);
        if !notification.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(&doc[line_start..]);
        result
    } else {
        // Append at the end of the exchange content
        let existing = exchange.content(&doc);
        let mut new_content = String::with_capacity(existing.len() + notification.len());
        new_content.push_str(existing);
        new_content.push_str(&notification);
        exchange.replace_content(&doc, &new_content)
    };

    // Atomic write
    let parent = file.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    std::io::Write::write_all(&mut tmp, new_doc.as_bytes())
        .with_context(|| "failed to write temp file")?;
    tmp.persist(file)
        .with_context(|| format!("failed to rename temp file to {}", file.display()))?;

    // Save snapshot relative to project root (not CWD) for thread safety
    let snap_rel = snapshot::path_for(file)?;
    if let Some(root) = find_project_root(file) {
        let snap_abs = root.join(&snap_rel);
        if let Some(snap_parent) = snap_abs.parent() {
            std::fs::create_dir_all(snap_parent)
                .with_context(|| format!("failed to create snapshot dir for {}", file.display()))?;
        }
        std::fs::write(&snap_abs, &new_doc)
            .with_context(|| format!("failed to update snapshot for {}", file.display()))?;
    } else {
        snapshot::save(file, &new_doc)
            .with_context(|| format!("failed to update snapshot for {}", file.display()))?;
    }

    eprintln!(
        "Notified exchange in {} (source: {})",
        file.display(),
        source.unwrap_or("none")
    );

    if commit {
        crate::git::commit(file)?;
    }

    Ok(())
}

/// Find the byte offset of a boundary marker within a content region.
/// Returns the relative offset from the start of the region.
fn find_boundary_position(content: &str) -> Option<usize> {
    // Boundary markers look like: <!-- agent:boundary:UUID -->
    let prefix = "<!-- agent:boundary:";
    content.find(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        dir
    }

    fn write_doc(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn basic_notify() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\nSome content\n<!-- /agent:exchange -->\n",
        );

        run(&doc, "Hello world", None, None, false).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("> **[NOTIFY]**"));
        assert!(result.contains("> Hello world"));
        assert!(result.contains("<!-- agent:exchange"));
        assert!(result.contains("<!-- /agent:exchange -->"));
    }

    #[test]
    fn notify_with_source() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        );

        run(&doc, "Update available", Some("build-monitor"), None, false).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("> **[NOTIFY from build-monitor]**"));
    }

    #[test]
    fn notify_without_source() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        );

        run(&doc, "Something happened", None, None, false).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("> **[NOTIFY]**"));
        assert!(!result.contains("from"));
    }

    #[test]
    fn notify_with_affects() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        );

        run(&doc, "API changed", None, Some("integration tests"), false).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("> **Re-evaluate:** integration tests"));
    }

    #[test]
    fn notify_without_affects() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        );

        run(&doc, "Just FYI", None, None, false).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(!result.contains("Re-evaluate"));
    }

    #[test]
    fn notify_before_boundary() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\nExisting content\n<!-- agent:boundary:abc123 -->\n<!-- /agent:exchange -->\n",
        );

        run(&doc, "Before boundary", None, None, false).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        let notify_pos = result.find("> Before boundary").unwrap();
        let boundary_pos = result.find("<!-- agent:boundary:abc123 -->").unwrap();
        assert!(notify_pos < boundary_pos, "notification should be before boundary");
    }

    #[test]
    fn multiline_message() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        );

        run(&doc, "Line one\nLine two\nLine three", None, None, false).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("> Line one"));
        assert!(result.contains("> Line two"));
        assert!(result.contains("> Line three"));
    }

    #[test]
    fn snapshot_updated_after_notify() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        );

        run(&doc, "Snapshot test", None, None, false).unwrap();

        let snap_path = dir.path().join(snapshot::path_for(&doc).unwrap());
        let snap = std::fs::read_to_string(snap_path).unwrap();
        assert!(snap.contains("> Snapshot test"));
    }
}
