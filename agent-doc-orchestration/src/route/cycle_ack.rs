//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_turn::cycle_ack::{
    CycleAckState, PromptBearingRouteContext, cycle_state_advances_start_ack,
    prompt_bearing_route_context_from_change,
};

pub(crate) fn wait_for_start_ack(
    file: &Path,
    baseline: Option<&agent_doc_cycle_state_io::CycleState>,
    timeout: Duration,
) -> Option<agent_doc_cycle_state_io::CycleState> {
    let start = std::time::Instant::now();
    let poll = Duration::from_millis(200);

    while start.elapsed() < timeout {
        if let Ok(Some(state)) = agent_doc_cycle_state_io::load(file)
            && cycle_state_advances_start_ack(
                CycleAckState {
                    cycle_id: &state.cycle_id,
                    phase: state.phase,
                    updated_at: state.updated_at,
                    last_event: &state.last_event,
                },
                baseline.map(|state| CycleAckState {
                    cycle_id: &state.cycle_id,
                    phase: state.phase,
                    updated_at: state.updated_at,
                    last_event: &state.last_event,
                }),
            )
        {
            return Some(state);
        }
        std::thread::sleep(poll);
    }
    None
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn retry_routed_cycle_ack_after_fresh_restart(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    baseline: Option<&agent_doc_cycle_state_io::CycleState>,
    marker: &str,
    ack_timeout: Duration,
) -> Result<Option<String>> {
    if harness.binary != "codex" {
        return Ok(None);
    }
    if !restart_via_supervisor_with_mode(file, session_id, "fresh") {
        return Ok(None);
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "route_cycle_start_retry_after_fresh_restart file={} pane={} harness={} marker={} timeout_secs={}",
            file.display(),
            pane,
            harness.binary,
            marker,
            ack_timeout.as_secs()
        ),
    );
    eprintln!(
        "[route] routed {} trigger for {} never started a new cycle in pane {} — restarting the live session fresh once before failing closed",
        harness.binary,
        file.display(),
        pane
    );

    wait_for_busy_restart_handoff(tmux, file, file_path, session_id, pane);
    let dispatch_pane =
        resolve_fresh_dispatch_target_after_ready_wait(tmux, session_id, pane, file_path, None)?;
    let ready = wait_for_agent_ready_outcome(
        tmux,
        &dispatch_pane,
        fresh_route_start_ack_timeout(cfg!(test)),
        harness,
    );
    if !ready.is_ready() {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_cycle_start_retry_fresh_restart_not_ready file={} pane={} harness={} outcome={}",
                file.display(),
                dispatch_pane,
                harness.binary,
                match &ready {
                    AgentReadyWaitOutcome::Ready => "ready",
                    AgentReadyWaitOutcome::Blocked { reason } => reason.as_str(),
                    AgentReadyWaitOutcome::TimedOut => "timed_out",
                }
            ),
        );
        emit_busy_route_diagnostic(tmux, &dispatch_pane, file, harness);
        if should_optimistically_accept_missing_cycle_ack(MissingCycleAckFacts {
            harness_binary: &harness.binary,
            live_child_for_file: true,
        }) {
            let baseline_id = baseline.map(|b| b.cycle_id.as_str());
            let miss = agent_doc_supervisor_io::startup_miss::record_startup_miss(
                file,
                &dispatch_pane,
                session_id,
                &harness.binary,
                agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger,
                baseline_id,
            )?;
            let miss_ts = agent_doc_supervisor::startup_miss::format_timestamp(miss.timestamp);
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_cycle_start_retry_fresh_restart_not_ready_optimistic file={} pane={} harness={} marker={} timeout_secs={} startup_miss_timestamp={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    marker,
                    fresh_route_start_ack_timeout(cfg!(test)).as_secs(),
                    miss_ts
                ),
            );
            eprintln!(
                "[route] fresh-restart retry for {} never restored a dispatch-ready prompt in pane {} after {}s, but the earlier reopen was already accepted — keeping the reroute optimistic",
                file.display(),
                dispatch_pane,
                fresh_route_start_ack_timeout(cfg!(test)).as_secs()
            );
            return Ok(Some(dispatch_pane));
        }
        match ready {
            AgentReadyWaitOutcome::Blocked { reason } => anyhow::bail!(
                "routed {} trigger for {} was accepted in pane {}, but the fresh-restart retry never reached a dispatch-ready prompt in pane {}: {}",
                harness.binary,
                file.display(),
                pane,
                dispatch_pane,
                reason
            ),
            AgentReadyWaitOutcome::TimedOut => anyhow::bail!(
                "routed {} trigger for {} was accepted in pane {}, but the fresh-restart retry never reached a dispatch-ready prompt in pane {} after waiting {}s",
                harness.binary,
                file.display(),
                pane,
                dispatch_pane,
                fresh_route_start_ack_timeout(cfg!(test)).as_secs()
            ),
            AgentReadyWaitOutcome::Ready => unreachable!("checked above"),
        };
    }

    let dispatch_start = match dispatch_existing_managed_reopen(
        tmux,
        file,
        session_id,
        &dispatch_pane,
        file_path,
        harness,
    ) {
        Ok(proof) => proof,
        Err(_) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_cycle_start_retry_fresh_restart_dispatch_not_consumed file={} pane={} harness={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary
                ),
            );
            return Ok(None);
        }
    };

    match wait_for_start_ack(file, baseline, ack_timeout) {
        Some(state) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_cycle_start_acknowledged_after_fresh_restart file={} pane={} harness={} cycle={} phase={} marker={} timeout_secs={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    state.cycle_id,
                    state.phase.as_str(),
                    marker,
                    ack_timeout.as_secs()
                ),
            );
            let _ = agent_doc_supervisor_io::startup_miss::clear_startup_miss(file);
            Ok(Some(dispatch_pane))
        }
        None => {
            if should_optimistically_accept_missing_cycle_ack(MissingCycleAckFacts {
                harness_binary: &harness.binary,
                live_child_for_file: true,
            }) {
                let baseline_id = baseline.map(|b| b.cycle_id.as_str());
                let miss = agent_doc_supervisor_io::startup_miss::record_startup_miss(
                    file,
                    &dispatch_pane,
                    session_id,
                    &harness.binary,
                    agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger,
                    baseline_id,
                )?;
                let miss_ts = agent_doc_supervisor::startup_miss::format_timestamp(miss.timestamp);
                emit_startup_miss_diagnostic(
                    tmux,
                    &dispatch_pane,
                    file,
                    &format!(
                        "fresh-restart retry trigger {} but no document cycle started for pending {} after {}s (startup-miss {})",
                        dispatch_start.dispatch_stage_label(),
                        marker,
                        ack_timeout.as_secs(),
                        miss_ts
                    ),
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_cycle_start_missing_after_fresh_restart_optimistic file={} pane={} harness={} marker={} timeout_secs={} startup_miss_timestamp={}",
                        file.display(),
                        dispatch_pane,
                        harness.binary,
                        marker,
                        ack_timeout.as_secs(),
                        miss_ts
                    ),
                );
                eprintln!(
                    "[route] fresh-restart reroute for {} never produced a new cycle ack for pending {} after {}s, but pane {} accepted the reopen — keeping the reroute optimistic",
                    file.display(),
                    marker,
                    ack_timeout.as_secs(),
                    dispatch_pane
                );
                return Ok(Some(dispatch_pane));
            }
            Ok(None)
        }
    }
}

