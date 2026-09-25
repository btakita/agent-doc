//! Route-owned supervisor completion thread.
//!
//! This module owns the effectful polling loop that notices a route-owned
//! document cycle has committed and decides whether the supervisor should reap
//! its owned pane. Callers provide the live supervisor state through a narrow
//! trait so this crate does not depend on orchestration internals.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use agent_doc_harness::HarnessConfig;
use agent_doc_supervisor::idle_reconcile::ready_busy_conflict_reconcile_decision;
use agent_doc_supervisor::route_owned::{
    RouteOwnedCycleFacts, RouteOwnedCyclePhase, RouteOwnedLivenessReason, RouteOwnedReapDecision,
    RouteOwnedReapEffect, RouteOwnedReapEffects, RouteOwnedReapPolicy, RouteOwnedStartPurpose,
    route_owned_cycle_committed_since_start, route_owned_liveness_reason_for_content,
    route_owned_reap_decision_for_purpose,
};

pub const ROUTE_OWNED_COMPLETION_POLL_INTERVAL: Duration = Duration::from_millis(500);
pub const ROUTE_OWNED_READY_BUSY_RECONCILE_TICKS: u32 = 4;

/// `#stashpaneunbounded`: how often the layout-provision orphan check may issue
/// its tmux observation. The completion loop polls at 500ms, and an orphan pane
/// is by definition doing nothing, so re-asking every tick would spend one tmux
/// query per provisioned pane per half-second to watch a fact that changes when
/// the operator moves a tab.
pub const ROUTE_OWNED_LAYOUT_PROVISION_ORPHAN_CHECK_INTERVAL: Duration = Duration::from_secs(15);

pub struct RouteOwnedCompletionConfig {
    pub file: PathBuf,
    pub baseline: Option<agent_doc_cycle_state_io::CycleState>,
    pub reap_policy: RouteOwnedReapPolicy,
    /// Why this route-owned supervisor was started. `#stashpaneunbounded`: a
    /// `LayoutProvision` supervisor holds a pane for an editor column and never
    /// dispatches, so its orphan condition differs from a dispatch owner's.
    pub start_purpose: RouteOwnedStartPurpose,
    pub harness: HarnessConfig,
    pub poll_interval: Duration,
    pub ready_busy_reconcile_ticks: u32,
    pub layout_provision_orphan_check_interval: Duration,
}

impl RouteOwnedCompletionConfig {
    pub fn new(
        file: PathBuf,
        baseline: Option<agent_doc_cycle_state_io::CycleState>,
        reap_policy: RouteOwnedReapPolicy,
        harness: HarnessConfig,
    ) -> Self {
        Self::with_start_purpose(
            file,
            baseline,
            reap_policy,
            RouteOwnedStartPurpose::default(),
            harness,
        )
    }

    pub fn with_start_purpose(
        file: PathBuf,
        baseline: Option<agent_doc_cycle_state_io::CycleState>,
        reap_policy: RouteOwnedReapPolicy,
        start_purpose: RouteOwnedStartPurpose,
        harness: HarnessConfig,
    ) -> Self {
        Self {
            file,
            baseline,
            reap_policy,
            start_purpose,
            harness,
            poll_interval: ROUTE_OWNED_COMPLETION_POLL_INTERVAL,
            ready_busy_reconcile_ticks: ROUTE_OWNED_READY_BUSY_RECONCILE_TICKS,
            layout_provision_orphan_check_interval:
                ROUTE_OWNED_LAYOUT_PROVISION_ORPHAN_CHECK_INTERVAL,
        }
    }
}

pub trait RouteOwnedCompletionState: Send + Sync + 'static {
    fn actor_ready(&self) -> bool;
    fn ready_busy_blocker_reason(&self, harness: &HarnessConfig) -> Option<String>;
    fn live_pane_busy_reason(&self, harness: &HarnessConfig) -> Option<String>;
    /// Observe the child renderer without trusting a possibly stale controller `Ready` state.
    /// Destructive orphan cleanup must use this stronger proof: a live turn can coexist with
    /// stale actor readiness while a resumed session is still producing output.
    fn observed_live_pane_busy_reason(&self, harness: &HarnessConfig) -> Option<String> {
        self.live_pane_busy_reason(harness)
    }
    /// Whether the owned child has admitted a real harness interaction.
    ///
    /// A layout-provision owner that carries a turn is no longer an unused
    /// layout placeholder. The completion loop promotes that fact to a sticky
    /// dispatch purpose before it can run orphan cleanup.
    fn live_pane_interaction_observed(&self, _harness: &HarnessConfig) -> bool {
        false
    }
    fn owned_pane_label(&self) -> String;
    /// Whether THIS supervisor's own pane currently sits in a `stash` window.
    ///
    /// `#stashpaneunbounded`: one question about the one pane this supervisor
    /// owns. It never enumerates panes and never matches on a working directory
    /// or command line, so no answer it gives can be about another session's
    /// pane. Defaults to `false` so a state that cannot observe its pane keeps
    /// the pre-existing keep-alive behaviour.
    fn owned_pane_is_stashed(&self) -> bool {
        false
    }
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

