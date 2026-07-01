//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_document::write_normalization::{
    AGENT_RESPONSE_COMPONENT, convergence_recovered_editor_wins_for_payload,
    convergence_recovered_editor_wins_outside_response,
};
#[cfg(test)]
use agent_doc_document_realtime::write_policy::live_prompt_drift_auto_recovery_safe;
use agent_doc_document_realtime::write_policy::{
    AckMismatchRecovery, classify_ack_mismatch_recovery,
    exchange_change_is_safe_historical_reduction, live_prompt_drift_recovery_target,
    normalize_visible_recovery_compare, should_refuse_disk_fallback,
    snapshot_contains_dropped_prompt, stale_snapshot_reset_drift,
};
use agent_doc_ipc_protocol::{is_socket_ack_timeout_error, is_socket_status_error};
use std::collections::HashSet;

pub fn guard_no_stale_snapshot_reset_drift(
    file: &Path,
    snapshot_doc: Option<&str>,
    current_doc: &str,
    phase: &str,
) -> Result<bool> {
    let Some(snapshot_doc) = snapshot_doc else {
        return Ok(false);
    };
    if let Ok(Some(cleaned)) =
        agent_doc_template::deleted_conversation_tail_cleanup(snapshot_doc, current_doc)
        && cleaned == current_doc
    {
        return Ok(false);
    }
    let Some(drift) = stale_snapshot_reset_drift(snapshot_doc, current_doc) else {
        return Ok(false);
    };
    let snapshot_len = drift.snapshot_len;
    let current_len = drift.current_len;
    if active_capture_response_removed(file, snapshot_doc, current_doc) {
        crate::ops_log::log_op(
            file,
            &format!(
                "stale_snapshot_rebase_skipped_active_capture file={} phase={} old_snap_len={} new_snap_len={}",
                file.display(),
                phase,
                snapshot_len,
                current_len
            ),
        );
        return Ok(false);
    }
    if let Some(reason) = classify_stale_snapshot_visible_rebase(file, snapshot_doc, current_doc) {
        crate::snapshot::save(file, current_doc)?;
        let crdt = agent_doc_merge::crdt::CrdtDoc::from_text(current_doc).encode_state();
        crate::snapshot::save_document_crdt(file, &crdt, current_doc)?;
        crate::ops_log::log_op(
            file,
            &format!(
                "stale_snapshot_visible_rebased file={} phase={} reason={} old_snap_len={} new_snap_len={}",
                file.display(),
                phase,
                reason,
                snapshot_len,
                current_len
            ),
        );
        return Ok(true);
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "stale_snapshot_reset_drift_blocked file={} phase={} snap_len={} file_len={}",
            file.display(),
            phase,
            snapshot_len,
            current_len
        ),
    );
    anyhow::bail!(
        "refusing {phase} for {}: snapshot is {} bytes but the visible file is {} bytes, which looks like a manual cleanup with stale snapshot/CRDT state. Reset the sidecars from the current file before writing: `agent-doc reset --from-current {}`",
        file.display(),
        snapshot_len,
        current_len,
        file.display()
    );
}

fn classify_stale_snapshot_visible_rebase(
    file: &Path,
    snapshot_doc: &str,
    current_doc: &str,
) -> Option<&'static str> {
    // `#provauth3`: the turn scope is the per-turn operator-edit provenance record,
    // but it is ABSENT after a `/clear` (fresh session resume). Do not hard-require
    // it — a binary-authored compaction is a known-origin reduction whose authority
    // does not depend on a live turn scope. Non-exchange component drift still needs
    // the scope to be classified as turn-independent, so that path fails closed
    // below when the scope is missing.
    let scope = agent_doc_turn_scope_io::load(file);
    // Known binary-origin signal: the binary recorded that it compacted this
    // document's exchange within the recent window. That makes a snapshot→visible
    // exchange shrink authoritative binary state, not a "suspicious manual cleanup"
    // — the central #provauth3 replacement of a content guess with a recorded
    // origin fact. (After a `/clear` the on-disk marker survives, so a resumed
    // session can still recognize its own prior compaction.)
    let recent_binary_compaction =
        crate::session_accretion::recent_exchange_compaction_timestamp(file)
            .ok()
            .flatten()
            .is_some();
    if active_capture_response_removed(file, snapshot_doc, current_doc) {
        return None;
    }

    let (snapshot_frontmatter, snapshot_body) =
        agent_doc_frontmatter::frontmatter::parse(snapshot_doc).ok()?;
    let (current_frontmatter, current_body) =
        agent_doc_frontmatter::frontmatter::parse(current_doc).ok()?;
    if !agent_doc_frontmatter::frontmatter::frontmatter_agent_only_equivalent(
        &snapshot_frontmatter,
        &current_frontmatter,
    ) {
        return None;
    }

    let snap_components = agent_doc_element::element::parse(snapshot_body).ok()?;
    let current_components = agent_doc_element::element::parse(current_body).ok()?;
    if snap_components.is_empty() || snap_components.len() != current_components.len() {
        return None;
    }

    let mut saw_exchange_trim = false;
    let mut saw_independent_component = false;
    for (snap_comp, current_comp) in snap_components.iter().zip(current_components.iter()) {
        if snap_comp.name != current_comp.name {
            return None;
        }
        if !is_backlog_component(&snap_comp.name)
            && snap_comp.patch_mode() != current_comp.patch_mode()
        {
            return None;
        }

        let snap_content =
            agent_doc_document::commit_normalization::normalize_component_content_for_absorb(
                snap_comp.content(snapshot_body),
            );
        let current_content =
            agent_doc_document::commit_normalization::normalize_component_content_for_absorb(
                current_comp.content(current_body),
            );
        if snap_content == current_content {
            continue;
        }

        if snap_comp.name == "exchange" {
            if exchange_change_is_safe_historical_reduction(
                snap_comp.content(snapshot_body),
                current_comp.content(current_body),
            ) {
                saw_exchange_trim = true;
                continue;
            }
            return None;
        }

        // A non-exchange component changed: this requires the turn scope to prove
        // the change is independent of the current turn. Without a scope we cannot
        // make that judgment, so fail closed (unchanged pre-#provauth3 behavior —
        // the old `?` on the scope load returned None before reaching this point).
        match scope.as_ref() {
            Some(scope)
                if component_change_is_turn_independent(
                    snapshot_body,
                    current_body,
                    &snap_comp.name,
                    scope,
                ) =>
            {
                saw_independent_component = true;
                continue;
            }
            _ => return None,
        }
    }

    match (saw_exchange_trim, saw_independent_component) {
        (true, true) => Some("historical_exchange_trim_unrelated_drift"),
        // Exchange-only safe reduction. Allow the rebase with a live turn scope
        // (in-session historical trim, the pre-#provauth3 path) OR a recorded
        // binary-origin compaction (post-`/clear` resume). Without either
        // provenance signal, fail closed so a genuine manual cleanup still trips
        // the guard and the operator is told to `reset --from-current`.
        (true, false) => {
            if scope.is_some() || recent_binary_compaction {
                Some("historical_exchange_trim")
            } else {
                None
            }
        }
        (false, true) => Some("unrelated_component_drift"),
        (false, false) => None,
    }
}

fn active_capture_response_removed(file: &Path, snapshot_doc: &str, current_doc: &str) -> bool {
    let Ok(Some(state)) = crate::cycle_state::load(file) else {
        return false;
    };
    if !state.is_open() {
        return false;
    }
    let Ok(Some(capture)) = crate::capture::load_active(file) else {
        return false;
    };
    !capture.response_body.trim().is_empty()
        && response_materialized_in_content(&capture.response_body, snapshot_doc)
        && !response_materialized_in_content(&capture.response_body, current_doc)
}

fn component_change_is_turn_independent(
    snap_body: &str,
    current_body: &str,
    component_name: &str,
    scope: &agent_doc_turn::turn_scope::TurnScope,
) -> bool {
    use agent_doc_turn::op_log::OpActor;
    use agent_doc_turn::turn_scope::{Address, classify_op};

    let events: Vec<_> = agent_doc_markdown_ast::events::diff_node_events(snap_body, current_body)
        .into_iter()
        .filter(|event| event.component == component_name)
        .collect();
    if events.is_empty() {
        return false;
    }

    events.iter().all(|event| {
        let address = Address::from_component_node_key(&event.component, &event.node_key);
        let node_index = event.after_index.or(event.before_index);
        !classify_op(
            OpActor::User,
            event.kind.as_str(),
            &address,
            node_index,
            scope,
        )
        .affects_turn()
    })
}

/// `#exch-intermix`: auto-recover the `live_prompt_drift_after_preflight`
/// closeout wedge by rebasing the missing agent response onto the realtime
/// document. Returns the recovered file content on success (the caller must
/// refresh its `file_content` and snapshot), or `None` when no recovery applies —
/// leaving the existing fail-closed guard to handle it.
///
/// Because this is automatic data mutation it is intentionally narrow and fails
/// closed on any doubt:
/// - the cycle must carry the `ipc_snapshot_adoption_blocked` flag (the drift
///   guard ran and preserved the response candidate for recovery),
/// - any recorded dropped prompt must still be present in the response candidate,
/// - realtime resolution must produce a response-only merge target.
pub fn try_auto_recover_live_prompt_drift(
    file: &Path,
    snapshot: &str,
    file_content: &str,
) -> Result<Option<String>> {
    let Some(cycle) = crate::cycle_state::load(file)? else {
        return Ok(None);
    };
    if !cycle.ipc_snapshot_adoption_blocked {
        return Ok(None);
    }
    // `#turnsaferecycle` Goal 2 — an `ipc_snapshot_adoption_blocked` drift against a
    // STALE supervisor is doomed to keep drifting; schedule an immediate forced PCP
    // recycle now (fail-open, gated on proven staleness) instead of only retrying the
    // buffer. This does not abort the in-flight recovery below — the recycle fires at
    // the controller's next serve-loop tick — it just guarantees the stale process is
    // replaced rather than thrashed.
    schedule_stale_supervisor_pcp_recycle(file, "live_prompt_drift_after_preflight");
    // #exch-intermix-falsedrop: a recorded dropped exchange/queue prompt only
    // represents real user-content loss when it is genuinely ABSENT from the
    // response candidate. A queue item consumed (struck) this cycle, or a user
    // prompt `content_ours` preserved, is recorded as "dropped" by the
    // drift-time candidate-vs-`content_ours` heuristic yet still survives in the
    // candidate. Only bail when a dropped prompt is missing from that candidate;
    // the realtime merge target below remains authoritative for current
    // operator-visible content.
    let dropped_missing_from_snapshot = cycle
        .dropped_exchange_prompts
        .iter()
        .chain(cycle.dropped_queue_prompts.iter())
        .any(|prompt| !snapshot_contains_dropped_prompt(snapshot, prompt));
    if dropped_missing_from_snapshot {
        return Ok(None);
    }
    let Some(recovery_target) = live_prompt_drift_recovery_target(
        snapshot,
        file_content,
        normalize_visible_recovery_compare,
    ) else {
        return Ok(None);
    };

    let ipc_project_root = file
        .canonicalize()
        .ok()
        .map(|c| resolve_ipc_project_root_pub(&c));
    let ipc_listener_active = ipc_project_root
        .as_deref()
        .map(crate::ipc_socket::is_listener_active)
        .unwrap_or(false);

    if let Some(project_root) = ipc_project_root.as_deref()
        && ipc_listener_active
    {
        match try_editor_converge_live_prompt_drift(
            file,
            project_root,
            &recovery_target,
            file_content,
        ) {
            Ok(Some(recovered)) => {
                log_live_prompt_drift_auto_recovered(
                    file,
                    &recovery_target,
                    file_content,
                    true,
                    "editor_ipc",
                );
                crate::flow::proof::log_flow_event(
                    file,
                    agent_doc_flow::types::FlowEvent::new(
                        agent_doc_flow::types::FlowName::DocumentMutation,
                        agent_doc_flow::types::FlowStage::IpcSnapshotAdoption,
                        agent_doc_flow::types::FlowOutcome::Completed,
                    )
                    .with_reason("live_prompt_drift_auto_recovered"),
                );
                eprintln!(
                    "[commit] auto-recovered live_prompt_drift wedge for {} via editor IPC convergence ({} bytes)",
                    file.display(),
                    recovery_target.len()
                );
                return Ok(Some(recovered));
            }
            Ok(None) => {}
            Err(err) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_error file={} error={}",
                        file.display(),
                        err
                    ),
                );
            }
        }
    }

    if ipc_listener_active {
        crate::ops_log::log_op(
            file,
            &format!(
                "[jbstalecache] auto_recovery_disk_write_blocked file={} target_len={} reason=editor_ipc_unconfirmed",
                file.display(),
                recovery_target.len()
            ),
        );
        return Ok(None);
    }

    atomic_write(file, &recovery_target).with_context(|| {
        format!(
            "live_prompt_drift auto-recover write for {}",
            file.display()
        )
    })?;
    crate::snapshot::save(file, &recovery_target)?;
    let crdt_doc = agent_doc_merge::crdt::CrdtDoc::from_text(&recovery_target);
    crate::snapshot::save_document_crdt(file, &crdt_doc.encode_state(), &recovery_target)?;
    log_live_prompt_drift_auto_recovered(
        file,
        &recovery_target,
        file_content,
        ipc_listener_active,
        "disk_fallback",
    );
    crate::flow::proof::log_flow_event(
        file,
        agent_doc_flow::types::FlowEvent::new(
            agent_doc_flow::types::FlowName::DocumentMutation,
            agent_doc_flow::types::FlowStage::IpcSnapshotAdoption,
            agent_doc_flow::types::FlowOutcome::Completed,
        )
        .with_reason("live_prompt_drift_auto_recovered"),
    );
    eprintln!(
        "[commit] auto-recovered live_prompt_drift wedge for {} — merged the missing response into the realtime document ({} bytes) so operator-visible edits stay authoritative",
        file.display(),
        recovery_target.len()
    );
    Ok(Some(recovery_target))
}

