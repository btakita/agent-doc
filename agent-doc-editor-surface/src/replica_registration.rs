//! The editor-replica registration projection decision behind the hot-reloaded
//! native boundary (GH #326 phase 5, the #295 "path 2" lever).
//!
//! When a JetBrains editor registers (or re-registers) its CRDT replica with
//! the Project Controller, three generations of the document can be in play:
//! the last text the editor proved the controller accepted (the settled
//! shadow), the live editor buffer, and the controller canonical the new
//! endpoint bootstrapped from. Registration then has to choose a direction:
//! adopt canonical into the buffer, publish the buffer from its proven shadow,
//! merge the buffer forward, defer to a captured local delta, or refuse and
//! hold the buffer exactly as the operator left it.
//!
//! That choice was the most frequently changed pure decision in
//! `CrdtReplicaManager` (`retainedRegistrationProjectionActionForAttachUtil` and
//! `retainedRegistrationProjectionActionUtil`: five commits between 2026-09-24
//! and 2026-10-07). Every change needed a plugin generation, and on the
//! exact-263 modular package that is an IDE restart. It lives here now: the
//! plugin captures the facts on its registration worker
//! ([`RegistrationProjectionEvidence`]) and asks the `registration_projection`
//! derivation which action to take.
//!
//! The derivation never sees document text. The plugin compares the three
//! generations itself, over the exact UTF-16 text it holds, and reports only
//! which are present and which are equal. Equality is therefore exactly the
//! plugin's own notion of identity (the same one its publication-edge rechecks
//! use), no document crosses the boundary, and no encoding can make two
//! different buffers compare equal. The containment proof and the clean
//! three-way merge are already native calls (`NativePatching`), so the plugin
//! reports their answers as facts too.
//!
//! The derivation decides only *which* action. Executing it (deregistering,
//! recording the failure, publishing, the publication-edge rechecks, and their
//! order) stays in the plugin. A new action needs a plugin generation, because
//! the plugin must know how to execute it; a plugin that does not recognise an
//! answer takes its frozen Kotlin copy of these rules. One parity fixture
//! (`editors/jetbrains/src/test/resources/editor-derivations/registration_projection.json`)
//! is answered by both. New rules land here, never there.

use serde::{Deserialize, Serialize};

/// The hold reason when a retained projection (or a fresh-controller reseed)
/// cannot be reconciled with the live buffer.
pub const HOLD_AMBIGUOUS_RETAINED_PROJECTION: &str = "ambiguous-retained-projection";
/// The hold reason when a fresh-controller reseed meets an editor that has no
/// settled ancestor (an IDE restart discards the JVM-local settled shadow).
pub const HOLD_RETAINED_RESEED_MISSING_SETTLED_SHADOW: &str =
    "retained-reseed-missing-settled-shadow";
/// The hold reason when an ordinary re-register (a native library reload)
/// meets a buffer holding text typed after its settled shadow
/// (`#reloadclobbersoperatortext`).
pub const HOLD_AMBIGUOUS_REREGISTER_PROJECTION: &str = "ambiguous-reregister-projection";

