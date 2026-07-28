use agent_doc_document::commit_normalization::canonicalize_answered_prompt_prefixes;
use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryRepositionDelivery {
    Delivered,
    RetainedForRetry,
    Unavailable,
}

impl BoundaryRepositionDelivery {
    const fn marks_visible_delivery(self) -> bool {
        matches!(self, Self::Delivered)
    }
}

pub trait BoundaryRepositionEffects {
    fn active_run(&self, file: &Path) -> bool;
    fn load_snapshot(&self, file: &Path) -> Result<Option<String>>;
    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()>;
    fn ipc_listener_active(&self, file: &Path) -> bool;
    fn read_to_string(&self, file: &Path) -> Result<String>;
    fn request_editor_boundary_reposition(
        &self,
        file: &Path,
        committed_boundary_id: Option<&str>,
        normalize_prefix_lines: &[String],
    ) -> Result<BoundaryRepositionDelivery>;
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()>;
}

/// Reposition boundary in snapshot AND working tree deterministically.
///
/// After commit, moves the boundary to the end of exchange in both the
/// snapshot and the working-tree file. The active-run guard prevents racing a
/// concurrent `agent-doc write`: in-flight runs skip only the working-tree
/// rewrite because the snapshot is binary-owned and is what git stages.
///
/// Returns true if the snapshot OR working tree content changed.
pub fn reposition_boundary_in_snapshot(
    effects: &impl BoundaryRepositionEffects,
    file: &Path,
) -> bool {
    let active_run = effects.active_run(file);
    let mut changed = false;

    if let Ok(Some(snap_content)) = effects.load_snapshot(file) {
        let prompt_canonicalized = canonicalize_answered_prompt_prefixes(&snap_content);
        let new_snap = agent_doc_template::reposition_boundary_to_end_clean(&prompt_canonicalized);
        if new_snap != snap_content {
            match effects.save_snapshot(file, &new_snap) {
                Ok(()) => {
                    eprintln!("[commit] repositioned boundary in snapshot");
                    changed = true;
                }
                Err(e) => {
                    eprintln!(
                        "[commit] failed to update snapshot after boundary reposition: {}",
                        e
                    );
                }
            }
        }
    }

    if active_run {
        eprintln!("[commit] skipping working-tree boundary reposition — active run detected");
        return changed;
    }

    if let Ok(working) = effects.read_to_string(file) {
        let snapshot_after_reposition = effects.load_snapshot(file).ok().flatten();
        let prompt_canonicalized = canonicalize_answered_prompt_prefixes(&working);
        let normalize_prefix_lines = snapshot_after_reposition
            .as_deref()
            .map(|snapshot| {
                agent_doc_element_exchange::extract_post_commit_normalization_targets(
                    snapshot,
                    &prompt_canonicalized,
                )
            })
            .unwrap_or_default();
        let prefix_repaired = if normalize_prefix_lines.is_empty() {
            prompt_canonicalized
        } else {
            agent_doc_element_exchange::normalize_exchange_prefixes_for_targets(
                &prompt_canonicalized,
                &normalize_prefix_lines,
            )
        };
        let repositioned =
            agent_doc_template::reposition_boundary_to_end_preserve_head(&prefix_repaired);
        if repositioned != working {
            let committed_boundary_id = snapshot_after_reposition.as_deref().and_then(|snapshot| {
                agent_doc_element_boundary::boundary::find_boundary_id(snapshot, "exchange")
            });
            if effects.ipc_listener_active(file) {
                let delivery = effects.request_editor_boundary_reposition(
                    file,
                    committed_boundary_id.as_deref(),
                    &normalize_prefix_lines,
                );
                match delivery {
                    Ok(BoundaryRepositionDelivery::Delivered) => {
                        eprintln!(
                            "[commit] delivered boundary reposition to the Lazily editor head"
                        );
                        changed |= BoundaryRepositionDelivery::Delivered.marks_visible_delivery();
                    }
                    Ok(BoundaryRepositionDelivery::RetainedForRetry) => {
                        eprintln!(
                            "[commit] editor boundary reposition retained for retry; visible delivery remains pending"
                        );
                    }
                    Ok(BoundaryRepositionDelivery::Unavailable) => {
                        eprintln!("[commit] editor boundary reposition retained for retry");
                    }
                    Err(e) => {
                        eprintln!(
                            "[commit] failed to deliver editor boundary reposition: {}",
                            e
                        );
                    }
                }
            } else {
                changed |= atomic_write_repositioned(
                    effects,
                    file,
                    &repositioned,
                    normalize_prefix_lines.len(),
                );
            }
        }
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::BoundaryRepositionDelivery;

    #[test]
    fn retained_boundary_target_is_not_visible_delivery_proof() {
        assert!(BoundaryRepositionDelivery::Delivered.marks_visible_delivery());
        assert!(!BoundaryRepositionDelivery::RetainedForRetry.marks_visible_delivery());
        assert!(!BoundaryRepositionDelivery::Unavailable.marks_visible_delivery());
    }
}

fn atomic_write_repositioned(
    effects: &impl BoundaryRepositionEffects,
    file: &Path,
    repositioned: &str,
    repaired_prefix_lines: usize,
) -> bool {
    match effects.atomic_write(file, repositioned) {
        Ok(()) => {
            if repaired_prefix_lines == 0 {
                eprintln!("[commit] repositioned boundary in working tree");
            } else {
                eprintln!(
                    "[commit] repaired {} prefix lines and repositioned boundary in working tree",
                    repaired_prefix_lines
                );
            }
            true
        }
        Err(e) => {
            eprintln!(
                "[commit] failed to reposition boundary in working tree: {}",
                e
            );
            false
        }
    }
}
