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

use crate::{CloseoutStage, DocumentScope, DocumentWriteDeferredReason, DocumentWriteSource};

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
    /// What produced the intent. Carries its position in the closeout sequence,
    /// so "a later stage of my own closeout overtook me" is an ordering
    /// comparison rather than a line-set diff (`#adwritesourceenum`).
    #[serde(default)]
    pub source: DocumentWriteSource,
    /// The stage of a strictly-newer converged write that sits strictly later in
    /// the closeout sequence than [`Self::source`], if one exists.
    ///
    /// Supplied by the projection ([`crate::DocumentProjection::superseding_closeout_stage`])
    /// rather than derived here, because answering it needs the document's write
    /// ordinals — which settlement deliberately does not observe.
    #[serde(default)]
    pub superseding_stage: Option<CloseoutStage>,
    /// True when the intent introduces an assistant response, so materializing
    /// that response in the converged document satisfies the intent even at a
    /// different hash. Delivery-only projections have no such payload and must
    /// settle on exact bytes.
    pub carries_response_payload: bool,
    /// True when the intent adds at least one non-blank line to the content it
    /// expected — i.e. it has a delta whose materialization is checkable.
    ///
    /// A deletion-only or whitespace-only intent adds nothing, so there is no
    /// delta to find in the converged content and it stays on exact bytes.
    pub carries_content_delta: bool,
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
    /// Does this content already contain every non-blank line the retained
    /// intent was going to add? See [`SatisfiedProof::SupersededDeltaMaterialized`].
    #[serde(default)]
    pub intent_delta_materialized: bool,
}

/// Rehydrate the exact-hash settlement inputs owned by the durable document
/// projection.
///
/// The controller's reactive graph is process-local, but editor and disk
/// authority observations are durable facts. Keeping their two planes
/// separately lets a replacement controller rebuild the graph after an
/// installed-binary handoff without waiting for a polling caller to observe
/// the same bytes again.
///
/// The write ordinal is the lineage fence: an observation made before the
/// current retained intent is not evidence for that intent, even when the
/// hashes happen to match. Payload/delta materialization remains false because
/// the durable fact stores only a hash; exact target equality is the only proof
/// this reconstruction can safely provide.
pub fn durable_exact_observations(
    document: &crate::DocumentProjection,
) -> (Option<ContentObservation>, Option<ContentObservation>) {
    let Some(pending) = document.pending_write.as_ref() else {
        return (None, None);
    };
    let observation = |authority: Option<&crate::DocumentAuthorityProjection>| {
        let authority = authority?;
        if authority.write_fact_ordinal < pending.ordinal {
            return None;
        }
        Some(ContentObservation {
            content_hash: authority.content_hash.clone()?,
            payload_materialized: false,
            intent_delta_materialized: false,
        })
    };
    (
        observation(document.latest_editor_authority.as_ref()),
        observation(document.latest_disk_authority.as_ref()),
    )
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
    /// The stamped target was superseded by a **later write of the same cycle**,
    /// and every non-blank line the intent was going to add is present in the
    /// converged content.
    ///
    /// This is not the operator-edit rebase above; it is agent-doc's own
    /// pipeline overtaking itself. A closeout routinely writes twice — the
    /// response/backlog `pending_write`, then the `pending_add_sync` queue
    /// mirror seconds later — and the second target is a superset of the first.
    /// If the closeout is interrupted between them, the first intent is retained
    /// against bytes the document has already moved past, and neither exact
    /// equality nor the response-payload proof can ever fire: the response was
    /// materialized by an *earlier* write, so it is not this intent's payload.
    /// The result is a gate that waits forever for a hash that will never
    /// reappear, refusing every future cycle. Observed 2026-07-26 on
    /// `tasks/agent-doc/agent-doc-bugs2.md` (`pending_write` target 85054 bytes
    /// superseded by `pending_add_sync` at 85183, `delivery_converged=true` for
    /// the whole wait).
    ///
    /// The delta is what makes this safe: the intent's *purpose* is to add
    /// content, and requiring every added line to be present is strictly
    /// stronger than "the document changed". Content that never carried the
    /// write cannot satisfy it.
    SupersededDeltaMaterialized,
    /// A strictly later stage of the **same closeout** converged after this
    /// intent was retained (`#adwritesourceenum`).
    ///
    /// This is [`Self::SupersededDeltaMaterialized`]'s question answered by the
    /// type instead of by a content diff. The stages are sequential — response
    /// write, then queue mirror, then post-commit reposition — and each writes a
    /// document the previous one produced, so a later stage converging *is* the
    /// earlier stage's target being carried forward.
    ///
    /// The delta proof stays: it covers concurrent operator rebases, which no
    /// stage ordering can express. This arm covers the case the delta proof
    /// cannot — an intent whose delta is empty or uncomputable (deletion-only,
    /// whitespace-only, or stamped before `expected_content` was retained),
    /// which `intent_added_lines` deliberately reports as unknown and which
    /// therefore stranded forever.
    SupersededByLaterCloseoutStage,
}

