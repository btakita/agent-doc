//! Retained document-write settlement as a derived fact (`#retainedsettlereactive`).
//!
//! "Is a retained document write still unsettled?" is a fact two consumers must
//! agree on: `preflight` refuses to open a cycle while one is outstanding, and
//! `session-check` reports closeout success. Before this module they derived it
//! from **different inputs** and never shared a cell:
//!
//! - `preflight` read `pending_document_write(file).is_some()` — a fresh
//!   `load_state_backbone_projection` reload at the decision seam, which is the
//!   storage-arbitration inversion `#lazily-hot-path` forbids;
//! - `session-check` compared canonical authority bytes against disk bytes and
//!   never consulted the retained intent at all.
//!
//! Both can be true at once, and when they are the session deadlocks: the
//! operator is told to run `session-check` (which says ok) and then `preflight`
//! (which says unsettled), forever. Observed 2026-07-25 on
//! `tasks/agent-doc/agent-doc-bugs2.md`.
//!
//! The mechanism behind the orphan is that `pending_write` is cleared only by a
//! `DocumentWriteConverged` fact whose `intent_id` **and** `target_hash` match.
//! When the operator types into the live editor while a write is in flight, the
//! delivered content is rebased, so the retained byte target becomes permanently
//! unreachable — even though the intent's *purpose* (materialize the response)
//! is satisfied by the converged document. Settlement keyed on stamped byte
//! equality cannot see that; settlement **derived from the document** can.
//!
//! So the fact is one [`Computed`] over observations, read by both consumers,
//! and the clear is an [`Effect`] gated on that derived value rather than a
//! `settle_*` call some code path had to remember to make.
//!
//! # Three outcomes, never two (`#idlerevisionreactive`)
//!
//! A probe that gates expensive work must keep "I did not look", "I looked and
//! got no answer", and "here is the answer" distinct. [`SettlementVerdict`]
//! therefore separates [`SettlementVerdict::Unobserved`] from
//! [`SettlementVerdict::Unsettled`]: an unobservable controller must not be
//! reported as an outstanding write, which is what would turn a transport blip
//! into a permanent refusal to open a cycle.

use lazily::{Computed, Source, ThreadSafeContext};
use serde::{Deserialize, Serialize};

use crate::{DocumentScope, DocumentWriteDeferredReason};

/// The facts about a retained intent that settlement depends on.
///
/// Deliberately narrower than `DocumentWriteIntentProjection`: settlement needs
/// the identity, the byte target it was stamped with, and whether the intent
/// carries a response payload whose materialization can stand in for that
/// target once an operator edit has rebased it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetainedIntentFacts {
    pub intent_id: String,
    pub target_hash: String,
    pub reason: DocumentWriteDeferredReason,
    /// True when the intent introduces an assistant response, so materializing
    /// that response in the converged document satisfies the intent even at a
    /// different hash. Delivery-only projections have no such payload and must
    /// settle on exact bytes.
    pub carries_response_payload: bool,
}

/// One observation of a content plane.
///
/// `payload_materialized` answers "does this content already contain the
/// retained intent's response payload?" — the question that distinguishes a
/// rebased-but-satisfied intent from a genuinely undelivered one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentObservation {
    pub content_hash: String,
    pub payload_materialized: bool,
}

/// Why a retained intent is not settleable yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnsettledCause {
    /// Authority and disk hold different bytes: delivery is still in flight.
    AuthorityDiskDiverged,
    /// Planes agree, but the intent's payload is absent from that agreed
    /// content — the write genuinely has not landed.
    PayloadAbsentFromConvergedContent,
}

impl UnsettledCause {
    pub const fn token(self) -> &'static str {
        match self {
            Self::AuthorityDiskDiverged => "authority_disk_diverged",
            Self::PayloadAbsentFromConvergedContent => "payload_absent_from_converged_content",
        }
    }
}

/// How a satisfied intent was proven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SatisfiedProof {
    /// Converged content is exactly the stamped byte target.
    ExactTarget,
    /// The stamped target is unreachable because a concurrent operator edit
    /// rebased it, but the intent's response payload is present in the
    /// converged content. This is the case the byte-equality clear could never
    /// reach, and the one that stranded the intent.
    RebasedPayloadMaterialized,
}