/// What one registration knows when it decides its projection direction.
/// Every field is optional on the wire: after a hot reload this library is
/// newer than the plugin generation calling it. An absent fact is unknown,
/// which the rules treat as the historical fail-closed adoption, exactly as the
/// plugin treats a null text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrationProjectionEvidence {
    /// Captured operator splices are queued: the registration installs an
    /// exact canonical endpoint without projecting it over the editor.
    #[serde(default)]
    pub defer_canonical_projection_for_pending_local: bool,
    /// The controller retained a canonical projection that was never delivered
    /// to this editor.
    #[serde(default)]
    pub canonical_projection_retained: bool,
    /// A fresh controller's empty hub is waiting for this editor's retained
    /// replica (a synchronization placeholder, not a document projection).
    #[serde(default)]
    pub retained_replica_reseed_pending: bool,
    /// Canonical is causally at or beyond the cached native CRDT frontier
    /// (`None`: unknown).
    #[serde(default)]
    pub canonical_covers_retained_frontier: Option<bool>,
    /// The editor holds a settled shadow: the last text it proved the
    /// controller accepted.
    #[serde(default)]
    pub has_published_shadow: bool,
    /// The live editor buffer could be read.
    #[serde(default)]
    pub has_buffer: bool,
    /// The new endpoint holds a canonical text.
    #[serde(default)]
    pub has_canonical: bool,
    /// Buffer and canonical are both present and identical.
    #[serde(default)]
    pub buffer_equals_canonical: bool,
    /// Buffer and settled shadow are both present and identical.
    #[serde(default)]
    pub buffer_equals_shadow: bool,
    /// Canonical and settled shadow are both present and identical.
    #[serde(default)]
    pub canonical_equals_shadow: bool,
    /// Canonical provably contains every operator edit from a proven common
    /// ancestor to the buffer (`#ambiguousholdforever`; `None`: not computed or
    /// unavailable). The ancestor is the settled shadow, or, when that proves
    /// nothing, the cached replica text whose frontier canonical covers
    /// (`#ackedreplicabase`).
    #[serde(default)]
    pub canonical_contains_operator_edits: Option<bool>,
    /// The three-way reconcile (ancestor, buffer, canonical) is clean
    /// (`#editorauth2`; `None`: not computed or unavailable), from the same
    /// ancestor as `canonical_contains_operator_edits` (`#ackedreplicabase`).
    #[serde(default)]
    pub clean_merge_available: Option<bool>,
}

/// The direction a registration projects. Wire tokens are snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationProjectionAction {
    /// Adopt the controller canonical.
    ApplyCanonical,
    /// Publish the live buffer from its proven settled shadow.
    PublishOperatorBuffer,
    /// Refuse the registration and leave the buffer exactly as it is.
    HoldOperatorBuffer,
    /// A captured local delta owns the visible buffer; do not project.
    DeferCanonicalProjection,
    /// Publish the clean three-way merge and project it into the buffer.
    MergeForward,
}

/// The `registration_projection` answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrationProjection {
    pub action: RegistrationProjectionAction,
    /// The registration failure reason a hold records (and the operator
    /// remedy is chosen by); `None` unless the action is a hold.
    pub hold_reason: Option<String>,
}

/// Decide the registration projection direction from causal facts, never
/// wall-clock arrival.
pub fn derive_registration_projection(
    evidence: &RegistrationProjectionEvidence,
) -> RegistrationProjection {
    let action = registration_action(evidence);
    let hold_reason = (action == RegistrationProjectionAction::HoldOperatorBuffer)
        .then(|| hold_reason(evidence).to_string());
    RegistrationProjection {
        action,
        hold_reason,
    }
}

fn registration_action(e: &RegistrationProjectionEvidence) -> RegistrationProjectionAction {
    use RegistrationProjectionAction::*;
    if e.defer_canonical_projection_for_pending_local {
        DeferCanonicalProjection
    } else if e.retained_replica_reseed_pending {
        // A fresh controller's empty hub is a synchronization placeholder, not
        // a document projection. Registration has already durably published the
        // retained replica before this decision, so exact equality between that
        // replica and the live buffer is convergence even without a settled
        // shadow. Divergent text still needs the independently settled ancestor.
        if e.buffer_equals_canonical {
            ApplyCanonical
        } else if e.has_published_shadow && e.has_buffer {
            PublishOperatorBuffer
        } else {
            HoldOperatorBuffer
        }
    } else if e.canonical_projection_retained {
        causal_action(e, e.canonical_covers_retained_frontier)
    } else if e.has_published_shadow
        && e.has_buffer
        && !e.buffer_equals_shadow
        && !e.buffer_equals_canonical
    {
        // `#reloadclobbersoperatortext`: an ordinary re-register is not a
        // retained projection, but the live buffer can still hold text typed
        // while the replica was detached. It gets the same causal decision,
        // without the retained frontier proof.
        causal_action(e, None)
    } else {
        ApplyCanonical
    }
}

