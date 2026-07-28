use anyhow::{Context, Result};
use std::path::Path;

use agent_doc_element_boundary::boundary::{BOUNDARY_PREFIX, insert};

/// CLI entry point: insert a boundary marker and print the UUID.
pub fn run(file: &Path, component: Option<&str>) -> Result<()> {
    let component_name = component.unwrap_or("exchange");

    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "boundary_insert",
    )?;
    let stale_count = content.matches(BOUNDARY_PREFIX).count();

    let (id, updated) = insert(&content, component_name)?;

    if stale_count > 0 {
        eprintln!(
            "[boundary] removed {} stale boundary marker(s) before inserting new one",
            stale_count
        );
    }

    agent_doc_document_realtime_io::atomic_write_through_authority(file, &updated)
        .with_context(|| format!("failed to write {}", file.display()))?;

    signal_editor_refresh(file);

    println!("{}", id);
    Ok(())
}

/// Signal every registered Lazily editor replica to refresh VCS state.
fn signal_editor_refresh(file: &Path) {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if let Some(root) = agent_doc_fs::find_project_root(&canonical) {
        for registration in
            agent_doc_crdt_relay_io::reliable_sync_editor_registrations_for_file(&canonical)
        {
            let _ = agent_doc_ipc_io::send_vcs_refresh_to_editor(
                &root,
                registration.pid,
                &registration.editor_id,
                &canonical.to_string_lossy(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use agent_doc_element_boundary::boundary::BOUNDARY_PREFIX;

    use super::*;

    #[test]
    fn run_writes_marker_without_rewriting_non_boundary_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("doc.md");
        let original = "<!-- agent:exchange -->\ncontent\n<!-- /agent:exchange -->\n";
        std::fs::write(&file, original).unwrap();

        run(&file, Some("exchange")).unwrap();

        let current = std::fs::read_to_string(&file).unwrap();
        assert!(
            current.contains(BOUNDARY_PREFIX),
            "boundary command should still prepare the working document"
        );
        let marker_line = current
            .lines()
            .find(|line| line.contains(BOUNDARY_PREFIX))
            .expect("boundary marker line");
        let without_marker = current
            .replace(marker_line, "")
            .replace("\n\n<!-- /agent:exchange -->", "\n<!-- /agent:exchange -->");
        assert_eq!(
            without_marker, original,
            "marker-only boundary setup must not rewrite non-boundary content"
        );
    }
}
