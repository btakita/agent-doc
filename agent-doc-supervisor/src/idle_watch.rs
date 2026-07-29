//! Pure supervisor idle-watch policy and messages.
//!
//! This module does not poll panes, run tmux, mutate files, or write ops logs.
//! Orchestration projects runtime facts into these helpers and performs the
//! effects itself.

use std::path::Path;
use std::time::Duration;

use agent_doc_state_scope::LocalProcessScope;
use lazily::{Computed, Source};

/// Scalar gate for a supervisor-owned captured-finalize resume. The effectful
/// idle watch supplies these facts. A durable captured operation already owns
/// the closeout lease, so recovery must remain live even while the harness turn
/// is blocked by its Stop hook. A pending Lazily current transition, IPC,
/// controller pressure, maintenance, and another resume worker remain the actual
/// concurrency gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapturedFinalizeResumeFacts {
    pub captured_operation_present: bool,
    pub actor_ready: bool,
    pub current_transition_pending: bool,
    pub ipc_inflight: u64,
    pub worker_in_flight: bool,
    pub retry_cooldown_elapsed: bool,
    pub controller_pressure_cooldown: bool,
    pub urgent_supervisor_maintenance: bool,
}

pub fn captured_finalize_resume_should_start(facts: CapturedFinalizeResumeFacts) -> bool {
    facts.captured_operation_present
        && !facts.current_transition_pending
        && facts.ipc_inflight == 0
        && !facts.worker_in_flight
        && facts.retry_cooldown_elapsed
        && !facts.controller_pressure_cooldown
        && !facts.urgent_supervisor_maintenance
}

/// Reactive trigger for one captured-finalize attempt.
///
/// State convergence is an input edge, not an error to retry. After an attempt
/// consumes the current edge, the graph remains quiet until either the
/// controller publishes a newer document-state edge or a genuinely failed
/// effect's backoff expires.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CapturedFinalizeResumeTriggerProjection {
    operation_key: Option<String>,
    state_epoch: u64,
    consumed_state_epoch: u64,
    effect_retry_epoch: u64,
    consumed_effect_retry_epoch: u64,
    needs_operator: bool,
}

impl CapturedFinalizeResumeTriggerProjection {
    fn ready(&self) -> bool {
        self.operation_key.is_some()
            && !self.needs_operator
            && (self.state_epoch > self.consumed_state_epoch
                || self.effect_retry_epoch > self.consumed_effect_retry_epoch)
    }
}

/// Process-scoped Lazily graph separating document-state wakeups from effect
/// retries for captured finalize.
pub struct CapturedFinalizeResumeTriggers {
    scope: LocalProcessScope,
    projection: Source<CapturedFinalizeResumeTriggerProjection>,
    ready: Computed<bool>,
}

impl Default for CapturedFinalizeResumeTriggers {
    fn default() -> Self {
        Self::new()
    }
}

impl CapturedFinalizeResumeTriggers {
    pub fn new() -> Self {
        let scope = LocalProcessScope::new();
        let projection = scope
            .ctx()
            .source(CapturedFinalizeResumeTriggerProjection::default());
        let projection_for_ready = projection;
        let ready = scope
            .ctx()
            .computed(move |ctx| ctx.get(&projection_for_ready).ready());
        Self {
            scope,
            projection,
            ready,
        }
    }

    /// Observe the durable identity currently eligible for recovery. A newly
    /// observed operation carries its own initial state edge.
    pub fn observe_operation(&self, operation_key: Option<String>) {
        let mut projection = self.scope.ctx().get(&self.projection);
        if projection.operation_key == operation_key {
            return;
        }
        projection.operation_key = operation_key;
        projection.needs_operator = false;
        projection.state_epoch = projection.state_epoch.saturating_add(1);
        projection.consumed_state_epoch = projection.state_epoch.saturating_sub(1);
        projection.consumed_effect_retry_epoch = projection.effect_retry_epoch;
        self.scope.ctx().set(&self.projection, projection);
    }

    /// Publish a controller-owned document-state edge.
    pub fn observe_state_edge(&self) {
        let mut projection = self.scope.ctx().get(&self.projection);
        projection.state_epoch = projection.state_epoch.saturating_add(1);
        self.scope.ctx().set(&self.projection, projection);
    }

    /// Publish expiry of a backoff for a failed effect. This is intentionally a
    /// different Source edge from document convergence.
    pub fn observe_effect_retry_due(&self) {
        let mut projection = self.scope.ctx().get(&self.projection);
        projection.effect_retry_epoch = projection.effect_retry_epoch.saturating_add(1);
        self.scope.ctx().set(&self.projection, projection);
    }

