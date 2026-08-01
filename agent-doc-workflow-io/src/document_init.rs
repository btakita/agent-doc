//! Document initialization sequencing for agent-doc.

use anyhow::Result;
use std::path::Path;

fn migrate_renamed_via_controller(doc: &Path) -> Result<bool> {
    let Some(transition) = agent_doc_snapshot_io::detect_document_path_transition(doc)? else {
        return Ok(false);
    };
    eprintln!(
        "[init] detected document path transition: {} → {}",
        transition.old_path.display(),
        transition.new_path.display(),
    );
    let observation =
        agent_doc_controller_io::project_controller::new_document_path_transition_observation(
            &transition.old_path,
            &transition.new_path,
            &format!("workflow-init:{}", std::process::id()),
        );
    let receipt = agent_doc_controller_io::project_controller::observe_document_path_transition(
        &transition.project_root,
        &observation,
    )?;
    anyhow::ensure!(
        receipt.converged,
        "document path transition remains {:?}: {}",
        receipt.phase,
        receipt.error.as_deref().unwrap_or("retry pending"),
    );
    Ok(receipt.state_events_rekeyed > 0
        || receipt.actor_rekeyed
        || receipt.sessions_rekeyed > 0
        || receipt.relay_hub_moved)
}

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
    let migrated = migrate_renamed_via_controller(doc)?;
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

/// Initialize from the caller's editor-authoritative document projection.
///
/// The returned string is the exact projection that should be used by the
/// caller for subsequent template/session resolution. No content is reopened
/// from disk. This keeps initialization, validation, pane ownership, and the
/// eventual commit on one canonical value.
pub fn ensure_initialized_with_content(
    doc: &Path,
    content: &str,
    mut write: impl FnMut(&Path, &str) -> Result<()>,
    commit: impl FnOnce(&Path) -> Result<bool>,
    logger: impl FnMut(&Path, &str),
) -> Result<(String, bool)> {
    let (frontmatter, _) = agent_doc_frontmatter::frontmatter::parse(content)?;
    let (resolved, uuid_assigned) = if frontmatter.format.is_some() && frontmatter.session.is_none()
    {
        let (updated, session_id) = agent_doc_frontmatter::frontmatter::ensure_session(content)?;
        eprintln!(
            "[init] assigning session UUID to {} from current authority projection",
            doc.display()
        );
        eprintln!("[init] assigned session UUID: {}", session_id);
        write(doc, &updated)?;
        (updated, true)
    } else {
        (content.to_string(), false)
    };

    let migrated = migrate_renamed_via_controller(doc)?;
    let snapshot_created = if migrated {
        false
    } else {
        agent_doc_snapshot_io::ensure_initial_snapshot_with_content(
            doc,
            &resolved,
            agent_doc_element_exchange::strip_exchange_content,
            logger,
        )?
    };
    if snapshot_created {
        // The commit effect owns native-save convergence and exact staging.
        // Pre-staging here could capture stale disk beneath a live editor.
        if let Err(error) = commit(doc) {
            eprintln!("[init] warning: failed to commit after init: {error}");
        }
    }
    Ok((resolved, uuid_assigned || migrated || snapshot_created))
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

    #[test]
    fn authority_projection_drives_snapshot_instead_of_stale_disk() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let stale_disk =
            "---\nagent_doc_session: stale-disk\nagent_doc_format: template\n---\n\nDisk\n";
        let editor =
            "---\nagent_doc_session: live-editor\nagent_doc_format: template\n---\n\nEditor\n";
        fs::write(&doc, stale_disk).unwrap();

        let wrote = Rc::new(Cell::new(false));
        let did_write = wrote.clone();
        let committed = Rc::new(Cell::new(false));
        let did_commit = committed.clone();
        let (resolved, initialized) = ensure_initialized_with_content(
            &doc,
            editor,
            move |_, _| {
                did_write.set(true);
                Ok(())
            },
            move |_| {
                did_commit.set(true);
                Ok(true)
            },
            noop_log,
        )
        .unwrap();

        assert_eq!(resolved, editor);
        assert!(initialized);
        assert!(
            !wrote.get(),
            "existing editor session must remain unchanged"
        );
        assert!(committed.get());
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .unwrap(),
            editor
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            stale_disk,
            "initialization must not overwrite the editor's unsaved buffer through disk"
        );
    }
}
