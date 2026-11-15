//! The effect phase of one queue-maintenance pass (`#implementlazilyowned`
//! plan step 9a).
//!
//! Queue maintenance computes its successor document before it writes
//! anything. Every durable effect the pass used to perform on its way to the
//! document cut (ops evidence lines, claim-ledger transitions, operator-delete
//! tombstones, mirror provenance, the skipped and projected-in-progress head
//! sets, the edited-head baseline adoption) is staged here instead, and the
//! later reads of the same pass see the staged values through read overlays
//! (the claim-ledger overlay, the planned tombstone set, the planned baseline,
//! the pending mirror provenance). The document cut is then the first effect
//! of the pass, and the phase is applied right after the cut's receipt, in the
//! original order, each effect still crossing its step of the owner's Lazily
//! plan. A pass that ends without a cut applies the phase at its exit; a pass
//! that fails or is superseded before its cut applies nothing.
//!
//! The phase is plain data, so a detached owner stages it in its durable plan
//! image before the cut: a worker resuming after a crash between the cut and
//! the phase applies it when the authority still equals the staged cut
//! ([`resume_staged_queue_maintenance_cut`]).

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use agent_doc_queue_io::queue_claim::{PlannedClaimLedgerOverlay, PlannedClaimWrite};
use agent_doc_queue_io::queue_tombstone::PlannedTombstoneWrite;

use crate::queue_maintenance_effect_cas::{QueueMaintenanceEffectSuperseded, StagedDocumentCut};
use crate::{PendingMaintenanceEffectFence, QueueMaintenanceEffectStep};

/// One durable effect of a queue-maintenance pass, deferred behind its
/// document cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum DeferredMaintenanceEffect {
    /// An ops-log evidence line.
    Ops { line: String },
    /// A claim-ledger transition.
    Claim { write: PlannedClaimWrite },
    /// The operator-delete tombstone state.
    Tombstones { write: PlannedTombstoneWrite },
    /// `#fr79` provenance of the heads the backlog mirror owns.
    MirroredQueueHeads { identities: Vec<String> },
    /// `#queueskip` skipped-head set for this cycle.
    SkippedQueueHeads { ids: Vec<String> },
    /// Projected in-progress heads for this cycle.
    ProjectedInProgressQueueHeads { heads: Vec<String> },
    /// Adopt the edited queue head into the recovery baseline; `region` is
    /// the queue component of the pass's content at that point.
    EditedQueueHeadBaseline { region: String },
}

/// The staged effect phase of one pass and the read overlays it implies.
pub(crate) struct QueueMaintenanceEffectPhase {
    effects: Vec<DeferredMaintenanceEffect>,
    claims: Option<PlannedClaimLedgerOverlay>,
    planned_tombstones: Option<HashSet<String>>,
    planned_baseline: Option<String>,
    pending_mirrored_heads: Vec<String>,
}

impl QueueMaintenanceEffectPhase {
    pub(crate) fn new(file: &Path) -> Self {
        Self {
            effects: Vec::new(),
            claims: Some(PlannedClaimLedgerOverlay::install(file)),
            planned_tombstones: None,
            planned_baseline: None,
            pending_mirrored_heads: Vec::new(),
        }
    }

    /// Stage an ops evidence line.
    pub(crate) fn log_op(&mut self, line: impl Into<String>) {
        self.effects
            .push(DeferredMaintenanceEffect::Ops { line: line.into() });
    }

    /// Stage a planned claim write; the rest of the pass reads it through the
    /// claim-ledger overlay. A planning failure is the warning the immediate
    /// effect printed.
    pub(crate) fn stage_claim_write(
        &mut self,
        file: &Path,
        planned: Result<Option<PlannedClaimWrite>>,
        kind: ClaimWriteKind,
    ) {
        match planned {
            Ok(Some(write)) => {
                if let Some(claims) = &self.claims {
                    claims.stage(&write);
                }
                self.effects
                    .push(DeferredMaintenanceEffect::Claim { write });
            }
            Ok(None) => {}
            Err(err) => eprintln!("{}", kind.warning(file, &err)),
        }
    }

    /// Stage a planned tombstone reconcile and return the tombstone set the
    /// mirror skips.
    pub(crate) fn stage_tombstones(
        &mut self,
        plan: agent_doc_queue_io::queue_tombstone::TombstoneReconcilePlan,
    ) -> HashSet<String> {
        if let Some(write) = plan.write {
            self.effects
                .push(DeferredMaintenanceEffect::Tombstones { write });
        }
        self.planned_tombstones = Some(plan.tombstones.clone());
        plan.tombstones
    }

    /// The operator-delete tombstone set as of this pass.
    pub(crate) fn current_tombstones(&self, file: &Path) -> HashSet<String> {
        self.planned_tombstones
            .clone()
            .unwrap_or_else(|| agent_doc_queue_io::queue_tombstone::current_tombstones(file))
    }

