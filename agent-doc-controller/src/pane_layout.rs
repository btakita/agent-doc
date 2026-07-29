//! Pure latest-wins ownership for the pane-layout projection worker.
//!
//! The IO layer supplies the thread and wake primitive. This state owns the
//! race-sensitive decision: a newer desired generation published while the
//! current effect is finishing must keep one worker active.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LatestProjectionWorkerState {
    pending_generation: u64,
    active: bool,
}

impl LatestProjectionWorkerState {
    /// Record a desired generation and return whether the caller must start the
    /// single worker. An already-active worker observes the newer generation
    /// through the same retained state.
    pub fn schedule(&mut self, generation: u64) -> bool {
        self.pending_generation = self.pending_generation.max(generation);
        if self.active {
            false
        } else {
            self.active = true;
            true
        }
    }

    pub fn pending_generation(&self) -> u64 {
        self.pending_generation
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn is_superseded(&self, generation: u64) -> bool {
        self.pending_generation > generation
    }

    /// Retire only when no newer retained generation is waiting. Holding the IO
    /// layer's mutex around this decision closes the publish-vs-exit race.
    pub fn retire_if_current(&mut self, completed_generation: u64) -> bool {
        if self.is_superseded(completed_generation) {
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
    fn newer_generation_prevents_the_active_worker_from_retiring() {
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