pub(crate) fn log_live_prompt_drift_auto_recovered(
    file: &Path,
    target: &str,
    file_content: &str,
    ipc_listener_active: bool,
    transport: &str,
) {
    crate::ops_log::log_op(
        file,
        &format!(
            "live_prompt_drift_auto_recovered file={} target_len={} file_len={} target_hash={} ipc_listener_active={} transport={}",
            file.display(),
            target.len(),
            file_content.len(),
            agent_doc_hash::content_hash(target),
            ipc_listener_active,
            transport
        ),
    );
}

/// `#supselfheal` Phase 2 — read the persisted editor-IPC wedge fact for `file` so
/// the route-owned supervisor idle watch can feed `write_wedged` into
/// `supervisor_recycle_action`. Returns `true` once the de-wedge circuit breaker
/// has latched `degraded` for the current session (the converge closeout path's
/// repeated refusals against a nominally-active listener). This is the wedge → owner
/// "request a recycle" channel: the converge process persists the latch, the
/// supervisor reads it here and combines it with its own staleness probe. The
/// converge side self-heals the marker the moment the socket recovers
/// (`ipc_direct_disk_degraded` → `listener_ack_recovered`), so a read of the raw
/// latch is intentional — the supervisor must not run its own socket probe. Best
/// effort: a missing/unreadable marker is "not wedged".
pub(crate) fn editor_ipc_write_wedged(project_root: &Path, file: &Path) -> bool {
    ipc_dewedge_marker_for_current_session(project_root, file)
        .ok()
        .flatten()
        .and_then(|value| value.get("degraded").and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

/// `#supselfheal` Phase 2 — log that a wedged editor-IPC write is now requesting a
/// supervisor recycle through the policy owner, instead of the converge path
/// silently looping refusals. Emitted once when the de-wedge latch first trips so
/// the wedge → recycle escalation is attributable in `ops.log`.
pub(crate) fn log_write_wedge_requests_supervisor_recycle(file: &Path, source: &str) {
    crate::ops_log::log_op(
        file,
        &format!(
            "write_wedged_supervisor_recycle_requested file={} source={} action=request_recycle_through_owner reason=repeated_ack_timeout_active_listener",
            file.display(),
            source
        ),
    );
}

/// `#turnsaferecycle` Goal 2 — pure: given stale-supervisor evidence at a proven
/// IPC drift, does the workflow kernel say to schedule an IMMEDIATE forced PCP
/// recycle (`RecycleNow`) rather than only surface advisory guidance? Routes through
/// the shared `decide_stale_supervisor` kernel so the stale-IPC path and the idle
/// watch make the same decision. An active IPC-drift closeout is treated as a
/// `turn_boundary` with pending work, so the only remaining gate is the operator's
/// auto-recycle opt-out.
pub(crate) fn stale_ipc_drift_forces_pcp_recycle(stale: bool, auto_recycle: bool) -> bool {
    matches!(
        agent_doc_workflow::decide_stale_supervisor(agent_doc_workflow::StaleSupervisorEvidence {
            stale,
            auto_recycle,
            turn_boundary: true,
            queue_head_pending: true,
        })
        .decision,
        agent_doc_workflow::WorkflowDecision::Supervisor(
            agent_doc_workflow::SupervisorWorkflowDecision::RecycleNow
        )
    )
}

/// `#turnsaferecycle` Goal 2 — when a stale-supervisor IPC drift is proven at write
/// closeout, immediately schedule a FORCED PCP recycle (`recycle_controller_force(..,
/// true)`) instead of only retrying the doomed buffer or emitting advisory guidance.
/// Fail-open: a missing project root, a fresh supervisor, or an opted-out auto-recycle
/// leaves the existing retry/advisory behavior in place. Returns `true` only when a
/// forced recycle was scheduled.
pub(crate) fn schedule_stale_supervisor_pcp_recycle(file: &Path, source: &str) -> bool {
    let Some(project_root) = agent_doc_fs::find_project_root(file) else {
        return false;
    };
    if crate::project_controller::stale_supervisor_warning_for_doc(file).is_none() {
        return false;
    }
    let auto_recycle = agent_doc_supervisor_io::config::supervisor_auto_recycle_enabled(file);
    if !stale_ipc_drift_forces_pcp_recycle(true, auto_recycle) {
        // Auto-recycle opted out → SurfaceStale: record advisory guidance, do not
        // force. The existing stale-supervisor warning already surfaces the manual
        // refresh path to the operator.
        crate::ops_log::log_op(
            file,
            &format!(
                "stale_supervisor_ipc_drift_surfaced file={} source={} action=advisory_only reason=auto_recycle_opted_out",
                file.display(),
                source
            ),
        );
        return false;
    }
    match crate::project_controller::recycle_controller_force(&project_root, true) {
        Ok(scheduled) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "stale_supervisor_ipc_drift_forced_recycle file={} source={} scheduled={} action=recycle_controller_force reason=stale_supervisor_ipc",
                    file.display(),
                    source,
                    scheduled
                ),
            );
            eprintln!(
                "[write] stale-supervisor IPC drift for {} ({source}); scheduling an immediate forced PCP recycle instead of thrashing the doomed write",
                file.display()
            );
            scheduled
        }
        Err(err) => {
            eprintln!(
                "[write] warning: failed to schedule forced PCP recycle on stale-supervisor IPC drift for {}: {err:#}",
                file.display()
            );
            false
        }
    }
}

/// `#turnsaferecycle` Goal 3 — the ONE shared stale-supervisor write-entry
/// short-circuit. Both IPC write entry points (`try_ipc`, `try_ipc_full_content`)
/// funnel through this before their proof-retry work, so every turn phase (preflight,
/// route, stream, session-check, finalize) defers UNIFORMLY instead of each phase
/// thrashing a doomed IPC write against a stale binary.
///
/// The staleness probe is the cheap on-disk marker the supervisor idle watch
/// publishes each tick (`agent_doc_turn_status_io::supervisor_stale`), so this adds no
/// RPC/`/proc` cost to the hot path and is absent (fresh) in unit tests. When stale it
/// schedules the recycle (Goal 2 forced PCP recycle + Goal 1 supervisor
/// recycle-request), records the recoverable `supervisor_freshness` binary outcome and
/// the `deferred_for_recycle` user-facing outcome, and returns the latter. `None` means
/// "supervisor fresh, proceed with the normal IPC write".
pub(crate) fn stale_supervisor_write_short_circuit(
    file: &Path,
    source: &str,
) -> Option<agent_doc_flow::outcome::UserFacingOutcome> {
    let base = file
        .canonicalize()
        .ok()
        .map(|canonical| resolve_ipc_project_root_pub(&canonical))?;
    if !agent_doc_turn_status_io::supervisor_stale(&base) {
        return None;
    }
    // Schedule the recycle so the stale process is replaced rather than thrashed:
    // the Goal 2 forced PCP recycle (re-gated on the authoritative staleness probe
    // inside the helper) plus the Goal 1 route-owned supervisor recycle-request.
    schedule_stale_supervisor_pcp_recycle(file, source);
    if let Err(err) = crate::project_controller::schedule_supervisor_recycle_for_doc(
        file,
        agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_INSTALL_FANOUT,
    ) {
        eprintln!(
            "[write] warning: failed to mark supervisor recycle-request for {}: {err:#}",
            file.display()
        );
    }
    let binary = agent_doc_flow::outcome::supervisor_stale_self_recycled_outcome();
    let ui = agent_doc_flow::outcome::deferred_for_recycle_outcome();
    crate::ops_log::log_op(
        file,
        &format!(
            "stale_supervisor_write_short_circuit file={} source={} {} {}",
            file.display(),
            source,
            binary.log_fields(),
            ui.log_fields()
        ),
    );
    eprintln!(
        "[write] stale supervisor hosting {} ({source}); deferring the IPC write for a recycle instead of thrashing the doomed buffer (deferred_for_recycle)",
        file.display()
    );
    Some(ui)
}

pub(crate) fn try_editor_converge_live_prompt_drift(
    file: &Path,
    project_root: &Path,
    target: &str,
    file_content: &str,
) -> Result<Option<String>> {
    let patches = live_prompt_drift_response_patches(file_content, target)?;
    let frontmatter = None;
    if patches.is_empty() && frontmatter.is_none() {
        crate::ops_log::log_op(
            file,
            &format!(
                "[jbstalecache] editor_convergence_skipped file={} skip=no_component_or_frontmatter_delta",
                file.display()
            ),
        );
        return Ok(None);
    }

    let canonical = file.canonicalize()?;
    let patch_id = uuid::Uuid::new_v4().to_string();
    let mut payload = serde_json::json!({
        "type": "patch",
        "file": canonical.to_string_lossy(),
        "patches": patches,
        "node_patches": [],
        "unmatched": "",
        "baseline": file_content,
        "reposition_boundary": false,
        "patch_id": patch_id,
    });
    if let Some(frontmatter) = frontmatter {
        payload["frontmatter"] = serde_json::Value::String(frontmatter);
    }
    if let Ok(Some(ref cycle)) = crate::cycle_state::load(file) {
        payload["cycle_id"] = serde_json::Value::String(cycle.cycle_id.clone());
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "[jbstalecache] editor_convergence_attempt file={} patch_id={} patches={} frontmatter={} target_hash={}",
            file.display(),
            payload
                .get("patch_id")
                .and_then(|value| value.as_str())
                .unwrap_or("-"),
            payload
                .get("patches")
                .and_then(|value| value.as_array())
                .map(Vec::len)
                .unwrap_or(0),
            payload.get("frontmatter").is_some(),
            agent_doc_hash::content_hash(target)
        ),
    );

    match crate::ipc_socket::send_message(project_root, &payload) {
        Ok(Some(_ack)) => {
            let patch_id = payload
                .get("patch_id")
                .and_then(|value| value.as_str())
                .unwrap_or("-");
            let sidecar = poll_ack_content_sidecar(
                project_root,
                patch_id,
                std::time::Duration::from_millis(500),
                std::time::Duration::from_millis(25),
            )?;
            let Some(recovered) = sidecar else {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_no_ack_content file={} patch_id={} action=block_external_disk_write",
                        file.display(),
                        patch_id
                    ),
                );
                return Ok(None);
            };
            if agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                &recovered,
            ) == agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                target,
            ) {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_succeeded file={} patch_id={} recovered_len={} transport=editor_ipc",
                        file.display(),
                        patch_id,
                        recovered.len()
                    ),
                );
                Ok(Some(recovered))
            } else if convergence_recovered_editor_wins_outside_response(&recovered, target) {
                // `#qpcwcmerge`: the editor buffer diverges from `content_ours` only
                // INSIDE components other than the agent's response component — its
                // live queue + same-cycle auto-strikes, or any plugin-defined
                // component — while the response and everything else match (the
                // response landed). Commit the editor buffer (editor-wins outside the
                // response) so HEAD equals the editor and the recurring post-commit
                // worktree drift (`#pcwc`) is eliminated, rather than blocking and
                // falling back to the `content_ours` disk write that drops the
                // editor's components.
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_succeeded file={} patch_id={} recovered_len={} target_len={} transport=editor_ipc resolution=editor_wins_outside_response #qpcwcmerge",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
                    ),
                );
                Ok(Some(recovered))
            } else {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_ack_mismatch file={} patch_id={} recovered_len={} target_len={} action=block_external_disk_write",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
                    ),
                );
                Ok(None)
            }
        }
        Ok(None) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "[jbstalecache] editor_convergence_no_ack file={} action=block_external_disk_write",
                    file.display()
                ),
            );
            Ok(None)
        }
        Err(err) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "[jbstalecache] editor_convergence_send_failed file={} error={} action=block_external_disk_write",
                    file.display(),
                    err
                ),
            );
            Ok(None)
        }
    }
}

fn live_prompt_drift_response_patches(
    file_content: &str,
    snapshot: &str,
) -> Result<Vec<serde_json::Value>> {
    let mut patches =
        agent_doc_document::component_patches::component_replace_patches(file_content, snapshot)?;
    // `live_prompt_drift` recovery is only authorized to materialize the agent's
    // response node. Non-response components and frontmatter belong to the live
    // editor/operator in this recovery path; if they differ, the containment gate
    // above has already failed closed instead of sending a patch that could reset
    // operator text.
    patches.retain(|patch| {
        patch.get("component").and_then(|value| value.as_str()) == Some(AGENT_RESPONSE_COMPONENT)
    });
    Ok(patches)
}

