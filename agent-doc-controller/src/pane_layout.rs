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

/// True when a pane's *recorded* window binding no longer matches the window the
/// pane actually lives in right now.
///
/// The actor record and the durable registry each carry a `window` captured when
/// the pane was bound, and nothing re-derives it. When a pane is later moved into
/// the `stash` window, both records keep naming the visible `agent-doc` window, so
/// every record-vs-record comparison still agrees and the drift is invisible — the
/// pane-layout projection then counts a stashed pane as a co-visible column and
/// mirrors editor focus onto a pane the operator cannot see.
///
/// The live window is the only authority for where a pane *is*; a recorded window
/// is only evidence of where it *was*.
///
/// Strict tightening, mirroring the cross-repo owner guard: an unknown or empty
/// value on either side is never drift. A pane we cannot locate must not be
/// reported as misplaced, or a transient tmux read turns into a false repair.
pub fn pane_window_binding_drifted(recorded_window: &str, live_window: Option<&str>) -> bool {
    let recorded = recorded_window.trim();
    if recorded.is_empty() {
        return false;
    }
    let Some(live) = live_window.map(str::trim).filter(|live| !live.is_empty()) else {
        return false;
    };
    recorded != live
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

    /// The operator-reported shape: the record says the visible `agent-doc`
    /// window, tmux has the pane in `stash`. Every record-vs-record check agrees,
    /// so this comparison is the only one that can see it.
    #[test]
    fn stashed_pane_is_drift_even_when_records_agree() {
        assert!(pane_window_binding_drifted("@894", Some("@904")));
    }

    #[test]
    fn matching_window_is_not_drift() {
        assert!(!pane_window_binding_drifted("@894", Some("@894")));
        assert!(!pane_window_binding_drifted(" @894 ", Some("@894")));
    }

    /// Strict tightening: a pane we cannot locate is never reported as misplaced,
    /// so a transient tmux read cannot trigger a false repair.
    #[test]
    fn unknown_or_empty_on_either_side_is_never_drift() {
        assert!(!pane_window_binding_drifted("@894", None));
        assert!(!pane_window_binding_drifted("@894", Some("")));
        assert!(!pane_window_binding_drifted("@894", Some("   ")));
        assert!(!pane_window_binding_drifted("", Some("@904")));
        assert!(!pane_window_binding_drifted("   ", Some("@904")));
        assert!(!pane_window_binding_drifted("", None));
    }
}