pub(crate) fn pending_prompt_bearing_context_for_route(
    file: &Path,
    baseline: Option<&agent_doc_cycle_state_io::CycleState>,
) -> Result<Option<PromptBearingRouteContext>> {
    if baseline.is_some_and(|state| state.is_open()) {
        return Ok(None);
    }
    let Some(change) = crate::session_check::first_unstarted_prompt_bearing_change(file)? else {
        return Ok(None);
    };
    let Some(context) = prompt_bearing_route_context_from_change(&change) else {
        return Ok(None);
    };
    Ok(Some(context))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn require_routed_cycle_ack(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    baseline: Option<&agent_doc_cycle_state_io::CycleState>,
    prompt_bearing_marker: Option<&str>,
    live_child_for_file: bool,
    dispatch_start: RoutedDispatchStartProof,
) -> Result<Option<String>> {
    if !should_require_routed_cycle_ack(RoutedCycleAckFacts {
        baseline_cycle_open: baseline.is_some_and(|state| state.is_open()),
        prompt_bearing_marker_present: prompt_bearing_marker.is_some(),
    }) {
        return Ok(None);
    }

    let marker = prompt_bearing_marker.expect("marker checked above");
    let ack_timeout = routed_cycle_ack_timeout(live_child_for_file, cfg!(test));
    if live_child_for_file {
        eprintln!(
            "[route] live agent-doc child active in pane {} for {} — waiting up to {}s for a new cycle ack for pending {}",
            pane,
            file.display(),
            ack_timeout.as_secs(),
            marker
        );
    }
    let ack_start = Instant::now();
    match wait_for_start_ack(file, baseline, ack_timeout) {
        Some(state) => {
            log_route_latency(
                file,
                "cycle_start_ack",
                ack_start.elapsed(),
                ack_timeout,
                pane,
                harness,
                "acknowledged",
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_cycle_start_acknowledged file={} pane={} harness={} cycle={} phase={} marker={} timeout_secs={}",
                    file.display(),
                    pane,
                    harness.binary,
                    state.cycle_id,
                    state.phase.as_str(),
                    marker,
                    ack_timeout.as_secs()
                ),
            );
            let _ = agent_doc_supervisor_io::startup_miss::clear_startup_miss(file);
            Ok(None)
        }
        None => {
            log_route_latency(
                file,
                "cycle_start_ack",
                ack_start.elapsed(),
                ack_timeout,
                pane,
                harness,
                "missing",
            );
            let optimistic_allowed =
                should_optimistically_accept_missing_cycle_ack(MissingCycleAckFacts {
                    harness_binary: &harness.binary,
                    live_child_for_file,
                });
            if live_child_for_file
                && let Some(dispatch_pane) = retry_routed_cycle_ack_after_fresh_restart(
                    tmux,
                    file,
                    pane,
                    session_id,
                    file_path,
                    harness,
                    baseline,
                    marker,
                    ack_timeout,
                )?
            {
                return Ok(Some(dispatch_pane));
            }
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_cycle_start_missing file={} pane={} harness={} marker={} timeout_secs={}",
                    file.display(),
                    pane,
                    harness.binary,
                    marker,
                    ack_timeout.as_secs()
                ),
            );
            let baseline_id = baseline.map(|b| b.cycle_id.as_str());
            let miss = agent_doc_supervisor_io::startup_miss::record_startup_miss(
                file,
                pane,
                session_id,
                &harness.binary,
                agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger,
                baseline_id,
            )?;
            let miss_ts = agent_doc_supervisor::startup_miss::format_timestamp(miss.timestamp);
            let dispatch_stage = dispatch_start.dispatch_stage_label();
            emit_startup_miss_diagnostic(
                tmux,
                pane,
                file,
                &format!(
                    "routed trigger {} but no document cycle started for pending {} after {}s (startup-miss {})",
                    dispatch_stage,
                    marker,
                    ack_timeout.as_secs(),
                    miss_ts
                ),
            );
            if optimistic_allowed {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_cycle_start_missing_optimistic file={} pane={} harness={} marker={} timeout_secs={} startup_miss_timestamp={}",
                        file.display(),
                        pane,
                        harness.binary,
                        marker,
                        ack_timeout.as_secs(),
                        miss_ts
                    ),
                );
                eprintln!(
                    "[route] routed {} trigger for {} never produced a new cycle ack for pending {} after {}s, but the correct pane accepted the reopen — keeping the reroute optimistic",
                    harness.binary,
                    file.display(),
                    marker,
                    ack_timeout.as_secs()
                );
                return Ok(Some(pane.to_string()));
            }
            anyhow::bail!(
                "routed {} trigger for {} was {} in pane {}, but no new document cycle started for pending {} after waiting {}s (startup_miss_timestamp={})",
                harness.binary,
                file.display(),
                dispatch_stage,
                pane,
                marker,
                ack_timeout.as_secs(),
                miss_ts
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use agent_doc_controller::dispatch::{PromptReadyBarrierFacts, classify_prompt_ready_barrier};
    use agent_doc_supervisor::ipc_protocol::{IpcMethod, IpcResponse};
    use agent_doc_supervisor_io::ipc::SupervisorIpc;
    #[test]
    fn route_enqueue_exchange_slash_command_keeps_literal_head_for_idle_drain() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let snapshot = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n"
        );
        let current = snapshot.replace(
            "<!-- /agent:exchange -->",
            "❯ /clear\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, &current).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, crate::ops_log::log_op).unwrap();

        let ctx = pending_prompt_bearing_context_for_route(&doc, None)
            .unwrap()
            .expect("exchange slash command should be route-visible");
        assert_eq!(ctx.prompt_text, "/clear");
        assert_eq!(ctx.slash_command.as_deref(), Some("/clear"));

        let _force_disk_guard = super::super::ForceDiskRouteWritesGuard::set(true);
        let outcome =
            enqueue_exchange_slash_command_for_idle_drain(&doc, &ctx, "test_exchange_slash")
                .unwrap()
                .expect("slash command should queue for idle drain");
        assert!(outcome.appended);
        assert_eq!(outcome.prompt_text, "/clear");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: go"));
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(updated.contains("\n/clear\n"), "{updated}");
        assert!(
            !updated.contains(":pushpin: /clear"),
            "slash command must stay literal so the idle-queue classifier sees it:\n{updated}"
        );
        assert_eq!(
            agent_doc_queue::queue_continuation::live_continuation_head(&updated).as_deref(),
            Some("/clear"),
            "queued exchange slash command should be the active literal drain head"
        );
        let snapshot = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            snapshot, updated,
            "route queueing must sync the snapshot so the command head is not treated as edited drift"
        );
    }
    #[test]
    fn route_enqueue_bare_exchange_slash_command_for_idle_drain() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let snapshot = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n"
        );
        let current = snapshot.replace(
            "<!-- /agent:exchange -->",
            "/clear\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, &current).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, crate::ops_log::log_op).unwrap();

        let ctx = pending_prompt_bearing_context_for_route(&doc, None)
            .unwrap()
            .expect("bare exchange slash command should be route-visible");
        assert_eq!(ctx.prompt_text, "/clear");
        assert_eq!(ctx.slash_command.as_deref(), Some("/clear"));

        let _force_disk_guard = super::super::ForceDiskRouteWritesGuard::set(true);
        let outcome = enqueue_exchange_slash_command_for_idle_drain(&doc, &ctx, "test_bare_slash")
            .unwrap()
            .expect("slash command should queue for idle drain");
        assert!(outcome.appended);
        assert_eq!(outcome.prompt_text, "/clear");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: go"));
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(updated.contains("\n/clear\n"), "{updated}");
        assert_eq!(
            agent_doc_queue::queue_continuation::live_continuation_head(&updated).as_deref(),
            Some("/clear"),
            "bare exchange slash command should be the active literal drain head"
        );
    }
    #[test]
    fn wait_for_start_ack_detects_new_preflight_cycle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("route-live-child-skip.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        let doc_for_thread = doc.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            agent_doc_cycle_state_io::start_preflight(&doc_for_thread, None, Some("# Session\n"))
                .unwrap();
        });

        let ack = wait_for_start_ack(&doc, None, Duration::from_secs(1));
        assert!(
            ack.is_some(),
            "fresh start should acknowledge a new preflight cycle"
        );
        assert_eq!(
            ack.unwrap().phase,
            agent_doc_turn::CyclePhase::PreflightStarted
        );
    }
    #[test]
    fn wait_for_start_ack_detects_new_committed_cycle_after_prior_commit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("route-live-pane-busy.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, None, Some("# Session\n")).unwrap();
        crate::pipeline_frontmatter::mark_committed(
            &doc,
            "commit_success",
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
        let baseline = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();

        let doc_for_thread = doc.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            agent_doc_cycle_state_io::start_preflight(&doc_for_thread, None, Some("# Session\n"))
                .unwrap();
            crate::pipeline_frontmatter::mark_committed(
                &doc_for_thread,
                "commit_success",
                Some("# Session\n"),
                Some("# Session\n"),
            )
            .unwrap();
        });

        let ack = wait_for_start_ack(&doc, Some(&baseline), Duration::from_secs(1))
            .expect("new committed cycle should count as startup acknowledgment");
        assert_ne!(ack.cycle_id, baseline.cycle_id);
        assert_eq!(ack.phase, agent_doc_turn::CyclePhase::Committed);
    }
    #[test]
    fn wait_for_start_ack_times_out_without_cycle_change() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("route-live-same-cycle.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, None, Some("# Session\n")).unwrap();
        crate::pipeline_frontmatter::mark_committed(
            &doc,
            "commit_success",
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
        let baseline = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();

        let ack = wait_for_start_ack(&doc, Some(&baseline), Duration::from_millis(250));
        assert!(
            ack.is_none(),
            "unchanged cycle state must not count as a fresh-start ack"
        );
    }
    #[test]
    fn wait_for_start_ack_ignores_same_committed_cycle_mutation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("route-live-ack-ok.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, None, Some("# Session\n")).unwrap();
        crate::pipeline_frontmatter::mark_committed(
            &doc,
            "commit_success",
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
        let baseline = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();

        let doc_for_thread = doc.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            crate::pipeline_frontmatter::mark_committed(
                &doc_for_thread,
                "commit_already_current",
                Some("# Session\n"),
                Some("# Session\n"),
            )
            .unwrap();
        });

        let ack = wait_for_start_ack(&doc, Some(&baseline), Duration::from_millis(350));
        assert!(
            ack.is_none(),
            "same committed cycle mutations must not count as a new routed-start ack"
        );
    }
    #[test]
    fn routed_cycle_ack_only_required_for_prompt_bearing_drift_on_closed_cycle() {
        assert!(!should_require_routed_cycle_ack(RoutedCycleAckFacts {
            baseline_cycle_open: false,
            prompt_bearing_marker_present: false,
        }));

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("route-live-owner-missing.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, None, Some("# Session\n")).unwrap();
        let open_state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert!(!should_require_routed_cycle_ack(RoutedCycleAckFacts {
            baseline_cycle_open: open_state.is_open(),
            prompt_bearing_marker_present: true,
        }));

        crate::pipeline_frontmatter::mark_committed(
            &doc,
            "commit_success",
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
        let committed_state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert!(should_require_routed_cycle_ack(RoutedCycleAckFacts {
            baseline_cycle_open: committed_state.is_open(),
            prompt_bearing_marker_present: true,
        }));
    }
    #[test]
    fn routed_cycle_ack_timeout_extends_for_live_children() {
        assert_eq!(
            routed_cycle_ack_timeout(false, cfg!(test)),
            Duration::from_secs(1)
        );
        assert_eq!(
            routed_cycle_ack_timeout(true, cfg!(test)),
            Duration::from_secs(2)
        );
    }
    #[test]
    fn fresh_route_start_ack_timeout_allows_restart_slack() {
        assert_eq!(
            fresh_route_start_ack_timeout(cfg!(test)),
            Duration::from_secs(2)
        );
    }
    #[test]
    fn pending_prompt_bearing_context_for_route_ignores_frontmatter_only_drift() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("route-frontmatter-only-drift.md");
        let snapshot = "---\nagent: claude\nagent_doc_session: test\n---\n\n\
<!-- agent:exchange patch=append -->\n\
### Re: prior — gpt-5\n\
Body\n\
<!-- /agent:exchange -->\n";
        let current = snapshot.replacen("agent: claude", "agent: codex", 1);
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, crate::ops_log::log_op).unwrap();

        let ctx = pending_prompt_bearing_context_for_route(&doc, None).unwrap();
        assert!(
            ctx.is_none(),
            "frontmatter-only drift must not force routed cycle acknowledgment"
        );
    }
    #[test]
    fn pending_prompt_bearing_context_for_route_ignores_answered_prompt_after_stale_boundary() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("route-stale-boundary-answered-tail.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "Can we run specific rubrics for fine tuning?\n",
            "### Re: specific rubrics — gpt-5\n\n",
            "Yes.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, crate::ops_log::log_op).unwrap();

        let ctx = pending_prompt_bearing_context_for_route(&doc, None).unwrap();
        assert!(
            ctx.is_none(),
            "answered prompt after a stale boundary must not force routed cycle acknowledgment"
        );
    }
    #[test]
    fn pending_prompt_bearing_context_for_route_ignores_raw_answered_prompt_after_stale_boundary() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("route-stale-boundary-raw-answered-tail.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "I renamed the repo to ClaudeScore/buildparty-investor-demo. Please update references\n",
            "I updated the repo-local references to the renamed GitHub repo.\n\n",
            "- `.gitmodules` now uses `git@github.com:ClaudeScore/buildparty-investor-demo.git`\n",
            "- The checked-out submodule at `buildparty-investor-demo/` now has `origin` set to the same URL\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, crate::ops_log::log_op).unwrap();

        let ctx = pending_prompt_bearing_context_for_route(&doc, None).unwrap();
        assert!(
            ctx.is_none(),
            "raw assistant completion prose after a stale-boundary prompt must not force routed cycle acknowledgment"
        );
    }
    #[test]
    fn pending_prompt_bearing_context_for_route_detects_plain_exchange_tail_prompt() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("route-stale-boundary-plain-tail.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "When I run `Run Agent Doc` on this document...nothing happens. Please diagnose the root cause failure and fix the root cause. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, crate::ops_log::log_op).unwrap();

        let ctx = pending_prompt_bearing_context_for_route(&doc, None)
            .unwrap()
            .expect("plain exchange-tail prompt should force routed ack gating");
        assert_eq!(
            ctx.marker,
            "prompt_target: When I run `Run Agent Doc` on this document...nothing happens. Please diagnose the root cause failure and fix the root cause. spec-test-build-install-commit-push"
        );
    }
}