/// `#w42v`: converge a compacted document through the editor instead of a direct
/// disk write that diverges from the open JB buffer (`File Cache Conflict`).
///
/// Mirrors the `#q7jm` live_prompt_drift convergence: when a JB IPC listener is
/// active, send component `op:replace` patches for the changed components
/// (`exchange`, etc.) and verify the editor's ack content matches the compacted
/// target. Returns `Ok(true)` when converged via editor IPC (the caller skips
/// the disk write) and `Err` when editor convergence is unavailable or unproven.
/// The error is intentional: direct disk writes behind or around editor
/// convergence are the File Cache Conflict source this guard prevents.
/// `#fcc0`/`#w42v`: converge a full-document write through the editor IPC when a
/// JB listener is active, returning `true` when the editor buffer has been
/// converged to `target` (no disk write needed).
///
/// When a listener is active this computes the component-scoped delta between
/// `current_content` and `target` and applies it via `op:replace` patches through
/// the Document API, so the open buffer never diverges from disk and no
/// `File Cache Conflict` dialog fires. `source` labels the `ops.log` writeback
/// transport lines (`<source>_writeback ... transport=editor_ipc|blocked`)
/// so each write site is attributable; see [`converge_document_or_disk`] for
/// the shared converge-or-disk wrapper every document-mutating write routes
/// through.
/// `#6b5h`: at a proven-no-delivery editor-converge refusal point, fail closed.
///
/// The realtime cutover removes the old synchronous "send patch, wait, then
/// disk-fallback" branch: once a live editor owner or sidecar is observed,
/// missing or untrusted ACK proof marks editor convergence required. A direct
/// disk write is allowed only in detached realtime, after the current visible
/// file is rechecked as the merge input.
fn refuse_unproven_editor_delivery(
    file: &Path,
    source: &str,
    reason: &str,
    patch_id: Option<&str>,
) -> Result<bool> {
    // The endpoint is "live" when a capable sidecar is present, or when the
    // plugin-owner lease holds AND the socket listener actually answers. A
    // dead/stale socket with only a stale lease pid (JB restarted mid-turn) is
    // "absent" — the label that previously read "live" with no IDE running
    // (#jbsocketrobust).
    let sidecar_live = live_editor_sidecar_present(file);
    let owner_holds =
        !agent_doc_plugin_owner::disk_write_permitted_for_file(&file.to_string_lossy());
    let editor_endpoint =
        if should_refuse_disk_fallback(sidecar_live, owner_holds, editor_ipc_listener_active(file))
        {
            "live"
        } else {
            "absent"
        };
    crate::ops_log::log_op(
        file,
        &format!(
            "{source}_writeback file={} transport=blocked reason={reason} editor_endpoint={} action=editor_convergence_required",
            file.display(),
            editor_endpoint
        ),
    );
    let detail = format!("editor_endpoint={editor_endpoint}");
    if let Err(err) = crate::cycle_state::record_editor_convergence_required(
        file,
        source,
        reason,
        patch_id,
        Some(&detail),
    ) {
        eprintln!(
            "[write] WARNING: failed to record editor-convergence blocked closeout for {}: {err}",
            file.display()
        );
    }
    anyhow::bail!(
        "{source}: refused direct disk write for {} while editor convergence is unproven (reason={reason}, editor_endpoint={editor_endpoint})",
        file.display()
    );
}

fn live_editor_sidecar_present(file: &Path) -> bool {
    let indicator_path = file
        .canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .to_string();
    agent_doc_debounce::live_buffer_snapshots(&indicator_path)
        .iter()
        .any(agent_doc_debounce::live_buffer_snapshot_editor_is_live)
}

/// Whether an IPC socket listener is actually answering for `file`'s project.
///
/// Unlike plugin-owner pid-liveness or live-buffer sidecar checks, this probes
/// the socket itself (connect succeeds ⇒ listener present; a stale socket file
/// is cleaned up and reported dead). So a JetBrains that restarted mid-turn — a
/// stale/dead socket while a lease pid or sidecar can still look alive — is
/// correctly seen as not-answering (#jbsocketrobust).
fn editor_ipc_listener_active(file: &Path) -> bool {
    file.canonicalize()
        .ok()
        .map(|c| resolve_ipc_project_root_pub(&c))
        .map(|root| crate::ipc_socket::is_listener_active(&root))
        .unwrap_or(false)
}

fn try_detached_disk_write(
    file: &Path,
    current: &str,
    target: &str,
    source: &str,
    reason: &str,
) -> Result<bool> {
    // A capable live-buffer sidecar always fails closed (it may hold unsaved
    // operator text). A plugin-owner lease, by contrast, is a pid check that is
    // only authoritative when the socket actually answers: a JetBrains restarted
    // mid-turn (or a crashed FFI listener) leaves a stale lease pid on a dead
    // socket. We only reach here after an IPC send already failed, so re-probe
    // the socket — if no listener answers and no capable sidecar is present, the
    // endpoint is stale and the response must land on disk rather than wedge
    // forever (#jbsocketrobust).
    let sidecar_live = live_editor_sidecar_present(file);
    let owner_holds =
        !agent_doc_plugin_owner::disk_write_permitted_for_file(&file.to_string_lossy());
    if should_refuse_disk_fallback(sidecar_live, owner_holds, editor_ipc_listener_active(file)) {
        return Ok(false);
    }

    guard_visible_write_idle_and_current(file, source, current)?;
    atomic_write(file, target).with_context(|| {
        format!(
            "{source}: failed detached disk write for {}",
            file.display()
        )
    })?;
    crate::ops_log::log_op(
        file,
        &format!(
            "{source}_writeback file={} transport=disk_detached reason={} len={} hash={}",
            file.display(),
            reason,
            target.len(),
            agent_doc_hash::content_hash(target)
        ),
    );
    Ok(true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AckMismatchRefreshOutcome {
    NoRecovery,
    RevertedToCurrent,
    ReplayedTarget,
}

fn refresh_editor_after_ack_mismatch(
    file: &Path,
    project_root: &Path,
    canonical: &Path,
    target: &str,
    recovered: &str,
    current_content: &str,
    source: &str,
) -> AckMismatchRefreshOutcome {
    let stale_hash = agent_doc_hash::content_hash(recovered);
    let Some(recovery) = classify_ack_mismatch_recovery(
        target,
        recovered,
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers,
    ) else {
        crate::ops_log::log_op(
            file,
            &format!(
                "{source}_ack_mismatch_editor_refresh file={} transport=blocked reason=untrusted_ack_content_contains_user_drift action=leave_editor_owned_ack_content stale_len={} stale_hash={}",
                file.display(),
                recovered.len(),
                &stale_hash[..stale_hash.len().min(12)]
            ),
        );
        return AckMismatchRefreshOutcome::NoRecovery;
    };
    let (refresh_content, action, success_outcome) = match recovery {
        AckMismatchRecovery::RevertUntrustedAckToCurrent => (
            current_content,
            "revert_untrusted_ack_content",
            AckMismatchRefreshOutcome::RevertedToCurrent,
        ),
        AckMismatchRecovery::ReplayMissingAgentResponseToTarget => (
            target,
            "replay_missing_agent_response",
            AckMismatchRefreshOutcome::ReplayedTarget,
        ),
    };
    let target_hash = agent_doc_hash::content_hash(refresh_content);
    let failure_action = match recovery {
        AckMismatchRecovery::RevertUntrustedAckToCurrent => {
            "left_untrusted_ack_content_editor_owned"
        }
        AckMismatchRecovery::ReplayMissingAgentResponseToTarget => {
            "left_missing_agent_response_editor_owned"
        }
    };
    let failure_reason = match recovery {
        AckMismatchRecovery::RevertUntrustedAckToCurrent => "safe_stale_prompt_refresh_failed",
        AckMismatchRecovery::ReplayMissingAgentResponseToTarget => {
            "safe_missing_agent_response_refresh_failed"
        }
    };
    match crate::ipc_socket::send_refresh_content(
        project_root,
        &canonical.to_string_lossy(),
        refresh_content,
        &stale_hash,
        recovered.len(),
    ) {
        Ok(true) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_ack_mismatch_editor_refresh file={} transport=editor_ipc action={} stale_len={} stale_hash={} target_len={} target_hash={}",
                    file.display(),
                    action,
                    recovered.len(),
                    &stale_hash[..stale_hash.len().min(12)],
                    refresh_content.len(),
                    &target_hash[..target_hash.len().min(12)]
                ),
            );
            success_outcome
        }
        Ok(false) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_ack_mismatch_editor_refresh file={} transport=blocked reason={} no_ack=true action={} stale_len={} stale_hash={}",
                    file.display(),
                    failure_reason,
                    failure_action,
                    recovered.len(),
                    &stale_hash[..stale_hash.len().min(12)]
                ),
            );
            AckMismatchRefreshOutcome::NoRecovery
        }
        Err(err) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_ack_mismatch_editor_refresh file={} transport=blocked reason={} send_failed=true error={} action={} stale_len={} stale_hash={}",
                    file.display(),
                    failure_reason,
                    err,
                    failure_action,
                    recovered.len(),
                    &stale_hash[..stale_hash.len().min(12)]
                ),
            );
            AckMismatchRefreshOutcome::NoRecovery
        }
    }
}

pub(crate) fn live_buffer_delivery_missing_operator_text_authority_after_refresh(
    file: &Path,
    content: &str,
    source: &str,
) -> Option<agent_doc_debounce::LiveBufferSnapshot> {
    let canonical_file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let indicator_path = canonical_file.to_string_lossy().to_string();
    let missing = agent_doc_debounce::live_buffer_delivery_missing_operator_text_authority(
        &indicator_path,
        content,
    )?;
    let project_root = resolve_ipc_project_root_pub(&canonical_file);
    if !crate::ipc_socket::is_listener_active(&project_root) {
        return match crate::ipc_socket::send_publish_live_buffer_file_signal(
            &project_root,
            &indicator_path,
        ) {
            Ok(true) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "{source}_editor_authority_refresh file={} transport=file_signal action=publish_live_buffer",
                        file.display()
                    ),
                );
                wait_for_operator_text_authority_refresh(&indicator_path, content, missing)
            }
            Ok(false) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "{source}_editor_authority_refresh file={} transport=blocked outcome=publish_live_buffer_file_signal_unavailable action=editor_reload_required",
                        file.display()
                    ),
                );
                Some(missing)
            }
            Err(err) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "{source}_editor_authority_refresh file={} transport=blocked outcome=publish_live_buffer_file_signal_failed error={} action=editor_reload_required",
                        file.display(),
                        err
                    ),
                );
                Some(missing)
            }
        };
    }

    match crate::ipc_socket::send_publish_live_buffer(&project_root, &indicator_path) {
        Ok(true) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_editor_authority_refresh file={} transport=editor_ipc action=publish_live_buffer",
                    file.display()
                ),
            );
            wait_for_operator_text_authority_refresh(&indicator_path, content, missing)
        }
        Ok(false) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_editor_authority_refresh file={} transport=blocked reason=publish_live_buffer_failed action=editor_reload_required",
                    file.display()
                ),
            );
            Some(missing)
        }
        Err(err) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_editor_authority_refresh file={} transport=blocked reason=publish_live_buffer_failed error={} action=editor_reload_required",
                    file.display(),
                    err
                ),
            );
            Some(missing)
        }
    }
}

