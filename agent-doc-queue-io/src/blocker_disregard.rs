//! `agent-doc queue disregard` write path (`#blockerdisregard`).
//!
//! The plan is pure. This module holds the queue mutation lease and submits
//! the successor through the editor/CRDT convergence path.

use std::path::Path;

use agent_doc_queue::blocker_disregard::{DisregardPlan, DisregardTarget};
use anyhow::Result;

use crate::queue_cmd::QueueCommandEffects;

/// Apply `target` through the queue's serialized, CAS-safe write boundary.
/// An unchanged document is not written.
pub fn edit_with_effects(
    effects: &impl QueueCommandEffects,
    file: &Path,
    target: DisregardTarget,
) -> Result<DisregardPlan> {
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    let current = effects.current_document_content(file, "blocker_disregard")?;
    let plan = agent_doc_queue::blocker_disregard::plan(&current, target)?;
    if !plan.changed {
        return Ok(plan);
    }
    effects.converge_document_or_disk(file, &plan.content, &current, "blocker_disregard")?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "blocker_disregard_applied file={} source_hash={} target_hash={}",
            file.display(),
            agent_doc_hash::content_hash(&current),
            agent_doc_hash::content_hash(&plan.content),
        ),
    );
    Ok(plan)
}
