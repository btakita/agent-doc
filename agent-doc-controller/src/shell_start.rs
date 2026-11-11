//! `#shellstart`: a claiming tmux pane sitting at a shell with no `agent-doc start`.
//!
//! An external process can unlink the agent-doc socket and leave the pane at
//! the operator's shell. The crash watchdog used to require a dead lease, an
//! open session log, and an exact `resume:` id before it would start again, and
//! Run Agent Doc treated that live shell as the existing session. This module
//! decides when to run `agent-doc start` instead. A serving start, a live
//! harness, or a foreign foreground does not get a second session.

use agent_doc_supervisor::crash_policy::{
    WatchdogBindingDecision, WatchdogRestartDecision, watchdog_restart_decision,
};
use agent_doc_supervisor::intentional_exit::{
    IntentionalExitDecision, IntentionalExitMarker, watchdog_intentional_exit_decision,
};

/// What to do with the tmux pane that claims a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClaimingShellStartDecision {
    /// The pane is alive, its foreground is a bare shell, and no `agent-doc
    /// start` is serving this document. Run `agent-doc start`.
    StartAgentDoc,
    /// A start is already serving this document. Do not start another session.
    AlreadyInSession,
    /// Dead pane, harness foreground, or some other program. Leave the
    /// caller's existing path.
    NotAShell,
}

/// `StartAgentDoc` only when the claiming pane is an alive bare shell and no
/// `agent-doc start` is running. A deleted socket is `agent_doc_start_running
/// == false`: the caller observed that, this function does not look at the
/// socket.
pub const fn decide_claiming_shell_start(
    pane_alive: bool,
    current_command_is_bare_shell: bool,
    agent_doc_start_running: bool,
) -> ClaimingShellStartDecision {
    if agent_doc_start_running {
        return ClaimingShellStartDecision::AlreadyInSession;
    }
    if pane_alive && current_command_is_bare_shell {
        ClaimingShellStartDecision::StartAgentDoc
    } else {
        ClaimingShellStartDecision::NotAShell
    }
}

/// Continue-mode cold start into a pane already classified as a bare shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellColdStartSubmit {
    /// Submit `agent-doc start`. `with_resume` is false when the document has
    /// no exact conversation id; that absence must not leave the shell unstarted.
    Submit { with_resume: bool },
    /// A serving start already owns the document.
    DoNotSubmit,
    /// This pane is not the claiming shell. Keep the caller's other refusal.
    NotThisPane,
}

pub const fn shell_cold_start_submit(
    pane_alive: bool,
    current_command_is_bare_shell: bool,
    agent_doc_start_running: bool,
    resume_present: bool,
) -> ShellColdStartSubmit {
    match decide_claiming_shell_start(
        pane_alive,
        current_command_is_bare_shell,
        agent_doc_start_running,
    ) {
        ClaimingShellStartDecision::StartAgentDoc => ShellColdStartSubmit::Submit {
            with_resume: resume_present,
        },
        ClaimingShellStartDecision::AlreadyInSession => ShellColdStartSubmit::DoNotSubmit,
        ClaimingShellStartDecision::NotAShell => ShellColdStartSubmit::NotThisPane,
    }
}

/// A live associated pane is the only candidate, it is a bare shell, and no
/// start is running. Route starts `agent-doc` there instead of asking for a
/// manual claim. A second live pane stays a manual claim.
pub const fn sole_live_shell_skips_manual_claim(
    winner_alive: bool,
    winner_is_bare_shell: bool,
    redundant_candidate_alive: bool,
    agent_doc_start_running: bool,
) -> bool {
    !redundant_candidate_alive
        && matches!(
            decide_claiming_shell_start(
                winner_alive,
                winner_is_bare_shell,
                agent_doc_start_running
            ),
            ClaimingShellStartDecision::StartAgentDoc
        )
}