    /// Stage mirror provenance for `identities`.
    pub(crate) fn stage_mirrored_queue_heads(&mut self, identities: Vec<String>) {
        if identities.is_empty() {
            return;
        }
        self.pending_mirrored_heads
            .extend(identities.iter().cloned());
        self.effects
            .push(DeferredMaintenanceEffect::MirroredQueueHeads { identities });
    }

    /// Mirror provenance as of this pass: the stored identities plus the ones
    /// this pass staged.
    pub(crate) fn mirrored_head_identities(&self, mut stored: HashSet<String>) -> HashSet<String> {
        stored.extend(self.pending_mirrored_heads.iter().cloned());
        stored
    }

    pub(crate) fn stage_skipped_queue_heads(&mut self, ids: Vec<String>) {
        self.effects
            .push(DeferredMaintenanceEffect::SkippedQueueHeads { ids });
    }

    pub(crate) fn stage_projected_in_progress_queue_heads(&mut self, heads: Vec<String>) {
        self.effects
            .push(DeferredMaintenanceEffect::ProjectedInProgressQueueHeads { heads });
    }

    /// Stage the edited-head baseline adoption for `current_content`; later
    /// baseline reads of the pass see the adopted baseline.
    pub(crate) fn stage_edited_queue_head_baseline(&mut self, file: &Path, current_content: &str) {
        let Some(region) = first_queue_region(current_content) else {
            return;
        };
        if let Some(baseline) = self.document_baseline(file)
            && let Some(adopted) =
                crate::edited_queue_head_adopted_into_snapshot(&baseline, &region)
        {
            self.planned_baseline = Some(adopted);
        }
        self.effects
            .push(DeferredMaintenanceEffect::EditedQueueHeadBaseline { region });
    }

    /// The recovery baseline as of this pass.
    pub(crate) fn document_baseline(&self, file: &Path) -> Option<String> {
        self.planned_baseline.clone().or_else(|| {
            agent_doc_snapshot_io::load_document_baseline(file)
                .ok()
                .flatten()
        })
    }

    /// Stage the cut this pass is about to write in the owner's durable plan
    /// image, with this phase.
    pub(crate) fn stage_cut(
        &self,
        file: &Path,
        effect_fence: Option<&dyn PendingMaintenanceEffectFence>,
        cut_content: &str,
    ) -> Result<()> {
        let Some(effect_fence) = effect_fence else {
            return Ok(());
        };
        effect_fence.stage_document_cut(
            file,
            StagedDocumentCut {
                cut: agent_doc_hash::content_hash(cut_content),
                effects: serde_json::to_string(&self.effects)?,
            },
        )
    }

    /// Apply the phase: after the pass's document cut is receipted, or at an
    /// exit without a cut. The read overlays end first, so the durable writes
    /// are not read twice.
    pub(crate) fn apply(
        &mut self,
        file: &Path,
        effect_fence: Option<&dyn PendingMaintenanceEffectFence>,
    ) -> Result<()> {
        self.claims = None;
        self.planned_tombstones = None;
        self.planned_baseline = None;
        self.pending_mirrored_heads.clear();
        let effects = std::mem::take(&mut self.effects);
        apply_deferred_effects(file, effect_fence, &effects)?;
        if let Some(effect_fence) = effect_fence {
            effect_fence.finish_document_cut(file)?;
        }
        Ok(())
    }
}

/// Which immediate claim effect a planned write stands for, for its warning.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ClaimWriteKind {
    PromotedClaims,
    PromptIdentityAdmission,
    ClosedHeadRelease,
    CompletionEvidence,
}

impl ClaimWriteKind {
    fn of(write: &PlannedClaimWrite) -> Self {
        match write {
            PlannedClaimWrite::TransferPromoted { .. } => Self::PromotedClaims,
            PlannedClaimWrite::TransferPromptIdentityAdmissions { .. } => {
                Self::PromptIdentityAdmission
            }
            PlannedClaimWrite::ReleaseClosedHead { .. } => Self::ClosedHeadRelease,
            PlannedClaimWrite::RecoverReportedCompletions { .. } => Self::CompletionEvidence,
        }
    }

    /// The warning the immediate effect printed on failure.
    fn warning(self, file: &Path, err: &anyhow::Error) -> String {
        match self {
            Self::PromotedClaims => format!(
                "[preflight] queue: WARNING: could not carry claims onto promoted heads: {err:#}"
            ),
            Self::PromptIdentityAdmission => format!(
                "[preflight] queue: WARNING: could not carry claims across stable-id admission: {err:#}"
            ),
            Self::ClosedHeadRelease => format!(
                "[preflight] WARNING: could not release the claim on a struck free-text head in {}: {err:#}",
                file.display()
            ),
            Self::CompletionEvidence => {
                format!("[preflight] queue: completion evidence recovery failed ({err:#})")
            }
        }
    }
}