impl SatisfiedProof {
    pub const fn token(self) -> &'static str {
        match self {
            Self::ExactTarget => "exact_target",
            Self::RebasedPayloadMaterialized => "rebased_payload_materialized",
            Self::SupersededDeltaMaterialized => "superseded_delta_materialized",
            Self::SupersededByLaterCloseoutStage => "superseded_by_later_closeout_stage",
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
        /// The settled intent's discriminant, carried so the emitted
        /// `DocumentWriteConverged` fact records *which closeout stage*
        /// converged rather than only who cleared it (`#adwritesourceenum`).
        #[serde(default)]
        intent_source: DocumentWriteSource,
    },
    /// Genuinely still awaiting delivery.
    Unsettled {
        intent_id: String,
        cause: UnsettledCause,
    },
}

/// What preflight should do with the shared settlement verdict before opening
/// a new cycle (`#0dsr`).
///
/// Settlement answers whether the retained write has already landed. Recovery
/// answers the next transition when it has not. Keeping that decision here
/// prevents preflight, session-check, and editor reconnect from inventing
/// separate replay policies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryAction {
    /// No replay is needed or no authoritative observation is available.
    Continue,
    /// Authority and disk still differ, so the existing delivery must settle;
    /// replaying now would race a live write.
    AwaitConvergence { intent_id: String },
    /// Authority and disk agree but do not contain the intent. Waiting cannot
    /// make progress; replay the durable semantic journal over that settled cut.
    ReplayStranded { intent_id: String },
}

impl SettlementVerdict {
    /// The single question `preflight` asks. `Unobserved` answers `false`
    /// deliberately: an unobservable plane is not proof of an outstanding write.
    pub fn blocks_new_cycle(&self) -> bool {
        matches!(self, Self::Unsettled { .. })
    }

    /// The single question `session-check` asks before reporting a completed
    /// closeout. Unlike preflight, closeout must fail closed when an intent is
    /// known but a content plane is unobservable: absence of an observation is
    /// not proof that the retained write settled.
    pub fn blocks_session_closeout(&self) -> bool {
        matches!(self, Self::Unobserved { .. } | Self::Unsettled { .. })
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

    /// The exhaustive preflight recovery decision derived from this verdict.
    pub fn recovery_action(&self) -> RecoveryAction {
        match self {
            Self::NoRetainedIntent | Self::Unobserved { .. } | Self::Satisfied { .. } => {
                RecoveryAction::Continue
            }
            Self::Unsettled {
                intent_id,
                cause: UnsettledCause::AuthorityDiskDiverged,
            } => RecoveryAction::AwaitConvergence {
                intent_id: intent_id.clone(),
            },
            Self::Unsettled {
                intent_id,
                cause: UnsettledCause::PayloadAbsentFromConvergedContent,
            } => RecoveryAction::ReplayStranded {
                intent_id: intent_id.clone(),
            },
        }
    }
}

/// The non-blank lines a retained intent was going to ADD to the content it
/// expected (`SupersededDeltaMaterialized`).
///
/// Line-set rather than a positional diff on purpose: the question is "did this
/// content end up in the document", not "did it end up at this offset". A later
/// superseding write may legitimately place the same lines at a different
/// position (the queue mirror inserts above them), and a positional diff would
/// call that a miss.
///
/// Blank lines are excluded because they carry no evidence — a document with any
/// blank line would otherwise "contain" a whitespace-only intent's whole delta.
/// Without `expected` (an intent stamped before expected-content was retained)
/// there is no baseline to subtract, so the delta is unknown and empty: an
/// unknown delta must not become a settlement proof.
pub fn intent_added_lines<'a>(expected: Option<&str>, target: &'a str) -> Vec<&'a str> {
    let Some(expected) = expected else {
        return Vec::new();
    };
    let baseline: std::collections::HashSet<&str> = expected
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let mut seen = std::collections::HashSet::new();
    target
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter(|line| !baseline.contains(line.trim()))
        .filter(|line| seen.insert(line.trim()))
        .collect()
}

