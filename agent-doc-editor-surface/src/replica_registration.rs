//! The editor-replica registration projection decision behind the hot-reloaded
//! native boundary (GH #326 phase 5, the #295 "path 2" lever).
//!
//! When a JetBrains editor registers (or re-registers) its CRDT replica with
//! the Project Controller, three generations of the document can be in play:
//! the last text the editor proved the controller accepted (the settled
//! shadow), the live editor buffer, and the controller projection the new
//! endpoint bootstrapped from. The live IDE buffer is the document authority;
//! the controller copy is only a projection. Registration therefore converges
//! an equal cut, defers while a captured local delta owns delivery, or publishes
//! the exact readable IDE buffer into the replacement endpoint. The exception
//! is a write the binary already accepted (`#acceptedwriterollforward`): when
//! the controller's own op record proves the buffer (or its settled shadow) is
//! a generation that only binary-authored writes carried to the canonical, the
//! buffer is the base and those writes roll forward onto it instead of being
//! deleted by a republished stale buffer.
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
    /// `#acceptedwriterollforward`: the controller's op record of its own
    /// binary-authored writes (`#attachauthorattribution`) names the live
    /// buffer, by content hash, as a canonical generation from which only those
    /// writes lead to this canonical (`None`: not reported). The buffer then
    /// holds no text the canonical lacks: it is a stale copy of writes the
    /// binary already accepted (an `--ack-text` strike, a `status set`
    /// payload), and projecting the canonical onto it rolls them forward.
    #[serde(default)]
    pub buffer_is_binary_attributed_ancestor: Option<bool>,
    /// `#acceptedwriterollforward`: the same op-record proof for the settled
    /// shadow. With a clean merge from that shadow (`clean_merge_available`),
    /// the operator's edits since the shadow and the binary's accepted writes
    /// since it both survive (`None`: not reported).
    #[serde(default)]
    pub shadow_is_binary_attributed_ancestor: Option<bool>,
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
    if e.defer_canonical_projection_for_pending_local {
        DeferCanonicalProjection
    } else if e.has_buffer && !e.buffer_equals_canonical {
        // `#idebufferauthority`: a readable IDE buffer is the source of truth.
        // A controller copy may contain every historical operator edit and
        // still not equal what the operator sees now. No containment, shadow,
        // or disk reading licenses replacing it.
        //
        // `#acceptedwriterollforward`: the one exception is the controller's
        // op record, which is not a reading of the text. When it names the
        // buffer (or the buffer's settled shadow) as a generation that only
        // binary-authored writes carried to this canonical, publishing the
        // buffer would delete writes the binary already accepted (the
        // 2026-11-24 02:17Z ack-text strike and the reverted `status set`
        // payloads on agent-doc-bugs.md). The buffer stays the base and those
        // writes roll forward onto it; an empty canonical is never projected.
        let rollable = e.canonical_empty != Some(true);
        if rollable && e.buffer_is_binary_attributed_ancestor == Some(true) {
            ApplyCanonical
        } else if rollable
            && e.shadow_is_binary_attributed_ancestor == Some(true)
            && e.has_published_shadow
            && !e.buffer_equals_shadow
            && e.clean_merge_available == Some(true)
        {
            MergeForward
        } else {
            AdoptOperatorBuffer
        }
    } else {
        ApplyCanonical
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
    fn a_reseed_without_a_settled_ancestor_publishes_the_ide_buffer() {
        let evidence = RegistrationProjectionEvidence {
            retained_replica_reseed_pending: true,
            buffer_equals_disk: Some(false),
            ..facts(None, Some("operator text"), Some("replica text"))
        };
        let answer = derive_registration_projection(&evidence);
        assert_eq!(answer.action, AdoptOperatorBuffer);
        assert_eq!(answer.hold_reason, None);
        // Disk corroboration is not registration authority.
        let unreported = RegistrationProjectionEvidence {
            buffer_equals_disk: None,
            ..evidence
        };
        assert_eq!(
            derive_registration_projection(&unreported).action,
            AdoptOperatorBuffer
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

    /// `#idebufferauthority`: prior holds and containment evidence are
    /// diagnostics; a readable mismatch always publishes the IDE buffer.
    #[test]
    fn a_reregister_after_an_attach_hold_still_publishes_the_ide_buffer() {
        let evidence = RegistrationProjectionEvidence {
            attach_held_for_operator: Some(true),
            ..facts(None, Some("operator buffer"), Some("controller canonical"))
        };
        let answer = derive_registration_projection(&evidence);
        assert_eq!(answer.action, AdoptOperatorBuffer);
        assert_eq!(answer.hold_reason, None);
        let proven = RegistrationProjectionEvidence {
            canonical_contains_operator_edits: Some(true),
            ..evidence.clone()
        };
        assert_eq!(
            derive_registration_projection(&proven).action,
            AdoptOperatorBuffer
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

    /// `#idebufferauthority`: no diagnostic proof changes mismatch direction.
    #[test]
    fn every_readable_ide_mismatch_publishes_the_ide_buffer() {
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
                                assert_eq!(answer.action, AdoptOperatorBuffer, "{evidence:?}");
                                assert_eq!(answer.hold_reason, None, "{evidence:?}");
                            }
                        }
                    }
                }
            }
        }
        // An older plugin that reports no emptiness facts gets the same answer.
        let unreported = RegistrationProjectionEvidence {
            canonical_empty: None,
            buffer_empty: None,
            ..facts(None, Some("buffer"), Some("canonical"))
        };
        assert_eq!(
            derive_registration_projection(&unreported).action,
            AdoptOperatorBuffer
        );
    }

    /// Equality and an unreadable buffer are the only controller-projection
    /// paths; empty and unknown controller cuts are still mismatches.
    #[test]
    fn equality_and_unreadable_buffer_accept_the_controller_projection() {
        for retained in [false, true] {
            let with = |e: RegistrationProjectionEvidence| RegistrationProjectionEvidence {
                canonical_projection_retained: retained,
                ..e
            };
            for (name, evidence) in [
                ("converged", with(facts(None, Some("same"), Some("same")))),
                (
                    "unreadable buffer",
                    with(facts(None, None, Some("canonical"))),
                ),
            ] {
                assert_eq!(
                    derive_registration_projection(&evidence).action,
                    ApplyCanonical,
                    "{name} retained={retained}"
                );
            }
            assert_eq!(
                derive_registration_projection(&with(facts(None, Some(""), Some("canonical"))))
                    .action,
                AdoptOperatorBuffer,
            );
            assert_eq!(
                derive_registration_projection(&with(facts(None, Some("buffer"), None))).action,
                AdoptOperatorBuffer,
            );
        }
        // Shadows do not change direction.
        assert_eq!(
            derive_registration_projection(&facts(Some("s"), Some("s"), Some("c"))).action,
            AdoptOperatorBuffer
        );
        assert_eq!(
            derive_registration_projection(&facts(Some("s"), Some("b"), Some("s"))).action,
            AdoptOperatorBuffer
        );
    }

    /// `#attachauthorattribution` remains diagnostic only: neither a positive
    /// nor negative attribution result may override a readable IDE buffer.
    #[test]
    fn attribution_never_overrides_the_readable_ide_buffer() {
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
        assert_eq!(adopted.action, AdoptOperatorBuffer);
        assert_eq!(adopted.hold_reason, None);
        for unattributed in [None, Some(false)] {
            let held = derive_registration_projection(&restarted(unattributed));
            assert_eq!(held.action, AdoptOperatorBuffer, "{unattributed:?}");
            assert_eq!(held.hold_reason, None);
        }
    }

    /// `#acceptedwriterollforward` (agent-doc-bugs.md, 2026-11-24 02:17Z): an
    /// `--ack-text` strike reached canonical, then the editor re-registered
    /// with a buffer that lacked it. The op record names that buffer as the
    /// strike's pre-image, so the strike rolls forward onto the buffer instead
    /// of the buffer's republication deleting it.
    #[test]
    fn a_stale_buffer_the_op_record_attributes_rolls_accepted_writes_forward() {
        let stale = "- ### 📥 do [#docscleanup]\n";
        let struck = "- ~~### 📥 do [#docscleanup]~~\n";
        for retained in [false, true] {
            for shadow in [None, Some(stale)] {
                let evidence = RegistrationProjectionEvidence {
                    canonical_projection_retained: retained,
                    buffer_is_binary_attributed_ancestor: Some(true),
                    ..facts(shadow, Some(stale), Some(struck))
                };
                let answer = derive_registration_projection(&evidence);
                assert_eq!(answer.action, ApplyCanonical, "{evidence:?}");
                assert_eq!(answer.hold_reason, None);
            }
        }
        // Without the op record the readable buffer still wins.
        for unproven in [None, Some(false)] {
            let evidence = RegistrationProjectionEvidence {
                buffer_is_binary_attributed_ancestor: unproven,
                ..facts(Some(stale), Some(stale), Some(struck))
            };
            assert_eq!(
                derive_registration_projection(&evidence).action,
                AdoptOperatorBuffer
            );
        }
        // A pending local delivery still defers, and an empty canonical is
        // never projected, whatever the record says.
        let pending = RegistrationProjectionEvidence {
            defer_canonical_projection_for_pending_local: true,
            buffer_is_binary_attributed_ancestor: Some(true),
            ..facts(None, Some(stale), Some(struck))
        };
        assert_eq!(
            derive_registration_projection(&pending).action,
            DeferCanonicalProjection
        );
        let empty = RegistrationProjectionEvidence {
            buffer_is_binary_attributed_ancestor: Some(true),
            ..facts(None, Some(stale), Some(""))
        };
        assert_eq!(
            derive_registration_projection(&empty).action,
            AdoptOperatorBuffer
        );
    }

    /// `#acceptedwriterollforward`: the operator typed after the settled
    /// shadow, and the binary's accepted writes since that shadow are on
    /// record. A clean merge from the shadow keeps both; anything less keeps
    /// the IDE buffer.
    #[test]
    fn an_attributed_shadow_merges_operator_edits_with_accepted_writes() {
        let shadow = "a\nb\n";
        let buffer = "a\nb\ntyped\n";
        let canonical = "a (closed)\nb\n";
        let evidence =
            |attributed: Option<bool>, merge: Option<bool>| RegistrationProjectionEvidence {
                shadow_is_binary_attributed_ancestor: attributed,
                clean_merge_available: merge,
                ..facts(Some(shadow), Some(buffer), Some(canonical))
            };
        assert_eq!(
            derive_registration_projection(&evidence(Some(true), Some(true))).action,
            MergeForward
        );
        for (attributed, merge) in [
            (Some(true), Some(false)),
            (Some(true), None),
            (None, Some(true)),
            (Some(false), Some(true)),
        ] {
            assert_eq!(
                derive_registration_projection(&evidence(attributed, merge)).action,
                AdoptOperatorBuffer,
                "{attributed:?} {merge:?}"
            );
        }
        // Text containment is still not the op record.
        let contained = RegistrationProjectionEvidence {
            canonical_contains_operator_edits: Some(true),
            clean_merge_available: Some(true),
            ..facts(Some(shadow), Some(buffer), Some(canonical))
        };
        assert_eq!(
            derive_registration_projection(&contained).action,
            AdoptOperatorBuffer
        );
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
    fn a_reload_reregister_over_detached_typing_publishes_the_ide_buffer() {
        let publish = facts(Some("s"), Some("b"), Some("s"));
        assert_eq!(
            derive_registration_projection(&publish).action,
            AdoptOperatorBuffer
        );
        let hold = derive_registration_projection(&facts(Some("s"), Some("b"), Some("c")));
        assert_eq!(hold.action, AdoptOperatorBuffer);
        assert_eq!(hold.hold_reason, None);
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
