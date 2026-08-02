//! Pure latest-wins ownership for the pane-layout projection worker.
//!
//! The IO layer supplies the effect worker. This state owns the race-sensitive
//! decision: a newer input revision published while the current effect is
//! finishing must keep one worker active.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LatestProjectionWorkerState {
    pending_revision: u64,
    active: bool,
}

impl LatestProjectionWorkerState {
    /// Record an exact-input revision and return whether the caller must start
    /// the single worker. An already-active worker observes the newer revision
    /// through the same retained state.
    pub fn schedule(&mut self, revision: u64) -> bool {
        self.pending_revision = self.pending_revision.max(revision);
        if self.active {
            false
        } else {
            self.active = true;
            true
        }
    }

    pub fn pending_revision(&self) -> u64 {
        self.pending_revision
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn is_superseded(&self, revision: u64) -> bool {
        self.pending_revision > revision
    }

    /// Retire only when no newer retained input revision is waiting. Holding
    /// the IO layer's mutex around this decision closes the publish-vs-exit race.
    pub fn retire_if_current(&mut self, completed_revision: u64) -> bool {
        if self.is_superseded(completed_revision) {
            return false;
        }
        self.active = false;
        true
    }

    pub fn deactivate(&mut self) {
        self.active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_input_revision_prevents_the_active_worker_from_retiring() {
        let mut state = LatestProjectionWorkerState::default();
        assert!(state.schedule(7));
        assert!(!state.schedule(8));
        assert!(state.is_superseded(7));
        assert!(!state.retire_if_current(7));
        assert!(state.is_active());
        assert!(state.retire_if_current(8));
        assert!(!state.is_active());
    }
}