/// The causal three-generation decision: a live buffer is published only when
/// canonical is exactly the shadow it was derived from; three distinct
/// generations hold unless a proof or a clean merge gives an exit.
fn causal_action(
    e: &RegistrationProjectionEvidence,
    canonical_covers_retained_frontier: Option<bool>,
) -> RegistrationProjectionAction {
    use RegistrationProjectionAction::*;
    if e.buffer_equals_canonical {
        // Convergence wins before the three-generation test; it is also the
        // terminal edge for a prior ambiguity hold.
        ApplyCanonical
    } else if !e.has_published_shadow || !e.has_buffer {
        // A restarted IDE has no shadow, and an unreadable buffer is not
        // divergence: the historical fail-closed adoption stands.
        ApplyCanonical
    } else if e.buffer_equals_shadow && canonical_covers_retained_frontier == Some(true) {
        // The buffer has not advanced past the last accepted projection and
        // canonical covers the cached native frontier: a safe recovery.
        ApplyCanonical
    } else if e.canonical_equals_shadow {
        PublishOperatorBuffer
    } else if e.canonical_contains_operator_edits == Some(true) {
        // `#ambiguousholdforever`: the proven rebase keeps all operator text.
        ApplyCanonical
    } else if e.clean_merge_available == Some(true) {
        // `#editorauth2`: merge forward instead of holding forever.
        MergeForward
    } else {
        HoldOperatorBuffer
    }
}

fn hold_reason(e: &RegistrationProjectionEvidence) -> &'static str {
    if e.canonical_projection_retained || e.retained_replica_reseed_pending {
        // A full editor restart loses the JVM-local settled shadow; that hold
        // deserves a different remedy from an ordinary three-generation conflict.
        if e.retained_replica_reseed_pending && !e.has_published_shadow {
            HOLD_RETAINED_RESEED_MISSING_SETTLED_SHADOW
        } else {
            HOLD_AMBIGUOUS_RETAINED_PROJECTION
        }
    } else {
        HOLD_AMBIGUOUS_REREGISTER_PROJECTION
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use RegistrationProjectionAction::*;

    /// Facts for three optional texts, computed the way the plugin does.
    fn facts(
        shadow: Option<&str>,
        buffer: Option<&str>,
        canonical: Option<&str>,
    ) -> RegistrationProjectionEvidence {
        let both =
            |a: Option<&str>, b: Option<&str>| matches!((a, b), (Some(a), Some(b)) if a == b);
        RegistrationProjectionEvidence {
            has_published_shadow: shadow.is_some(),
            has_buffer: buffer.is_some(),
            has_canonical: canonical.is_some(),
            buffer_equals_canonical: both(buffer, canonical),
            buffer_equals_shadow: both(buffer, shadow),
            canonical_equals_shadow: both(canonical, shadow),
            ..Default::default()
        }
    }

    #[test]
    fn an_empty_observation_adopts_canonical() {
        let answer = derive_registration_projection(&RegistrationProjectionEvidence::default());
        assert_eq!(answer.action, ApplyCanonical);
        assert_eq!(answer.hold_reason, None);
    }

    #[test]
    fn a_reseed_without_a_settled_ancestor_holds_with_its_own_reason() {
        let evidence = RegistrationProjectionEvidence {
            retained_replica_reseed_pending: true,
            ..facts(None, Some("operator text"), Some(""))
        };
        let answer = derive_registration_projection(&evidence);
        assert_eq!(answer.action, HoldOperatorBuffer);
        assert_eq!(
            answer.hold_reason.as_deref(),
            Some(HOLD_RETAINED_RESEED_MISSING_SETTLED_SHADOW)
        );
    }

    #[test]
    fn a_reload_reregister_over_detached_typing_publishes_or_holds() {
        let publish = facts(Some("s"), Some("b"), Some("s"));
        assert_eq!(
            derive_registration_projection(&publish).action,
            PublishOperatorBuffer
        );
        let hold = derive_registration_projection(&facts(Some("s"), Some("b"), Some("c")));
        assert_eq!(hold.action, HoldOperatorBuffer);
        assert_eq!(
            hold.hold_reason.as_deref(),
            Some(HOLD_AMBIGUOUS_REREGISTER_PROJECTION)
        );
    }

    #[test]
    fn the_answer_round_trips_through_snake_case_json() {
        let answer = RegistrationProjection {
            action: MergeForward,
            hold_reason: None,
        };
        let json = serde_json::to_value(&answer).unwrap();
        assert_eq!(json["action"], "merge_forward");
        assert!(json["hold_reason"].is_null());
    }
}
