//! # Module: notify
//!
//! ## Spec
//! - Appends a blockquote notification to a template document's exchange component.
//! - Notification format is a `> **[NOTIFY from <source>]** (<timestamp>)` blockquote.
//! - Appends before the boundary marker if one exists, otherwise at the end of the
//!   exchange component content.
//! - After appending, writes the document atomically, updates the snapshot, and
//!   optionally commits.
//! - `--pending-add` / `--pending-add-gated` adds items to the document's `agent:pending`
//!   component. Auto-creates the component if absent (after exchange close tag).
//!   `--no-create-pending` opts out of auto-creation.
//! - When only pending ops are requested (no message), the exchange blockquote is skipped
//!   (pending-only mode).
//! - Pending errors become no-ops when `--no-create-pending` is set and the component is absent.
//!
//! ## Agentic Contracts
//! - `run(file, message, source, affects, commit, pending_add, pending_add_gated, no_create)`
//!   — returns `Err` if the file is missing, message is required but absent, or the exchange
//!   component is not found (when a message is provided).
//! - Pending-only mode (no message + pending items): skips exchange, updates snapshot, commits.
//! - Snapshot is always updated after a successful write.
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
//! - pending_add_auto_creates_component: no pending component → auto-created, item added
//! - pending_add_to_existing_component: existing pending component → item added at the beginning
//! - pending_add_gated_creates_gated_item: `--pending-add-gated` → item with `[/]` state
//! - no_create_pending_skips_when_absent: `--no-create-pending` + absent → no-op
//! - pending_add_with_message_writes_both: pending + message → both written
//! - pending_only_updates_snapshot: pending-only → snapshot updated
//! - auto_create_inserts_after_exchange: component placed after exchange close tag
//! - message_required_without_pending: no message, no pending → `Err`

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

use crate::{component, component::is_backlog_component, pending, pending_cmd, snapshot};

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

/// Ensure the document has an `agent:pending` component; create one if absent.
/// Returns `Ok(true)` if a component was created, `Ok(false)` if it already existed.
/// Under `no_create`, returns `Err` if absent.
fn ensure_pending_component(file: &Path, no_create: bool) -> Result<bool> {
    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let components = component::parse(&doc)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;

    if components.iter().any(|c| is_backlog_component(&c.name)) {
        return Ok(false);
    }

    if no_create {
        bail!("document has no pending component and --no-create-pending was set");
    }

    // Insert before ## Assistant / ## Pending / end-of-file, whichever comes first.
    // Prefer inserting before the exchange close tag if it exists, otherwise at end.
    let insert_pos = find_pending_insert_position(&doc, &components);

    let pending_block = "\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n";
    let mut new_doc = String::with_capacity(doc.len() + pending_block.len());
    new_doc.push_str(&doc[..insert_pos]);
    new_doc.push_str(pending_block);
    new_doc.push_str(&doc[insert_pos..]);

    std::fs::write(file, &new_doc)?;
    eprintln!(
        "[notify] auto-created agent:pending component in {}",
        file.display()
    );
    Ok(true)
}

/// Find the best position to insert a pending component.
fn find_pending_insert_position(doc: &str, components: &[component::Component]) -> usize {
    // After the exchange close tag if one exists
    if let Some(exchange) = components.iter().find(|c| c.name == "exchange") {
        return exchange.close_end;
    }
    // Otherwise at end of document
    doc.len()
}

