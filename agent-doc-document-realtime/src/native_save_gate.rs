//! Why the one-shot editor-native save gate is closed (`#savegateopaque`).
//!
//! The native save is the only way a retained write reaches disk while a live
//! editor owns the buffer, and it is gated on two independent facts: the visible
//! editor-projection **receipt** for the exact target hash, and the live editor
//! authority being **sufficient** (same text, a live editor, delivery converged).
//! When either is false no save request is sent at all — `request_sent=false` —
//! and the closeout stays retained.
//!
//! Until now that whole conjunction collapsed into one token,
//! `native_save_gate_not_ready`, which named the gate but not which half of it
//! was shut. Two consequences, both observed live:
//!
//! * The operator-facing `operator_action` was derived by asking
//!   [`ReplicaSignalClass::from_diagnosis_token`] about that token. It is not a
//!   replica-signal token, so the lookup returned `None` and the action was
//!   `none` — "nothing for you to do" — for every closed-gate shape alike,
//!   including ones only a human can clear. Observed 2026-09-28 on
//!   `tasks/agent-doc/agent-doc-bugs.md`: the operator was told to stand down for
//!   38 minutes while no save request had ever been made, and again the same day
//!   on `src/haiven-dev/tasks/infra.md`.
//! * Diagnosing a wedge meant reconstructing the gate's inputs from surrounding
//!   `crdt_current_text` lines, because the one line that names the gate records
//!   none of them.
//!
//! This module classifies the closure instead. It is pure policy over facts the
//! caller already has; the adapter logs the token and applies the action.
//!
//! **It changes no write behaviour.** A closed gate still sends no request, still
//! writes no disk bytes, and a retained write still commits itself once delivery
//! converges. What changes is only whether the operator can tell a gate that is
//! still converging apart from one that never will.

/// The fact that is holding the native-save gate shut.
///
/// Ordered by precedence: the receipt is checked first because the authority
/// observation is meaningless without it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeSaveGateBlocker {
    /// No visible editor-projection receipt for this exact target hash, and no
    /// proof that the endpoint refused. The genuinely transient shape: the
    /// receipt is still in flight.
    ///
    /// `delivery_converged` is availability; the receipt is the proof the bytes
    /// are visible in the buffer, and the gap between them is ordinary. Observed
    /// 2026-09-28 on `src/haiven-dev/tasks/infra.md`: the gate sat here for
    /// roughly nine minutes and then converged and committed on its own, so
    /// escalating this shape would raise a false alarm on a healthy wait.
    ReceiptPending,
    /// The endpoint ANSWERED and refused, and that refusal is still current for
    /// this liveness witness. A refused replica vetoes the visible-delivery
    /// receipt forever (`#refusedreceiptveto`), so no amount of waiting clears
    /// this — it is the one closed-gate shape with terminal proof behind it.
    ReceiptVetoedByRefusedEndpoint,
    /// The live editor authority no longer matches the retained target: the
    /// buffer moved on, so this target can never be the one saved.
    AuthorityTextDiverged,
    /// No live editor owns the buffer, so there is nobody to ask to save.
    NoLiveEditor,
    /// Delivery has not converged yet. The genuinely transient shape.
    DeliveryUnconverged,
    /// The relay could not answer at all.
    AuthorityUnobservable,
}

impl NativeSaveGateBlocker {
    /// Classify a closed gate from the facts the caller already observed.
    ///
    /// `authority` is `None` when the relay did not answer `Current`.
    pub fn classify(
        visible_receipt: bool,
        endpoint_definitively_refused: bool,
        authority: Option<NativeSaveAuthorityObservation<'_>>,
    ) -> Self {
        if !visible_receipt && endpoint_definitively_refused {
            return Self::ReceiptVetoedByRefusedEndpoint;
        }
        let Some(observation) = authority else {
            return Self::AuthorityUnobservable;
        };
        if !visible_receipt {
            return Self::ReceiptPending;
        }
        if observation.live_editors == 0 {
            return Self::NoLiveEditor;
        }
        if observation.authoritative_text != observation.canonical {
            return Self::AuthorityTextDiverged;
        }
        if !observation.delivery_converged {
            return Self::DeliveryUnconverged;
        }
        // Every conjunct holds, so the gate is not closed at all. Callers only
        // classify a closed gate; report the receipt as the blocker rather than
        // inventing a variant that cannot be reached from a shut gate.
        Self::ReceiptPending
    }

