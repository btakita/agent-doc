//! Write convergence sidecar adapters.
//!
//! This crate owns file-backed write-convergence decisions that sit between
//! pure realtime/write policy and durable sidecars. It keeps those decision
//! graphs out of the orchestration command crate.

use agent_doc_document_realtime::write_policy::{
    exchange_change_is_safe_historical_reduction, stale_snapshot_reset_drift,
};
use agent_doc_element::element::is_backlog_component;
use agent_doc_turn::response_replay::response_materialized_in_content;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Read the ack-content sidecar file written by the plugin after apply.
/// Keyed by `patch_id` (same UUID the binary embedded in the patch payload).
/// Deletes the sidecar on success. Returns None if no sidecar present (old plugin).
pub fn read_ack_content_sidecar(project_root: &Path, patch_id: &str) -> Result<Option<String>> {
    let sidecar = project_root
        .join(".agent-doc/ack-content")
        .join(format!("{patch_id}.md"));
    if !sidecar.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&sidecar)
        .with_context(|| format!("failed to read ack-content sidecar {sidecar:?}"))?;
    let _ = std::fs::remove_file(&sidecar);
    Ok(Some(content))
}

pub fn cleanup_legacy_ipc_degraded(project_root: &Path) {
    let marker = project_root.join(".agent-doc/ipc-degraded");
    if marker.is_file()
        && let Err(e) = std::fs::remove_file(&marker)
    {
        eprintln!(
            "[write] WARNING: failed to remove legacy IPC degraded marker {}: {}",
            marker.display(),
            e
        );
    }
}

pub const IPC_DEWEDGE_TIMEOUT_THRESHOLD: u64 = 2;

pub fn ipc_dewedge_marker_path(project_root: &Path, file: &Path) -> Result<PathBuf> {
    let hash = agent_doc_fs::document_state_hash(file)?;
    Ok(project_root
        .join(".agent-doc/ipc-degraded")
        .join(format!("{hash}.json")))
}

pub fn ipc_dewedge_marker_for_current_session(
    project_root: &Path,
    file: &Path,
) -> Result<Option<serde_json::Value>> {
    let marker = ipc_dewedge_marker_path(project_root, file)?;
    if !marker.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marker)
        .with_context(|| format!("failed to read IPC degraded marker {}", marker.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse IPC degraded marker {}", marker.display()))?;
    let marker_session = value
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let session_id =
        agent_doc_frontmatter_io::session::read_session_id(file).unwrap_or_else(|| "-".to_string());
    if marker_session != session_id {
        return Ok(None);
    }
    Ok(Some(value))
}

pub fn ipc_direct_disk_degraded(project_root: &Path, file: &Path) -> Result<bool> {
    let degraded = ipc_dewedge_marker_for_current_session(project_root, file)?
        .and_then(|value| value.get("degraded").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    if !degraded {
        return Ok(false);
    }
    // `#ipc-degrade-self-heal`: the degrade latch is a circuit breaker, not a
    // permanent session verdict. It may clear only when the plugin proves it can
    // accept AND ack a lightweight message.
    match agent_doc_ipc_io::probe_listener_ack(project_root, ipc_dewedge_probe_timeout()) {
        Ok(true) => {
            remove_ipc_dewedge_marker(project_root, file, "listener_ack_recovered")?;
            return Ok(false);
        }
        Ok(false) => {}
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "ipc_socket_degraded_self_heal_probe_failed file={} reason={}",
                    file.display(),
                    err.to_string().replace(char::is_whitespace, "_")
                ),
            );
        }
    }
    Ok(true)
}

fn ipc_dewedge_probe_timeout() -> std::time::Duration {
    if cfg!(test) {
        std::time::Duration::from_millis(250)
    } else {
        std::time::Duration::from_millis(750)
    }
}

pub fn log_ipc_dewedge_direct_disk_skip(file: &Path, transport: &str) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_listener_degraded_direct_disk file={} transport={} reason=repeated_ack_timeout",
            file.display(),
            transport
        ),
    );
}

/// `#ipc-degraded-prefers-file-ipc`: a latched-degraded socket means only the
/// plugin's *socket* listener is wedged. The file-IPC patch queue uses a
/// separate plugin file watcher that is very likely still alive, so a degraded
/// write routes through it (the plugin applies via the Document API) instead of
/// a raw disk write that manufactures an IDEA "File Cache Conflict". If file IPC
/// also fails to prove delivery, the write fails closed for retry.
pub fn log_ipc_dewedge_prefer_file_ipc(file: &Path, transport: &str) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_socket_degraded_prefer_file_ipc file={} transport={} reason=repeated_ack_timeout disk_write=disabled",
            file.display(),
            transport
        ),
    );
}