impl SatisfiedProof {
    pub const fn token(self) -> &'static str {
        match self {
            Self::ExactTarget => "exact_target",
            Self::RebasedPayloadMaterialized => "rebased_payload_materialized",
        }
    }
}

/// The derived settlement fact. Both `preflight` and `session-check` read this
/// one value instead of deriving their own from different inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementVerdict {
    /// No durable intent is outstanding.
    NoRetainedIntent,
    /// An intent is outstanding but a content plane could not be observed. This
    /// is *not* an outstanding write for gating purposes — see
    /// `#idlerevisionreactive`. Reporting "I could not look" as "unsettled" is
    /// what converts a transport blip into a permanent cycle refusal.
    Unobserved { intent_id: String },
    /// The intent's purpose is met by the converged document and it should be
    /// cleared.
    Satisfied {
        intent_id: String,
        retained_target_hash: String,
        settled_hash: String,
        proof: SatisfiedProof,
    },
    /// Genuinely still awaiting delivery.
    Unsettled {
        intent_id: String,
        cause: UnsettledCause,
    },
}

impl SettlementVerdict {
    /// The single question `preflight` asks. `Unobserved` answers `false`
    /// deliberately: an unobservable plane is not proof of an outstanding write.
    pub fn blocks_new_cycle(&self) -> bool {
        matches!(self, Self::Unsettled { .. })
    }

    /// The single question the settle effect asks.
    pub fn should_clear_intent(&self) -> bool {
        matches!(self, Self::Satisfied { .. })
    }

    pub fn intent_id(&self) -> Option<&str> {
        match self {
            Self::NoRetainedIntent => None,
            Self::Unobserved { intent_id }
            | Self::Satisfied { intent_id, .. }
            | Self::Unsettled { intent_id, .. } => Some(intent_id.as_str()),
        }
    }
}

/// The whole decision as a pure total function of its observations.
///
/// Kept separate from the cells so it is unit-testable with fixed inputs, and
/// so the reactive wiring has nothing in it but wiring.
pub fn settlement_verdict(
    pending: Option<&RetainedIntentFacts>,
    authority: Option<&ContentObservation>,
    disk: Option<&ContentObservation>,
) -> SettlementVerdict {
    let Some(pending) = pending else {
        return SettlementVerdict::NoRetainedIntent;
    };
    let (Some(authority), Some(disk)) = (authority, disk) else {
        return SettlementVerdict::Unobserved {
            intent_id: pending.intent_id.clone(),
        };
    };
    if authority.content_hash != disk.content_hash {
        return SettlementVerdict::Unsettled {
            intent_id: pending.intent_id.clone(),
            cause: UnsettledCause::AuthorityDiskDiverged,
        };
    }
    // Planes agree. Either the agreed content *is* the stamped target, or a
    // concurrent operator edit rebased it and only the payload can prove the
    // intent landed.
    let proof = if authority.content_hash == pending.target_hash {
        Some(SatisfiedProof::ExactTarget)
    } else if pending.carries_response_payload && authority.payload_materialized {
        Some(SatisfiedProof::RebasedPayloadMaterialized)
    } else {
        None
    };
    match proof {
        Some(proof) => SettlementVerdict::Satisfied {
            intent_id: pending.intent_id.clone(),
            retained_target_hash: pending.target_hash.clone(),
            settled_hash: authority.content_hash.clone(),
            proof,
        },
        None => SettlementVerdict::Unsettled {
            intent_id: pending.intent_id.clone(),
            cause: UnsettledCause::PayloadAbsentFromConvergedContent,
        },
    }
}

/// Document-scoped cells holding retained-write settlement.
///
/// The observations are [`Source`]s and the verdict is a [`Computed`] over them, so every consumer that reads `verdict()`
/// reads the *same* derived value and cannot drift from another consumer's
/// private derivation. Storage hydrates the sources at startup; it never
/// arbitrates here (`#lzdurablesink`).
pub struct RetainedWriteSettlement {
    ctx: ThreadSafeContext,
    pending: Source<Option<RetainedIntentFacts>>,
    authority: Source<Option<ContentObservation>>,
    disk: Source<Option<ContentObservation>>,
    verdict: Computed<SettlementVerdict>,
}