fn wait_for_operator_text_authority_refresh(
    indicator_path: &str,
    content: &str,
    mut latest_missing: agent_doc_debounce::LiveBufferSnapshot,
) -> Option<agent_doc_debounce::LiveBufferSnapshot> {
    for _ in 0..20 {
        match agent_doc_debounce::live_buffer_delivery_missing_operator_text_authority(
            indicator_path,
            content,
        ) {
            Some(still_missing) => {
                latest_missing = still_missing;
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            None => return None,
        }
    }
    Some(latest_missing)
}

pub fn try_editor_converge(
    file: &Path,
    target: &str,
    current_content: &str,
    source: &str,
) -> Result<bool> {
    let canonical_file = file
        .canonicalize()
        .with_context(|| format!("{source}: failed to resolve {}", file.display()))?;
    let project_root = resolve_ipc_project_root_pub(&canonical_file);
    // `#fcc0e`: integrate the converger with the `#ipcdrift` degraded-latch
    // circuit breaker. A session whose socket listener latched degraded
    // (repeated ack timeouts) may skip the socket, but must still prefer the
    // plugin-owned file-IPC queue before refusing the write. The latch self-heals
    // (`#ipc-degrade-self-heal`):
    // `ipc_direct_disk_degraded` re-probes listener liveness and clears the
    // marker the moment the socket recovers.
    cleanup_legacy_ipc_degraded(&project_root);
    if current_content == target {
        crate::ops_log::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=already_current",
                file.display()
            ),
        );
        return Ok(true);
    }
    if let Some(snapshot) = live_buffer_delivery_missing_operator_text_authority_after_refresh(
        &canonical_file,
        current_content,
        source,
    ) {
        let editor_id = snapshot.editor_id.as_deref().unwrap_or("unknown");
        crate::ops_log::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=blocked reason=editor_capability_missing capability={} editor_id={} live_len={} live_hash={} action=editor_reload_required",
                file.display(),
                agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
                editor_id,
                snapshot.len,
                snapshot.hash
            ),
        );
        anyhow::bail!(
            "{source}: refused editor convergence for {} because live editor buffer {} lacks required capability {}",
            file.display(),
            editor_id,
            agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY
        );
    }
    match ipc_direct_disk_degraded(&project_root, file) {
        Ok(true) => {
            log_ipc_dewedge_prefer_file_ipc(file, source);
            let canonical = file.canonicalize()?;
            let patch_id = uuid::Uuid::new_v4().to_string();
            let Some(mut payload) =
                editor_convergence_payload(&canonical, target, current_content, source, &patch_id)?
            else {
                if try_detached_disk_write(
                    file,
                    current_content,
                    target,
                    source,
                    "listener_degraded_no_component_delta",
                )? {
                    return Ok(true);
                }
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} transport=blocked degraded_cause=no_component_delta action=refuse_external_disk_write",
                        file.display()
                    ),
                );
                anyhow::bail!(
                    "{source}: refused direct disk write for {} while editor IPC listener is degraded (cause=no_component_delta)",
                    file.display()
                );
            };
            target_payload_to_live_editor(file, &mut payload, "file_ipc_convergence");
            if try_editor_converge_file_ipc(
                file,
                &project_root,
                &payload,
                &patch_id,
                target,
                source,
                "listener_degraded",
            )? {
                return Ok(true);
            }
            if try_detached_disk_write(
                file,
                current_content,
                target,
                source,
                "listener_degraded_editor_detached",
            )? {
                return Ok(true);
            }
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_writeback file={} transport=blocked degraded_cause=listener_degraded action=refuse_external_disk_write",
                    file.display()
                ),
            );
            anyhow::bail!(
                "{source}: refused direct disk write for {} while editor IPC listener is degraded",
                file.display()
            );
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!(
                "[write] WARNING: {source} converge degradation check failed (non-fatal): {e}"
            );
        }
    }
    if !crate::ipc_socket::is_listener_active(&project_root) {
        if try_detached_disk_write(file, current_content, target, source, "no_listener")? {
            return Ok(true);
        }
        return refuse_unproven_editor_delivery(file, source, "no_listener", None);
    }

    let canonical = canonical_file;
    let patch_id = uuid::Uuid::new_v4().to_string();
    let Some(mut payload) =
        editor_convergence_payload(&canonical, target, current_content, source, &patch_id)?
    else {
        if try_detached_disk_write(file, current_content, target, source, "no_component_delta")? {
            return Ok(true);
        }
        crate::ops_log::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=blocked reason=no_component_delta action=refuse_external_disk_write",
                file.display()
            ),
        );
        anyhow::bail!(
            "{source}: refused direct disk write for {} while editor IPC listener is active (reason=no_component_delta)",
            file.display()
        );
    };
    target_payload_to_live_editor(file, &mut payload, "editor_convergence");

    crate::ops_log::log_op(
        file,
        &format!(
            "{source}_editor_convergence_attempt file={} patch_id={} patches={} node_patches={} frontmatter={}",
            file.display(),
            patch_id,
            payload
                .get("patches")
                .and_then(|value| value.as_array())
                .map(Vec::len)
                .unwrap_or(0),
            payload
                .get("node_patches")
                .and_then(|value| value.as_array())
                .map(Vec::len)
                .unwrap_or(0),
            payload.get("frontmatter").is_some(),
        ),
    );

    match crate::ipc_socket::send_message(&project_root, &payload) {
        Ok(Some(_ack)) => {
            let sidecar = poll_ack_content_sidecar(
                &project_root,
                &patch_id,
                std::time::Duration::from_millis(500),
                std::time::Duration::from_millis(25),
            )?;
            let Some(recovered) = sidecar else {
                // `#6b5h`: ack received but no content sidecar proves application —
                // fail closed instead of routing a sync wait failure to disk.
                return refuse_unproven_editor_delivery(
                    file,
                    source,
                    "no_ack_content",
                    Some(&patch_id),
                );
            };
            if agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                &recovered,
            ) == agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                target,
            ) {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} patch_id={} recovered_len={} transport=editor_ipc",
                        file.display(),
                        patch_id,
                        recovered.len()
                    ),
                );
                // `#fcc0e`: a confirmed editor convergence proves the socket
                // listener is live; clear any accrued ack-timeout votes (the
                // degraded latch itself only clears on the liveness re-probe).
                if let Err(e) = clear_ipc_socket_ack_timeouts(&project_root, file, source) {
                    eprintln!(
                        "[write] WARNING: {source} converge ack-timeout clear failed (non-fatal): {e}"
                    );
                }
                Ok(true)
            } else if convergence_recovered_editor_wins_for_payload(&recovered, target, &payload) {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} patch_id={} recovered_len={} target_len={} transport=editor_ipc resolution=editor_wins_outside_touched_components",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
                    ),
                );
                // The ACK sidecar proves the live editor buffer contains the
                // authored payload effects. Keep its concurrent non-strict
                // component edits rather than forcing an exact target replay.
                if let Err(e) = clear_ipc_socket_ack_timeouts(&project_root, file, source) {
                    eprintln!(
                        "[write] WARNING: {source} converge ack-timeout clear failed (non-fatal): {e}"
                    );
                }
                Ok(true)
            } else {
                let recovery = refresh_editor_after_ack_mismatch(
                    file,
                    &project_root,
                    &canonical,
                    target,
                    &recovered,
                    current_content,
                    source,
                );
                if recovery == AckMismatchRefreshOutcome::ReplayedTarget {
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "{source}_writeback file={} patch_id={} recovered_len={} target_len={} transport=editor_ipc recovery=ack_mismatch_replayed_target",
                            file.display(),
                            patch_id,
                            recovered.len(),
                            target.len()
                        ),
                    );
                    if let Err(e) = clear_ipc_socket_ack_timeouts(&project_root, file, source) {
                        eprintln!(
                            "[write] WARNING: {source} converge ack-timeout clear failed (non-fatal): {e}"
                        );
                    }
                    return Ok(true);
                }
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} patch_id={} transport=blocked reason=ack_mismatch recovered_len={} target_len={} action=editor_convergence_required",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
                    ),
                );
                // The ACK came back but content drifted. This is unproven editor
                // convergence, not authorization to write through disk.
                refuse_unproven_editor_delivery(file, source, "ack_mismatch", Some(&patch_id))
            }
        }
        Ok(None) => {
            if try_detached_disk_write(file, current_content, target, source, "no_ack")? {
                return Ok(true);
            }
            // Missing ACK against a live editor marks the editor path stale; it
            // must not trigger a direct disk fallback.
            refuse_unproven_editor_delivery(file, source, "no_ack", Some(&patch_id))
        }
        Err(err) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "{source}_writeback file={} reason=send_failed error={} note=converge_send_error",
                    file.display(),
                    err
                ),
            );
            // A terminal `status:error` means the socket listener received the
            // patch but its socket-side apply path rejected it. That is still not
            // permission to raw-write the file, but the plugin-owned file-IPC
            // watcher may be able to apply the exact same patch and prove the
            // resulting buffer through ack-content in this same cycle.
            if is_socket_status_error(err.to_string())
                && try_editor_converge_file_ipc(
                    file,
                    &project_root,
                    &payload,
                    &patch_id,
                    target,
                    source,
                    "socket_status_error",
                )?
            {
                return Ok(true);
            }
            // `#fcc0e`: feed the de-wedge circuit breaker — a socket ack timeout
            // here counts toward the latch so a repeatedly-wedged listener trips
            // degraded and subsequent converges skip the doomed socket up front.
            // (Recovery targets a live editor; an editor-less session disk-falls
            // back below, but recording the socket failure is still harmless.)
            if is_socket_ack_timeout_error(err.to_string()) {
                match record_ipc_socket_ack_timeout(&project_root, file, Some(&patch_id), source) {
                    Ok(true) => {
                        eprintln!(
                            "[write] IPC listener degraded for {} after repeated {source} ack timeouts",
                            file.display()
                        );
                        // `#supselfheal` Phase 2: the latch just tripped — the editor
                        // write is wedged against a nominally-active listener. Record
                        // that the wedge is now a supervisor-recycle request (the
                        // route-owned supervisor reads the latched marker via
                        // `editor_ipc_write_wedged` and recycles a stale binary)
                        // instead of looping silent refusals.
                        log_write_wedge_requests_supervisor_recycle(file, source);
                    }
                    Ok(false) => {}
                    Err(e) => eprintln!(
                        "[write] WARNING: {source} converge ack-timeout record failed (non-fatal): {e}"
                    ),
                }
            }
            if try_detached_disk_write(file, current_content, target, source, "send_failed")? {
                return Ok(true);
            }
            // Send failure against a live editor marks the editor path stale; it
            // must not trigger a direct disk fallback.
            refuse_unproven_editor_delivery(file, source, "send_failed", Some(&patch_id))
        }
    }
}

fn try_editor_converge_file_ipc(
    file: &Path,
    project_root: &Path,
    payload: &serde_json::Value,
    patch_id: &str,
    target: &str,
    source: &str,
    reason: &str,
) -> Result<bool> {
    let patches_dir = project_root.join(".agent-doc/patches");
    if !patches_dir.exists() {
        crate::ops_log::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=blocked degraded_cause={reason}_no_file_ipc action=refuse_external_disk_write",
                file.display()
            ),
        );
        return Ok(false);
    }
    let patch_file = patches_dir.join(format!("{patch_id}.json"));
    let patch_count = payload
        .get("patches")
        .and_then(|value| value.as_array())
        .map(Vec::len)
        .unwrap_or(0)
        + payload
            .get("node_patches")
            .and_then(|value| value.as_array())
            .map(Vec::len)
            .unwrap_or(0)
        + usize::from(payload.get("frontmatter").is_some());
    crate::ops_log::log_op(
        file,
        &format!(
            "{source}_file_ipc_convergence_attempt file={} patch_id={} degraded_cause={} patches={}",
            file.display(),
            patch_id,
            reason,
            patch_count
        ),
    );
    if write_ipc_and_poll(
        &patch_file,
        payload,
        file,
        patch_count,
        IpcPollOptions::convergence(project_root, Some(target)),
    )? {
        crate::ops_log::log_op(
            file,
            &format!(
                "{source}_writeback file={} patch_id={} transport=file_ipc degraded_cause={}",
                file.display(),
                patch_id,
                reason
            ),
        );
        return Ok(true);
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "{source}_writeback file={} patch_id={} transport=blocked degraded_cause={reason}_file_ipc_unproven action=refuse_external_disk_write",
            file.display(),
            patch_id
        ),
    );
    Ok(false)
}

pub(crate) fn editor_convergence_payload(
    canonical_file: &Path,
    target: &str,
    current_content: &str,
    source: &str,
    patch_id: &str,
) -> Result<Option<serde_json::Value>> {
    let mut patches =
        agent_doc_document::component_patches::component_replace_patches(current_content, target)?;
    let frontmatter = live_prompt_drift_convergence_frontmatter(current_content, target);
    let node_patches = queue_consume_node_patches(current_content, target, source);

    if !node_patches.is_empty() {
        let node_patched_components = node_patches
            .iter()
            .filter_map(|patch| patch.get("component").and_then(|value| value.as_str()))
            .map(str::to_string)
            .collect::<HashSet<_>>();
        patches.retain(|patch| {
            patch
                .get("component")
                .and_then(|value| value.as_str())
                .is_none_or(|component| !node_patched_components.contains(component))
        });
    }

    if patches.is_empty() && node_patches.is_empty() && frontmatter.is_none() {
        return Ok(None);
    }

    let normalized_baseline =
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
            current_content,
        );
    let mut payload = serde_json::json!({
        "type": "patch",
        "file": canonical_file.to_string_lossy(),
        "patches": patches,
        "node_patches": node_patches,
        "unmatched": "",
        "baseline": current_content,
        "baseline_hash": agent_doc_hash::content_hash(current_content),
        "baseline_normalized_hash": agent_doc_hash::content_hash(&normalized_baseline),
        "reposition_boundary": false,
        "patch_id": patch_id,
    });
    if let Some(frontmatter) = frontmatter {
        payload["frontmatter"] = serde_json::Value::String(frontmatter);
    }
    Ok(Some(payload))
}

fn queue_consume_node_patches(
    current_content: &str,
    target: &str,
    source: &str,
) -> Vec<serde_json::Value> {
    if source != "queue_consume" {
        return Vec::new();
    }
    build_ipc_node_patches_json(Some(current_content), Some(target))
        .into_iter()
        .filter(|patch| patch.get("component").and_then(|value| value.as_str()) == Some("queue"))
        .collect()
}

/// `#fcc0`: the single converge-or-disk gate every document-mutating write site
/// routes through. When a JB editor listener is active it converges `target`
/// through the editor IPC (component `op:replace` — no `File Cache Conflict`
/// dialog). If editor convergence is unavailable or unproven, the write fails
/// closed instead of falling back to disk. `current` is the expected current
/// document content (held under the caller's doc lock) and drives the editor
/// delta.
pub fn converge_document_or_disk(
    file: &Path,
    target: &str,
    current: &str,
    source: &str,
) -> Result<()> {
    if try_editor_converge(file, target, current, source)? {
        return Ok(());
    }
    anyhow::bail!(
        "{source}: refused direct disk write for {} because editor convergence did not complete",
        file.display()
    )
}

/// `#fcc0`: converge-only gate for the component-mutating CLI write
/// sites that historically wrote straight to disk with a bare `std::fs::write`
/// (the `agent:pending` / `agent:review` operator ops, `dedupe`, preflight
/// `run_pending_maintenance`, the `agent_doc_pipeline:` frontmatter mirror). When
/// a JB editor listener is active it converges `target` through the editor IPC
/// (component/frontmatter `op:replace` — no `File Cache Conflict` dialog). If
/// editor convergence is unavailable or unproven, the write fails closed instead
/// of falling back to the historical plain disk write. `current` is the expected
/// current on-disk content the editor delta is computed against; `source` labels
/// the `ops.log` `<source>_writeback` line.
pub fn converge_or_disk_write(
    file: &Path,
    current: &str,
    target: &str,
    source: &str,
) -> Result<()> {
    if try_editor_converge(file, target, current, source)? {
        return Ok(());
    }
    anyhow::bail!(
        "{source}: refused direct disk write for {} because editor convergence did not complete",
        file.display()
    )
}

pub(crate) fn live_prompt_drift_convergence_frontmatter(
    file_content: &str,
    snapshot: &str,
) -> Option<String> {
    let file_frontmatter = agent_doc_frontmatter::frontmatter::raw_frontmatter_yaml(file_content);
    let snapshot_frontmatter = agent_doc_frontmatter::frontmatter::raw_frontmatter_yaml(snapshot)?;
    if file_frontmatter == Some(snapshot_frontmatter) {
        None
    } else {
        Some(snapshot_frontmatter.to_string())
    }
}

#[cfg(test)]
mod core_tests {
    #![allow(unused_imports)]
    use super::*;
    use fs2::FileExt;
    use std::fs;
    use std::fs::OpenOptions;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn stale_supervisor_write_short_circuit_passes_through_when_fresh() {
        // `#turnsaferecycle` Goal 3: with no stale-supervisor marker present (the
        // supervisor idle watch never ran in a unit context), the shared guard is a
        // no-op and returns None so the normal IPC write proceeds. This is what keeps
        // every existing write test unaffected by the guard.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        assert!(stale_supervisor_write_short_circuit(&file, "unit_test").is_none());
    }

