use anyhow::Result;
use std::path::{Path, PathBuf};

use agent_doc_document::transient_markers::{
    normalize_post_commit_re_heading_drift, normalize_transient_agent_doc_markers,
    repair_stale_agent_response_collapse_doc,
};

pub trait TransientCleanupEffects {
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()>;
    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()>;
    fn save_document_crdt(&self, file: &Path, legacy_state: &[u8], markdown: &str) -> Result<()>;
    fn editor_attached(&self, file: &Path) -> bool;
    fn log_op(&self, file: &Path, message: &str);
    fn project_root_containing(&self, file: &Path) -> Option<PathBuf>;
    fn ipc_listener_active(&self, project_root: &Path) -> bool;
    fn send_vcs_refresh(&self, project_root: &Path) -> Result<bool>;
    fn write_vcs_refresh_signal(&self, signal_file: &Path) -> Result<()>;
}

pub fn vcs_refresh_signal_path(
    effects: &impl TransientCleanupEffects,
    file: &Path,
) -> Option<PathBuf> {
    let canonical = file.canonicalize().ok()?;
    let project_root = effects
        .project_root_containing(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    let signal_file = project_root.join(".agent-doc/patches/vcs-refresh.signal");
    signal_file.parent().filter(|p| p.exists())?;
    Some(signal_file)
}

pub fn signal_vcs_refresh(effects: &impl TransientCleanupEffects, file: &Path) -> Option<bool> {
    let signal_file = vcs_refresh_signal_path(effects, file)?;
    match effects.write_vcs_refresh_signal(&signal_file) {
        Ok(()) => {
            eprintln!("[commit] VCS refresh signal written");
            Some(true)
        }
        Err(e) => {
            eprintln!("[commit] VCS refresh signal failed: {} (non-fatal)", e);
            Some(false)
        }
    }
}

pub fn refresh_live_closeout_sidecars(
    effects: &impl TransientCleanupEffects,
    file: &Path,
    committed_doc: &str,
    signal_editor_refresh: bool,
) -> Result<Option<bool>> {
    if agent_doc_frontmatter::frontmatter::content_uses_crdt_write(committed_doc) {
        if effects.editor_attached(file) {
            effects.log_op(
                file,
                &format!(
                    "crdt_checkpoint_skip file={} source=commit reason=editor_authority_owns_sidecar_lock len={}",
                    file.display(),
                    committed_doc.len()
                ),
            );
        } else {
            let crdt = agent_doc_merge::crdt::CrdtDoc::from_text(committed_doc).encode_state();
            effects.save_document_crdt(file, &crdt, committed_doc)?;
        }
    }

    if !signal_editor_refresh {
        return Ok(None);
    }

    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(root) = effects.project_root_containing(&canonical) else {
        return Ok(None);
    };

    if effects.ipc_listener_active(&root) && effects.send_vcs_refresh(&root).unwrap_or(false) {
        return Ok(Some(true));
    }

    let Some(signal_file) = vcs_refresh_signal_path(effects, file) else {
        return Ok(None);
    };
    match effects.write_vcs_refresh_signal(&signal_file) {
        Ok(()) => Ok(Some(true)),
        Err(e) => {
            eprintln!(
                "[commit] VCS refresh signal failed during closeout sidecar refresh: {}",
                e
            );
            Ok(Some(false))
        }
    }
}

pub fn repair_stale_agent_response_collapse_worktree(
    effects: &impl TransientCleanupEffects,
    file: &Path,
    head_doc: &str,
    file_content: &str,
) -> Result<Option<String>> {
    let Some(repaired) = repair_stale_agent_response_collapse_doc(head_doc, file_content) else {
        return Ok(None);
    };

    effects.atomic_write(file, &repaired)?;
    if repaired == head_doc {
        effects.save_snapshot(file, head_doc)?;
    }
    refresh_live_closeout_sidecars(effects, file, &repaired, true)?;
    effects.log_op(
        file,
        &format!(
            "stale_agent_response_collapse_cleanup file={} basis=head preserved_local_drift={}",
            file.display(),
            repaired != head_doc
        ),
    );
    Ok(Some(repaired))
}

pub fn repair_clean_head_if_only_transient_worktree_drift(
    effects: &impl TransientCleanupEffects,
    file: &Path,
    file_content: &str,
) -> Result<Option<(Option<String>, String)>> {
    let Some(head_doc) = crate::revision::show_head(file)? else {
        return Ok(None);
    };
    if file_content == head_doc {
        return Ok(None);
    }
    if let Some(repaired) =
        repair_stale_agent_response_collapse_worktree(effects, file, &head_doc, file_content)?
    {
        return Ok(Some((Some(head_doc.clone()), repaired)));
    }
    if let Some(repaired) =
        agent_doc_document::write_normalization::reconcile_postcommit_exchange_to_head(
            file_content,
            &head_doc,
        )
    {
        effects.atomic_write(file, &repaired)?;
        if repaired == head_doc {
            effects.save_snapshot(file, &head_doc)?;
        }
        refresh_live_closeout_sidecars(effects, file, &repaired, true)?;
        effects.log_op(
            file,
            &format!(
                "postcommit_exchange_reconcile_to_head file={} basis=head preserved_non_exchange_drift={}",
                file.display(),
                repaired != head_doc
            ),
        );
        return Ok(Some((Some(head_doc.clone()), repaired)));
    }
    if normalize_transient_agent_doc_markers(file_content)
        != normalize_transient_agent_doc_markers(&head_doc)
        && normalize_post_commit_re_heading_drift(file_content)
            != normalize_post_commit_re_heading_drift(&head_doc)
    {
        return Ok(None);
    }

    effects.atomic_write(file, &head_doc)?;
    effects.save_snapshot(file, &head_doc)?;
    refresh_live_closeout_sidecars(effects, file, &head_doc, true)?;
    effects.log_op(
        file,
        &format!("transient_cleanup file={} basis=head", file.display()),
    );
    Ok(Some((Some(head_doc.clone()), head_doc)))
}
