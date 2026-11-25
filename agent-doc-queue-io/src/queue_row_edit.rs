//! `agent-doc queue edit` (`#patchruncomponent`): edit or delete one line of a
//! queue tree through the binary-owned queue write path.
//!
//! The command is a read/plan/CAS-write transaction like `queue attr`: it holds
//! the queue-edit lease, plans from the current editor authority
//! ([`agent_doc_queue::row_edit::plan_row_edit`]), and submits the exact
//! source/target pair through `QueueCommandEffects::converge_document_or_disk`,
//! so an attached editor receives it through the CRDT path and a concurrent
//! operator edit is merged or refused at the shared write-policy boundary
//! instead of being overwritten.
//!
//! A row a worker is bound to keys its claim on the row's text. An edit that
//! would change that identity (or delete the row) refuses: the claim would lose
//! its row. Edit the row's notes instead, or release the claim first.

use std::path::Path;

use agent_doc_queue::queue_claim::claim_identity;
use agent_doc_queue::row_edit::{
    RowEditOp, RowEditPlan, RowEditSelector, plan_row_edit, row_work_text,
};
use anyhow::{Context, Result, bail};

use crate::queue_cmd::QueueCommandEffects;

/// Plan and apply one queue line edit. With `dry_run`, plan only.
pub fn edit_with_effects(
    effects: &impl QueueCommandEffects,
    file: &Path,
    queue_id: Option<&str>,
    selector: &RowEditSelector,
    expect: Option<&str>,
    op: &RowEditOp,
    dry_run: bool,
) -> Result<RowEditPlan> {
    let _queue_edit_guard = (!dry_run)
        .then(|| crate::queue_edit_owner::QueueEditGuard::acquire(file))
        .transpose()?;
    let current = effects.current_document_content(file, "queue_row_edit")?;
    let plan = plan_row_edit(&current, queue_id, selector, expect, op)
        .with_context(|| format!("{}: queue edit refused", file.display()))?;
    if !plan.changed {
        return Ok(plan);
    }
    refuse_claimed_row_identity_change(file, &current, &plan)?;
    if dry_run {
        return Ok(plan);
    }
    effects
        .converge_document_or_disk(file, &plan.document, &current, "queue_row_edit")
        .context("queue edit: failed to write document")?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_row_edit_applied queue_id={} line={} op={} source_hash={} target_hash={}",
            plan.queue,
            plan.line,
            if plan.after.is_some() {
                "replace"
            } else {
                "delete"
            },
            agent_doc_hash::content_hash(&current),
            agent_doc_hash::content_hash(&plan.document),
        ),
    );
    Ok(plan)
}

/// Refuse an edit that changes (or deletes) the row identity an active claim
/// is keyed on.
fn refuse_claimed_row_identity_change(
    file: &Path,
    current: &str,
    plan: &RowEditPlan,
) -> Result<()> {
    let before = row_work_text(&plan.before);
    if before.is_empty() {
        return Ok(());
    }
    let before_identity = claim_identity(before);
    let after_identity = plan
        .after
        .as_deref()
        .map(|after| claim_identity(row_work_text(after)));
    if after_identity.as_ref() == Some(&before_identity) {
        return Ok(());
    }
    let claims = crate::queue_claim::active_claims_for_content(file, current)
        .context("queue edit: read active claims")?;
    if let Some(claim) = claims
        .iter()
        .find(|claim| claim.identity == before_identity)
    {
        bail!(
            "{}: queue edit refused: line {} is the row {:?} that {} (generation {}) is claimed for; {} would detach the claim from its row. Edit a note under it, or release the claim first",
            file.display(),
            plan.line,
            claim.item_text,
            claim.owner,
            claim.generation,
            if plan.after.is_some() {
                "changing its text"
            } else {
                "deleting it"
            },
        );
    }
    Ok(())
}