impl RetainedWriteSettlement {
    /// Join the document's graph. Settlement's lifetime is the open document:
    /// a retained intent outlives any one turn, and dropping the document drops
    /// the intent with it.
    pub fn new_in(scope: &DocumentScope) -> Self {
        Self::build(scope.ctx().clone())
    }

    /// Standalone instance for unit tests, kept beside `new_in` per
    /// `#stategraphjoin`.
    pub fn new() -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        Self::build(ThreadSafeContext::new())
    }

    fn build(ctx: ThreadSafeContext) -> Self {
        let pending = ctx.source(None::<RetainedIntentFacts>);
        let authority = ctx.source(None::<ContentObservation>);
        let disk = ctx.source(None::<ContentObservation>);
        let verdict = {
            ctx.computed(move |ctx| {
                settlement_verdict(
                    ctx.get(&pending).as_ref(),
                    ctx.get(&authority).as_ref(),
                    ctx.get(&disk).as_ref(),
                )
            })
        };
        Self {
            ctx,
            pending,
            authority,
            disk,
            verdict,
        }
    }

    /// The context this settlement joined, so callers can build effects that
    /// observe [`Self::verdict_cell`] in the same graph.
    pub fn ctx(&self) -> &ThreadSafeContext {
        &self.ctx
    }

    pub fn observe_pending(&self, pending: Option<RetainedIntentFacts>) {
        self.ctx.set(&self.pending, pending);
    }

    pub fn observe_authority(&self, authority: Option<ContentObservation>) {
        self.ctx.set(&self.authority, authority);
    }

    pub fn observe_disk(&self, disk: Option<ContentObservation>) {
        self.ctx.set(&self.disk, disk);
    }

    /// The shared derived fact.
    pub fn verdict(&self) -> SettlementVerdict {
        self.ctx.get(&self.verdict)
    }

    /// The content observations currently held, so a caller that resolved them
    /// locally can forward them to the process that owns the shared graph.
    pub fn observations(&self) -> (Option<ContentObservation>, Option<ContentObservation>) {
        (self.ctx.get(&self.authority), self.ctx.get(&self.disk))
    }

    /// The verdict cell itself, for callers gating an `Effect` on it.
    pub fn verdict_cell(&self) -> &Computed<SettlementVerdict> {
        &self.verdict
    }
}

impl Default for RetainedWriteSettlement {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(target_hash: &str, carries_response_payload: bool) -> RetainedIntentFacts {
        RetainedIntentFacts {
            intent_id: "intent-1".to_string(),
            target_hash: target_hash.to_string(),
            reason: DocumentWriteDeferredReason::CrdtDeliveryAckPending,
            carries_response_payload,
        }
    }

    fn observed(content_hash: &str, payload_materialized: bool) -> ContentObservation {
        ContentObservation {
            content_hash: content_hash.to_string(),
            payload_materialized,
        }
    }

    #[test]
    fn no_intent_never_blocks() {
        let verdict = settlement_verdict(None, Some(&observed("a", false)), Some(&observed("a", false)));
        assert_eq!(verdict, SettlementVerdict::NoRetainedIntent);
        assert!(!verdict.blocks_new_cycle());
        assert!(!verdict.should_clear_intent());
    }

    #[test]
    fn exact_target_convergence_satisfies() {
        let verdict = settlement_verdict(
            Some(&intent("a", false)),
            Some(&observed("a", false)),
            Some(&observed("a", false)),
        );
        assert!(verdict.should_clear_intent());
        assert!(!verdict.blocks_new_cycle());
        match verdict {
            SettlementVerdict::Satisfied { proof, .. } => {
                assert_eq!(proof, SatisfiedProof::ExactTarget)
            }
            other => panic!("expected Satisfied, got {other:?}"),
        }
    }