    /// The `save_diagnosis` token for this blocker.
    ///
    /// Kept under the historical `native_save_gate_not_ready` prefix so existing
    /// log greps and the `editor_projection_persistence_pending` assertions keep
    /// matching, with the cause appended after a colon.
    pub fn diagnosis_token(self) -> &'static str {
        match self {
            Self::ReceiptPending => "native_save_gate_not_ready:receipt_pending",
            Self::ReceiptVetoedByRefusedEndpoint => {
                "native_save_gate_not_ready:receipt_vetoed_by_refused_endpoint"
            }
            Self::AuthorityTextDiverged => "native_save_gate_not_ready:authority_text_diverged",
            Self::NoLiveEditor => "native_save_gate_not_ready:no_live_editor",
            Self::DeliveryUnconverged => "native_save_gate_not_ready:delivery_unconverged",
            Self::AuthorityUnobservable => "native_save_gate_not_ready:authority_unobservable",
        }
    }

    /// Whether clearing this blocker needs a human to look at the editor.
    ///
    /// The bar is the one `#refusedsaveopaque` set for save *outcomes*: escalate
    /// only what was reached, ANSWERED, and refused. Everything else — a receipt
    /// still in flight, an unconverged delivery, a buffer that moved on, an
    /// editor that is restarting, a relay that did not answer this once — may
    /// still clear itself, and `infra.md` proved that a nine-minute wait here is
    /// a healthy one. Escalating an unproven shape trades a stand-down-forever
    /// wedge for a cry-wolf, which is the same defect wearing the other sign.
    ///
    /// So the escalation is evidence-gated, not timeout-gated: exactly one
    /// variant carries terminal proof, and only it reaches the operator.
    ///
    /// This does **not** authorize a disk write. It changes the reported action
    /// only; the no-force-disk contract is untouched.
    pub fn needs_operator_inspection(self) -> bool {
        matches!(self, Self::ReceiptVetoedByRefusedEndpoint)
    }
}

