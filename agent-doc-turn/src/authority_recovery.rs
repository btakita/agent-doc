//! Pure read-authority recovery policy.
//!
//! I/O adapters classify their concrete relay result into this vocabulary, ask
//! this module for the next transition, and then apply the selected effect. Disk
//! reads, plugin refreshes, sleeps, and logging remain at the adapter boundary.

/// The authority observation shared by realtime and preflight read adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityObservation {
    Current,
    Detached,
    MissingReplica,
    SyncPending,
    Error,
}

/// Facts needed to select the next read-authority recovery transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorityRecoveryFacts {
    pub observation: AuthorityObservation,
    pub editor_open: bool,
    pub retries_remaining: bool,
    /// Some adapters own a distinct model-rebuild effect after their bounded
    /// retry loop. Others already performed that work and must not repeat it.
    pub rebuild_after_retry_exhaustion: bool,
    /// The live editor endpoint ANSWERED and refused to serve this document.
    ///
    /// This is proof, not a guess, and it is the fact `editor_open` cannot
    /// supply: attachment is a latch that outlives the endpoint's own refusal.
    /// Without it the resolver had a reachable state with no outgoing
    /// transition — an attached latch plus a missing replica plus an endpoint
    /// that will never rebuild it — and only an operator reopening the editor
    /// tab could clear it. `formal/tla/EditorReplicaStrand.tla` proves that state
    /// is a genuine state-graph deadlock, and that this fact is what removes it.
    ///
    /// Must never be set for a timeout, a refused connection, or a missing
    /// socket: those are retryable, and treating them as refusals would turn the
    /// disk-descent guard into a silent `--force-disk`.
    pub endpoint_definitively_refused: bool,
}

/// The effect an I/O adapter must apply next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityRecoveryDecision {
    AcceptCurrent,
    Retry { request_plugin_refresh: bool },
    RebuildFromPlugin,
    DescendToDisk,
    FailClosed,
}

/// Why the IPC build-mismatch recovery definitively refused another editor
/// native-library reload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMismatchRefusal {
    SenderExecutableReplaced,
    ReloadAlreadyRequested,
}

/// Facts used to choose terminal guidance after attached-editor authority
/// recovery is exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachedEditorRecoveryFacts {
    pub build_mismatch_refusal: Option<BuildMismatchRefusal>,
    pub plugin_bytes_superseded: bool,
    pub endpoints_found: bool,
    pub all_endpoints_unreachable: bool,
}

/// The only terminal actions an attached-editor refusal may prescribe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachedEditorRecoveryDecision {
    /// The command sender is the stale side. Replacing editor bytes cannot
    /// resolve the mismatch; wait for the route-owned supervisor replacement.
    WaitForSenderRecycle,
    /// A reload already landed against this listener build and the mismatch
    /// survived it. Wait for a new endpoint generation instead of repeating it.
    WaitForEndpointGeneration,
    RestartEditor,
    ReregisterEditor,
    ReloadEditor,
}

/// Decide terminal recovery without consulting presentation strings.
pub const fn decide_attached_editor_recovery(
    facts: AttachedEditorRecoveryFacts,
) -> AttachedEditorRecoveryDecision {
    if let Some(refusal) = facts.build_mismatch_refusal {
        return match refusal {
            BuildMismatchRefusal::SenderExecutableReplaced => {
                AttachedEditorRecoveryDecision::WaitForSenderRecycle
            }
            BuildMismatchRefusal::ReloadAlreadyRequested => {
                AttachedEditorRecoveryDecision::WaitForEndpointGeneration
            }
        };
    }
    if facts.plugin_bytes_superseded || facts.all_endpoints_unreachable {
        AttachedEditorRecoveryDecision::RestartEditor
    } else if !facts.endpoints_found {
        AttachedEditorRecoveryDecision::ReregisterEditor
    } else {
        AttachedEditorRecoveryDecision::ReloadEditor
    }
}

