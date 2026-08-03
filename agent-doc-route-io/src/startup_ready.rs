//! Route startup readiness polling.

use agent_doc_controller::dispatch::{
    AutoStartDispatchBlock, AutoStartDispatchReadyFacts, FreshStartAdmissionOutcome,
    classify_auto_start_dispatch_ready_block, fresh_start_admission_outcome,
    pane_composer_has_pending_trigger,
};
use agent_doc_harness::{HarnessConfig, PaneComposerProjection, PaneComposerReadinessEvidence};
use anyhow::Result;
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

/// Evaluate prompt readiness from the visible composer region. Claude and
/// Codex may render arbitrary user-configured status text below the cursor;
/// OpenCode retains its full-pane chrome proof.
pub fn pane_ready_prompt_candidate(
    tmux: &Tmux,
    pane_id: &str,
    content: &str,
    harness: &HarnessConfig,
) -> Option<PaneComposerReadinessEvidence> {
    match agent_doc_harness::project_pane_composer_at_cursor(
        content,
        harness,
        agent_doc_tmux_io::pane_cursor_y(tmux, pane_id),
    ) {
        PaneComposerProjection::ReadyEmpty { evidence } => Some(evidence),
        PaneComposerProjection::OperatorDraft { .. }
        | PaneComposerProjection::Busy
        | PaneComposerProjection::Absent => None,
    }
}

/// Detect an operator-owned composer draft using the same cursor-scoped prompt
/// region as readiness.
pub fn pane_composer_draft(
    tmux: &Tmux,
    pane_id: &str,
    content: &str,
    harness: &HarnessConfig,
) -> Option<String> {
    match agent_doc_harness::project_pane_composer_at_cursor(
        content,
        harness,
        agent_doc_tmux_io::pane_cursor_y(tmux, pane_id),
    ) {
        PaneComposerProjection::OperatorDraft { preview } => Some(preview),
        PaneComposerProjection::ReadyEmpty { .. }
        | PaneComposerProjection::Busy
        | PaneComposerProjection::Absent => None,
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
    let mut last_ready_evidence: Option<PaneComposerReadinessEvidence> = None;
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
        if let Ok(content) = agent_doc_tmux_io::capture_pane_with_ansi(tmux, pane_id) {
            if let Some(reason) = harness.dispatch_blocker_reason(&content) {
                ready_streak = 0;
                last_ready_evidence = None;
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

            match pane_ready_prompt_candidate(tmux, pane_id, &content, harness) {
                Some(evidence) => {
                    if last_ready_evidence.as_ref() == Some(&evidence) {
                        ready_streak += 1;
                    } else {
                        ready_streak = 1;
                        last_ready_evidence = Some(evidence);
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
                    last_ready_evidence = None;
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
    matches!(
        fresh_start_missing_admission_outcome(tmux, pane, harness, ""),
        FreshStartAdmissionOutcome::IdleNoOpKeep
    )
}

/// (#jbtsiftnosub2) Classify a no-cycle fresh start from a single pane capture.
///
/// Distinguishes a legitimate idle no-op (`IdleNoOpKeep`) from a stranded
/// "typed but not submitted" trigger (`StrandedTriggerResubmit`) by checking
/// whether the injected `trigger` is still visible unsubmitted in the composer.
/// Pass an empty `trigger` to skip the pending-composer check (idle-vs-miss only).
pub fn fresh_start_missing_admission_outcome(
    tmux: &Tmux,
    pane: &str,
    harness: &HarnessConfig,
    trigger: &str,
) -> FreshStartAdmissionOutcome {
    match agent_doc_tmux_io::capture_pane(tmux, pane) {
        Ok(content) => {
            let pane_dispatch_ready =
                pane_ready_prompt_candidate(tmux, pane, &content, harness).is_some();
            let trigger_pending = pane_composer_has_pending_trigger(&content, trigger);
            fresh_start_admission_outcome(false, pane_dispatch_ready, trigger_pending)
        }
        Err(_) => fresh_start_admission_outcome(false, false, false),
    }
}

/// `#jbtsiftnosub`: re-verify, immediately before an auto-start send, that the
/// freshly created pane has reached a harness dispatch-ready prompt.
pub fn auto_start_dispatch_ready_block(
    tmux: &Tmux,
    pane: &str,
    harness: &HarnessConfig,
) -> Option<AutoStartDispatchBlock> {
    let pane_shows_dispatch_ready_prompt = agent_doc_tmux_io::capture_pane(tmux, pane)
        .ok()
        .and_then(|content| pane_ready_prompt_candidate(tmux, pane, &content, harness))
        .is_some();
    let bare_shell_command = agent_doc_tmux_io::target_current_command(tmux, pane)
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .filter(|cmd| agent_doc_tmux::pane_current_command_is_bare_shell(cmd));
    classify_auto_start_dispatch_ready_block(AutoStartDispatchReadyFacts {
        pane_shows_dispatch_ready_prompt,
        bare_shell_command,
    })
}

/// Gate an auto-start send behind a bounded re-verify that the freshly created
/// pane has reached a harness dispatch-ready prompt.
pub fn reverify_auto_start_dispatch_ready(
    tmux: &Tmux,
    file: &std::path::Path,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();
    let poll_interval = Duration::from_millis(150);
    let last_block = loop {
        match auto_start_dispatch_ready_block(tmux, pane, harness) {
            None => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "auto_start_dispatch_ready_confirmed file={} pane={} harness={} elapsed_secs={} #jbtsiftnosub",
                        file.display(),
                        pane,
                        harness.binary,
                        start.elapsed().as_secs()
                    ),
                );
                return Ok(());
            }
            Some(block) if start.elapsed() >= timeout => break block,
            Some(_) => {}
        }
        std::thread::sleep(poll_interval);
    };
    match last_block {
        AutoStartDispatchBlock::StartingPane => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "dispatch_into_starting_pane file={} pane={} harness={} timeout_secs={} reason=harness_not_dispatch_ready_before_auto_start_send",
                    file.display(),
                    pane,
                    harness.binary,
                    timeout.as_secs()
                ),
            );
            anyhow::bail!(
                "route refusing to dispatch {} into pane {}: harness '{}' is still starting (no dispatch-ready prompt after {}s). The cold-start composer is not yet submit-ready — re-run `agent-doc route`/`Run Agent Doc` once the {} prompt is up, or claim/restart the harness.",
                harness.trigger_command(file_path),
                pane,
                harness.binary,
                timeout.as_secs(),
                harness.binary,
            );
        }
        AutoStartDispatchBlock::DeadShell(shell) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "dispatch_into_shell file={} pane={} harness={} pane_current_command={} timeout_secs={} reason=harness_exited_to_bare_shell_before_auto_start_send",
                    file.display(),
                    pane,
                    harness.binary,
                    shell,
                    timeout.as_secs()
                ),
            );
            anyhow::bail!(
                "route refusing to dispatch {} into pane {}: harness '{}' is not running (pane is a bare '{}' shell). The harness crashed/exited during cold-start — claim/restart the harness before routing.",
                harness.trigger_command(file_path),
                pane,
                harness.binary,
                shell,
            );
        }
    }
}