/// Is every line in `added` present in `content`?
///
/// Empty `added` answers `false`, not `true`. Vacuous truth is the dangerous
/// answer here: it would settle every intent whose delta could not be computed.
pub fn added_lines_materialized_in(added: &[&str], content: &str) -> bool {
    if added.is_empty() {
        return false;
    }
    let present: std::collections::HashSet<&str> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    added.iter().all(|line| present.contains(line.trim()))
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
    } else if pending.carries_content_delta && authority.intent_delta_materialized {
        Some(SatisfiedProof::SupersededDeltaMaterialized)
    } else if pending.superseding_stage.is_some() {
        // Ordered last on purpose: it is the weakest of the four, proving the
        // intent's *successor* landed rather than the intent's own content. It
        // only fires once the content-bearing proofs have declined, and only
        // when the projection has established both that the superseding write is
        // newer and that it belongs to a later stage of the same closeout.
        Some(SatisfiedProof::SupersededByLaterCloseoutStage)
    } else {
        None
    };
    match proof {
        Some(proof) => SettlementVerdict::Satisfied {
            intent_id: pending.intent_id.clone(),
            retained_target_hash: pending.target_hash.clone(),
            settled_hash: authority.content_hash.clone(),
            proof,
            intent_source: pending.source.clone(),
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
            source: DocumentWriteSource::PendingWrite,
            superseding_stage: None,
            carries_response_payload,
            carries_content_delta: false,
        }
    }

    fn observed(content_hash: &str, payload_materialized: bool) -> ContentObservation {
        ContentObservation {
            content_hash: content_hash.to_string(),
            payload_materialized,
            intent_delta_materialized: false,
        }
    }

    #[test]
    fn no_intent_never_blocks() {
        let verdict = settlement_verdict(
            None,
            Some(&observed("a", false)),
            Some(&observed("a", false)),
        );
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

    #[test]
    fn converged_missing_payload_requests_stranded_replay() {
        let verdict = settlement_verdict(
            Some(&intent("stamped", true)),
            Some(&observed("settled-current", false)),
            Some(&observed("settled-current", false)),
        );
        assert_eq!(
            verdict.recovery_action(),
            RecoveryAction::ReplayStranded {
                intent_id: "intent-1".to_string(),
            },
            "waiting cannot change a missing payload once authority and disk agree"
        );
    }

    #[test]
    fn diverged_planes_await_existing_delivery_instead_of_replaying() {
        let verdict = settlement_verdict(
            Some(&intent("stamped", true)),
            Some(&observed("authority", false)),
            Some(&observed("disk", false)),
        );
        assert_eq!(
            verdict.recovery_action(),
            RecoveryAction::AwaitConvergence {
                intent_id: "intent-1".to_string(),
            },
            "preflight must not race a write whose authority and disk planes still differ"
        );
    }

    #[test]
    fn satisfied_and_unobserved_verdicts_do_not_replay() {
        let satisfied = settlement_verdict(
            Some(&intent("exact", false)),
            Some(&observed("exact", false)),
            Some(&observed("exact", false)),
        );
        assert_eq!(satisfied.recovery_action(), RecoveryAction::Continue);

        let unobserved = settlement_verdict(Some(&intent("exact", false)), None, None);
        assert_eq!(unobserved.recovery_action(), RecoveryAction::Continue);
    }

    /// The 2026-07-26 deadlock. A closeout writes twice: the `pending_write`
    /// carrying response+backlog, then the `pending_add_sync` queue mirror whose
    /// target is a superset. Interrupted between them, the first intent is
    /// stamped against bytes its own successor already replaced. Its target hash
    /// will never reappear, and `carries_response_payload` is false because the
    /// response cell's hash belongs to an *earlier* write — so before the delta
    /// proof, nothing could settle it and every later cycle was refused.
    #[test]
    fn a_target_superseded_by_the_closeouts_own_later_write_settles_on_its_delta() {
        let mut superseded = intent("pending_write_target", false);
        superseded.carries_content_delta = true;
        let mut converged = observed("queue_mirror_target", false);
        converged.intent_delta_materialized = true;

        let verdict = settlement_verdict(
            Some(&superseded),
            Some(&converged),
            Some(&converged.clone()),
        );
        assert!(
            verdict.should_clear_intent(),
            "an intent whose successor carried its content must not block every future cycle"
        );
        assert!(!verdict.blocks_new_cycle());
        match verdict {
            SettlementVerdict::Satisfied { proof, .. } => {
                assert_eq!(proof, SatisfiedProof::SupersededDeltaMaterialized)
            }
            other => panic!("expected Satisfied, got {other:?}"),
        }
    }

    /// The delta proof must not become a way to settle anything. If the added
    /// lines are NOT all present, the write genuinely has not landed and the
    /// gate must keep holding — that is the invariant protecting operator text.
    #[test]
    fn a_delta_that_did_not_land_still_blocks() {
        let mut superseded = intent("stamped", false);
        superseded.carries_content_delta = true;
        let elsewhere = observed("other", false); // intent_delta_materialized: false

        let verdict = settlement_verdict(
            Some(&superseded),
            Some(&elsewhere),
            Some(&elsewhere.clone()),
        );
        assert!(verdict.blocks_new_cycle());
        assert!(!verdict.should_clear_intent());

        // And an intent with nothing to add cannot borrow another intent's
        // materialization: no delta, no delta proof.
        let no_delta = intent("stamped", false);
        let mut materialized = observed("other", false);
        materialized.intent_delta_materialized = true;
        assert!(
            settlement_verdict(
                Some(&no_delta),
                Some(&materialized),
                Some(&materialized.clone())
            )
            .blocks_new_cycle(),
            "a deletion-only or unknown-delta intent must stay on exact bytes"
        );
    }

    /// The case the delta proof structurally cannot reach (`#adwritesourceenum`).
    ///
    /// `a_delta_that_did_not_land_still_blocks` asserts, correctly, that an
    /// intent with no computable delta "must stay on exact bytes" — which for a
    /// target its own successor already replaced means staying stranded forever.
    /// A deletion-only closeout write, or one stamped before `expected_content`
    /// was retained, lands exactly there. Stage ordering answers it without
    /// weakening the delta rule: the projection has proven a *newer* write from
    /// a *later stage of the same closeout* converged.
    #[test]
    fn an_undeltaable_intent_settles_once_its_own_later_stage_converged() {
        let mut superseded = intent("pending_write_target", false);
        superseded.source = DocumentWriteSource::PendingWrite;
        superseded.carries_content_delta = false;
        superseded.superseding_stage = Some(CloseoutStage::QueueMirror);
        let converged = observed("queue_mirror_target", false);

        let verdict = settlement_verdict(
            Some(&superseded),
            Some(&converged),
            Some(&converged.clone()),
        );
        assert!(
            verdict.should_clear_intent(),
            "the queue mirror converging must settle the response write it superseded"
        );
        assert!(!verdict.blocks_new_cycle());
        match verdict {
            SettlementVerdict::Satisfied { proof, .. } => {
                assert_eq!(proof, SatisfiedProof::SupersededByLaterCloseoutStage)
            }
            other => panic!("expected Satisfied, got {other:?}"),
        }
    }

    /// The stage arm is the weakest proof, so it must never outrank a
    /// content-bearing one — a settled intent should still report *why* it is
    /// settled as precisely as the evidence allows.
    #[test]
    fn content_bearing_proofs_outrank_the_stage_ordering_proof() {
        let mut both = intent("stamped", true);
        both.superseding_stage = Some(CloseoutStage::QueueMirror);
        match settlement_verdict(
            Some(&both),
            Some(&observed("rebased", true)),
            Some(&observed("rebased", true)),
        ) {
            SettlementVerdict::Satisfied { proof, .. } => {
                assert_eq!(proof, SatisfiedProof::RebasedPayloadMaterialized)
            }
            other => panic!("expected Satisfied, got {other:?}"),
        }

        let mut exact = intent("stamped", false);
        exact.superseding_stage = Some(CloseoutStage::PostCommitReposition);
        match settlement_verdict(
            Some(&exact),
            Some(&observed("stamped", false)),
            Some(&observed("stamped", false)),
        ) {
            SettlementVerdict::Satisfied { proof, .. } => {
                assert_eq!(proof, SatisfiedProof::ExactTarget)
            }
            other => panic!("expected Satisfied, got {other:?}"),
        }
    }

    /// Without a proven superseding stage the arm must not fire — otherwise it
    /// becomes a way to settle any intent, which is exactly the operator-text
    /// hazard `a_delta_that_did_not_land_still_blocks` guards.
    #[test]
    fn no_superseding_stage_means_the_stage_arm_never_fires() {
        let mut stranded = intent("stamped", false);
        stranded.superseding_stage = None;
        let elsewhere = observed("other", false);
        let verdict =
            settlement_verdict(Some(&stranded), Some(&elsewhere), Some(&elsewhere.clone()));
        assert!(verdict.blocks_new_cycle());
        match verdict {
            SettlementVerdict::Unsettled { cause, .. } => {
                assert_eq!(cause, UnsettledCause::PayloadAbsentFromConvergedContent)
            }
            other => panic!("expected Unsettled, got {other:?}"),
        }

        // Diverged planes still win: a superseding stage must not paper over a
        // delivery that is still in flight.
        let mut superseded = intent("stamped", false);
        superseded.superseding_stage = Some(CloseoutStage::QueueMirror);
        assert!(
            settlement_verdict(
                Some(&superseded),
                Some(&observed("authority", false)),
                Some(&observed("disk", false)),
            )
            .blocks_new_cycle(),
            "authority/disk divergence must still block"
        );
    }

    #[test]
    fn the_delta_is_the_lines_the_intent_adds_and_an_unknown_baseline_yields_none() {
        let expected = "alpha\n\nbeta\n";
        let target = "alpha\n\nbeta\n\nadded one\nadded two\n";
        assert_eq!(
            intent_added_lines(Some(expected), target),
            vec!["added one", "added two"]
        );

        // Position is not part of the question: a superseding write may place the
        // same lines elsewhere, and a positional diff would call that a miss.
        let added = intent_added_lines(Some(expected), target);
        assert!(added_lines_materialized_in(
            &added,
            "preamble\nadded one\nalpha\nadded two\nbeta\n"
        ));
        assert!(!added_lines_materialized_in(
            &added,
            "alpha\nbeta\nadded one\n"
        ));

        // No baseline means the delta is UNKNOWN, not empty-and-satisfied.
        assert!(intent_added_lines(None, target).is_empty());
        // Vacuous truth is the dangerous answer: an empty delta proves nothing.
        assert!(!added_lines_materialized_in(&[], "anything at all"));
        // A whitespace-only intent carries no evidence either.
        assert!(intent_added_lines(Some("alpha\n"), "alpha\n\n\n").is_empty());
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
            assert!(
                verdict.blocks_session_closeout(),
                "session-check must not report success without observing settlement",
            );
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
    use lazily::{ThreadSafeComputedMap, ThreadSafeContext, ThreadSafeSourceMap};

    /// Does a `ThreadSafeComputedMap` entry that reads a `ThreadSafeSourceMap`
    /// through its own tracking view actually subscribe to that cell?
    ///
    /// lazily 0.50 hands the slot factory the entry's tracking view
    /// (`Fn(&ThreadSafeContext, &K) -> V`); before that the only way to derive
    /// across maps was a captured context clone, and this probe existed to prove
    /// the clone tracked at all. The question it answers is still the one that
    /// matters — "Computed in name only" fails silently and looks like a stale
    /// value much later — so the probe stays, now pointed at the supported API a
    /// keyed derived registry is actually built on.
    #[test]
    fn slot_map_entry_tracks_a_cell_map_dependency_through_its_tracking_view() {
        let ctx = ThreadSafeContext::new();
        let inputs: ThreadSafeSourceMap<String, u32> = ThreadSafeSourceMap::new(&ctx);
        let derived: ThreadSafeComputedMap<String, u32> = ThreadSafeComputedMap::new(&ctx);
        inputs.set(&ctx, "doc".to_string(), 1);

        let factory_inputs = inputs.clone();
        let read = |derived: &ThreadSafeComputedMap<String, u32>| {
            let factory_inputs = factory_inputs.clone();
            derived.get_or_insert_with(&ctx, "doc".to_string(), move |ctx, key| {
                factory_inputs.observe(ctx, key).unwrap_or(0) + 100
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
            intent_delta_materialized: false,
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
            source: DocumentWriteSource::PendingWrite,
            superseding_stage: None,
            carries_response_payload: true,
            carries_content_delta: false,
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
            source: DocumentWriteSource::PendingWrite,
            superseding_stage: None,
            carries_response_payload: true,
            carries_content_delta: false,
        }));
        assert!(
            matches!(suppressed.verdict(), SettlementVerdict::Unobserved { .. }),
            "this is the failure the early return produced"
        );
    }
}
