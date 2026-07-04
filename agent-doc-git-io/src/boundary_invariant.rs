use anyhow::Result;
use std::path::Path;

pub trait BoundaryInvariantEffects {
    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()>;
    fn log_op(&self, file: &Path, message: &str);
}

/// Enforce the single-boundary invariant on the just-committed HEAD artifact.
///
/// The pre-stage snapshot collapse should already guarantee this, but a
/// previously-accreted blob is caught here and self-healed with a binary-owned
/// follow-up collapse commit. This never races the live editor: it re-collapses
/// committed content, not the editor buffer.
pub fn enforce_committed_single_boundary_invariant(
    effects: &impl BoundaryInvariantEffects,
    file: &Path,
    git_root: &Path,
    resolved: &Path,
) {
    let Ok(Some(head_blob)) = crate::revision::show_head(file) else {
        return;
    };
    let boundary_count = head_blob
        .matches(agent_doc_element_boundary::boundary::BOUNDARY_PREFIX)
        .count();
    if boundary_count <= 1 {
        return;
    }
    eprintln!(
        "[commit] boundary_invariant_violation: committed HEAD carries {} agent:boundary markers (expected 1) - self-healing collapse",
        boundary_count
    );
    effects.log_op(
        file,
        &format!(
            "boundary_invariant_violation phase=post_commit file={} committed_boundaries={}",
            file.display(),
            boundary_count
        ),
    );
    let collapsed = agent_doc_template::reposition_boundary_to_end_clean(&head_blob);
    let collapsed_count = collapsed
        .matches(agent_doc_element_boundary::boundary::BOUNDARY_PREFIX)
        .count();
    if collapsed == head_blob || collapsed_count > 1 {
        eprintln!(
            "[commit] boundary_invariant self-heal could not reduce to a single boundary (still {}); leaving for next cycle",
            collapsed_count
        );
        return;
    }
    if let Err(e) = effects.save_snapshot(file, &collapsed) {
        eprintln!(
            "[commit] boundary_invariant self-heal snapshot save failed: {} (non-fatal)",
            e
        );
    }
    match crate::transaction::stage_and_commit_once(
        git_root,
        resolved,
        Some(collapsed.as_str()),
        "agent-doc: collapse accreted agent:boundary markers (#boundaryaccum1)",
    ) {
        Ok(_) => {
            eprintln!("[commit] boundary_invariant self-heal collapse committed");
            effects.log_op(
                file,
                &format!(
                    "boundary_invariant_selfheal_committed file={}",
                    file.display()
                ),
            );
        }
        Err(_) => {
            eprintln!("[commit] boundary_invariant self-heal collapse failed (non-fatal)");
        }
    }
}
