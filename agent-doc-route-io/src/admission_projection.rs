//! Reactive routed-admission projection I/O.

use anyhow::Result;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::direct_pane_dispatch::log_route_latency;
use crate::dispatch::{RouteDispatchEffects, dispatch_existing_managed_reopen};
use crate::dispatch_recovery::resolve_fresh_dispatch_target_after_ready_wait;
use crate::restart_handoff::wait_for_busy_restart_handoff;
use crate::startup_ready::{AgentReadyWaitOutcome, wait_for_agent_ready_outcome};
use crate::supervisor_runtime::restart_via_supervisor_with_mode;
use agent_doc_controller::dispatch::{
    MissingAdmissionProjectionFacts, RoutedAdmissionFacts, RoutedDispatchStartProof,
    fresh_route_admission_timeout, routed_admission_timeout_with_client_deadline,
    should_optimistically_accept_missing_admission_projection,
    should_require_routed_admission_projection,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_turn::op_log::OpsLogEvent;
use agent_doc_turn::prompt_bearing_route::PromptBearingRouteContext;
use tmux_router::Tmux;

#[derive(Debug, Clone, Copy)]
pub struct RouteAdmissionEffects {
    pub route_dispatch_effects: RouteDispatchEffects,
    pub emit_startup_miss_diagnostic: fn(&Tmux, &str, &Path, &str),
    pub emit_busy_route_diagnostic: fn(&Tmux, &str, &Path, &HarnessConfig),
}

pub fn wait_for_start_projection(
    file: &Path,
    baseline: Option<&agent_doc_cycle_state_io::CycleState>,
    timeout: Duration,
) -> Option<agent_doc_state_backbone::TurnAdmissionProjection> {
    agent_doc_controller_io::project_controller::await_turn_admission_projection_for_file(
        file,
        baseline.map(|state| state.cycle_id.as_str()),
        timeout,
    )
    .ok()
    .flatten()
}

#[allow(clippy::too_many_arguments)]
pub fn retry_routed_admission_after_fresh_restart(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    baseline: Option<&agent_doc_cycle_state_io::CycleState>,
    marker: &str,
    admission_timeout: Duration,
    effects: RouteAdmissionEffects,
) -> Result<Option<String>> {
    if !fresh_admission_restart_supported(&harness.binary) {
        return Ok(None);
    }
    if !restart_via_supervisor_with_mode(file, session_id, "fresh") {
        return Ok(None);
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_admission_retry_after_fresh_restart file={} pane={} harness={} marker={} timeout_secs={}",
            file.display(),
            pane,
            harness.binary,
            marker,
            admission_timeout.as_secs()
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
        fresh_route_admission_timeout(cfg!(test)),
        harness,
    );
    if !ready.is_ready() {
        agent_doc_ops_log_io::log_op(
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
        (effects.emit_busy_route_diagnostic)(tmux, &dispatch_pane, file, harness);
        if should_optimistically_accept_missing_admission_projection(
            MissingAdmissionProjectionFacts {
                harness_binary: &harness.binary,
                live_child_for_file: true,
            },
        ) {
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
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_cycle_start_retry_fresh_restart_not_ready_optimistic file={} pane={} harness={} marker={} timeout_secs={} startup_miss_timestamp={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    marker,
                    fresh_route_admission_timeout(cfg!(test)).as_secs(),
                    miss_ts
                ),
            );
            eprintln!(
                "[route] fresh-restart retry for {} never restored a dispatch-ready prompt in pane {} after {}s, but the earlier reopen was already accepted — keeping the reroute optimistic",
                file.display(),
                dispatch_pane,
                fresh_route_admission_timeout(cfg!(test)).as_secs()
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
                fresh_route_admission_timeout(cfg!(test)).as_secs()
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
        effects.route_dispatch_effects,
    ) {
        Ok(proof) => proof,
        Err(_) => {
            agent_doc_ops_log_io::log_op(
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

    match wait_for_start_projection(file, baseline, admission_timeout) {
        Some(state) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_admission_projected_after_fresh_restart file={} pane={} harness={} cycle={} phase={} marker={} timeout_secs={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    state.cycle_id,
                    state.phase.as_str(),
                    marker,
                    admission_timeout.as_secs()
                ),
            );
            let _ = agent_doc_supervisor_io::startup_miss::clear_startup_miss(file);
            Ok(Some(dispatch_pane))
        }
        None => {
            if should_optimistically_accept_missing_admission_projection(
                MissingAdmissionProjectionFacts {
                    harness_binary: &harness.binary,
                    live_child_for_file: true,
                },
            ) {
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
                (effects.emit_startup_miss_diagnostic)(
                    tmux,
                    &dispatch_pane,
                    file,
                    &format!(
                        "fresh-restart retry trigger {} but no document cycle started for pending {} after {}s (startup-miss {})",
                        dispatch_start.dispatch_stage_label(),
                        marker,
                        admission_timeout.as_secs(),
                        miss_ts
                    ),
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{} file={} pane={} harness={} marker={} timeout_secs={} startup_miss_timestamp={}",
                        OpsLogEvent::RouteCycleStartMissingAfterFreshRestartOptimistic,
                        file.display(),
                        dispatch_pane,
                        harness.binary,
                        marker,
                        admission_timeout.as_secs(),
                        miss_ts
                    ),
                );
                eprintln!(
                    "[route] fresh-restart reroute for {} never projected prompt admission for pending {} after {}s, but pane {} accepted the reopen — keeping the reroute optimistic",
                    file.display(),
                    marker,
                    admission_timeout.as_secs(),
                    dispatch_pane
                );
                return Ok(Some(dispatch_pane));
            }
            Ok(None)
        }
    }
}

fn fresh_admission_restart_supported(harness_binary: &str) -> bool {
    matches!(harness_binary, "claude" | "codex")
}

pub fn pending_prompt_bearing_context_for_route(
    file: &Path,
    baseline: Option<&agent_doc_cycle_state_io::CycleState>,
) -> Result<Option<PromptBearingRouteContext>> {
    if baseline.is_some_and(|state| state.is_open()) {
        return Ok(None);
    }
    let steering = agent_doc_session_check_io::realtime_steering_since_turn_baseline(file)?;
    let Some(context) = prompt_bearing_route_context_from_steering(&steering) else {
        return Ok(None);
    };
    Ok(Some(context))
}

fn prompt_bearing_route_context_from_steering(
    steering: &agent_doc_document_realtime::baseline_comparison::RealtimeSteering,
) -> Option<PromptBearingRouteContext> {
    let marker = steering.label()?;
    let preview = steering.preview()?.trim();
    if !matches!(
        steering,
        agent_doc_document_realtime::baseline_comparison::RealtimeSteering::PromptTarget { .. }
            | agent_doc_document_realtime::baseline_comparison::RealtimeSteering::ContentEdit { .. }
    ) {
        return None;
    }
    let prompt_text = agent_doc_queue::route_dispatch::route_prompt_text_for_change(preview)
        .unwrap_or_else(|| preview.trim_start_matches('❯').trim().to_string());
    let slash_command = agent_doc_queue::queue_command::slash_command_text(&prompt_text);
    Some(PromptBearingRouteContext {
        marker: format!("{marker}: {preview}"),
        prompt_text,
        slash_command,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn require_routed_admission_projection(
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
    // `#jbroutasync`: the requesting client's own deadline
    // (`submit.deadline_ms`), when it supplied one. Waiting past it cannot
    // produce a useful outcome.
    client_deadline: Option<std::time::Duration>,
    effects: RouteAdmissionEffects,
) -> Result<Option<String>> {
    if !should_require_routed_admission_projection(RoutedAdmissionFacts {
        baseline_cycle_open: baseline.is_some_and(|state| state.is_open()),
        prompt_bearing_marker_present: prompt_bearing_marker.is_some(),
    }) {
        return Ok(None);
    }

    let marker = prompt_bearing_marker.expect("marker checked above");
    let admission_timeout = routed_admission_timeout_with_client_deadline(
        live_child_for_file,
        cfg!(test),
        client_deadline,
    );
    if live_child_for_file {
        eprintln!(
            "[route] live agent-doc child active in pane {} for {} — waiting up to {}s for the admission projection for pending {}",
            pane,
            file.display(),
            admission_timeout.as_secs(),
            marker
        );
    }
    let projection_wait_started = Instant::now();
    match wait_for_start_projection(file, baseline, admission_timeout) {
        Some(state) => {
            log_route_latency(
                file,
                "admission_projection",
                projection_wait_started.elapsed(),
                admission_timeout,
                pane,
                harness,
                "projected",
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_admission_projected file={} pane={} harness={} cycle={} phase={} marker={} timeout_secs={}",
                    file.display(),
                    pane,
                    harness.binary,
                    state.cycle_id,
                    state.phase.as_str(),
                    marker,
                    admission_timeout.as_secs()
                ),
            );
            let _ = agent_doc_supervisor_io::startup_miss::clear_startup_miss(file);
            Ok(None)
        }
        None => {
            log_route_latency(
                file,
                "admission_projection",
                projection_wait_started.elapsed(),
                admission_timeout,
                pane,
                harness,
                "missing",
            );
            let optimistic_allowed = should_optimistically_accept_missing_admission_projection(
                MissingAdmissionProjectionFacts {
                    harness_binary: &harness.binary,
                    live_child_for_file,
                },
            );
            if live_child_for_file
                && let Some(dispatch_pane) = retry_routed_admission_after_fresh_restart(
                    tmux,
                    file,
                    pane,
                    session_id,
                    file_path,
                    harness,
                    baseline,
                    marker,
                    admission_timeout,
                    effects,
                )?
            {
                return Ok(Some(dispatch_pane));
            }
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{} file={} pane={} harness={} marker={} timeout_secs={}",
                    OpsLogEvent::RouteCycleStartMissing,
                    file.display(),
                    pane,
                    harness.binary,
                    marker,
                    admission_timeout.as_secs()
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
            (effects.emit_startup_miss_diagnostic)(
                tmux,
                pane,
                file,
                &format!(
                    "routed trigger {} but no document cycle started for pending {} after {}s (startup-miss {})",
                    dispatch_stage,
                    marker,
                    admission_timeout.as_secs(),
                    miss_ts
                ),
            );
            if optimistic_allowed {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{} file={} pane={} harness={} marker={} timeout_secs={} startup_miss_timestamp={}",
                        OpsLogEvent::RouteCycleStartMissingOptimistic,
                        file.display(),
                        pane,
                        harness.binary,
                        marker,
                        admission_timeout.as_secs(),
                        miss_ts
                    ),
                );
                eprintln!(
                    "[route] routed {} trigger for {} never projected prompt admission for pending {} after {}s, but the correct pane accepted the reopen — keeping the reroute optimistic",
                    harness.binary,
                    file.display(),
                    marker,
                    admission_timeout.as_secs()
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
                admission_timeout.as_secs(),
                miss_ts
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_controller::dispatch::routed_admission_timeout;

    #[test]
    fn stale_hook_snapshot_recovery_restarts_supported_harnesses() {
        assert!(fresh_admission_restart_supported("claude"));
        assert!(fresh_admission_restart_supported("codex"));
        assert!(!fresh_admission_restart_supported("opencode"));
    }

    struct TestPipelineFrontmatterEffects;

    impl agent_doc_cycle_state_io::pipeline_frontmatter::PipelineFrontmatterEffects
        for TestPipelineFrontmatterEffects
    {
        fn read_current_document_content(
            &self,
            file: &Path,
            _source: &str,
        ) -> anyhow::Result<String> {
            Ok(std::fs::read_to_string(file)?)
        }

        fn converge_or_disk_write(
            &self,
            file: &Path,
            _current_content: &str,
            target_content: &str,
            _reason: &str,
        ) -> anyhow::Result<()> {
            std::fs::write(file, target_content)?;
            Ok(())
        }

        fn log_op(&self, file: &Path, message: &str) {
            agent_doc_ops_log_io::log_op(file, message);
        }
    }

    const TEST_PIPELINE_FRONTMATTER_EFFECTS: TestPipelineFrontmatterEffects =
        TestPipelineFrontmatterEffects;

    #[test]
    fn routed_admission_only_required_for_prompt_bearing_drift_on_closed_cycle() {
        assert!(!should_require_routed_admission_projection(
            RoutedAdmissionFacts {
                baseline_cycle_open: false,
                prompt_bearing_marker_present: false,
            }
        ));

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("route-live-owner-missing.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, None, Some("# Session\n")).unwrap();
        let open_state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert!(!should_require_routed_admission_projection(
            RoutedAdmissionFacts {
                baseline_cycle_open: open_state.is_open(),
                prompt_bearing_marker_present: true,
            }
        ));

        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
        let committed_state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert!(should_require_routed_admission_projection(
            RoutedAdmissionFacts {
                baseline_cycle_open: committed_state.is_open(),
                prompt_bearing_marker_present: true,
            }
        ));
    }
    #[test]
    fn routed_admission_timeout_extends_for_live_children() {
        assert_eq!(
            routed_admission_timeout(false, cfg!(test)),
            Duration::from_secs(1)
        );
        assert_eq!(
            routed_admission_timeout(true, cfg!(test)),
            Duration::from_secs(2)
        );
    }
    #[test]
    fn fresh_route_admission_timeout_allows_restart_slack() {
        assert_eq!(
            fresh_route_admission_timeout(cfg!(test)),
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let ctx = pending_prompt_bearing_context_for_route(&doc, None).unwrap();
        assert!(
            ctx.is_none(),
            "frontmatter-only drift must not force routed admission"
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let ctx = pending_prompt_bearing_context_for_route(&doc, None).unwrap();
        assert!(
            ctx.is_none(),
            "answered prompt after a stale boundary must not force routed admission"
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let ctx = pending_prompt_bearing_context_for_route(&doc, None).unwrap();
        assert!(
            ctx.is_none(),
            "raw assistant completion prose after a stale-boundary prompt must not force routed admission"
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let ctx = pending_prompt_bearing_context_for_route(&doc, None)
            .unwrap()
            .expect("plain exchange-tail prompt should require a routed admission projection");
        assert_eq!(
            ctx.marker,
            "prompt_target: When I run `Run Agent Doc` on this document...nothing happens. Please diagnose the root cause failure and fix the root cause. spec-test-build-install-commit-push"
        );
    }
}
