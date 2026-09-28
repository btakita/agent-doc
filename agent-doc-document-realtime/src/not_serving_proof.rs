//! What proves a live editor endpoint will not serve a document.
//!
//! # Why this exists
//!
//! `attached_editor_never_descends_to_disk` protects unsaved operator text: while
//! something is serving the buffer, disk is a stale replica and must not be read.
//! The invariant is right. Its *precondition* is an attachment latch, and a latch
//! can outlive the thing it stands for — a cdylib generation swap drops the
//! document's replica while the latch still says attached. That resting state has
//! no outgoing transition, which is why the only recovery was an operator
//! reopening the editor tab.
//!
//! `formal/tla/EditorReplicaStrand.tla` closed that for one shape: an endpoint
//! that ANSWERS and REFUSES proves it no longer serves the document, so the latch
//! may demote. That covered `IPC receipt rejected` and, later, an exhausted build
//! mismatch.
//!
//! # The shape it did not cover
//!
//! An endpoint can also **accept** the re-registration request and then never
//! produce a replica. The editor process is alive, its socket answers, it is
//! serving other documents, and it says yes to every request — the replica for
//! *this* document simply belonged to the native generation the reload retired,
//! and nothing re-creates it until the tab is touched.
//!
//! That lands in neither existing bucket. It is not a refusal, so no proof is
//! recorded; it is not silence, so the caller reads `notified > 0` as progress
//! and waits. The re-registration loop spends its whole bounded budget, records
//! `self_heal_exhausted` so it stops retrying, and that exhaustion is never
//! promoted to a proof — so `decide_authority_recovery` stays in `FailClosed`
//! forever.
//!
//! The model had the same gap, in the module written to stop exactly this kind of
//! gap: `ReregisterAccepted` was guarded by `endpointServes`, so "accepted"
//! implied "rebuilt" by construction. Convergence was an axiom again, just moved
//! from the rejection edge to the acceptance edge.
//!
//! # Why acceptance-exhaustion is proof and a timeout is not
//!
//! The distinction the rest of this design rests on is *who answered*, never *how
//! long we waited* — treating a timeout as a verdict would turn the latch's exit
//! into a silent `--force-disk`. Acceptance-without-service keeps that rule:
//!
//! * the endpoint **answered**, repeatedly, so it is not absent or unreachable;
//! * the liveness witness **did not change** across the whole bounded loop, so no
//!   new registration appeared that could have served the document. A re-attach
//!   always produces a new registration and therefore a new witness, which is
//!   what makes this evidence expire on its own rather than latch;
//! * therefore the only endpoint that could serve this document has said yes N
//!   times and still holds no model for it. Retrying re-asks the same endpoint,
//!   in the same generation, the same question.
//!
//! Absence of an answer stays retryable, which is the half that must not move:
//! nothing answered is not evidence that nothing will.

/// Why an endpoint is proven not to serve a document.
///
/// Kept as distinct variants because they are distinct diagnoses: one says the
/// endpoint declined, the other says it agreed and did nothing. Collapsing them
/// would hide which half of the design is failing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotServingProof {
    /// A live endpoint answered and refused — a receipt, or a build mismatch
    /// whose reload-only recovery was already exhausted.
    AnsweredAndRefused,
    /// A live endpoint accepted every request and never produced the replica,
    /// across a fully spent budget and an unchanged liveness witness.
    AcceptedWithoutServing,
}

impl NotServingProof {
    pub fn token(self) -> &'static str {
        match self {
            Self::AnsweredAndRefused => "answered_and_refused",
            Self::AcceptedWithoutServing => "accepted_without_serving",
        }
    }
}

/// Whether a spent re-registration loop proved the endpoint will not serve.
///
/// `budget_spent` must mean the loop ran to exhaustion: a caller that gave up
/// early has not shown the endpoint anything, and reporting proof from a partial
/// attempt is how a bound becomes a guess.
///
/// `corroborated` is what keeps the acceptance clause away from a busy editor.
/// The budget is 3 attempts at a 250ms backoff -- under a second -- and an IDE
/// that is indexing or in a GC pause will accept every request inside that
/// window while a perfectly healthy re-registration is still in flight. One
/// spent budget is therefore not enough to distinguish "retired generation" from
/// "slow": both look identical for ~750ms. So the acceptance clause requires the
/// document to have been observed unserved at the SAME liveness witness on an
/// earlier, independent resolve. A slow editor that lands its replica between the
/// two observations reports healthy, which clears the memo and the corroboration
/// with it, so it can never reach this clause.
///
/// A refusal needs no corroboration: the endpoint stated the answer, and a second
/// identical statement adds nothing.
pub fn reregistration_not_serving_proof(
    definitive_refusals: usize,
    accepted_requests: usize,
    budget_spent: bool,
    corroborated: bool,
) -> Option<NotServingProof> {
    if definitive_refusals > 0 {
        return Some(NotServingProof::AnsweredAndRefused);
    }
    if budget_spent && accepted_requests > 0 && corroborated {
        return Some(NotServingProof::AcceptedWithoutServing);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{NotServingProof, reregistration_not_serving_proof};

    /// A refusal is terminal the moment it happens; it does not wait for the
    /// budget, and this is the behaviour that already shipped.
    #[test]
    fn a_refusal_proves_it_immediately() {
        assert_eq!(
            reregistration_not_serving_proof(1, 0, false, false),
            Some(NotServingProof::AnsweredAndRefused)
        );
    }

    /// The cdylib strand. The endpoint said yes to every request, the replica
    /// never appeared, and an earlier resolve at the same witness saw the same
    /// thing — so the latch has a proof it can demote on.
    #[test]
    fn corroborated_acceptance_across_a_spent_budget_proves_it() {
        assert_eq!(
            reregistration_not_serving_proof(0, 3, true, true),
            Some(NotServingProof::AcceptedWithoutServing)
        );
    }

    /// The busy-editor guard. One spent budget is under a second; an IDE in a GC
    /// pause accepts every request in that window with a healthy re-registration
    /// still in flight. A single observation must not demote it.
    #[test]
    fn a_single_uncorroborated_budget_does_not_prove_it() {
        assert_eq!(reregistration_not_serving_proof(0, 3, true, false), None);
    }

    /// The half that must not move. Nothing answered, so nothing is proven —
    /// treating this as proof would make the latch's exit a silent `--force-disk`
    /// against an editor that is merely slow or briefly unreachable.
    #[test]
    fn silence_proves_nothing_however_long_it_lasts() {
        assert_eq!(reregistration_not_serving_proof(0, 0, true, true), None);
    }

    /// A caller that gave up early has not shown the endpoint anything.
    #[test]
    fn an_unspent_budget_proves_nothing() {
        assert_eq!(reregistration_not_serving_proof(0, 3, false, true), None);
    }

    /// A refusal outranks acceptance when both occurred: it is the stronger and
    /// more specific statement, and the diagnosis should say so.
    #[test]
    fn a_refusal_outranks_acceptance_in_the_diagnosis() {
        assert_eq!(
            reregistration_not_serving_proof(1, 3, true, false),
            Some(NotServingProof::AnsweredAndRefused)
        );
    }

    #[test]
    fn tokens_are_distinct_and_stable() {
        assert_eq!(
            NotServingProof::AnsweredAndRefused.token(),
            "answered_and_refused"
        );
        assert_eq!(
            NotServingProof::AcceptedWithoutServing.token(),
            "accepted_without_serving"
        );
    }
}