    /// The live deadlock. A concurrent operator edit rebases the delivered
    /// content, so the stamped `target_hash` is permanently unreachable while
    /// authority and disk agree perfectly and the response is present. Byte
    /// equality strands this forever; the payload proof settles it.
    #[test]
    fn rebased_target_with_materialized_payload_satisfies() {
        let verdict = settlement_verdict(
            Some(&intent("stamped", true)),
            Some(&observed("rebased", true)),
            Some(&observed("rebased", true)),
        );
        assert!(
            verdict.should_clear_intent(),
            "an operator edit that rebases the target must not strand the intent"
        );
        assert!(!verdict.blocks_new_cycle());
        match verdict {
            SettlementVerdict::Satisfied {
                proof,
                retained_target_hash,
                settled_hash,
                ..
            } => {
                assert_eq!(proof, SatisfiedProof::RebasedPayloadMaterialized);
                assert_eq!(retained_target_hash, "stamped");
                assert_eq!(settled_hash, "rebased");
            }
            other => panic!("expected Satisfied, got {other:?}"),
        }
    }

    /// A delivery-only projection has no response payload to stand in for the
    /// byte target, so a rebase leaves it genuinely unsettled. Settling it on
    /// convergence alone would bless content that never carried the write.
    #[test]
    fn rebased_delivery_only_projection_stays_unsettled() {
        let verdict = settlement_verdict(
            Some(&intent("stamped", false)),
            Some(&observed("rebased", true)),
            Some(&observed("rebased", true)),
        );
        assert!(verdict.blocks_new_cycle());
        assert!(!verdict.should_clear_intent());
    }

    #[test]
    fn diverged_planes_are_unsettled_not_satisfied() {
        let verdict = settlement_verdict(
            Some(&intent("stamped", true)),
            Some(&observed("authority", true)),
            Some(&observed("disk", true)),
        );
        assert!(verdict.blocks_new_cycle());
        match verdict {
            SettlementVerdict::Unsettled { cause, .. } => {
                assert_eq!(cause, UnsettledCause::AuthorityDiskDiverged)
            }
            other => panic!("expected Unsettled, got {other:?}"),
        }
    }

    #[test]
    fn converged_without_payload_is_unsettled() {
        let verdict = settlement_verdict(
            Some(&intent("stamped", true)),
            Some(&observed("rebased", false)),
            Some(&observed("rebased", false)),
        );
        assert!(verdict.blocks_new_cycle());
        match verdict {
            SettlementVerdict::Unsettled { cause, .. } => {
                assert_eq!(cause, UnsettledCause::PayloadAbsentFromConvergedContent)
            }
            other => panic!("expected Unsettled, got {other:?}"),
        }
    }

    /// `#idlerevisionreactive`: "I could not look" must stay distinct from "I
    /// looked and it is outstanding", or an unobservable controller becomes a
    /// permanent refusal to open a cycle.
    #[test]
    fn unobserved_plane_does_not_block_a_new_cycle() {
        for (authority, disk) in [
            (None, Some(observed("a", true))),
            (Some(observed("a", true)), None),
            (None, None),
        ] {
            let verdict =
                settlement_verdict(Some(&intent("a", true)), authority.as_ref(), disk.as_ref());
            assert!(
                matches!(verdict, SettlementVerdict::Unobserved { .. }),
                "unobserved planes must not be reported as an outstanding write"
            );
            assert!(!verdict.blocks_new_cycle());
            assert!(!verdict.should_clear_intent());
        }
    }

    /// The property the whole module exists for: one derived cell, so two
    /// consumers cannot answer the same question differently.
    #[test]
    fn both_consumers_read_one_derived_value() {
        let settlement = RetainedWriteSettlement::new();
        settlement.observe_pending(Some(intent("stamped", true)));
        settlement.observe_authority(Some(observed("rebased", true)));
        settlement.observe_disk(Some(observed("rebased", true)));

        // `preflight`'s question and `session-check`'s question, same cell.
        let preflight_view = settlement.verdict();
        let session_check_view = settlement.verdict();
        assert_eq!(preflight_view, session_check_view);
        assert!(!preflight_view.blocks_new_cycle());
        assert!(session_check_view.should_clear_intent());
    }

    /// Invalidation must cross: a new observation changes the shared verdict
    /// without anyone re-deriving it by hand.
    #[test]
    fn new_observations_invalidate_the_shared_verdict() {
        let settlement = RetainedWriteSettlement::new();
        settlement.observe_pending(Some(intent("stamped", true)));
        settlement.observe_authority(Some(observed("authority", true)));
        settlement.observe_disk(Some(observed("disk", true)));
        assert!(settlement.verdict().blocks_new_cycle());

        settlement.observe_disk(Some(observed("authority", true)));
        assert!(
            settlement.verdict().should_clear_intent(),
            "the verdict must update because its inputs changed, not because a caller recomputed it"
        );

        settlement.observe_pending(None);
        assert_eq!(settlement.verdict(), SettlementVerdict::NoRetainedIntent);
    }
}