    pub fn consume_attempt(&self) {
        let mut projection = self.scope.ctx().get(&self.projection);
        projection.consumed_state_epoch = projection.state_epoch;
        projection.consumed_effect_retry_epoch = projection.effect_retry_epoch;
        self.scope.ctx().set(&self.projection, projection);
    }

    pub fn require_operator(&self) {
        let mut projection = self.scope.ctx().get(&self.projection);
        projection.needs_operator = true;
        self.scope.ctx().set(&self.projection, projection);
    }

    pub fn ready(&self) -> bool {
        self.scope.ctx().get(&self.ready)
    }
}

/// Exponential retry capped at one attempt per 30 seconds. Transient editor or
/// controller recovery can therefore complete unattended without recreating the
/// high-frequency finalize/backpressure loop it is intended to heal.
pub fn captured_finalize_resume_retry_delay(attempt: u32) -> Duration {
    const BASE_SECS: u64 = 2;
    const MAX_SECS: u64 = 30;
    let shift = attempt.saturating_sub(1).min(4);
    Duration::from_secs((BASE_SECS << shift).min(MAX_SECS))
}

/// `#supinstallfeedback` phases of the supervisor dogfood auto-install, used to
/// build the user-visible owned-pane status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorAutoInstallPhase {
    Started,
    Succeeded,
    Failed,
}

/// Build the owned-pane status message for an auto-install phase. The `Started`
/// line explicitly tells the operator not to press Enter because the visible
/// keepalive prompt can make the rebuild look like it is waiting on a keypress.
pub fn supervisor_auto_install_pane_message(phase: SupervisorAutoInstallPhase) -> &'static str {
    match phase {
        SupervisorAutoInstallPhase::Started => {
            "agent-doc: rebuilding the freshly-committed binary (~1 min) — do NOT press Enter; the supervisor auto-restarts when the build finishes"
        }
        SupervisorAutoInstallPhase::Succeeded => {
            "agent-doc: rebuild complete — recycling onto the fresh binary"
        }
        SupervisorAutoInstallPhase::Failed => {
            "agent-doc: auto-install failed — run the dogfood refresh to rebuild; staying on the current binary"
        }
    }
}

/// `#qstallguard` Layer C: should the supervisor idle-watch skip dispatch on a
/// queue under an accepted `admin queue pause`?
///
/// A pause suppresses unattended reinjection floods, not all draining. Skip only
/// when an in-session `/loop` owns the drain or there is no drainable head. With
/// no loop owner and a drainable head, the supervisor may perform the bounded
/// failsafe drain.
pub fn paused_idle_watch_should_skip(
    paused: bool,
    has_drainable_head: bool,
    loop_owner_lease_fresh: bool,
) -> bool {
    if !paused {
        return false;
    }
    loop_owner_lease_fresh || !has_drainable_head
}

/// Select the submit-mode diagnostic for an idle-queue dispatch from scalar
/// supervisor facts.
pub fn idle_queue_submit_mode(has_inject_pane: bool, harness_binary: &str) -> &'static str {
    if has_inject_pane {
        agent_doc_tmux_commands::tmux_submit_mode_for_harness(harness_binary)
    } else {
        "pty_cr"
    }
}

