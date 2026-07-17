//! Route-owned supervisor completion thread.
//!
//! This module owns the effectful polling loop that notices a route-owned
//! document cycle has committed and decides whether the supervisor should reap
//! its owned pane. Callers provide the live supervisor state through a narrow
//! trait so this crate does not depend on orchestration internals.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use agent_doc_harness::HarnessConfig;
use agent_doc_supervisor::idle_reconcile::ready_busy_conflict_reconcile_decision;
use agent_doc_supervisor::route_owned::{
    RouteOwnedCycleFacts, RouteOwnedCyclePhase, RouteOwnedLivenessReason, RouteOwnedReapDecision,
    RouteOwnedReapPolicy, route_owned_cycle_committed_since_start,
    route_owned_liveness_reason_for_content, route_owned_reap_decision,
};

pub const ROUTE_OWNED_COMPLETION_POLL_INTERVAL: Duration = Duration::from_millis(500);
pub const ROUTE_OWNED_READY_BUSY_RECONCILE_TICKS: u32 = 4;

pub type SessionLogEventFn = fn(&mut Option<File>, &str);

pub struct RouteOwnedCompletionConfig {
    pub file: PathBuf,
    pub baseline: Option<agent_doc_cycle_state_io::CycleState>,
    pub reap_policy: RouteOwnedReapPolicy,
    pub harness: HarnessConfig,
    pub poll_interval: Duration,
    pub ready_busy_reconcile_ticks: u32,
}

impl RouteOwnedCompletionConfig {
    pub fn new(
        file: PathBuf,
        baseline: Option<agent_doc_cycle_state_io::CycleState>,
        reap_policy: RouteOwnedReapPolicy,
        harness: HarnessConfig,
    ) -> Self {
        Self {
            file,
            baseline,
            reap_policy,
            harness,
            poll_interval: ROUTE_OWNED_COMPLETION_POLL_INTERVAL,
            ready_busy_reconcile_ticks: ROUTE_OWNED_READY_BUSY_RECONCILE_TICKS,
        }
    }
}

pub trait RouteOwnedCompletionState: Send + Sync + 'static {
    fn actor_ready(&self) -> bool;
    fn ready_busy_blocker_reason(&self, harness: &HarnessConfig) -> Option<String>;
    fn live_pane_busy_reason(&self, harness: &HarnessConfig) -> Option<String>;
    fn owned_pane_label(&self) -> String;
    fn paused_queue_has_no_supervisor_drainable_head(&self, _file: &Path) -> bool {
        false
    }
    fn request_child_stop(&self);
}

pub fn route_owned_facts_from_cycle_state(
    state: &agent_doc_cycle_state_io::CycleState,
) -> RouteOwnedCycleFacts {
    let phase = if state.phase == agent_doc_turn::CyclePhase::Committed {
        RouteOwnedCyclePhase::Committed
    } else if state.is_open() {
        RouteOwnedCyclePhase::Open
    } else {
        RouteOwnedCyclePhase::Closed
    };
    RouteOwnedCycleFacts {
        cycle_id: state.cycle_id.clone(),
        phase,
        updated_at: state.updated_at,
        last_event: state.last_event.clone(),
        committed_file_hash: state.file_hash.clone(),
    }
}

pub fn route_owned_liveness_reason_for_file(
    file: &Path,
    facts: &RouteOwnedCycleFacts,
) -> Option<RouteOwnedLivenessReason> {
    let content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(err) => {
            return Some(RouteOwnedLivenessReason::AdapterFailure(format!(
                "read_failed:{err}"
            )));
        }
    };
    route_owned_liveness_reason_for_content(&content, facts.committed_file_hash.as_deref())
}

pub fn route_owned_liveness_after_paused_queue_suppression(
    reason: Option<RouteOwnedLivenessReason>,
    paused_queue_has_no_supervisor_drainable_head: bool,
) -> Option<RouteOwnedLivenessReason> {
    if paused_queue_has_no_supervisor_drainable_head
        && matches!(
            reason.as_ref(),
            Some(RouteOwnedLivenessReason::QueueNonEmpty)
        )
    {
        return None;
    }
    reason
}