#[cfg(test)]
mod reactive_map_probe {
    use lazily::{ThreadSafeSourceMap, ThreadSafeContext, ThreadSafeComputedMap};

    /// Does a `ThreadSafeComputedMap` entry whose factory captures a context clone
    /// and reads a `ThreadSafeSourceMap` actually subscribe to that cell?
    ///
    /// The slot factory is `Fn(&K) -> V` with no context parameter (the
    /// ctx-taking `mint_with` is private), so the only way to derive across maps
    /// is a captured clone. If that does not track, the entry is "Computed in
    /// name only" and a keyed derived registry cannot be expressed this way.
    #[test]
    fn slot_map_entry_capturing_a_ctx_clone_tracks_a_cell_map_dependency() {
        let ctx = ThreadSafeContext::new();
        let inputs: ThreadSafeSourceMap<String, u32> = ThreadSafeSourceMap::new(&ctx);
        let derived: ThreadSafeComputedMap<String, u32> = ThreadSafeComputedMap::new(&ctx);
        inputs.set(&ctx, "doc".to_string(), 1);

        let factory_ctx = ctx.clone();
        let factory_inputs = inputs.clone();
        let read = |derived: &ThreadSafeComputedMap<String, u32>| {
            let factory_ctx = factory_ctx.clone();
            let factory_inputs = factory_inputs.clone();
            derived.get_or_insert_with(&ctx, "doc".to_string(), move |key| {
                factory_inputs.observe(&factory_ctx, key).unwrap_or(0) + 100
            })
        };

        assert_eq!(read(&derived), 101);
        inputs.set(&ctx, "doc".to_string(), 2);
        assert_eq!(
            read(&derived),
            102,
            "a slot entry must invalidate when the cell map it read changes; \
             if this fails the thread-safe slot map cannot express a derived registry"
        );
    }
}

#[cfg(test)]
mod suppression_guard {
    use super::*;

    fn observed(hash: &str) -> ContentObservation {
        ContentObservation {
            content_hash: hash.to_string(),
            payload_materialized: true,
        }
    }

    /// A consumer that resolves content planes but whose own storage view shows
    /// no pending intent must still report those planes, because the shared
    /// graph — not that consumer — decides what is outstanding.
    ///
    /// The regression: an early return skipped both content reads when the local
    /// ledger replay saw no intent, so the shared graph received
    /// `authority: None, disk: None` and answered `Unobserved` for a document
    /// whose planes were converged. A storage read in one process must never be
    /// able to suppress the shared verdict.
    #[test]
    fn converged_planes_settle_even_when_the_local_view_missed_the_intent() {
        let settlement = RetainedWriteSettlement::new();

        // Local view saw no intent, but the planes were still observed.
        settlement.observe_pending(None);
        settlement.observe_authority(Some(observed("converged")));
        settlement.observe_disk(Some(observed("converged")));
        assert_eq!(settlement.verdict(), SettlementVerdict::NoRetainedIntent);

        // The authority's view arrives: the intent exists, and because the
        // planes were reported the verdict can settle immediately.
        settlement.observe_pending(Some(RetainedIntentFacts {
            intent_id: "intent-1".to_string(),
            target_hash: "stamped".to_string(),
            reason: DocumentWriteDeferredReason::CrdtDeliveryAckPending,
            carries_response_payload: true,
        }));
        assert!(
            settlement.verdict().should_clear_intent(),
            "observations must survive a local view that missed the intent"
        );

        // Had the planes been suppressed, the same authority view would stall.
        let suppressed = RetainedWriteSettlement::new();
        suppressed.observe_pending(Some(RetainedIntentFacts {
            intent_id: "intent-1".to_string(),
            target_hash: "stamped".to_string(),
            reason: DocumentWriteDeferredReason::CrdtDeliveryAckPending,
            carries_response_payload: true,
        }));
        assert!(
            matches!(suppressed.verdict(), SettlementVerdict::Unobserved { .. }),
            "this is the failure the early return produced"
        );
    }
}