/// The first `queue` component of `content`, as text.
fn first_queue_region(content: &str) -> Option<String> {
    let components = agent_doc_element::element::parse(content).ok()?;
    let queue = components.iter().find(|c| c.name == "queue")?;
    Some(content[queue.open_start..queue.close_end].to_string())
}

/// Replay identity of one deferred effect inside its step family.
fn effect_intent(family: &str, effect: &DeferredMaintenanceEffect) -> Result<String> {
    Ok(format!(
        "{family}:{}",
        agent_doc_hash::content_hash(&serde_json::to_string(effect)?)
    ))
}

/// Apply `effects` in order, each through its step of the owner's Lazily
/// plan. Every effect is replayable: a resumed plan skips one that already has
/// a receipt. A backend failure stays the warning the immediate effect
/// printed; a superseded authority stops the phase.
pub(crate) fn apply_deferred_effects(
    file: &Path,
    effect_fence: Option<&dyn PendingMaintenanceEffectFence>,
    effects: &[DeferredMaintenanceEffect],
) -> Result<()> {
    for effect in effects {
        match effect {
            DeferredMaintenanceEffect::Ops { line } => {
                crate::queue_maintenance_log_op(file, effect_fence, line)?;
            }
            DeferredMaintenanceEffect::Claim { write } => {
                crate::queue_maintenance_replayable_effect(
                    file,
                    effect_fence,
                    QueueMaintenanceEffectStep::Ledger,
                    effect_intent("claim", effect)?,
                    || {
                        if let Err(err) =
                            agent_doc_queue_io::queue_claim::apply_planned_claim_write(file, write)
                        {
                            eprintln!("{}", ClaimWriteKind::of(write).warning(file, &err));
                        }
                        Ok(())
                    },
                )?;
            }
            DeferredMaintenanceEffect::Tombstones { write } => {
                crate::queue_maintenance_replayable_effect(
                    file,
                    effect_fence,
                    QueueMaintenanceEffectStep::Ledger,
                    effect_intent("tombstones", effect)?,
                    || {
                        agent_doc_queue_io::queue_tombstone::apply_planned_tombstone_write(
                            file, write,
                        );
                        Ok(())
                    },
                )?;
            }
            DeferredMaintenanceEffect::MirroredQueueHeads { identities } => {
                crate::queue_maintenance_replayable_effect(
                    file,
                    effect_fence,
                    QueueMaintenanceEffectStep::Ledger,
                    effect_intent("mirrored_heads", effect)?,
                    || {
                        if let Err(err) = agent_doc_project_root_io::project_root_containing(file)
                            .context("queue head provenance: no project root for state.db")
                            .and_then(|root| {
                                let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
                                agent_doc_sqlite::state_store::record_mirrored_queue_heads_in_db(
                                    &conn,
                                    &file.display().to_string(),
                                    identities,
                                )
                            })
                        {
                            // Provenance is an optimization for a later strike
                            // decision, never a correctness gate: a miss just
                            // leaves heads unattributed, which fails safe
                            // (never struck). Warn rather than fail the cycle.
                            eprintln!(
                                "[preflight] warning: failed to record queue head provenance for {}: {err:#}",
                                file.display()
                            );
                        }
                        Ok(())
                    },
                )?;
            }
            DeferredMaintenanceEffect::SkippedQueueHeads { ids } => {
                crate::queue_maintenance_replayable_effect(
                    file,
                    effect_fence,
                    QueueMaintenanceEffectStep::Ledger,
                    effect_intent("skipped_heads", effect)?,
                    || {
                        if let Err(err) =
                            agent_doc_cycle_state_io::set_skipped_queue_head_ids(file, ids)
                        {
                            eprintln!(
                                "[preflight] queue: failed to persist skipped-head set ({err:#})"
                            );
                        }
                        Ok(())
                    },
                )?;
            }
            DeferredMaintenanceEffect::ProjectedInProgressQueueHeads { heads } => {
                crate::queue_maintenance_replayable_effect(
                    file,
                    effect_fence,
                    QueueMaintenanceEffectStep::Ledger,
                    effect_intent("projected_heads", effect)?,
                    || {
                        if let Err(err) =
                            agent_doc_cycle_state_io::set_projected_in_progress_queue_heads(
                                file, heads,
                            )
                        {
                            eprintln!(
                                "[preflight] queue: failed to persist projected in-progress heads ({err:#})"
                            );
                        }
                        Ok(())
                    },
                )?;
            }
            DeferredMaintenanceEffect::EditedQueueHeadBaseline { region } => {
                crate::queue_maintenance_replayable_effect(
                    file,
                    effect_fence,
                    QueueMaintenanceEffectStep::Snapshot,
                    format!(
                        "snapshot:queue-head:{}",
                        agent_doc_hash::content_hash(region)
                    ),
                    || {
                        crate::checkpoint_adopted_queue_baseline(file, |baseline| {
                            crate::edited_queue_head_adopted_into_snapshot(baseline, region)
                        });
                        Ok(())
                    },
                )?;
            }
        }
    }
    Ok(())
}

