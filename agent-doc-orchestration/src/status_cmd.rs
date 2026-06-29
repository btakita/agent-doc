//! CLI-driven status component mutations for `agent-doc write --status`.
//!
//! Replaces the content of `<!-- agent:status -->` with the provided text.

use anyhow::{Context, Result};
use std::path::Path;

use agent_doc_element::element;

fn find_status_component(file: &Path) -> Result<(String, element::Component)> {
    let content = std::fs::read_to_string(file).context("failed to read document")?;
    let components = element::parse(&content).context("failed to parse components")?;
    let comp = components
        .into_iter()
        .find(|c| c.name == "status")
        .context("document has no status component")?;
    Ok((content, comp))
}

/// Replace the status component content with the provided text.
pub fn set(file: &Path, text: &str) -> Result<()> {
    set_with_options(file, text, false)
}

pub fn set_with_options(file: &Path, text: &str, force_disk: bool) -> Result<()> {
    let (full_content, comp) = find_status_component(file)?;
    let new_content = format!("\n{}\n", text);
    let new_doc = comp.replace_content(&full_content, &new_content);
    if force_disk {
        std::fs::write(file, &new_doc)
            .with_context(|| format!("status_set: failed to write {}", file.display()))?;
        crate::write::record_document_write_provenance(file, &new_doc);
        crate::ops_log::log_op(
            file,
            &format!(
                "status_set_writeback file={} transport=disk_force reason=force_disk len={} hash={}",
                file.display(),
                new_doc.len(),
                crate::ops_log::content_hash(&new_doc)
            ),
        );
        return Ok(());
    }
    // #fccaudit: route the status mutation through the editor-IPC converge gate
    // so it never writes the session document behind a live JB editor buffer
    // (File Cache Conflict). With no listener and no live editor sidecar, the
    // converger uses the guarded detached-disk authority path.
    crate::write::converge_or_disk_write(file, &full_content, &new_doc, "status_set")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_writes_status_detached_disk_without_listener() {
        // Realtime cutover: `set` routes through `converge_or_disk_write`. With
        // no editor IPC listener and no live editor sidecar, the current file is
        // the guarded detached-disk realtime replica.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(
            &doc,
            concat!(
                "## Status\n\n",
                "<!-- agent:status patch=replace -->\n",
                "old status\n",
                "<!-- /agent:status -->\n",
            ),
        )
        .unwrap();

        set(&doc, "new status").unwrap();

        let on_disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            on_disk.contains("new status"),
            "status should be written through detached disk when no editor owns the doc: {on_disk}"
        );
        assert!(
            !on_disk.contains("old status"),
            "old status should be replaced: {on_disk}"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("status_set_writeback")
                && log.contains("transport=disk_detached")
                && log.contains("reason=no_listener"),
            "detached status write should be attributable:\n{log}"
        );
    }

    #[test]
    fn set_blocks_status_write_with_live_editor_sidecar() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");
        let source = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "old status\n",
            "<!-- /agent:status -->\n",
        );
        std::fs::write(&doc, source).unwrap();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc.to_string_lossy(),
            &format!("{source}\noperator typed text\n"),
            "jetbrains-new",
            "jetbrains",
            "0.2.197",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let err = set(&doc, "new status").unwrap_err().to_string();
        assert!(
            err.contains("no_listener"),
            "status write with a live editor sidecar must fail closed without delivery proof: {err}"
        );

        let on_disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !on_disk.contains("new status"),
            "status should not be written behind a live editor sidecar: {on_disk}"
        );
        assert!(
            on_disk.contains("old status"),
            "old status should remain unchanged: {on_disk}"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("transport=disk_detached"),
            "live editor sidecar must block detached disk write:\n{log}"
        );
    }

    #[test]
    fn force_disk_set_writes_status_without_listener() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        std::fs::write(
            &doc,
            concat!(
                "## Status\n\n",
                "<!-- agent:status patch=replace -->\n",
                "old status\n",
                "<!-- /agent:status -->\n",
            ),
        )
        .unwrap();

        set_with_options(&doc, "new status", true)
            .expect("force-disk status update should write without listener");

        let on_disk = std::fs::read_to_string(&doc).unwrap();
        assert!(on_disk.contains("new status"));
        assert!(!on_disk.contains("old status"));
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("status_set_writeback")
                && log.contains("transport=disk_force")
                && log.contains("reason=force_disk"),
            "force-disk status write should be attributable:\n{log}"
        );
    }
}
