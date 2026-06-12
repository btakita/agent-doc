//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;


pub(crate) fn cycle_state_advances_start_ack(
    current: &crate::cycle_state::CycleState,
    baseline: Option<&crate::cycle_state::CycleState>,
) -> bool {
    match baseline {
        None => true,
        Some(previous) if previous.is_open() => {
            current.cycle_id != previous.cycle_id
                || current.updated_at != previous.updated_at
                || current.phase != previous.phase
                || current.last_event != previous.last_event
        }
        Some(previous) => current.cycle_id != previous.cycle_id,
    }
}

pub(crate) fn wait_for_start_ack(
    file: &Path,
    baseline: Option<&crate::cycle_state::CycleState>,
    timeout: Duration,
) -> Option<crate::cycle_state::CycleState> {
    let start = std::time::Instant::now();
    let poll = Duration::from_millis(200);

    while start.elapsed() < timeout {
        if let Ok(Some(state)) = crate::cycle_state::load(file)
            && cycle_state_advances_start_ack(&state, baseline)
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
    baseline: Option<&crate::cycle_state::CycleState>,
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
        fresh_route_start_ack_timeout(),
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
        if should_optimistically_accept_missing_cycle_ack(harness, true) {
            let baseline_id = baseline.map(|b| b.cycle_id.as_str());
            let miss = crate::startup_miss::record(
                file,
                &dispatch_pane,
                session_id,
                &harness.binary,
                crate::startup_miss::StartupMissOrigin::RoutedTrigger,
                baseline_id,
            )?;
            let miss_ts = crate::startup_miss::format_timestamp(miss.timestamp);
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_cycle_start_retry_fresh_restart_not_ready_optimistic file={} pane={} harness={} marker={} timeout_secs={} startup_miss_timestamp={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    marker,
                    fresh_route_start_ack_timeout().as_secs(),
                    miss_ts
                ),
            );
            eprintln!(
                "[route] fresh-restart retry for {} never restored a dispatch-ready prompt in pane {} after {}s, but the earlier reopen was already accepted — keeping the reroute optimistic",
                file.display(),
                dispatch_pane,
                fresh_route_start_ack_timeout().as_secs()
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
                fresh_route_start_ack_timeout().as_secs()
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
                    cycle_phase_name(state.phase),
                    marker,
                    ack_timeout.as_secs()
                ),
            );
            let _ = crate::startup_miss::clear(file);
            Ok(Some(dispatch_pane))
        }
        None => {
            if should_optimistically_accept_missing_cycle_ack(harness, true) {
                let baseline_id = baseline.map(|b| b.cycle_id.as_str());
                let miss = crate::startup_miss::record(
                    file,
                    &dispatch_pane,
                    session_id,
                    &harness.binary,
                    crate::startup_miss::StartupMissOrigin::RoutedTrigger,
                    baseline_id,
                )?;
                let miss_ts = crate::startup_miss::format_timestamp(miss.timestamp);
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

pub(crate) fn cycle_phase_name(phase: crate::cycle_state::CyclePhase) -> &'static str {
    match phase {
        crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
        crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
        crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
        crate::cycle_state::CyclePhase::Committed => "committed",
        crate::cycle_state::CyclePhase::Abandoned => "abandoned",
    }
}

pub(crate) fn fresh_route_start_ack_timeout() -> Duration {
    crate::flow::routed_reopen::fresh_route_start_ack_timeout(cfg!(test))
}

pub(crate) fn routed_cycle_ack_timeout(live_child_for_file: bool) -> Duration {
    crate::flow::routed_reopen::routed_cycle_ack_timeout(live_child_for_file, cfg!(test))
}

pub(crate) fn should_require_routed_cycle_ack(
    baseline: Option<&crate::cycle_state::CycleState>,
    prompt_bearing_marker: Option<&str>,
) -> bool {
    prompt_bearing_marker.is_some() && !baseline.is_some_and(|state| state.is_open())
}

pub(crate) fn should_optimistically_accept_missing_cycle_ack(
    harness: &HarnessConfig,
    live_child_for_file: bool,
) -> bool {
    harness.binary == "codex" && live_child_for_file
}

pub(crate) fn pending_prompt_bearing_context_for_route(
    file: &Path,
    baseline: Option<&crate::cycle_state::CycleState>,
) -> Result<Option<PendingPromptBearingRouteContext>> {
    if baseline.is_some_and(|state| state.is_open()) {
        return Ok(None);
    }
    let Some(change) = crate::session_check::first_unstarted_prompt_bearing_change(file)? else {
        return Ok(None);
    };
    let marker = match change.kind {
        crate::diff::PromptBearingChangeKind::PromptTarget => "prompt_target",
        crate::diff::PromptBearingChangeKind::ContentEdit => "content_edit",
        crate::diff::PromptBearingChangeKind::RecoveryArtifact
        | crate::diff::PromptBearingChangeKind::BoundaryArtifact => return Ok(None),
    };
    let preview = change
        .text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(change.text.as_str())
        .trim();
    let prompt_text = queue_prompt_text_for_route_change(&change.text)
        .unwrap_or_else(|| preview.trim_start_matches('❯').trim().to_string());
    let slash_command = crate::queue_command::slash_command_text(&prompt_text);
    Ok(Some(PendingPromptBearingRouteContext {
        marker: format!("{marker}: {preview}"),
        prompt_text,
        slash_command,
    }))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn require_routed_cycle_ack(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    baseline: Option<&crate::cycle_state::CycleState>,
    prompt_bearing_marker: Option<&str>,
    live_child_for_file: bool,
    dispatch_start: RoutedDispatchStartProof,
) -> Result<Option<String>> {
    if !should_require_routed_cycle_ack(baseline, prompt_bearing_marker) {
        return Ok(None);
    }

    let marker = prompt_bearing_marker.expect("marker checked above");
    let ack_timeout = routed_cycle_ack_timeout(live_child_for_file);
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
                    cycle_phase_name(state.phase),
                    marker,
                    ack_timeout.as_secs()
                ),
            );
            let _ = crate::startup_miss::clear(file);
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
                should_optimistically_accept_missing_cycle_ack(harness, live_child_for_file);
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
            let miss = crate::startup_miss::record(
                file,
                pane,
                session_id,
                &harness.binary,
                crate::startup_miss::StartupMissOrigin::RoutedTrigger,
                baseline_id,
            )?;
            let miss_ts = crate::startup_miss::format_timestamp(miss.timestamp);
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

pub(crate) fn recent_lines_contain_trigger(content: &str, trigger: &str) -> bool {
    let recent_lines: Vec<String> = content
        .lines()
        .rev()
        .take(8)
        .map(prompt::strip_ansi)
        .collect();
    recent_lines
        .iter()
        .any(|line| line_contains_trigger(line, trigger))
        || recent_lines_contain_wrapped_trigger(&recent_lines, trigger)
}

pub(crate) fn line_contains_trigger(line: &str, trigger: &str) -> bool {
    let mut offset = 0usize;
    while let Some(found) = line[offset..].find(trigger) {
        let start = offset + found;
        let end = start + trigger.len();
        let prev_ok = line[..start]
            .chars()
            .next_back()
            .map(|ch| ch.is_whitespace() || matches!(ch, '>' | '❯' | '⏵'))
            .unwrap_or(true);
        let next_ok = line[end..]
            .chars()
            .next()
            .map(|ch| ch.is_whitespace())
            .unwrap_or(true);
        if prev_ok && next_ok {
            return true;
        }
        offset = start + 1;
    }
    false
}

pub(crate) fn compact_trigger_text(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

pub(crate) fn strip_leading_prompt_prefix(line: &str) -> &str {
    let trimmed = line.trim_start();
    for prompt in ["❯", ">", "›", "⏵"] {
        if let Some(rest) = trimmed.strip_prefix(prompt) {
            return rest.trim_start();
        }
    }
    trimmed
}

pub(crate) fn shares_trigger_prefix(fragment: &str, trigger: &str) -> bool {
    let mut frag = fragment.chars();
    let mut trig = trigger.chars();
    loop {
        match (frag.next(), trig.next()) {
            (Some(left), Some(right)) if left == right => {}
            (Some(_), Some(_)) => return false,
            (None, _) | (_, None) => return true,
        }
    }
}

pub(crate) fn recent_lines_contain_wrapped_trigger(recent_lines_rev: &[String], trigger: &str) -> bool {
    let compact_trigger = compact_trigger_text(trigger);
    if compact_trigger.is_empty() {
        return false;
    }
    let lines: Vec<&String> = recent_lines_rev.iter().rev().collect();
    for start in 0..lines.len() {
        let first = compact_trigger_text(strip_leading_prompt_prefix(lines[start]));
        if first.is_empty() || !shares_trigger_prefix(&first, &compact_trigger) {
            continue;
        }
        let mut joined = first;
        if joined.contains(&compact_trigger) {
            return true;
        }
        for next in lines.iter().skip(start + 1).take(3) {
            joined.push_str(&compact_trigger_text(next));
            if joined.contains(&compact_trigger) {
                return true;
            }
            if joined.len() > compact_trigger.len() + 32 {
                break;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
use crate::flow::routed_reopen::{PromptReadyBarrierFacts, classify_prompt_ready_barrier};
use crate::supervisor::ipc::{IpcMethod, IpcResponse, SupervisorIpc};
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
    crate::snapshot::save(&doc, snapshot).unwrap();

    let ctx = pending_prompt_bearing_context_for_route(&doc, None)
        .unwrap()
        .expect("exchange slash command should be route-visible");
    assert_eq!(ctx.prompt_text, "/clear");
    assert_eq!(ctx.slash_command.as_deref(), Some("/clear"));

    let outcome = enqueue_exchange_slash_command_for_idle_drain(&doc, &ctx, "test_exchange_slash")
        .unwrap()
        .expect("slash command should queue for idle drain");
    assert!(outcome.appended);
    assert_eq!(outcome.prompt_text, "/clear");

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(updated.contains("queue: start"));
    assert!(updated.contains("<!-- agent:queue -->"));
    assert!(!updated.contains("agent:queue auto"));
    assert!(updated.contains("\n/clear\n"), "{updated}");
    assert!(
        !updated.contains(":pushpin: /clear"),
        "slash command must stay literal so the idle-queue classifier sees it:\n{updated}"
    );
    assert_eq!(
        crate::queue_continuation::live_continuation_head(&doc, &updated).as_deref(),
        Some("/clear"),
        "queued exchange slash command should be the active literal drain head"
    );
    let snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
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
    crate::snapshot::save(&doc, snapshot).unwrap();

    let ctx = pending_prompt_bearing_context_for_route(&doc, None)
        .unwrap()
        .expect("bare exchange slash command should be route-visible");
    assert_eq!(ctx.prompt_text, "/clear");
    assert_eq!(ctx.slash_command.as_deref(), Some("/clear"));

    let outcome = enqueue_exchange_slash_command_for_idle_drain(&doc, &ctx, "test_bare_slash")
        .unwrap()
        .expect("slash command should queue for idle drain");
    assert!(outcome.appended);
    assert_eq!(outcome.prompt_text, "/clear");

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(updated.contains("queue: start"));
    assert!(updated.contains("<!-- agent:queue -->"));
    assert!(!updated.contains("agent:queue auto"));
    assert!(updated.contains("\n/clear\n"), "{updated}");
    assert_eq!(
        crate::queue_continuation::live_continuation_head(&doc, &updated).as_deref(),
        Some("/clear"),
        "bare exchange slash command should be the active literal drain head"
    );
}
#[test]
fn recent_lines_contain_trigger_matches_claude_trigger() {
    let content = "\
history line
\x1b[32m❯\x1b[0m /agent-doc test.md
";
    assert!(recent_lines_contain_trigger(content, "/agent-doc test.md"));
    assert!(!recent_lines_contain_trigger(content, "agent-doc test.md"));
}
#[test]
fn recent_lines_contain_trigger_matches_codex_trigger() {
    let content = "\
history line
> agent-doc test.md
";
    assert!(recent_lines_contain_trigger(content, "agent-doc test.md"));
    assert!(!recent_lines_contain_trigger(content, "/agent-doc test.md"));
}
#[test]
fn recent_lines_contain_trigger_matches_wrapped_codex_trigger() {
    let trigger =
        "agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md";
    let content = "\
› agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/claud
escore-3.md
gpt-5.4 high · ~/work/btakita/agent-loop/src/session-share · Context 31% used
";
    assert!(
        recent_lines_contain_trigger(content, trigger),
        "wrapped Codex composer lines must still count as pending input"
    );
}
#[test]
fn line_contains_trigger_rejects_codex_substring_inside_claude_trigger() {
    assert!(line_contains_trigger(
        "❯ /agent-doc test.md",
        "/agent-doc test.md"
    ));
    assert!(!line_contains_trigger(
        "❯ /agent-doc test.md",
        "agent-doc test.md"
    ));
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
        crate::cycle_state::start_preflight(&doc_for_thread, None, Some("# Session\n")).unwrap();
    });

    let ack = wait_for_start_ack(&doc, None, Duration::from_secs(1));
    assert!(
        ack.is_some(),
        "fresh start should acknowledge a new preflight cycle"
    );
    assert_eq!(
        ack.unwrap().phase,
        crate::cycle_state::CyclePhase::PreflightStarted
    );
}
#[test]
fn wait_for_start_ack_detects_new_committed_cycle_after_prior_commit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("route-live-pane-busy.md");
    std::fs::write(&doc, "# Session\n").unwrap();

    crate::cycle_state::start_preflight(&doc, None, Some("# Session\n")).unwrap();
    crate::cycle_state::mark_committed(
        &doc,
        "commit_success",
        Some("# Session\n"),
        Some("# Session\n"),
    )
    .unwrap();
    let baseline = crate::cycle_state::load(&doc).unwrap().unwrap();

    let doc_for_thread = doc.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        crate::cycle_state::start_preflight(&doc_for_thread, None, Some("# Session\n")).unwrap();
        crate::cycle_state::mark_committed(
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
    assert_eq!(ack.phase, crate::cycle_state::CyclePhase::Committed);
}
#[test]
fn wait_for_start_ack_times_out_without_cycle_change() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("route-live-same-cycle.md");
    std::fs::write(&doc, "# Session\n").unwrap();

    crate::cycle_state::start_preflight(&doc, None, Some("# Session\n")).unwrap();
    crate::cycle_state::mark_committed(
        &doc,
        "commit_success",
        Some("# Session\n"),
        Some("# Session\n"),
    )
    .unwrap();
    let baseline = crate::cycle_state::load(&doc).unwrap().unwrap();

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

    crate::cycle_state::start_preflight(&doc, None, Some("# Session\n")).unwrap();
    crate::cycle_state::mark_committed(
        &doc,
        "commit_success",
        Some("# Session\n"),
        Some("# Session\n"),
    )
    .unwrap();
    let baseline = crate::cycle_state::load(&doc).unwrap().unwrap();

    let doc_for_thread = doc.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        crate::cycle_state::mark_committed(
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
    assert!(!should_require_routed_cycle_ack(None, None));

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("route-live-owner-missing.md");
    std::fs::write(&doc, "# Session\n").unwrap();
    crate::cycle_state::start_preflight(&doc, None, Some("# Session\n")).unwrap();
    let open_state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert!(!should_require_routed_cycle_ack(
        Some(&open_state),
        Some("prompt_target: ❯ follow-up question"),
    ));

    crate::cycle_state::mark_committed(
        &doc,
        "commit_success",
        Some("# Session\n"),
        Some("# Session\n"),
    )
    .unwrap();
    let committed_state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert!(should_require_routed_cycle_ack(
        Some(&committed_state),
        Some("prompt_target: ❯ follow-up question"),
    ));
}
#[test]
fn routed_cycle_ack_timeout_extends_for_live_children() {
    assert_eq!(routed_cycle_ack_timeout(false), Duration::from_secs(1));
    assert_eq!(routed_cycle_ack_timeout(true), Duration::from_secs(2));
}
#[test]
fn fresh_route_start_ack_timeout_allows_restart_slack() {
    assert_eq!(fresh_route_start_ack_timeout(), Duration::from_secs(2));
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
    crate::snapshot::save(&doc, snapshot).unwrap();

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
    crate::snapshot::save(&doc, snapshot).unwrap();

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
    crate::snapshot::save(&doc, snapshot).unwrap();

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
    crate::snapshot::save(&doc, snapshot).unwrap();

    let ctx = pending_prompt_bearing_context_for_route(&doc, None)
        .unwrap()
        .expect("plain exchange-tail prompt should force routed ack gating");
    assert_eq!(
        ctx.marker,
        "prompt_target: When I run `Run Agent Doc` on this document...nothing happens. Please diagnose the root cause failure and fix the root cause. spec-test-build-install-commit-push"
    );
}
}
