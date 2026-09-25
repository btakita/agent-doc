//! Typed pane execution authority (`#paneexecutionauthority`).
//!
//! Pane admission used to be re-derived in I/O crates as a string comparison
//! between the durable registry and ambient `TMUX_PANE`. That made controller
//! effects depend on the controller host pane, admitted a wrong-pane preflight
//! far enough to mutate recovery state, and could recommend a claim against a
//! genuinely live owner. This module owns the finite policy. Adapters publish
//! observations; the [`Computed`] verdict is the only decision surface.

use lazily::{Computed, Source, StateTable, ThreadSafeContext, thread_safe_projected_state_table};
use serde::{Deserialize, Serialize};

use crate::DocumentScope;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityOperation {
    ReadOnly,
    Mutation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum InvocationIdentity {
    Headless,
    ExplicitActor {
        pane_id: String,
        generation: Option<u64>,
        proves_document_owner: bool,
    },
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerLiveness {
    Live,
    Stale,
    Indeterminate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum OwnerObservation {
    Absent,
    Present {
        pane_id: String,
        generation: Option<u64>,
        liveness: OwnerLiveness,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AuthorityVerdict {
    PermitReadOnly,
    PermitHeadless,
    PermitUnregistered,
    PermitOwner {
        pane_id: String,
        generation: Option<u64>,
    },
    PermitProvenStaleBinding {
        stale_owner_pane_id: String,
        pane_id: String,
        generation: Option<u64>,
    },
    RejectGenerationMismatch {
        pane_id: String,
        invocation_generation: Option<u64>,
        owner_generation: Option<u64>,
    },
    RejectLiveOwnerMismatch {
        owner_pane_id: String,
        invocation_pane_id: String,
    },
    RejectStaleOwner {
        owner_pane_id: String,
        invocation_pane_id: String,
    },
    RejectIndeterminateOwner {
        owner_pane_id: String,
        invocation_pane_id: String,
    },
    RejectUnavailableInvocation,
}

impl AuthorityVerdict {
    pub const fn permits(&self) -> bool {
        matches!(
            self,
            Self::PermitReadOnly
                | Self::PermitHeadless
                | Self::PermitUnregistered
                | Self::PermitOwner { .. }
                | Self::PermitProvenStaleBinding { .. }
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthorityInput {
    ReadOnly,
    HeadlessMutation,
    UnregisteredMutation,
    MatchingOwnerMutation,
    ProvenStaleBindingMutation,
    GenerationMismatchMutation,
    ForeignLiveOwnerMutation,
    ForeignStaleOwnerMutation,
    ForeignIndeterminateOwnerMutation,
    UnavailableInvocationMutation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthorityDecision {
    PermitReadOnly,
    PermitHeadless,
    PermitUnregistered,
    PermitOwner,
    PermitProvenStaleBinding,
    RejectGenerationMismatch,
    RejectLiveOwnerMismatch,
    RejectStaleOwner,
    RejectIndeterminateOwner,
    RejectUnavailableInvocation,
}

struct PaneExecutionAuthorityTable;

impl StateTable for PaneExecutionAuthorityTable {
    type Input = AuthorityInput;
    type Decision = AuthorityDecision;

    fn decide(input: &Self::Input) -> Self::Decision {
        match input {
            AuthorityInput::ReadOnly => AuthorityDecision::PermitReadOnly,
            AuthorityInput::HeadlessMutation => AuthorityDecision::PermitHeadless,
            AuthorityInput::UnregisteredMutation => AuthorityDecision::PermitUnregistered,
            AuthorityInput::MatchingOwnerMutation => AuthorityDecision::PermitOwner,
            AuthorityInput::ProvenStaleBindingMutation => {
                AuthorityDecision::PermitProvenStaleBinding
            }
            AuthorityInput::GenerationMismatchMutation => {
                AuthorityDecision::RejectGenerationMismatch
            }
            AuthorityInput::ForeignLiveOwnerMutation => AuthorityDecision::RejectLiveOwnerMismatch,
            AuthorityInput::ForeignStaleOwnerMutation => AuthorityDecision::RejectStaleOwner,
            AuthorityInput::ForeignIndeterminateOwnerMutation => {
                AuthorityDecision::RejectIndeterminateOwner
            }
            AuthorityInput::UnavailableInvocationMutation => {
                AuthorityDecision::RejectUnavailableInvocation
            }
        }
    }
}

#[cfg(test)]
impl lazily::FiniteState for AuthorityInput {
    fn all() -> Vec<Self> {
        vec![
            Self::ReadOnly,
            Self::HeadlessMutation,
            Self::UnregisteredMutation,
            Self::MatchingOwnerMutation,
            Self::ProvenStaleBindingMutation,
            Self::GenerationMismatchMutation,
            Self::ForeignLiveOwnerMutation,
            Self::ForeignStaleOwnerMutation,
            Self::ForeignIndeterminateOwnerMutation,
            Self::UnavailableInvocationMutation,
        ]
    }
}

fn authority_input(
    operation: AuthorityOperation,
    invocation: &InvocationIdentity,
    owner: &OwnerObservation,
) -> AuthorityInput {
    if operation == AuthorityOperation::ReadOnly {
        return AuthorityInput::ReadOnly;
    }
    match (invocation, owner) {
        (InvocationIdentity::Headless, OwnerObservation::Absent) => {
            AuthorityInput::HeadlessMutation
        }
        (InvocationIdentity::Headless, OwnerObservation::Present { .. }) => {
            AuthorityInput::UnavailableInvocationMutation
        }
        (InvocationIdentity::Unavailable, _) => AuthorityInput::UnavailableInvocationMutation,
        (InvocationIdentity::ExplicitActor { .. }, OwnerObservation::Absent) => {
            AuthorityInput::UnregisteredMutation
        }
        (
            InvocationIdentity::ExplicitActor {
                pane_id: invocation_pane,
                generation: invocation_generation,
                proves_document_owner,
            },
            OwnerObservation::Present {
                pane_id: owner_pane,
                generation: owner_generation,
                liveness,
            },
        ) => {
            if invocation_pane == owner_pane {
                if invocation_generation == owner_generation && *proves_document_owner {
                    AuthorityInput::MatchingOwnerMutation
                } else if invocation_generation == owner_generation {
                    match liveness {
                        OwnerLiveness::Live => AuthorityInput::ForeignLiveOwnerMutation,
                        OwnerLiveness::Stale => AuthorityInput::ForeignStaleOwnerMutation,
                        OwnerLiveness::Indeterminate => {
                            AuthorityInput::ForeignIndeterminateOwnerMutation
                        }
                    }
                } else {
                    AuthorityInput::GenerationMismatchMutation
                }
            } else {
                match liveness {
                    OwnerLiveness::Live => AuthorityInput::ForeignLiveOwnerMutation,
                    OwnerLiveness::Stale if *proves_document_owner => {
                        AuthorityInput::ProvenStaleBindingMutation
                    }
                    OwnerLiveness::Stale => AuthorityInput::ForeignStaleOwnerMutation,
                    OwnerLiveness::Indeterminate => {
                        AuthorityInput::ForeignIndeterminateOwnerMutation
                    }
                }
            }
        }
    }
}

fn materialize_verdict(
    decision: AuthorityDecision,
    invocation: &InvocationIdentity,
    owner: &OwnerObservation,
) -> AuthorityVerdict {
    let invocation_parts = match invocation {
        InvocationIdentity::ExplicitActor {
            pane_id,
            generation,
            proves_document_owner,
        } => Some((pane_id.clone(), *generation, *proves_document_owner)),
        _ => None,
    };
    let owner_parts = match owner {
        OwnerObservation::Present {
            pane_id,
            generation,
            ..
        } => Some((pane_id.clone(), *generation)),
        OwnerObservation::Absent => None,
    };
    match decision {
        AuthorityDecision::PermitReadOnly => AuthorityVerdict::PermitReadOnly,
        AuthorityDecision::PermitHeadless => AuthorityVerdict::PermitHeadless,
        AuthorityDecision::PermitUnregistered => AuthorityVerdict::PermitUnregistered,
        AuthorityDecision::PermitOwner => {
            let (pane_id, generation) = owner_parts.expect("owner decision requires owner facts");
            AuthorityVerdict::PermitOwner {
                pane_id,
                generation,
            }
        }
        AuthorityDecision::RejectGenerationMismatch => {
            let (pane_id, invocation_generation, _) =
                invocation_parts.expect("generation mismatch requires invocation facts");
            let (_, owner_generation) =
                owner_parts.expect("generation mismatch requires owner facts");
            AuthorityVerdict::RejectGenerationMismatch {
                pane_id,
                invocation_generation,
                owner_generation,
            }
        }
        AuthorityDecision::RejectLiveOwnerMismatch => {
            let (invocation_pane_id, _, _) =
                invocation_parts.expect("owner mismatch requires invocation facts");
            let (owner_pane_id, _) = owner_parts.expect("owner mismatch requires owner facts");
            AuthorityVerdict::RejectLiveOwnerMismatch {
                owner_pane_id,
                invocation_pane_id,
            }
        }
        AuthorityDecision::RejectStaleOwner => {
            let (invocation_pane_id, _, _) =
                invocation_parts.expect("stale owner requires invocation facts");
            let (owner_pane_id, _) = owner_parts.expect("stale owner requires owner facts");
            AuthorityVerdict::RejectStaleOwner {
                owner_pane_id,
                invocation_pane_id,
            }
        }
        AuthorityDecision::RejectIndeterminateOwner => {
            let (invocation_pane_id, _, _) =
                invocation_parts.expect("indeterminate owner requires invocation facts");
            let (owner_pane_id, _) = owner_parts.expect("indeterminate owner requires owner facts");
            AuthorityVerdict::RejectIndeterminateOwner {
                owner_pane_id,
                invocation_pane_id,
            }
        }
        AuthorityDecision::RejectUnavailableInvocation => {
            AuthorityVerdict::RejectUnavailableInvocation
        }
        AuthorityDecision::PermitProvenStaleBinding => {
            let (pane_id, generation, proves_document_owner) =
                invocation_parts.expect("proven stale binding requires invocation facts");
            debug_assert!(proves_document_owner);
            let (stale_owner_pane_id, _) =
                owner_parts.expect("proven stale binding requires owner facts");
            AuthorityVerdict::PermitProvenStaleBinding {
                stale_owner_pane_id,
                pane_id,
                generation,
            }
        }
    }
}

pub fn decide(
    operation: AuthorityOperation,
    invocation: &InvocationIdentity,
    owner: &OwnerObservation,
) -> AuthorityVerdict {
    materialize_verdict(
        PaneExecutionAuthorityTable::decide(&authority_input(operation, invocation, owner)),
        invocation,
        owner,
    )
}

/// Document-scoped authority observations and their single derived verdict.
pub struct PaneExecutionAuthority {
    ctx: ThreadSafeContext,
    operation: Source<AuthorityOperation>,
    invocation: Source<InvocationIdentity>,
    owner: Source<OwnerObservation>,
    verdict: Computed<AuthorityVerdict>,
}

impl PaneExecutionAuthority {
    pub fn new_in(scope: &DocumentScope) -> Self {
        let ctx = scope.ctx().clone();
        let operation = ctx.source(AuthorityOperation::ReadOnly);
        let invocation = ctx.source(InvocationIdentity::Unavailable);
        let owner = ctx.source(OwnerObservation::Absent);
        let decision =
            thread_safe_projected_state_table::<PaneExecutionAuthorityTable, _>(&ctx, move |ctx| {
                authority_input(ctx.get(&operation), &ctx.get(&invocation), &ctx.get(&owner))
            });
        let verdict = ctx.computed(move |ctx| {
            materialize_verdict(ctx.get(&decision), &ctx.get(&invocation), &ctx.get(&owner))
        });
        Self {
            ctx,
            operation,
            invocation,
            owner,
            verdict,
        }
    }

    pub fn observe_operation(&self, operation: AuthorityOperation) {
        self.ctx.set(&self.operation, operation);
    }

    pub fn observe_invocation(&self, invocation: InvocationIdentity) {
        self.ctx.set(&self.invocation, invocation);
    }

    pub fn observe_owner(&self, owner: OwnerObservation) {
        self.ctx.set(&self.owner, owner);
    }

    pub fn verdict(&self) -> AuthorityVerdict {
        self.ctx.get(&self.verdict)
    }

    pub fn verdict_cell(&self) -> &Computed<AuthorityVerdict> {
        &self.verdict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn explicit(pane: &str, generation: Option<u64>) -> InvocationIdentity {
        InvocationIdentity::ExplicitActor {
            pane_id: pane.to_string(),
            generation,
            proves_document_owner: true,
        }
    }

    fn owner(pane: &str, generation: Option<u64>, liveness: OwnerLiveness) -> OwnerObservation {
        OwnerObservation::Present {
            pane_id: pane.to_string(),
            generation,
            liveness,
        }
    }

    #[test]
    fn typed_table_covers_every_constructible_authority_row() {
        let coverage = lazily::table_coverage::<PaneExecutionAuthorityTable>();
        assert_eq!(coverage.len(), 10);
        coverage.assert_decisions_exactly(&[
            AuthorityDecision::PermitReadOnly,
            AuthorityDecision::PermitHeadless,
            AuthorityDecision::PermitUnregistered,
            AuthorityDecision::PermitOwner,
            AuthorityDecision::PermitProvenStaleBinding,
            AuthorityDecision::RejectGenerationMismatch,
            AuthorityDecision::RejectLiveOwnerMismatch,
            AuthorityDecision::RejectStaleOwner,
            AuthorityDecision::RejectIndeterminateOwner,
            AuthorityDecision::RejectUnavailableInvocation,
        ]);
        coverage.assert_every_row("decided without a fall-through state", |_, _| true);
    }

    #[test]
    fn live_owner_mismatch_is_never_reclassified_as_repairable() {
        assert!(matches!(
            decide(
                AuthorityOperation::Mutation,
                &explicit("%other", None),
                &owner("%owner", Some(7), OwnerLiveness::Live),
            ),
            AuthorityVerdict::RejectLiveOwnerMismatch { .. }
        ));
    }

    #[test]
    fn headless_mutation_is_permitted_only_without_a_registered_owner() {
        assert_eq!(
            decide(
                AuthorityOperation::Mutation,
                &InvocationIdentity::Headless,
                &OwnerObservation::Absent,
            ),
            AuthorityVerdict::PermitHeadless,
        );
        assert_eq!(
            decide(
                AuthorityOperation::Mutation,
                &InvocationIdentity::Headless,
                &owner("%owner", Some(1), OwnerLiveness::Live),
            ),
            AuthorityVerdict::RejectUnavailableInvocation,
        );
    }

    #[test]
    fn pane_reuse_cannot_cross_an_actor_generation() {
        assert!(matches!(
            decide(
                AuthorityOperation::Mutation,
                &explicit("%owner", Some(6)),
                &owner("%owner", Some(7), OwnerLiveness::Live),
            ),
            AuthorityVerdict::RejectGenerationMismatch { .. }
        ));
    }

    #[test]
    fn legacy_same_pane_string_without_exact_process_proof_is_not_authority() {
        assert!(matches!(
            decide(
                AuthorityOperation::Mutation,
                &InvocationIdentity::ExplicitActor {
                    pane_id: "%legacy".to_string(),
                    generation: None,
                    proves_document_owner: false,
                },
                &owner("%legacy", None, OwnerLiveness::Indeterminate),
            ),
            AuthorityVerdict::RejectIndeterminateOwner { .. }
        ));
    }

    #[test]
    fn exact_process_owner_supersedes_only_a_proven_stale_binding() {
        assert!(matches!(
            decide(
                AuthorityOperation::Mutation,
                &explicit("%current", None),
                &owner("%stale", Some(7), OwnerLiveness::Stale),
            ),
            AuthorityVerdict::PermitProvenStaleBinding { .. }
        ));
        assert!(matches!(
            decide(
                AuthorityOperation::Mutation,
                &explicit("%current", None),
                &owner("%live", Some(7), OwnerLiveness::Live),
            ),
            AuthorityVerdict::RejectLiveOwnerMismatch { .. }
        ));
    }

    #[test]
    fn reactive_verdict_invalidates_when_owner_changes() {
        let scope = DocumentScope::new();
        let authority = PaneExecutionAuthority::new_in(&scope);
        authority.observe_operation(AuthorityOperation::Mutation);
        authority.observe_invocation(explicit("%1", Some(3)));
        authority.observe_owner(owner("%1", Some(3), OwnerLiveness::Live));
        assert!(authority.verdict().permits());

        authority.observe_owner(owner("%2", Some(4), OwnerLiveness::Live));
        assert!(matches!(
            authority.verdict(),
            AuthorityVerdict::RejectLiveOwnerMismatch { .. }
        ));
    }

    #[test]
    fn read_only_probe_never_acquires_mutation_authority() {
        assert_eq!(
            decide(
                AuthorityOperation::ReadOnly,
                &InvocationIdentity::Unavailable,
                &owner("%owner", Some(1), OwnerLiveness::Indeterminate),
            ),
            AuthorityVerdict::PermitReadOnly,
        );
    }
}