pub fn record_ipc_socket_ack_timeout(
    project_root: &Path,
    file: &Path,
    patch_id: Option<&str>,
    transport: &str,
) -> Result<bool> {
    cleanup_legacy_ipc_degraded(project_root);
    let marker = ipc_dewedge_marker_path(project_root, file)?;
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create IPC degraded marker directory {}",
                parent.display()
            )
        })?;
    }
    let prior = ipc_dewedge_marker_for_current_session(project_root, file)?;
    let prior_timeouts = prior
        .as_ref()
        .and_then(|value| value.get("consecutive_timeouts").and_then(|v| v.as_u64()))
        .unwrap_or(0);
    let consecutive_timeouts = prior_timeouts.saturating_add(1);
    let degraded = agent_doc_supervisor::lifecycle::write_wedged_from_ipc_failures(
        consecutive_timeouts,
        true,
        IPC_DEWEDGE_TIMEOUT_THRESHOLD,
    );
    let value = serde_json::json!({
        "session_id": agent_doc_frontmatter_io::session::read_session_id(file)
            .unwrap_or_else(|| "-".to_string()),
        "consecutive_timeouts": consecutive_timeouts,
        "degraded": degraded,
        "last_patch_id": patch_id.unwrap_or("-"),
        "last_transport": transport,
    });
    atomic_write(&marker, &serde_json::to_string_pretty(&value)?)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_socket_ack_timeout_recorded file={} transport={} patch_id={} consecutive_timeouts={} degraded={}",
            file.display(),
            transport,
            patch_id.unwrap_or("-"),
            consecutive_timeouts,
            degraded
        ),
    );
    Ok(degraded)
}

pub fn remove_ipc_dewedge_marker(project_root: &Path, file: &Path, reason: &str) -> Result<()> {
    let marker = ipc_dewedge_marker_path(project_root, file)?;
    if marker.exists() {
        std::fs::remove_file(&marker).with_context(|| {
            format!("failed to remove IPC degraded marker {}", marker.display())
        })?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ipc_socket_ack_timeouts_cleared file={} reason={}",
                file.display(),
                reason
            ),
        );
    }
    Ok(())
}

pub fn clear_ipc_socket_ack_timeouts(project_root: &Path, file: &Path, reason: &str) -> Result<()> {
    let Some(value) = ipc_dewedge_marker_for_current_session(project_root, file)? else {
        return Ok(());
    };
    // A routine successful write clears accrued timeout votes, but it must NOT
    // clear a *degraded* latch on its own. The degraded latch is cleared only by
    // a proven-live listener re-probe (`#ipc-degrade-self-heal`).
    if value
        .get("degraded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Ok(());
    }
    remove_ipc_dewedge_marker(project_root, file, reason)
}

/// Poll for the ack-content sidecar with timeout.
pub fn poll_ack_content_sidecar(
    project_root: &Path,
    patch_id: &str,
    timeout: std::time::Duration,
    poll_interval: std::time::Duration,
) -> Result<Option<String>> {
    let start = std::time::Instant::now();
    loop {
        match read_ack_content_sidecar(project_root, patch_id)? {
            Some(content) => return Ok(Some(content)),
            None if start.elapsed() >= timeout => return Ok(None),
            None => std::thread::sleep(poll_interval),
        }
    }
}

/// `#supselfheal` Phase 2 — read the persisted editor-IPC wedge fact for `file`
/// so the route-owned supervisor idle watch can feed `write_wedged` into
/// `supervisor_recycle_action`. Best effort: a missing/unreadable marker is
/// "not wedged".
pub fn editor_ipc_write_wedged(project_root: &Path, file: &Path) -> bool {
    ipc_dewedge_marker_for_current_session(project_root, file)
        .ok()
        .flatten()
        .and_then(|value| value.get("degraded").and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

/// `#supselfheal` Phase 2 — log that a wedged editor-IPC write is now requesting
/// a supervisor recycle through the policy owner.
pub fn log_write_wedge_requests_supervisor_recycle(file: &Path, source: &str) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "write_wedged_supervisor_recycle_requested file={} source={} action=request_recycle_through_owner reason=repeated_ack_timeout_active_listener",
            file.display(),
            source
        ),
    );
}

