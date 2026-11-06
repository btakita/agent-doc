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
    /// (`#ackedreplicabase`). The plugin also reports `true` when the controller's
    /// record of its own binary-authored writes names the buffer as a canonical
    /// generation those writes alone carried to this canonical
    /// (`#attachauthorattribution`): the buffer then holds no operator edit the
    /// canonical lacks.
    #[serde(default)]
    pub canonical_contains_operator_edits: Option<bool>,
    /// The three-way reconcile (ancestor, buffer, canonical) is clean
    /// (`#editorauth2`; `None`: not computed or unavailable), from the same
    /// ancestor as `canonical_contains_operator_edits` (`#ackedreplicabase`).
    #[serde(default)]
    pub clean_merge_available: Option<bool>,
    /// The canonical text is present and zero-length (`#emptycanonicaldeadlock`;
    /// `None`: not reported, e.g. a plugin generation older than this fact). An
    /// empty canonical holds no text, so it never outranks a non-empty buffer.
    #[serde(default)]
    pub canonical_empty: Option<bool>,
    /// The live buffer is present and zero-length (`None`: not reported).
    #[serde(default)]
    pub buffer_empty: Option<bool>,
    /// The live buffer is exactly the saved document on disk
    /// (`#emptycanonicaldeadlock`; `None`: not reported or disk unreadable).
    #[serde(default)]
    pub buffer_equals_disk: Option<bool>,
    /// This editor already refused an attach of this document on a projection
    /// hold and the operator has supplied no new evidence since
    /// (`#emptycanonicaldeadlock`; `None`: not reported). A later
    /// controller-driven re-register must not take the restarted-IDE adoption
    /// fallback over the buffer that hold protected.
    #[serde(default)]
    pub attach_held_for_operator: Option<bool>,
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
    /// `#emptycanonicaldeadlock`: the controller canonical holds no text
    /// independent of this editor (it is empty, or a fresh controller's reseed
    /// of this editor's own retained replica while the operator-visible buffer
    /// is the saved document), so the live buffer becomes canonical. No settled
    /// shadow is needed: nothing else holds text, so no operator evidence can
    /// change the answer.
    AdoptOperatorBuffer,
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
    let action = unguarded_registration_action(e);
    // `#emptycanonicaldeadlock`: on 2026-11-04 a controller-driven
    // `editor_replica_reregister` re-registered `fpe.md` minutes after the
    // editor held it, found no settled shadow, and took the restarted-IDE
    // adoption fallback: it tried to project the controller canonical over the
    // operator buffer the hold had protected (only the template guard refused
    // it). A held buffer stays held until something proves adoption lossless.
    if action == ApplyCanonical
        && e.attach_held_for_operator == Some(true)
        && e.has_buffer
        && !e.buffer_equals_canonical
        && e.canonical_contains_operator_edits != Some(true)
    {
        HoldOperatorBuffer
    } else {
        action
    }
}