/// Decide the next read-authority transition.
///
/// The load-bearing invariant is that disk is reachable only after the editor is
/// proven not to be serving this document. An attached editor with an unavailable
/// model retries, rebuilds, or fails closed; it never silently adopts disk as
/// current text.
///
/// "Proven not serving" has two witnesses, and for months there was only one.
/// `Detached` is the ordinary witness. The second is
/// `endpoint_definitively_refused`: a live endpoint that answers and rejects has
/// told us it will not serve this document, which the `editor_open` latch cannot
/// express. With only the first witness, an attachment latch left over from a
/// cdylib generation swap made this function total in appearance only — its
/// `FailClosed` arm was a dead end no in-binary action could leave, so every
/// recovery attempt was a retry of an already-exhausted path and the operator had
/// to reopen the editor tab. `formal/tla/EditorReplicaStrand.tla` checks that
/// dead end as a state-graph deadlock, and its wedge configuration is required to
/// keep failing so this arm cannot quietly become unreachable-by-modelling.
///
/// A refusal is only consulted after the retry budget is spent, so a plugin that
/// rejects while mid-reload still gets every retry it would have had.
pub const fn decide_authority_recovery(facts: AuthorityRecoveryFacts) -> AuthorityRecoveryDecision {
    use AuthorityObservation::{Current, Detached, Error, MissingReplica, SyncPending};
    use AuthorityRecoveryDecision::{
        AcceptCurrent, DescendToDisk, FailClosed, RebuildFromPlugin, Retry,
    };

    match facts.observation {
        Current => AcceptCurrent,
        Detached => DescendToDisk,
        MissingReplica | SyncPending | Error if facts.retries_remaining => Retry {
            request_plugin_refresh: matches!(facts.observation, MissingReplica),
        },
        // Retries are spent and the endpoint itself said no. Rebuilding from the
        // plugin would ask the same endpoint that just refused, and failing
        // closed would park in the dead end, so descend on the proof.
        MissingReplica | SyncPending | Error if facts.endpoint_definitively_refused => {
            DescendToDisk
        }
        MissingReplica | SyncPending
            if facts.editor_open && facts.rebuild_after_retry_exhaustion =>
        {
            RebuildFromPlugin
        }
        MissingReplica | SyncPending | Error if facts.editor_open => FailClosed,
        MissingReplica | SyncPending | Error => DescendToDisk,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitive_build_mismatch_refusals_override_reload_shaped_fallbacks() {
        let base = AttachedEditorRecoveryFacts {
            build_mismatch_refusal: None,
            plugin_bytes_superseded: false,
            endpoints_found: true,
            all_endpoints_unreachable: false,
        };

        assert_eq!(
            decide_attached_editor_recovery(AttachedEditorRecoveryFacts {
                build_mismatch_refusal: Some(BuildMismatchRefusal::SenderExecutableReplaced),
                ..base
            }),
            AttachedEditorRecoveryDecision::WaitForSenderRecycle,
        );
        assert_eq!(
            decide_attached_editor_recovery(AttachedEditorRecoveryFacts {
                build_mismatch_refusal: Some(BuildMismatchRefusal::ReloadAlreadyRequested),
                ..base
            }),
            AttachedEditorRecoveryDecision::WaitForEndpointGeneration,
        );
    }

    #[test]
    fn attached_editor_terminal_recovery_table_is_exhaustive() {
        use AttachedEditorRecoveryDecision::{
            ReloadEditor, ReregisterEditor, RestartEditor, WaitForEndpointGeneration,
            WaitForSenderRecycle,
        };

        let cases = [
            (
                Some(BuildMismatchRefusal::SenderExecutableReplaced),
                false,
                true,
                false,
                WaitForSenderRecycle,
            ),
            (
                Some(BuildMismatchRefusal::ReloadAlreadyRequested),
                false,
                true,
                false,
                WaitForEndpointGeneration,
            ),
            (None, true, true, false, RestartEditor),
            (None, false, true, true, RestartEditor),
            (None, false, false, false, ReregisterEditor),
            (None, false, true, false, ReloadEditor),
        ];
        for (build_mismatch_refusal, superseded, endpoints_found, unreachable, expected) in cases {
            assert_eq!(
                decide_attached_editor_recovery(AttachedEditorRecoveryFacts {
                    build_mismatch_refusal,
                    plugin_bytes_superseded: superseded,
                    endpoints_found,
                    all_endpoints_unreachable: unreachable,
                }),
                expected,
            );
        }
    }

    fn decide(
        observation: AuthorityObservation,
        editor_open: bool,
        retries_remaining: bool,
        rebuild_after_retry_exhaustion: bool,
    ) -> AuthorityRecoveryDecision {
        decide_authority_recovery(AuthorityRecoveryFacts {
            observation,
            editor_open,
            retries_remaining,
            rebuild_after_retry_exhaustion,
            endpoint_definitively_refused: false,
        })
    }

    const OBSERVATIONS: [AuthorityObservation; 5] = [
        AuthorityObservation::Current,
        AuthorityObservation::Detached,
        AuthorityObservation::MissingReplica,
        AuthorityObservation::SyncPending,
        AuthorityObservation::Error,
    ];

    fn all_facts() -> impl Iterator<Item = AuthorityRecoveryFacts> {
        OBSERVATIONS.into_iter().flat_map(|observation| {
            [false, true].into_iter().flat_map(move |editor_open| {
                [false, true]
                    .into_iter()
                    .flat_map(move |retries_remaining| {
                        [false, true].into_iter().flat_map(move |rebuild| {
                            [false, true]
                                .into_iter()
                                .map(move |refused| AuthorityRecoveryFacts {
                                    observation,
                                    editor_open,
                                    retries_remaining,
                                    rebuild_after_retry_exhaustion: rebuild,
                                    endpoint_definitively_refused: refused,
                                })
                        })
                    })
            })
        })
    }

    /// The totality property `formal/tla/EditorReplicaStrand.tla` checks, asserted
    /// here over the WHOLE fact space this policy can be asked about (5 x 2^4 = 80
    /// combinations, exhaustive — not a sample).
    ///
    /// `FailClosed` is the only decision an attached document cannot leave on its
    /// own, so it may be selected only while the endpoint has NOT refused. If a
    /// refusal can still reach `FailClosed`, the dead end is back: the attachment
    /// latch is held by an endpoint that will never rebuild the replica, and no
    /// in-binary action makes progress.
    #[test]
    fn a_definitive_refusal_never_selects_the_one_decision_that_cannot_make_progress() {
        for facts in all_facts() {
            if !facts.endpoint_definitively_refused {
                continue;
            }
            let decision = decide_authority_recovery(facts);
            assert_ne!(
                decision,
                AuthorityRecoveryDecision::FailClosed,
                "a proven refusal must never park in the dead end: {facts:?}"
            );
            assert_ne!(
                decision,
                AuthorityRecoveryDecision::RebuildFromPlugin,
                "rebuilding asks the endpoint that just refused: {facts:?}"
            );
        }
    }

    /// The safety half. A refusal is the ONLY thing that may unlock disk for an
    /// attached document; without one, an attached document must never descend.
    /// This is what keeps the fix from becoming a disguised `--force-disk`.
    #[test]
    fn an_attached_document_descends_only_on_a_proven_refusal() {
        for facts in all_facts() {
            if decide_authority_recovery(facts) != AuthorityRecoveryDecision::DescendToDisk {
                continue;
            }
            assert!(
                !facts.editor_open
                    || facts.endpoint_definitively_refused
                    || facts.observation == AuthorityObservation::Detached,
                "disk descended for an attached document with no proof it stopped serving: {facts:?}"
            );
        }
    }

    /// A refusal must not short-circuit the retry budget: a plugin that rejects
    /// while mid-reload still gets every retry it would have had.
    #[test]
    fn a_refusal_is_consulted_only_after_the_retry_budget_is_spent() {
        for observation in [
            AuthorityObservation::MissingReplica,
            AuthorityObservation::SyncPending,
            AuthorityObservation::Error,
        ] {
            assert_eq!(
                decide_authority_recovery(AuthorityRecoveryFacts {
                    observation,
                    editor_open: true,
                    retries_remaining: true,
                    rebuild_after_retry_exhaustion: false,
                    endpoint_definitively_refused: true,
                }),
                AuthorityRecoveryDecision::Retry {
                    request_plugin_refresh: matches!(
                        observation,
                        AuthorityObservation::MissingReplica
                    ),
                },
                "retries outrank a refusal while any remain"
            );
        }
    }

    /// The exact production shape: attached latch, replica gone, endpoint answers
    /// and rejects, retries spent, rebuild already performed. Before the fix this
    /// was `FailClosed` forever.
    #[test]
    fn the_stranded_replica_shape_descends_instead_of_wedging() {
        assert_eq!(
            decide_authority_recovery(AuthorityRecoveryFacts {
                observation: AuthorityObservation::MissingReplica,
                editor_open: true,
                retries_remaining: false,
                rebuild_after_retry_exhaustion: true,
                endpoint_definitively_refused: true,
            }),
            AuthorityRecoveryDecision::DescendToDisk
        );
        // And with no refusal proof it still fails closed, unchanged.
        assert_eq!(
            decide_authority_recovery(AuthorityRecoveryFacts {
                observation: AuthorityObservation::MissingReplica,
                editor_open: true,
                retries_remaining: false,
                rebuild_after_retry_exhaustion: false,
                endpoint_definitively_refused: false,
            }),
            AuthorityRecoveryDecision::FailClosed
        );
    }

    #[test]
    fn current_is_accepted_and_detached_descends() {
        assert_eq!(
            decide(AuthorityObservation::Current, true, true, true),
            AuthorityRecoveryDecision::AcceptCurrent
        );
        assert_eq!(
            decide(AuthorityObservation::Detached, false, true, true),
            AuthorityRecoveryDecision::DescendToDisk
        );
    }

    #[test]
    fn bounded_retries_refresh_only_a_missing_replica() {
        assert_eq!(
            decide(AuthorityObservation::MissingReplica, true, true, false),
            AuthorityRecoveryDecision::Retry {
                request_plugin_refresh: true,
            }
        );
        assert_eq!(
            decide(AuthorityObservation::SyncPending, true, true, false),
            AuthorityRecoveryDecision::Retry {
                request_plugin_refresh: false,
            }
        );
        assert_eq!(
            decide(AuthorityObservation::Error, false, true, false),
            AuthorityRecoveryDecision::Retry {
                request_plugin_refresh: false,
            }
        );
    }

    #[test]
    fn attached_transients_rebuild_or_fail_closed_after_retries() {
        for observation in [
            AuthorityObservation::MissingReplica,
            AuthorityObservation::SyncPending,
        ] {
            assert_eq!(
                decide(observation, true, false, true),
                AuthorityRecoveryDecision::RebuildFromPlugin
            );
            assert_eq!(
                decide(observation, true, false, false),
                AuthorityRecoveryDecision::FailClosed
            );
        }
    }

    #[test]
    fn unavailable_authority_descends_only_after_editor_detaches() {
        for observation in [
            AuthorityObservation::MissingReplica,
            AuthorityObservation::SyncPending,
            AuthorityObservation::Error,
        ] {
            assert_eq!(
                decide(observation, false, false, false),
                AuthorityRecoveryDecision::DescendToDisk
            );
        }
        assert_eq!(
            decide(AuthorityObservation::Error, true, false, false),
            AuthorityRecoveryDecision::FailClosed
        );
    }
}