/// The live editor authority facts the gate reads, as one borrowed record so a
/// caller cannot pass them in the wrong order.
#[derive(Debug, Clone, Copy)]
pub struct NativeSaveAuthorityObservation<'a> {
    pub authoritative_text: &'a str,
    pub canonical: &'a str,
    pub live_editors: usize,
    pub delivery_converged: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation<'a>(
        text: &'a str,
        canonical: &'a str,
        live_editors: usize,
        delivery_converged: bool,
    ) -> NativeSaveAuthorityObservation<'a> {
        NativeSaveAuthorityObservation {
            authoritative_text: text,
            canonical,
            live_editors,
            delivery_converged,
        }
    }

    /// The live `infra.md` shape: a live editor, converged delivery, matching
    /// text, and still no receipt — with NO refusal on record. It committed on
    /// its own about nine minutes later, so this must stay a quiet wait.
    #[test]
    fn a_pending_receipt_without_a_refusal_does_not_cry_wolf() {
        let blocker = NativeSaveGateBlocker::classify(
            false,
            false,
            Some(observation("same", "same", 1, true)),
        );
        assert_eq!(blocker, NativeSaveGateBlocker::ReceiptPending);
        assert!(
            !blocker.needs_operator_inspection(),
            "a receipt still in flight is a healthy wait, not an operator task"
        );
        assert_eq!(
            blocker.diagnosis_token(),
            "native_save_gate_not_ready:receipt_pending"
        );
    }

    /// The same missing receipt, but the endpoint ANSWERED and refused. That
    /// veto never clears, so this is the one shape that must reach a human.
    #[test]
    fn a_receipt_vetoed_by_a_refused_endpoint_reaches_the_operator() {
        let blocker = NativeSaveGateBlocker::classify(
            false,
            true,
            Some(observation("same", "same", 1, true)),
        );
        assert_eq!(
            blocker,
            NativeSaveGateBlocker::ReceiptVetoedByRefusedEndpoint
        );
        assert!(blocker.needs_operator_inspection());
        assert_eq!(
            blocker.diagnosis_token(),
            "native_save_gate_not_ready:receipt_vetoed_by_refused_endpoint"
        );
    }

    /// The escalation is evidence-gated, not timeout-gated: proven refusal is
    /// the ONLY thing that reaches the operator. Every other closed-gate shape
    /// may still clear itself, and reporting it would be the cry-wolf dual of
    /// the stand-down-forever wedge.
    #[test]
    fn only_a_proven_refusal_escalates() {
        for quiet in [
            NativeSaveGateBlocker::ReceiptPending,
            NativeSaveGateBlocker::AuthorityTextDiverged,
            NativeSaveGateBlocker::NoLiveEditor,
            NativeSaveGateBlocker::DeliveryUnconverged,
            NativeSaveGateBlocker::AuthorityUnobservable,
        ] {
            assert!(
                !quiet.needs_operator_inspection(),
                "{quiet:?} is unproven and must not raise an operator action"
            );
        }
        assert!(
            NativeSaveGateBlocker::ReceiptVetoedByRefusedEndpoint.needs_operator_inspection()
        );
    }

    /// A refusal on record outranks every other reading: it is terminal proof,
    /// and the authority observation cannot contradict it.
    #[test]
    fn a_recorded_refusal_outranks_an_unobservable_relay() {
        assert_eq!(
            NativeSaveGateBlocker::classify(false, true, None),
            NativeSaveGateBlocker::ReceiptVetoedByRefusedEndpoint
        );
    }

    /// A refusal recorded against an older witness is already cleared upstream,
    /// so with the receipt PRESENT the gate must not report a veto.
    #[test]
    fn a_present_receipt_is_never_reported_as_vetoed() {
        assert_eq!(
            NativeSaveGateBlocker::classify(true, true, Some(observation("same", "same", 1, false))),
            NativeSaveGateBlocker::DeliveryUnconverged
        );
    }

    #[test]
    fn each_blocker_has_a_distinct_token_under_the_historical_prefix() {
        let all = [
            NativeSaveGateBlocker::ReceiptPending,
            NativeSaveGateBlocker::ReceiptVetoedByRefusedEndpoint,
            NativeSaveGateBlocker::AuthorityTextDiverged,
            NativeSaveGateBlocker::NoLiveEditor,
            NativeSaveGateBlocker::DeliveryUnconverged,
            NativeSaveGateBlocker::AuthorityUnobservable,
        ];
        let mut tokens: Vec<&str> = all.iter().map(|b| b.diagnosis_token()).collect();
        tokens.sort_unstable();
        let before = tokens.len();
        tokens.dedup();
        assert_eq!(tokens.len(), before, "tokens must be distinguishable");
        for token in tokens {
            assert!(
                token.starts_with("native_save_gate_not_ready:"),
                "{token} must stay greppable under the historical prefix"
            );
        }
    }

    #[test]
    fn an_unobservable_relay_is_not_reported_as_a_missing_receipt() {
        let blocker = NativeSaveGateBlocker::classify(true, false, None);
        assert_eq!(blocker, NativeSaveGateBlocker::AuthorityUnobservable);
    }

    #[test]
    fn no_live_editor_outranks_a_text_mismatch() {
        // With nobody holding the buffer there is no save to request, and the
        // text comparison is against a buffer that no longer exists.
        assert_eq!(
            NativeSaveGateBlocker::classify(true, false, Some(observation("a", "b", 0, true))),
            NativeSaveGateBlocker::NoLiveEditor
        );
    }
}
