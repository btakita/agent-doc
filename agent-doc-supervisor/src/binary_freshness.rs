//! Process-scoped supervisor binary freshness projection.
//!
//! The filesystem observations are effects performed by the runtime. This module
//! keeps the observation as a Lazily [`Source`] and derives the recycle decision as
//! a [`Computed`], so every consumer reads one process-scoped fact instead of
//! maintaining another independently-updated stale flag.

use agent_doc_state_scope::ProcessScope;
use lazily::{Computed, Source, ThreadSafeContext};

/// One observation of the running supervisor and currently installed binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BinaryFreshnessObservation {
    /// The latest runtime identity probe reported a stale binary.
    pub identity_stale: bool,
    /// Inode of `/proc/<supervisor-pid>/exe`, when observable.
    pub running_exe_inode: Option<u64>,
    /// Inode of the currently installed `agent-doc` binary, when observable.
    pub installed_binary_inode: Option<u64>,
}

impl BinaryFreshnessObservation {
    fn is_stale(self) -> bool {
        self.identity_stale
            || matches!(
                (self.running_exe_inode, self.installed_binary_inode),
                (Some(running), Some(installed)) if running != installed
            )
    }
}

/// Lazily-backed process state for the supervisor's binary freshness.
pub struct BinaryFreshnessState {
    ctx: ThreadSafeContext,
    observation: Source<BinaryFreshnessObservation>,
    stale: Computed<bool>,
}

impl BinaryFreshnessState {
    /// Join the supervisor process graph.
    pub fn new_in(scope: &ProcessScope) -> Self {
        let ctx = scope.ctx().clone();
        let observation = ctx.source(BinaryFreshnessObservation::default());
        let stale = ctx.computed(move |ctx| ctx.get(&observation).is_stale());
        Self {
            ctx,
            observation,
            stale,
        }
    }

    /// Publish the latest effectful filesystem observation.
    pub fn observe(&self, observation: BinaryFreshnessObservation) {
        self.ctx.set(&self.observation, observation);
    }

    /// Return the derived recycle decision.
    pub fn stale(&self) -> bool {
        self.ctx.get(&self.stale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_projection_tracks_identity_and_inode_observations() {
        let scope = ProcessScope::new();
        let state = BinaryFreshnessState::new_in(&scope);
        assert!(!state.stale());

        state.observe(BinaryFreshnessObservation {
            running_exe_inode: Some(11),
            installed_binary_inode: Some(11),
            ..Default::default()
        });
        assert!(!state.stale());

        state.observe(BinaryFreshnessObservation {
            running_exe_inode: Some(11),
            installed_binary_inode: Some(12),
            ..Default::default()
        });
        assert!(state.stale());

        state.observe(BinaryFreshnessObservation {
            identity_stale: true,
            ..Default::default()
        });
        assert!(state.stale());

        state.observe(BinaryFreshnessObservation::default());
        assert!(!state.stale());
    }
}