pub fn load_route_owned_cycle_state(
    file: &Path,
) -> anyhow::Result<Option<agent_doc_cycle_state_io::CycleState>> {
    agent_doc_cycle_state_io::load_with_closeout_projection(file)
}

pub fn spawn_route_owned_completion_thread<S>(
    state: Arc<S>,
    config: RouteOwnedCompletionConfig,
    completed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    mut session_log: Option<File>,
    log_session_event: SessionLogEventFn,
) -> std::thread::JoinHandle<()>
where
    S: RouteOwnedCompletionState,
{
    std::thread::Builder::new()
        .name("route-owned-completion".into())
        .spawn(move || {
            let RouteOwnedCompletionConfig {
                file,
                baseline,
                reap_policy,
                harness,
                poll_interval,
                ready_busy_reconcile_ticks,
            } = config;
            let mut baseline = baseline.as_ref().map(route_owned_facts_from_cycle_state);
            let mut logged_busy_cycle: Option<String> = None;
            let mut ready_busy_ticks: u32 = 0;
            let mut ready_busy_key: Option<(String, String)> = None;
            let mut ready_busy_logged_key: Option<(String, String)> = None;
            while !stop.load(Ordering::Relaxed) && !completed.load(Ordering::Relaxed) {
                if let Ok(Some(cycle_state)) = load_route_owned_cycle_state(&file) {
                    let facts = route_owned_facts_from_cycle_state(&cycle_state);
                    if !route_owned_cycle_committed_since_start(&facts, baseline.as_ref()) {
                        if facts.phase.is_committed()
                            && state.paused_queue_has_no_supervisor_drainable_head(&file)
                        {
                            let decision = route_owned_reap_decision(reap_policy, None);
                            let event = format!(
                                "route_owned_reap_decision policy={} decision={} reason={} cycle={} event={} suppression=paused_queue_no_supervisor_drainable_head",
                                reap_policy.as_str(),
                                if decision.reap { "reap" } else { "keep_alive" },
                                decision.reason,
                                cycle_state.cycle_id,
                                cycle_state.last_event
                            );
                            log_session_event(&mut session_log, &event);
                            agent_doc_ops_log_io::log_op(&file, &event);
                            if decision.reap {
                                completed.store(true, Ordering::Relaxed);
                                state.request_child_stop();
                                return;
                            }
                        }
                        if !sleep_with_stop(&stop, poll_interval) {
                            return;
                        }
                        continue;
                    }

                    let actor_ready = state.actor_ready();
                    let ready_busy_reason = if actor_ready {
                        state.ready_busy_blocker_reason(&harness)
                    } else {
                        None
                    };
                    let key = ready_busy_reason
                        .as_ref()
                        .map(|reason| (cycle_state.cycle_id.clone(), reason.clone()));
                    if key.is_some() && key == ready_busy_key {
                        ready_busy_ticks = ready_busy_ticks.saturating_add(1);
                    } else {
                        ready_busy_key = key.clone();
                        ready_busy_ticks = u32::from(key.is_some());
                    }
                    let ready_busy_reconciled = ready_busy_conflict_reconcile_decision(
                        actor_ready,
                        ready_busy_reason.as_deref(),
                        false,
                        ready_busy_ticks,
                        ready_busy_reconcile_ticks,
                    );
                    if ready_busy_reconciled
                        && key.is_some()
                        && ready_busy_logged_key.as_ref() != key.as_ref()
                    {
                        let reason = ready_busy_reason.as_deref().unwrap_or("unknown");
                        let event = format!(
                            "owned_pane_ready_busy_conflict source=route_owned_completion harness={} pane={} reason={:?} after_ticks={} cycle={} event={}",
                            harness.binary,
                            state.owned_pane_label(),
                            reason,
                            ready_busy_reconcile_ticks,
                            cycle_state.cycle_id,
                            cycle_state.last_event
                        );
                        log_session_event(&mut session_log, &event);
                        agent_doc_ops_log_io::log_op(&file, &event);
                        ready_busy_logged_key = key.clone();
                    }

                    let live_pane_busy_reason = if ready_busy_reconciled {
                        None
                    } else {
                        state.live_pane_busy_reason(&harness)
                    };

                    let decision = if let Some(reason) = live_pane_busy_reason {
                        RouteOwnedReapDecision {
                            reap: false,
                            reason,
                        }
                    } else {
                        let liveness_reason = route_owned_liveness_after_paused_queue_suppression(
                            route_owned_liveness_reason_for_file(&file, &facts),
                            state.paused_queue_has_no_supervisor_drainable_head(&file),
                        );
                        route_owned_reap_decision(
                            reap_policy,
                            liveness_reason,
                        )
                    };
                    let event = format!(
                        "route_owned_reap_decision policy={} decision={} reason={} cycle={} event={}",
                        reap_policy.as_str(),
                        if decision.reap { "reap" } else { "keep_alive" },
                        decision.reason,
                        cycle_state.cycle_id,
                        cycle_state.last_event
                    );
                    let busy_guard = decision.reason.starts_with("live_pane_busy_no_idle_prompt");
                    if !busy_guard || logged_busy_cycle.as_deref() != Some(&cycle_state.cycle_id) {
                        log_session_event(&mut session_log, &event);
                        agent_doc_ops_log_io::log_op(&file, &event);
                    }
                    if decision.reap {
                        completed.store(true, Ordering::Relaxed);
                        state.request_child_stop();
                        return;
                    }
                    if busy_guard {
                        logged_busy_cycle = Some(cycle_state.cycle_id.clone());
                    } else {
                        logged_busy_cycle = None;
                        baseline = Some(facts);
                    }
                }
                if !sleep_with_stop(&stop, poll_interval) {
                    return;
                }
            }
        })
        .expect("spawn route-owned completion thread")
}