/// `#turnsaferecycle` Goal 2 — pure: given stale-supervisor evidence at a proven
/// IPC drift, does the workflow kernel say to schedule an immediate forced PCP
/// recycle (`RecycleNow`) rather than only surface advisory guidance?
pub fn stale_ipc_drift_forces_pcp_recycle(stale: bool, auto_recycle: bool) -> bool {
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

/// `#turnsaferecycle` Goal 2 — schedule a forced PCP recycle for a proven stale
/// supervisor IPC drift. Fail-open: missing root, fresh supervisor, opted-out
/// auto-recycle, or scheduling failure leaves existing retry/advisory behavior.
pub fn schedule_stale_supervisor_pcp_recycle(file: &Path, source: &str) -> bool {
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return false;
    };
    if agent_doc_controller_io::project_controller::stale_supervisor_warning_for_doc(file).is_none()
    {
        return false;
    }
    let auto_recycle = agent_doc_supervisor_io::config::supervisor_auto_recycle_enabled(file);
    if !stale_ipc_drift_forces_pcp_recycle(true, auto_recycle) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "stale_supervisor_ipc_drift_surfaced file={} source={} action=advisory_only reason=auto_recycle_opted_out",
                file.display(),
                source
            ),
        );
        return false;
    }
    match agent_doc_controller_io::project_controller::recycle_controller_force(&project_root, true)
    {
        Ok(scheduled) => {
            agent_doc_ops_log_io::log_op(
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

/// `#turnsaferecycle` Goal 3 — the shared stale-supervisor write-entry
/// short-circuit for IPC write entry points.
pub fn stale_supervisor_write_short_circuit(
    file: &Path,
    source: &str,
) -> Option<agent_doc_flow::outcome::UserFacingOutcome> {
    let base = file
        .canonicalize()
        .ok()
        .map(|canonical| agent_doc_project_root_io::resolve_ipc_project_root(&canonical))?;
    if !agent_doc_turn_status_io::supervisor_stale(&base) {
        return None;
    }
    schedule_stale_supervisor_pcp_recycle(file, source);
    if let Err(err) = agent_doc_supervisor_io::recycle_request::request_recycle_for_doc(
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
    agent_doc_ops_log_io::log_op(
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
        agent_doc_ops_log_io::log_op(
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
        agent_doc_snapshot_io::save(file, current_doc, agent_doc_ops_log_io::log_op)?;
        let crdt = agent_doc_merge::crdt::CrdtDoc::from_text(current_doc).encode_state();
        agent_doc_merge_io::save_document_crdt(file, &crdt, current_doc)?;
        agent_doc_ops_log_io::log_op(
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

    agent_doc_ops_log_io::log_op(
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
    let scope = agent_doc_turn_scope_io::load(file);
    let recent_binary_compaction =
        agent_doc_session_accretion_io::recent_exchange_compaction_timestamp(file)
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
    let Ok(Some(state)) = agent_doc_cycle_state_io::load(file) else {
        return false;
    };
    if !state.is_open() {
        return false;
    }
    let Ok(Some(capture)) = agent_doc_capture_io::load_active(file) else {
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

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn ipc_ack_timeouts_degrade_current_session_to_file_ipc_retry() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test-session\n---\n\ncontent").unwrap();

        assert!(
            !record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap(),
            "first timeout should only record health state"
        );
        assert!(
            record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap(),
            "second consecutive timeout should mark the listener degraded"
        );
        assert!(
            ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "current session should now bypass IPC"
        );

        fs::write(&doc, "---\nsession: next-session\n---\n\ncontent").unwrap();
        assert!(
            !ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "a new session id must not inherit the old session's degraded marker"
        );
    }

    #[test]
    fn degraded_latch_self_heals_when_listener_recovers() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: heal-session\n---\n\ncontent").unwrap();

        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap();
        assert!(
            ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "two timeouts with no live listener should stay degraded"
        );

        let root_clone = dir.path().to_path_buf();
        let server = std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&root_clone, |_msg| {
                Some(r#"{"type":"ack","id":"x"}"#.to_string())
            });
        });
        std::thread::sleep(std::time::Duration::from_millis(150));

        assert!(
            !ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "a recovered live listener must self-heal the degrade latch"
        );
        let marker = ipc_dewedge_marker_path(dir.path(), &doc).unwrap();
        assert!(
            !marker.exists(),
            "self-heal must remove the degraded marker"
        );

        let _ = std::fs::remove_file(agent_doc_ipc_io::socket_path(dir.path()));
        drop(server);
    }

    #[test]
    fn stale_supervisor_write_short_circuit_passes_through_when_fresh() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        assert!(stale_supervisor_write_short_circuit(&file, "unit_test").is_none());
    }

    #[test]
    fn stale_supervisor_write_short_circuit_defers_when_marker_present() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let canonical = file.canonicalize().unwrap();
        let base = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
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
        assert!(
            stale_ipc_drift_forces_pcp_recycle(true, true),
            "stale + auto-recycle must force RecycleNow"
        );
        assert!(
            !stale_ipc_drift_forces_pcp_recycle(true, false),
            "auto-recycle opted out must stay advisory"
        );
        assert!(
            !stale_ipc_drift_forces_pcp_recycle(false, true),
            "a fresh supervisor is never a recycle candidate"
        );
    }

    #[test]
    fn editor_ipc_write_wedged_reads_latched_degraded_marker() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path();
        let file = project_root.join("plan.md");
        fs::write(&file, "# plan\n").unwrap();
        assert!(!editor_ipc_write_wedged(project_root, &file));
        for _ in 0..IPC_DEWEDGE_TIMEOUT_THRESHOLD {
            record_ipc_socket_ack_timeout(project_root, &file, Some("p1"), "finalize").unwrap();
        }
        assert!(
            editor_ipc_write_wedged(project_root, &file),
            "a latched degraded marker should read as a write wedge"
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
    fn stale_snapshot_reset_drift_rebases_compact_summary_after_clear_via_binary_origin_marker() {
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
        agent_doc_snapshot_io::save(&doc, &snapshot, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_session_accretion_io::record_recent_exchange_compaction(&doc).unwrap();

        let rebased =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "preflight")
                .expect("binary-origin compaction marker should rebase the stale snapshot");

        assert!(rebased, "guard should report a snapshot refresh");
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap(),
            Some(current.to_string())
        );
    }
}