    #[test]
    fn stale_supervisor_write_short_circuit_defers_when_marker_present() {
        // `#turnsaferecycle` Goal 3: when the idle-watch stale marker is present, the
        // shared guard short-circuits with the `deferred_for_recycle` user-facing
        // outcome (skip the doomed write, defer for the scheduled recycle).
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let canonical = file.canonicalize().unwrap();
        let base = resolve_ipc_project_root_pub(&canonical);
        agent_doc_turn_status_io::set_supervisor_stale_marker(&base, true).unwrap();

        let outcome = stale_supervisor_write_short_circuit(&file, "unit_test")
            .expect("stale marker must short-circuit the write");
        assert_eq!(
            outcome.outcome,
            agent_doc_flow::outcome::UserFacingOutcomeKind::DeferredForRecycle
        );

        agent_doc_turn_status_io::set_supervisor_stale_marker(&base, false).unwrap();
    }

    #[test]
    fn stale_ipc_drift_forces_pcp_recycle_only_when_stale_and_auto_recycle_on() {
        // `#turnsaferecycle` Goal 2: a proven stale-supervisor IPC drift with
        // auto-recycle ON schedules an immediate forced PCP recycle (RecycleNow);
        // opted-out auto-recycle stays advisory; a fresh supervisor never recycles.
        assert!(
            stale_ipc_drift_forces_pcp_recycle(true, true),
            "stale + auto-recycle must force RecycleNow"
        );
        assert!(
            !stale_ipc_drift_forces_pcp_recycle(true, false),
            "auto-recycle opted out must stay advisory (SurfaceStale)"
        );
        assert!(
            !stale_ipc_drift_forces_pcp_recycle(false, true),
            "a fresh supervisor is never a recycle candidate"
        );
    }

    fn doc_with_queue_and_exchange(queue_body: &str, response: &str) -> String {
        format!(
            "---\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange -->\n{response}\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n{queue_body}\n<!-- /agent:queue -->\n"
        )
    }

    fn queue_node_key_for_id(doc: &str, id: &str) -> String {
        agent_doc_markdown_ast::mutations::all_item_nodes(doc)
            .into_iter()
            .find(|node| node.component == "queue" && node.item.id == id)
            .map(|node| node.node_key)
            .unwrap_or_else(|| panic!("missing queue node id {id}"))
    }