/// How the controller watchdog treats one open actor whose pane might be a shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClaimingShellWatchdogAction {
    /// Launch `agent-doc start` in the claiming pane. A missing lease, a closed
    /// session log, and a missing resume id do not block this.
    StartAgentDoc { attempt: usize },
    /// Restart budget is spent. Tell the operator once; do not spawn again.
    Halt { restarts_in_window: usize },
    /// Do not start, and do not fall through to the crash watchdog.
    Skip { reason: &'static str },
    /// Not a claiming shell with no start. The crash-watchdog gates still apply.
    DeferToCrashWatchdog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimingShellWatchdogFacts {
    pub queue_paused: bool,
    pub binding: WatchdogBindingDecision,
    pub shell: ClaimingShellStartDecision,
    pub exit: IntentionalExitDecision,
    pub prior_restarts_in_window: usize,
    pub restart_cap: usize,
}

pub fn claiming_shell_watchdog_action(
    facts: ClaimingShellWatchdogFacts,
) -> ClaimingShellWatchdogAction {
    if facts.shell != ClaimingShellStartDecision::StartAgentDoc {
        return ClaimingShellWatchdogAction::DeferToCrashWatchdog;
    }
    if facts.queue_paused {
        return ClaimingShellWatchdogAction::Skip {
            reason: "queue_paused",
        };
    }
    // Operator stop and an in-grace replacement cold start both mean a start
    // is already spoken for. Bypassing the grace double-kills the successor.
    if !facts.exit.allows_restart() {
        return ClaimingShellWatchdogAction::Skip {
            reason: facts.exit.as_str(),
        };
    }
    if !facts.binding.allows_restart() {
        return ClaimingShellWatchdogAction::Skip {
            reason: "binding_not_live",
        };
    }
    match watchdog_restart_decision(facts.prior_restarts_in_window, facts.restart_cap) {
        WatchdogRestartDecision::Restart { attempt } => {
            ClaimingShellWatchdogAction::StartAgentDoc { attempt }
        }
        WatchdogRestartDecision::Halt { restarts_in_window } => {
            ClaimingShellWatchdogAction::Halt { restarts_in_window }
        }
    }
}

/// Exit hold for a claiming shell whose supervisor lease pid is gone.
///
/// The crash watchdog keys the marker on the dead pid. Socket deletion can
/// remove that pid while the marker still names this pane and generation. A
/// matching operator stop or in-grace cold start still holds. Any other marker
/// does not.
pub fn shell_start_exit_hold(
    marker: Option<&IntentionalExitMarker>,
    pane_id: &str,
    generation: u64,
    now_ms: u64,
) -> IntentionalExitDecision {
    let Some(marker) = marker else {
        return IntentionalExitDecision::NoMarker;
    };
    if marker.pane_id != pane_id || marker.generation != generation {
        return IntentionalExitDecision::StaleMarker;
    }
    watchdog_intentional_exit_decision(Some(marker), marker.supervisor_pid, generation, now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_supervisor::intentional_exit::REPLACEMENT_COLD_START_SIGNAL;

    #[test]
    fn claiming_shell_without_a_start_runs_agent_doc_in_all_those_cases() {
        assert_eq!(
            decide_claiming_shell_start(true, true, false),
            ClaimingShellStartDecision::StartAgentDoc
        );
        // A deleted socket is "no start running", even when the pane command is a shell.
        assert_eq!(
            shell_cold_start_submit(true, true, false, false),
            ShellColdStartSubmit::Submit { with_resume: false }
        );
        assert_eq!(
            shell_cold_start_submit(true, true, false, true),
            ShellColdStartSubmit::Submit { with_resume: true }
        );
    }

    #[test]
    fn a_running_start_does_not_get_a_second_session() {
        assert_eq!(
            decide_claiming_shell_start(true, true, true),
            ClaimingShellStartDecision::AlreadyInSession
        );
        assert_eq!(
            decide_claiming_shell_start(true, false, true),
            ClaimingShellStartDecision::AlreadyInSession
        );
        assert_eq!(
            shell_cold_start_submit(true, true, true, false),
            ShellColdStartSubmit::DoNotSubmit
        );
    }

    #[test]
    fn dead_panes_and_foreign_foregrounds_are_not_a_claiming_shell() {
        assert_eq!(
            decide_claiming_shell_start(false, true, false),
            ClaimingShellStartDecision::NotAShell
        );
        assert_eq!(
            decide_claiming_shell_start(true, false, false),
            ClaimingShellStartDecision::NotAShell
        );
        assert_eq!(
            shell_cold_start_submit(true, false, false, false),
            ShellColdStartSubmit::NotThisPane
        );
    }

    #[test]
    fn sole_live_shell_skips_manual_claim_only_when_start_is_absent() {
        assert!(sole_live_shell_skips_manual_claim(true, true, false, false));
        assert!(!sole_live_shell_skips_manual_claim(true, true, true, false));
        assert!(!sole_live_shell_skips_manual_claim(true, true, false, true));
        assert!(!sole_live_shell_skips_manual_claim(
            true, false, false, false
        ));
        assert!(!sole_live_shell_skips_manual_claim(
            false, true, false, false
        ));
    }

    fn shell_facts(exit: IntentionalExitDecision) -> ClaimingShellWatchdogFacts {
        ClaimingShellWatchdogFacts {
            queue_paused: false,
            binding: WatchdogBindingDecision::Live,
            shell: ClaimingShellStartDecision::StartAgentDoc,
            exit,
            prior_restarts_in_window: 0,
            restart_cap: 3,
        }
    }

    #[test]
    fn watchdog_starts_a_claiming_shell_without_a_lease_or_open_session() {
        // Missing lease and a closed session log are caller facts: they are not
        // inputs here, so they cannot block the start.
        assert_eq!(
            claiming_shell_watchdog_action(shell_facts(IntentionalExitDecision::NoMarker)),
            ClaimingShellWatchdogAction::StartAgentDoc { attempt: 1 }
        );
        assert_eq!(
            claiming_shell_watchdog_action(shell_facts(
                IntentionalExitDecision::ReplacementColdStartExpired
            )),
            ClaimingShellWatchdogAction::StartAgentDoc { attempt: 1 }
        );
    }

    #[test]
    fn watchdog_holds_operator_stops_cold_starts_stash_pause_and_budget() {
        assert_eq!(
            claiming_shell_watchdog_action(shell_facts(IntentionalExitDecision::Intentional)),
            ClaimingShellWatchdogAction::Skip {
                reason: "intentional_exit"
            }
        );
        assert_eq!(
            claiming_shell_watchdog_action(shell_facts(
                IntentionalExitDecision::ReplacementColdStartPending
            )),
            ClaimingShellWatchdogAction::Skip {
                reason: "replacement_cold_start_pending"
            }
        );
        assert_eq!(
            claiming_shell_watchdog_action(ClaimingShellWatchdogFacts {
                queue_paused: true,
                ..shell_facts(IntentionalExitDecision::NoMarker)
            }),
            ClaimingShellWatchdogAction::Skip {
                reason: "queue_paused"
            }
        );
        assert_eq!(
            claiming_shell_watchdog_action(ClaimingShellWatchdogFacts {
                binding: WatchdogBindingDecision::PaneStashed,
                ..shell_facts(IntentionalExitDecision::NoMarker)
            }),
            ClaimingShellWatchdogAction::Skip {
                reason: "binding_not_live"
            }
        );
        assert_eq!(
            claiming_shell_watchdog_action(ClaimingShellWatchdogFacts {
                prior_restarts_in_window: 3,
                ..shell_facts(IntentionalExitDecision::NoMarker)
            }),
            ClaimingShellWatchdogAction::Halt {
                restarts_in_window: 3
            }
        );
    }

    #[test]
    fn watchdog_defers_when_a_session_is_already_running_or_the_pane_is_not_a_shell() {
        assert_eq!(
            claiming_shell_watchdog_action(ClaimingShellWatchdogFacts {
                shell: ClaimingShellStartDecision::AlreadyInSession,
                ..shell_facts(IntentionalExitDecision::NoMarker)
            }),
            ClaimingShellWatchdogAction::DeferToCrashWatchdog
        );
        assert_eq!(
            claiming_shell_watchdog_action(ClaimingShellWatchdogFacts {
                shell: ClaimingShellStartDecision::NotAShell,
                ..shell_facts(IntentionalExitDecision::NoMarker)
            }),
            ClaimingShellWatchdogAction::DeferToCrashWatchdog
        );
    }

    fn marker(signal: &str, pane: &str, generation: u64) -> IntentionalExitMarker {
        IntentionalExitMarker {
            supervisor_pid: 42,
            generation,
            pane_id: pane.to_string(),
            session_id: "session".to_string(),
            signal: signal.to_string(),
            recorded_at_ms: 1_000,
        }
    }

    #[test]
    fn a_missing_lease_still_honours_a_marker_for_this_pane() {
        assert_eq!(
            shell_start_exit_hold(None, "%7", 3, 1_000),
            IntentionalExitDecision::NoMarker
        );
        assert_eq!(
            shell_start_exit_hold(Some(&marker("SIGTERM", "%7", 3)), "%7", 3, 1_000),
            IntentionalExitDecision::Intentional
        );
        assert_eq!(
            shell_start_exit_hold(
                Some(&marker(REPLACEMENT_COLD_START_SIGNAL, "%7", 3)),
                "%7",
                3,
                1_000
            ),
            IntentionalExitDecision::ReplacementColdStartPending
        );
        assert_eq!(
            shell_start_exit_hold(Some(&marker("SIGTERM", "%8", 3)), "%7", 3, 1_000),
            IntentionalExitDecision::StaleMarker
        );
        assert_eq!(
            shell_start_exit_hold(Some(&marker("SIGTERM", "%7", 2)), "%7", 3, 1_000),
            IntentionalExitDecision::StaleMarker
        );
    }
}