/// Append a notification to a document's exchange component, update the snapshot,
/// and optionally commit. When `pending_add` items are present, adds them to the
/// pending component (auto-creating it if needed) and skips the exchange blockquote.
#[allow(clippy::too_many_arguments)]
pub fn run(
    file: &Path,
    message: Option<&str>,
    source: Option<&str>,
    affects: Option<&str>,
    commit: bool,
    pending_add: &[String],
    pending_add_gated: &[String],
    no_create_pending: bool,
) -> Result<()> {
    if !file.exists() {
        bail!("file not found: {}", file.display());
    }

    let has_pending_ops = !pending_add.is_empty() || !pending_add_gated.is_empty();
    let pending_only = has_pending_ops && message.is_none();

    // --- Pending operations ---
    if has_pending_ops {
        // Ensure pending component exists (auto-create unless opted out)
        match ensure_pending_component(file, no_create_pending) {
            Ok(_) => {}
            Err(e) => {
                // Under auto-create mode, errors become no-ops
                if no_create_pending {
                    eprintln!("[notify] skipping pending ops: {}", e);
                    if pending_only {
                        return Ok(());
                    }
                } else {
                    return Err(e);
                }
            }
        }

        // Add pending items
        let doc_id = pending_cmd::doc_id_for(file);
        for item in pending_add.iter().rev() {
            match add_pending_item(file, item, &doc_id, false) {
                Ok(id) => eprintln!("[notify] added pending item #{}", id),
                Err(e) => eprintln!("[notify] pending-add no-op: {}", e),
            }
        }
        for item in pending_add_gated.iter().rev() {
            match add_pending_item(file, item, &doc_id, true) {
                Ok(id) => eprintln!("[notify] added gated pending item #{}", id),
                Err(e) => eprintln!("[notify] pending-add-gated no-op: {}", e),
            }
        }
    }

    // --- Exchange notification (skip in pending-only mode) ---
    if pending_only {
        // Still update snapshot and commit
        let doc = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        save_snapshot(file, &doc)?;

        if commit {
            crate::git::commit(file)?;
        }
        return Ok(());
    }

    let message = message
        .ok_or_else(|| anyhow::anyhow!("message is required when --pending-add is not used"))?;

    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let components = component::parse(&doc)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;

    let exchange = components
        .iter()
        .find(|c| c.name == "exchange")
        .ok_or_else(|| anyhow::anyhow!("component 'exchange' not found in {}", file.display()))?;

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

    save_snapshot(file, &new_doc)?;

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

/// Add a single pending item to a document's pending component.
/// Returns the assigned hash ID on success.
fn add_pending_item(file: &Path, item: &str, doc_id: &str, gated: bool) -> Result<String> {
    let content = std::fs::read_to_string(file).context("failed to read document")?;
    let components = component::parse(&content).context("failed to parse components")?;
    let comp = components
        .into_iter()
        .find(|c| is_backlog_component(&c.name))
        .context("document has no backlog/pending component")?;
    let existing = &content[comp.open_end..comp.close_start];
    let (new_content, id) = pending::op_add(existing, item, doc_id, gated)?;
    let new_doc = comp.replace_content(&content, &new_content);
    std::fs::write(file, &new_doc)?;
    Ok(id)
}

/// Save snapshot relative to project root for thread safety.
fn save_snapshot(file: &Path, doc: &str) -> Result<()> {
    let snap_rel = snapshot::path_for(file)?;
    if let Some(root) = find_project_root(file) {
        let snap_abs = root.join(&snap_rel);
        if let Some(snap_parent) = snap_abs.parent() {
            std::fs::create_dir_all(snap_parent)
                .with_context(|| format!("failed to create snapshot dir for {}", file.display()))?;
        }
        std::fs::write(&snap_abs, doc)
            .with_context(|| format!("failed to update snapshot for {}", file.display()))?;
    } else {
        snapshot::save(file, doc)
            .with_context(|| format!("failed to update snapshot for {}", file.display()))?;
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

        run(
            &doc,
            Some("Hello world"),
            None,
            None,
            false,
            &[],
            &[],
            false,
        )
        .unwrap();

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

        run(
            &doc,
            Some("Update available"),
            Some("build-monitor"),
            None,
            false,
            &[],
            &[],
            false,
        )
        .unwrap();

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

        run(
            &doc,
            Some("Something happened"),
            None,
            None,
            false,
            &[],
            &[],
            false,
        )
        .unwrap();

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

        run(
            &doc,
            Some("API changed"),
            None,
            Some("integration tests"),
            false,
            &[],
            &[],
            false,
        )
        .unwrap();

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

        run(&doc, Some("Just FYI"), None, None, false, &[], &[], false).unwrap();

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

        run(
            &doc,
            Some("Before boundary"),
            None,
            None,
            false,
            &[],
            &[],
            false,
        )
        .unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        let notify_pos = result.find("> Before boundary").unwrap();
        let boundary_pos = result.find("<!-- agent:boundary:abc123 -->").unwrap();
        assert!(
            notify_pos < boundary_pos,
            "notification should be before boundary"
        );
    }

    #[test]
    fn multiline_message() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        );

        run(
            &doc,
            Some("Line one\nLine two\nLine three"),
            None,
            None,
            false,
            &[],
            &[],
            false,
        )
        .unwrap();

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

        run(
            &doc,
            Some("Snapshot test"),
            None,
            None,
            false,
            &[],
            &[],
            false,
        )
        .unwrap();

        let snap_path = dir.path().join(snapshot::path_for(&doc).unwrap());
        let snap = std::fs::read_to_string(snap_path).unwrap();
        assert!(snap.contains("> Snapshot test"));
    }

    // --- Pending-add tests ---

    #[test]
    fn pending_add_auto_creates_component() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\nContent\n<!-- /agent:exchange -->\n",
        );

        let items = vec!["implement feature X".to_string()];
        run(&doc, None, None, None, false, &items, &[], false).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("<!-- agent:backlog"));
        assert!(result.contains("<!-- /agent:backlog -->"));
        assert!(result.contains("implement feature X"));
        assert!(result.contains("[ ]"));
        assert!(result.contains("[#"));
        // Pending-only: no exchange blockquote
        assert!(!result.contains("> **[NOTIFY"));
    }

    #[test]
    fn pending_add_to_existing_component() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\nContent\n<!-- /agent:exchange -->\n<!-- agent:pending -->\n- [ ] [#abcd] existing item\n<!-- /agent:pending -->\n",
        );

        let items = vec!["new item".to_string()];
        run(&doc, None, None, None, false, &items, &[], false).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        let pending = result
            .split("<!-- agent:pending -->\n")
            .nth(1)
            .and_then(|rest| rest.split("\n<!-- /agent:pending -->").next())
            .unwrap();
        let lines: Vec<&str> = pending.lines().collect();
        assert!(
            lines[0].contains("new item"),
            "expected new item first, got: {}",
            pending
        );
        assert!(
            lines[1].contains("existing item"),
            "expected existing item second, got: {}",
            pending
        );
        assert!(result.matches("[#").count() >= 2);
    }

    #[test]
    fn pending_add_multiple_items_keep_sequence_order_at_front() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\nContent\n<!-- /agent:exchange -->\n<!-- agent:pending -->\n- [ ] [#abcd] existing item\n<!-- /agent:pending -->\n",
        );

        let items = vec!["first new".to_string(), "second new".to_string()];
        run(&doc, None, None, None, false, &items, &[], false).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        let pending = result
            .split("<!-- agent:pending -->\n")
            .nth(1)
            .and_then(|rest| rest.split("\n<!-- /agent:pending -->").next())
            .unwrap();
        let lines: Vec<&str> = pending.lines().collect();
        assert!(
            lines[0].contains("first new"),
            "expected first new item first, got: {}",
            pending
        );
        assert!(
            lines[1].contains("second new"),
            "expected second new item second, got: {}",
            pending
        );
        assert!(
            lines[2].contains("existing item"),
            "expected existing item after new items, got: {}",
            pending
        );
    }

    #[test]
    fn pending_add_gated_creates_gated_item() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\nContent\n<!-- /agent:exchange -->\n",
        );

        let gated = vec!["gated feature".to_string()];
        run(&doc, None, None, None, false, &[], &gated, false).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("[/]"));
        assert!(result.contains("gated feature"));
    }

    #[test]
    fn no_create_pending_skips_when_absent() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\nContent\n<!-- /agent:exchange -->\n",
        );

        let items = vec!["ignored item".to_string()];
        // With --no-create-pending and pending-only, should return Ok (no-op)
        run(&doc, None, None, None, false, &items, &[], true).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        // Nothing changed — no pending component created, no blockquote
        assert!(!result.contains("agent:pending"));
        assert!(!result.contains("ignored item"));
    }

    #[test]
    fn pending_add_with_message_writes_both() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\nContent\n<!-- /agent:exchange -->\n",
        );

        let items = vec!["new pending item".to_string()];
        run(
            &doc,
            Some("Notification msg"),
            None,
            None,
            false,
            &items,
            &[],
            false,
        )
        .unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        // Both pending item and exchange blockquote should be present
        assert!(result.contains("new pending item"));
        assert!(result.contains("> **[NOTIFY]**"));
        assert!(result.contains("> Notification msg"));
    }

    #[test]
    fn pending_only_updates_snapshot() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\nContent\n<!-- /agent:exchange -->\n",
        );

        let items = vec!["snapshot item".to_string()];
        run(&doc, None, None, None, false, &items, &[], false).unwrap();

        let snap_path = dir.path().join(snapshot::path_for(&doc).unwrap());
        let snap = std::fs::read_to_string(snap_path).unwrap();
        assert!(snap.contains("snapshot item"));
    }

    #[test]
    fn auto_create_inserts_after_exchange() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "# Title\n\n<!-- agent:exchange patch=append -->\nContent\n<!-- /agent:exchange -->\n\n## Footer\n",
        );

        let items = vec!["test item".to_string()];
        run(&doc, None, None, None, false, &items, &[], false).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        let exchange_close = result.find("<!-- /agent:exchange -->").unwrap();
        let backlog_open = result.find("<!-- agent:backlog").unwrap();
        assert!(
            backlog_open > exchange_close,
            "backlog should be after exchange close"
        );
    }

    #[test]
    fn message_required_without_pending() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        );

        let result = run(&doc, None, None, None, false, &[], &[], false);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("message is required")
        );
    }
}