    fn start_ack_mismatch_then_refresh_listener(
        project_root: &Path,
        ack_content: String,
    ) -> std::thread::JoinHandle<()> {
        let listener_root = project_root.to_path_buf();
        std::thread::spawn(move || {
            let root_clone = listener_root.clone();
            let _ = crate::ipc_socket::start_listener(&listener_root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let msg_type = v.get("type").and_then(|value| value.as_str()).unwrap_or("");
                let patch_id = v
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                let content = if msg_type == "refresh_content" {
                    v.get("content")
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                        .to_string()
                } else {
                    ack_content.clone()
                };
                if let Some(file_path) = v.get("file").and_then(|value| value.as_str()) {
                    let _ = std::fs::write(file_path, &content);
                }
                let ack_dir = root_clone.join(".agent-doc/ack-content");
                let _ = std::fs::create_dir_all(&ack_dir);
                let _ = std::fs::write(ack_dir.join(format!("{patch_id}.md")), &content);
                Some(serde_json::json!({"type": "ack", "id": patch_id}).to_string())
            });
        })
    }

    #[test]
    fn editor_ipc_write_wedged_reads_latched_degraded_marker() {
        // `#supselfheal` Phase 2: the supervisor-facing reader returns true once the
        // de-wedge latch has persisted `degraded` for the current session, and false
        // when there is no marker. Drive it through the real persistence path.
        let dir = TempDir::new().unwrap();
        let project_root = dir.path();
        let file = project_root.join("plan.md");
        fs::write(&file, "# plan\n").unwrap();
        // No marker yet → not wedged.
        assert!(!editor_ipc_write_wedged(project_root, &file));
        // Record ack timeouts up to the latch threshold → degraded persisted.
        for _ in 0..IPC_DEWEDGE_TIMEOUT_THRESHOLD {
            record_ipc_socket_ack_timeout(project_root, &file, Some("p1"), "finalize").unwrap();
        }
        assert!(
            editor_ipc_write_wedged(project_root, &file),
            "a latched degraded marker should read as a write wedge"
        );
    }

    #[test]
    fn live_prompt_drift_auto_recovery_safe_accepts_benign_wedge() {
        // Snapshot owns the response the fragmented disk file lost; no disk-only
        // user prompt → safe to auto-recover.
        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline();
        assert!(
            live_prompt_drift_auto_recovery_safe(
                &snapshot,
                &fragmented,
                normalize_visible_recovery_compare,
            ),
            "benign live-prompt-drift wedge should be recoverable"
        );
    }
    #[test]
    fn live_prompt_drift_response_patches_ignore_operator_owned_components() {
        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- existing backlog text\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- existing backlog text with operator word\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_baseline()
        );

        let generic = agent_doc_document::component_patches::component_replace_patches(
            &fragmented,
            &snapshot,
        )
        .unwrap();
        let generic_components: Vec<&str> = generic
            .iter()
            .filter_map(|patch| patch.get("component").and_then(|value| value.as_str()))
            .collect();
        assert!(
            generic_components.contains(&"exchange") && generic_components.contains(&"backlog"),
            "generic convergence should notice both component deltas: {generic:?}"
        );

        let response_only = live_prompt_drift_response_patches(&fragmented, &snapshot).unwrap();
        assert_eq!(
            response_only.len(),
            1,
            "live drift recovery only owns exchange"
        );
        assert_eq!(response_only[0]["component"], "exchange");
    }

    #[test]
    fn try_compact_editor_converge_writes_detached_disk_without_listener() {
        // Detached realtime: with no live editor listener and no live editor
        // sidecar, the current file is authoritative and the converger may use a
        // guarded direct disk write. This is not a snapshot fallback.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let current = crate::test_support::drift_baseline();
        let compacted = crate::test_support::drift_content_ours();
        std::fs::write(&doc, &current).unwrap();

        let converged = try_editor_converge(&doc, &compacted, &current, "compact").unwrap();
        assert!(converged, "detached compact should write the target");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            compacted,
            "no-listener compact convergence should write the compacted target"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("compact_writeback")
                && log.contains("transport=disk_detached")
                && log.contains("reason=no_listener"),
            "no-listener compact must record a detached disk writeback:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "no-listener compact must not advertise disk fallback:\n{log}"
        );
    }
    #[test]
    fn try_compact_editor_converge_converges_via_editor_ipc_with_listener() {
        // `#jbcompactcrdt`/`#w42v`: with a live JB IPC listener, compaction must
        // converge the compacted document through the editor (`transport=editor_ipc`)
        // instead of a direct disk write that diverges from the open buffer and
        // raises a `File Cache Conflict`.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::compact_convergence_source();
        let compacted = crate::test_support::compact_convergence_compacted();
        fs::write(&doc, &source).unwrap();

        // The fake editor acks with the compacted content, mirroring a JB plugin
        // that applied the exchange `op:replace` and converged its buffer.
        let _listener = crate::test_support::start_live_prompt_drift_ack_listener(
            dir.path(),
            compacted.clone(),
        );
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let converged = try_editor_converge(&doc, &compacted, &source, "compact").unwrap();
        assert!(
            converged,
            "an active JB IPC listener that converges the buffer must report editor_ipc transport"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("compact_editor_convergence_attempt"),
            "compact convergence attempt should be observable in ops.log:\n{log}"
        );
        assert!(
            log.contains("compact_writeback") && log.contains("transport=editor_ipc"),
            "successful compaction must record the editor_ipc writeback transport:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "a converged compaction must not also take the disk fallback:\n{log}"
        );
    }
    /// Pre-consume document with a `go` queue head an operator could be concurrently
    /// editing while the queue is struck.
    /// Post-consume document: only the `queue` head is struck; every other
    /// component is byte-identical (queue consume never touches the exchange).
    #[test]
    fn queue_consume_writeback_converges_via_editor_ipc_with_listener() {
        // `#fcc0`: the queue-consume write must route through the shared
        // converger so an active JB listener converges the struck queue through
        // the editor (`transport=editor_ipc`, `queue_consume`-labelled) instead of
        // a direct disk write that raises a `File Cache Conflict`.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let _listener =
            crate::test_support::start_live_prompt_drift_ack_listener(dir.path(), target.clone());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let converged = try_editor_converge(&doc, &target, &source, "queue_consume").unwrap();
        assert!(
            converged,
            "an active JB IPC listener that converges the buffer must report editor_ipc transport"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_editor_convergence_attempt"),
            "queue-consume convergence attempt should be source-labelled in ops.log:\n{log}"
        );
        assert!(
            log.contains("queue_consume_writeback") && log.contains("transport=editor_ipc"),
            "a converged queue consume must record the editor_ipc writeback transport:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "a converged queue consume must not also take the disk fallback:\n{log}"
        );
    }

    #[test]
    fn queue_consume_socket_status_error_falls_back_to_proven_file_ipc() {
        // A live editor socket can accept a patch, emit the early pending ack,
        // then reject the terminal apply (`status:error`) because the editor is
        // busy or the socket-side apply path lost its generation race. That must
        // not authorize a raw disk write, but it should try the plugin-owned
        // file-IPC queue in the same cycle and accept it only with ack-content.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            &source,
            "jetbrains-test-editor",
            "jetbrains",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let listener_root = dir.path().to_path_buf();
        let _listener = std::thread::spawn(move || {
            let _ = crate::ipc_socket::start_listener(&listener_root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = v
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                Some(
                    serde_json::json!({
                        "type": "ack",
                        "id": patch_id,
                        "status": "error",
                        "reason": "socket_apply_failed"
                    })
                    .to_string(),
                )
            });
        });
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let watcher_dir = agent_doc_dir.join("patches");
        let watcher_ack_dir = agent_doc_dir.join("ack-content");
        let watcher_doc = doc.clone();
        let watcher_doc_str = doc_str.clone();
        let watcher_target = target.clone();
        let watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.extension().is_some_and(|e| e == "json") {
                        continue;
                    }
                    let payload_text = fs::read_to_string(&path).unwrap();
                    let payload: serde_json::Value = serde_json::from_str(&payload_text).unwrap();
                    let patch_id = payload
                        .get("patch_id")
                        .and_then(|value| value.as_str())
                        .unwrap()
                        .to_string();
                    fs::write(&watcher_doc, &watcher_target).unwrap();
                    agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
                        &watcher_doc_str,
                        &watcher_target,
                        "jetbrains-test-editor",
                        "jetbrains",
                        "test",
                        &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
                    )
                    .unwrap();
                    fs::write(
                        watcher_ack_dir.join(format!("{patch_id}.md")),
                        &watcher_target,
                    )
                    .unwrap();
                    fs::remove_file(path).unwrap();
                    return true;
                }
            }
            false
        });

        let converged = try_editor_converge(&doc, &target, &source, "queue_consume").unwrap();
        assert!(
            converged,
            "socket status:error should retry through proven file IPC before failing closed"
        );
        assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("send_failed")
                && log.contains("IPC ack status error"),
            "socket status error should remain auditable:\n{log}"
        );
        assert!(
            log.contains("queue_consume_file_ipc_convergence_attempt")
                && log.contains("degraded_cause=socket_status_error")
                && log.contains("transport=file_ipc"),
            "socket status error should fall back to proven file IPC:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "socket status-error fallback must not raw-write behind the plugin:\n{log}"
        );
        assert_eq!(fs::read_to_string(&doc).unwrap(), target);
    }

    #[test]
    fn queue_consume_ack_mismatch_refreshes_editor_back_to_preconsume() {
        // `#fcc0-ack-mismatch`: when the editor acks with content that does not
        // match the target, the disk write must still fail closed. The previous
        // behavior left that untrusted ACK content in the live editor buffer, so a
        // later flush could persist a stale queue strike. Refresh it back to the
        // pre-consume document using the ACK content as the stale hash guard.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        let stale_ack = target.replace(
            "<!-- /agent:exchange -->",
            "> **Queue prompt:** stale leftover from failed queue consume\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, &source).unwrap();

        let root = dir.path().to_path_buf();
        let _listener = start_ack_mismatch_then_refresh_listener(&root, stale_ack);
        crate::test_support::wait_for_live_prompt_drift_listener(&root);
        crate::test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("ack_mismatch"),
            "ACK mismatch should still fail closed: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "untrusted ACK content should be refreshed back to the pre-consume editor buffer"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=blocked")
                && log.contains("ack_mismatch"),
            "ACK mismatch must remain a blocked writeback:\n{log}"
        );
        assert!(
            log.contains("queue_consume_ack_mismatch_editor_refresh")
                && log.contains("action=revert_untrusted_ack_content"),
            "ACK mismatch should refresh the editor back to the pre-consume buffer:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "ACK mismatch must not take the disk fallback:\n{log}"
        );
    }

    #[test]
    fn queue_consume_ack_accepts_node_patch_with_editor_owned_queue_addition() {
        // `#qpcwcmerge`: queue consume owns the exact node-keyed strike, not the
        // whole live queue component. If the editor ACK proves that strike landed
        // while also carrying a concurrent operator queue addition, accept the
        // ACK-visible buffer instead of replaying or rejecting it.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        let recovered = target.replace(
            "<!-- /agent:queue -->",
            "- do [#qftlossdelta]\n<!-- /agent:queue -->",
        );
        fs::write(&doc, &source).unwrap();

        let root = dir.path().to_path_buf();
        let _listener = start_ack_mismatch_then_refresh_listener(&root, recovered.clone());
        crate::test_support::wait_for_live_prompt_drift_listener(&root);
        crate::test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .expect("queue consume should accept proven node patch plus editor-owned queue edits");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            recovered,
            "the ACK-visible live buffer should remain authoritative"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=editor_ipc")
                && log.contains("resolution=editor_wins_outside_touched_components"),
            "queue consume should accept the editor-owned queue addition:\n{log}"
        );
        assert!(
            !log.contains("ack_mismatch")
                && !log.contains("editor_convergence_required")
                && !log.contains("transport=disk_fallback"),
            "proven editor-owned queue drift must not be treated as a failed convergence:\n{log}"
        );
    }

    #[test]
    fn pending_write_shorter_ack_replays_missing_agent_response() {
        // `#ack-shorter-replay`: a plugin ACK that proves every non-exchange
        // component but is missing the newly materialized `### Re:` block is not
        // user drift. Refresh the editor to the target response and treat the
        // write as converged instead of leaving the cycle interrupted.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = doc_with_queue_and_exchange("- do [#head]\n", "");
        let target = doc_with_queue_and_exchange(
            "- do [#head]\n",
            "### Re: do [#head]\n\nAnswered from the agent.\n",
        );
        let shorter_ack = source.clone();
        assert!(
            shorter_ack.len() < target.len(),
            "test setup should model the shorter recovered ack"
        );
        fs::write(&doc, &source).unwrap();

        let root = dir.path().to_path_buf();
        let _listener = start_ack_mismatch_then_refresh_listener(&root, shorter_ack);
        crate::test_support::wait_for_live_prompt_drift_listener(&root);
        crate::test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        converge_document_or_disk(&doc, &target, &source, "pending_write")
            .expect("safe shorter ack should replay the target response through the editor");

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            target,
            "safe shorter ack should leave the editor/disk at the target response"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("pending_write_ack_mismatch_editor_refresh")
                && log.contains("action=replay_missing_agent_response"),
            "shorter ack should refresh the editor to the target response:\n{log}"
        );
        assert!(
            log.contains("pending_write_writeback")
                && log.contains("transport=editor_ipc")
                && log.contains("recovery=ack_mismatch_replayed_target"),
            "shorter ack recovery should be recorded as successful editor convergence:\n{log}"
        );
        assert!(
            !log.contains("action=refuse_external_disk_write"),
            "safe shorter ack must not be recorded as a refused external disk write:\n{log}"
        );
    }

    #[test]
    fn queue_consume_ack_mismatch_does_not_refresh_user_prompt_drift() {
        // If the ACK content carries a genuine concurrent editor prompt, the
        // binary must still refuse the disk write but must not refresh the editor
        // back to the pre-consume document, because that would drop user work.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        let user_ack = target.replace(
            "<!-- /agent:exchange -->",
            "❯ do [#followup] preserve this concurrent prompt\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, &source).unwrap();

        let root = dir.path().to_path_buf();
        let _listener = start_ack_mismatch_then_refresh_listener(&root, user_ack.clone());
        crate::test_support::wait_for_live_prompt_drift_listener(&root);
        crate::test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("ack_mismatch"),
            "ACK mismatch should still fail closed: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            user_ack,
            "user prompt drift must remain editor-owned instead of being refreshed away"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_ack_mismatch_editor_refresh")
                && log.contains("untrusted_ack_content_contains_user_drift")
                && log.contains("action=leave_editor_owned_ack_content"),
            "user drift should block the refresh path:\n{log}"
        );
        assert!(
            !log.contains("action=revert_untrusted_ack_content"),
            "user drift must not be reverted:\n{log}"
        );
    }

    #[test]
    fn queue_consume_editor_convergence_payload_is_node_keyed_and_fenced() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");
        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let payload = editor_convergence_payload(
            &doc.canonicalize().unwrap(),
            &target,
            &source,
            "queue_consume",
            "patch-queue-consume",
        )
        .unwrap()
        .expect("queue consume should produce an editor convergence payload");

        assert_eq!(
            payload["baseline_hash"].as_str(),
            Some(agent_doc_hash::content_hash(&source).as_str()),
            "socket convergence payloads must carry the raw generation fence"
        );
        assert_eq!(
            payload["baseline_normalized_hash"].as_str(),
            Some(
                agent_doc_hash::content_hash(
                    &agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                        &source
                    )
                )
                .as_str()
            ),
            "socket convergence payloads must also carry the transient-marker-normalized fence"
        );
        assert!(
            payload["patches"]
                .as_array()
                .unwrap()
                .iter()
                .all(|patch| patch["component"] != "queue"),
            "queue consume must not send a broad legacy queue component replace: {payload:?}"
        );
        let node_patches = payload["node_patches"].as_array().unwrap();
        assert!(
            node_patches
                .iter()
                .any(|patch| { patch["component"] == "queue" && patch["op"] == "strike" }),
            "queue consume must be expressed as an exact node-keyed strike: {payload:?}"
        );
    }
    #[test]
    fn try_editor_converge_treats_active_listener_already_current_as_noop() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        fs::write(&doc, &source).unwrap();

        let _listener = crate::test_support::start_ack_without_content_listener(dir.path());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let converged = try_editor_converge(&doc, &source, &source, "pending_write").unwrap();
        assert!(
            converged,
            "already-current active-listener converge should be a no-op success"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "already-current converge must not mutate the document"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("pending_write_writeback") && log.contains("transport=already_current"),
            "already-current converge should be observable without disk fallback:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback") && !log.contains("transport=blocked"),
            "already-current converge must not fall back or block:\n{log}"
        );
    }
    #[test]
    fn converge_document_or_disk_writes_detached_disk_without_listener() {
        // Detached realtime: with no live editor listener and no live editor
        // sidecar, the current file is authoritative and the shared converger
        // may use a guarded direct disk write.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .expect("detached queue consume should write the target");

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            target,
            "with no listener the converger should write the target to disk"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=disk_detached")
                && log.contains("reason=no_listener"),
            "a no-listener queue consume must record the source-labelled detached writeback:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "no-listener queue consume must not record disk fallback:\n{log}"
        );
    }

    #[test]
    fn converge_document_or_disk_blocks_diverged_under_capable_live_buffer_before_ipc() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &doc_str,
            &format!("{source}\noperator typed text\n"),
            Some("jetbrains-old"),
        )
        .unwrap();

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("operator_text_authority_v1"),
            "under-capable editor sidecar must block with the missing capability: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "under-capable editor sidecar must not let the converger mutate disk"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("reason=editor_capability_missing")
                && log.contains("capability=operator_text_authority_v1")
                && log.contains("editor_id=jetbrains-old")
                && !log.contains("queue_consume_editor_convergence_attempt"),
            "capability guard must fire before IPC attempt:\n{log}"
        );
    }

    #[test]
    fn converge_document_or_disk_blocks_matching_under_capable_live_buffer_before_ipc() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &doc_str,
            &source,
            Some("jetbrains-old"),
        )
        .unwrap();

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("operator_text_authority_v1"),
            "matching under-capable editor sidecar must block delivery too: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "matching under-capable editor sidecar must not let the converger mutate disk"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("reason=editor_capability_missing")
                && log.contains("capability=operator_text_authority_v1")
                && log.contains("editor_id=jetbrains-old")
                && !log.contains("queue_consume_editor_convergence_attempt"),
            "delivery capability guard must fire before IPC attempt:\n{log}"
        );
    }

    #[test]
    fn capability_guard_refreshes_live_buffer_sidecar_over_editor_ipc() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &doc_str,
            &source,
            Some("jetbrains-old"),
        )
        .unwrap();

        let captured = std::sync::Arc::new(std::sync::Mutex::new(None::<serde_json::Value>));
        let captured_clone = captured.clone();
        let listener_root = dir.path().to_path_buf();
        let doc_for_listener = doc_str.clone();
        let source_for_listener = source.clone();
        let server = std::thread::spawn(move || {
            let _ = crate::ipc_socket::start_listener(&listener_root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                *captured_clone.lock().unwrap() = Some(v.clone());
                if v.get("type").and_then(|value| value.as_str()) == Some("publish_live_buffer") {
                    let published =
                        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
                            &doc_for_listener,
                            &source_for_listener,
                            "jetbrains-old",
                            "jetbrains",
                            "test",
                            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
                        );
                    published.ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            });
        });
        std::thread::sleep(Duration::from_millis(120));

        let missing = live_buffer_delivery_missing_operator_text_authority_after_refresh(
            &doc,
            &source,
            "queue_consume",
        );
        assert!(
            missing.is_none(),
            "a capable editor refresh should clear the stale missing-authority sidecar"
        );
        let msg = captured
            .lock()
            .unwrap()
            .clone()
            .expect("listener saw publish_live_buffer");
        assert_eq!(msg["type"], "publish_live_buffer");
        assert_eq!(msg["file"], doc_str);

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_editor_authority_refresh")
                && log.contains("transport=editor_ipc")
                && log.contains("action=publish_live_buffer"),
            "authority refresh should be logged as read-only editor IPC:\n{log}"
        );

        let _ = std::fs::remove_file(crate::ipc_socket::socket_path(dir.path()));
        drop(server);
    }

    #[test]
    fn capability_guard_refreshes_live_buffer_sidecar_over_file_signal() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &doc_str,
            &source,
            Some("vscode-old"),
        )
        .unwrap();

        let captured = std::sync::Arc::new(std::sync::Mutex::new(None::<serde_json::Value>));
        let captured_clone = captured.clone();
        let signal_root = dir.path().to_path_buf();
        let doc_for_signal = doc_str.clone();
        let source_for_signal = source.clone();
        let signal_thread = std::thread::spawn(move || {
            let signal = signal_root
                .join(".agent-doc")
                .join("patches")
                .join("publish-live-buffer.signal");
            for _ in 0..100 {
                if let Ok(raw) = fs::read_to_string(&signal) {
                    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
                    *captured_clone.lock().unwrap() = Some(v.clone());
                    if v.get("type").and_then(|value| value.as_str()) == Some("publish_live_buffer")
                    {
                        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
                            &doc_for_signal,
                            &source_for_signal,
                            "vscode-old",
                            "vscode",
                            "test",
                            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
                        )
                        .unwrap();
                    }
                    let _ = fs::remove_file(&signal);
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("publish-live-buffer file signal was not written");
        });

        let missing = live_buffer_delivery_missing_operator_text_authority_after_refresh(
            &doc,
            &source,
            "queue_consume",
        );
        signal_thread.join().unwrap();
        assert!(
            missing.is_none(),
            "a capable file-signal refresh should clear the stale missing-authority sidecar"
        );
        let msg = captured
            .lock()
            .unwrap()
            .clone()
            .expect("file signal was captured");
        assert_eq!(msg["type"], "publish_live_buffer");
        assert_eq!(msg["file"], doc_str);
        assert!(
            msg.get("content").is_none() && msg.get("patches").is_none(),
            "publish-live-buffer signal must be read-only: {msg}"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_editor_authority_refresh")
                && log.contains("transport=file_signal")
                && log.contains("action=publish_live_buffer"),
            "authority refresh should be logged as read-only file signal IPC:\n{log}"
        );
    }

    #[test]
    fn converge_document_or_disk_blocks_detached_disk_with_capable_live_buffer() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            &format!("{source}\noperator typed text\n"),
            "jetbrains-new",
            "jetbrains",
            "0.2.197",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("no_listener"),
            "capable sidecar without listener should fail closed instead of detached disk: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "live editor sidecar must leave the on-disk document unchanged"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            !log.contains("reason=editor_capability_missing"),
            "capable sidecar must not trip the capability guard:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_detached"),
            "live editor sidecar must block detached disk write:\n{log}"
        );
    }

    #[test]
    fn converge_document_or_disk_route_source_writes_detached_disk_without_listener() {
        // `#fccroute`: the three route/dispatch session-document write sites
        // (`route_session_id`, `route_dedup_scrub`, `route_queue_activation`) now
        // route their disk writes through `converge_document_or_disk` so a live JB
        // editor converges them instead of hitting the File Cache Conflict dialog.
        // With no listener or live editor sidecar, detached realtime writes the
        // current file through the guarded disk path. Cover each route label so a
        // future regression on any one of them is caught.
        for source_label in [
            "route_session_id",
            "route_dedup_scrub",
            "route_queue_activation",
        ] {
            let dir = TempDir::new().unwrap();
            let agent_doc_dir = dir.path().join(".agent-doc");
            fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
            let doc = dir.path().join("plan.md");

            let source = crate::test_support::queue_consume_convergence_source();
            let target = crate::test_support::queue_consume_convergence_target();
            fs::write(&doc, &source).unwrap();

            converge_document_or_disk(&doc, &target, &source, source_label)
                .unwrap_or_else(|err| panic!("{source_label}: detached write failed: {err}"));

            assert_eq!(
                fs::read_to_string(&doc).unwrap(),
                target,
                "{source_label}: with no listener the converger must write the target"
            );
            let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
            assert!(
                log.contains(&format!("{source_label}_writeback"))
                    && log.contains("transport=disk_detached")
                    && log.contains("reason=no_listener"),
                "{source_label}: no-listener route write must record a source-labelled detached writeback:\n{log}"
            );
            assert!(
                !log.contains("transport=disk_fallback"),
                "{source_label}: no-listener route write must not record disk fallback:\n{log}"
            );
        }
    }
    #[test]
    fn converge_document_or_disk_blocks_disk_fallback_with_active_listener_without_ack_content() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let _listener = crate::test_support::start_ack_without_content_listener(dir.path());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());
        // `#6b5h`: a real editor is attached — seed a live plugin-owner lease so
        // the guard fails closed (protects the buffer) rather than treating the
        // ack-without-content listener as the editor-less CLI-only case.
        crate::test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("refused direct disk write"),
            "active listener without ack-content should block disk fallback: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "an unproven editor IPC apply must not be followed by an external disk write"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=blocked")
                && log.contains("reason=no_ack_content"),
            "active listener failure must be logged as a blocked disk write:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "active listener failure must not be logged as a disk fallback:\n{log}"
        );
    }
    #[test]
    fn converge_or_disk_write_writes_detached_disk_without_listener() {
        // Detached realtime: the converge-or-disk gate used by pending/review,
        // dedupe, preflight-maintenance, and pipeline-mirror write sites may
        // write disk directly only when no editor endpoint or live sidecar owns
        // the document.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        converge_or_disk_write(&doc, &source, &target, "pending_write")
            .expect("detached pending write should write the target");

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            target,
            "with no listener the converger must write the target to disk"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("pending_write_writeback")
                && log.contains("transport=disk_detached")
                && log.contains("reason=no_listener"),
            "a no-listener plain converge must record the source-labelled detached writeback:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "a no-listener plain converge must not record disk fallback:\n{log}"
        );
    }
    #[test]
    fn converge_or_disk_write_blocks_plain_disk_fallback_with_active_listener_without_ack_content()
    {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let _listener = crate::test_support::start_ack_without_content_listener(dir.path());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());
        // `#6b5h`: a real editor is attached — seed a live plugin-owner lease so
        // the guard fails closed on unproven delivery.
        crate::test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        let err = converge_or_disk_write(&doc, &source, &target, "pending_write")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("refused direct disk write"),
            "active listener without ack-content should block plain disk fallback: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "plain component maintenance must not write behind a running editor plugin"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("pending_write_writeback")
                && log.contains("transport=blocked")
                && log.contains("reason=no_ack_content"),
            "active listener failure must be logged as a blocked plain disk write:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "active listener failure must not be logged as a disk fallback:\n{log}"
        );
    }
    #[test]
    fn converge_document_or_disk_editorless_socket_blocks_without_ack_proof() {
        // `#6b5h` cutover: a pure-CLI session may see a connectable
        // controller-hosted socket with NO plugin editor behind it. An
        // ack-without-content listener still does not prove editor convergence, so
        // the realtime path fails closed instead of routing the write to disk.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");

        let source = crate::test_support::queue_consume_convergence_source();
        let target = crate::test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        let _listener = crate::test_support::start_ack_without_content_listener(dir.path());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());
        // No plugin-owner lease seeded → no live editor endpoint, but the
        // connectable socket still requires convergence proof.

        let err = converge_document_or_disk(&doc, &target, &source, "queue_consume")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("editor convergence is unproven"),
            "editorless socket without ack proof should fail closed: {err}"
        );

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "unproven editor convergence must not be followed by a disk write"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=blocked")
                && log.contains("reason=no_ack_content")
                && log.contains("editor_endpoint=absent")
                && log.contains("action=editor_convergence_required"),
            "editorless socket must record a fail-closed convergence requirement:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "editorless socket must not route missing ACK proof to disk fallback:\n{log}"
        );
    }
    #[test]
    fn live_prompt_drift_auto_recovery_safe_rejects_no_wedge() {
        // Snapshot == file: no wedge, nothing to recover, must not fire.
        let snapshot = crate::test_support::drift_content_ours();
        assert!(
            !live_prompt_drift_auto_recovery_safe(
                &snapshot,
                &snapshot,
                normalize_visible_recovery_compare,
            ),
            "no drift means no auto-recovery"
        );
    }
    #[test]
    fn live_prompt_drift_auto_recovery_safe_rejects_disk_only_exchange_prompt() {
        // The visible file carries a NEW user prompt the snapshot never saw —
        // adopting content_ours would silently drop it. Fail closed.
        let snapshot = crate::test_support::drift_content_ours();
        let mut fragmented = crate::test_support::drift_baseline();
        fragmented = fragmented.replace(
            "❯ do #fix\n<!-- /agent:exchange -->",
            "❯ do #fix\n❯ do #brand-new-user-prompt-typed-after-preflight\n<!-- /agent:exchange -->",
        );
        assert!(
            !live_prompt_drift_auto_recovery_safe(
                &snapshot,
                &fragmented,
                normalize_visible_recovery_compare,
            ),
            "a disk-only user prompt must block auto-recovery"
        );
    }
    #[test]
    fn live_prompt_drift_auto_recovery_preserves_disk_only_queue_item() {
        // A user-added `do [#id]` queue line is disjoint realtime state: the
        // response can land while the queue edit remains in the merged target.
        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline().replace(
            "- do [#fix]\n<!-- /agent:queue -->",
            "- do [#fix]\n- do [#user-added-queue-item]\n<!-- /agent:queue -->",
        );
        let target = live_prompt_drift_recovery_target(
            &snapshot,
            &fragmented,
            normalize_visible_recovery_compare,
        )
        .expect("queue edits should be preserved while the response lands");
        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- do [#user-added-queue-item]"));
    }

    #[test]
    fn live_prompt_drift_auto_recovery_preserves_partial_exchange_word() {
        // A raw word typed into the exchange after preflight is operator-visible
        // document text even when it is not yet a complete prompt. Recovery may
        // append the missing agent response, but it must not reset the exchange
        // back to the pre-typing snapshot.
        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline().replace(
            "❯ do #fix\n<!-- /agent:exchange -->",
            "❯ do #fix\noperator-partial-wo\n<!-- /agent:exchange -->",
        );

        let target = live_prompt_drift_recovery_target(
            &snapshot,
            &fragmented,
            normalize_visible_recovery_compare,
        )
        .expect("partial exchange text should be preserved while the response lands");
        assert!(target.contains("### Re: do #fix"));
        assert!(
            target.contains("operator-partial-wo"),
            "operator-typed partial word must survive recovery:\n{target}"
        );
    }

    #[test]
    fn live_prompt_drift_auto_recovery_preserves_disk_only_backlog_text() {
        // Ordinary operator text is just as authoritative as prompt-shaped text:
        // realtime recovery keeps it and adds only the missing response.
        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- existing backlog text\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- existing backlog text with operator word\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_baseline()
        );

        let target = live_prompt_drift_recovery_target(
            &snapshot,
            &fragmented,
            normalize_visible_recovery_compare,
        )
        .expect("backlog edits should be preserved while the response lands");
        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- existing backlog text with operator word"));
        assert!(!target.contains("- existing backlog text\n<!-- /agent:backlog -->"));
    }

    #[test]
    fn live_prompt_drift_auto_recovery_preserves_operator_deleted_backlog_text() {
        // Operator deletions are also authoritative. Recovery must not resurrect
        // a deleted backlog line while restoring the agent response.
        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- keep this\n- operator deleted this\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- keep this\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_baseline()
        );

        let target = live_prompt_drift_recovery_target(
            &snapshot,
            &fragmented,
            normalize_visible_recovery_compare,
        )
        .expect("backlog deletions should be preserved while the response lands");
        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- keep this"));
        assert!(!target.contains("operator deleted this"));
    }

    #[test]
    fn live_prompt_drift_auto_recovery_preserves_operator_edited_backlog_text() {
        // Same for edits/replacements: the file line is not a prompt, but the
        // operator-visible value must win over the older snapshot value.
        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- original backlog wording\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- edited backlog wording\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_baseline()
        );

        let target = live_prompt_drift_recovery_target(
            &snapshot,
            &fragmented,
            normalize_visible_recovery_compare,
        )
        .expect("backlog edits should be preserved while the response lands");
        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- edited backlog wording"));
        assert!(!target.contains("- original backlog wording"));
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_rebases_onto_post_preflight_response_block_deletion() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        let historical =
            "### Re: do #old — gpt-5\n\nHistorical answer the operator deleted after preflight.\n";
        let preflight = crate::test_support::drift_baseline().replace(
            "❯ do #fix\n",
            &format!("❯ do #old\n{historical}❯ do #fix\n"),
        );
        let snapshot = crate::test_support::drift_content_ours().replace(
            "❯ do #fix\n",
            &format!("❯ do #old\n{historical}❯ do #fix\n"),
        );
        let current = preflight.replace(historical, "");
        fs::write(&doc, &current).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        // Preflight observed the historical response. The operator deleted it
        // before auto-recovery ran, so recovery must not resurrect it while
        // trying to restore the new response.
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&preflight)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &current).unwrap();
        assert!(
            recovered.as_deref().is_some_and(|content| {
                content.contains("### Re: do #fix")
                    && !content.contains("Historical answer the operator deleted")
            }),
            "post-preflight response-block deletion should be preserved while the new response lands"
        );
        let disk = fs::read_to_string(&doc).unwrap();
        assert!(disk.contains("### Re: do #fix"));
        assert!(!disk.contains("Historical answer the operator deleted"));
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_advances_snapshot_to_operator_preserving_merge() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- original backlog wording\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- edited backlog wording\n<!-- /agent:backlog -->\n",
            crate::test_support::drift_baseline()
        );
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented)
            .unwrap()
            .expect("response should merge onto edited backlog");

        assert!(recovered.contains("### Re: do #fix"));
        assert!(recovered.contains("- edited backlog wording"));
        assert!(!recovered.contains("- original backlog wording"));
        assert_eq!(fs::read_to_string(&doc).unwrap(), recovered);
        assert_eq!(
            snapshot::load(&doc).unwrap().as_deref(),
            Some(recovered.as_str()),
            "snapshot must advance to the operator-preserving merged document"
        );
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_writes_realtime_merge_when_blocked_and_safe() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        // The drift guard fired this cycle and adopted content_ours.
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert_eq!(
            recovered.as_deref(),
            Some(snapshot.as_str()),
            "the no-operator-drift merge equals the candidate response snapshot"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            snapshot,
            "the working-tree file should now carry the full response"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("live_prompt_drift_auto_recovered"),
            "auto-recovery must leave an observable ops.log marker:\n{log}"
        );
    }
    #[test]
    fn try_auto_recover_live_prompt_drift_prefers_editor_ipc_when_listener_active() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let _listener =
            crate::test_support::start_live_prompt_drift_ack_listener(dir.path(), snapshot.clone());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert_eq!(
            recovered.as_deref(),
            Some(snapshot.as_str()),
            "recovery should accept the editor-applied snapshot"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            snapshot,
            "the fake editor listener should converge the working tree through IPC"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("[jbstalecache] editor_convergence_attempt")
                && log.contains("[jbstalecache] editor_convergence_succeeded"),
            "active listener recovery should be observable as editor convergence:\n{log}"
        );
        assert!(
            log.contains("live_prompt_drift_auto_recovered")
                && log.contains("transport=editor_ipc")
                && log.contains("ipc_listener_active=true"),
            "recovery marker should name the editor transport:\n{log}"
        );
        assert!(
            !log.contains("auto_recovery_disk_write_during_ipc_listener")
                && !log.contains("transport=disk_fallback"),
            "successful editor convergence must not take the stale-cache disk fallback:\n{log}"
        );
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_editor_ipc_preserves_partial_exchange_word() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline().replace(
            "❯ do #fix\n<!-- /agent:exchange -->",
            "❯ do #fix\noperator-partial-wo\n<!-- /agent:exchange -->",
        );
        let recovery_target = live_prompt_drift_recovery_target(
            &snapshot,
            &fragmented,
            normalize_visible_recovery_compare,
        )
        .expect("partial exchange text should be preserved in the target");
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let _listener = crate::test_support::start_live_prompt_drift_ack_listener(
            dir.path(),
            recovery_target.clone(),
        );
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert_eq!(
            recovered.as_deref(),
            Some(recovery_target.as_str()),
            "editor IPC recovery should accept the operator-preserving target"
        );
        let visible = fs::read_to_string(&doc).unwrap();
        assert!(
            visible.contains("operator-partial-wo") && visible.contains("### Re: do #fix"),
            "the fake editor listener must retain the partial word and land the response:\n{visible}"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("transport=editor_ipc")
                && log.contains("ipc_listener_active=true")
                && !log.contains("transport=disk_fallback"),
            "partial-word recovery must go through editor IPC without disk fallback:\n{log}"
        );
    }

    #[test]
    fn try_auto_recover_live_prompt_drift_blocks_disk_fallback_with_active_listener_without_ack_content()
     {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let _listener = crate::test_support::start_ack_without_content_listener(dir.path());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert!(
            recovered.is_none(),
            "active listener without ack-content must block binary-owned disk recovery"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            fragmented,
            "auto-recovery must not write the merged target behind the editor"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("[jbstalecache] editor_convergence_no_ack_content")
                && log.contains("action=block_external_disk_write"),
            "unproven editor convergence must be logged as a blocked write:\n{log}"
        );
        assert!(
            log.contains("[jbstalecache] auto_recovery_disk_write_blocked")
                && log.contains("reason=editor_ipc_unconfirmed"),
            "auto-recovery must record that it refused the disk fallback:\n{log}"
        );
        assert!(
            !log.contains("auto_recovery_disk_write_during_ipc_listener")
                && !log.contains("transport=disk_fallback"),
            "active listener recovery must not take or advertise the disk fallback:\n{log}"
        );
    }
    #[test]
    fn try_auto_recover_live_prompt_drift_skips_without_blocked_flag() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        // A cycle exists but the drift guard never fired (flag stays false) →
        // not the wedge we own.
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert!(
            recovered.is_none(),
            "without the drift flag this is not the auto-recovery case"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            fragmented,
            "the working tree must be untouched when recovery does not apply"
        );
    }
    #[test]
    fn try_auto_recover_live_prompt_drift_skips_when_dropped_prompts_recorded() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        let snapshot = crate::test_support::drift_content_ours();
        let fragmented = crate::test_support::drift_baseline();
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();
        // A genuine dropped user prompt was recorded this cycle → session-check
        // owns the fail-closed; auto-recovery must NOT paper over it.
        crate::cycle_state::record_dropped_exchange_prompts(&doc, &["do #dropped".to_string()])
            .unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert!(
            recovered.is_none(),
            "recorded dropped prompts must block auto-recovery (fail closed)"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            fragmented,
            "the working tree must be untouched when a dropped prompt was recorded"
        );
    }
    #[test]
    fn snapshot_contains_dropped_prompt_matches_consumed_and_active() {
        let snapshot = concat!(
            "<!-- agent:queue go -->\n",
            "- ~~do [#consumed]~~\n",
            "- do [#active]\n",
            "<!-- /agent:queue -->\n",
        );
        // Consumed (struck) item still present → not lost.
        assert!(snapshot_contains_dropped_prompt(snapshot, "do [#consumed]"));
        // Active item present → not lost.
        assert!(snapshot_contains_dropped_prompt(snapshot, "do [#active]"));
        // Genuinely absent → real loss.
        assert!(!snapshot_contains_dropped_prompt(snapshot, "do [#gone]"));
    }
    #[test]
    fn try_auto_recover_live_prompt_drift_fires_when_dropped_prompt_is_consumed_in_snapshot() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");

        // Snapshot consumed the queued `do [#fix]` (struck) and carries the full
        // `### Re:` response; the fragmented disk file also struck it but lost the
        // response body → wedge shape.
        let snapshot =
            crate::test_support::drift_content_ours().replace("- do [#fix]\n", "- ~~do [#fix]~~\n");
        let fragmented =
            crate::test_support::drift_baseline().replace("- do [#fix]\n", "- ~~do [#fix]~~\n");
        fs::write(&doc, &fragmented).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&snapshot), Some(&fragmented)).unwrap();
        crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();
        // The drift heuristic recorded the consumed item as a dropped queue prompt.
        crate::cycle_state::record_dropped_queue_prompts(&doc, &["do [#fix]".to_string()]).unwrap();

        let recovered = try_auto_recover_live_prompt_drift(&doc, &snapshot, &fragmented).unwrap();
        assert!(
            recovered.is_some(),
            "a dropped prompt that survives (struck) in the snapshot must not block recovery"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            snapshot,
            "auto-recovery must write the realtime merge target to disk"
        );

        // `#jbstalecache`: the recovery write records the IPC-listener state so the
        // operator can correlate a stale-cache dialog with this disk write. No live
        // listener exists in the test env, so the canonical marker reports
        // `ipc_listener_active=false` and the dedicated stale-cache-risk line stays
        // silent (it only fires when a listener is genuinely active).
        let ops_log =
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("live_prompt_drift_auto_recovered")
                && ops_log.contains("ipc_listener_active=false"),
            "recovery marker must record the IPC-listener state:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("[jbstalecache]"),
            "the stale-cache-risk marker must stay silent without an active listener:\n{ops_log}"
        );
    }
    #[test]
    fn stale_snapshot_reset_drift_blocks_large_snapshot_only_content() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let stale_exchange = "duplicated response\n".repeat(20);
        let snapshot = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange patch=append -->\n{}<!-- /agent:exchange -->\n",
            stale_exchange
        );
        let current = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange patch=append -->\nclean\n<!-- /agent:exchange -->\n";

        let result =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "stream write");

        let message = result
            .expect_err("stale larger snapshot must fail closed")
            .to_string();
        assert!(
            message.contains("agent-doc reset --from-current"),
            "recovery guidance should name deterministic sidecar reset: {message}"
        );
    }

    #[test]
    fn stale_snapshot_reset_drift_rebases_historical_exchange_trim_and_sibling_queue_add() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed").unwrap();
        let old_blocks = (0..12)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(12)
                )
            })
            .collect::<String>();
        let kept_block = "### Re: kept - gpt-5\n\nKept response.\n";
        let snapshot = format!(
            "---\nagent_doc_session: test\nagent: opencode\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{old_blocks}{kept_block}<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue auto -->\n- do [#active]\n<!-- /agent:queue -->\n"
        );
        let current = format!(
            "---\nagent_doc_session: test\nagent: claude\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{kept_block}<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue auto -->\n- do [#active]\n- do [#sibling]\n<!-- /agent:queue -->\n"
        );
        fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();
        let active_node_key = queue_node_key_for_id(&snapshot, "active");
        let scope = agent_doc_turn::turn_scope::TurnScope::for_driver_with_exchange_tail(
            Some(agent_doc_turn::turn_scope::Address::node(
                "queue",
                0,
                &active_node_key,
            )),
            Some(0),
        );
        agent_doc_turn_scope_io::save(&doc, &scope).unwrap();

        let rebased =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), &current, "preflight")
                .expect("historical trim plus sibling queue add should rebase");

        assert!(rebased, "guard should report a snapshot refresh");
        assert_eq!(crate::snapshot::load(&doc).unwrap(), Some(current));
        let ops_log =
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("stale_snapshot_visible_rebased")
                && ops_log.contains("historical_exchange_trim_unrelated_drift"),
            "rebase marker should explain the scoped drift:\n{ops_log}"
        );
    }

    #[test]
    fn stale_snapshot_reset_drift_rebases_compact_summary_replacement_on_stream_write() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed").unwrap();
        let old_blocks = (0..12)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(12)
                )
            })
            .collect::<String>();
        let snapshot = format!(
            "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{old_blocks}<!-- agent:boundary:old -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
        );
        let current = "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Session Summary\n\n*Compacted. Content archived to `.agent-doc/archives/session.md`*\n\nCompacted content:\n- Archived 12 response topic(s): archived 0; archived 1; archived 2; 9 more\n- Prior summary/context: compacted prior responses\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n";
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();
        let scope =
            agent_doc_turn::turn_scope::TurnScope::for_driver_with_exchange_tail(None, Some(0));
        agent_doc_turn_scope_io::save(&doc, &scope).unwrap();

        let rebased =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "stream write")
                .expect("compact summary replacement should rebase stale pre-compact snapshot");

        assert!(rebased, "guard should report a snapshot refresh");
        assert_eq!(
            crate::snapshot::load(&doc).unwrap(),
            Some(current.to_string())
        );
        let ops_log =
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("stale_snapshot_visible_rebased")
                && ops_log.contains("phase=stream write")
                && ops_log.contains("historical_exchange_trim"),
            "stream-write rebase marker should explain compact-summary drift:\n{ops_log}"
        );
    }

    #[test]
    fn stale_snapshot_reset_drift_rebases_compact_summary_after_clear_via_binary_origin_marker() {
        // `#provauth3`: a session resumed after `/clear` has NO turn scope, but the
        // binary-authored compaction marker survives on disk. The guard must treat
        // the pre-compact snapshot vs compacted file shrink as authoritative
        // binary-origin state and rebase, instead of tripping "looks like a manual
        // cleanup" (the bug hit at the start of this dogfood session).
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed").unwrap();
        let old_blocks = (0..12)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(12)
                )
            })
            .collect::<String>();
        let snapshot = format!(
            "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{old_blocks}<!-- agent:boundary:old -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
        );
        let current = "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Session Summary\n\n*Compacted. Content archived to `.agent-doc/archives/session.md`*\n\nCompacted content:\n- Archived 12 response topic(s): archived 0; archived 1; archived 2; 9 more\n- Prior summary/context: compacted prior responses\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n";
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();
        // No turn_scope saved (post-`/clear`). The binary-origin signal is the
        // recorded compaction marker.
        crate::session_accretion::record_recent_exchange_compaction(&doc).unwrap();

        let rebased =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "preflight")
                .expect("binary-origin compaction marker should rebase the stale snapshot");

        assert!(rebased, "guard should report a snapshot refresh");
        assert_eq!(
            crate::snapshot::load(&doc).unwrap(),
            Some(current.to_string())
        );
        let ops_log =
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("stale_snapshot_visible_rebased")
                && ops_log.contains("historical_exchange_trim"),
            "post-clear compaction rebase marker should explain the drift:\n{ops_log}"
        );
    }

    #[test]
    fn stale_snapshot_reset_drift_skips_rebase_when_active_capture_response_missing_from_visible() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let current = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n\
