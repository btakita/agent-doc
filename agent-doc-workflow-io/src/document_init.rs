//! Document initialization sequencing for agent-doc.

use anyhow::Result;
use std::path::Path;

/// Perform initialization for a document entering the agent-doc lifecycle.
///
/// This composes the focused ledger/file adapters and leaves the concrete
/// commit implementation injected by the caller.
pub fn ensure_initialized(
    doc: &Path,
    commit: impl FnOnce(&Path) -> Result<bool>,
    logger: impl FnMut(&Path, &str),
) -> Result<bool> {
    let uuid_assigned = ensure_session_uuid(doc)?;
    let migrated = agent_doc_snapshot_io::try_migrate_renamed(doc)?;
    let snapshot_created = if migrated {
        false
    } else {
        agent_doc_snapshot_io::ensure_initial_snapshot(
            doc,
            agent_doc_element_exchange::strip_exchange_content,
            logger,
        )?
    };
    if snapshot_created {
        ensure_git_tracked_with_commit(doc, commit)?;
    }
    Ok(uuid_assigned || migrated || snapshot_created)
}

/// Assign a session UUID to an agent-doc formatted file that lacks one.
pub fn ensure_session_uuid(doc: &Path) -> Result<bool> {
    let result = agent_doc_frontmatter_io::session::ensure_session_uuid_for_formatted_file(doc)?;
    if result.assigned {
        eprintln!(
            "[init] assigning session UUID to {} (has format but no session)",
            doc.display()
        );
        if let Some(session_id) = result.session_id {
            eprintln!("[init] assigned session UUID: {}", session_id);
        }
    }
    Ok(result.assigned)
}

/// Stage an untracked file and run the caller's commit implementation.
pub fn ensure_git_tracked_with_commit(
    doc: &Path,
    commit: impl FnOnce(&Path) -> Result<bool>,
) -> Result<()> {
    if agent_doc_git_io::status::is_in_git_repo(doc) && !agent_doc_git_io::status::is_tracked(doc)?
    {
        eprintln!("[init] file is untracked — staging with git add");
        agent_doc_git_io::status::add(doc)?;
    }

    if let Err(e) = commit(doc) {
        eprintln!("[init] warning: failed to commit after init: {e}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::fs;
    use std::rc::Rc;
    use tempfile::TempDir;

    fn noop_log(_: &Path, _: &str) {}

    #[test]
    fn ensure_initialized_assigns_uuid_and_creates_snapshot() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        fs::write(
            &doc,
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\nBody\n",
        )
        .unwrap();

        let committed = Rc::new(Cell::new(false));
        let did_commit = committed.clone();
        let initialized = ensure_initialized(
            &doc,
            move |_| {
                did_commit.set(true);
                Ok(false)
            },
            noop_log,
        )
        .unwrap();

        assert!(initialized);
        assert!(committed.get());
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("agent_doc_session:"));
        assert!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn ensure_initialized_does_not_commit_when_snapshot_exists() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        fs::write(
            &doc,
            "---\nagent_doc_session: existing\nagent_doc_format: template\n---\n\nBody\n",
        )
        .unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(&doc, "existing snapshot", noop_log)
            .unwrap();

        let committed = Rc::new(Cell::new(false));
        let did_commit = committed.clone();
        let initialized = ensure_initialized(
            &doc,
            move |_| {
                did_commit.set(true);
                Ok(false)
            },
            noop_log,
        )
        .unwrap();

        assert!(!initialized);
        assert!(!committed.get());
    }
}
