//! Queue auto-clear hand-off probe (`#bgclearpolicy`).
//!
//! With queue auto-clear on (the default for an active `agent:queue`), the
//! in-session drain hands each item boundary to the document's supervisor so
//! the supervisor can reset the agent context before dispatching the next head.
//! This module answers "does this drain yield at this boundary?" for the three
//! in-session continuation surfaces — preflight, `session-check`, and the Codex
//! Stop hook — so they cannot disagree.
//!
//! A hand-off is only valid when the receiver can take it: the supervisor must
//! be ready (`supervisor_drain_readiness_for_doc`) and must not owe an earlier
//! hand-off (`#supdrainyieldfalsifiable`). Otherwise the in-session drain keeps
//! going in the same context — never a stall.

use std::path::Path;

use agent_doc_controller::status::SupervisorDrainReadiness;
use agent_doc_queue::queue_auto_clear;

/// One evaluation of the auto-clear hand-off for a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoClearYield {
    /// The queue marker does not carry `disable-auto-clear`.
    pub enabled: bool,
    /// Supervisor readiness; `None` when it was not probed (auto-clear off or
    /// nothing left to continue).
    pub readiness: Option<SupervisorDrainReadiness>,
    /// An earlier supervisor hand-off is still undrained.
    pub handoff_overdue: bool,
    /// The in-session drain ends its turn and lets the supervisor clear and
    /// dispatch the next head.
    pub yields: bool,
}

impl AutoClearYield {
    pub fn readiness_reason(&self) -> &'static str {
        self.readiness
            .map(SupervisorDrainReadiness::reason)
            .unwrap_or("not_probed")
    }
}

/// Evaluate the hand-off for `file` whose current text is `content`.
/// `continuation_required` is the in-session continuation verdict the caller
/// would otherwise act on.
pub fn in_session_auto_clear_yield(
    file: &Path,
    content: &str,
    continuation_required: bool,
) -> AutoClearYield {
    in_session_auto_clear_yield_with(
        content,
        continuation_required,
        || agent_doc_controller_io::project_controller::supervisor_drain_readiness_for_doc(file),
        || {
            agent_doc_controller_io::project_controller::undrained_supervisor_drain_handoff_age(
                file, content,
            )
            .is_some()
        },
    )
}

/// [`in_session_auto_clear_yield`] with the supervisor probes injected.
pub fn in_session_auto_clear_yield_with(
    content: &str,
    continuation_required: bool,
    readiness: impl FnOnce() -> SupervisorDrainReadiness,
    handoff_overdue: impl FnOnce() -> bool,
) -> AutoClearYield {
    let enabled = queue_auto_clear::queue_auto_clear_enabled(content);
    if !(enabled && continuation_required) {
        return AutoClearYield {
            enabled,
            readiness: None,
            handoff_overdue: false,
            yields: false,
        };
    }
    let readiness = readiness();
    let handoff_overdue = readiness.is_ready() && handoff_overdue();
    AutoClearYield {
        enabled,
        readiness: Some(readiness),
        handoff_overdue,
        yields: queue_auto_clear::in_session_drain_yields_to_auto_clear(
            enabled,
            continuation_required,
            readiness.is_ready(),
            handoff_overdue,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUEUE: &str = "<!-- agent:queue go -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n";
    const QUEUE_NO_CLEAR: &str =
        "<!-- agent:queue go disable-auto-clear -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n";
    const READY: SupervisorDrainReadiness = SupervisorDrainReadiness::Ready { supervisor_pid: 7 };

    #[test]
    fn yields_to_a_ready_supervisor_by_default() {
        let decision = in_session_auto_clear_yield_with(QUEUE, true, || READY, || false);
        assert!(decision.enabled);
        assert!(decision.yields);
        assert_eq!(decision.readiness_reason(), "ready");
    }

    #[test]
    fn disable_auto_clear_keeps_the_in_session_drain_without_probing() {
        let decision = in_session_auto_clear_yield_with(
            QUEUE_NO_CLEAR,
            true,
            || panic!("readiness must not be probed when auto-clear is off"),
            || panic!("hand-off must not be probed when auto-clear is off"),
        );
        assert!(!decision.enabled);
        assert!(!decision.yields);
    }

    #[test]
    fn no_live_supervisor_or_an_overdue_handoff_keeps_the_in_session_drain() {
        let missing = in_session_auto_clear_yield_with(
            QUEUE,
            true,
            || SupervisorDrainReadiness::NoLiveSupervisor,
            || panic!("an unready supervisor has no hand-off to check"),
        );
        assert!(!missing.yields);
        assert_eq!(missing.readiness_reason(), "no_live_supervisor");

        let overdue = in_session_auto_clear_yield_with(QUEUE, true, || READY, || true);
        assert!(overdue.handoff_overdue);
        assert!(!overdue.yields);
    }

    #[test]
    fn nothing_to_continue_never_yields() {
        let decision = in_session_auto_clear_yield_with(
            QUEUE,
            false,
            || panic!("no continuation, no probe"),
            || panic!("no continuation, no probe"),
        );
        assert!(!decision.yields);
    }
}
