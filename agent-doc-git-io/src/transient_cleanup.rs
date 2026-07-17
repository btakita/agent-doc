use anyhow::Result;
use std::path::Path;

use agent_doc_document::transient_markers::{
    normalize_post_commit_re_heading_drift, normalize_transient_agent_doc_markers,
    repair_stale_agent_response_collapse_doc,
};

pub trait TransientCleanupEffects {
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()>;
    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()>;
    fn log_op(&self, file: &Path, message: &str);
    fn send_vcs_refresh(&self, file: &Path) -> Result<bool>;
}

pub fn signal_vcs_refresh(effects: &impl TransientCleanupEffects, file: &Path) -> Option<bool> {
    match effects.send_vcs_refresh(file) {
        Ok(sent) => {
            eprintln!("[commit] VCS refresh delivered through editor endpoints");
            Some(sent)
        }
        Err(e) => {
            eprintln!("[commit] VCS refresh signal failed: {} (non-fatal)", e);
            Some(false)
        }
    }
}

pub fn signal_live_closeout_refresh(
    effects: &impl TransientCleanupEffects,
    file: &Path,
    signal_editor_refresh: bool,
) -> Result<Option<bool>> {
    if !signal_editor_refresh {
        return Ok(None);
    }

    effects.send_vcs_refresh(file).map(Some)
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
    signal_live_closeout_refresh(effects, file, true)?;
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
        signal_live_closeout_refresh(effects, file, true)?;
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
    signal_live_closeout_refresh(effects, file, true)?;
    effects.log_op(
        file,
        &format!("transient_cleanup file={} basis=head", file.display()),
    );
    Ok(Some((Some(head_doc.clone()), head_doc)))
}