❯ Please reply\n\
<!-- agent:boundary:old -->\n\
<!-- /agent:exchange -->\n";
        let response_body = format!(
            "### Re: Please reply — gpt-5\n\n{}\n",
            "Captured response paragraph.\n".repeat(20)
        );
        let response_patch =
            format!("<!-- patch:exchange -->\n{response_body}<!-- /patch:exchange -->\n");
        let snapshot = current.replace(
            "<!-- agent:boundary:old -->",
            &format!("{response_body}<!-- agent:boundary:new -->"),
        );
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, current).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(current), Some(current)).unwrap();
        crate::capture::capture_response(&doc, &response_patch).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();

        let rebased = guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "commit")
            .expect("active captured response must not trip stale-snapshot reset repair");

        assert!(
            !rebased,
            "active capture should leave the response-bearing snapshot in place"
        );
        assert_eq!(
            crate::snapshot::load(&doc).unwrap(),
            Some(snapshot),
            "prompt-only visible text must not overwrite the response snapshot"
        );
        let ops_log =
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("stale_snapshot_rebase_skipped_active_capture"),
            "skip marker should explain why the stale rebase was suppressed:\n{ops_log}"
        );
    }

    #[test]
    fn stale_snapshot_reset_drift_blocks_compact_summary_without_scope_or_marker() {
        // `#provauth3` safety rail: an exchange shrink to a compaction-shaped block
        // with NEITHER a live turn scope NOR a recorded binary compaction has no
        // provenance, so it must still fail closed (a genuine accidental cleanup
        // that happens to look like a summary must not be auto-adopted).
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed").unwrap();
        let old_blocks = (0..12)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(12)
                )
            })
            .collect::<String>();
        let snapshot = format!(
            "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{old_blocks}<!-- agent:boundary:old -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
        );
        let current = "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Session Summary\n\n*Compacted. Content archived to `.agent-doc/archives/session.md`*\n\nCompacted content:\n- Archived 12 response topic(s): archived 0; archived 1; archived 2; 9 more\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n";
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();
        // No turn_scope and no compaction marker → no provenance signal.

        let err = guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "preflight")
            .expect_err("compaction-shaped shrink without provenance must fail closed");
        assert!(
            err.to_string().contains("agent-doc reset --from-current"),
            "unproven shrink should keep deterministic reset guidance: {err}"
        );
        assert_eq!(crate::snapshot::load(&doc).unwrap(), Some(snapshot));
    }

    #[test]
    fn stale_snapshot_reset_drift_blocks_fake_session_summary_without_compact_marker() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed").unwrap();
        let old_blocks = (0..12)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(12)
                )
            })
            .collect::<String>();
        let snapshot = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{old_blocks}<!-- /agent:exchange -->\n"
        );
        let current = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Session Summary\n\nOperator-authored replacement without compact archive proof.\n<!-- /agent:exchange -->\n";
        crate::snapshot::save(&doc, &snapshot).unwrap();
        let scope =
            agent_doc_turn::turn_scope::TurnScope::for_driver_with_exchange_tail(None, Some(0));
        agent_doc_turn_scope_io::save(&doc, &scope).unwrap();

        let err =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "stream write")
                .expect_err("non-compact exchange rewrite must still fail closed");

        assert!(
            err.to_string().contains("agent-doc reset --from-current"),
            "unsafe exchange rewrite should keep deterministic reset guidance: {err}"
        );
        assert_eq!(crate::snapshot::load(&doc).unwrap(), Some(snapshot));
    }

    #[test]
    fn stale_snapshot_reset_drift_blocks_when_active_queue_driver_changes() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed").unwrap();
        let old_blocks = (0..12)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(12)
                )
            })
            .collect::<String>();
        let kept_block = "### Re: kept - gpt-5\n\nKept response.\n";
        let snapshot = format!(
            "---\nagent_doc_session: test\nagent: opencode\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{old_blocks}{kept_block}<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue auto -->\n- do [#active]\n<!-- /agent:queue -->\n"
        );
        let current = format!(
            "---\nagent_doc_session: test\nagent: claude\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{kept_block}<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue auto -->\n- do [#sibling]\n<!-- /agent:queue -->\n"
        );
        fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();
        let active_node_key = queue_node_key_for_id(&snapshot, "active");
        let scope = agent_doc_turn::turn_scope::TurnScope::for_driver_with_exchange_tail(
            Some(agent_doc_turn::turn_scope::Address::node(
                "queue",
                0,
                &active_node_key,
            )),
            Some(0),
        );
        agent_doc_turn_scope_io::save(&doc, &scope).unwrap();
        let (_, snapshot_body) = agent_doc_frontmatter::frontmatter::parse(&snapshot).unwrap();
        let (_, current_body) = agent_doc_frontmatter::frontmatter::parse(&current).unwrap();
        let queue_events: Vec<_> =
            agent_doc_markdown_ast::events::diff_node_events(snapshot_body, current_body)
                .into_iter()
                .filter(|event| event.component == "queue")
                .collect();
        assert!(
            !component_change_is_turn_independent(snapshot_body, current_body, "queue", &scope),
            "fixture should affect the active queue driver; events={queue_events:?}"
        );

        let err = guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), &current, "preflight")
            .expect_err("active queue driver edit must stay structural");

        assert!(
            err.to_string().contains("agent-doc reset --from-current"),
            "unsafe drift should keep deterministic reset guidance: {err}"
        );
        assert_eq!(crate::snapshot::load(&doc).unwrap(), Some(snapshot));
    }

    #[test]
    fn stale_snapshot_reset_drift_allows_small_size_delta() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let snapshot = "a".repeat(1000);
        let current = "b".repeat(940);

        let result =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), &current, "stream write");

        assert!(
            result.is_ok(),
            "minor snapshot/file size drift should not block writes"
        );
    }
}