/// Finish the effect phase of a cut a crashed predecessor staged
/// (`#implementlazilyowned` plan step 9a).
///
/// A detached owner stages its cut and effect phase in the plan's durable
/// image before writing the cut. When a worker resumes that request:
///
/// - the authority still equals the staged cut: the cut landed. Receipt it if
///   the predecessor died before its receipt, then apply the phase (effects
///   that already have a receipt are skipped). The plan now expects the cut,
///   so the request continues from it instead of being refused as superseded.
/// - the authority equals what the plan expected before the cut: the cut never
///   landed. The phase is dropped and the request runs from its admission.
/// - anything else: whether the cut landed is unknown, so the phase is
///   dropped; the request's next step then supersedes as before.
pub fn resume_staged_queue_maintenance_cut(
    file: &Path,
    effect_fence: &dyn PendingMaintenanceEffectFence,
) -> Result<()> {
    let Some(staged) = effect_fence.staged_document_cut() else {
        return Ok(());
    };
    let effects = match serde_json::from_str::<Vec<DeferredMaintenanceEffect>>(&staged.effects) {
        Ok(effects) => effects,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_maintenance_plan_cut_unreadable file={} error={err}",
                    file.display()
                ),
            );
            return effect_fence.finish_document_cut(file);
        }
    };
    let held = effect_fence.expected_authority_hash();
    if held.as_deref() != Some(staged.cut.as_str()) {
        let observed = effect_fence.observed_authority_hash(file)?;
        if observed.as_deref() != Some(staged.cut.as_str()) {
            let landed = if observed == held { "no" } else { "unknown" };
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_maintenance_plan_cut_dropped file={} landed={landed} effects={}",
                    file.display(),
                    effects.len()
                ),
            );
            return effect_fence.finish_document_cut(file);
        }
        match effect_fence.record_applied_cut_hash(file, &staged.cut) {
            Ok(()) => {}
            Err(err)
                if err
                    .downcast_ref::<QueueMaintenanceEffectSuperseded>()
                    .is_some() =>
            {
                // The authority moved between the observation and the receipt:
                // the plan is superseded, so its next step stops the request.
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "queue_maintenance_plan_cut_dropped file={} landed=unknown effects={}",
                        file.display(),
                        effects.len()
                    ),
                );
                return effect_fence.finish_document_cut(file);
            }
            Err(err) => return Err(err),
        }
    }
    apply_deferred_effects(file, Some(effect_fence), &effects)?;
    effect_fence.finish_document_cut(file)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_maintenance_plan_cut_resumed file={} effects={}",
            file.display(),
            effects.len()
        ),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deferred_effects_round_trip_through_the_plan_image() {
        let effects = vec![
            DeferredMaintenanceEffect::Ops {
                line: "free_text_coin_held file=x".to_string(),
            },
            DeferredMaintenanceEffect::Claim {
                write: agent_doc_queue_io::queue_claim::plan_release_closed_head("head"),
            },
            DeferredMaintenanceEffect::SkippedQueueHeads {
                ids: vec!["a".to_string()],
            },
            DeferredMaintenanceEffect::ProjectedInProgressQueueHeads {
                heads: vec!["do [#a]".to_string()],
            },
            DeferredMaintenanceEffect::MirroredQueueHeads {
                identities: vec!["a".to_string()],
            },
            DeferredMaintenanceEffect::EditedQueueHeadBaseline {
                region: "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->".to_string(),
            },
        ];
        let encoded = serde_json::to_string(&effects).unwrap();
        let decoded: Vec<DeferredMaintenanceEffect> = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, effects);
        assert_ne!(
            effect_intent("claim", &effects[1]).unwrap(),
            effect_intent("claim", &effects[2]).unwrap()
        );
    }

    #[test]
    fn first_queue_region_adopts_like_the_whole_document() {
        let snapshot = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:queue auto -->\n- do [#old]\n<!-- /agent:queue -->\n",
        );
        let current = snapshot.replace("do [#old]", "do [#new]");
        let region = first_queue_region(&current).unwrap();
        assert_eq!(
            crate::edited_queue_head_adopted_into_snapshot(snapshot, &region),
            crate::edited_queue_head_adopted_into_snapshot(snapshot, &current),
        );
        assert!(crate::edited_queue_head_adopted_into_snapshot(snapshot, &region).is_some());
    }
}
