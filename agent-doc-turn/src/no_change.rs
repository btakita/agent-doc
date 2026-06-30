//! Pure no-change turn diagnostics.
//!
//! Orchestration owns snapshot comparison and cycle-state file loading. This
//! module owns the policy that decides whether a no-diff run should stay quiet
//! or surface an abnormal prior cycle.

use crate::CyclePhase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoChangeCycleStateInput<'a> {
    pub cycle_id: &'a str,
    pub file: &'a str,
    pub phase: CyclePhase,
    pub last_event: &'a str,
    pub has_capture: bool,
    pub has_response_hash: bool,
    pub had_pending_mutations: bool,
    pub has_pending_done_ids: bool,
    pub has_pending_kept_open_ids: bool,
    pub has_reaped_pending_ids: bool,
    pub has_pending_gated_ids: bool,
    pub pending_added_this_cycle: bool,
}

impl NoChangeCycleStateInput<'_> {
    pub const fn has_bookkeeping_without_response(self) -> bool {
        !self.has_capture
            && !self.has_response_hash
            && (self.had_pending_mutations
                || self.has_pending_done_ids
                || self.has_pending_kept_open_ids
                || self.has_reaped_pending_ids
                || self.has_pending_gated_ids
                || self.pending_added_this_cycle)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoChangeVerdict {
    Clean,
    Abnormal { summary: String, recovery: String },
}

pub fn classify_no_change_cycle_state(
    state: Option<NoChangeCycleStateInput<'_>>,
) -> NoChangeVerdict {
    let Some(state) = state else {
        return NoChangeVerdict::Clean;
    };

    if state.phase == CyclePhase::Abandoned {
        if state
            .last_event
            .starts_with("recursive_direct_invocation_blocked")
        {
            return NoChangeVerdict::Abnormal {
                summary: format!(
                    "the previous run was blocked as a recursive direct invocation and its cycle ({}) was abandoned, so no normal dispatch/response completed",
                    state.cycle_id
                ),
                recovery:
                    "if the owning pane is now idle but the document still reports busy, reconcile it without killing the pane via `agent-doc session status <FILE>` (or `agent-doc session clear <FILE>`) - idle pane evidence repairs a stale busy actor back to ready. Otherwise dispatch from the document's managed pane (editor Run Agent Doc) or restart the owner with `agent-doc start <FILE>` instead of a nested direct `agent-doc <FILE>`"
                        .to_string(),
            };
        }
        return NoChangeVerdict::Abnormal {
            summary: format!(
                "the previous cycle ({}) was abandoned (last_event={}) and never reached a committed response",
                state.cycle_id, state.last_event
            ),
            recovery:
                "re-run `agent-doc <FILE>` to start a fresh cycle, or inspect `.agent-doc/logs/` for that cycle id"
                    .to_string(),
        };
    }

    if state.phase == CyclePhase::Committed && state.has_bookkeeping_without_response() {
        return NoChangeVerdict::Abnormal {
            summary: format!(
                "the latest cycle ({}) committed without an assistant response body (bookkeeping-only closeout: last_event={}); the prior run was likely abandoned or repaired without producing a response",
                state.cycle_id, state.last_event
            ),
            recovery: format!(
                "re-run `agent-doc {}` from a non-owner pane or use `agent-doc start {}` to provision a fresh pane, then inspect `.agent-doc/logs/` for cycle {} history",
                state.file, state.file, state.cycle_id
            ),
        };
    }

    NoChangeVerdict::Clean
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(
        cycle_id: &'a str,
        file: &'a str,
        phase: CyclePhase,
        last_event: &'a str,
    ) -> NoChangeCycleStateInput<'a> {
        NoChangeCycleStateInput {
            cycle_id,
            file,
            phase,
            last_event,
            has_capture: false,
            has_response_hash: false,
            had_pending_mutations: false,
            has_pending_done_ids: false,
            has_pending_kept_open_ids: false,
            has_reaped_pending_ids: false,
            has_pending_gated_ids: false,
            pending_added_this_cycle: false,
        }
    }

    #[test]
    fn no_change_after_recursive_block_reports_typed_diagnostic() {
        let state = input(
            "cycle-1",
            "x.md",
            CyclePhase::Abandoned,
            "recursive_direct_invocation_blocked recursive direct invocation would deadlock",
        );
        match classify_no_change_cycle_state(Some(state)) {
            NoChangeVerdict::Abnormal { summary, recovery } => {
                assert!(summary.contains("recursive direct invocation"));
                assert!(summary.contains("cycle-1"));
                assert!(recovery.contains("managed pane"));
                assert!(recovery.contains("agent-doc session status"));
                assert!(recovery.contains("agent-doc session clear"));
                assert!(recovery.contains("without killing the pane"));
            }
            NoChangeVerdict::Clean => panic!("expected an abnormal no-change verdict"),
        }
    }

    #[test]
    fn no_change_after_generic_abandoned_cycle_reports_typed_diagnostic() {
        let state = input("cycle-2", "x.md", CyclePhase::Abandoned, "stale_preflight");
        assert!(matches!(
            classify_no_change_cycle_state(Some(state)),
            NoChangeVerdict::Abnormal { .. }
        ));
    }

    #[test]
    fn no_change_with_committed_cycle_stays_clean() {
        let state = input("cycle-3", "x.md", CyclePhase::Committed, "commit");
        assert_eq!(
            classify_no_change_cycle_state(Some(state)),
            NoChangeVerdict::Clean
        );
        assert_eq!(classify_no_change_cycle_state(None), NoChangeVerdict::Clean);
    }

    #[test]
    fn no_change_after_committed_bookkeeping_only_cycle_reports_abnormal() {
        let mut state = input(
            "cycle-repair-1",
            "tasks/sampleorders.md",
            CyclePhase::Committed,
            "commit_success",
        );
        state.had_pending_mutations = true;
        state.has_reaped_pending_ids = true;
        match classify_no_change_cycle_state(Some(state)) {
            NoChangeVerdict::Abnormal { summary, recovery } => {
                assert!(summary.contains("cycle-repair-1"));
                assert!(summary.contains("bookkeeping-only"));
                assert!(summary.contains("commit_success"));
                assert!(recovery.contains("tasks/sampleorders.md"));
                assert!(recovery.contains("non-owner pane"));
                assert!(recovery.contains("agent-doc start"));
            }
            NoChangeVerdict::Clean => {
                panic!("expected Abnormal for committed no-response bookkeeping cycle")
            }
        }
    }

    #[test]
    fn no_change_committed_no_response_no_bookkeeping_stays_clean() {
        let state = input("cycle-4", "x.md", CyclePhase::Committed, "commit_success");
        assert_eq!(
            classify_no_change_cycle_state(Some(state)),
            NoChangeVerdict::Clean
        );
    }

    #[test]
    fn no_change_committed_with_response_stays_clean() {
        let mut state = input("cycle-5", "x.md", CyclePhase::Committed, "commit_success");
        state.has_capture = true;
        state.has_response_hash = true;
        state.had_pending_mutations = true;
        assert_eq!(
            classify_no_change_cycle_state(Some(state)),
            NoChangeVerdict::Clean
        );
    }
}