pub fn idle_queue_context_reset_ops_log_message(
    file: &Path,
    harness_binary: &str,
    clear_cmd: &str,
    target: &str,
    active_head: &str,
    reason: &str,
) -> String {
    format!(
        "idle_queue_watch_context_reset file={} harness={} cmd={:?} target={} head_bytes={} head_sha256={} reason={:?}",
        file.display(),
        harness_binary,
        clear_cmd,
        target,
        active_head.len(),
        agent_doc_hash::content_hash(active_head),
        reason,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_resume_facts() -> CapturedFinalizeResumeFacts {
        CapturedFinalizeResumeFacts {
            captured_operation_present: true,
            actor_ready: true,
            current_transition_pending: false,
            ipc_inflight: 0,
            worker_in_flight: false,
            retry_cooldown_elapsed: true,
            controller_pressure_cooldown: false,
            urgent_supervisor_maintenance: false,
        }
    }

    #[test]
    fn captured_finalize_resume_requires_a_quiet_single_flight_boundary() {
        assert!(captured_finalize_resume_should_start(ready_resume_facts()));
        assert!(captured_finalize_resume_should_start(
            CapturedFinalizeResumeFacts {
                actor_ready: false,
                ..ready_resume_facts()
            }
        ));
        for blocked in [
            CapturedFinalizeResumeFacts {
                current_transition_pending: true,
                ..ready_resume_facts()
            },
            CapturedFinalizeResumeFacts {
                ipc_inflight: 1,
                ..ready_resume_facts()
            },
            CapturedFinalizeResumeFacts {
                worker_in_flight: true,
                ..ready_resume_facts()
            },
            CapturedFinalizeResumeFacts {
                controller_pressure_cooldown: true,
                ..ready_resume_facts()
            },
        ] {
            assert!(!captured_finalize_resume_should_start(blocked));
        }
    }

    #[test]
    fn captured_finalize_resume_backoff_is_bounded() {
        assert_eq!(
            captured_finalize_resume_retry_delay(1),
            Duration::from_secs(2)
        );
        assert_eq!(
            captured_finalize_resume_retry_delay(2),
            Duration::from_secs(4)
        );
        assert_eq!(
            captured_finalize_resume_retry_delay(5),
            Duration::from_secs(30)
        );
        assert_eq!(
            captured_finalize_resume_retry_delay(99),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn captured_finalize_resume_waits_for_a_new_state_edge_instead_of_retrying() {
        let triggers = CapturedFinalizeResumeTriggers::new();
        triggers.observe_operation(Some("cycle:capture:response".to_string()));
        assert!(triggers.ready());
        triggers.consume_attempt();
        assert!(!triggers.ready(), "a consumed state edge must stay quiet");

        triggers.observe_state_edge();
        assert!(
            triggers.ready(),
            "a new controller state edge rearms recovery"
        );
        triggers.consume_attempt();
        assert!(!triggers.ready());

        triggers.observe_effect_retry_due();
        assert!(
            triggers.ready(),
            "effect failure backoff is an explicit, separate trigger"
        );
    }

    #[test]
    fn supervisor_auto_install_pane_message_started_warns_against_keypress() {
        let started = supervisor_auto_install_pane_message(SupervisorAutoInstallPhase::Started);
        assert!(started.contains("rebuild"), "must mention the rebuild");
        assert!(
            started.contains("do NOT press Enter"),
            "must warn against the misleading keepalive keypress"
        );
        assert!(
            started.contains("auto-restart"),
            "must promise the supervisor restarts itself"
        );

        let ok = supervisor_auto_install_pane_message(SupervisorAutoInstallPhase::Succeeded);
        let fail = supervisor_auto_install_pane_message(SupervisorAutoInstallPhase::Failed);
        assert!(ok.contains("complete") && ok.contains("recycling"));
        assert!(fail.contains("failed") && fail.contains("current binary"));
        assert_ne!(started, ok);
        assert_ne!(ok, fail);
    }

    #[test]
    fn paused_failsafe_drains_only_when_no_loop_owner_holds_a_drainable_head() {
        assert!(
            paused_idle_watch_should_skip(true, true, true),
            "loop owner present -> defer (skip)"
        );
        assert!(
            paused_idle_watch_should_skip(true, false, false),
            "no drainable head -> skip"
        );
        assert!(
            !paused_idle_watch_should_skip(true, true, false),
            "paused + drainable + no loop owner -> drain"
        );
        assert!(!paused_idle_watch_should_skip(false, true, false));
        assert!(!paused_idle_watch_should_skip(false, false, true));
    }

    #[test]
    fn idle_queue_submit_mode_uses_enter_for_codex_owner_pane() {
        assert_eq!(idle_queue_submit_mode(true, "codex"), "tmux_text_enter");
    }

    #[test]
    fn idle_queue_submit_mode_uses_pty_cr_without_owner_pane() {
        assert_eq!(idle_queue_submit_mode(false, "codex"), "pty_cr");
    }

    #[test]
    fn context_reset_ops_log_message_keeps_stable_fields() {
        let active_head = "agent:queue item";
        let message = idle_queue_context_reset_ops_log_message(
            Path::new("plan.md"),
            "codex",
            "/clear",
            "%1",
            active_head,
            "fresh context",
        );

        assert_eq!(
            message,
            format!(
                "idle_queue_watch_context_reset file=plan.md harness=codex cmd=\"/clear\" target=%1 head_bytes=16 head_sha256={} reason=\"fresh context\"",
                agent_doc_hash::content_hash(active_head)
            )
        );
    }
}