fn unguarded_registration_action(
    e: &RegistrationProjectionEvidence,
) -> RegistrationProjectionAction {
    use RegistrationProjectionAction::*;
    // `#emptycanonicaldeadlock`: an empty canonical holds no text, so it never
    // adopts (or merges) over a buffer that holds some. The buffer becomes
    // canonical instead; there is nothing on the other side to lose.
    let empty_canonical_over_text = e.has_buffer
        && e.canonical_empty == Some(true)
        && e.buffer_empty == Some(false)
        && !e.buffer_equals_canonical;
    if e.defer_canonical_projection_for_pending_local {
        DeferCanonicalProjection
    } else if empty_canonical_over_text {
        AdoptOperatorBuffer
    } else if e.retained_replica_reseed_pending {
        // A fresh controller's empty hub is a synchronization placeholder, not
        // a document projection. Registration has already durably published the
        // retained replica before this decision, so exact equality between that
        // replica and the live buffer is convergence even without a settled
        // shadow. Divergent text with a settled ancestor publishes from it.
        //
        // `#emptycanonicaldeadlock`: without an ancestor the reseeded canonical
        // is still only THIS editor's retained replica: the fresh controller
        // held nothing else. When the operator-visible buffer is also the saved
        // document, no other copy holds text, so the buffer wins outright
        // instead of pausing for operator evidence that cannot exist (fpe.md,
        // 2026-11-04: the controller refused disk because the editor was
        // attached, and the editor refused to attach over its own older
        // replica). The controller keeps the reseeded text as a recovery
        // sidecar. Only an unsaved buffer that differs from both still holds.
        if e.buffer_equals_canonical {
            ApplyCanonical
        } else if e.has_published_shadow && e.has_buffer {
            PublishOperatorBuffer
        } else if e.has_buffer && e.buffer_equals_disk == Some(true) {
            AdoptOperatorBuffer
        } else {
            HoldOperatorBuffer
        }
    } else if unproven_canonical_over_shadowless_buffer(e) {
        // `#restartadoptscanonical`: a plain IDE restart discards the JVM-local
        // settled shadow while the controller hub survives with its own text.
        // Adopting that canonical was the "historical fail-closed" fallback, but
        // with no shadow nothing says which side is newer: the buffer may hold
        // operator text saved to disk (or typed) that the controller never
        // accepted. Editor-visible text is authoritative; a canonical replaces
        // it only with proof that it already holds every operator edit.
        if e.canonical_contains_operator_edits == Some(true) {
            ApplyCanonical
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

/// `#restartadoptscanonical`: the registering editor has no settled shadow (a
/// restarted IDE, or a document this editor never settled), and its readable,
/// non-empty buffer differs from a known canonical. Nothing orders the two
/// generations, so the canonical is unproven. An unreadable buffer, an unknown
/// canonical, and an empty buffer (which holds no text to lose) keep the
/// historical adoption.
fn unproven_canonical_over_shadowless_buffer(e: &RegistrationProjectionEvidence) -> bool {
    !e.has_published_shadow
        && e.has_buffer
        && e.has_canonical
        && !e.buffer_equals_canonical
        && e.buffer_empty != Some(true)
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
        // An unreadable buffer is not divergence. A shadowless buffer reaches
        // here only when adoption loses nothing (`#restartadoptscanonical`:
        // unknown canonical or empty buffer); every other shadowless buffer was
        // held or proven above.
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
            canonical_empty: canonical.map(str::is_empty),
            buffer_empty: buffer.map(str::is_empty),
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
    fn a_reseed_without_a_settled_ancestor_holds_an_unsaved_two_sided_divergence() {
        // A genuine conflict: the reseeded replica and the unsaved buffer both
        // hold real, differing text and disk corroborates neither.
        let evidence = RegistrationProjectionEvidence {
            retained_replica_reseed_pending: true,
            buffer_equals_disk: Some(false),
            ..facts(None, Some("operator text"), Some("replica text"))
        };
        let answer = derive_registration_projection(&evidence);
        assert_eq!(answer.action, HoldOperatorBuffer);
        assert_eq!(
            answer.hold_reason.as_deref(),
            Some(HOLD_RETAINED_RESEED_MISSING_SETTLED_SHADOW)
        );
        // An older plugin that reports no disk fact keeps the same hold.
        let unreported = RegistrationProjectionEvidence {
            buffer_equals_disk: None,
            ..evidence
        };
        assert_eq!(
            derive_registration_projection(&unreported).action,
            HoldOperatorBuffer
        );
    }

    /// `#emptycanonicaldeadlock`: an empty fresh canonical plus an attached
    /// buffer is not a conflict. The buffer becomes canonical with no operator
    /// step, whatever the shadow, disk, or proof facts say.
    #[test]
    fn an_empty_reseed_canonical_adopts_the_live_buffer_without_operator_evidence() {
        for shadow in [None, Some("older")] {
            for disk in [None, Some(false), Some(true)] {
                let evidence = RegistrationProjectionEvidence {
                    retained_replica_reseed_pending: true,
                    buffer_equals_disk: disk,
                    ..facts(shadow, Some("# doc\n\noperator text\n"), Some(""))
                };
                let answer = derive_registration_projection(&evidence);
                assert_eq!(answer.action, AdoptOperatorBuffer, "{shadow:?} {disk:?}");
                assert_eq!(answer.hold_reason, None);
            }
        }
    }

    /// `#emptycanonicaldeadlock`: an empty canonical is never projected over a
    /// non-empty buffer on ANY registration path: not as a retained
    /// projection, not as an ordinary re-register, not via a "proof".
    #[test]
    fn an_empty_canonical_is_never_adopted_over_a_non_empty_buffer() {
        for retained in [false, true] {
            for reseed in [false, true] {
                for covers in [None, Some(false), Some(true)] {
                    for contains in [None, Some(false), Some(true)] {
                        for merge in [None, Some(false), Some(true)] {
                            for shadow in [None, Some(""), Some("s")] {
                                let evidence = RegistrationProjectionEvidence {
                                    canonical_projection_retained: retained,
                                    retained_replica_reseed_pending: reseed,
                                    canonical_covers_retained_frontier: covers,
                                    canonical_contains_operator_edits: contains,
                                    clean_merge_available: merge,
                                    ..facts(shadow, Some("operator text"), Some(""))
                                };
                                let action = derive_registration_projection(&evidence).action;
                                assert!(
                                    !matches!(action, ApplyCanonical | MergeForward),
                                    "{evidence:?} -> {action:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
        // An empty buffer over an empty canonical is convergence.
        let both_empty = RegistrationProjectionEvidence {
            retained_replica_reseed_pending: true,
            ..facts(None, Some(""), Some(""))
        };
        assert_eq!(
            derive_registration_projection(&both_empty).action,
            ApplyCanonical
        );
    }

    /// `#emptycanonicaldeadlock`, the fpe.md shape: the fresh controller's
    /// canonical is this editor's own older retained replica, the buffer holds
    /// operator edits that also reached disk. Nothing else holds text, so the
    /// buffer wins with no operator step.
    #[test]
    fn a_reseed_of_the_editors_own_replica_yields_to_the_saved_buffer() {
        let evidence = RegistrationProjectionEvidence {
            retained_replica_reseed_pending: true,
            buffer_equals_disk: Some(true),
            ..facts(None, Some("replica plus operator edits"), Some("replica"))
        };
        let answer = derive_registration_projection(&evidence);
        assert_eq!(answer.action, AdoptOperatorBuffer);
        assert_eq!(answer.hold_reason, None);
    }

    /// `#emptycanonicaldeadlock`: a controller-driven re-register after an
    /// attach hold must not take the no-shadow adoption fallback over the held
    /// buffer; it holds again unless adoption is proven lossless.
    #[test]
    fn a_reregister_after_an_attach_hold_never_falls_back_to_adoption() {
        let evidence = RegistrationProjectionEvidence {
            attach_held_for_operator: Some(true),
            ..facts(None, Some("operator buffer"), Some("controller canonical"))
        };
        let answer = derive_registration_projection(&evidence);
        assert_eq!(answer.action, HoldOperatorBuffer);
        assert_eq!(
            answer.hold_reason.as_deref(),
            Some(HOLD_AMBIGUOUS_REREGISTER_PROJECTION)
        );
        // `#restartadoptscanonical`: without the hold fact (a restarted IDE
        // forgot it) the shadowless buffer still holds; there is no adoption
        // fallback left to fall back to.
        let fresh = RegistrationProjectionEvidence {
            attach_held_for_operator: None,
            ..evidence.clone()
        };
        assert_eq!(
            derive_registration_projection(&fresh).action,
            HoldOperatorBuffer
        );
        // A proof that canonical contains the operator edits still adopts.
        let proven = RegistrationProjectionEvidence {
            canonical_contains_operator_edits: Some(true),
            ..evidence.clone()
        };
        assert_eq!(
            derive_registration_projection(&proven).action,
            ApplyCanonical
        );
        // Convergence ends the hold.
        let converged = RegistrationProjectionEvidence {
            attach_held_for_operator: Some(true),
            ..facts(None, Some("same"), Some("same"))
        };
        assert_eq!(
            derive_registration_projection(&converged).action,
            ApplyCanonical
        );
    }

    /// `#restartadoptscanonical`: a plain IDE restart while the controller hub
    /// survives. The JVM-local settled shadow is gone and the surviving
    /// canonical differs from the buffer (saved or unsaved, retained
    /// projection or not, whatever the frontier, merge, disk, or forgotten-hold
    /// facts say). Without a containment proof the buffer is held, never
    /// overwritten.
    #[test]
    fn a_restarted_editor_never_adopts_a_differing_canonical_without_proof() {
        for retained in [false, true] {
            for covers in [None, Some(false), Some(true)] {
                for merge in [None, Some(false), Some(true)] {
                    for disk in [None, Some(false), Some(true)] {
                        for held in [None, Some(true)] {
                            for contains in [None, Some(false), Some(true)] {
                                let evidence = RegistrationProjectionEvidence {
                                    canonical_projection_retained: retained,
                                    canonical_covers_retained_frontier: covers,
                                    clean_merge_available: merge,
                                    buffer_equals_disk: disk,
                                    attach_held_for_operator: held,
                                    canonical_contains_operator_edits: contains,
                                    ..facts(
                                        None,
                                        Some("# doc\n\nsaved operator text\n"),
                                        Some("# doc\n\nsurviving controller text\n"),
                                    )
                                };
                                let answer = derive_registration_projection(&evidence);
                                if contains == Some(true) {
                                    // The proof the canonical is newer.
                                    assert_eq!(answer.action, ApplyCanonical, "{evidence:?}");
                                    continue;
                                }
                                assert_eq!(answer.action, HoldOperatorBuffer, "{evidence:?}");
                                assert_eq!(
                                    answer.hold_reason.as_deref(),
                                    Some(if retained {
                                        HOLD_AMBIGUOUS_RETAINED_PROJECTION
                                    } else {
                                        HOLD_AMBIGUOUS_REREGISTER_PROJECTION
                                    }),
                                    "{evidence:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
        // An older plugin that reports no emptiness facts holds the same way.
        let unreported = RegistrationProjectionEvidence {
            canonical_empty: None,
            buffer_empty: None,
            ..facts(None, Some("buffer"), Some("canonical"))
        };
        assert_eq!(
            derive_registration_projection(&unreported).action,
            HoldOperatorBuffer
        );
    }

    /// `#restartadoptscanonical`: a shadowless registration still adopts
    /// whenever adoption loses nothing.
    #[test]
    fn a_restarted_editor_adopts_canonical_only_when_nothing_is_lost() {
        for retained in [false, true] {
            let with = |e: RegistrationProjectionEvidence| RegistrationProjectionEvidence {
                canonical_projection_retained: retained,
                ..e
            };
            for (name, evidence) in [
                ("converged", with(facts(None, Some("same"), Some("same")))),
                (
                    "empty buffer",
                    with(facts(None, Some(""), Some("canonical"))),
                ),
                (
                    "unreadable buffer",
                    with(facts(None, None, Some("canonical"))),
                ),
                ("unknown canonical", with(facts(None, Some("buffer"), None))),
            ] {
                assert_eq!(
                    derive_registration_projection(&evidence).action,
                    ApplyCanonical,
                    "{name} retained={retained}"
                );
            }
        }
        // A restored settled shadow (the persisted one) re-enters the causal
        // decision: a buffer still at its shadow adopts the newer canonical,
        // and a controller still at the shadow takes the operator's buffer.
        assert_eq!(
            derive_registration_projection(&facts(Some("s"), Some("s"), Some("c"))).action,
            ApplyCanonical
        );
        assert_eq!(
            derive_registration_projection(&facts(Some("s"), Some("b"), Some("s"))).action,
            PublishOperatorBuffer
        );
    }

    /// `#attachauthorattribution`, the 2026-11-05 restart shape: a retained
    /// projection, no settled shadow, the buffer is the saved document, and the
    /// editor already held once. Attribution reaches the derivation as the
    /// containment fact, which adopts canonical on that path; without it the
    /// same facts hold `ambiguous-retained-projection`.
    #[test]
    fn an_attributed_restart_buffer_adopts_canonical_and_an_unattributed_one_holds() {
        let restarted = |contains: Option<bool>| RegistrationProjectionEvidence {
            canonical_projection_retained: true,
            buffer_equals_disk: Some(true),
            attach_held_for_operator: Some(true),
            canonical_contains_operator_edits: contains,
            ..facts(
                None,
                Some("- ### 📥 do [#implement]\n"),
                Some("- ### ☑️ [#subagent:impl#3] do [#implement]\n"),
            )
        };
        let adopted = derive_registration_projection(&restarted(Some(true)));
        assert_eq!(adopted.action, ApplyCanonical);
        assert_eq!(adopted.hold_reason, None);
        for unattributed in [None, Some(false)] {
            let held = derive_registration_projection(&restarted(unattributed));
            assert_eq!(held.action, HoldOperatorBuffer, "{unattributed:?}");
            assert_eq!(
                held.hold_reason.as_deref(),
                Some(HOLD_AMBIGUOUS_RETAINED_PROJECTION)
            );
        }
    }

    #[test]
    fn adopt_operator_buffer_round_trips_through_snake_case_json() {
        let json = serde_json::to_value(RegistrationProjection {
            action: AdoptOperatorBuffer,
            hold_reason: None,
        })
        .unwrap();
        assert_eq!(json["action"], "adopt_operator_buffer");
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
