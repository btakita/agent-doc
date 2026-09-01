//! Process-scoped ownership of one authoritative actor generation.
//!
//! Controller compare-and-swap rejection is an ownership transition, not a
//! transient write failure. Once a newer generation exists, the old supervisor
//! must retire all lifecycle effects instead of retrying on every poll.

use agent_doc_state_scope::ProcessScope;
use lazily::{Computed, Source, ThreadSafeContext};

const GENERATION_CAS_RETIRED_MARKER: &str = "generation compare-and-swap failed";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorGenerationFailureTransition {
    Retired,
    AlreadyRetired,
    Retryable,
}

pub struct ActorGenerationLease {
    ctx: ThreadSafeContext,
    retired: Source<bool>,
    active: Computed<bool>,
}

impl ActorGenerationLease {
    pub fn new_in(scope: &ProcessScope) -> Self {
        let ctx = scope.ctx().clone();
        let retired = ctx.source(false);
        let retired_for_active = retired;
        let active = ctx.computed(move |ctx| !ctx.get(&retired_for_active));
        Self {
            ctx,
            retired,
            active,
        }
    }

    pub fn active(&self) -> bool {
        self.ctx.get(&self.active)
    }

    /// Observe one failed controller transition and retire exactly once when a
    /// newer authoritative generation rejected this process's lease.
    pub fn observe_failure(&self, detail: &str) -> ActorGenerationFailureTransition {
        if !detail.contains(GENERATION_CAS_RETIRED_MARKER) {
            return ActorGenerationFailureTransition::Retryable;
        }
        if !self.active() {
            return ActorGenerationFailureTransition::AlreadyRetired;
        }
        self.ctx.set(&self.retired, true);
        ActorGenerationFailureTransition::Retired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_cas_conflict_retires_the_lazily_lease_once() {
        let scope = ProcessScope::new();
        let lease = ActorGenerationLease::new_in(&scope);
        assert!(lease.active());
        assert_eq!(
            lease.observe_failure(
                "controller actor generation compare-and-swap failed: expected 1650, found 1652"
            ),
            ActorGenerationFailureTransition::Retired
        );
        assert!(!lease.active());
        assert_eq!(
            lease.observe_failure(
                "controller actor generation compare-and-swap failed: expected 1650, found 1652"
            ),
            ActorGenerationFailureTransition::AlreadyRetired
        );
    }

    #[test]
    fn transient_controller_failure_keeps_the_generation_active() {
        let scope = ProcessScope::new();
        let lease = ActorGenerationLease::new_in(&scope);
        assert_eq!(
            lease.observe_failure("database is locked"),
            ActorGenerationFailureTransition::Retryable
        );
        assert!(lease.active());
    }
}