pub fn spawn_route_owned_completion_thread<S, L>(
    state: Arc<S>,
    config: RouteOwnedCompletionConfig,
    completed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    mut session_log: Option<L>,
    log_session_event: fn(&mut Option<L>, &str),
) -> std::thread::JoinHandle<()>
where
    S: RouteOwnedCompletionState,
    L: Send + 'static,
{
    std::thread::Builder::new()
        .name("route-owned-completion".into())
        .spawn(move || {
            let RouteOwnedCompletionConfig {
                file,
                baseline,
                reap_policy,
                start_purpose,
                harness,
                poll_interval,
                ready_busy_reconcile_ticks,
                layout_provision_orphan_check_interval,
            } = config;
            let mut baseline = baseline.as_ref().map(route_owned_facts_from_cycle_state);
            let reap_effects = RouteOwnedReapEffects::new();
            let mut effective_start_purpose = start_purpose;
            // A layout-provision start may resume a child beside a terminal cycle left by the
            // previous pane. Give that fresh child one complete observation interval to admit
            // its new prompt before treating the inherited terminal cycle as an orphan. An
            // immediate first check races child attachment and reaps the new pane as stale.
            let mut next_orphan_check =
                Instant::now() + layout_provision_orphan_check_interval;
            let mut ready_busy_ticks: u32 = 0;
            let mut ready_busy_key: Option<(String, String)> = None;
            let mut ready_busy_logged_key: Option<(String, String)> = None;
            while !stop.load(Ordering::Relaxed) && !completed.load(Ordering::Relaxed) {
                if effective_start_purpose == RouteOwnedStartPurpose::LayoutProvision
                    && state.live_pane_interaction_observed(&harness)
                {
                    effective_start_purpose = RouteOwnedStartPurpose::Dispatch;
                    let event = format!(
                        "route_owned_start_purpose_promoted prior={} new={} reason=live_child_interaction pane={}",
                        start_purpose.as_str(),
                        effective_start_purpose.as_str(),
                        state.owned_pane_label(),
                    );
                    log_session_event(&mut session_log, &event);
                    agent_doc_ops_log_io::log_op(&file, &event);
                }
                let layout_provision_owner = reap_policy == RouteOwnedReapPolicy::KeepAlive
                    && effective_start_purpose == RouteOwnedStartPurpose::LayoutProvision;
                if let Ok(Some(cycle_state)) = load_route_owned_cycle_state(&file) {
                    let facts = route_owned_facts_from_cycle_state(&cycle_state);
                    // `#stashpaneunbounded`: evaluated BEFORE the commit-edge
                    // machinery below, because that machinery never fires for the
                    // shape that actually accumulates — a pane provisioned for an
                    // editor column and then never used. Its document's cycle
                    // never changes, so `route_owned_cycle_committed_since_start`
                    // stays false and no decision is ever reached. The pane then
                    // lives as long as the tmux server.
                    if layout_provision_owner
                        && !facts.phase.is_open()
                        && Instant::now() >= next_orphan_check
                    {
                        next_orphan_check =
                            Instant::now() + layout_provision_orphan_check_interval;
                        if state.owned_pane_is_stashed()
                            && state.observed_live_pane_busy_reason(&harness).is_none()
                        {
                            let liveness_reason =
                                route_owned_liveness_reason_for_file(&file, &facts);
                            let decision = route_owned_reap_decision_for_purpose(
                                reap_policy,
                                effective_start_purpose,
                                liveness_reason,
                                true,
                            );
                            if decision.reap {
                                let event = format!(
                                    "route_owned_reap_decision policy={} purpose={} decision=reap reason={} pane={} cycle={} event={}",
                                    reap_policy.as_str(),
                                    effective_start_purpose.as_str(),
                                    decision.reason,
                                    state.owned_pane_label(),
                                    cycle_state.cycle_id,
                                    cycle_state.last_event,
                                );
                                log_session_event(&mut session_log, &event);
                                agent_doc_ops_log_io::log_op(&file, &event);
                                completed.store(true, Ordering::Relaxed);
                                state.request_child_stop();
                                return;
                            }
                        }
                    }
                    if !route_owned_cycle_committed_since_start(&facts, baseline.as_ref()) {
                        if facts.phase.is_committed()
                            && state.paused_queue_has_no_supervisor_drainable_head(&file)
                        {
                            let decision = route_owned_reap_decision_for_purpose(
                                reap_policy,
                                effective_start_purpose,
                                None,
                                layout_provision_owner && state.owned_pane_is_stashed(),
                            );
                            let effect = RouteOwnedReapEffect {
                                policy: reap_policy,
                                decision: decision.clone(),
                                cycle_id: cycle_state.cycle_id.clone(),
                                cycle_event: cycle_state.last_event.clone(),
                                suppression: Some(
                                    "paused_queue_no_supervisor_drainable_head".to_string(),
                                ),
                            };
                            if let Some(effect) = reap_effects.observe_and_take(effect) {
                                let event = format!(
                                    "route_owned_reap_decision policy={} decision={} reason={} cycle={} event={} suppression={}",
                                    effect.policy.as_str(),
                                    if effect.decision.reap { "reap" } else { "keep_alive" },
                                    effect.decision.reason,
                                    effect.cycle_id,
                                    effect.cycle_event,
                                    effect.suppression.as_deref().unwrap_or("none"),
                                );
                                log_session_event(&mut session_log, &event);
                                agent_doc_ops_log_io::log_op(&file, &event);
                            }
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
                        route_owned_reap_decision_for_purpose(
                            reap_policy,
                            effective_start_purpose,
                            liveness_reason,
                            layout_provision_owner && state.owned_pane_is_stashed(),
                        )
                    };
                    let busy_guard = decision.reason.starts_with("live_pane_busy_no_idle_prompt");
                    let effect = RouteOwnedReapEffect {
                        policy: reap_policy,
                        decision: decision.clone(),
                        cycle_id: cycle_state.cycle_id.clone(),
                        cycle_event: cycle_state.last_event.clone(),
                        suppression: None,
                    };
                    if let Some(effect) = reap_effects.observe_and_take(effect) {
                        let event = format!(
                            "route_owned_reap_decision policy={} decision={} reason={} cycle={} event={}",
                            effect.policy.as_str(),
                            if effect.decision.reap { "reap" } else { "keep_alive" },
                            effect.decision.reason,
                            effect.cycle_id,
                            effect.cycle_event,
                        );
                        log_session_event(&mut session_log, &event);
                        agent_doc_ops_log_io::log_op(&file, &event);
                    }
                    if decision.reap {
                        completed.store(true, Ordering::Relaxed);
                        state.request_child_stop();
                        return;
                    }
                    if !busy_guard {
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
    use std::sync::atomic::AtomicU64;

    struct StashedCompletionState {
        started_at: Instant,
        stop_elapsed_millis: AtomicU64,
        busy: AtomicBool,
        busy_probe_count: AtomicU64,
        interaction_observed: AtomicBool,
        interaction_probe_count: AtomicU64,
    }

    impl RouteOwnedCompletionState for StashedCompletionState {
        fn actor_ready(&self) -> bool {
            false
        }

        fn ready_busy_blocker_reason(&self, _harness: &HarnessConfig) -> Option<String> {
            None
        }

        fn live_pane_busy_reason(&self, _harness: &HarnessConfig) -> Option<String> {
            None
        }

        fn observed_live_pane_busy_reason(&self, _harness: &HarnessConfig) -> Option<String> {
            self.busy_probe_count.fetch_add(1, Ordering::Relaxed);
            self.busy
                .load(Ordering::Relaxed)
                .then(|| "observed active child turn".to_string())
        }

        fn live_pane_interaction_observed(&self, _harness: &HarnessConfig) -> bool {
            self.interaction_probe_count.fetch_add(1, Ordering::Relaxed);
            self.interaction_observed.load(Ordering::Relaxed)
        }

        fn owned_pane_label(&self) -> String {
            "%test".to_string()
        }

        fn owned_pane_is_stashed(&self) -> bool {
            true
        }

        fn request_child_stop(&self) {
            self.stop_elapsed_millis.store(
                self.started_at.elapsed().as_millis() as u64,
                Ordering::Relaxed,
            );
        }
    }

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

    #[test]
    fn fresh_layout_provision_waits_before_reaping_stale_committed_cycle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some("body"), Some("body")).unwrap();
        agent_doc_cycle_state_io::mark_committed(&doc, "test", Some("body"), Some("body")).unwrap();
        let baseline = load_route_owned_cycle_state(&doc).unwrap().unwrap();

        let orphan_interval = Duration::from_millis(50);
        let state = Arc::new(StashedCompletionState {
            started_at: Instant::now(),
            stop_elapsed_millis: AtomicU64::new(u64::MAX),
            busy: AtomicBool::new(false),
            busy_probe_count: AtomicU64::new(0),
            interaction_observed: AtomicBool::new(false),
            interaction_probe_count: AtomicU64::new(0),
        });
        let completed = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let mut config = RouteOwnedCompletionConfig::with_start_purpose(
            doc,
            Some(baseline),
            RouteOwnedReapPolicy::KeepAlive,
            RouteOwnedStartPurpose::LayoutProvision,
            HarnessConfig::codex(),
        );
        config.poll_interval = Duration::from_millis(1);
        config.layout_provision_orphan_check_interval = orphan_interval;

        let handle = spawn_route_owned_completion_thread(
            Arc::clone(&state),
            config,
            completed,
            stop,
            None::<()>,
            |_, _| {},
        );
        handle.join().unwrap();

        let stopped_after = state.stop_elapsed_millis.load(Ordering::Relaxed);
        assert!(
            stopped_after >= orphan_interval.as_millis() as u64,
            "fresh layout provision was reaped after {stopped_after}ms before its {orphan_interval:?} admission grace elapsed"
        );
    }

    #[test]
    fn active_child_output_blocks_stashed_layout_provision_reap() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some("body"), Some("body")).unwrap();
        agent_doc_cycle_state_io::mark_committed(&doc, "test", Some("body"), Some("body")).unwrap();
        let baseline = load_route_owned_cycle_state(&doc).unwrap().unwrap();

        let state = Arc::new(StashedCompletionState {
            started_at: Instant::now(),
            stop_elapsed_millis: AtomicU64::new(u64::MAX),
            busy: AtomicBool::new(true),
            busy_probe_count: AtomicU64::new(0),
            interaction_observed: AtomicBool::new(false),
            interaction_probe_count: AtomicU64::new(0),
        });
        let completed = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let mut config = RouteOwnedCompletionConfig::with_start_purpose(
            doc,
            Some(baseline),
            RouteOwnedReapPolicy::KeepAlive,
            RouteOwnedStartPurpose::LayoutProvision,
            HarnessConfig::codex(),
        );
        config.poll_interval = Duration::from_millis(1);
        config.layout_provision_orphan_check_interval = Duration::from_millis(5);

        let handle = spawn_route_owned_completion_thread(
            Arc::clone(&state),
            config,
            Arc::clone(&completed),
            Arc::clone(&stop),
            None::<()>,
            |_, _| {},
        );
        let probe_deadline = Instant::now() + Duration::from_secs(1);
        while state.busy_probe_count.load(Ordering::Relaxed) == 0 && Instant::now() < probe_deadline
        {
            std::thread::sleep(Duration::from_millis(1));
        }

        assert!(
            state.busy_probe_count.load(Ordering::Relaxed) > 0,
            "completion thread never evaluated the orphan liveness guard"
        );
        assert!(!completed.load(Ordering::Relaxed));
        assert_eq!(state.stop_elapsed_millis.load(Ordering::Relaxed), u64::MAX);
        stop.store(true, Ordering::Relaxed);
        handle.join().unwrap();
    }

    #[test]
    fn used_layout_provision_stays_alive_after_returning_to_idle_prompt() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some("body"), Some("body")).unwrap();
        agent_doc_cycle_state_io::mark_committed(&doc, "test", Some("body"), Some("body")).unwrap();
        let baseline = load_route_owned_cycle_state(&doc).unwrap().unwrap();

        let state = Arc::new(StashedCompletionState {
            started_at: Instant::now(),
            stop_elapsed_millis: AtomicU64::new(u64::MAX),
            busy: AtomicBool::new(true),
            busy_probe_count: AtomicU64::new(0),
            interaction_observed: AtomicBool::new(true),
            interaction_probe_count: AtomicU64::new(0),
        });
        let completed = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let mut config = RouteOwnedCompletionConfig::with_start_purpose(
            doc,
            Some(baseline),
            RouteOwnedReapPolicy::KeepAlive,
            RouteOwnedStartPurpose::LayoutProvision,
            HarnessConfig::codex(),
        );
        config.poll_interval = Duration::from_millis(1);
        config.layout_provision_orphan_check_interval = Duration::from_millis(10);

        let handle = spawn_route_owned_completion_thread(
            Arc::clone(&state),
            config,
            Arc::clone(&completed),
            Arc::clone(&stop),
            None::<()>,
            |_, _| {},
        );
        let promotion_deadline = Instant::now() + Duration::from_secs(1);
        while state.interaction_probe_count.load(Ordering::Relaxed) == 0
            && Instant::now() < promotion_deadline
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(state.interaction_probe_count.load(Ordering::Relaxed) > 0);

        state.busy.store(false, Ordering::Relaxed);
        state.interaction_observed.store(false, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(40));

        assert!(
            !completed.load(Ordering::Relaxed),
            "a pane that carried a turn must not become a layout-only orphan at its next idle prompt"
        );
        assert_eq!(state.stop_elapsed_millis.load(Ordering::Relaxed), u64::MAX);
        stop.store(true, Ordering::Relaxed);
        handle.join().unwrap();
    }
}
