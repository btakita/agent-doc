//! Route startup readiness polling.

use agent_doc_controller::dispatch::{FreshStartAckOutcome, fresh_start_ack_outcome};
use agent_doc_harness::HarnessConfig;
use std::time::{Duration, Instant};
use tmux_router::Tmux;

/// Poll cadence for route agent readiness.
pub const AGENT_READY_POLL_INTERVAL: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentReadyWaitOutcome {
    Ready,
    Blocked { reason: String },
    TimedOut,
}

impl AgentReadyWaitOutcome {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    pub fn blocker_reason(&self) -> Option<&str> {
        match self {
            Self::Blocked { reason } => Some(reason.as_str()),
            _ => None,
        }
    }
}

/// Poll a tmux pane until the agent is ready to accept input.
pub fn wait_for_agent_ready(
    tmux: &Tmux,
    pane_id: &str,
    timeout: Duration,
    harness: &HarnessConfig,
) -> bool {
    wait_for_agent_ready_outcome(tmux, pane_id, timeout, harness).is_ready()
}

pub fn wait_for_agent_ready_outcome(
    tmux: &Tmux,
    pane_id: &str,
    timeout: Duration,
    harness: &HarnessConfig,
) -> AgentReadyWaitOutcome {
    let start = Instant::now();
    let poll_interval = AGENT_READY_POLL_INTERVAL;
    let mut poll_count = 0u32;
    let mut ready_streak = 0u32;
    let mut last_ready_line: Option<String> = None;
    let mut blocker_streak = 0u32;
    let mut last_blocker: Option<String> = None;

    while start.elapsed() < timeout {
        if !tmux.pane_alive(pane_id) {
            eprintln!(
                "[route] {} pane {} is dead - fast-failing ready wait after {:.1}s (recovery will reroute)",
                harness.binary,
                pane_id,
                start.elapsed().as_secs_f64()
            );
            return AgentReadyWaitOutcome::TimedOut;
        }
        if let Ok(content) = agent_doc_tmux_io::capture_pane(tmux, pane_id) {
            if let Some(reason) = harness.dispatch_blocker_reason(&content) {
                ready_streak = 0;
                last_ready_line = None;
                if last_blocker.as_deref() == Some(reason.as_str()) {
                    blocker_streak += 1;
                } else {
                    blocker_streak = 1;
                    last_blocker = Some(reason.clone());
                    if reason == "active permission prompt" {
                        agent_doc_tmux_io::input_diag::log_prompt_detection(
                            agent_doc_tmux_io::input_diag::InputDiagSink::new(
                                None,
                                agent_doc_ops_log_io::log_op,
                            ),
                            "route.wait_for_agent_ready",
                            &format!("pane:{pane_id}"),
                            &harness.binary,
                            &reason,
                            "entered",
                        );
                    }
                }
                if blocker_streak >= 2 {
                    eprintln!(
                        "[route] {} blocked after {:.1}s in pane {}: {}",
                        harness.binary,
                        start.elapsed().as_secs_f64(),
                        pane_id,
                        reason
                    );
                    return AgentReadyWaitOutcome::Blocked { reason };
                }
            } else {
                blocker_streak = 0;
                last_blocker = None;
            }

            match agent_doc_harness::ready_prompt_candidate(&content, harness) {
                Some(line) => {
                    if last_ready_line.as_deref() == Some(line.as_str()) {
                        ready_streak += 1;
                    } else {
                        ready_streak = 1;
                        last_ready_line = Some(line);
                    }
                    if ready_streak >= 2 {
                        eprintln!(
                            "[route] {} ready after {:.1}s ({} polls)",
                            harness.binary,
                            start.elapsed().as_secs_f64(),
                            poll_count
                        );
                        return AgentReadyWaitOutcome::Ready;
                    }
                }
                None => {
                    ready_streak = 0;
                    last_ready_line = None;
                }
            }

            poll_count += 1;
            if poll_count.is_multiple_of(10) {
                let last_line = content
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
                    .unwrap_or_default();
                eprintln!(
                    "[route] Still waiting for {} ({:.0}s)... last line: {}",
                    harness.binary,
                    start.elapsed().as_secs_f64(),
                    agent_doc_turn_executor_tmux::prompt::truncate_log_line(&last_line, 60)
                );
            }
        }
        std::thread::sleep(poll_interval);
    }
    AgentReadyWaitOutcome::TimedOut
}

/// Whether a no-cycle fresh start returned to a dispatch-ready prompt.
pub fn fresh_start_pane_idle_ready(tmux: &Tmux, pane: &str, harness: &HarnessConfig) -> bool {
    match agent_doc_tmux_io::capture_pane(tmux, pane) {
        Ok(content) => matches!(
            fresh_start_ack_outcome(
                false,
                agent_doc_harness::ready_prompt_candidate(&content, harness).is_some(),
            ),
            FreshStartAckOutcome::IdleNoOpKeep
        ),
        Err(_) => false,
    }
}