fn sleep_with_stop(stop: &AtomicBool, total: Duration) -> bool {
    let deadline = Instant::now() + total;
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        let remaining = deadline.saturating_duration_since(now);
        std::thread::sleep(std::cmp::min(remaining, Duration::from_millis(100)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_liveness_adapter_reports_read_failure() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.md");
        let facts = RouteOwnedCycleFacts {
            cycle_id: "cycle".to_string(),
            phase: RouteOwnedCyclePhase::Committed,
            updated_at: 1,
            last_event: "commit_success".to_string(),
            committed_file_hash: None,
        };

        let reason = route_owned_liveness_reason_for_file(&missing, &facts)
            .expect("missing file should report an adapter failure");

        assert!(matches!(
            reason,
            RouteOwnedLivenessReason::AdapterFailure(reason) if reason.starts_with("read_failed:")
        ));
    }

    #[test]
    fn paused_queue_suppression_drops_only_queue_liveness() {
        assert_eq!(
            route_owned_liveness_after_paused_queue_suppression(
                Some(RouteOwnedLivenessReason::QueueNonEmpty),
                true,
            ),
            None
        );
        assert_eq!(
            route_owned_liveness_after_paused_queue_suppression(
                Some(RouteOwnedLivenessReason::BacklogNonEmpty),
                true,
            ),
            Some(RouteOwnedLivenessReason::BacklogNonEmpty)
        );
        assert_eq!(
            route_owned_liveness_after_paused_queue_suppression(
                Some(RouteOwnedLivenessReason::QueueNonEmpty),
                false,
            ),
            Some(RouteOwnedLivenessReason::QueueNonEmpty)
        );
    }

    #[test]
    fn route_owned_cycle_state_reads_terminal_state_db_projection() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some("body"), Some("body")).unwrap();
        agent_doc_cycle_state_io::mark_committed(&doc, "test", Some("body"), Some("body")).unwrap();
        let state = load_route_owned_cycle_state(&doc)
            .unwrap()
            .expect("state.db projection");
        assert!(
            !state.is_open(),
            "route-owned completion should honor the terminal state.db projection"
        );
        assert!(
            !dir.path().join(".agent-doc/state/cycles").exists(),
            "cycle transitions must not emit compatibility files"
        );
    }
}
