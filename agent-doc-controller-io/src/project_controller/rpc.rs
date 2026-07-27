//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_controller::dispatch::{
    DISPATCH_COALESCED_IN_FLIGHT_MARKER, DISPATCH_STALE_GENERATION_REDIRECT_MARKER,
    DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER, DispatchBlockedProofFacts,
    StaleQueuePauseRecovery, append_dispatch_proof_payload, dispatch_blocked_proof_fields,
    dispatch_command_kind_is_operator_reopen, dispatch_diagnostic_field,
    dispatch_error_stale_generation_redirect_target, dispatch_should_coalesce_in_flight,
    pause_reason_is_stale_supervisor_churn_stop, queue_pause_predates_boot,
    spent_preset_id_from_pause_reason, stale_supervisor_pid_from_pause_reason,
};
use agent_doc_controller::status;
use agent_doc_controller::supervisor_replacement::{
    SupervisorReplacementRequestFields, parse_supervisor_replacement_request,
};
use agent_doc_controller::timeout::is_timeout_error;
use agent_doc_document_realtime::watch_authority::{DiskChangeSignal, WatchAction, WatchDelivery};
use agent_doc_turn_executor::binary::current_agent_doc_binary;
use std::collections::BTreeSet;

const CONTROLLER_CRDT_CURRENT_TEXT_POLL_TIMEOUT: Duration = Duration::from_secs(1);
// Foreground closeout reads may briefly queue behind a replica bootstrap or
// durable outbox fold. Give that observation enough time to see the ACK that
// already landed; idle revision probes retain the sub-second budget below.
const CONTROLLER_CRDT_CURRENT_TEXT_READ_TIMEOUT: Duration = Duration::from_secs(5);
const CONTROLLER_CRDT_REVISION_READ_TIMEOUT: Duration = Duration::from_millis(750);
const CONTROLLER_CRDT_CURRENT_TEXT_TIMEOUT: Duration = Duration::from_secs(120);
/// `#lazily-hot-path` W1 — ceiling for a single server-side visible-write receipt
/// await. Matches the CRDT current-text budget above: the convergence wait is
/// legitimately long, so the client's real deadline (not a global hang budget)
/// governs, chunked into awaits no longer than this.
const CONTROLLER_VISIBLE_WRITE_AWAIT_MAX: Duration = Duration::from_secs(120);
/// `#lazily-hot-path` Theme A — ceiling for one delivery-convergence subscription.
/// The controller parks on the hub's Lazily convergence cell; there is no polling
/// cadence and no repeated RPC while the observed revision is unchanged.
const CONTROLLER_DELIVERY_CONVERGENCE_AWAIT_MAX: Duration = Duration::from_secs(120);
const CONTROLLER_MODEL_PRESSURE_COOLDOWN: Duration = Duration::from_secs(30);
const CONTROLLER_MODEL_PRESSURE_STATE_KEY: &str = "controller_model_pressure_deadline";
/// A CP-owned git commit (barrier + stage + commit + boundary reposition) can run
/// several seconds — well past the 5s default RPC timeout — so `commit_document`
/// gets a generous ceiling.
const CONTROLLER_COMMIT_DOCUMENT_TIMEOUT: Duration = Duration::from_secs(120);
const CONTROLLER_COMPACT_DOCUMENT_TIMEOUT: Duration = Duration::from_secs(300);
static EMBEDDED_NATIVE_HOST: AtomicBool = AtomicBool::new(false);
#[cfg(not(any(test, feature = "test-support")))]
const CONTROLLER_SYNC_TMUX_LAYOUT_TIMEOUT: Duration = Duration::from_secs(35);

/// Mark this process as a reloadable native-library host.
///
/// A JVM-hosted cdylib must not spawn a controller-lifetime child-reaper thread,
/// because that thread would pin the old generation across `dlclose`. Controller
/// launch instead goes through a short-lived external `agent-doc` helper.
pub fn mark_embedded_native_host() {
    EMBEDDED_NATIVE_HOST.store(true, Ordering::SeqCst);
}

pub(crate) fn connect(project_root: &Path) -> Result<interprocess::local_socket::Stream> {
    connect_path(&socket_path(project_root))
}

fn controller_model_pressure_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn controller_model_pressure_jitter_secs(doc: &Path) -> u64 {
    let hash = agent_doc_hash::content_hash(&doc.to_string_lossy());
    u64::from_str_radix(hash.get(..2).unwrap_or("00"), 16).unwrap_or(0) % 8
}

fn read_controller_model_pressure_deadline(project_root: &Path) -> Result<Option<u64>> {
    let conn = agent_doc_sqlite::state_store::open_state_db(project_root)?;
    agent_doc_sqlite::state_store::load_project_runtime_state_from_db(
        &conn,
        CONTROLLER_MODEL_PRESSURE_STATE_KEY,
    )?
    .map(|raw| {
        raw.parse::<u64>()
            .context("invalid controller model pressure deadline in state.db")
    })
    .transpose()
}

fn record_controller_model_pressure(project_root: &Path, doc: &Path, source: &str, error: &str) {
    let mut conn = match agent_doc_sqlite::state_store::open_state_db(project_root) {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("[agent-doc] failed to open state.db for controller model pressure: {err:#}");
            return;
        }
    };
    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(err) => {
            eprintln!("[agent-doc] failed to begin controller model pressure transaction: {err}");
            return;
        }
    };
    let now = controller_model_pressure_now_secs();
    let deadline = now.saturating_add(CONTROLLER_MODEL_PRESSURE_COOLDOWN.as_secs());
    let existing_deadline = agent_doc_sqlite::state_store::load_project_runtime_state_from_db(
        &tx,
        CONTROLLER_MODEL_PRESSURE_STATE_KEY,
    )
    .ok()
    .flatten()
    .and_then(|raw| raw.parse::<u64>().ok())
    .unwrap_or(0);
    // Do not rewrite/log the same project-wide marker on every foreground
    // retry. Refresh only after half its cooldown has elapsed.
    let refresh_after = now.saturating_add(CONTROLLER_MODEL_PRESSURE_COOLDOWN.as_secs() / 2);
    if existing_deadline > refresh_after {
        return;
    }
    let retained_deadline = existing_deadline.max(deadline);
    if let Err(err) = agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
        &tx,
        CONTROLLER_MODEL_PRESSURE_STATE_KEY,
        &retained_deadline.to_string(),
        now.saturating_mul(1000),
    )
    .and_then(|()| tx.commit().map_err(Into::into))
    {
        eprintln!("[agent-doc] failed to persist controller model pressure in state.db: {err:#}");
        return;
    }
    agent_doc_ops_log_io::log_op(
        doc,
        &format!(
            "controller_model_pressure_recorded file={} source={} cooldown_secs={} deadline={} error={}",
            doc.display(),
            source,
            CONTROLLER_MODEL_PRESSURE_COOLDOWN.as_secs(),
            retained_deadline,
            error.replace('\n', "\\n")
        ),
    );
}

fn clear_expired_controller_model_pressure(project_root: &Path) {
    let deadline = match read_controller_model_pressure_deadline(project_root) {
        Ok(Some(deadline)) => deadline,
        Ok(None) => return,
        Err(err) => {
            eprintln!(
                "[agent-doc] failed to read controller model pressure from state.db: {err:#}"
            );
            return;
        }
    };
    if controller_model_pressure_now_secs() < deadline {
        return;
    }
    let result = agent_doc_sqlite::state_store::open_state_db(project_root).and_then(|conn| {
        agent_doc_sqlite::state_store::clear_project_runtime_state_in_db(
            &conn,
            CONTROLLER_MODEL_PRESSURE_STATE_KEY,
        )
    });
    if let Err(err) = result {
        eprintln!("[agent-doc] failed to clear controller model pressure from state.db: {err:#}");
    }
}

/// Project-wide idle-observation cooldown. A single failed controller-model
/// read quiets every supervisor in the project; foreground write/finalize RPCs
/// deliberately do not consult this gate.
pub fn controller_model_pressure_cooldown_active_for_doc(doc: &Path) -> bool {
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(doc) else {
        return false;
    };
    let deadline = match read_controller_model_pressure_deadline(&project_root) {
        Ok(Some(deadline)) => deadline,
        Ok(None) => return false,
        Err(err) => {
            eprintln!("[agent-doc] controller model pressure state unavailable: {err:#}");
            return true;
        }
    };
    controller_model_pressure_now_secs()
        < deadline.saturating_add(controller_model_pressure_jitter_secs(doc))
}

pub(crate) fn connect_path(path: &Path) -> Result<interprocess::local_socket::Stream> {
    let name = path.to_fs_name::<GenericFilePath>()?;
    interprocess::local_socket::ConnectOptions::new()
        .name(name)
        .connect_sync()
        .context("failed to connect to project controller")
}

pub(crate) fn request(project_root: &Path, command: &str) -> Result<String> {
    request_path(&socket_path(project_root), command)
}

pub(crate) fn request_path(path: &Path, command: &str) -> Result<String> {
    request_path_json(path, serde_json::json!({ "command": command }))
}

pub(crate) fn request_with_reason(
    project_root: &Path,
    command: &str,
    reason: &str,
) -> Result<String> {
    request_path_with_reason(&socket_path(project_root), command, reason)
}

pub(crate) fn request_path_with_reason(path: &Path, command: &str, reason: &str) -> Result<String> {
    request_path_json(
        path,
        serde_json::json!({ "command": command, "reason": reason }),
    )
}

fn request_path_json(path: &Path, request_value: serde_json::Value) -> Result<String> {
    let stream = connect_path(path)?;
    stream
        .set_recv_timeout(Some(CONTROLLER_RPC_TIMEOUT))
        .context("failed to set project controller response timeout")?;
    let (reader_half, mut writer_half) = stream.split();
    let mut request = serde_json::to_string(&request_value)?;
    request.push('\n');
    writer_half.write_all(request.as_bytes())?;
    writer_half.flush()?;

    let mut reader = BufReader::new(reader_half);
    let mut response = String::new();
    read_controller_response_line(&mut reader, &mut response)?;
    Ok(response.trim().to_string())
}

pub(crate) fn read_controller_response_line<R: BufRead>(
    reader: &mut R,
    response: &mut String,
) -> Result<()> {
    read_controller_response_line_with_timeout(reader, response, CONTROLLER_RPC_TIMEOUT)
}

fn read_controller_response_line_with_timeout<R: BufRead>(
    reader: &mut R,
    response: &mut String,
    timeout: Duration,
) -> Result<()> {
    match reader.read_line(response) {
        Ok(0) => anyhow::bail!("project controller closed connection without a response"),
        Ok(_) => Ok(()),
        Err(err) if is_timeout_error(&err) => anyhow::bail!(
            "timed out after {:.1}s waiting for project controller response",
            timeout.as_secs_f32()
        ),
        Err(err) => Err(err).context("failed to read project controller response"),
    }
}

fn controller_transport_drop_is_retryable(err: &anyhow::Error) -> bool {
    if err.chain().any(|cause| {
        cause.downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.kind(),
                ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::BrokenPipe
                    | ErrorKind::UnexpectedEof
            )
        })
    }) {
        return true;
    }
    format!("{err:#}").contains("project controller closed connection without a response")
}

fn compact_controller_error(err: &anyhow::Error) -> String {
    format!("{err:#}")
        .replace('\n', " | ")
        .chars()
        .take(240)
        .collect()
}

fn retry_controller_transport_drop<T>(
    retry_log_path: &Path,
    command: &str,
    mut request_once: impl FnMut() -> Result<T>,
) -> Result<T> {
    match request_once() {
        Err(err) if controller_transport_drop_is_retryable(&err) => {
            let detail = compact_controller_error(&err);
            agent_doc_ops_log_io::log_op(
                retry_log_path,
                &format!(
                    "controller_rpc_transport_retry command={} reason=stale_or_recycled_controller detail={}",
                    command, detail
                ),
            );
            request_once()
        }
        other => other,
    }
}

pub(crate) fn request_controller<T: DeserializeOwned>(
    project_root: &Path,
    request: ControllerRequest,
) -> Result<T> {
    request_controller_with_timeout(project_root, request, CONTROLLER_RPC_TIMEOUT)
}

#[cfg(any(test, feature = "test-support"))]
pub fn request_crdt_replica_for_test(
    project_root: &Path,
    file: &Path,
    diagnostic_payload: serde_json::Value,
) -> Result<serde_json::Value> {
    request_existing_controller_with_timeout(
        project_root,
        ControllerRequest {
            command: "crdt_replica".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(diagnostic_payload.to_string()),
        },
        CONTROLLER_RPC_TIMEOUT,
    )
}

fn request_controller_with_timeout<T: DeserializeOwned>(
    project_root: &Path,
    request: ControllerRequest,
    timeout: Duration,
) -> Result<T> {
    let stream = connect_or_launch(project_root, LaunchMode::Lazy)?;
    request_controller_on_stream_with_timeout(project_root, request, timeout, stream)
}

fn request_existing_controller_with_timeout<T: DeserializeOwned>(
    project_root: &Path,
    request: ControllerRequest,
    timeout: Duration,
) -> Result<T> {
    let stream = connect(project_root)?;
    request_controller_on_stream_with_timeout(project_root, request, timeout, stream)
}

fn request_controller_on_stream_with_timeout<T: DeserializeOwned>(
    project_root: &Path,
    request: ControllerRequest,
    timeout: Duration,
    stream: interprocess::local_socket::Stream,
) -> Result<T> {
    stream
        .set_recv_timeout(Some(timeout))
        .context("failed to set project controller response timeout")?;
    let (reader_half, mut writer_half) = stream.split();
    // #af88 B enforcement: stamp the caller's own binary identity onto the wire as
    // skew-safe extra JSON keys (ControllerRequest has no `deny_unknown_fields`, so
    // older controllers ignore them). The full identity lets a same-version newer
    // install prove that it may replace an older long-lived process without letting
    // an older caller tear down a newer controller.
    let mut request_value = serde_json::to_value(&request)?;
    if let Some(obj) = request_value.as_object_mut() {
        obj.insert(
            "binary_version".to_string(),
            serde_json::Value::String(identity_version()),
        );
        if let Ok(identity) = current_binary_identity() {
            obj.insert(
                "binary_identity".to_string(),
                serde_json::to_value(identity)?,
            );
        }
    }
    let mut raw = serde_json::to_string(&request_value)?;
    raw.push('\n');
    writer_half.write_all(raw.as_bytes())?;
    writer_half.flush()?;

    let mut reader = BufReader::new(reader_half);
    let mut response = String::new();
    read_controller_response_line_with_timeout(&mut reader, &mut response, timeout)?;
    decode_controller_response(project_root, &request, response.trim())
}

#[cfg_attr(any(test, feature = "test-support"), allow(dead_code))]
fn request_controller_with_transport_retry<T: DeserializeOwned>(
    project_root: &Path,
    request: ControllerRequest,
    retry_log_path: &Path,
) -> Result<T> {
    let command = request.command.clone();
    retry_controller_transport_drop(retry_log_path, &command, || {
        request_controller(project_root, request.clone())
    })
}

pub(crate) fn decode_controller_response<T: DeserializeOwned>(
    project_root: &Path,
    request: &ControllerRequest,
    raw_response: &str,
) -> Result<T> {
    let envelope: ControllerEnvelope<T> =
        serde_json::from_str(raw_response).with_context(|| {
            format!(
                "failed to parse project controller response envelope for command `{}`: raw={}",
                request.command, raw_response
            )
        })?;
    if envelope.ok {
        match envelope.data {
            Some(data) => Ok(data),
            None => {
                if let Some(file) = request.file.as_ref() {
                    let log_file = if file.is_absolute() {
                        file.clone()
                    } else {
                        project_root.join(file)
                    };
                    agent_doc_ops_log_io::log_op(
                        &log_file,
                        &format!(
                            "controller_response_missing_data command={} raw={}",
                            request.command, raw_response
                        ),
                    );
                }
                anyhow::bail!(
                    "project controller command `{}` returned ok response without data: raw={}",
                    request.command,
                    raw_response
                )
            }
        }
    } else {
        anyhow::bail!(
            "project controller command `{}` failed: {}",
            request.command,
            envelope
                .error
                .unwrap_or_else(|| "project controller request failed".to_string())
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoordinationClaimResponse {
    pub acquired: bool,
}

/// Atomically claim a set of ephemeral coordination scopes from the live
/// controller's Lazily graph.
pub fn try_claim_coordination(
    project_root: &Path,
    scopes: &[String],
    owner_token: &str,
    owner_pid: u32,
) -> Result<bool> {
    anyhow::ensure!(!scopes.is_empty(), "coordination scopes must not be empty");
    anyhow::ensure!(
        !owner_token.trim().is_empty(),
        "coordination owner token must not be empty"
    );
    let response: CoordinationClaimResponse = request_controller_with_transport_retry(
        project_root,
        ControllerRequest {
            command: "coordination_claim".to_string(),
            file: None,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: Some(serde_json::to_string(scopes)?),
            caller: Some(owner_token.to_string()),
            reason: None,
            supervisor_pid: Some(owner_pid),
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
        project_root,
    )?;
    Ok(response.acquired)
}

/// Release scopes previously acquired by [`try_claim_coordination`].
pub fn release_coordination(
    project_root: &Path,
    scopes: &[String],
    owner_token: &str,
) -> Result<()> {
    let _: serde_json::Value = request_controller_with_transport_retry(
        project_root,
        ControllerRequest {
            command: "coordination_release".to_string(),
            file: None,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: Some(serde_json::to_string(scopes)?),
            caller: Some(owner_token.to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
        project_root,
    )?;
    Ok(())
}

pub fn start_session(
    project_root: &Path,
    request: StartSessionRequest,
) -> Result<agent_doc_sqlite::state_store::ActorRecord> {
    request_controller(
        project_root,
        ControllerRequest {
            command: "start_session".to_string(),
            file: Some(request.file),
            session_id: Some(request.session_id),
            pane_id: Some(request.pane_id),
            window_id: Some(request.window_id),
            generation: Some(request.generation),
            state: None,
            caller: Some("start".to_string()),
            reason: Some("session_start".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn register_supervisor(
    project_root: &Path,
    registration: SupervisorRegistration,
) -> Result<agent_doc_sqlite::state_store::ActorRecord> {
    request_controller(
        project_root,
        ControllerRequest {
            command: "register_supervisor".to_string(),
            file: Some(registration.file),
            session_id: Some(registration.session_id),
            pane_id: Some(registration.pane_id),
            window_id: None,
            generation: Some(registration.generation),
            state: Some(registration.runtime_state),
            caller: None,
            reason: None,
            supervisor_pid: Some(registration.supervisor_pid),
            supervisor_socket: Some(registration.supervisor_socket),
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn mark_lifecycle(
    project_root: &Path,
    request: LifecycleRequest,
) -> Result<agent_doc_sqlite::state_store::ActorRecord> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        handle_mark_lifecycle(
            &bootstrap,
            None,
            ControllerRequest {
                command: "mark_lifecycle".to_string(),
                file: Some(request.file),
                session_id: Some(request.session_id),
                pane_id: Some(request.pane_id),
                window_id: None,
                generation: Some(request.generation),
                state: Some(request.state.as_str().to_string()),
                caller: Some(request.caller),
                reason: Some(request.reason),
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: None,
                diagnostic_payload: None,
            },
        )
    }

    #[cfg(not(any(test, feature = "test-support")))]
    request_controller(
        project_root,
        ControllerRequest {
            command: "mark_lifecycle".to_string(),
            file: Some(request.file),
            session_id: Some(request.session_id),
            pane_id: Some(request.pane_id),
            window_id: None,
            generation: Some(request.generation),
            state: Some(request.state.as_str().to_string()),
            caller: Some(request.caller),
            reason: Some(request.reason),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn refresh_supervisor_lease(
    project_root: &Path,
    request: SupervisorHeartbeatRequest,
) -> Result<SupervisorLeaseStatus> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        handle_supervisor_heartbeat(
            &bootstrap,
            None,
            ControllerRequest {
                command: "supervisor_heartbeat".to_string(),
                file: Some(request.file),
                session_id: Some(request.session_id),
                pane_id: Some(request.pane_id),
                window_id: None,
                generation: Some(request.generation),
                state: Some(request.runtime_state),
                caller: None,
                reason: None,
                supervisor_pid: request.supervisor_pid,
                supervisor_socket: request.supervisor_socket,
                command_kind: None,
                diagnostic_payload: None,
            },
        )
    }

    #[cfg(not(any(test, feature = "test-support")))]
    request_controller(
        project_root,
        ControllerRequest {
            command: "supervisor_heartbeat".to_string(),
            file: Some(request.file),
            session_id: Some(request.session_id),
            pane_id: Some(request.pane_id),
            window_id: None,
            generation: Some(request.generation),
            state: Some(request.runtime_state),
            caller: None,
            reason: None,
            supervisor_pid: request.supervisor_pid,
            supervisor_socket: request.supervisor_socket,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn authoritative_actor_binding(
    project_root: &Path,
    file: &Path,
) -> Result<Option<agent_doc_sqlite::state_store::ActorRecord>> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let document_id = agent_doc_session_actor_io::canonical_document_id_in(
            project_root,
            &file.to_string_lossy(),
        );
        load_actor_record(project_root, &document_id)
    }

    #[cfg(not(any(test, feature = "test-support")))]
    {
        let response: ActorBindingResponse =
            retry_controller_transport_drop(file, "actor_binding", || {
                request_controller(
                    project_root,
                    ControllerRequest {
                        command: "actor_binding".to_string(),
                        file: Some(file.to_path_buf()),
                        session_id: None,
                        pane_id: None,
                        window_id: None,
                        generation: None,
                        state: None,
                        caller: None,
                        reason: None,
                        supervisor_pid: None,
                        supervisor_socket: None,
                        command_kind: None,
                        diagnostic_payload: None,
                    },
                )
            })?;
        Ok(response.record)
    }
}

/// Cold durable fallback for code already executing inside the project
/// controller. External callers must use [`authoritative_actor_binding`] so the
/// live Lazily/controller image remains the primary authority instead of
/// reopening SQLite on the coordination hot path.
fn durable_actor_binding(
    project_root: &Path,
    file: &Path,
) -> Result<Option<agent_doc_sqlite::state_store::ActorRecord>> {
    let document_id =
        agent_doc_session_actor_io::canonical_document_id_in(project_root, &file.to_string_lossy());
    load_actor_record(project_root, &document_id)
}

pub fn authorize_dispatch(
    project_root: &Path,
    request: DispatchRequest,
) -> Result<DispatchAuthorization> {
    // `#qflood`: every dispatch caller (route auto-start on file change, idle
    // queue continuation, `/loop`) funnels through here. Log the invocation —
    // command_kind / diagnostic_payload identify the caller — so an operator
    // flood repro reveals which path re-invokes dispatch while the pane is
    // mid-turn. Pure observability: no behavior change, paired with the existing
    // dispatch receipt (the outcome) to show invoke→outcome per dispatch.
    agent_doc_ops_log_io::log_op(
        &request.file,
        &format!(
            "queue_dispatch_invoked file={} pane={} generation={} command_kind={} payload={}",
            request.file.display(),
            request.pane_id,
            request.generation,
            request.command_kind,
            request
                .diagnostic_payload
                .chars()
                .take(160)
                .collect::<String>(),
        ),
    );
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        let dispatch_request = |generation: u64| ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(request.file.clone()),
            session_id: Some(request.session_id.clone()),
            pane_id: Some(request.pane_id.clone()),
            window_id: None,
            generation: Some(generation),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some(request.command_kind.clone()),
            diagnostic_payload: Some(request.diagnostic_payload.clone()),
        };
        match handle_dispatch(&bootstrap, None, dispatch_request(request.generation)) {
            Err(err) => {
                // `#anw0`: a racing dispatch that lost the supersede race against a
                // newer dispatchable generation self-heals by retrying once against
                // the redirect target. One retry only — bounded, never a loop.
                if let Some(target) =
                    dispatch_error_stale_generation_redirect_target(&format!("{err:#}"))
                {
                    agent_doc_ops_log_io::log_op(
                        &request.file,
                        &format!(
                            "dispatch_retry_after_stale_generation file={} prior_generation={} next_generation={}",
                            request.file.display(),
                            request.generation,
                            target
                        ),
                    );
                    handle_dispatch(&bootstrap, None, dispatch_request(target))
                } else {
                    Err(err)
                }
            }
            ok => ok,
        }
    }

    #[cfg(not(any(test, feature = "test-support")))]
    {
        let controller_request = ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(request.file.clone()),
            session_id: Some(request.session_id),
            pane_id: Some(request.pane_id),
            window_id: None,
            generation: Some(request.generation),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some(request.command_kind),
            diagnostic_payload: Some(request.diagnostic_payload),
        };
        // `#ctlstalebin`: if the dispatch reached a stale controller and was refused,
        // retry exactly once. The retry's `request_controller` → `connect_or_launch`
        // promotes the freshly-installed binary via the two-phase handoff, so the
        // re-dispatch lands on the fresh controller. One retry only — a still-failing
        // recycle surfaces the error to the caller instead of looping.
        let file_for_log = request.file.clone();
        let request_dispatch = |controller_request: ControllerRequest| {
            request_controller_with_transport_retry::<DispatchAuthorization>(
                project_root,
                controller_request,
                &file_for_log,
            )
        };
        match request_dispatch(controller_request.clone()) {
            Err(err) if err.to_string().contains("controller_binary_stale") => {
                agent_doc_ops_log_io::log_op(
                    &request.file,
                    &format!(
                        "dispatch_retry_after_stale_binary file={}",
                        request.file.display()
                    ),
                );
                request_dispatch(controller_request)
            }
            Err(err)
                if dispatch_error_stale_generation_redirect_target(&err.to_string()).is_some() =>
            {
                // `#anw0`: the dispatch lost the supersede race against a newer
                // dispatchable generation. Retry exactly once against the redirect
                // target so racing dispatch self-heals instead of failing closed.
                let target =
                    dispatch_error_stale_generation_redirect_target(&err.to_string()).unwrap();
                agent_doc_ops_log_io::log_op(
                    &request.file,
                    &format!(
                        "dispatch_retry_after_stale_generation file={} next_generation={}",
                        request.file.display(),
                        target
                    ),
                );
                let mut redirected = controller_request.clone();
                redirected.generation = Some(target);
                request_dispatch(redirected)
            }
            other => other,
        }
    }
}

pub fn session_operator_status(project_root: &Path, file: &Path) -> Result<SessionOperatorStatus> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let document_id = agent_doc_session_actor_io::canonical_document_id_in(
            project_root,
            &file.to_string_lossy(),
        );
        let conn = open_state_db(project_root)?;
        load_session_operator_status_from_db(&conn, &document_id)
    }

    #[cfg(not(any(test, feature = "test-support")))]
    {
        request_controller(
            project_root,
            ControllerRequest {
                command: "session_status".to_string(),
                file: Some(file.to_path_buf()),
                session_id: None,
                pane_id: None,
                window_id: None,
                generation: None,
                state: None,
                caller: None,
                reason: None,
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: None,
                diagnostic_payload: None,
            },
        )
    }
}

pub fn inspect_actor(
    project_root: &Path,
    file: Option<&Path>,
    session_id: Option<&str>,
    pane_id: Option<&str>,
) -> Result<ControllerActorInspection> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        handle_inspect_actor(
            &bootstrap,
            ControllerRequest {
                command: "inspect_actor".to_string(),
                file: file.map(Path::to_path_buf),
                session_id: session_id.map(ToOwned::to_owned),
                pane_id: pane_id.map(ToOwned::to_owned),
                window_id: None,
                generation: None,
                state: None,
                caller: None,
                reason: None,
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: None,
                diagnostic_payload: None,
            },
        )
    }

    #[cfg(not(any(test, feature = "test-support")))]
    request_controller(
        project_root,
        ControllerRequest {
            command: "inspect_actor".to_string(),
            file: file.map(Path::to_path_buf),
            session_id: session_id.map(ToOwned::to_owned),
            pane_id: pane_id.map(ToOwned::to_owned),
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn tmux_focus_state(project_root: &Path) -> Result<ControllerTmuxFocusState> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        handle_tmux_focus_state(&bootstrap)
    }

    #[cfg(not(any(test, feature = "test-support")))]
    request_controller(
        project_root,
        ControllerRequest {
            command: "tmux_focus_state".to_string(),
            file: None,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

/// Content observations the caller resolved, sent to the controller so the
/// verdict is derived in the one live graph rather than in each caller.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetainedWriteObservations {
    pub authority_hash: Option<String>,
    pub authority_payload_materialized: bool,
    pub disk_hash: Option<String>,
    pub disk_payload_materialized: bool,
    /// `SupersededDeltaMaterialized` evidence. `#[serde(default)]` so a caller
    /// built by an older binary still deserializes — it simply offers no delta
    /// proof, which is the pre-existing behavior.
    #[serde(default)]
    pub authority_intent_delta_materialized: bool,
    #[serde(default)]
    pub disk_intent_delta_materialized: bool,
}

impl RetainedWriteObservations {
    fn into_planes(
        self,
    ) -> (
        Option<agent_doc_state_backbone::retained_write::ContentObservation>,
        Option<agent_doc_state_backbone::retained_write::ContentObservation>,
    ) {
        let plane =
            |hash: Option<String>, payload_materialized: bool, intent_delta_materialized: bool| {
                hash.map(|content_hash| {
                    agent_doc_state_backbone::retained_write::ContentObservation {
                        content_hash,
                        payload_materialized,
                        intent_delta_materialized,
                    }
                })
            };
        (
            plane(
                self.authority_hash,
                self.authority_payload_materialized,
                self.authority_intent_delta_materialized,
            ),
            plane(
                self.disk_hash,
                self.disk_payload_materialized,
                self.disk_intent_delta_materialized,
            ),
        )
    }
}

pub(crate) fn handle_retained_write_settlement(
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::retained_write::SettlementVerdict> {
    let file = request_file(&request)?;
    let document_hash = agent_doc_hash::document_id_for_path(&file);
    let observations = request
        .diagnostic_payload
        .as_deref()
        .and_then(|payload| serde_json::from_str::<RetainedWriteObservations>(payload).ok())
        .unwrap_or_default();
    let (authority, disk) = observations.into_planes();
    Ok(runtime.document_retained_write_verdict(&document_hash, &file, authority, disk))
}

/// Ask the controller for the document's retained-write settlement verdict.
///
/// This is the hop that makes the fact *shared*. `preflight` and `session-check`
/// are separate short-lived processes: without it they would each replay the
/// SQLite ledger and derive privately, which is exactly the divergence
/// `#retainedsettlereactive` exists to remove.
pub fn retained_write_settlement(
    project_root: &Path,
    file: &Path,
    observations: &RetainedWriteObservations,
) -> Result<agent_doc_state_backbone::retained_write::SettlementVerdict> {
    request_controller(
        project_root,
        ControllerRequest {
            command: "retained_write_settlement".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(serde_json::to_string(observations)?),
        },
    )
}

pub fn focus_document_pane(project_root: &Path, file: &Path) -> Result<ControllerTmuxFocusReceipt> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        handle_focus_document_pane(
            &bootstrap,
            ControllerRequest {
                command: "focus_document_pane".to_string(),
                file: Some(file.to_path_buf()),
                session_id: None,
                pane_id: None,
                window_id: None,
                generation: None,
                state: None,
                caller: None,
                reason: None,
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: None,
                diagnostic_payload: None,
            },
        )
    }

    #[cfg(not(any(test, feature = "test-support")))]
    {
        if command_plane_enabled() {
            let payload = FocusDocumentPaneCommandPayload {
                project_root: Some(project_root.display().to_string()),
                document_path: file.display().to_string(),
                no_promotion: true,
                active_window_guard: true,
            };
            return request_command_submit_payload(
                project_root,
                Some(file.to_path_buf()),
                "focus_document_pane",
                "agent-doc.focus_document_pane.v1",
                &format!("{}:selected-document-focus", project_root.display()),
                CONTROLLER_RPC_TIMEOUT,
                &payload,
            );
        }
        request_controller(
            project_root,
            ControllerRequest {
                command: "focus_document_pane".to_string(),
                file: Some(file.to_path_buf()),
                session_id: None,
                pane_id: None,
                window_id: None,
                generation: None,
                state: None,
                caller: None,
                reason: None,
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: None,
                diagnostic_payload: None,
            },
        )
    }
}

pub fn sync_tmux_layout(
    project_root: &Path,
    invocation: ControllerTmuxLayoutSyncInvocation,
) -> Result<ControllerTmuxLayoutSyncReceipt> {
    let diagnostic_payload =
        serde_json::to_string(&invocation).context("serialize sync tmux layout invocation")?;
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        handle_sync_tmux_layout(
            &bootstrap,
            ControllerRequest {
                command: "sync_tmux_layout".to_string(),
                file: invocation.focus.as_ref().map(PathBuf::from),
                session_id: None,
                pane_id: None,
                window_id: None,
                generation: None,
                state: None,
                caller: None,
                reason: None,
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: None,
                diagnostic_payload: Some(diagnostic_payload),
            },
        )
    }

    #[cfg(not(any(test, feature = "test-support")))]
    {
        if command_plane_enabled() {
            let payload = SyncTmuxLayoutCommandPayload {
                project_root: project_root.display().to_string(),
                columns: invocation.columns.clone(),
                window: invocation.window.clone(),
                focus: invocation.focus.clone(),
                no_autostart: invocation.no_autostart,
                exact_visible: invocation.exact_visible,
                caller_kind: if invocation.no_autostart {
                    "automatic".to_string()
                } else {
                    "manual".to_string()
                },
            };
            return request_command_submit_payload(
                project_root,
                invocation.focus.as_ref().map(PathBuf::from),
                "sync_tmux_layout",
                "agent-doc.sync_tmux_layout.v1",
                &format!("{}:sync", project_root.display()),
                CONTROLLER_SYNC_TMUX_LAYOUT_TIMEOUT,
                &payload,
            );
        }
        request_controller_with_timeout(
            project_root,
            ControllerRequest {
                command: "sync_tmux_layout".to_string(),
                file: invocation.focus.as_ref().map(PathBuf::from),
                session_id: None,
                pane_id: None,
                window_id: None,
                generation: None,
                state: None,
                caller: None,
                reason: None,
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: None,
                diagnostic_payload: Some(diagnostic_payload),
            },
            CONTROLLER_SYNC_TMUX_LAYOUT_TIMEOUT,
        )
    }
}

/// Ask the controller what tmux is showing, given the layout the caller sees.
///
/// The read half of the layout mirror (`#tmuxsyncstate`). `actual_documents` is
/// the controller's own observation — the documents its panes hold, in pane
/// order — so a caller comparing the two sides never has to report a fact only
/// the controller has. The editor-surface graph pulls this to fill in the tmux
/// side of its mirror, which is why it is a plain read: no autostart, no
/// reconcile, no pane mutation.
pub fn tmux_layout_sync_state(
    project_root: &Path,
    invocation: ControllerTmuxLayoutSyncStateInvocation,
) -> Result<ControllerTmuxLayoutSyncStateReport> {
    let diagnostic_payload = serde_json::to_string(&invocation)
        .context("serialize tmux layout sync state invocation")?;
    let file = invocation.focus.as_ref().map(PathBuf::from);
    request_controller(
        project_root,
        ControllerRequest {
            command: "tmux_layout_sync_state".to_string(),
            file,
            session_id: None,
            pane_id: None,
            window_id: invocation.window.clone(),
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(diagnostic_payload),
        },
    )
}

#[cfg(not(any(test, feature = "test-support")))]
fn command_plane_enabled() -> bool {
    std::env::var("AGENT_DOC_COMMAND_PLANE")
        .map(|value| value != "0")
        .unwrap_or(true)
}

#[cfg(not(any(test, feature = "test-support")))]
#[derive(Debug, Serialize, Deserialize)]
struct SyncTmuxLayoutCommandPayload {
    project_root: String,
    columns: Vec<String>,
    #[serde(default)]
    window: Option<String>,
    #[serde(default)]
    focus: Option<String>,
    #[serde(default)]
    no_autostart: bool,
    #[serde(default)]
    exact_visible: bool,
    #[serde(default)]
    caller_kind: String,
}

#[cfg(not(any(test, feature = "test-support")))]
fn request_command_submit_payload<T, P>(
    project_root: &Path,
    file: Option<PathBuf>,
    name: &str,
    payload_type: &str,
    idempotency_key: &str,
    timeout: Duration,
    payload: &P,
) -> Result<T>
where
    T: DeserializeOwned,
    P: Serialize,
{
    let payload_json = serde_json::to_string(payload)
        .with_context(|| format!("failed to serialize {name} command payload"))?;
    let payload_hash = agent_doc_hash::content_hash(&payload_json);
    let command_nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_else(|_| u128::from(timestamp_secs()));
    let command_id = format!(
        "cmd-{}-{}-{}",
        name.replace('_', "-"),
        command_nonce,
        &payload_hash[..payload_hash.len().min(12)]
    );
    let submit = lazily::CommandSubmit {
        command_id: command_id.clone(),
        causation_id: command_id.clone(),
        source: "agent-doc-native".to_string(),
        target: "project-controller".to_string(),
        namespace: "agent-doc".to_string(),
        name: name.to_string(),
        authority_generation: 0,
        idempotency_key: idempotency_key.to_string(),
        deadline_ms: timeout.as_millis().try_into().unwrap_or(u64::MAX),
        policy: lazily::CommandPolicy {
            dedupe: lazily::DedupePolicy::SameIdempotencyKey,
            supersede: true,
            cancel_on_preempt: true,
        },
        payload_type: payload_type.to_string(),
        payload_hash: format!("sha256:{payload_hash}"),
        payload: lazily::IpcValue::Inline(payload_json.into_bytes()),
        required_features: vec!["causal-receipts".to_string(), "command-events".to_string()],
    };
    let message = lazily::CommandMessage::CommandSubmit(Box::new(submit));
    let response: serde_json::Value = request_controller_with_timeout(
        project_root,
        ControllerRequest {
            command: "editor_command_submit".to_string(),
            file,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(serde_json::to_string(&message)?),
        },
        timeout,
    )?;
    let exit_code = response
        .get("exit_code")
        .and_then(|value| value.as_i64())
        .unwrap_or(1);
    if exit_code != 0 {
        let output = response
            .get("output")
            .and_then(|value| value.as_str())
            .unwrap_or("command-plane request failed");
        anyhow::bail!("command-plane {name} rejected: {output}");
    }
    let payload = response
        .get("payload")
        .cloned()
        .context("command-plane response missing payload")?;
    serde_json::from_value(payload).with_context(|| format!("decode {name} command payload"))
}

pub fn control_queue(
    project_root: &Path,
    file: Option<&Path>,
    action: &str,
    observed_generation: Option<u64>,
    reason: Option<&str>,
    item_id: Option<&str>,
) -> Result<ControllerAdminReceipt> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        handle_queue_control(
            &bootstrap,
            ControllerRequest {
                command: "queue_control".to_string(),
                file: file.map(Path::to_path_buf),
                session_id: None,
                pane_id: None,
                window_id: None,
                generation: observed_generation,
                state: Some(action.to_string()),
                caller: Some("admin".to_string()),
                reason: reason.map(ToOwned::to_owned),
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: Some(action.to_string()),
                diagnostic_payload: item_id.map(ToOwned::to_owned),
            },
        )
    }

    #[cfg(not(any(test, feature = "test-support")))]
    request_controller(
        project_root,
        ControllerRequest {
            command: "queue_control".to_string(),
            file: file.map(Path::to_path_buf),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: observed_generation,
            state: Some(action.to_string()),
            caller: Some("admin".to_string()),
            reason: reason.map(ToOwned::to_owned),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some(action.to_string()),
            diagnostic_payload: item_id.map(ToOwned::to_owned),
        },
    )
}

pub fn admin_reap(
    project_root: &Path,
    file: Option<&Path>,
    session_id: Option<&str>,
    pane_id: Option<&str>,
    observed_generation: u64,
    reason: &str,
) -> Result<ControllerAdminReceipt> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        handle_admin_control(
            &bootstrap,
            ControllerRequest {
                command: "admin_control".to_string(),
                file: file.map(Path::to_path_buf),
                session_id: session_id.map(ToOwned::to_owned),
                pane_id: pane_id.map(ToOwned::to_owned),
                window_id: None,
                generation: Some(observed_generation),
                state: Some("reap".to_string()),
                caller: Some("admin".to_string()),
                reason: Some(reason.to_string()),
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: Some("reap".to_string()),
                diagnostic_payload: None,
            },
        )
    }

    #[cfg(not(any(test, feature = "test-support")))]
    request_controller(
        project_root,
        ControllerRequest {
            command: "admin_control".to_string(),
            file: file.map(Path::to_path_buf),
            session_id: session_id.map(ToOwned::to_owned),
            pane_id: pane_id.map(ToOwned::to_owned),
            window_id: None,
            generation: Some(observed_generation),
            state: Some("reap".to_string()),
            caller: Some("admin".to_string()),
            reason: Some(reason.to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("reap".to_string()),
            diagnostic_payload: None,
        },
    )
}

pub fn admin_handoff(
    project_root: &Path,
    file: &Path,
    to_pane: &str,
    observed_generation: u64,
    reason: &str,
) -> Result<ControllerAdminReceipt> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        handle_admin_control(
            &bootstrap,
            ControllerRequest {
                command: "admin_control".to_string(),
                file: Some(file.to_path_buf()),
                session_id: None,
                pane_id: Some(to_pane.to_string()),
                window_id: None,
                generation: Some(observed_generation),
                state: Some("handoff".to_string()),
                caller: Some("admin".to_string()),
                reason: Some(reason.to_string()),
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: Some("handoff".to_string()),
                diagnostic_payload: None,
            },
        )
    }

    #[cfg(not(any(test, feature = "test-support")))]
    request_controller(
        project_root,
        ControllerRequest {
            command: "admin_control".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: Some(to_pane.to_string()),
            window_id: None,
            generation: Some(observed_generation),
            state: Some("handoff".to_string()),
            caller: Some("admin".to_string()),
            reason: Some(reason.to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("handoff".to_string()),
            diagnostic_payload: None,
        },
    )
}

pub fn repair_projection(
    project_root: &Path,
    file: Option<&Path>,
    projection: &str,
    observed_generation: Option<u64>,
    reason: Option<&str>,
) -> Result<ControllerAdminReceipt> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        handle_projection_repair(
            &bootstrap,
            ControllerRequest {
                command: "projection_repair".to_string(),
                file: file.map(Path::to_path_buf),
                session_id: None,
                pane_id: None,
                window_id: None,
                generation: observed_generation,
                state: Some(projection.to_string()),
                caller: Some("admin".to_string()),
                reason: reason.map(ToOwned::to_owned),
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: Some("projection_repair".to_string()),
                diagnostic_payload: None,
            },
        )
    }

    #[cfg(not(any(test, feature = "test-support")))]
    request_controller(
        project_root,
        ControllerRequest {
            command: "projection_repair".to_string(),
            file: file.map(Path::to_path_buf),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: observed_generation,
            state: Some(projection.to_string()),
            caller: Some("admin".to_string()),
            reason: reason.map(ToOwned::to_owned),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("projection_repair".to_string()),
            diagnostic_payload: None,
        },
    )
}

pub fn attach_pane(
    project_root: &Path,
    request: AttachPaneRequest,
) -> Result<agent_doc_sqlite::state_store::ActorRecord> {
    request_controller(
        project_root,
        ControllerRequest {
            command: "attach_pane".to_string(),
            file: Some(request.file),
            session_id: Some(request.session_id),
            pane_id: Some(request.pane_id),
            window_id: Some(request.window_id),
            generation: None,
            state: None,
            caller: Some("session".to_string()),
            reason: Some("manual_attach".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn authorize_operator_command(
    project_root: &Path,
    file: &Path,
    command_kind: &str,
) -> Result<DispatchAuthorization> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        handle_operator_command(
            &bootstrap,
            None,
            ControllerRequest {
                command: "operator_command".to_string(),
                file: Some(file.to_path_buf()),
                session_id: None,
                pane_id: None,
                window_id: None,
                generation: None,
                state: None,
                caller: None,
                reason: None,
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: Some(command_kind.to_string()),
                diagnostic_payload: Some("session operator command".to_string()),
            },
        )
    }

    #[cfg(not(any(test, feature = "test-support")))]
    request_controller(
        project_root,
        ControllerRequest {
            command: "operator_command".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some(command_kind.to_string()),
            diagnostic_payload: Some("session operator command".to_string()),
        },
    )
}

pub fn request_supervisor_replacement(
    project_root: &Path,
    request: SupervisorReplacementRequest,
) -> Result<SupervisorReplacementReceipt> {
    let diagnostic_payload = serde_json::json!({
        "force": request.force,
        "mode": request.mode.clone(),
        "caller": "session_restart_supervisor",
    })
    .to_string();
    request_controller(
        project_root,
        ControllerRequest {
            command: "supervisor_replacement".to_string(),
            file: Some(request.file),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: Some(request.mode),
            caller: Some("session".to_string()),
            reason: Some(if request.force {
                "operator_force_request".to_string()
            } else {
                "operator_request".to_string()
            }),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(diagnostic_payload),
        },
    )
}

pub fn status(project_root: &Path) -> Result<ControllerStatus> {
    match request(project_root, "status") {
        Ok(response) => {
            let mut status: ControllerStatus = serde_json::from_str(&response)
                .context("failed to parse project controller status response")?;
            status.active = true;
            status.stale_duplicate_pids = discover_stale_duplicate_pids(project_root, status.pid);
            status.freshness = Some(status::controller_freshness_status(
                controller_freshness_facts(status.pid, None),
            ));
            Ok(status)
        }
        Err(_) => {
            let bootstrap = read_bootstrap(project_root)?;
            let bootstrap_facts = bootstrap.as_ref().map(controller_bootstrap_status_facts);
            let controller_pid = bootstrap_facts.as_ref().map(|state| state.pid);
            Ok(status::inactive_controller_status(
                project_root,
                socket_path(project_root),
                bootstrap_facts.as_ref(),
                discover_stale_duplicate_pids(project_root, None),
                status::controller_freshness_status(controller_freshness_facts(
                    controller_pid,
                    None,
                )),
                status::control_plane_status(
                    false,
                    control_plane_store_counts(project_root)?,
                    None,
                ),
            ))
        }
    }
}

/// `#fccsupwarn2` — IO check for the route-owned HOST supervisor that serves `file`.
///
/// THE GAP behind `#fccsupwarn`: `stale_supervisor_warning_for_doc` only inspected the
/// lazy controller's recorded binary. In a live session the controller was fresh (its
/// recorded identity matched the new install → no warning) while the long-lived
/// route-owned host supervisor was hours stale and silently kept producing the dialogs.
/// This check resolves the host supervisor PID for `file` via the authoritative actor
/// binding + supervisor lease, then compares the inode it maps via `/proc/<pid>/exe`
/// against the installed binary's inode via
/// [`agent_doc_supervisor::config::host_supervisor_is_stale`] — so a supervisor
/// that re-exec'd onto the fresh binary in place reads fresh.
///
/// Fully fail-open: a missing project root, no authoritative binding, no live supervisor
/// PID, a dead PID, an unreadable `/proc/<pid>`, or any stat error yields `None` so this
/// read-only check can never block a live cycle.
pub(crate) fn host_supervisor_stale_warning_for_doc(file: &Path) -> Option<String> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
    let record = durable_actor_binding(&project_root, file).ok().flatten()?;
    let conn = open_state_db(&project_root).ok()?;
    let lease = load_supervisor_lease_from_db(&conn, &record.document_id, record.generation)
        .ok()
        .flatten()?;
    let supervisor_pid = lease.supervisor_pid?;
    if !process_is_alive(supervisor_pid) {
        return None;
    }
    // `#fccsupwarn2`: compare the supervisor's RUNNING binary inode (via
    // `/proc/<pid>/exe`) against the installed binary's inode. A supervisor that
    // hot-reloaded onto the fresh binary in place (`execve`) maps the install inode and
    // must read FRESH even though its process start time predates the install.
    let installed_inode = agent_doc_fs::inode_of_path(&current_binary_identity().ok()?.path)?;
    let running_inode = agent_doc_fs::running_exe_inode_for_pid(supervisor_pid);
    if !agent_doc_supervisor::config::host_supervisor_is_stale(running_inode, installed_inode) {
        return None;
    }
    Some(status::host_supervisor_stale_warning_message(
        supervisor_pid,
    ))
}

/// `#fccsupwarn`/`#fccsupwarn2` — IO wrapper: resolve the live processes hosting `file`
/// and return a stale-binary warning if EITHER the lazy controller OR the route-owned
/// host supervisor is serving a stale build. The controller check (`#fccsupwarn`) covers
/// the in-process / handoff path; the host-supervisor check (`#fccsupwarn2`) covers the
/// separate long-lived `agent-doc start --route-owned` process that actually writes the
/// document and is the common silent-stale offender. Fail-open — a missing project root,
/// an unreachable controller, a missing lease, or any stat error yields `None` so the
/// read-only check can never block a cycle.
pub fn stale_supervisor_warning_for_doc(file: &Path) -> Option<String> {
    if let Some(message) = host_supervisor_stale_warning_for_doc(file) {
        return Some(message);
    }
    if !reliable_sync_editor_live_for_file(file) {
        return None;
    }
    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
    if let Ok(controller_status) = status(&project_root) {
        let current_binary = current_binary_identity().ok();
        if let Some(message) =
            status::supervisor_stale_warning_message(&controller_status, current_binary.as_ref())
        {
            return Some(message);
        }
    }
    None
}

/// Detect a stale route-owned supervisor at a turn stage and unconditionally
/// schedule its safe idle-boundary recycle.
///
/// A proven stale binary is an integrity condition, not a preference: the
/// ordinary auto-recycle opt-out controls proactive recycling, but cannot leave
/// a known-stale supervisor serving later generation/write/commit stages.
pub fn recycle_stale_supervisor_for_turn_stage(file: &Path, stage: &str) -> Option<String> {
    let (mut message, recycle_status) = if let Some(message) =
        stale_supervisor_warning_for_doc(file)
    {
        (message, schedule_stale_supervisor_cp_recycle(file, stage))
    } else if reliable_sync_editor_live_for_file(file)
        && matches!(
            agent_doc_crdt_relay_io::current_text_for_file_nonblocking(file),
            Ok(agent_doc_crdt_relay_io::CurrentText::Current {
                live_editors: 0,
                ..
            })
        )
    {
        (
            format!(
                "route-owned editor authority for {} has zero registered relay replicas; the serving supervisor/editor bridge is stale",
                file.display()
            ),
            schedule_stale_editor_replica_cp_recycle(file, stage),
        )
    } else {
        return None;
    };
    message.push_str(&format!(
        " Automatic safe-boundary recycle request status: {recycle_status}."
    ));
    Some(message)
}

/// `#fccsupwarn4` — preflight stale-supervisor self-heal.
///
/// The warning probe above is deliberately read-only because status/doctor callers
/// should not mutate live supervisor state. Preflight is different: it has just
/// proven that the document's serving supervisor maps an old binary, and the
/// non-destructive repair is to ask the owner to recycle at the next idle boundary.
/// This helper owns that effect through the same recycle-request marker consumed by
/// the CP/supervisor recycle graph. Fail-open: every refusal is logged and returned
/// as a status string, never raised into the live cycle.
pub fn schedule_stale_supervisor_cp_recycle(file: &Path, source: &str) -> String {
    schedule_supervisor_cp_recycle(
        file,
        source,
        agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_STALE_SUPERVISOR_TURN_STAGE,
        "stale_supervisor_cp_recycle_requested",
        "supervisor_binary_stale",
    )
}

/// Schedule recovery when an editor-owned document has lost its relay member or
/// when the canonical document and its disk projection diverge after closeout.
///
/// Re-registering the editor replica is the primary repair. Recycling an already
/// current supervisor cannot create that editor-owned membership and used to
/// produce an execve storm while the same recovery event was still in flight.
/// Retain supervisor recycle only as the fallback when the controller cannot
/// publish the editor event.
pub fn schedule_stale_editor_replica_cp_recycle(file: &Path, source: &str) -> String {
    match agent_doc_crdt_relay_io::signal_crdt_replica_event_with_counts(
        file,
        agent_doc_crdt_relay_io::CrdtReplicaEventReason::AckRecoveryForceRefresh,
        0,
    ) {
        Ok(outcome) => {
            // `#mrnh` / `#ghosteditorliveness`: the unit-form signal returns `Ok(())`
            // even when the plane holds ZERO live registrations, and callers logged
            // that as `editor_replica_reregister=requested` — a phantom "recovery is
            // pending" that never converges because there is no editor to re-register.
            // Report the counted outcome instead: `no_live_registration` when nothing
            // exists to nudge, so session-check stops implying an automatic
            // re-registration is in flight and falls through to disk/committed
            // authority.
            let status = if outcome.found == 0 {
                "request_skipped reason=no_live_editor editor_replica_reregister=no_live_registration"
                    .to_string()
            } else {
                format!(
                    "request_skipped reason=editor_reregister_primary editor_replica_reregister={}",
                    outcome.diagnosis()
                )
            };
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "stale_editor_replica_recovery_requested file={} source={} action=reregister_editor_replica request_status={} reason=editor_authority_unavailable_or_diverged",
                    file.display(),
                    source,
                    status,
                ),
            );
            status
        }
        Err(err) => {
            let fallback = schedule_supervisor_cp_recycle(
                file,
                source,
                agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_STALE_EDITOR_REPLICA_TURN_STAGE,
                "stale_editor_replica_cp_recycle_requested",
                "editor_authority_unavailable_or_diverged",
            );
            format!(
                "editor_replica_reregister=failed:{} fallback={fallback}",
                format!("{err:#}").replace('\n', "\\n")
            )
        }
    }
}

fn schedule_supervisor_cp_recycle(
    file: &Path,
    source: &str,
    reason: &str,
    event: &str,
    log_reason: &str,
) -> String {
    let request_status =
        if let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) {
            let project_root_display = if project_root.as_os_str().is_empty() {
                ".".to_string()
            } else {
                project_root.display().to_string()
            };
            match checkpoint_route_owned_document_crdt(file, "stale_supervisor_cp_recycle") {
                Ok(checkpoint_status) => {
                    match agent_doc_supervisor_io::recycle_request::request_recycle_for_doc(
                        file, reason,
                    ) {
                        Ok(()) => format!(
                            "requested project_root={} checkpoint={}",
                            project_root_display,
                            checkpoint_status.as_deref().unwrap_or("ok")
                        ),
                        Err(err) => format!(
                            "request_failed project_root={} error={}",
                            project_root_display,
                            format!("{err:#}").replace('\n', "\\n")
                        ),
                    }
                }
                Err(err) => format!(
                    "request_skipped project_root={} reason=crdt_checkpoint_error error={}",
                    project_root_display,
                    format!("{err:#}").replace('\n', "\\n")
                ),
            }
        } else {
            "request_skipped reason=no_project_root".to_string()
        };
    let reregister_status = match agent_doc_crdt_relay_io::signal_crdt_replica_event(
        file,
        agent_doc_crdt_relay_io::CrdtReplicaEventReason::AckRecoveryForceRefresh,
        0,
    ) {
        Ok(()) => "requested".to_string(),
        Err(err) => format!("failed:{err}"),
    };
    let recovery_status = format!("{request_status} editor_replica_reregister={reregister_status}");
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{event} file={} source={} action=request_recycle_through_owner request_status={} reason={log_reason}",
            file.display(),
            source,
            recovery_status
        ),
    );
    recovery_status
}

/// `#ctlrecycle` — idle grace before a stale/recycle-requested process actually
/// recycles. A process must observe "wants-recycle AND idle" continuously for this
/// long so a brief lull between queue items never triggers a recycle. Override with
/// `AGENT_DOC_RECYCLE_IDLE_GRACE_SECS`.
pub fn recycle_idle_grace() -> Duration {
    let secs = std::env::var(RECYCLE_IDLE_GRACE_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_RECYCLE_IDLE_GRACE_SECS);
    Duration::from_secs(secs)
}

/// `#supautoinstall` — resolve the agent-doc crate source root for a DOGFOODING session
/// (an agent-doc session editing agent-doc's own source). A superproject may contain
/// `src/agent-doc` while also hosting unrelated project documents; those documents must not
/// inherit dogfood build/install policy just because the crate is nearby.
pub fn dogfood_agent_doc_crate_root(file: &Path) -> Option<PathBuf> {
    let file = file.canonicalize().ok()?;
    let project_root = agent_doc_project_root_io::project_root_containing(&file)?;
    for candidate in [project_root.clone(), project_root.join("src/agent-doc")] {
        let cargo = candidate.join("Cargo.toml");
        if let Ok(content) = std::fs::read_to_string(&cargo)
            && content.contains("name = \"agent-doc\"")
            && agent_doc_supervisor::config::is_agent_doc_dogfood_session(
                &file,
                &project_root,
                &candidate,
            )
        {
            return Some(candidate);
        }
    }
    None
}

/// `#supautoinstall` — run the dogfood local install for agent-doc's own source
/// from `crate_root`. Runs IN THE SUPERVISOR at an idle boundary (never the
/// finalize client mid-cycle), which is what root-fixes the mid-session-install
/// drift. After it succeeds the installed binary is newer than the running
/// supervisor process, so the existing `process_binary_is_stale` recycle path
/// hot-reloads onto it. Returns `Err` naming the failed step.
pub fn run_supervisor_auto_install(crate_root: &Path) -> Result<()> {
    run_supervisor_auto_install_with_retry(
        crate_root,
        AUTO_INSTALL_MAX_ATTEMPTS,
        Duration::from_secs(AUTO_INSTALL_RETRY_BACKOFF_SECS),
    )
}

/// Auto-install promotion is allowed only from a stable committed checkout.
/// Manual `make install` remains available for deliberate dirty-tree dogfooding,
/// but the background supervisor must not publish half-edited source and recycle
/// the live fleet while an author is still typing.
pub fn supervisor_auto_install_worktree_clean(crate_root: &Path) -> Result<bool> {
    let output = std::process::Command::new("git")
        .args([
            "status",
            "--porcelain",
            "--untracked-files=normal",
            "--",
            ".",
        ])
        .current_dir(crate_root)
        .output()
        .with_context(|| {
            format!(
                "spawn git status for supervisor auto-install source {}",
                crate_root.display()
            )
        })?;
    if !output.status.success() {
        anyhow::bail!(
            "git status failed for supervisor auto-install source {}: {}",
            crate_root.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout.iter().all(u8::is_ascii_whitespace))
}

/// `#autoinstallretry` — number of times to attempt the auto-install step sequence
/// before falling back to operator refresh, and the backoff between attempts. The
/// `make install` build step is most failures' culprit and is almost always
/// TRANSIENT: the supervisor builds from the live working tree, so it can catch
/// a mid-edit non-compiling window (an agent/operator part-way through a
/// multi-file edit) or lose a cargo build/target lock to a concurrent
/// `make check`. A bounded retry with backoff rescues both — by the next
/// attempt the edit has committed (edits land atomically) and the lock has
/// freed — instead of giving up after one failure and stalling the installed
/// binary at the last good commit (which is what produced the false
/// "stale binary, install manually" handoffs).
const AUTO_INSTALL_MAX_ATTEMPTS: u32 = 3;
const AUTO_INSTALL_RETRY_BACKOFF_SECS: u64 = 20;

/// `#restartstderrbleed` — materialize the supervisor-owned auto-install stdio
/// policy into [`std::process::Stdio`] values for a child process.
///
/// Why this exists: the route-owned supervisor renders the agent TUI into its own
/// tmux pane via **fd1 (stdout)**, while only **fd2 (stderr)** is process-wide
/// redirected to `.agent-doc/logs/supervisor-stderr.log` (`SupervisorStderrRedirect`).
/// A `make install` child spawned with inherited stdio therefore sends `make`'s
/// unsuppressed recipe echo (`cargo install --path ...`) and any cargo stdout
/// straight to fd1 — i.e. straight into the agent pane, corrupting the live TUI /
/// prompt during a supervisor/CP restart. Pointing the child's stdout at a dup of
/// the supervisor's stderr keeps that output on the same channel the rest of the
/// build diagnostics already use (the redirected log when route-owned), and never
/// on fd1. Nulling stdin stops a build sub-process from consuming forwarded
/// operator keystrokes.
fn auto_install_child_stdio_from_plan(
    plan: agent_doc_supervisor::auto_install_stdio::AutoInstallChildStdioPlan,
) -> (
    std::process::Stdio,
    std::process::Stdio,
    std::process::Stdio,
) {
    (
        auto_install_stream_from_plan(plan.stdin),
        auto_install_stream_from_plan(plan.stdout),
        auto_install_stream_from_plan(plan.stderr),
    )
}

fn auto_install_stream_from_plan(
    stream: agent_doc_supervisor::auto_install_stdio::AutoInstallStdioStream,
) -> std::process::Stdio {
    use agent_doc_supervisor::auto_install_stdio::AutoInstallStdioStream;

    match stream {
        AutoInstallStdioStream::Null => std::process::Stdio::null(),
        AutoInstallStdioStream::Inherit => std::process::Stdio::inherit(),
        #[cfg(unix)]
        AutoInstallStdioStream::DuplicateFd(target_fd) => auto_install_stream_dup_fd(target_fd),
    }
}

#[cfg(unix)]
fn auto_install_stream_dup_fd(target_fd: std::os::fd::RawFd) -> std::process::Stdio {
    use std::os::fd::FromRawFd;
    use std::process::Stdio;

    // NEVER inherit: a failed dup falls back to a discard sink, not fd1.
    // CLOEXEC: the dup is only for this Command; it must not survive a
    // supervisor self-execve or it leaks one log-fd per recycle.
    let fd = agent_doc_supervisor_process::pty::dup_cloexec(target_fd);
    match fd {
        Ok(fd) => unsafe { Stdio::from_raw_fd(fd) },
        Err(_) => Stdio::null(),
    }
}

/// Open the supervisor stderr log for auto-install child output
/// (`#restartbleednonroute`). `None` when no project root resolves or the file
/// cannot be opened, in which case the caller keeps the fd2 plan.
#[cfg(unix)]
fn auto_install_stderr_log_file(crate_root: &Path) -> Option<std::fs::File> {
    let project_root = agent_doc_project_root_io::project_root_containing(crate_root)?;
    let path =
        agent_doc_supervisor_process::start_command::route_owned_stderr_log_path(&project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
}

/// Run the auto-install sequence ONCE through `make install`. The Makefile owns
/// the local-dev profile, incremental target dir, linker selection, and cdylib
/// install flags. The target is idempotent, so retrying it is safe.
fn run_auto_install_steps_once(crate_root: &Path) -> Result<()> {
    let steps: [(&str, &[&str]); 1] = [("make", &["install"])];
    // `#restartbleednonroute`: prefer an explicit LOG fd over fd2.
    //
    // The default plan dups fd2, which is off-pane only while
    // `SupervisorStderrRedirect` is active. On a non-route-owned TUI — or a
    // route-owned supervisor whose redirect fell back to inactive on error —
    // fd2 IS the agent pane, so `make install` output bleeds straight into the
    // live session. Holding the log file open for the whole sequence makes the
    // child's stdio independent of route ownership; fd2 remains the fallback so
    // a log that cannot be opened degrades to today's behavior rather than
    // discarding build output.
    #[cfg(unix)]
    let stderr_log = auto_install_stderr_log_file(crate_root);
    for (program, args) in steps {
        // `#restartstderrbleed` — never inherit stdio: fd1 is the agent pane.
        #[cfg(unix)]
        let plan = match stderr_log.as_ref() {
            Some(file) => {
                use std::os::fd::AsRawFd;
                agent_doc_supervisor::auto_install_stdio::auto_install_child_stdio_plan_to_fd(
                    file.as_raw_fd(),
                )
            }
            None => agent_doc_supervisor::auto_install_stdio::auto_install_child_stdio_plan(),
        };
        #[cfg(not(unix))]
        let plan = agent_doc_supervisor::auto_install_stdio::auto_install_child_stdio_plan();
        let (stdin, stdout, stderr) = auto_install_child_stdio_from_plan(plan);
        let status = std::process::Command::new(program)
            .args(args)
            .current_dir(crate_root)
            // `make install` normally owns the one fleet-wide recycle wave.
            // A supervisor auto-install coordinates that wave below so it can
            // mark supervisors before controllers and avoid marking the fleet
            // twice (the live crash/relaunch storm reproduced this double fanout).
            .env("AGENT_DOC_RECYCLE_ON_INSTALL", "0")
            .stdin(stdin)
            .stdout(stdout)
            .stderr(stderr)
            .status()
            .with_context(|| format!("failed to spawn `{program} {}`", args.join(" ")))?;
        if !status.success() {
            anyhow::bail!(
                "auto-install step `{program} {}` failed with status {status}",
                args.join(" ")
            );
        }
    }
    // `#installhandoff`: exactly one coordinated recycle wave. Mark supervisors
    // first so every document has a durable handoff request before controllers
    // begin recycling. The initiating supervisor is included and self-execves at
    // its idle boundary; live harness children remain attached to their PTYs.
    match recycle_supervisors_all_projects() {
        Ok((marked, skipped)) => {
            if marked > 0 || skipped > 0 {
                eprintln!(
                    "[agent-doc] supervisor auto-install handoff: {marked} route-owned supervisor(s) marked before controller recycle ({skipped} skipped)"
                );
            }
        }
        Err(err) => {
            eprintln!(
                "[agent-doc] warning: supervisor recycle fan-out after auto-install failed: {err:#}"
            );
        }
    }
    match recycle_controllers_all_projects() {
        Ok((marked, skipped)) => {
            if marked > 0 || skipped > 0 {
                eprintln!(
                    "[agent-doc] supervisor auto-install handoff: {marked} controller(s) marked after supervisor handoff ({skipped} skipped)"
                );
            }
        }
        Err(err) => {
            eprintln!(
                "[agent-doc] warning: controller recycle fan-out after supervisor auto-install failed: {err:#}"
            );
        }
    }
    Ok(())
}

/// `#autoinstallretry` — retry the auto-install sequence up to `max_attempts`,
/// sleeping `backoff` between attempts, so a transient `make install` failure
/// (mid-edit working tree / build-lock contention) self-heals instead of
/// stalling the installed binary. Returns the LAST attempt's error after
/// exhausting retries (the caller then falls back to operator refresh).
fn run_supervisor_auto_install_with_retry(
    crate_root: &Path,
    max_attempts: u32,
    backoff: Duration,
) -> Result<()> {
    let mut last_err = None;
    for attempt in 1..=max_attempts.max(1) {
        match run_auto_install_steps_once(crate_root) {
            Ok(()) => return Ok(()),
            Err(err) => {
                if agent_doc_supervisor::config::auto_install_should_retry(attempt, max_attempts) {
                    eprintln!(
                        "[agent-doc] supervisor auto-install attempt {attempt}/{max_attempts} failed ({err}); retrying in {}s (#autoinstallretry — transient build state: mid-edit working tree or build-lock contention usually clears by the next attempt)",
                        backoff.as_secs()
                    );
                    last_err = Some(err);
                    std::thread::sleep(backoff);
                } else {
                    return Err(err);
                }
            }
        }
    }
    // Unreachable in practice (the loop returns on the final attempt), but keep a
    // total fallback so the signature stays honest.
    Err(last_err.unwrap_or_else(|| {
        anyhow::anyhow!("auto-install exhausted retries with no recorded error")
    }))
}

fn resume_spent_preset_pause(
    project_root: &Path,
    file: &Path,
    document_id: &str,
    reason: &str,
) -> Result<()> {
    resume_document_queue_control(project_root, file, document_id, reason)
}

fn resume_document_queue_control(
    project_root: &Path,
    file: &Path,
    document_id: &str,
    reason: &str,
) -> Result<()> {
    let conn = open_state_db(project_root)?;
    state_store::upsert_queue_control_in_db(
        &conn,
        &state_store::QueueControlInsert {
            scope_kind: "document",
            scope_id: document_id,
            state: "resumed",
            reason: Some(reason),
            operation_receipt_id: None,
        },
    )?;
    agent_doc_ops_log_io::log_op(file, reason);
    Ok(())
}

fn clear_superseded_stale_supervisor_pause(
    project_root: &Path,
    file: &Path,
    document_id: &str,
    record: &agent_doc_sqlite::state_store::ActorRecord,
    control: &QueueControlStatus,
) -> Result<bool> {
    if control.scope_kind != "document"
        || control.scope_id != document_id
        || control.state != "paused"
    {
        return Ok(false);
    }
    let Some(reason) = control.reason.as_deref() else {
        return Ok(false);
    };
    if !pause_reason_is_stale_supervisor_churn_stop(reason) {
        return Ok(false);
    }
    if matches!(
        record.state,
        agent_doc_sqlite::state_store::ActorState::Blocked
            | agent_doc_sqlite::state_store::ActorState::Closed
    ) {
        return Ok(false);
    }

    let conn = open_state_db(project_root)?;
    let lease = load_supervisor_lease_from_db(&conn, document_id, record.generation)?;
    let stale_pid = stale_supervisor_pid_from_pause_reason(reason);
    let current_pid = lease.as_ref().and_then(|lease| lease.supervisor_pid);
    let stale_pid_dead = stale_pid.is_some_and(|pid| !process_is_alive(pid));
    let stale_pid_dead_after_reboot = stale_pid_dead
        && queue_pause_predates_boot(
            control.updated_at,
            crate::process::system_boot_timestamp_secs(timestamp_secs()),
        );
    let superseded_by_actor_transition = record.last_transition.prior_generation
        < record.generation
        && record.last_transition.new_generation == record.generation
        && record.last_transition.timestamp >= control.updated_at;
    let superseded_by_supervisor_pid =
        stale_pid.is_some() && current_pid.is_some() && stale_pid != current_pid;
    if !superseded_by_actor_transition
        && !superseded_by_supervisor_pid
        && !stale_pid_dead_after_reboot
    {
        return Ok(false);
    }

    resume_document_queue_control(
        project_root,
        file,
        document_id,
        &format!(
            "stale_supervisor_pause_superseded file={} stale_pid={} stale_pid_dead={} pause_predates_boot={} current_pid={} session={} pane={} generation={} actor_transition_at={} control_updated_at={} result=cleared",
            file.display(),
            stale_pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            stale_pid_dead,
            stale_pid_dead_after_reboot,
            current_pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            record.session_id,
            record.pane_id,
            record.generation,
            record.last_transition.timestamp,
            control.updated_at,
        ),
    )?;
    Ok(true)
}

fn repair_spent_preset_pause_before_dispatch(
    project_root: &Path,
    file: &Path,
    document_id: &str,
    control: &QueueControlStatus,
) -> Result<bool> {
    if control.state != "paused" {
        return Ok(false);
    }
    let Some(reason) = control.reason.as_deref() else {
        return Ok(false);
    };
    let Some(preset_id) = spent_preset_id_from_pause_reason(reason) else {
        return Ok(false);
    };
    let content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "spent-preset pause repair: failed to read {}",
            file.display()
        )
    })?;
    if !agent_doc_queue::queue_response::active_queue_head_is_registered_preset(
        &content, &preset_id,
    )? {
        resume_spent_preset_pause(
            project_root,
            file,
            document_id,
            &format!(
                "spent_preset_pause_repaired file={} preset=#{} action=resume_absent_head result=cleared",
                file.display(),
                preset_id
            ),
        )?;
        return Ok(true);
    }

    let outcome = runtime_effects()?
        .consume_queue_prompt_force_disk(file)
        .with_context(|| {
            format!(
                "spent-preset pause repair: failed to consume #{}",
                preset_id
            )
        })?;
    if let Some(outcome) = outcome {
        resume_spent_preset_pause(
            project_root,
            file,
            document_id,
            &format!(
                "spent_preset_pause_repaired file={} preset=#{} action=consume_head consumed={:?} remaining={} drained={} result=cleared",
                file.display(),
                preset_id,
                outcome.consumed_text,
                outcome.remaining,
                outcome.drained
            ),
        )?;
        return Ok(true);
    }
    Ok(false)
}

pub(crate) fn discover_stale_duplicate_pids(
    project_root: &Path,
    authoritative_pid: Option<u32>,
) -> Vec<u32> {
    let mut pids = BTreeSet::new();
    if let Ok(Some(state)) = read_bootstrap(project_root) {
        if Some(state.pid) != authoritative_pid {
            pids.insert(state.pid);
        }
        if let Some(pid) = state.previous_controller_pid
            && Some(pid) != authoritative_pid
        {
            pids.insert(pid);
        }
    }

    for pid in crate::process::project_controller_pids(project_root) {
        if Some(pid) == authoritative_pid || pid == std::process::id() {
            continue;
        }
        pids.insert(pid);
    }

    pids.retain(|pid| {
        Some(*pid) != authoritative_pid && *pid != std::process::id() && process_is_alive(*pid)
    });
    pids.into_iter().collect()
}

pub(crate) fn reap_verified_controller_pid(project_root: &Path, pid: u32, generation: u64) {
    if pid == std::process::id() || !is_same_project_controller_pid(project_root, pid) {
        return;
    }
    crate::process::send_signal(pid, crate::process::ProcessSignal::Term);
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(750) {
        if !process_is_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    if is_same_project_controller_pid(project_root, pid) {
        crate::process::send_signal(pid, crate::process::ProcessSignal::Kill);
        eprintln!(
            "[controller] reaped stale same-project controller pid={pid} generation={generation}"
        );
    }
}

pub(crate) fn reap_stale_duplicate_controllers(
    project_root: &Path,
    authoritative_pid: Option<u32>,
    generation: u64,
) {
    for pid in discover_stale_duplicate_pids(project_root, authoritative_pid) {
        reap_verified_controller_pid(project_root, pid, generation);
    }
}

pub(crate) fn shutdown_stale_controller(project_root: &Path) {
    let _ = request_with_reason(project_root, "shutdown", "stale_controller_replacement");
    let start = Instant::now();
    while start.elapsed() < CONNECT_WAIT {
        if connect(project_root).is_err() {
            return;
        }
        std::thread::sleep(CONNECT_POLL);
    }
}

/// `#lazily-recycle-request` — record a pending recycle/restart REQUEST on the
/// lazily statechart (phase `Requested`). This is the durable, subscribable
/// replacement for the on-disk `recycle_request` marker: producers dual-write it
/// alongside the marker; route callers observe the phase via the state
/// subscription instead of polling the marker file.
pub fn supervisor_recycle_requested(
    project_root: &Path,
    reason: &str,
) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
    ensure_controller_running(project_root, LaunchMode::Lazy)?;
    request_controller(
        project_root,
        ControllerRequest {
            command: "supervisor_recycle_requested".to_string(),
            file: None,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("supervisor".to_string()),
            reason: Some(reason.to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn supervisor_recycle_requested_for_file(
    file: &Path,
    reason: &str,
) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    supervisor_recycle_requested(&project_root, reason)
}

pub fn supervisor_recycle_started(
    project_root: &Path,
    reason: &str,
) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
    ensure_controller_running(project_root, LaunchMode::Lazy)?;
    request_controller(
        project_root,
        ControllerRequest {
            command: "supervisor_recycle_started".to_string(),
            file: None,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("supervisor".to_string()),
            reason: Some(reason.to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn supervisor_recycle_started_for_file(
    file: &Path,
    reason: &str,
) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    supervisor_recycle_started(&project_root, reason)
}

pub fn supervisor_recycle_settled(
    project_root: &Path,
    reason: &str,
) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
    ensure_controller_running(project_root, LaunchMode::Lazy)?;
    request_controller(
        project_root,
        ControllerRequest {
            command: "supervisor_recycle_settled".to_string(),
            file: None,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("supervisor".to_string()),
            reason: Some(reason.to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn supervisor_recycle_settled_for_file(
    file: &Path,
    reason: &str,
) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    supervisor_recycle_settled(&project_root, reason)
}

pub fn supervisor_recycle_status(
    project_root: &Path,
) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
    if connect(project_root).is_err() {
        return Ok(agent_doc_state_backbone::SupervisorRecycleProjection::default());
    }
    request_controller(
        project_root,
        ControllerRequest {
            command: "supervisor_recycle_status".to_string(),
            file: None,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("route".to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn supervisor_recycle_status_for_file(
    file: &Path,
) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return Ok(agent_doc_state_backbone::SupervisorRecycleProjection::default());
    };
    supervisor_recycle_status(&project_root)
}

pub fn supervisor_recycle_pending(project_root: &Path) -> bool {
    supervisor_recycle_status(project_root)
        .map(|projection| {
            matches!(
                projection.phase,
                agent_doc_state_backbone::SupervisorRecyclePhase::InFlight
            )
        })
        .unwrap_or(false)
}

pub fn supervisor_recycle_pending_for_file(file: &Path) -> bool {
    supervisor_recycle_status_for_file(file)
        .map(|projection| {
            matches!(
                projection.phase,
                agent_doc_state_backbone::SupervisorRecyclePhase::InFlight
            )
        })
        .unwrap_or(false)
}

fn supervisor_recycle_reason_is_yield(reason: Option<&str>) -> bool {
    matches!(
        reason,
        Some(agent_doc_supervisor::recycle_yield::RECYCLE_YIELD_STALE_BINARY)
            | Some(agent_doc_supervisor::recycle_yield::RECYCLE_YIELD_STATE_FLUSH)
    )
}

/// Ask the in-session loop to yield one boundary through the CP recycle graph.
///
/// This replaces the legacy `.agent-doc/recycle-yield` sidecar. A yield is a
/// pre-recycle in-flight phase: route callers defer through the same lazily-backed
/// projection, while queue/preflight/session-check surfaces suppress unattended
/// continuation until the projection settles.
pub fn supervisor_recycle_yield_requested_for_file(
    file: &Path,
    reason: &str,
) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
    supervisor_recycle_started_for_file(file, reason)
}

pub fn supervisor_recycle_yield_pending_for_file(file: &Path) -> bool {
    supervisor_recycle_status_for_file(file)
        .map(|projection| {
            matches!(
                projection.phase,
                agent_doc_state_backbone::SupervisorRecyclePhase::InFlight
            ) && supervisor_recycle_reason_is_yield(projection.reason.as_deref())
        })
        .unwrap_or(false)
}

pub fn clear_supervisor_recycle_yield_for_file(file: &Path) -> Result<bool> {
    let projection = supervisor_recycle_status_for_file(file)?;
    if !matches!(
        projection.phase,
        agent_doc_state_backbone::SupervisorRecyclePhase::InFlight
    ) || !supervisor_recycle_reason_is_yield(projection.reason.as_deref())
    {
        return Ok(false);
    }
    supervisor_recycle_settled_for_file(file, "recycle_yield_cleared")?;
    Ok(true)
}

pub struct RouteSubmitProjectionGuard {
    file: PathBuf,
    pane: String,
    harness: String,
    reason: String,
    submit_epoch: u64,
}

impl Drop for RouteSubmitProjectionGuard {
    fn drop(&mut self) {
        let _ = route_submit_settled_for_file(
            &self.file,
            &self.pane,
            &self.harness,
            &self.reason,
            self.submit_epoch,
        );
    }
}

pub fn begin_route_submit(
    file: &Path,
    pane: &str,
    harness: &str,
) -> Result<RouteSubmitProjectionGuard> {
    begin_route_submit_with_reason(
        file,
        pane,
        harness,
        agent_doc_state_backbone::ROUTE_DISPATCH_SUBMIT_REASON,
    )
}

pub fn begin_route_submit_with_reason(
    file: &Path,
    pane: &str,
    harness: &str,
    reason: &str,
) -> Result<RouteSubmitProjectionGuard> {
    let projection = route_submit_started_for_file(file, pane, harness, reason)?;
    Ok(RouteSubmitProjectionGuard {
        file: file.to_path_buf(),
        pane: pane.to_string(),
        harness: harness.to_string(),
        reason: reason.to_string(),
        submit_epoch: projection.submit_epoch,
    })
}

pub fn route_submit_started_for_file(
    file: &Path,
    pane: &str,
    harness: &str,
    reason: &str,
) -> Result<agent_doc_state_backbone::RouteSubmitProjection> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    ensure_controller_running(&project_root, LaunchMode::Lazy)?;
    request_controller(
        &project_root,
        ControllerRequest {
            command: "route_submit_started".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: Some(pane.to_string()),
            window_id: None,
            generation: None,
            state: None,
            caller: Some("route".to_string()),
            reason: Some(reason.to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some(harness.to_string()),
            diagnostic_payload: None,
        },
    )
}

pub fn route_submit_settled_for_file(
    file: &Path,
    pane: &str,
    harness: &str,
    reason: &str,
    submit_epoch: u64,
) -> Result<agent_doc_state_backbone::RouteSubmitProjection> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    if connect(&project_root).is_err() {
        return Ok(agent_doc_state_backbone::RouteSubmitProjection::default());
    }
    request_controller(
        &project_root,
        ControllerRequest {
            command: "route_submit_settled".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: Some(pane.to_string()),
            window_id: None,
            generation: Some(submit_epoch),
            state: None,
            caller: Some("route".to_string()),
            reason: Some(reason.to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some(harness.to_string()),
            diagnostic_payload: None,
        },
    )
}

pub fn mark_route_submit_blocked(
    file: &Path,
    pane: &str,
    harness: &str,
    reason: &str,
) -> Result<agent_doc_state_backbone::RouteSubmitProjection> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    ensure_controller_running(&project_root, LaunchMode::Lazy)?;
    request_controller(
        &project_root,
        ControllerRequest {
            command: "route_submit_blocked".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: Some(pane.to_string()),
            window_id: None,
            generation: None,
            state: None,
            caller: Some("route".to_string()),
            reason: Some(reason.to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some(harness.to_string()),
            diagnostic_payload: None,
        },
    )
}

pub fn route_submit_status_for_file(
    file: &Path,
) -> Result<agent_doc_state_backbone::RouteSubmitProjection> {
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return Ok(agent_doc_state_backbone::RouteSubmitProjection::default());
    };
    if connect(&project_root).is_err() {
        return Ok(agent_doc_state_backbone::RouteSubmitProjection::default());
    }
    request_controller(
        &project_root,
        ControllerRequest {
            command: "route_submit_status".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("supervisor".to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn route_submit_in_flight_for_file(file: &Path) -> Result<bool> {
    route_submit_status_for_file(file).map(|projection| projection.is_pending_at(timestamp_secs()))
}

pub fn wait_for_supervisor_recycle_settle(
    project_root: &Path,
) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
    if connect(project_root).is_err() {
        return Ok(agent_doc_state_backbone::SupervisorRecycleProjection::default());
    }
    request_controller(
        project_root,
        ControllerRequest {
            command: "supervisor_recycle_wait_settled".to_string(),
            file: None,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("route".to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn wait_for_supervisor_recycle_settle_for_file(
    file: &Path,
) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return Ok(agent_doc_state_backbone::SupervisorRecycleProjection::default());
    };
    wait_for_supervisor_recycle_settle(&project_root)
}

pub fn ensure_controller_running_for_file(file: &Path) -> Result<()> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    ensure_controller_running(&project_root, LaunchMode::Lazy)
}

pub use agent_doc_state_backbone::{
    CloseoutOwnerClaimOutcome, CloseoutOwnerClaimRequest, CloseoutOwnerProjection,
};
pub const CLOSEOUT_OWNER_LEASE_SECS: u64 = agent_doc_state_backbone::CLOSEOUT_OWNER_LEASE_SECS;
pub const CLOSEOUT_RECOVERY_LEASE_SECS: u64 =
    agent_doc_state_backbone::CLOSEOUT_RECOVERY_LEASE_SECS;
pub const CLOSEOUT_ROLE_SESSION_CHECK_RECOVERY: &str =
    agent_doc_state_backbone::CLOSEOUT_ROLE_SESSION_CHECK_RECOVERY;

/// Lease duration for a closeout-owner `role` (`#closeoutwaitchurn`).
pub fn closeout_owner_lease_secs(role: &str) -> u64 {
    agent_doc_state_backbone::closeout_owner_lease_secs(role)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CloseoutOwnerReleaseRequest {
    cycle_id: String,
    owner_id: String,
    reason: String,
    released_secs: u64,
}

pub fn new_closeout_owner_id(role: &str) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{role}-{}-{nonce}", std::process::id())
}

/// Claim or refresh closeout ownership through the Lazily document actor over
/// the command plane (`#lzdurablesink`).
///
/// The controller serializes the projection decision and fact append. SQLite is
/// only the actor's persistence substrate and is never read by this client.
pub fn claim_closeout_owner_for_file(
    file: &Path,
    request: CloseoutOwnerClaimRequest,
) -> Result<CloseoutOwnerClaimOutcome> {
    use super::command_plane::{CloseoutOwnerClaimPayload, build_closeout_owner_claim_submit};
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    #[cfg(feature = "test-support")]
    ensure_state_actor_for_tests(&project_root)?;
    #[cfg(not(feature = "test-support"))]
    ensure_controller_running(&project_root, LaunchMode::Lazy)?;
    let document_path = file
        .canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .to_string();
    // A per-call nonce keeps each claim a distinct command (a lease refresh must
    // not dedupe onto a prior command); the CAS idempotency lives in
    // `decide_owner_claim`.
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let command_id = format!(
        "closeout-owner-claim:{}:{}:{nonce}",
        request.expected_cycle_id.as_deref().unwrap_or("current"),
        request.owner_id
    );
    let submit = build_closeout_owner_claim_submit(
        &command_id,
        &command_id,
        0,
        CloseoutOwnerClaimPayload {
            document_path,
            request,
        },
    )?;
    let submit_json = serde_json::to_string(&submit)?;
    request_controller(
        &project_root,
        ControllerRequest::command_plane_submit(submit_json),
    )
}

pub fn release_closeout_owner_for_file(
    file: &Path,
    cycle_id: &str,
    owner_id: &str,
    reason: &str,
) -> Result<bool> {
    use super::command_plane::{CloseoutOwnerReleasePayload, build_closeout_owner_release_submit};
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    ensure_controller_running(&project_root, LaunchMode::Lazy)?;
    let document_path = file
        .canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .to_string();
    let released_secs = timestamp_secs();
    let command_id = format!("closeout-owner-release:{cycle_id}:{owner_id}:{released_secs}");
    let submit = build_closeout_owner_release_submit(
        &command_id,
        &command_id,
        0,
        CloseoutOwnerReleasePayload {
            document_path,
            cycle_id: cycle_id.to_string(),
            owner_id: owner_id.to_string(),
            reason: reason.to_string(),
            released_secs,
        },
    )?;
    let submit_json = serde_json::to_string(&submit)?;
    request_controller(
        &project_root,
        ControllerRequest::command_plane_submit(submit_json),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueueContextClearPayload {
    command: String,
    head_sha256: Option<String>,
    head_bytes: Option<usize>,
}

pub fn queue_context_clear_started_for_file(
    file: &Path,
    target: &str,
    harness: &str,
    command: &str,
    source: &str,
    active_head: Option<&str>,
) -> Result<agent_doc_state_backbone::QueueContextClearProjection> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    ensure_controller_running(&project_root, LaunchMode::Lazy)?;
    let payload = QueueContextClearPayload {
        command: command.to_string(),
        head_sha256: active_head.map(agent_doc_hash::content_hash),
        head_bytes: active_head.map(str::len),
    };
    request_controller(
        &project_root,
        ControllerRequest {
            command: "queue_context_clear_started".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: Some(target.to_string()),
            window_id: None,
            generation: None,
            state: None,
            caller: Some("supervisor".to_string()),
            reason: Some(source.to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some(harness.to_string()),
            diagnostic_payload: Some(serde_json::to_string(&payload)?),
        },
    )
}

pub fn queue_context_clear_manual_cooldown_for_file(
    file: &Path,
    target: &str,
    harness: &str,
    command: &str,
) -> Result<agent_doc_state_backbone::QueueContextClearProjection> {
    queue_context_clear_started_for_file(
        file,
        target,
        harness,
        command,
        agent_doc_state_backbone::QUEUE_CONTEXT_CLEAR_SOURCE_OPERATOR_MANUAL_COOLDOWN,
        None,
    )
}

pub fn queue_context_clear_deferred_for_file(
    file: &Path,
    target: &str,
    harness: &str,
    command: &str,
    source: &str,
    active_head: Option<&str>,
) -> Result<agent_doc_state_backbone::QueueContextClearProjection> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    ensure_controller_running(&project_root, LaunchMode::Lazy)?;
    let payload = QueueContextClearPayload {
        command: command.to_string(),
        head_sha256: active_head.map(agent_doc_hash::content_hash),
        head_bytes: active_head.map(str::len),
    };
    request_controller(
        &project_root,
        ControllerRequest {
            command: "queue_context_clear_deferred".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: Some(target.to_string()),
            window_id: None,
            generation: None,
            state: None,
            caller: Some("supervisor".to_string()),
            reason: Some(source.to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some(harness.to_string()),
            diagnostic_payload: Some(serde_json::to_string(&payload)?),
        },
    )
}

pub fn queue_context_clear_settled_for_file(
    file: &Path,
    target: &str,
    harness: &str,
    command: &str,
    source: Option<&str>,
    clear_epoch: u64,
) -> Result<agent_doc_state_backbone::QueueContextClearProjection> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    if connect(&project_root).is_err() {
        return Ok(agent_doc_state_backbone::QueueContextClearProjection::default());
    }
    let payload = QueueContextClearPayload {
        command: command.to_string(),
        head_sha256: None,
        head_bytes: None,
    };
    request_controller(
        &project_root,
        ControllerRequest {
            command: "queue_context_clear_settled".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: Some(target.to_string()),
            window_id: None,
            generation: Some(clear_epoch),
            state: None,
            caller: Some("supervisor".to_string()),
            reason: source.map(str::to_string),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some(harness.to_string()),
            diagnostic_payload: Some(serde_json::to_string(&payload)?),
        },
    )
}

pub fn queue_context_clear_status_for_file(
    file: &Path,
) -> Result<agent_doc_state_backbone::QueueContextClearProjection> {
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return Ok(agent_doc_state_backbone::QueueContextClearProjection::default());
    };
    if connect(&project_root).is_err() {
        return Ok(agent_doc_state_backbone::QueueContextClearProjection::default());
    }
    request_controller(
        &project_root,
        ControllerRequest {
            command: "queue_context_clear_status".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("supervisor".to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn queue_context_clear_in_flight_for_file(
    file: &Path,
) -> Result<Option<agent_doc_state_backbone::QueueContextClearProjection>> {
    let projection = queue_context_clear_status_for_file(file)?;
    if projection.is_pending_at(timestamp_secs()) {
        Ok(Some(projection))
    } else {
        Ok(None)
    }
}

pub fn queue_context_clear_deferred_operator_for_file(
    file: &Path,
) -> Result<Option<agent_doc_state_backbone::QueueContextClearProjection>> {
    let projection = queue_context_clear_status_for_file(file)?;
    if projection.is_deferred_operator_clear() {
        Ok(Some(projection))
    } else {
        Ok(None)
    }
}

pub fn queue_context_clear_manual_cooldown_active_for_file(
    file: &Path,
) -> Result<Option<agent_doc_state_backbone::QueueContextClearProjection>> {
    let projection = queue_context_clear_status_for_file(file)?;
    if projection.is_manual_operator_clear_cooldown() {
        Ok(Some(projection))
    } else {
        Ok(None)
    }
}

pub fn clear_queue_context_clear_in_flight_for_file(file: &Path) -> Result<bool> {
    let projection = queue_context_clear_status_for_file(file)?;
    if !matches!(
        projection.phase,
        agent_doc_state_backbone::QueueContextClearPhase::InFlight
    ) {
        return Ok(false);
    }
    queue_context_clear_settled_for_file(
        file,
        &projection.target,
        &projection.harness,
        &projection.command,
        projection.source.as_deref(),
        projection.clear_epoch,
    )?;
    Ok(true)
}

pub fn clear_queue_context_clear_manual_cooldown_for_file(file: &Path) -> Result<bool> {
    let projection = queue_context_clear_status_for_file(file)?;
    if !projection.is_manual_operator_clear_cooldown() {
        return Ok(false);
    }
    queue_context_clear_settled_for_file(
        file,
        &projection.target,
        &projection.harness,
        &projection.command,
        projection.source.as_deref(),
        projection.clear_epoch,
    )?;
    Ok(true)
}

pub fn clear_queue_context_clear_deferred_for_file(file: &Path) -> Result<bool> {
    let projection = queue_context_clear_status_for_file(file)?;
    if !matches!(
        projection.phase,
        agent_doc_state_backbone::QueueContextClearPhase::Deferred
    ) {
        return Ok(false);
    }
    queue_context_clear_settled_for_file(
        file,
        &projection.target,
        &projection.harness,
        &projection.command,
        projection.source.as_deref(),
        projection.clear_epoch,
    )?;
    Ok(true)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueueDrainStallPayload {
    cycle_id: Option<String>,
}

pub fn record_queue_drain_stall_continuation_pending_for_file(
    file: &Path,
    cycle_id: &str,
) -> Result<agent_doc_state_backbone::QueueDrainStallProjection> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    ensure_controller_running(&project_root, LaunchMode::Lazy)?;
    let payload = QueueDrainStallPayload {
        cycle_id: Some(cycle_id.to_string()),
    };
    request_controller(
        &project_root,
        ControllerRequest {
            command: "queue_drain_stall_continuation_recorded".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("queue_drain_stall".to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(serde_json::to_string(&payload)?),
        },
    )
}

pub fn queue_drain_stall_status_for_file(
    file: &Path,
) -> Result<agent_doc_state_backbone::QueueDrainStallProjection> {
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return Ok(agent_doc_state_backbone::QueueDrainStallProjection::default());
    };
    if connect(&project_root).is_err() {
        return Ok(agent_doc_state_backbone::QueueDrainStallProjection::default());
    }
    request_controller(
        &project_root,
        ControllerRequest {
            command: "queue_drain_stall_status".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("queue_drain_stall".to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn queue_drain_stall_continuation_pending_for_file(
    file: &Path,
) -> Result<Option<agent_doc_state_backbone::QueueDrainStallProjection>> {
    let projection = queue_drain_stall_status_for_file(file)?;
    if projection.is_pending() {
        Ok(Some(projection))
    } else {
        Ok(None)
    }
}

pub fn clear_queue_drain_stall_continuation_pending_for_file(
    file: &Path,
    reason: &str,
) -> Result<bool> {
    let projection = queue_drain_stall_status_for_file(file)?;
    if !projection.is_pending() {
        return Ok(false);
    }
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return Ok(false);
    };
    if connect(&project_root).is_err() {
        return Ok(false);
    }
    let payload = QueueDrainStallPayload { cycle_id: None };
    request_controller::<agent_doc_state_backbone::QueueDrainStallProjection>(
        &project_root,
        ControllerRequest {
            command: "queue_drain_stall_continuation_cleared".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: Some(projection.stall_epoch),
            state: None,
            caller: Some("queue_drain_stall".to_string()),
            reason: Some(reason.to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(serde_json::to_string(&payload)?),
        },
    )?;
    Ok(true)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VisibleWriteCommitCandidatePayload {
    patch_id: String,
    model_revision: u64,
    editor_visible_hash: String,
    commit_candidate_hash: String,
    commit_candidate_content: String,
    source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VisibleWriteCommitCandidateStatus {
    proof: Option<agent_doc_state_backbone::VisibleWriteCommitCandidateProjection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VisibleWriteCommitCandidatePatchStatus {
    proof: Option<agent_doc_state_backbone::VisibleWriteCommitCandidateProjection>,
}

/// `#lazily-hot-path` Theme A — answer to a delivery-convergence await.
///
/// `observed` is the honest third state: the relay hub lives in a process-local
/// registry, so a controller that hosts no hub for the document cannot report
/// convergence at all. Collapsing that into `converged: true` (the way
/// `delivery_converged_for_file` defaults for a local caller) would let a waiter
/// treat "I cannot see this document" as "delivery finished".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryConvergenceStatus {
    pub observed: bool,
    pub converged: bool,
    pub version: u64,
    #[serde(default)]
    pub recovery_signal_observed: bool,
    #[serde(default)]
    pub force_refresh_sent: bool,
}

/// Replica-wakeup policy carried by one long-lived convergence subscription.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DeliveryConvergenceRecovery {
    pub live_editors: usize,
    pub elapsed_ms: u64,
    pub signal_interval_ms: u64,
    pub force_refresh_after_ms: u64,
    pub force_refresh_sent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VisibleWriteMaterializedCarryForwardPayload {
    model_revision: u64,
    live_buffer_hash: String,
    file_content_hash: String,
    commit_candidate_hash: String,
    source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VisibleWriteMaterializedCarryForwardStatus {
    proof: Option<agent_doc_state_backbone::VisibleWriteMaterializedCarryForwardProjection>,
}

fn visible_write_commit_candidate_hash(candidate_content: &str) -> String {
    agent_doc_hash::content_hash(
        &agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
            candidate_content,
        ),
    )
}

static VISIBLE_WRITE_MODEL_REVISION: AtomicU64 = AtomicU64::new(1);

fn next_visible_write_model_revision(project_root: &Path, canonical: &Path) -> u64 {
    let document_hash = agent_doc_hash::document_id_for_path(canonical);
    let latest_projected = load_state_backbone_projection(project_root)
        .ok()
        .and_then(|projection| {
            projection
                .document(&document_hash)
                .and_then(|document| document.visible_write.latest_model_revision)
        })
        .unwrap_or_default();
    let wall_revision = timestamp_secs().saturating_mul(1_000_000);
    loop {
        let current = VISIBLE_WRITE_MODEL_REVISION.load(Ordering::Relaxed);
        let next = current
            .saturating_add(1)
            .max(latest_projected.saturating_add(1))
            .max(wall_revision);
        match VISIBLE_WRITE_MODEL_REVISION.compare_exchange(
            current,
            next,
            Ordering::SeqCst,
            Ordering::Relaxed,
        ) {
            Ok(_) => return next,
            Err(_) => continue,
        }
    }
}

pub fn record_visible_write_commit_candidate_for_file(
    file: &Path,
    patch_id: &str,
    candidate_content: &str,
    source: &str,
) -> Result<agent_doc_state_backbone::VisibleWriteCommitCandidateProjection> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    record_visible_write_commit_candidate_for_project_file(
        &project_root,
        file,
        patch_id,
        candidate_content,
        source,
    )
}

/// Record a visible-write receipt in the caller's explicit project ledger.
///
/// Editor ABIs already carry the owning project root. Keeping that root through
/// the receipt boundary prevents an out-of-tree document or test fixture from
/// accidentally resolving against the controller process's current project.
pub fn record_visible_write_commit_candidate_for_project_file(
    project_root: &Path,
    file: &Path,
    patch_id: &str,
    candidate_content: &str,
    source: &str,
) -> Result<agent_doc_state_backbone::VisibleWriteCommitCandidateProjection> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let commit_candidate_hash = visible_write_commit_candidate_hash(candidate_content);
    let model_revision = next_visible_write_model_revision(project_root, &canonical);
    let payload = VisibleWriteCommitCandidatePayload {
        patch_id: patch_id.to_string(),
        model_revision,
        editor_visible_hash: commit_candidate_hash.clone(),
        commit_candidate_hash,
        commit_candidate_content: candidate_content.to_string(),
        source: source.to_string(),
    };
    record_visible_write_commit_candidate_direct(
        project_root,
        &canonical,
        &document_hash,
        &payload,
        &anyhow::anyhow!("durable_lazily_receipt_primary"),
    )
}

pub fn visible_write_commit_candidate_applied_for_file(
    file: &Path,
    commit_candidate_hash: &str,
) -> Option<agent_doc_state_backbone::VisibleWriteCommitCandidateProjection> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if connect(&project_root).is_ok() {
        let payload = serde_json::json!({
            "commit_candidate_hash": commit_candidate_hash,
        });
        let request = ControllerRequest {
            command: "visible_write_commit_candidate_status".to_string(),
            file: Some(canonical.to_path_buf()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("visible_write".to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(payload.to_string()),
        };
        if let Ok(status) =
            request_controller::<VisibleWriteCommitCandidateStatus>(&project_root, request)
            && status.proof.is_some()
        {
            return status.proof;
        }
    }
    visible_write_commit_candidate_from_projection(
        &load_state_backbone_projection(&project_root).ok()?,
        &canonical,
        commit_candidate_hash,
    )
}

pub fn visible_write_commit_candidate_for_patch_file(
    file: &Path,
    patch_id: &str,
) -> Option<agent_doc_state_backbone::VisibleWriteCommitCandidateProjection> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if connect(&project_root).is_ok() {
        let payload = serde_json::json!({
            "patch_id": patch_id,
        });
        let request = ControllerRequest {
            command: "visible_write_commit_candidate_patch_status".to_string(),
            file: Some(canonical.to_path_buf()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("visible_write".to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(payload.to_string()),
        };
        if let Ok(status) =
            request_controller::<VisibleWriteCommitCandidatePatchStatus>(&project_root, request)
            && status.proof.is_some()
        {
            return status.proof;
        }
    }
    visible_write_commit_candidate_for_patch_from_projection(
        &load_state_backbone_projection(&project_root).ok()?,
        &canonical,
        patch_id,
    )
}

/// `#lazily-hot-path` W1 — await the visible-write receipt for `patch_id` instead of
/// re-deriving it on a private timer.
///
/// The live controller records the receipt and holds the authoritative in-memory
/// projection, so it is the one process that can *push* the arrival. This asks it to
/// wait up to `wait`, and answers the moment the fact lands. Returns `Ok(None)` when
/// the wait elapsed with no receipt, and `Err` only when the controller could not be
/// asked at all — callers fall back to the durable projection read, which keeps a
/// missing controller a slow path rather than a wedge.
pub fn await_visible_write_commit_candidate_for_patch_file(
    file: &Path,
    patch_id: &str,
    wait: std::time::Duration,
) -> Result<Option<agent_doc_state_backbone::VisibleWriteCommitCandidateProjection>> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let wait = wait.min(CONTROLLER_VISIBLE_WRITE_AWAIT_MAX);
    let payload = serde_json::json!({
        "patch_id": patch_id,
        "wait_ms": u64::try_from(wait.as_millis()).unwrap_or(u64::MAX),
    });
    let request = ControllerRequest {
        command: "visible_write_commit_candidate_patch_await".to_string(),
        file: Some(canonical.to_path_buf()),
        session_id: None,
        pane_id: None,
        window_id: None,
        generation: None,
        state: None,
        caller: Some("visible_write".to_string()),
        reason: None,
        supervisor_pid: None,
        supervisor_socket: None,
        command_kind: None,
        diagnostic_payload: Some(payload.to_string()),
    };
    // The response cannot arrive before the server-side await elapses, so the recv
    // budget must outlast it; the margin covers request/response serialization.
    let recv_timeout = wait.saturating_add(CONTROLLER_RPC_TIMEOUT);
    let status: VisibleWriteCommitCandidatePatchStatus =
        request_existing_controller_with_timeout(&project_root, request, recv_timeout)?;
    Ok(status.proof)
}

/// `#lazily-hot-path` Theme A — wait (up to `wait`) for the controller's relay hub to
/// report delivery convergence for `file`.
///
/// This is the cross-process half of [`agent_doc_crdt_relay_io::delivery_convergence_witness_for_file`]:
/// the hub registry is process-local, so a CLI process asking its *own* registry
/// learns nothing. Returns `Ok(None)` when the controller hosts no hub for the
/// document (nothing to wait for), and `Err` when the controller could not be asked —
/// callers must treat both as "proceed with your own bounded checks", never as
/// "delivery finished".
pub fn await_delivery_convergence_for_file(
    file: &Path,
    wait: std::time::Duration,
) -> Result<Option<DeliveryConvergenceStatus>> {
    request_delivery_convergence_for_file(file, None, wait, None)
}

/// Await the first delivery-convergence input change after `after_version`.
///
/// This is the write-loop subscription path: the revision cursor closes the
/// gap between its current-text observation and the controller-side park.
pub fn await_delivery_convergence_change_for_file(
    file: &Path,
    after_version: u64,
    wait: std::time::Duration,
    recovery: Option<DeliveryConvergenceRecovery>,
) -> Result<Option<DeliveryConvergenceStatus>> {
    request_delivery_convergence_for_file(file, Some(after_version), wait, recovery)
}

/// Await delivery convergence in the process that owns the relay hub.
///
/// Both the controller RPC handler and embedded-relay callers use this helper,
/// so the subscription and ACK-recovery timers have one implementation.
pub fn await_local_delivery_convergence_change_for_file(
    file: &Path,
    after_version: Option<u64>,
    wait: std::time::Duration,
    recovery: Option<DeliveryConvergenceRecovery>,
) -> Result<Option<DeliveryConvergenceStatus>> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let wait = wait.min(CONTROLLER_DELIVERY_CONVERGENCE_AWAIT_MAX);
    let started = Instant::now();
    let deadline = started.checked_add(wait);
    let mut recovery_signal_observed = false;
    let mut force_refresh_sent = recovery.is_some_and(|recovery| recovery.force_refresh_sent);
    let mut next_signal = recovery
        .map(|recovery| started + Duration::from_millis(recovery.signal_interval_ms.max(1)));
    let mut force_refresh_at = recovery.and_then(|recovery| {
        (!recovery.force_refresh_sent).then(|| {
            started
                + Duration::from_millis(
                    recovery
                        .force_refresh_after_ms
                        .saturating_sub(recovery.elapsed_ms),
                )
        })
    });

    loop {
        let now = Instant::now();
        let slice_deadline = [deadline, next_signal, force_refresh_at]
            .into_iter()
            .flatten()
            .min();
        let slice = slice_deadline
            .map(|deadline| deadline.saturating_duration_since(now))
            .unwrap_or(wait);
        let Some(witness) = agent_doc_crdt_relay_io::await_delivery_convergence_for_file(
            &canonical,
            after_version,
            slice,
        )?
        else {
            return Ok(None);
        };
        if witness.converged
            || after_version.is_some_and(|after| witness.version != after)
            || deadline.is_some_and(|deadline| Instant::now() >= deadline)
            || recovery.is_none()
        {
            return Ok(Some(DeliveryConvergenceStatus {
                observed: true,
                converged: witness.converged,
                version: witness.version,
                recovery_signal_observed,
                force_refresh_sent,
            }));
        }

        let now = Instant::now();
        let force_refresh_due =
            !force_refresh_sent && force_refresh_at.is_some_and(|deadline| now >= deadline);
        let signal_due = force_refresh_due || next_signal.is_some_and(|deadline| now >= deadline);
        if signal_due {
            let recovery = recovery.expect("signal timers require recovery policy");
            let reason = if force_refresh_due {
                force_refresh_sent = true;
                force_refresh_at = None;
                agent_doc_crdt_relay_io::CrdtReplicaEventReason::AckRecoveryForceRefresh
            } else {
                agent_doc_crdt_relay_io::CrdtReplicaEventReason::AckReplay
            };
            if agent_doc_crdt_relay_io::signal_crdt_replica_event(
                &canonical,
                reason,
                recovery.live_editors,
            )
            .is_ok()
            {
                recovery_signal_observed = true;
            }
            next_signal = Some(now + Duration::from_millis(recovery.signal_interval_ms.max(1)));
        }
    }
}

fn request_delivery_convergence_for_file(
    file: &Path,
    after_version: Option<u64>,
    wait: std::time::Duration,
    recovery: Option<DeliveryConvergenceRecovery>,
) -> Result<Option<DeliveryConvergenceStatus>> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let wait = wait.min(CONTROLLER_DELIVERY_CONVERGENCE_AWAIT_MAX);
    let payload = serde_json::json!({
        "wait_ms": u64::try_from(wait.as_millis()).unwrap_or(u64::MAX),
        "after_version": after_version,
        "recovery": recovery,
    });
    let request = ControllerRequest {
        command: "delivery_convergence_await".to_string(),
        file: Some(canonical.to_path_buf()),
        session_id: None,
        pane_id: None,
        window_id: None,
        generation: None,
        state: None,
        caller: Some("delivery_convergence".to_string()),
        reason: None,
        supervisor_pid: None,
        supervisor_socket: None,
        command_kind: None,
        diagnostic_payload: Some(payload.to_string()),
    };
    let recv_timeout = wait.saturating_add(CONTROLLER_RPC_TIMEOUT);
    let status: DeliveryConvergenceStatus =
        request_existing_controller_with_timeout(&project_root, request, recv_timeout)?;
    Ok(status.observed.then_some(status))
}

/// `#ctrlkillreregister` Tier 3 — which of **this peer's** registrations still lack a
/// replica, derived from the controller's converged liveness plane.
///
/// The editor calls this about itself and then re-registers what it is missing. That
/// is the replicated-state form of the rebuild: the controller pushes nothing, so
/// there is no endpoint to fail to reach, and it is correct whichever side restarted
/// — a controller that lost its process-local hub, an editor that reconnected, or a
/// registration that only arrived later.
pub fn peer_replicas_missing(
    project_root: &Path,
    pid: u64,
    held: &[String],
) -> Result<Vec<agent_doc_reliable_sync_io::liveness::EditorRegistration>> {
    let payload = serde_json::json!({ "pid": pid, "held": held });
    let request = ControllerRequest {
        command: "peer_replicas_missing".to_string(),
        file: None,
        session_id: None,
        pane_id: None,
        window_id: None,
        generation: None,
        state: None,
        caller: Some("editor_replica".to_string()),
        reason: None,
        supervisor_pid: None,
        supervisor_socket: None,
        command_kind: None,
        diagnostic_payload: Some(payload.to_string()),
    };
    request_existing_controller_with_timeout(project_root, request, CONTROLLER_RPC_TIMEOUT)
}

pub fn record_visible_write_materialized_carry_forward_for_file(
    file: &Path,
    live_buffer_content: &str,
    file_content: &str,
    commit_candidate_content: &str,
    source: &str,
) -> Result<agent_doc_state_backbone::VisibleWriteMaterializedCarryForwardProjection> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let live_buffer_hash = visible_write_commit_candidate_hash(live_buffer_content);
    let file_content_hash = visible_write_commit_candidate_hash(file_content);
    let commit_candidate_hash = visible_write_commit_candidate_hash(commit_candidate_content);
    let model_revision = next_visible_write_model_revision(&project_root, &canonical);
    let payload = VisibleWriteMaterializedCarryForwardPayload {
        model_revision,
        live_buffer_hash,
        file_content_hash,
        commit_candidate_hash,
        source: source.to_string(),
    };
    ensure_controller_running(&project_root, LaunchMode::Lazy)?;
    request_controller(
        &project_root,
        ControllerRequest {
            command: "visible_write_materialized_carry_forward_observed".to_string(),
            file: Some(canonical),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: Some(model_revision),
            state: None,
            caller: Some("visible_write".to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(serde_json::to_string(&payload)?),
        },
    )
}

pub fn visible_write_materialized_carry_forward_for_file(
    file: &Path,
    commit_candidate_hash: &str,
    file_content_hash: &str,
    live_buffer_hash: &str,
) -> Option<agent_doc_state_backbone::VisibleWriteMaterializedCarryForwardProjection> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if connect(&project_root).is_ok() {
        let payload = serde_json::json!({
            "commit_candidate_hash": commit_candidate_hash,
            "file_content_hash": file_content_hash,
            "live_buffer_hash": live_buffer_hash,
        });
        let request = ControllerRequest {
            command: "visible_write_materialized_carry_forward_status".to_string(),
            file: Some(canonical.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("visible_write".to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(payload.to_string()),
        };
        if let Ok(status) =
            request_controller::<VisibleWriteMaterializedCarryForwardStatus>(&project_root, request)
            && status.proof.is_some()
        {
            return status.proof;
        }
    }
    visible_write_materialized_carry_forward_from_projection(
        &load_state_backbone_projection(&project_root).ok()?,
        &canonical,
        commit_candidate_hash,
        file_content_hash,
        live_buffer_hash,
    )
}

/// `#ctlrecycle` R2 — ask the active controller for `project_root` to recycle at its
/// next idle boundary (`agent-doc admin recycle`). Returns `Ok(true)` when a
/// controller was reached, `Ok(false)` when none is running (nothing to recycle).
/// Never launches a controller — connect-only, so it is a no-op on a clean project.
pub fn recycle_controller(project_root: &Path) -> Result<bool> {
    recycle_controller_force(project_root, false)
}

/// `#recycleforce` — `recycle_controller` with an explicit operator force flag.
/// When `force` is true, the controller-side handler recycles at the next serve-loop
/// tick WITHOUT waiting on the in-flight-dispatch idle gate (`agent-doc admin recycle
/// --force`). `force == false` is byte-for-byte the prior defer-at-idle behavior.
pub fn recycle_controller_force(project_root: &Path, force: bool) -> Result<bool> {
    let checkpoint =
        checkpoint_route_owned_documents_for_project(project_root, "controller_recycle_request")?;
    warn_controller_recycle_checkpoint_failures(&project_root.display().to_string(), checkpoint);
    // The `recycle` RPC re-execs only the *authoritative* controller reachable over
    // the project socket. Capture its result but DON'T early-return — the orphan
    // reap below must run even when no authoritative controller answers (an orphaned
    // `Preparing` zombie can be the only process in this root, invisible to the
    // socket recycle).
    let command = if force { "recycle_force" } else { "recycle" };
    let result = if connect(project_root).is_ok() {
        request(project_root, command).map(|response| response.contains("\"ok\":true"))
    } else {
        Ok(false)
    };
    // #stuckhandoff2: clear stale bootstrap-owned handoffs first, then process-scan
    // orphaned `Preparing` zombies in this root so `admin recycle` clears the
    // wedged-preparing class immediately instead of relying on M1's later
    // self-watchdog tick or the next gc/connect. The shared threshold spares a
    // healthy young handoff (including one a recycle just launched). Runs
    // regardless of the recycle RPC outcome.
    let threshold = stale_preparing_controller_threshold();
    let _ =
        terminate_stale_preparing_controllers_for_caller(project_root, threshold, false, "recycle");
    let _ =
        reap_orphaned_preparing_controllers_for_caller(project_root, threshold, false, "recycle");
    result
}

/// `#ctlrecycle` R2 — recycle the active controller in EVERY project root that has a
/// running `controller serve` process (the cross-project breadth of `admin recycle
/// --all-projects`). Walks `/proc` for controllers, dedups by canonical project
/// root, and sends each a `recycle`. Returns `(recycled, skipped)`.
pub fn recycle_controllers_all_projects() -> Result<(usize, usize)> {
    recycle_controllers_all_projects_force(false)
}

/// `#recycleforce` — `recycle_controllers_all_projects` with an explicit operator
/// force flag applied to every project's recycle (`agent-doc admin recycle
/// --all-projects --force`). `force == false` is the prior defer-at-idle behavior.
pub fn recycle_controllers_all_projects_force(force: bool) -> Result<(usize, usize)> {
    let roots = crate::process::controller_project_roots(std::process::id());
    let mut recycled = 0;
    let mut skipped = 0;
    for root in roots {
        match recycle_controller_force(&root, force) {
            Ok(true) => recycled += 1,
            _ => skipped += 1,
        }
    }
    // #stuckhandoff2: orphan reaping is project-scoped inside `recycle_controller`,
    // and `roots` above already includes every root with a `controller serve
    // --handoff-state preparing` process (the orphan's own cmdline matches), so each
    // wedged-preparing orphan's root gets a `recycle_controller` call that reaps it —
    // no separate cross-project sweep needed here (gc still runs that on its tick).
    Ok((recycled, skipped))
}

/// `#turnsaferecycle` Goal 1 — the supervisor breadth of an install fan-out. Today
/// [`recycle_controllers_all_projects_force`] only marks lazy CONTROLLER (CP)
/// processes; the long-lived `agent-doc start --route-owned` supervisors that
/// actually write documents are left to self-detect staleness. This walks `/proc`
/// for every route-owned supervisor, dedups by served document, and writes each a
/// recycle-request marker so they recycle onto the freshly-installed binary at the
/// next idle boundary. Returns `(marked, skipped)`. `force` is recorded in the
/// marker reason for parity with the controller fan-out; the supervisor honors the
/// request regardless.
pub fn recycle_supervisors_all_projects() -> Result<(usize, usize)> {
    recycle_supervisors_all_projects_force(false)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CrdtCheckpointSummary {
    pub checkpointed: usize,
    pub detached: usize,
    pub skipped: usize,
    pub failed: usize,
}

impl CrdtCheckpointSummary {
    pub fn all_clear(self) -> bool {
        self.failed == 0
    }
}

fn warn_controller_recycle_checkpoint_failures(scope: &str, checkpoint: CrdtCheckpointSummary) {
    if checkpoint.all_clear() {
        return;
    }
    eprintln!(
        "[agent-doc] warning: continuing controller recycle for {scope} after CRDT durable checkpoint failed for {} document(s) ({} checkpointed, {} detached, {} skipped); supervisor recycle fan-out still skips any uncheckpointed document",
        checkpoint.failed, checkpoint.checkpointed, checkpoint.detached, checkpoint.skipped,
    );
}

fn checkpoint_crdt_via_controller_document_model(
    canonical: &Path,
    source: &str,
) -> Result<Option<String>> {
    match agent_doc_crdt_relay_io::checkpoint_durable_projection_for_file(canonical, source) {
        Ok(agent_doc_crdt_relay_io::DurableProjectionCheckpoint::Detached) => {
            agent_doc_ops_log_io::log_op(
                canonical,
                &format!(
                    "controller_crdt_checkpoint file={} source={} status=detached authority=cp_model transport=local_document_model",
                    canonical.display(),
                    source,
                ),
            );
            Ok(Some("detached".to_string()))
        }
        Ok(agent_doc_crdt_relay_io::DurableProjectionCheckpoint::Checkpointed {
            bytes,
            changed,
            live_editors,
            text_len,
            text_hash,
        }) => {
            agent_doc_ops_log_io::log_op(
                canonical,
                &format!(
                    "controller_crdt_checkpoint file={} source={} status=checkpointed authority=cp_model transport=local_document_model bytes={} changed={} live_editors={} text_len={} text_hash={}",
                    canonical.display(),
                    source,
                    bytes,
                    changed,
                    live_editors,
                    text_len,
                    text_hash,
                ),
            );
            Ok(Some("checkpointed".to_string()))
        }
        Ok(agent_doc_crdt_relay_io::DurableProjectionCheckpoint::Deferred { reason }) => {
            agent_doc_ops_log_io::log_op(
                canonical,
                &format!(
                    "controller_crdt_checkpoint file={} source={} status=deferred authority=cp_model transport=local_document_model reason={} recovery=background_yrs_repair",
                    canonical.display(),
                    source,
                    reason,
                ),
            );
            Ok(Some("deferred".to_string()))
        }
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                canonical,
                &format!(
                    "controller_crdt_checkpoint_failed file={} source={} authority=cp_model transport=local_document_model error={:?}",
                    canonical.display(),
                    source,
                    err.to_string(),
                ),
            );
            Err(err).with_context(|| {
                format!(
                    "controller document-model CRDT durable checkpoint failed for {}",
                    canonical.display()
                )
            })
        }
    }
}

fn checkpoint_route_owned_document_crdt(doc: &Path, source: &str) -> Result<Option<String>> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let file_arg = canonical.to_string_lossy().to_string();
    let authority = crdt_authority_for_file(&file_arg);
    if !authority.editor_attached() {
        agent_doc_ops_log_io::log_op(
            &canonical,
            &format!(
                "controller_crdt_checkpoint_skipped file={} source={} authority=git reason=detached_authority",
                canonical.display(),
                source,
            ),
        );
        return Ok(Some("detached".to_string()));
    }
    if agent_doc_project_root_io::project_root_containing(&canonical).is_none() {
        agent_doc_ops_log_io::log_op(
            doc,
            &format!(
                "controller_crdt_checkpoint_skipped file={} source={} reason=no_project_root",
                doc.display(),
                source,
            ),
        );
        return Ok(None);
    }
    checkpoint_crdt_via_controller_document_model(&canonical, source)
}

fn controller_current_text_error_allows_projection_recovery(err: &anyhow::Error) -> bool {
    let message = format!("{err:#}");
    controller_transport_drop_is_retryable(err)
        || message.contains("timed out after")
        || (message.contains("document model startup/reconciliation failed")
            && message.contains("Lazily-current request over editor_ipc failed"))
}

fn recover_current_text_from_local_projection_after_controller_error(
    canonical: &Path,
    source: &str,
    authority: agent_doc_document_realtime::crdt_authority::CrdtAuthority,
    err: &anyhow::Error,
    phase: &str,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    agent_doc_ops_log_io::log_op(
        canonical,
        &format!(
            "controller_crdt_current_text_projection_recovery file={} source={} phase={} error={} recovery=local_durable_projection_after_publish_timeout",
            canonical.display(),
            source,
            phase,
            compact_controller_error(err),
        ),
    );
    agent_doc_crdt_relay_io::ensure_document_model_with_current_text_recovery_observer(
        canonical,
        source,
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica,
        || agent_doc_crdt_relay_io::current_text_for_file_with_authority(canonical, authority),
        || {
            agent_doc_crdt_relay_io::current_text_for_file_with_authority_recovering_projection(
                canonical, authority,
            )
        },
    )
}

struct ControllerCurrentTextRead {
    canonical: PathBuf,
    project_root: PathBuf,
    authority: agent_doc_document_realtime::crdt_authority::CrdtAuthority,
    current: agent_doc_crdt_relay_io::CurrentText,
}

fn current_text_controller_initial_read_for_doc(
    doc: &Path,
    source: &str,
    recover_after_controller_error: bool,
    flush_barrier: bool,
) -> Result<Option<ControllerCurrentTextRead>> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let file_arg = canonical.to_string_lossy().to_string();
    let authority = crdt_authority_for_file(&file_arg);
    if !authority.editor_attached() {
        agent_doc_ops_log_io::log_op(
            &canonical,
            &format!(
                "controller_crdt_current_text_skipped file={} source={} authority=git reason=detached_authority",
                canonical.display(),
                source,
            ),
        );
        // `#ctrlrespawnenoent2`: resolve the real root even on the detached
        // path. This used to hand back `PathBuf::new()` as a "not applicable"
        // sentinel, but the field is a plain `PathBuf`, so nothing stopped a
        // consumer from using it — and an empty path reaching a controller
        // launch makes `Command::current_dir("")` fail with ENOENT, which reads
        // as a missing binary rather than a bad root. Observed as
        // `--project-root ` (empty) in a launch failure while `binary=` in the
        // same message was correct.
        // The `unwrap_or_default()` this replaced still produced the very empty
        // `PathBuf` sentinel described above whenever root resolution failed,
        // which then surfaced downstream as the misleading "project root is
        // empty" launch error instead of the real cause. There is no usable
        // read without a root, so report no read rather than a bad one.
        let Some(detached_project_root) =
            agent_doc_project_root_io::project_root_containing(&canonical)
        else {
            agent_doc_ops_log_io::log_op(
                &canonical,
                &format!(
                    "controller_crdt_current_text_skipped file={} source={} reason=no_project_root_detached",
                    canonical.display(),
                    source,
                ),
            );
            return Ok(None);
        };
        return Ok(Some(ControllerCurrentTextRead {
            canonical,
            project_root: detached_project_root,
            authority,
            current: agent_doc_crdt_relay_io::CurrentText::Detached,
        }));
    }
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        agent_doc_ops_log_io::log_op(
            doc,
            &format!(
                "controller_crdt_current_text_skipped file={} source={} reason=no_project_root",
                doc.display(),
                source,
            ),
        );
        return Ok(None);
    };
    let controller_socket = socket_path(&project_root);
    if let Err(err) = connect_path(&controller_socket) {
        agent_doc_ops_log_io::log_op(
            &canonical,
            &format!(
                "controller_crdt_current_text_controller_unavailable file={} source={} socket={} error={} recovery=ensure_controller_running",
                canonical.display(),
                source,
                controller_socket.display(),
                format!("{err:#}").replace('\n', "\\n")
            ),
        );
        ensure_controller_running(&project_root, LaunchMode::Lazy).with_context(|| {
            format!(
                "failed to start project controller for CRDT current-text relay at {}",
                controller_socket.display()
            )
        })?;
    }
    let first = match request_controller_crdt_current_text_with_flush_barrier(
        &project_root,
        &canonical,
        source,
        flush_barrier,
    ) {
        Ok(first) => first,
        Err(err)
            if recover_after_controller_error
                && controller_current_text_error_allows_projection_recovery(&err) =>
        {
            let current = recover_current_text_from_local_projection_after_controller_error(
                &canonical,
                source,
                authority,
                &err,
                "initial_request",
            )?;
            return Ok(Some(ControllerCurrentTextRead {
                canonical,
                project_root,
                authority,
                current,
            }));
        }
        Err(err) => return Err(err),
    };
    Ok(Some(ControllerCurrentTextRead {
        canonical,
        project_root,
        authority,
        current: first,
    }))
}

pub fn current_text_via_controller_model_read_for_doc(
    doc: &Path,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::CurrentText>> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let file_arg = canonical.to_string_lossy().to_string();
    let authority = crdt_authority_for_file(&file_arg);
    if !authority.editor_attached() {
        agent_doc_ops_log_io::log_op(
            &canonical,
            &format!(
                "controller_crdt_current_text_skipped file={} source={} authority=git reason=detached_authority",
                canonical.display(),
                source,
            ),
        );
        return Ok(Some(agent_doc_crdt_relay_io::CurrentText::Detached));
    }
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        agent_doc_ops_log_io::log_op(
            doc,
            &format!(
                "controller_crdt_current_text_skipped file={} source={} reason=no_project_root",
                doc.display(),
                source,
            ),
        );
        return Ok(None);
    };
    match request_existing_controller_crdt_current_text_read(&project_root, &canonical, source) {
        Ok(current) => {
            clear_expired_controller_model_pressure(&project_root);
            Ok(Some(current))
        }
        Err(err) => {
            record_controller_model_pressure(
                &project_root,
                &canonical,
                source,
                &format!("{err:#}"),
            );
            agent_doc_ops_log_io::log_op(
                &canonical,
                &format!(
                    "controller_crdt_current_text_read_unavailable file={} source={} socket={} timeout_ms={} error={} recovery=idle_disk_fallback",
                    canonical.display(),
                    source,
                    socket_path(&project_root).display(),
                    CONTROLLER_CRDT_CURRENT_TEXT_READ_TIMEOUT.as_millis(),
                    format!("{err:#}").replace('\n', "\\n"),
                ),
            );
            Err(err)
        }
    }
}

/// Read the compact CRDT revision from an already-running controller.
///
/// This intentionally does not materialize canonical markdown or emit the
/// full-text observation log. Quiescent supervisors use it as the lazy
/// invalidation key for queue parsing.
pub fn revision_via_controller_model_read_for_doc(
    doc: &Path,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::CurrentRevision>> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let file_arg = canonical.to_string_lossy().to_string();
    let authority = crdt_authority_for_file(&file_arg);
    if !authority.editor_attached() {
        return Ok(Some(agent_doc_crdt_relay_io::CurrentRevision::Detached));
    }
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return Ok(None);
    };
    match request_existing_controller_crdt_revision_read(&project_root, &canonical, source) {
        Ok(revision) => {
            clear_expired_controller_model_pressure(&project_root);
            Ok(Some(revision))
        }
        Err(err) => {
            record_controller_model_pressure(
                &project_root,
                &canonical,
                source,
                &format!("{err:#}"),
            );
            Err(err)
        }
    }
}

pub fn current_text_via_controller_model_for_doc(
    doc: &Path,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::CurrentText>> {
    let Some(read) = current_text_controller_initial_read_for_doc(doc, source, true, true)? else {
        return Ok(None);
    };
    let ControllerCurrentTextRead {
        canonical,
        project_root,
        authority,
        current: first,
    } = read;
    if matches!(
        first,
        agent_doc_crdt_relay_io::CurrentText::Detached
            | agent_doc_crdt_relay_io::CurrentText::Current { .. }
    ) {
        return Ok(Some(first));
    }
    let ensured =
        agent_doc_crdt_relay_io::ensure_document_model_with_current_text_recovery_observer(
            &canonical,
            source,
            first,
            || {
                request_controller_crdt_current_text_with_timeout(
                    &project_root,
                    &canonical,
                    source,
                    CONTROLLER_CRDT_CURRENT_TEXT_POLL_TIMEOUT,
                    true,
                )
            },
            || {
                match agent_doc_crdt_relay_io::current_text_for_file_with_authority_recovering_projection(
                    &canonical, authority,
                ) {
                    Ok(
                        current
                        @ (agent_doc_crdt_relay_io::CurrentText::Detached
                        | agent_doc_crdt_relay_io::CurrentText::Current { .. }),
                    ) => Ok(current),
                    Ok(_) => match request_controller_crdt_current_text_with_timeout_recovering_projection(
                        &project_root,
                        &canonical,
                        source,
                        CONTROLLER_CRDT_CURRENT_TEXT_POLL_TIMEOUT,
                    ) {
                        Ok(current) => Ok(current),
                        Err(err) if controller_current_text_error_allows_projection_recovery(&err) => {
                            recover_current_text_from_local_projection_after_controller_error(
                                &canonical,
                                source,
                                authority,
                                &err,
                                "recovery_observer",
                            )
                        }
                        Err(err) => Err(err),
                    },
                    Err(err) => Err(err),
                }
            },
        )?;
    Ok(Some(ensured))
}

fn request_controller_crdt_current_text_with_flush_barrier(
    project_root: &Path,
    canonical: &Path,
    source: &str,
    flush_barrier: bool,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    request_controller_crdt_current_text_with_timeout(
        project_root,
        canonical,
        source,
        CONTROLLER_CRDT_CURRENT_TEXT_TIMEOUT,
        flush_barrier,
    )
}

fn request_controller_crdt_current_text_with_timeout(
    project_root: &Path,
    canonical: &Path,
    source: &str,
    timeout: Duration,
    flush_barrier: bool,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    request_controller_crdt_current_text_with_options(
        project_root,
        canonical,
        source,
        timeout,
        false,
        flush_barrier,
    )
}

fn request_controller_crdt_current_text_with_timeout_recovering_projection(
    project_root: &Path,
    canonical: &Path,
    source: &str,
    timeout: Duration,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    request_controller_crdt_current_text_with_options(
        project_root,
        canonical,
        source,
        timeout,
        true,
        true,
    )
}

fn request_controller_crdt_current_text_with_options(
    project_root: &Path,
    canonical: &Path,
    source: &str,
    timeout: Duration,
    recover_projection: bool,
    flush_barrier: bool,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    let data: serde_json::Value = request_controller_with_timeout(
        project_root,
        ControllerRequest {
            command: "crdt_current_text".to_string(),
            file: Some(canonical.to_path_buf()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(
                serde_json::json!({
                    "source": source,
                    "recover_projection": recover_projection,
                    "flush_barrier": flush_barrier,
                })
                .to_string(),
            ),
        },
        timeout,
    )?;
    let current = controller_current_text_from_data(&data)?;
    log_controller_current_text_result(canonical, source, &current);
    Ok(current)
}

fn request_existing_controller_crdt_current_text_read(
    project_root: &Path,
    canonical: &Path,
    source: &str,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    let data: serde_json::Value = request_existing_controller_with_timeout(
        project_root,
        ControllerRequest {
            command: "crdt_current_text".to_string(),
            file: Some(canonical.to_path_buf()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(
                serde_json::json!({
                    "source": source,
                    "recover_projection": false,
                    "flush_barrier": false,
                })
                .to_string(),
            ),
        },
        CONTROLLER_CRDT_CURRENT_TEXT_READ_TIMEOUT,
    )?;
    let current = controller_current_text_from_data(&data)?;
    log_controller_current_text_result(canonical, source, &current);
    Ok(current)
}

fn request_existing_controller_crdt_revision_read(
    project_root: &Path,
    canonical: &Path,
    source: &str,
) -> Result<agent_doc_crdt_relay_io::CurrentRevision> {
    let data: serde_json::Value = request_existing_controller_with_timeout(
        project_root,
        ControllerRequest {
            command: "crdt_revision".to_string(),
            file: Some(canonical.to_path_buf()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(serde_json::json!({ "source": source }).to_string()),
        },
        CONTROLLER_CRDT_REVISION_READ_TIMEOUT,
    )?;
    serde_json::from_value(data).context("failed to parse controller CRDT revision response")
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ControllerCrdtCpWriteResult {
    pub write: Option<agent_doc_crdt_relay_io::CpRelayWrite>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ControllerCrdtCpWritePayload {
    expected_current: String,
    content: String,
    source: Option<String>,
}

pub fn apply_cp_write_via_controller_model_for_doc(
    doc: &Path,
    expected_current: &str,
    content: &str,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::CpRelayWrite>> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let file_arg = canonical.to_string_lossy().to_string();
    let authority = crdt_authority_for_file(&file_arg);
    if !authority.editor_attached() {
        agent_doc_ops_log_io::log_op(
            &canonical,
            &format!(
                "controller_crdt_cp_write_skipped file={} source={} authority=git reason=detached_authority",
                canonical.display(),
                source,
            ),
        );
        return Ok(None);
    }
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        agent_doc_ops_log_io::log_op(
            doc,
            &format!(
                "controller_crdt_cp_write_skipped file={} source={} reason=no_project_root",
                doc.display(),
                source,
            ),
        );
        return Ok(None);
    };
    ensure_controller_running(&project_root, LaunchMode::Lazy)?;
    let payload = ControllerCrdtCpWritePayload {
        expected_current: expected_current.to_string(),
        content: content.to_string(),
        source: Some(source.to_string()),
    };
    let result: ControllerCrdtCpWriteResult = request_controller(
        &project_root,
        ControllerRequest {
            command: "crdt_cp_write".to_string(),
            file: Some(canonical),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("crdt_relay".to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(serde_json::to_string(&payload)?),
        },
    )?;
    Ok(result.write)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerResponseCellAddResult {
    pub write: Option<agent_doc_crdt_relay_io::ResponseCellRelayWrite>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ControllerResponseCellAddPayload {
    cycle_id: String,
    operation_id: String,
    response_sha256: String,
    response: String,
    #[serde(default)]
    committed_content: Option<String>,
    #[serde(default)]
    checkpoint_only: bool,
    source: Option<String>,
}

/// Add one assistant response cell through the controller-owned canonical CRDT
/// model. The caller supplies no whole-document baseline or replacement.
pub fn add_response_cell_via_controller_model_for_doc(
    doc: &Path,
    cycle_id: &str,
    operation_id: &str,
    response_sha256: &str,
    response: &str,
    committed_content: Option<&str>,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::ResponseCellRelayWrite>> {
    response_cell_via_controller_model_for_doc(
        doc,
        cycle_id,
        operation_id,
        response_sha256,
        response,
        committed_content,
        false,
        source,
    )
}

/// Persist a cumulative, semantically complete response checkpoint without
/// advancing the response cycle to `write_applied`. A later sealed response uses
/// the ordinary add path, records the durable closeout fact, and commits.
pub fn checkpoint_response_cell_via_controller_model_for_doc(
    doc: &Path,
    cycle_id: &str,
    operation_id: &str,
    response_sha256: &str,
    response: &str,
    committed_content: Option<&str>,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::ResponseCellRelayWrite>> {
    response_cell_via_controller_model_for_doc(
        doc,
        cycle_id,
        operation_id,
        response_sha256,
        response,
        committed_content,
        true,
        source,
    )
}

#[allow(clippy::too_many_arguments)]
fn response_cell_via_controller_model_for_doc(
    doc: &Path,
    cycle_id: &str,
    operation_id: &str,
    response_sha256: &str,
    response: &str,
    committed_content: Option<&str>,
    checkpoint_only: bool,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::ResponseCellRelayWrite>> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let file_arg = canonical.to_string_lossy().to_string();
    if !crdt_authority_for_file(&file_arg).editor_attached() {
        return Ok(None);
    }
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return Ok(None);
    };
    ensure_controller_running(&project_root, LaunchMode::Lazy)?;
    let payload = ControllerResponseCellAddPayload {
        cycle_id: cycle_id.to_string(),
        operation_id: operation_id.to_string(),
        response_sha256: response_sha256.to_string(),
        response: response.to_string(),
        committed_content: committed_content.map(str::to_string),
        checkpoint_only,
        source: Some(source.to_string()),
    };
    let result: ControllerResponseCellAddResult = request_controller(
        &project_root,
        ControllerRequest {
            command: "response_cell_add".to_string(),
            file: Some(canonical),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("response_cell_add".to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(serde_json::to_string(&payload)?),
        },
    )?;
    Ok(result.write)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ControllerCrdtTextAdoptResult {
    pub changed: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ControllerCrdtTextAdoptPayload {
    text: String,
    source: Option<String>,
}

/// Fold editor-visible text into the controller-owned canonical relay model.
///
/// Callers must already have a verified editor receipt. This is the process
/// boundary companion to `adopt_editor_text_for_file`: the project controller,
/// rather than a short-lived CLI process, owns the canonical model consumed by
/// the closeout commit barrier.
pub fn adopt_editor_text_via_controller_model_for_doc(
    doc: &Path,
    text: &str,
    source: &str,
) -> Result<Option<bool>> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        agent_doc_ops_log_io::log_op(
            doc,
            &format!(
                "controller_crdt_text_adopt_skipped file={} source={} reason=no_project_root",
                doc.display(),
                source,
            ),
        );
        return Ok(None);
    };
    ensure_controller_running(&project_root, LaunchMode::Lazy)?;
    let payload = ControllerCrdtTextAdoptPayload {
        text: text.to_string(),
        source: Some(source.to_string()),
    };
    let result: ControllerCrdtTextAdoptResult = request_controller(
        &project_root,
        ControllerRequest {
            command: "crdt_text_adopt".to_string(),
            file: Some(canonical),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("verified_editor_receipt".to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(serde_json::to_string(&payload)?),
        },
    )?;
    Ok(result.changed)
}

pub fn commit_barrier_via_controller_model_for_doc(doc: &Path) -> Result<bool> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let file_arg = canonical.to_string_lossy().to_string();
    let authority = crdt_authority_for_file(&file_arg);
    if !authority.editor_attached() {
        agent_doc_ops_log_io::log_op(
            &canonical,
            &format!(
                "controller_crdt_commit_barrier_skipped file={} authority=git reason=detached_authority",
                canonical.display(),
            ),
        );
        return Ok(true);
    }
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        agent_doc_ops_log_io::log_op(
            doc,
            &format!(
                "controller_crdt_commit_barrier_skipped file={} reason=no_project_root",
                doc.display(),
            ),
        );
        return Ok(true);
    };
    let controller_socket = socket_path(&project_root);
    if let Err(err) = connect_path(&controller_socket) {
        agent_doc_ops_log_io::log_op(
            &canonical,
            &format!(
                "controller_crdt_commit_barrier_controller_unavailable file={} socket={} error={} recovery=ensure_controller_running",
                canonical.display(),
                controller_socket.display(),
                format!("{err:#}").replace('\n', "\\n")
            ),
        );
        ensure_controller_running(&project_root, LaunchMode::Lazy).with_context(|| {
            format!(
                "failed to start project controller for CRDT commit barrier at {}",
                controller_socket.display()
            )
        })?;
    }
    request_controller(
        &project_root,
        ControllerRequest {
            command: "crdt_commit_barrier".to_string(),
            file: Some(canonical),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

/// Delegate the git commit for `doc` to the CP controller — the authoritative
/// owner of the converged relay canonical — instead of committing as a
/// non-authoritative CLI replica. Returns:
/// - `Ok(Some(outcome))` when the controller performed the commit;
/// - `Ok(None)` when there is no live editor authority to defer to (headless), so
///   the caller's local disk/snapshot commit is already authoritative.
///
/// This is what lets `Compact Exchange` / `write --commit` / `finalize` land on a
/// document with a live editor: the controller commits in-process where its
/// canonical is authority, instead of the CLI failing closed with
/// `editor is the current authority ... was not used as commit authority`.
pub fn commit_document_via_controller(
    doc: &Path,
    authoritative_compaction: bool,
) -> Result<Option<ControllerCommitDocumentOutcome>> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let file_arg = canonical.to_string_lossy().to_string();
    let authority = crdt_authority_for_file(&file_arg);
    if !authority.editor_attached() {
        // Headless: no live editor to defer to; the caller's local commit is
        // already authoritative.
        return Ok(None);
    }
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return Ok(None);
    };
    // Delegate ONLY to an already-running controller. A live editor implies a
    // running controller (the plugin talks to it); if none is reachable, fall back
    // to the caller's local commit rather than launching a controller mid-commit.
    let controller_socket = socket_path(&project_root);
    let stream = match connect_path(&controller_socket) {
        Ok(stream) => stream,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                &canonical,
                &format!(
                    "controller_commit_document_unavailable file={} socket={} error={} recovery=local_commit",
                    canonical.display(),
                    controller_socket.display(),
                    format!("{err:#}").replace('\n', "\\n")
                ),
            );
            return Ok(None);
        }
    };
    let payload = serde_json::to_string(&ControllerCommitDocumentPayload {
        authoritative_compaction,
    })
    .context("failed to serialize commit_document payload")?;
    // The connected stream is the liveness proof. Consume that exact stream for
    // the request instead of probing and reconnecting: a recycle between two
    // connects otherwise turns a reachable controller into `connection refused`
    // and invites an unsafe force-disk/manual retry loop.
    let outcome: ControllerCommitDocumentOutcome = request_controller_on_stream_with_timeout(
        &project_root,
        ControllerRequest {
            command: "commit_document".to_string(),
            file: Some(canonical),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(payload),
        },
        CONTROLLER_COMMIT_DOCUMENT_TIMEOUT,
        stream,
    )?;
    Ok(Some(outcome))
}

/// Submit Compact Exchange to the CP. The caller never computes or applies a
/// document rewrite; it waits for the controller-owned operation to complete.
pub fn compact_document_via_controller(
    doc: &Path,
    invocation: ControllerCompactDocumentInvocation,
) -> Result<()> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        anyhow::bail!(
            "cannot execute Compact Exchange for {} through the CP: no project root",
            canonical.display(),
        );
    };
    let commit = invocation.commit;
    let payload = serde_json::to_string(&invocation)
        .context("failed to serialize compact_document payload")?;
    let request = ControllerRequest {
        command: "compact_document".to_string(),
        file: Some(canonical),
        session_id: None,
        pane_id: None,
        window_id: None,
        generation: None,
        state: None,
        caller: None,
        reason: None,
        supervisor_pid: None,
        supervisor_socket: None,
        command_kind: None,
        diagnostic_payload: Some(payload),
    };
    let submit = |request| {
        request_controller_with_timeout::<serde_json::Value>(
            &project_root,
            request,
            CONTROLLER_COMPACT_DOCUMENT_TIMEOUT,
        )
    };
    match submit(request.clone()) {
        Err(err) if err.to_string().contains("controller_binary_stale") => {
            submit(request)?;
        }
        other => {
            other?;
        }
    }
    if commit {
        eprintln!("{COMPACT_COMMIT_SCOPE_NOTE}");
    }
    Ok(())
}

pub fn record_committed_baseline_via_controller_model_for_doc(doc: &Path) -> Result<bool> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let file_arg = canonical.to_string_lossy().to_string();
    let authority = crdt_authority_for_file(&file_arg);
    if !authority.editor_attached() {
        agent_doc_ops_log_io::log_op(
            &canonical,
            &format!(
                "controller_crdt_record_committed_baseline_skipped file={} authority=git reason=detached_authority",
                canonical.display(),
            ),
        );
        return Ok(false);
    }
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        agent_doc_ops_log_io::log_op(
            doc,
            &format!(
                "controller_crdt_record_committed_baseline_skipped file={} reason=no_project_root",
                doc.display(),
            ),
        );
        return Ok(false);
    };
    let controller_socket = socket_path(&project_root);
    if let Err(err) = connect_path(&controller_socket) {
        agent_doc_ops_log_io::log_op(
            &canonical,
            &format!(
                "controller_crdt_record_committed_baseline_controller_unavailable file={} socket={} error={} recovery=ensure_controller_running",
                canonical.display(),
                controller_socket.display(),
                format!("{err:#}").replace('\n', "\\n")
            ),
        );
        ensure_controller_running(&project_root, LaunchMode::Lazy).with_context(|| {
            format!(
                "failed to start project controller for CRDT committed-baseline record at {}",
                controller_socket.display()
            )
        })?;
    }
    request_controller(
        &project_root,
        ControllerRequest {
            command: "crdt_record_committed_baseline".to_string(),
            file: Some(canonical),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

#[derive(Debug, Serialize, Deserialize)]
struct ControllerDiskProjectionPayload {
    projection_b64: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ControllerDiskProjectionReconcileResult {
    pub changed: Option<bool>,
}

pub fn reconcile_disk_projection_via_controller_model_for_doc(
    doc: &Path,
    projection: &[u8],
) -> Result<Option<bool>> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        agent_doc_ops_log_io::log_op(
            doc,
            &format!(
                "controller_crdt_disk_projection_reconcile_skipped file={} reason=no_project_root",
                doc.display(),
            ),
        );
        return Ok(None);
    };
    let result: ControllerDiskProjectionReconcileResult = request_controller(
        &project_root,
        ControllerRequest {
            command: "crdt_disk_projection_reconcile".to_string(),
            file: Some(canonical),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(
                serde_json::to_string(&ControllerDiskProjectionPayload {
                    projection_b64: base64_standard_encode(projection),
                })
                .context("failed to encode disk projection reconcile payload")?,
            ),
        },
    )?;
    Ok(result.changed)
}

#[derive(Debug, Serialize, Deserialize)]
struct ControllerWatchSignalResult {
    action: String,
}

pub fn route_disk_change_signal_via_controller_model_for_doc(
    doc: &Path,
    delivery: &WatchDelivery,
) -> Result<WatchAction> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        agent_doc_ops_log_io::log_op(
            doc,
            &format!(
                "controller_crdt_route_disk_change_signal_skipped file={} reason=no_project_root",
                doc.display(),
            ),
        );
        return Ok(WatchAction::None);
    };
    let result: ControllerWatchSignalResult = request_controller(
        &project_root,
        ControllerRequest {
            command: "crdt_route_disk_change_signal".to_string(),
            file: Some(canonical),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(
                serde_json::to_string(&DiskChangeSignal::from_delivery(delivery))
                    .context("failed to encode disk-change route signal payload")?,
            ),
        },
    )?;
    watch_action_from_payload(&result.action)
}

fn handle_crdt_commit_barrier_rpc(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<bool> {
    let requested_file = request_file(&request)?;
    let canonical = canonical_controller_request_file(bootstrap, &requested_file);
    commit_barrier_for_closeout(runtime, &canonical)
}

/// `durable_response_cell` readiness is read from the live in-memory Lazily
/// projection (`#lzdurablesink`). This runs inside the controller process, so it
/// MUST NOT replay `state.db`, nor round-trip a read back through the controller
/// socket: while the controller is live it is the single authority. The relay
/// barrier below is a separate editor→canonical flush, not a state read.
fn commit_barrier_for_closeout(runtime: &ControllerRuntime, canonical: &Path) -> Result<bool> {
    let document_hash = agent_doc_hash::document_id_for_path(canonical);
    let durable_response_cell = runtime
        .document_state_projection(&document_hash)?
        .is_some_and(|document| document.closeout.response_cell.is_some());
    let ready = if durable_response_cell {
        agent_doc_crdt_relay_io::commit_barrier_for_durable_response_cell(canonical)
    } else {
        agent_doc_crdt_relay_io::commit_barrier_for_file(canonical)
    };
    agent_doc_ops_log_io::log_op(
        canonical,
        &format!(
            "controller_crdt_commit_barrier file={} durable_response_cell={} ready={}",
            canonical.display(),
            durable_response_cell,
            ready,
        ),
    );
    Ok(ready)
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ControllerCommitDocumentPayload {
    #[serde(default)]
    authoritative_compaction: bool,
}

/// Perform the git commit for a document from inside the controller process. This
/// is the CP-owned commit: the controller hosts the converged relay canonical, so
/// running the commit here — rather than the CLI asking over the socket and then
/// committing as a non-authoritative replica — lets a document with a live editor
/// commit authoritatively instead of failing closed
/// (`editor is the current authority ... was not used as commit authority`).
///
/// Flow: converge live editor ops into the canonical (`commit_barrier_for_file`
/// flushes editor→canonical), then delegate the actual commit to the binary-wired
/// runtime effect (`agent-doc-commit-io`, which depends on this crate). The effect
/// sets the preconverged flag so the in-process commit reads the just-converged
/// canonical as authority and does not round-trip back to this controller.
fn handle_commit_document_rpc(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<ControllerCommitDocumentOutcome> {
    let requested_file = request_file(&request)?;
    let canonical = canonical_controller_request_file(bootstrap, &requested_file);
    let payload: ControllerCommitDocumentPayload = match request.diagnostic_payload.as_deref() {
        Some(json) => {
            serde_json::from_str(json).context("failed to parse commit_document payload")?
        }
        None => ControllerCommitDocumentPayload::default(),
    };
    // Converge: flush live editor ops into the canonical so the in-process commit
    // reads the authoritative merged content. We proceed regardless of the
    // editor-behind readiness bit — this is an explicit authority commit, the
    // canonical is authority, and editors reconcile via the replace-capable replica
    // delivery.
    let barrier_ready = commit_barrier_for_closeout(runtime, &canonical)?;
    agent_doc_ops_log_io::log_op(
        &canonical,
        &format!(
            "controller_commit_document file={} authoritative_compaction={} barrier_ready={}",
            canonical.display(),
            payload.authoritative_compaction,
            barrier_ready
        ),
    );
    runtime_effects()?.commit_document(&canonical, payload.authoritative_compaction)
}

fn handle_compact_document_rpc(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<serde_json::Value> {
    let requested_file = request_file(&request)?;
    let canonical = canonical_controller_request_file(bootstrap, &requested_file);
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let invocation: ControllerCompactDocumentInvocation =
        serde_json::from_str(&payload_json).context("failed to parse compact_document payload")?;
    let barrier_ready = commit_barrier_for_closeout(runtime, &canonical)?;
    agent_doc_ops_log_io::log_op(
        &canonical,
        &format!(
            "controller_compact_document file={} component={} commit={} barrier_ready={}",
            canonical.display(),
            invocation.component_name.as_deref().unwrap_or("exchange"),
            invocation.commit,
            barrier_ready,
        ),
    );
    runtime_effects()?.compact_document(&canonical, invocation)?;
    Ok(serde_json::json!({ "executed_by": "cp" }))
}

fn handle_crdt_record_committed_baseline_rpc(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<bool> {
    let requested_file = request_file(&request)?;
    let canonical = canonical_controller_request_file(bootstrap, &requested_file);
    agent_doc_crdt_relay_io::record_committed_baseline_for_file(&canonical);
    Ok(true)
}

fn handle_crdt_disk_projection_reconcile_rpc(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<ControllerDiskProjectionReconcileResult> {
    let requested_file = request_file(&request)?;
    let canonical = canonical_controller_request_file(bootstrap, &requested_file);
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: ControllerDiskProjectionPayload =
        serde_json::from_str(&payload_json).context("failed to parse disk projection payload")?;
    let projection = base64_standard_decode(&payload.projection_b64)
        .context("disk projection reconcile payload has invalid base64")?;
    let changed =
        agent_doc_crdt_relay_io::reconcile_disk_projection_for_file(&canonical, &projection)?;
    Ok(ControllerDiskProjectionReconcileResult { changed })
}

fn handle_crdt_route_disk_change_signal_rpc(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<ControllerWatchSignalResult> {
    let requested_file = request_file(&request)?;
    let canonical = canonical_controller_request_file(bootstrap, &requested_file);
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let signal: DiskChangeSignal = serde_json::from_str(&payload_json)
        .context("failed to parse disk-change signal payload")?;
    let delivery = signal.into_delivery();
    let action = agent_doc_crdt_relay_io::route_disk_change_signal(&canonical, &delivery)?;
    Ok(ControllerWatchSignalResult {
        action: watch_action_payload(action).to_string(),
    })
}

fn watch_action_payload(action: WatchAction) -> &'static str {
    match action {
        WatchAction::None => "none",
        WatchAction::DeferForEditSettle => "defer_for_edit_settle",
        WatchAction::ReconcileIntoCanonical => "reconcile_into_canonical",
        WatchAction::ApplyAsDiskAuthority => "apply_as_disk_authority",
    }
}

fn watch_action_from_payload(action: &str) -> Result<WatchAction> {
    match action {
        "none" => Ok(WatchAction::None),
        "defer_for_edit_settle" => Ok(WatchAction::DeferForEditSettle),
        "reconcile_into_canonical" => Ok(WatchAction::ReconcileIntoCanonical),
        "apply_as_disk_authority" => Ok(WatchAction::ApplyAsDiskAuthority),
        other => anyhow::bail!("unknown disk-change signal action `{other}`"),
    }
}

/// `#ctrlrespawnenoent` — retry schedule for a controller launch that hits
/// `ENOENT` because a concurrent install is replacing the binary.
///
/// Kept as constants plus a pure schedule function so the budget can be asserted
/// without spawning processes or sleeping (`#wsflake2`: tests that reach for
/// real processes or global state are what made this suite unreliable).
pub(crate) mod launch_enoent {
    pub(crate) const LAUNCH_ENOENT_BACKOFF_INITIAL: std::time::Duration =
        std::time::Duration::from_millis(100);
    pub(crate) const LAUNCH_ENOENT_BACKOFF_MAX: std::time::Duration =
        std::time::Duration::from_millis(1_500);
    pub(crate) const LAUNCH_ENOENT_TOTAL_BUDGET: std::time::Duration =
        std::time::Duration::from_secs(15);

    /// Cumulative wait before each retry, mirroring the loop's
    /// `elapsed + backoff < budget` admission rule.
    ///
    /// Test-only: this is the mirror the suite asserts the real retry loop
    /// against (the loop itself inlines the same rule against the constants
    /// above, which ARE production-used). Without the gate, `-D warnings`
    /// fails the whole workspace clippy leg on a non-test build.
    #[cfg(test)]
    pub(crate) fn retry_schedule_ms() -> Vec<u128> {
        let mut elapsed = std::time::Duration::ZERO;
        let mut backoff = LAUNCH_ENOENT_BACKOFF_INITIAL;
        let mut waits = Vec::new();
        while elapsed + backoff < LAUNCH_ENOENT_TOTAL_BUDGET {
            elapsed += backoff;
            waits.push(elapsed.as_millis());
            backoff = (backoff * 2).min(LAUNCH_ENOENT_BACKOFF_MAX);
        }
        waits
    }
}

fn controller_current_text_from_data(
    data: &serde_json::Value,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    match data.get("status").and_then(|status| status.as_str()) {
        Some("detached") => Ok(agent_doc_crdt_relay_io::CurrentText::Detached),
        Some("editor_attached_model_missing") => {
            Ok(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica)
        }
        Some("editor_sync_pending") => Ok(agent_doc_crdt_relay_io::CurrentText::EditorSyncPending),
        Some("current") => {
            let text = data
                .get("text")
                .and_then(|text| text.as_str())
                .context("controller current text response missing text")?
                .to_string();
            let live_editors = data
                .get("live_editors")
                .and_then(|value| value.as_u64())
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(0);
            let delivery_converged = data
                .get("delivery_converged")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let delivery_version = data
                .get("delivery_version")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            Ok(agent_doc_crdt_relay_io::CurrentText::Current {
                text,
                live_editors,
                delivery_converged,
                delivery_version,
            })
        }
        Some(status) => anyhow::bail!("unknown controller current text status `{status}`"),
        None => anyhow::bail!("controller current text response missing status"),
    }
}

fn controller_current_text_response(
    current: agent_doc_crdt_relay_io::CurrentText,
) -> serde_json::Value {
    match current {
        agent_doc_crdt_relay_io::CurrentText::Detached => {
            serde_json::json!({ "status": "detached" })
        }
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica => {
            serde_json::json!({ "status": "editor_attached_model_missing" })
        }
        agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => {
            serde_json::json!({ "status": "editor_sync_pending" })
        }
        agent_doc_crdt_relay_io::CurrentText::Current {
            text,
            live_editors,
            delivery_converged,
            delivery_version,
        } => serde_json::json!({
            "status": "current",
            "text_len": text.len(),
            "text_hash": agent_doc_hash::content_hash(&text),
            "text": text,
            "live_editors": live_editors,
            "delivery_converged": delivery_converged,
            "delivery_version": delivery_version,
        }),
    }
}

fn log_controller_current_text_result(
    canonical: &Path,
    source: &str,
    current: &agent_doc_crdt_relay_io::CurrentText,
) {
    match current {
        agent_doc_crdt_relay_io::CurrentText::Detached => agent_doc_ops_log_io::log_op(
            canonical,
            &format!(
                "controller_crdt_current_text file={} source={} status=detached authority=cp_model",
                canonical.display(),
                source,
            ),
        ),
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica => {
            agent_doc_ops_log_io::log_op(
                canonical,
                &format!(
                    "controller_crdt_current_text file={} source={} status=editor_attached_model_missing authority=cp_model",
                    canonical.display(),
                    source,
                ),
            )
        }
        agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => agent_doc_ops_log_io::log_op(
            canonical,
            &format!(
                "controller_crdt_current_text file={} source={} status=editor_sync_pending authority=cp_model",
                canonical.display(),
                source,
            ),
        ),
        agent_doc_crdt_relay_io::CurrentText::Current {
            text,
            live_editors,
            delivery_converged,
            delivery_version,
        } => agent_doc_ops_log_io::log_op(
            canonical,
            &format!(
                "controller_crdt_current_text file={} source={} status=current authority=cp_model text_len={} text_hash={} live_editors={} delivery_converged={} delivery_version={}",
                canonical.display(),
                source,
                text.len(),
                agent_doc_hash::content_hash(text),
                live_editors,
                delivery_converged,
                delivery_version,
            ),
        ),
    }
}

#[derive(Debug, Deserialize)]
struct ControllerCrdtReplicaPayload {
    method: String,
    identity: Option<String>,
    #[serde(default)]
    state_vector_b64: Option<String>,
    update_b64: Option<String>,
    patch_id: Option<String>,
    generation: Option<u64>,
    content_hash: Option<String>,
    awareness_b64: Option<String>,
    source: Option<String>,
    /// `#ackeditorstamps`: editor-side wall-clock epoch ms for the first three
    /// moments of the delivery-ACK round trip. Absent from any replica that has
    /// not been updated, so every consumer must treat them as optional.
    #[serde(default)]
    pulled_at_ms: Option<u64>,
    #[serde(default)]
    applied_at_ms: Option<u64>,
    #[serde(default)]
    receipt_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ControllerCrdtCurrentTextPayload {
    source: Option<String>,
    #[serde(default)]
    recover_projection: bool,
    #[serde(default = "default_crdt_current_text_flush_barrier")]
    flush_barrier: bool,
}

fn default_crdt_current_text_flush_barrier() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct ControllerEditorRoutePayload {
    #[serde(default)]
    relative_path: Option<String>,
    #[serde(default)]
    layout_args: Vec<String>,
    #[serde(default)]
    dispatch_only: Option<bool>,
    #[serde(default)]
    plain_trigger: Option<bool>,
    #[serde(default)]
    wait_for_ready_secs: Option<u64>,
    #[serde(default)]
    force_disk: Option<bool>,
    #[serde(default)]
    attempt_id: Option<String>,
    #[serde(default)]
    route_key: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    selected_text: Option<String>,
    #[serde(default)]
    steering_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct ControllerEditorRouteResult {
    exit_code: i32,
    output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    steering: Option<ControllerTurnSteeringReceipt>,
}

fn handle_crdt_current_text_rpc(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<serde_json::Value> {
    let requested_file = request_file(&request)?;
    let canonical = canonical_controller_request_file(bootstrap, &requested_file);
    let file_arg = canonical.to_string_lossy().to_string();
    let payload = crdt_current_text_payload(&request)?;
    let source = crdt_current_text_source(&payload);
    let authority = crdt_authority_for_file(&file_arg);
    if !authority.editor_attached() {
        let current = agent_doc_crdt_relay_io::CurrentText::Detached;
        log_controller_current_text_result(&canonical, &source, &current);
        return Ok(controller_current_text_response(current));
    }
    let current = if payload.recover_projection {
        agent_doc_crdt_relay_io::current_text_for_file_with_authority_recovering_projection(
            &canonical, authority,
        )?
    } else if !payload.flush_barrier {
        agent_doc_crdt_relay_io::current_text_for_file_with_authority_nonblocking(
            &canonical, authority,
        )?
    } else {
        agent_doc_crdt_relay_io::current_text_for_file_with_authority(&canonical, authority)?
    };
    log_controller_current_text_result(&canonical, &source, &current);
    Ok(controller_current_text_response(current))
}

fn handle_crdt_revision_rpc(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<serde_json::Value> {
    let requested_file = request_file(&request)?;
    let canonical = canonical_controller_request_file(bootstrap, &requested_file);
    let file_arg = canonical.to_string_lossy().to_string();
    let authority = crdt_authority_for_file(&file_arg);
    let revision =
        agent_doc_crdt_relay_io::current_revision_for_file_with_authority(&canonical, authority)?;
    serde_json::to_value(revision).context("failed to serialize controller CRDT revision response")
}

fn crdt_current_text_payload(
    request: &ControllerRequest,
) -> Result<ControllerCrdtCurrentTextPayload> {
    let Some(payload_json) = request.diagnostic_payload.as_deref() else {
        return Ok(ControllerCrdtCurrentTextPayload {
            source: None,
            recover_projection: false,
            flush_barrier: default_crdt_current_text_flush_barrier(),
        });
    };
    if payload_json.trim().is_empty() {
        return Ok(ControllerCrdtCurrentTextPayload {
            source: None,
            recover_projection: false,
            flush_barrier: default_crdt_current_text_flush_barrier(),
        });
    }
    serde_json::from_str(payload_json).context("failed to parse CRDT current-text payload")
}

fn crdt_current_text_source(payload: &ControllerCrdtCurrentTextPayload) -> String {
    payload
        .source
        .as_deref()
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .unwrap_or("controller_model")
        .to_string()
}

fn handle_crdt_cp_write_rpc(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<ControllerCrdtCpWriteResult> {
    let requested_file = request_file(&request)?;
    let canonical = canonical_controller_request_file(bootstrap, &requested_file);
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: ControllerCrdtCpWritePayload =
        serde_json::from_str(&payload_json).context("failed to parse CRDT CP write payload")?;
    let source = payload
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("controller_cp_write");
    let write = agent_doc_crdt_relay_io::apply_cp_write_for_file(
        &canonical,
        &payload.expected_current,
        &payload.content,
        source,
    )?;
    Ok(ControllerCrdtCpWriteResult { write })
}

fn handle_response_cell_add_rpc(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<ControllerResponseCellAddResult> {
    let requested_file = request_file(&request)?;
    let canonical = canonical_controller_request_file(bootstrap, &requested_file);
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: ControllerResponseCellAddPayload =
        serde_json::from_str(&payload_json).context("failed to parse response cell add payload")?;
    let source = payload
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("controller_response_cell_add");
    let write = agent_doc_crdt_relay_io::add_response_cell_for_file(
        &canonical,
        payload.committed_content.as_deref(),
        &payload.response,
        source,
    )?;
    if let Some(write) = &write
        && !payload.checkpoint_only
    {
        agent_doc_cycle_state_io::append_response_cell_added(
            &canonical,
            &payload.cycle_id,
            &payload.operation_id,
            &write.cell_id,
            &payload.response_sha256,
            &write.content_hash,
            write.applied,
        )?;
    }
    Ok(ControllerResponseCellAddResult { write })
}

fn handle_crdt_text_adopt_rpc(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<ControllerCrdtTextAdoptResult> {
    let requested_file = request_file(&request)?;
    let canonical = canonical_controller_request_file(bootstrap, &requested_file);
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: ControllerCrdtTextAdoptPayload =
        serde_json::from_str(&payload_json).context("failed to parse CRDT text-adopt payload")?;
    let source = payload
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("controller_text_adopt");
    let changed = agent_doc_crdt_relay_io::adopt_editor_text_for_file(&canonical, &payload.text)?;
    agent_doc_ops_log_io::log_op(
        &canonical,
        &format!(
            "controller_crdt_text_adopt file={} source={} changed={} content_hash={}",
            canonical.display(),
            source,
            changed
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unavailable".to_string()),
            agent_doc_hash::content_hash(&payload.text),
        ),
    );
    Ok(ControllerCrdtTextAdoptResult { changed })
}

fn handle_crdt_replica_rpc(
    bootstrap: &ControllerBootstrap,
    runtime: Option<&ControllerRuntime>,
    request: ControllerRequest,
) -> Result<serde_json::Value> {
    let requested_file = request_file(&request)?;
    let canonical = canonical_controller_request_file(bootstrap, &requested_file);
    let file_arg = canonical.to_string_lossy().to_string();
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: ControllerCrdtReplicaPayload =
        serde_json::from_str(&payload_json).context("failed to parse CRDT replica payload")?;
    let source = payload.source.as_deref().unwrap_or("jetbrains_plugin");
    let identity = payload
        .identity
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("CRDT replica payload missing identity")?;
    let method_name = payload.method.as_str();
    let authority = crdt_authority_for_file(&file_arg);
    if !authority.editor_attached() {
        agent_doc_ops_log_io::log_op(
            &canonical,
            &format!(
                "controller_crdt_replica_refused file={} method={} source={} reason=detached_authority",
                canonical.display(),
                method_name,
                source,
            ),
        );
        return Ok(crdt_replica_refused_data("detached_authority"));
    }

    // Capture the rolling Lazily/CRDT canonical before accepting the editor
    // delta. Queue control gestures must be compared with the immediately
    // preceding fixed point, not the cycle-opening merge snapshot: otherwise a
    // stop→resume marker edit is indistinguishable from the stale marker that
    // the preceding frontmatter edit just overrode.
    let previous_text = if method_name == "replica_update" {
        match agent_doc_crdt_relay_io::current_text_for_file_with_authority_nonblocking(
            &canonical, authority,
        )? {
            agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => Some(text),
            agent_doc_crdt_relay_io::CurrentText::Detached
            | agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
            | agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => None,
        }
    } else {
        None
    };
    let data = controller_crdt_replica_data(&canonical, method_name, identity, &payload)?;
    if method_name == "replica_update"
        && let Some(runtime) = runtime
        && let Err(error) = observe_realtime_steering_after_replica_update(
            bootstrap,
            runtime,
            &canonical,
            authority,
            previous_text.as_deref(),
        )
    {
        // Steering is an observational projection over the already-accepted
        // CRDT update. Preserve the edit and retry projection on the next
        // replica update instead of failing the editor's canonical mutation.
        agent_doc_ops_log_io::log_op(
            &canonical,
            &format!(
                "realtime_steering_projection_deferred file={} reason={error:#}",
                canonical.display(),
            ),
        );
    }
    agent_doc_ops_log_io::log_op(
        &canonical,
        &format!(
            "controller_crdt_replica_handled file={} method={} source={} authority=cp_model data_kind={}",
            canonical.display(),
            method_name,
            source,
            data.get("kind")
                .and_then(|kind| kind.as_str())
                .unwrap_or("ok"),
        ),
    );
    Ok(data)
}

fn observe_realtime_steering_after_replica_update(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    canonical: &Path,
    authority: agent_doc_document_realtime::crdt_authority::CrdtAuthority,
    previous_text: Option<&str>,
) -> Result<bool> {
    let document_hash = agent_doc_hash::document_id_for_path(canonical);
    let state = runtime.document_state_projection(&document_hash)?;
    // `#qactsync-live`: the controller projection is the Lazily-owned hot
    // state. Do not reopen SQLite or take a snapshot lock for every keystroke.
    let baseline = state
        .as_ref()
        .and_then(|state| state.document.merge_baseline.as_ref())
        .map(|baseline| baseline.content.as_str());
    let agent_doc_crdt_relay_io::CurrentText::Current { text, .. } =
        agent_doc_crdt_relay_io::current_text_for_file_with_authority_nonblocking(
            canonical, authority,
        )?
    else {
        return Ok(false);
    };

    let (converged, changed) =
        agent_doc_queue::control_binding::converge_queue_control_binding_content(
            &text,
            previous_text,
        )?;
    let current = if changed {
        // The CRDT relay is the existing durable effect sink. Feed the pure
        // convergence result into it; do not add a second journal or watcher.
        agent_doc_crdt_relay_io::apply_cp_write_for_file(
            canonical,
            &text,
            &converged,
            "realtime_queue_control_binding",
        )?;
        converged.as_str()
    } else {
        text.as_str()
    };

    let Some(state) = state.as_ref() else {
        return Ok(changed);
    };
    let (Some(cycle_id), Some(phase)) = (state.closeout.cycle_id.as_deref(), state.closeout.phase)
    else {
        return Ok(changed);
    };
    if !phase.is_open() {
        return Ok(changed);
    }
    let Some(baseline) = baseline else {
        return Ok(changed);
    };
    let event = realtime_steering_event_for_text(&document_hash, cycle_id, baseline, current);
    let observed = append_apply_state_event(bootstrap, runtime, event)?;
    Ok(changed || observed)
}

fn realtime_steering_event_for_text(
    document_hash: &str,
    cycle_id: &str,
    baseline: &str,
    current: &str,
) -> agent_doc_state_backbone::StateEvent {
    let content_hash = agent_doc_hash::content_hash(current);
    let steering = agent_doc_document_realtime::baseline_comparison::BaselineComparison::new(
        baseline, current,
    )
    .realtime_steering_all()
    .turn_projection();
    agent_doc_state_backbone::StateEvent::new(
        format!("realtime-steering:{document_hash}:{cycle_id}:{content_hash}"),
        agent_doc_state_backbone::StateFact::RealtimeSteeringObserved {
            document_hash: document_hash.to_string(),
            cycle_id: cycle_id.to_string(),
            steering,
            content_hash,
        },
    )
}

fn handle_editor_route_rpc(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<ControllerEditorRouteResult> {
    let requested_file = request_file(&request)?;
    let canonical = canonical_controller_request_file(bootstrap, &requested_file);
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: ControllerEditorRoutePayload =
        serde_json::from_str(&payload_json).context("failed to parse editor route payload")?;
    let relative_path = editor_route_relative_path(bootstrap, &canonical, &payload)?;
    let layout_args = validate_editor_route_layout_args(&payload.layout_args)?;
    let wait_secs = payload.wait_for_ready_secs.unwrap_or(15).min(600);
    let source = payload
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("jetbrains_plugin");

    if payload.selected_text.is_some() || payload.steering_id.is_some() {
        agent_doc_ops_log_io::log_op(
            &canonical,
            &format!(
                "controller_editor_route_legacy_steering_normalized file={} source={} steering_id={} selected_bytes={} outcome=plain_trigger",
                canonical.display(),
                source,
                payload.steering_id.as_deref().unwrap_or("none"),
                payload.selected_text.as_deref().map(str::len).unwrap_or(0),
            ),
        );
    }

    agent_doc_ops_log_io::log_op(
        &canonical,
        &format!(
            "controller_editor_route_started file={} source={} relative_path={} layout_args={} attempt_id={} route_key={}",
            canonical.display(),
            source,
            relative_path,
            layout_args.len(),
            payload.attempt_id.as_deref().unwrap_or("none"),
            payload.route_key.as_deref().unwrap_or("none"),
        ),
    );

    let _attempt_guard =
        crate::route_snapshot::EditorRouteAttemptIdGuard::set(payload.attempt_id.as_deref());
    let result = runtime_effects()?.run_editor_route(ControllerEditorRouteInvocation {
        file: canonical.clone(),
        relative_path: relative_path.clone(),
        pane: None,
        layout_args,
        dispatch_only: payload.dispatch_only.unwrap_or(true),
        plain_trigger: payload.plain_trigger.unwrap_or(true),
        wait_for_ready_secs: Some(wait_secs),
        force_disk: payload.force_disk.unwrap_or(false),
        prune_before_lookup: true,
    })?;
    agent_doc_ops_log_io::log_op(
        &canonical,
        &format!(
            "controller_editor_route_settled file={} source={} relative_path={} exit_code={} output_bytes={} attempt_id={} route_key={}",
            canonical.display(),
            source,
            relative_path,
            result.exit_code,
            result.output.len(),
            payload.attempt_id.as_deref().unwrap_or("none"),
            payload.route_key.as_deref().unwrap_or("none"),
        ),
    );
    Ok(ControllerEditorRouteResult {
        exit_code: result.exit_code,
        output: result.output,
        steering: None,
    })
}

/// Shadow endpoint for the lazily command/RPC message plane (`command-plane-v1`,
/// `#lzmsgpcp`). Decodes an `agent-doc.*.v1` payload from a `CommandSubmit`,
/// dispatches the existing controller path, and returns a folded
/// `CommandProjection` plus the progress events and the terminal causal receipt.
///
/// This is additive: the classic endpoints stay available when
/// `AGENT_DOC_COMMAND_PLANE=0`. Terminal authority is the causal receipt, not
/// the transport ACK — command failures become terminal `rejected` receipts in
/// the returned projection, not transport errors.
fn handle_editor_command_submit_rpc(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<serde_json::Value> {
    let (submit, payload_json) = parse_editor_command_submit_request(&request)?;
    let command_result =
        dispatch_command_submit_payload(bootstrap, &request, &submit, payload_json);
    terminal_command_submit_response(&submit, command_result)
}

fn parse_editor_command_submit_request(
    request: &ControllerRequest,
) -> Result<(lazily::CommandSubmit, String)> {
    let submit_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let message: lazily::CommandMessage =
        serde_json::from_str(&submit_json).context("failed to parse CommandSubmit envelope")?;
    let submit = match message {
        lazily::CommandMessage::CommandSubmit(submit) => submit,
        other => anyhow::bail!("editor_command_submit expects a CommandSubmit, got {other:?}"),
    };

    if submit.namespace != "agent-doc" {
        anyhow::bail!(
            "unsupported command namespace: {} for {}",
            submit.namespace,
            submit.name
        );
    }
    let expected_payload_prefix = format!("agent-doc.{}.", submit.name);
    if !submit.payload_type.starts_with(&expected_payload_prefix) {
        anyhow::bail!(
            "unsupported payload_type for {}: {}",
            submit.name,
            submit.payload_type
        );
    }

    let payload_bytes = match &submit.payload {
        lazily::IpcValue::Inline(bytes) => bytes.clone(),
        lazily::IpcValue::SharedBlob(_) => {
            anyhow::bail!("shared-blob command payloads are not supported by the shadow endpoint")
        }
    };
    let payload_json = String::from_utf8(payload_bytes).context("command payload is not UTF-8")?;

    Ok((*submit, payload_json))
}

fn command_submit_progress_events(
    command_id: &str,
    generation: u64,
    include_started: bool,
) -> lazily::CommandEvents {
    let mut events = vec![
        lazily::CommandEvent {
            event_id: format!("{command_id}-observed"),
            command_id: command_id.to_string(),
            kind: lazily::CommandEventKind::Observed,
            generation,
            detail: None,
        },
        lazily::CommandEvent {
            event_id: format!("{command_id}-accepted"),
            command_id: command_id.to_string(),
            kind: lazily::CommandEventKind::Accepted,
            generation,
            detail: None,
        },
    ];
    if include_started {
        events.push(lazily::CommandEvent {
            event_id: format!("{command_id}-started"),
            command_id: command_id.to_string(),
            kind: lazily::CommandEventKind::Started,
            generation,
            detail: None,
        });
    }
    lazily::CommandEvents { events }
}

fn command_submit_projection(
    submit: &lazily::CommandSubmit,
    progress: &lazily::CommandEvents,
) -> lazily::CommandProjection {
    let mut projection = lazily::CommandProjection::new();
    projection.submit(submit);
    projection.apply_message(&lazily::CommandMessage::CommandEvents(progress.clone()));
    projection
}

fn terminal_command_submit_response(
    submit: &lazily::CommandSubmit,
    command_result: CommandSubmitDispatchResult,
) -> Result<serde_json::Value> {
    let command_id = submit.command_id.clone();
    let generation = submit.authority_generation;
    let progress = command_submit_progress_events(&command_id, generation, true);
    let mut projection = command_submit_projection(submit, &progress);
    let receipt_id = format!("{command_id}-receipt");
    let receipt = if command_result.terminal_applied {
        lazily::applied_receipt(&receipt_id, &command_id, "project-controller", generation)
    } else {
        lazily::rejected_receipt(
            &receipt_id,
            &command_id,
            "project-controller",
            generation,
            command_result.terminal_reason.clone().unwrap_or_else(|| {
                format!("{} exit_code={}", submit.name, command_result.exit_code)
            }),
        )
    };
    projection.observe_receipt(&receipt);

    Ok(serde_json::json!({
        "command_id": command_id,
        "exit_code": command_result.exit_code,
        "output": command_result.output,
        "payload": command_result.payload,
        "projection": serde_json::to_value(projection.to_image())?,
        "events": serde_json::to_value(progress)?,
        "receipt": serde_json::to_value(receipt)?,
    }))
}

const ASYNC_EDITOR_COMMAND_RESULT_TTL: Duration = Duration::from_secs(5 * 60);
const ASYNC_EDITOR_COMMAND_RESULT_CAPACITY: usize = 256;

#[derive(Clone)]
struct AsyncEditorCommandResult {
    response: serde_json::Value,
    updated_at: std::time::Instant,
}

fn async_editor_command_results()
-> &'static parking_lot::Mutex<std::collections::HashMap<String, AsyncEditorCommandResult>> {
    static RESULTS: std::sync::LazyLock<
        parking_lot::Mutex<std::collections::HashMap<String, AsyncEditorCommandResult>>,
    > = std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));
    &RESULTS
}

fn retain_async_editor_command_result(command_id: &str, response: serde_json::Value) {
    let now = std::time::Instant::now();
    let mut results = async_editor_command_results().lock();
    results.retain(|_, result| {
        now.duration_since(result.updated_at) <= ASYNC_EDITOR_COMMAND_RESULT_TTL
    });
    if results.len() >= ASYNC_EDITOR_COMMAND_RESULT_CAPACITY
        && !results.contains_key(command_id)
        && let Some(oldest) = results
            .iter()
            .min_by_key(|(_, result)| result.updated_at)
            .map(|(id, _)| id.clone())
    {
        results.remove(&oldest);
    }
    results.insert(
        command_id.to_string(),
        AsyncEditorCommandResult {
            response,
            updated_at: now,
        },
    );
}

fn async_editor_command_result(command_id: &str) -> Option<serde_json::Value> {
    let now = std::time::Instant::now();
    let mut results = async_editor_command_results().lock();
    results.retain(|_, result| {
        now.duration_since(result.updated_at) <= ASYNC_EDITOR_COMMAND_RESULT_TTL
    });
    results
        .get(command_id)
        .map(|result| result.response.clone())
}

#[derive(Deserialize)]
struct EditorCommandStatusPayload {
    command_id: String,
}

fn handle_editor_command_status_rpc(request: ControllerRequest) -> Result<serde_json::Value> {
    let payload: EditorCommandStatusPayload = serde_json::from_str(&request_string(
        &request.diagnostic_payload,
        "diagnostic_payload",
    )?)
    .context("parse editor_command_status payload")?;
    async_editor_command_result(&payload.command_id).with_context(|| {
        format!(
            "unknown or expired async editor command: {}",
            payload.command_id
        )
    })
}

/// Submit an editor command and return after CP admission.
///
/// This is the message-passing fast path for editor gestures such as
/// `Sync Tmux Layout` and focus handoff. The terminal `editor_command_submit`
/// endpoint above intentionally remains available for RPC callers that need the
/// final causal receipt before returning.
fn handle_editor_command_submit_async_rpc(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<serde_json::Value> {
    let (submit, payload_json) = parse_editor_command_submit_request(&request)?;
    validate_async_editor_command_payload(&submit, &payload_json)?;

    let command_id = submit.command_id.clone();
    let generation = submit.authority_generation;
    let progress = command_submit_progress_events(&command_id, generation, false);
    let projection = command_submit_projection(&submit, &progress);
    let accepted_response = serde_json::json!({
        "command_id": command_id,
        "exit_code": 0,
        "output": format!("{} accepted", submit.name),
        "payload": {
            "accepted": true,
            "command": submit.name,
            "command_id": command_id,
        },
        "projection": serde_json::to_value(projection.to_image())?,
        "events": serde_json::to_value(progress)?,
        "receipt": serde_json::Value::Null,
    });
    retain_async_editor_command_result(&command_id, accepted_response.clone());

    let worker_bootstrap = bootstrap.clone();
    let worker_request = request.clone();
    let worker_submit = submit.clone();
    let worker_project_root = bootstrap.project_root.clone();
    let worker_command_id = command_id.clone();
    let worker_name = submit.name.clone();
    if let Err(err) = spawn_editor_command_async_worker(move || {
        let result = dispatch_command_submit_payload(
            &worker_bootstrap,
            &worker_request,
            &worker_submit,
            payload_json,
        );
        let terminal_reason = result.terminal_reason.as_deref().unwrap_or("");
        agent_doc_ops_log_io::log_op(
            &worker_project_root,
            &format!(
                "editor_command_async_completed command={} command_id={} exit_code={} applied={} reason={} output={}",
                worker_name,
                worker_command_id,
                result.exit_code,
                result.terminal_applied,
                terminal_reason,
                compact_command_output(&result.output),
            ),
        );
        let terminal_response = terminal_command_submit_response(&worker_submit, result)
            .unwrap_or_else(|response_err| {
                serde_json::json!({
                    "command_id": worker_command_id,
                    "exit_code": 1,
                    "output": format!("failed to encode async command completion: {response_err:#}"),
                })
            });
        retain_async_editor_command_result(&worker_command_id, terminal_response);
    }) {
        async_editor_command_results().lock().remove(&command_id);
        return Err(err);
    }

    Ok(accepted_response)
}

fn validate_async_editor_command_payload(
    submit: &lazily::CommandSubmit,
    payload_json: &str,
) -> Result<()> {
    match submit.name.as_str() {
        "sync_tmux_layout" => {
            let _: ControllerTmuxLayoutSyncInvocation =
                serde_json::from_str(payload_json).context("parse sync_tmux_layout payload")?;
        }
        "focus_document_pane" => {
            let _: FocusDocumentPaneCommandPayload =
                serde_json::from_str(payload_json).context("parse focus_document_pane payload")?;
        }
        "editor_route" => {
            let _: ControllerEditorRoutePayload =
                serde_json::from_str(payload_json).context("parse editor_route payload")?;
        }
        other => anyhow::bail!("unsupported async editor command: {other}"),
    }
    Ok(())
}

fn compact_command_output(output: &str) -> String {
    output.replace('\n', " | ").chars().take(320).collect()
}

#[cfg(any(test, feature = "test-support"))]
fn spawn_editor_command_async_worker<F>(work: F) -> Result<()>
where
    F: FnOnce() + Send + 'static,
{
    work();
    Ok(())
}

#[cfg(not(any(test, feature = "test-support")))]
fn spawn_editor_command_async_worker<F>(work: F) -> Result<()>
where
    F: FnOnce() + Send + 'static,
{
    std::thread::Builder::new()
        .name("agent-doc-editor-command-async".to_string())
        .spawn(work)
        .context("failed to spawn editor command async worker")?;
    Ok(())
}

struct CommandSubmitDispatchResult {
    exit_code: i32,
    output: String,
    payload: serde_json::Value,
    terminal_applied: bool,
    terminal_reason: Option<String>,
}

impl CommandSubmitDispatchResult {
    fn applied<T: Serialize>(output: String, payload: &T) -> Result<Self> {
        Ok(Self {
            exit_code: 0,
            output,
            payload: serde_json::to_value(payload)?,
            terminal_applied: true,
            terminal_reason: None,
        })
    }

    fn rejected(command_name: &str, reason: String) -> Self {
        Self {
            exit_code: 1,
            output: reason.clone(),
            payload: serde_json::json!({ "error": reason }),
            terminal_applied: false,
            terminal_reason: Some(format!("{command_name} failed")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct FocusDocumentPaneCommandPayload {
    document_path: String,
    #[serde(default)]
    project_root: Option<String>,
    #[serde(default)]
    no_promotion: bool,
    #[serde(default)]
    active_window_guard: bool,
}

fn empty_controller_request(command: &str) -> ControllerRequest {
    ControllerRequest {
        command: command.to_string(),
        file: None,
        session_id: None,
        pane_id: None,
        window_id: None,
        generation: None,
        state: None,
        caller: None,
        reason: None,
        supervisor_pid: None,
        supervisor_socket: None,
        command_kind: None,
        diagnostic_payload: None,
    }
}

fn dispatch_command_submit_payload(
    bootstrap: &ControllerBootstrap,
    request: &ControllerRequest,
    submit: &lazily::CommandSubmit,
    payload_json: String,
) -> CommandSubmitDispatchResult {
    match submit.name.as_str() {
        "editor_route" => {
            let mut route_request = empty_controller_request("editor_route");
            route_request.file = request.file.clone();
            route_request.diagnostic_payload = Some(payload_json);
            match handle_editor_route_rpc(bootstrap, route_request) {
                Ok(result) => {
                    let terminal_applied = result.exit_code == 0;
                    let terminal_reason = (!terminal_applied)
                        .then(|| format!("editor_route exit_code={}", result.exit_code));
                    CommandSubmitDispatchResult {
                        exit_code: result.exit_code,
                        output: result.output.clone(),
                        payload: serde_json::to_value(&result).unwrap_or(serde_json::Value::Null),
                        terminal_applied,
                        terminal_reason,
                    }
                }
                Err(err) => {
                    CommandSubmitDispatchResult::rejected("editor_route", format!("{err:#}"))
                }
            }
        }
        "sync_tmux_layout" => {
            let invocation: ControllerTmuxLayoutSyncInvocation =
                match serde_json::from_str(&payload_json) {
                    Ok(invocation) => invocation,
                    Err(err) => {
                        return CommandSubmitDispatchResult::rejected(
                            "sync_tmux_layout",
                            format!("parse sync_tmux_layout payload: {err:#}"),
                        );
                    }
                };
            let mut sync_request = empty_controller_request("sync_tmux_layout");
            sync_request.file = invocation.focus.as_ref().map(PathBuf::from);
            sync_request.diagnostic_payload = Some(match serde_json::to_string(&invocation) {
                Ok(payload) => payload,
                Err(err) => {
                    return CommandSubmitDispatchResult::rejected(
                        "sync_tmux_layout",
                        format!("serialize sync_tmux_layout payload: {err:#}"),
                    );
                }
            });
            match handle_sync_tmux_layout(bootstrap, sync_request) {
                Ok(receipt) => {
                    let output =
                        serde_json::to_string(&receipt).unwrap_or_else(|_| receipt.reason.clone());
                    CommandSubmitDispatchResult::applied(output, &receipt).unwrap_or_else(|err| {
                        CommandSubmitDispatchResult::rejected(
                            "sync_tmux_layout",
                            format!("serialize sync_tmux_layout receipt: {err:#}"),
                        )
                    })
                }
                Err(err) => {
                    CommandSubmitDispatchResult::rejected("sync_tmux_layout", format!("{err:#}"))
                }
            }
        }
        "focus_document_pane" => {
            let payload: FocusDocumentPaneCommandPayload = match serde_json::from_str(&payload_json)
            {
                Ok(payload) => payload,
                Err(err) => {
                    return CommandSubmitDispatchResult::rejected(
                        "focus_document_pane",
                        format!("parse focus_document_pane payload: {err:#}"),
                    );
                }
            };
            let _guard_flags = (
                payload.project_root.as_deref(),
                payload.no_promotion,
                payload.active_window_guard,
            );
            let mut focus_request = empty_controller_request("focus_document_pane");
            focus_request.file = Some(PathBuf::from(payload.document_path));
            match handle_focus_document_pane(bootstrap, focus_request) {
                Ok(receipt) => {
                    let output =
                        serde_json::to_string(&receipt).unwrap_or_else(|_| receipt.reason.clone());
                    CommandSubmitDispatchResult::applied(output, &receipt).unwrap_or_else(|err| {
                        CommandSubmitDispatchResult::rejected(
                            "focus_document_pane",
                            format!("serialize focus_document_pane receipt: {err:#}"),
                        )
                    })
                }
                Err(err) => {
                    CommandSubmitDispatchResult::rejected("focus_document_pane", format!("{err:#}"))
                }
            }
        }
        other => CommandSubmitDispatchResult::rejected(
            other,
            format!("unsupported agent-doc command: {other}"),
        ),
    }
}

fn editor_route_relative_path(
    bootstrap: &ControllerBootstrap,
    canonical: &Path,
    payload: &ControllerEditorRoutePayload,
) -> Result<String> {
    if let Some(relative_path) = payload
        .relative_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        anyhow::ensure!(
            !relative_path.starts_with('-'),
            "editor route relative_path must not start with '-'"
        );
        return Ok(relative_path.to_string());
    }

    let project_root = bootstrap
        .project_root
        .canonicalize()
        .unwrap_or_else(|_| bootstrap.project_root.clone());
    let path = canonical
        .strip_prefix(&project_root)
        .unwrap_or(canonical)
        .to_string_lossy()
        .to_string();
    anyhow::ensure!(!path.trim().is_empty(), "editor route path is empty");
    anyhow::ensure!(
        !path.starts_with('-'),
        "editor route path must not start with '-'"
    );
    Ok(path)
}

fn validate_editor_route_layout_args(args: &[String]) -> Result<Vec<String>> {
    let mut validated = Vec::with_capacity(args.len());
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--col" | "--focus" => {
                let value = iter
                    .next()
                    .with_context(|| format!("editor route layout arg {flag} missing value"))?;
                let empty_column_placeholder = flag == "--col" && value.is_empty();
                anyhow::ensure!(
                    empty_column_placeholder || !value.trim().is_empty(),
                    "editor route layout arg {flag} has empty value"
                );
                validated.push(flag.clone());
                validated.push(value.clone());
            }
            other => anyhow::bail!("unsupported editor route layout arg `{other}`"),
        }
    }
    Ok(validated)
}

fn controller_crdt_replica_data(
    canonical: &Path,
    method_name: &str,
    identity: &str,
    payload: &ControllerCrdtReplicaPayload,
) -> Result<serde_json::Value> {
    match method_name {
        "replica_register" => {
            let retained_state_vector = payload
                .state_vector_b64
                .as_deref()
                .map(base64_standard_decode)
                .transpose()
                .context("CRDT replica register payload has invalid state_vector_b64")?;
            match agent_doc_crdt_relay_io::register_replica_for_file_incremental(
                canonical,
                identity,
                retained_state_vector.as_deref(),
            )? {
                Some(registration) => Ok(serde_json::json!({
                    "client_id": registration.client_id,
                    "bootstrap_b64": base64_standard_encode(&registration.bootstrap),
                    "bootstrap_kind": if registration.incremental { "delta" } else { "full" },
                    "canonical_state_vector_b64": base64_standard_encode(
                        &registration.canonical_state_vector
                    ),
                    "lineage": agent_doc_crdt_relay_io::current_lineage_for_file(canonical)?
                        .context("registered replica is missing its canonical lineage")?,
                })),
                None => Ok(crdt_replica_refused_data("detached_authority")),
            }
        }
        "replica_deregister" => {
            let removed =
                agent_doc_crdt_relay_io::deregister_replica_for_file(canonical, identity)?;
            Ok(serde_json::json!({ "removed": removed }))
        }
        "replica_update" => {
            let update_b64 = payload
                .update_b64
                .as_deref()
                .context("CRDT replica update payload missing update_b64")?;
            let update = base64_standard_decode(update_b64)
                .context("CRDT replica update payload has invalid base64")?;
            match agent_doc_crdt_relay_io::relay_replica_update_for_file(
                canonical, identity, &update,
            )? {
                Some(fan_out) => {
                    let targets: Vec<serde_json::Value> = fan_out
                        .targets
                        .iter()
                        .map(|target| {
                            serde_json::json!({
                                "client_id": target,
                                "update_b64": base64_standard_encode(&fan_out.update),
                            })
                        })
                        .collect();
                    Ok(serde_json::json!({
                        "origin": fan_out.origin,
                        "canonical_len": fan_out.canonical_len,
                        "targets": targets,
                    }))
                }
                None => Ok(crdt_replica_refused_data("detached_authority")),
            }
        }
        "replica_pull" => {
            if let Some(canonical_text) =
                agent_doc_crdt_relay_io::pull_rebootstrap_for_file(canonical, identity)?
            {
                return Ok(serde_json::json!({
                    "kind": "replace",
                    "replace": canonical_text,
                }));
            }
            match agent_doc_crdt_relay_io::pull_replica_updates_for_file(canonical, identity)? {
                Some(pull) => {
                    let updates: Vec<serde_json::Value> = pull
                        .updates
                        .iter()
                        .map(|update| {
                            serde_json::json!({
                                "patch_id": update.patch_id,
                                "origin": update.origin,
                                "target": update.target,
                                "generation": update.generation,
                                "expected_content_hash": update.expected_content_hash,
                                "update_b64": base64_standard_encode(&update.update),
                            })
                        })
                        .collect();
                    Ok(serde_json::json!({
                        "kind": "delta",
                        "client_id": pull.client_id,
                        "updates": updates,
                        "current_generation": pull.delivery.current_generation,
                        "last_ack_generation": pull.delivery.last_ack_generation,
                        "pending_updates": pull.delivery.pending_updates,
                    }))
                }
                None => Ok(crdt_replica_refused_data("detached_authority")),
            }
        }
        "replica_ack" => {
            let patch_id = payload
                .patch_id
                .as_deref()
                .context("CRDT replica ack payload missing patch_id")?;
            let generation = payload
                .generation
                .context("CRDT replica ack payload missing generation")?;
            // `#ackeditorstamps`: stamp the fourth moment — the binary observing
            // the receipt — before the ack is applied, so the reported leg is the
            // transport, not the relay's own bookkeeping.
            let observed_at_ms = editor_ack_observed_at_ms();
            let editor_profile = render_editor_ack_profile(
                payload.pulled_at_ms,
                payload.applied_at_ms,
                payload.receipt_at_ms,
                observed_at_ms,
            );
            match agent_doc_crdt_relay_io::ack_replica_update_for_file_with_content_hash(
                canonical,
                identity,
                patch_id,
                generation,
                payload.content_hash.as_deref(),
            )? {
                Some(acknowledged) => {
                    if let Some(profile) = editor_profile {
                        agent_doc_ops_log_io::log_op(
                            canonical,
                            &format!(
                                "crdt_replica_ack_editor_profile file={} patch_id={patch_id} generation={generation} acknowledged={acknowledged} profile=[{profile}]",
                                canonical.display(),
                            ),
                        );
                    }
                    Ok(serde_json::json!({ "acknowledged": acknowledged }))
                }
                None => Ok(crdt_replica_refused_data("detached_authority")),
            }
        }
        "replica_awareness" => {
            let awareness_b64 = payload
                .awareness_b64
                .as_deref()
                .context("CRDT replica awareness payload missing awareness_b64")?;
            let json = base64_standard_decode(awareness_b64)
                .context("CRDT replica awareness payload has invalid base64")?;
            let state: agent_doc_document_realtime::crdt_relay::AwarenessState =
                serde_json::from_slice(&json)
                    .context("CRDT replica awareness payload has invalid JSON")?;
            match agent_doc_crdt_relay_io::set_replica_awareness_for_file(
                canonical, identity, state,
            )? {
                Some(snapshot) => {
                    let presence: Vec<serde_json::Value> = snapshot
                        .iter()
                        .map(|(client_id, state)| {
                            serde_json::json!({
                                "client_id": client_id,
                                "awareness_b64": base64_standard_encode(
                                    &serde_json::to_vec(state).unwrap_or_default()
                                ),
                            })
                        })
                        .collect();
                    Ok(serde_json::json!({ "presence": presence }))
                }
                None => Ok(crdt_replica_refused_data("detached_authority")),
            }
        }
        other => anyhow::bail!("unsupported CRDT replica method `{other}`"),
    }
}

fn editor_ack_observed_at_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// Decompose the delivery-ACK round trip into its three editor-side legs
/// (`#ackeditorstamps`).
///
/// The binary's own `profile=[state=Nms]` breakdown attributes a wait among the
/// states the binary can see, which leaves the editor opaque: an
/// `delivery_ack_pending=11000ms` could be a late delivery, a slow apply, or a
/// fast apply with a slow receipt, and those have nothing in common as fixes.
/// Two hypotheses about this same latency were each disproved the instant a log
/// line carried an attribution field, so emit the legs rather than a total.
///
/// Legs are only rendered when both of their endpoints are stamped. An
/// unstamped end (an older plugin, or a self-echo ACK that never went through
/// the buffer) renders nothing for that leg — a missing leg is a fact, whereas
/// substituting `0` would silently report the epoch as a timestamp. The whole
/// profile is `None` when no leg is derivable, so an un-updated replica adds no
/// log noise.
///
/// The two ends are different processes, so a leg can be negative under clock
/// skew; it is reported as `skew` rather than clamped to `0`, because a clamped
/// zero reads as "instant" and would be the wrong conclusion.
fn render_editor_ack_profile(
    pulled_at_ms: Option<u64>,
    applied_at_ms: Option<u64>,
    receipt_at_ms: Option<u64>,
    observed_at_ms: u64,
) -> Option<String> {
    fn leg(name: &str, from: Option<u64>, to: Option<u64>) -> Option<String> {
        let (from, to) = (from?, to?);
        Some(match to.checked_sub(from) {
            Some(delta) => format!("{name}={delta}ms"),
            None => format!("{name}=skew"),
        })
    }

    let legs: Vec<String> = [
        leg("received_to_applied", pulled_at_ms, applied_at_ms),
        leg("applied_to_receipt", applied_at_ms, receipt_at_ms),
        leg("receipt_to_observed", receipt_at_ms, Some(observed_at_ms)),
        // The end-to-end editor leg, so a profile missing its middle stamp
        // (self-echo ACK) still bounds the editor half.
        leg("received_to_observed", pulled_at_ms, Some(observed_at_ms)),
    ]
    .into_iter()
    .flatten()
    .collect();

    (!legs.is_empty()).then(|| legs.join(" "))
}

fn base64_standard_encode(bytes: &[u8]) -> String {
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
}

fn base64_standard_decode(value: &str) -> Result<Vec<u8>, base64::DecodeError> {
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, value)
}

fn canonical_controller_request_file(
    bootstrap: &ControllerBootstrap,
    requested_file: &Path,
) -> PathBuf {
    let candidate = if requested_file.is_absolute() {
        requested_file.to_path_buf()
    } else {
        bootstrap.project_root.join(requested_file)
    };
    candidate.canonicalize().unwrap_or(candidate)
}

fn crdt_replica_refused_data(reason: &str) -> serde_json::Value {
    serde_json::json!({
        "refused": true,
        "reason": reason,
    })
}

pub fn checkpoint_route_owned_documents_for_project(
    project_root: &Path,
    source: &str,
) -> Result<CrdtCheckpointSummary> {
    let store = load_actor_store(project_root)?;
    let mut summary = CrdtCheckpointSummary::default();
    for record in store.values() {
        match checkpoint_route_owned_document_crdt(Path::new(&record.document_id), source) {
            Ok(Some(status)) if status == "checkpointed" => summary.checkpointed += 1,
            Ok(Some(status)) if status == "detached" => summary.detached += 1,
            Ok(Some(_)) | Ok(None) => summary.skipped += 1,
            Err(err) => {
                summary.failed += 1;
                agent_doc_ops_log_io::log_op(
                    Path::new(&record.document_id),
                    &format!(
                        "controller_crdt_checkpoint_failed file={} source={} error={:?}",
                        record.document_id,
                        source,
                        err.to_string(),
                    ),
                );
                eprintln!(
                    "[agent-doc] warning: failed to checkpoint CRDT durable projection for {} before {source}: {err:#}",
                    record.document_id
                );
            }
        }
    }
    Ok(summary)
}

pub fn recycle_supervisors_all_projects_force(force: bool) -> Result<(usize, usize)> {
    let docs = crate::process::route_owned_supervisor_documents(std::process::id());
    let reason = if force {
        "install_fanout_force"
    } else {
        agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_INSTALL_FANOUT
    };
    let mut marked = 0;
    let mut skipped = 0;
    for doc in docs {
        match checkpoint_route_owned_document_crdt(&doc, "supervisor_recycle_request") {
            Ok(_) => {}
            Err(err) => {
                skipped += 1;
                eprintln!(
                    "[agent-doc] warning: not marking supervisor recycle-request for {} because CRDT durable checkpoint failed: {err:#}",
                    doc.display()
                );
                continue;
            }
        }
        match agent_doc_supervisor_io::recycle_request::request_recycle_for_doc(&doc, reason) {
            Ok(()) => marked += 1,
            Err(err) => {
                skipped += 1;
                eprintln!(
                    "[agent-doc] warning: failed to mark supervisor recycle-request for {}: {err:#}",
                    doc.display()
                );
            }
        }
    }
    Ok((marked, skipped))
}

/// Result of a typed reload-library fan-out to live editor registrations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReloadLibraryFanoutReport {
    pub projects: usize,
    pub endpoints: usize,
    pub delivered: usize,
    pub restart_required: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorNativeReloadPolicy {
    HotReload,
    RestartRequired,
}

fn editor_native_reload_policy(
    editor_id: &str,
    capabilities: &[String],
) -> EditorNativeReloadPolicy {
    if editor_id.starts_with("vscode-")
        || (editor_id.starts_with("jetbrains-")
            && capabilities
                .iter()
                .any(|value| value == "native_hot_reload_generation_v1"))
    {
        EditorNativeReloadPolicy::HotReload
    } else {
        EditorNativeReloadPolicy::RestartRequired
    }
}

/// Send one PID-scoped `reload_library` intent to every live editor process
/// whose adapter is explicitly known to support safe native hot reload.
///
/// Registrations come from the reliable-sync Lazily projection. A process that
/// has several open documents receives one intent per project, not one per
/// document. VS Code and JetBrains are explicitly hot-reload capable; unknown
/// adapters are counted as restart-required and fail closed.
/// Failures are counted so install can remain best-effort without inventing a
/// filesystem broadcast path.
pub fn reload_library_all_projects(lib_version: &str) -> ReloadLibraryFanoutReport {
    let mut report = ReloadLibraryFanoutReport::default();
    for project_root in crate::process::controller_project_roots(std::process::id()) {
        if !project_root.join(".agent-doc").is_dir() {
            continue;
        }
        report.projects += 1;
        let Ok(status) = reliable_sync_status(&project_root) else {
            report.failed += 1;
            continue;
        };
        let mut endpoints = status
            .registrations
            .into_iter()
            .map(|registration| {
                (
                    registration.pid,
                    registration.editor_id,
                    registration.capabilities,
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        // `#editorendpointzero`: the registration record can be empty while the
        // editor is alive and listening on its PID-scoped socket — the state that
        // made this fan-out report `0/0` and leave the document permanently
        // wedged. Fall back to socket+liveness discovery so a live editor is
        // always reachable, whatever the record says. `editor_id` is only a
        // payload field, so an unknown one does not block delivery.
        for pid in agent_doc_ipc_io::discover_listening_editor_pids(&project_root) {
            if !endpoints.iter().any(|(known, _, _)| *known == pid) {
                endpoints.insert((pid, String::new(), Vec::new()));
            }
        }
        report.endpoints += endpoints.len();
        for (pid, editor_id, capabilities) in endpoints {
            if !agent_doc_ipc_io::is_listener_active_for_pid(&project_root, pid) {
                report.failed += 1;
                continue;
            }
            if editor_native_reload_policy(&editor_id, &capabilities)
                == EditorNativeReloadPolicy::RestartRequired
            {
                report.restart_required += 1;
                continue;
            }
            match agent_doc_ipc_io::send_reload_library_to_editor(
                &project_root,
                pid,
                &editor_id,
                lib_version,
            ) {
                Ok(true) => report.delivered += 1,
                Ok(false) | Err(_) => report.failed += 1,
            }
        }
    }
    report
}

/// M4 (#stuckhandoff2) — client handoff drop-guard. The two-phase handoff is
/// driven by the invoking client: it launches a replacement controller in
/// `Preparing`, then promotes it to `Stable` via `promote_handoff`. If the client
/// is interrupted or an RPC fails between those two steps (a `?` early-return /
/// panic in `handoff_stale_controller`), the half-launched replacement is left
/// wedged in `Preparing` forever — the exact orphan M1's self-watchdog and the
/// M3/M5 reapers exist to clean up *after the fact*. This guard prevents the wedge
/// at the source: on drop without a completed promotion it tells that replacement
/// (still listening on the temp socket) to `shutdown` and rolls the old public
/// controller back through `abort_handoff`, so an aborted handoff never leaves
/// either side in `Preparing`. The success path calls [`HandoffDropGuard::complete`]
/// after the socket rename + reap so a promoted, now-authoritative controller is
/// never shut down.
#[cfg(test)]
pub(crate) struct HandoffDropGuard<'a> {
    project_root: &'a Path,
    temp_sock: &'a Path,
    completed: bool,
}

#[cfg(test)]
impl<'a> HandoffDropGuard<'a> {
    pub(crate) fn new(project_root: &'a Path, temp_sock: &'a Path) -> Self {
        Self {
            project_root,
            temp_sock,
            completed: false,
        }
    }

    pub(crate) fn complete(&mut self) {
        self.completed = true;
    }
}

#[cfg(test)]
impl Drop for HandoffDropGuard<'_> {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        // Aborted before promotion: best-effort shutdown of the half-launched
        // replacement and rollback of the old public controller's `Preparing`
        // marker. Both operations are idempotent; failures leave the watchdog and
        // process reapers as backstops.
        let _ = request_path_with_reason(self.temp_sock, "shutdown", "handoff_drop_guard");
        let rollback = request(self.project_root, "abort_handoff");
        agent_doc_ops_log_io::log_op(
            self.project_root,
            &format!(
                "handoff_drop_guard_aborted_handoff_shutdown temp_sock={} rollback={}",
                self.temp_sock.display(),
                rollback
                    .as_ref()
                    .map(|_| "ok".to_string())
                    .unwrap_or_else(|err| format!("failed:{}", compact_controller_error(err)))
            ),
        );
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn handoff_stale_controller(
    project_root: &Path,
    launch_mode: LaunchMode,
    old_status: ControllerStatus,
) -> Result<interprocess::local_socket::Stream> {
    let public_sock = socket_path(project_root);
    let old_pid = old_status.pid;
    let old_generation = old_status.controller_generation.unwrap_or(1);
    let new_generation = old_generation.saturating_add(1).max(1);
    let temp_sock = project_root.join(".agent-doc").join(format!(
        "controller-handoff-{}-{}.sock",
        std::process::id(),
        new_generation
    ));
    let _ = std::fs::remove_file(&temp_sock);
    let _ = request(project_root, "prepare_handoff");

    launch_detached_at(
        project_root,
        launch_mode,
        Some(&temp_sock),
        Some(new_generation),
        old_pid,
        ControllerHandoffState::Preparing,
    )?;
    // M4: from here until the promotion+rename succeed, any early return aborts the
    // handoff and must not leak the `Preparing` replacement.
    let mut drop_guard = HandoffDropGuard::new(project_root, &temp_sock);
    let _temp_stream = wait_for_controller_path_with_timeout(&temp_sock, HANDOFF_CONNECT_WAIT)?;
    let replacement_status: ControllerStatus = serde_json::from_str(
        &request_path(&temp_sock, "handoff_status")
            .context("failed to read replacement handoff status")?,
    )
    .context("failed to parse replacement controller status")?;
    let promoted_response = request_path(&temp_sock, "promote_handoff")?;
    if !promoted_response.contains("\"ok\":true") {
        anyhow::bail!("replacement controller refused promotion: {promoted_response}");
    }

    if public_sock.exists() {
        let _ = std::fs::remove_file(&public_sock);
    }
    std::fs::rename(&temp_sock, &public_sock).with_context(|| {
        format!(
            "failed to promote controller socket {} to {}",
            temp_sock.display(),
            public_sock.display()
        )
    })?;

    reap_stale_duplicate_controllers(project_root, replacement_status.pid, new_generation);
    // Promotion + rename succeeded — the replacement is the authoritative public
    // controller now, so the drop-guard must not shut it down.
    drop_guard.complete();

    wait_for_controller(project_root)
}

pub fn connect_or_launch(
    project_root: &Path,
    launch_mode: LaunchMode,
) -> Result<interprocess::local_socket::Stream> {
    connect_or_launch_with_claim_wait(project_root, launch_mode, LAUNCH_CLAIM_WAIT)
}

fn active_controller_status_is_adoptable(
    active_status: &ControllerStatus,
    current_binary: Option<&ControllerBinaryIdentity>,
) -> bool {
    active_status.active
        && active_status
            .handoff_state
            .unwrap_or(ControllerHandoffState::Stable)
            == ControllerHandoffState::Stable
        && !active_controller_status_needs_binary_replacement(active_status, current_binary)
}

fn active_controller_status_needs_binary_replacement(
    active_status: &ControllerStatus,
    current_binary: Option<&ControllerBinaryIdentity>,
) -> bool {
    status::controller_binary_identity_is_newer(
        current_binary,
        active_status.controller_binary.as_ref(),
    )
}

fn active_controller_status_non_stable_handoff_state(
    active_status: &ControllerStatus,
) -> Option<ControllerHandoffState> {
    active_status
        .active
        .then_some(active_status.handoff_state?)
        .filter(|state| *state != ControllerHandoffState::Stable)
}

fn abort_non_stable_active_controller_for_recovery(
    project_root: &Path,
    active_status: &ControllerStatus,
    phase: &str,
) {
    let Some(handoff_state) = active_controller_status_non_stable_handoff_state(active_status)
    else {
        return;
    };
    let rollback = request(project_root, "abort_handoff");
    agent_doc_ops_log_io::log_op(
        project_root,
        &format!(
            "controller_active_non_stable_abort_requested phase={phase} pid={} generation={} handoff_state={:?} rollback={}",
            active_status
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "none".to_string()),
            active_status.controller_generation.unwrap_or(0),
            handoff_state,
            rollback
                .as_ref()
                .map(|_| "ok".to_string())
                .unwrap_or_else(|err| format!("failed:{}", compact_controller_error(err)))
        ),
    );
}

fn log_stable_stale_controller_restart(project_root: &Path, active_status: &ControllerStatus) {
    agent_doc_ops_log_io::log_op(
        project_root,
        &format!(
            "controller_active_stale_binary_restart_requested pid={} generation={}",
            active_status
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "none".to_string()),
            active_status.controller_generation.unwrap_or(0),
        ),
    );
}

fn connect_or_launch_with_claim_wait(
    project_root: &Path,
    launch_mode: LaunchMode,
    launch_claim_wait: Duration,
) -> Result<interprocess::local_socket::Stream> {
    if let Ok(active_status) = status(project_root)
        && active_status.active
    {
        let current_binary = current_binary_identity().ok();
        if active_controller_status_is_adoptable(&active_status, current_binary.as_ref()) {
            reap_stale_duplicate_controllers(
                project_root,
                active_status.pid,
                active_status.controller_generation.unwrap_or(1),
            );
            return connect(project_root);
        } else if let Some(handoff_state) =
            active_controller_status_non_stable_handoff_state(&active_status)
        {
            agent_doc_ops_log_io::log_op(
                project_root,
                &format!(
                    "controller_active_non_stable_deferred phase=pre_lock pid={} generation={} handoff_state={:?}",
                    active_status
                        .pid
                        .map(|pid| pid.to_string())
                        .unwrap_or_else(|| "none".to_string()),
                    active_status.controller_generation.unwrap_or(0),
                    handoff_state,
                ),
            );
        }
    }

    // Block (bounded) on bootstrap-claim contention instead of failing fast: another
    // launcher (concurrent start, sibling document, or a just-execve'd self-recycle
    // racing its predecessor) is mid-launch for the shared project root, and the
    // double-checked `status` + `connect` below adopts whatever it publishes
    // (#suprecyclelock). Only a genuinely wedged holder returns an error — and even
    // then, adopt a live matching controller it may have published before wedging.
    let launch_claim = match LaunchClaim::acquire_blocking(project_root, launch_claim_wait) {
        Ok(claim) => claim,
        Err(err) => {
            if let Ok(active_status) = status(project_root)
                && active_status.active
            {
                let current_binary = current_binary_identity().ok();
                if active_controller_status_is_adoptable(&active_status, current_binary.as_ref()) {
                    log_launch_claim_waiter_adopted(project_root, &active_status, "timeout");
                    reap_stale_duplicate_controllers(
                        project_root,
                        active_status.pid,
                        active_status.controller_generation.unwrap_or(1),
                    );
                    return connect(project_root);
                }
            }
            return Err(err);
        }
    };
    let waited_on_launch_claim = launch_claim.waited();
    // Self-heal: kill any predecessor wedged in `Preparing`/`Promoted` past the
    // threshold *before* we adopt or promote, so a stuck controller cannot keep
    // re-corrupting the working tree and respawn the `1002 → 1004 → 1006`
    // handoff loop (#kqr6 / #sjwm / #stuckhandoff).
    let _ = terminate_stale_preparing_controllers_for_caller(
        project_root,
        stale_preparing_controller_threshold(),
        false,
        "connect_or_launch",
    );
    // M3 (#stuckhandoff2): also process-scan for *orphaned* preparing controllers the
    // record-scoped reaper above cannot see (their pid is no longer the bootstrap pid).
    let _ = reap_orphaned_preparing_controllers_for_caller(
        project_root,
        stale_preparing_controller_threshold(),
        false,
        "connect_or_launch",
    );
    if let Ok(active_status) = status(project_root)
        && active_status.active
    {
        let current_binary = current_binary_identity().ok();
        if active_controller_status_is_adoptable(&active_status, current_binary.as_ref()) {
            if waited_on_launch_claim {
                log_launch_claim_waiter_adopted(project_root, &active_status, "acquired");
            }
            reap_stale_duplicate_controllers(
                project_root,
                active_status.pid,
                active_status.controller_generation.unwrap_or(1),
            );
            return connect(project_root);
        } else if active_controller_status_non_stable_handoff_state(&active_status).is_some() {
            abort_non_stable_active_controller_for_recovery(
                project_root,
                &active_status,
                "post_lock",
            );
            if let Ok(recovered_status) = status(project_root)
                && active_controller_status_is_adoptable(&recovered_status, current_binary.as_ref())
            {
                reap_stale_duplicate_controllers(
                    project_root,
                    recovered_status.pid,
                    recovered_status.controller_generation.unwrap_or(1),
                );
                return connect(project_root);
            }
        }
    }
    if connect(project_root).is_ok() {
        if let Ok(old_status) = status(project_root)
            && old_status.active
        {
            let current_binary = current_binary_identity().ok();
            if active_controller_status_is_adoptable(&old_status, current_binary.as_ref()) {
                reap_stale_duplicate_controllers(
                    project_root,
                    old_status.pid,
                    old_status.controller_generation.unwrap_or(1),
                );
                return connect(project_root);
            }
            if old_status
                .handoff_state
                .unwrap_or(ControllerHandoffState::Stable)
                == ControllerHandoffState::Stable
            {
                log_stable_stale_controller_restart(project_root, &old_status);
            } else {
                abort_non_stable_active_controller_for_recovery(
                    project_root,
                    &old_status,
                    "fallback",
                );
                if let Ok(recovered_status) = status(project_root) {
                    if active_controller_status_is_adoptable(
                        &recovered_status,
                        current_binary.as_ref(),
                    ) {
                        reap_stale_duplicate_controllers(
                            project_root,
                            recovered_status.pid,
                            recovered_status.controller_generation.unwrap_or(1),
                        );
                        return connect(project_root);
                    }
                    if recovered_status
                        .handoff_state
                        .unwrap_or(ControllerHandoffState::Stable)
                        == ControllerHandoffState::Stable
                    {
                        log_stable_stale_controller_restart(project_root, &recovered_status);
                    } else {
                        anyhow::bail!(
                            "project controller is active but not authoritative (handoff_state={:?}); retry after handoff recovery",
                            old_status
                                .handoff_state
                                .unwrap_or(ControllerHandoffState::Preparing)
                        );
                    }
                }
            }
        }
        shutdown_stale_controller(project_root);
    }

    launch_detached(project_root, launch_mode)?;
    wait_for_controller_after_launch(project_root)
}

fn log_launch_claim_waiter_adopted(
    project_root: &Path,
    active_status: &ControllerStatus,
    phase: &str,
) {
    agent_doc_ops_log_io::log_op(
        project_root,
        &format!(
            "controller_launch_claim_waiter_adopted_published_controller phase={} pid={} generation={}",
            phase,
            active_status
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "none".to_string()),
            active_status.controller_generation.unwrap_or(1)
        ),
    );
}

pub fn ensure_controller_running(project_root: &Path, launch_mode: LaunchMode) -> Result<()> {
    let stream = connect_or_launch(project_root, launch_mode)?;
    drop(stream);
    Ok(())
}

fn active_public_controller_blocks_public_launch(project_root: &Path) -> Option<ControllerStatus> {
    let active_status = status(project_root).ok()?;
    let current_binary = current_binary_identity().ok();
    active_controller_status_is_adoptable(&active_status, current_binary.as_ref())
        .then_some(active_status)
}

pub(crate) fn launch_detached(project_root: &Path, launch_mode: LaunchMode) -> Result<()> {
    launch_detached_at(
        project_root,
        launch_mode,
        None,
        None,
        None,
        ControllerHandoffState::Stable,
    )
}

pub(crate) fn launch_detached_at(
    project_root: &Path,
    launch_mode: LaunchMode,
    listen_socket: Option<&Path>,
    controller_generation: Option<u64>,
    previous_controller_pid: Option<u32>,
    handoff_state: ControllerHandoffState,
) -> Result<()> {
    if EMBEDDED_NATIVE_HOST.load(Ordering::SeqCst) {
        return launch_detached_via_helper(
            project_root,
            launch_mode,
            listen_socket,
            controller_generation,
            previous_controller_pid,
            handoff_state,
        );
    }
    let build_command = |exe: &Path| -> Command {
        let mut command = Command::new(exe);
        command
            .current_dir(project_root)
            .arg("controller")
            .arg("serve")
            .arg("--project-root")
            .arg(project_root)
            .arg("--launch-mode")
            .arg(launch_mode.as_str());
        if let Some(path) = listen_socket {
            command.arg("--listen-socket").arg(path);
        }
        if let Some(generation) = controller_generation {
            command
                .arg("--controller-generation")
                .arg(generation.to_string());
        }
        if let Some(pid) = previous_controller_pid {
            command
                .arg("--previous-controller-pid")
                .arg(pid.to_string());
        }
        if handoff_state != ControllerHandoffState::Stable {
            command.arg("--handoff-state").arg(match handoff_state {
                ControllerHandoffState::Stable => "stable",
                ControllerHandoffState::Preparing => "preparing",
                ControllerHandoffState::Promoted => "promoted",
                ControllerHandoffState::Retiring => "retiring",
                ControllerHandoffState::Failed => "failed",
            });
        }
        close_inherited_fds_on_exec(&mut command);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    };
    // `#ctlrlaunchenoent`: a concurrent `cargo install` / `make install-full`
    // atomically replaces the installed `agent-doc` binary (unlink + rename).
    // `current_agent_doc_binary` already skips a deleted `current_exe` mapping and
    // re-resolves the on-disk binary, but resolve→`spawn` is a TOCTOU window — the
    // binary can be validated as launchable and then swapped out in the
    // microseconds before `spawn()`, so the exec fails with a transient `ENOENT`.
    // Retry across that brief replacement window (re-resolving each attempt so a
    // freshly-installed path is picked up) instead of surfacing `failed to launch
    // project controller: No such file or directory` and cascading into a 5s
    // controller-response timeout during a fleet recycle.
    // `#ctrlrespawnenoent`: the original window here was 5 x 100ms = 500ms, sized
    // for the microsecond unlink+rename of an atomic replace. A real `make
    // install` is not that: `cargo install` rebuilds and copies a ~70MB binary,
    // and the path can be absent for seconds. Operator-reported 2026-07-19 —
    // every agent-doc command on the project failed, all 6 attempts exhausted
    // inside 500ms, and recovery required starting `controller serve` by hand.
    // Span a realistic install instead, with exponential backoff so the common
    // microsecond case still resolves on the first retry.
    use launch_enoent::{
        LAUNCH_ENOENT_BACKOFF_INITIAL, LAUNCH_ENOENT_BACKOFF_MAX, LAUNCH_ENOENT_TOTAL_BUDGET,
    };
    // `#ctrlrespawnenoent2`: an empty or missing project root makes
    // `Command::current_dir` fail with ENOENT no matter how healthy the binary
    // is, and no amount of retrying can fix it. Without this guard the
    // ENOENT retry budget below (widened to ~13.5s for real install races)
    // burns that whole budget on a permanently-unsatisfiable condition and then
    // blames a concurrent install. Fail fast and name the real cause.
    if project_root.as_os_str().is_empty() || !project_root.is_dir() {
        anyhow::bail!(
            "cannot launch project controller: project root {:?} is empty or not a directory.              This is a caller passing a bad root, NOT a missing binary and NOT a concurrent              install — retrying cannot help (#ctrlrespawnenoent2)",
            project_root,
        );
    }
    let launch_started = std::time::Instant::now();
    let mut backoff = LAUNCH_ENOENT_BACKOFF_INITIAL;
    let mut attempt: u32 = 0;
    let child = loop {
        let exe = current_agent_doc_binary()?;
        match build_command(&exe).spawn() {
            Ok(child) => {
                if attempt > 0 {
                    agent_doc_ops_log_io::log_op(
                        project_root,
                        &format!(
                            "controller_launch_recovered_after_enoent project_root={} binary={} spawn_attempts={} waited_ms={} (#ctrlrespawnenoent)",
                            project_root.display(),
                            exe.display(),
                            attempt + 1,
                            launch_started.elapsed().as_millis(),
                        ),
                    );
                }
                break child;
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::NotFound
                    && launch_started.elapsed() + backoff < LAUNCH_ENOENT_TOTAL_BUDGET =>
            {
                attempt += 1;
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(LAUNCH_ENOENT_BACKOFF_MAX);
                continue;
            }
            Err(err) => {
                // Name the cause. This used to surface to the operator wrapped in
                // `editor_attached_model_missing`, which points diagnosis at the
                // editor when the editor is fine and the controller is simply
                // absent.
                let enoent = err.kind() == std::io::ErrorKind::NotFound;
                let hint = if enoent {
                    format!(
                        "; the binary path was still missing after {}ms — a concurrent `cargo install` / `make install` may have been replacing it. \
                         This is a MISSING CONTROLLER, not an editor problem. Recover with `agent-doc controller serve --project-root {} --launch-mode lazy`",
                        launch_started.elapsed().as_millis(),
                        project_root.display(),
                    )
                } else {
                    String::new()
                };
                return Err(anyhow::Error::new(err).context(format!(
                    "failed to launch project controller (binary={}, spawn_attempts={}){hint}",
                    exe.display(),
                    attempt + 1
                )));
            }
        }
    };
    // Reap the detached controller instead of dropping the handle: under a
    // long-lived launcher (the route-owned supervisor re-`ensure`ing every
    // reconcile tick) a replacement controller that immediately finds a live
    // peer owning the socket exits fast, and an unreaped handle becomes a
    // `<defunct>` zombie parented to the supervisor forever (`#zombiereap`).
    agent_doc_supervisor_process::detached_child::reap_detached(child);
    Ok(())
}

fn launch_detached_via_helper(
    project_root: &Path,
    launch_mode: LaunchMode,
    listen_socket: Option<&Path>,
    controller_generation: Option<u64>,
    previous_controller_pid: Option<u32>,
    handoff_state: ControllerHandoffState,
) -> Result<()> {
    let exe = current_agent_doc_binary()?;
    let mut command = Command::new(&exe);
    command
        .current_dir(project_root)
        .arg("controller")
        .arg("launch-detached")
        .arg("--project-root")
        .arg(project_root)
        .arg("--launch-mode")
        .arg(launch_mode.as_str());
    if let Some(path) = listen_socket {
        command.arg("--listen-socket").arg(path);
    }
    if let Some(generation) = controller_generation {
        command
            .arg("--controller-generation")
            .arg(generation.to_string());
    }
    if let Some(pid) = previous_controller_pid {
        command
            .arg("--previous-controller-pid")
            .arg(pid.to_string());
    }
    if handoff_state != ControllerHandoffState::Stable {
        command.arg("--handoff-state").arg(match handoff_state {
            ControllerHandoffState::Stable => "stable",
            ControllerHandoffState::Preparing => "preparing",
            ControllerHandoffState::Promoted => "promoted",
            ControllerHandoffState::Retiring => "retiring",
            ControllerHandoffState::Failed => "failed",
        });
    }
    close_inherited_fds_on_exec(&mut command);
    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| {
            format!(
                "failed to launch controller through detached helper {}",
                exe.display()
            )
        })?;
    if !status.success() {
        anyhow::bail!("detached controller helper exited with {status}");
    }
    Ok(())
}

#[cfg(unix)]
fn close_inherited_fds_on_exec(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    let close_limit = inherited_fd_close_limit();
    // The controller is often launched by an editor plugin or harness process.
    // Start a new session so terminal/harness process-group cleanup cannot kill
    // the daemon when the launcher crashes. Some editor file descriptors are not
    // CLOEXEC, so explicitly drop everything except stdio before exec as well.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            for fd in 3..close_limit {
                libc::close(fd);
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn close_inherited_fds_on_exec(_command: &mut Command) {}

#[cfg(unix)]
fn inherited_fd_close_limit() -> i32 {
    let limit = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
    if (4..=65_536).contains(&limit) {
        limit as i32
    } else {
        1024
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn wait_for_controller(
    project_root: &Path,
) -> Result<interprocess::local_socket::Stream> {
    wait_for_controller_path(&socket_path(project_root))
}

pub(crate) fn wait_for_controller_after_launch(
    project_root: &Path,
) -> Result<interprocess::local_socket::Stream> {
    wait_for_controller_path_with_timeout(&socket_path(project_root), LAUNCH_CONNECT_WAIT)
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn wait_for_controller_path(path: &Path) -> Result<interprocess::local_socket::Stream> {
    wait_for_controller_path_with_timeout(path, CONNECT_WAIT)
}

pub(crate) fn wait_for_controller_path_with_timeout(
    path: &Path,
    timeout: Duration,
) -> Result<interprocess::local_socket::Stream> {
    let start = Instant::now();
    loop {
        if let Ok(stream) = connect_path(path) {
            return Ok(stream);
        }
        if start.elapsed() >= timeout {
            anyhow::bail!(
                "timed out waiting for project controller at {}",
                path.display()
            );
        }
        std::thread::sleep(CONNECT_POLL);
    }
}

#[allow(dead_code)]
pub fn serve(project_root: &Path, launch_mode: LaunchMode) -> Result<()> {
    serve_with_options(
        project_root,
        launch_mode,
        None,
        None,
        None,
        ControllerHandoffState::Stable,
    )
}

/// Minimal live Lazily actor used by cross-crate tests.
///
/// This deliberately omits controller process-global services (relay-hub
/// ownership, exit watchers, supervisors, and recycle watchdogs). It exercises
/// the same actor memory, RPC handlers, and durable fact sink as production
/// without making unrelated tests share process-global controller state.
#[cfg(feature = "test-support")]
pub struct StateActorTestHandle {
    should_stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(feature = "test-support")]
fn state_actors_for_tests() -> &'static Mutex<BTreeMap<PathBuf, StateActorTestHandle>> {
    static ACTORS: OnceLock<Mutex<BTreeMap<PathBuf, StateActorTestHandle>>> = OnceLock::new();
    ACTORS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

#[cfg(feature = "test-support")]
fn ensure_state_actor_for_tests(project_root: &Path) -> Result<()> {
    let canonical = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let mut actors = state_actors_for_tests().lock();
    if actors.contains_key(&canonical) {
        return Ok(());
    }
    shutdown_stale_controller(&canonical);
    let actor = start_state_actor_for_tests(&canonical)?;
    actors.insert(canonical, actor);
    Ok(())
}

#[cfg(feature = "test-support")]
impl Drop for StateActorTestHandle {
    fn drop(&mut self) {
        self.should_stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            eprintln!("[agent-doc] test Lazily state actor thread panicked");
        }
    }
}

#[cfg(feature = "test-support")]
pub fn start_state_actor_for_tests(project_root: &Path) -> Result<StateActorTestHandle> {
    let sock = socket_path(project_root);
    if sock.exists() {
        std::fs::remove_file(&sock)
            .with_context(|| format!("remove stale test actor socket {}", sock.display()))?;
    }
    if let Some(parent) = sock.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bootstrap = write_bootstrap(project_root, LaunchMode::Lazy)?;
    let runtime = ControllerRuntime::new_arc(bootstrap)?;
    let name = sock.clone().to_fs_name::<GenericFilePath>()?;
    let listener = ListenerOptions::new()
        .name(name)
        .create_sync()
        .with_context(|| format!("failed to listen on {}", sock.display()))?;
    listener
        .set_nonblocking(ListenerNonblockingMode::Accept)
        .context("failed to set test state actor listener nonblocking")?;

    let should_stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&should_stop);
    let actor_root = project_root.to_path_buf();
    let thread = std::thread::spawn(move || {
        agent_doc_cycle_state_io::set_in_controller_request(true);
        while !thread_stop.load(Ordering::SeqCst) && actor_root.is_dir() {
            match listener.accept() {
                Ok(stream) => {
                    if let Err(err) = serve_client(stream, &runtime, &thread_stop, &sock) {
                        eprintln!("[agent-doc] test Lazily state actor client failed: {err:#}");
                    }
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    std::thread::sleep(CONNECT_POLL);
                }
                Err(_) => break,
            }
        }
        if let Err(err) = std::fs::remove_file(&sock)
            && err.kind() != ErrorKind::NotFound
        {
            eprintln!(
                "[agent-doc] failed to remove test Lazily state actor socket {}: {err}",
                sock.display()
            );
        }
    });
    Ok(StateActorTestHandle {
        should_stop,
        thread: Some(thread),
    })
}

#[derive(Debug)]
struct ProjectRootIncarnation {
    inode: Option<u64>,
    /// Retaining the original directory handle prevents Unix filesystems from
    /// recycling its inode while detached startup is still deciding whether
    /// the path names the same root.
    #[cfg(unix)]
    _directory: std::fs::File,
}

impl ProjectRootIncarnation {
    fn capture(project_root: &Path) -> Result<Self> {
        if !project_root.is_dir() {
            anyhow::bail!(
                "project root {} disappeared before controller startup",
                project_root.display()
            );
        }
        #[cfg(unix)]
        let directory = std::fs::File::open(project_root).with_context(|| {
            format!(
                "failed to retain project-root incarnation handle for {}",
                project_root.display()
            )
        })?;
        Ok(Self {
            inode: agent_doc_fs::inode_of_path(project_root),
            #[cfg(unix)]
            _directory: directory,
        })
    }

    fn still_matches(&self, project_root: &Path) -> bool {
        project_root.is_dir()
            && match self.inode {
                Some(inode) => agent_doc_fs::inode_of_path(project_root) == Some(inode),
                None => true,
            }
    }
}

pub(crate) fn serve_with_options(
    project_root: &Path,
    launch_mode: LaunchMode,
    listen_socket: Option<PathBuf>,
    controller_generation: Option<u64>,
    previous_controller_pid: Option<u32>,
    handoff_state: ControllerHandoffState,
) -> Result<()> {
    // `#ensurereplicagensup`: this process is about to become the relay-hub
    // owner. Everything that keys off `hub_registry()` needs to distinguish "the
    // replica has not registered yet" (worth waiting on, only meaningful here)
    // from "the hub lives in another process" (waiting can never help).
    // `#lazily-hot-path`: this process is now the project controller. Mark the
    // serve thread so client helpers it invokes during request handling
    // (load_document_projection, mark_*) use the local path instead of
    // self-RPCing the socket they are currently serving (which would deadlock
    // the single-request serve loop).
    agent_doc_cycle_state_io::set_in_controller_request(true);
    agent_doc_crdt_relay_io::mark_process_as_relay_hub_owner();
    // Detached startup races with short-lived callers (especially TempDir-backed
    // tests). Capture the caller's directory incarnation before any bootstrap
    // helper can create `.agent-doc`; a deleted root must never be resurrected by
    // the child and retained forever as an orphan controller.
    let root_incarnation = ProjectRootIncarnation::capture(project_root)?;
    let public_sock = socket_path(project_root);
    let mut sock = listen_socket.unwrap_or_else(|| public_sock.clone());
    if sock == public_sock
        && handoff_state == ControllerHandoffState::Stable
        && let Some(active_status) = active_public_controller_blocks_public_launch(project_root)
    {
        agent_doc_ops_log_io::log_op(
            project_root,
            &format!(
                "controller_public_launch_skipped_existing_authoritative pid={} generation={}",
                active_status
                    .pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                active_status.controller_generation.unwrap_or(1),
            ),
        );
        return Ok(());
    }
    // M1b (#stuckhandoff2 reopen): a controller launched on a non-public socket is
    // a handoff *replacement* (`controller-handoff-*` temp socket from
    // `handoff_stale_controller`). It becomes authoritative only when its client
    // renames that temp socket onto the public path; until then it is a candidate
    // for the structural stranded-replacement watchdog below. The initial
    // controller serves directly on the public socket, so this stays `None`.
    //
    // `#stuckhandoffselfheal`: `sock`/`handoff_temp_socket` are `mut` because a
    // promoted-but-stranded replacement now completes its OWN promotion in the serve
    // loop (renames the temp socket onto `public_sock`), after which it serves as the
    // authoritative public controller and is no longer a replacement.
    let mut handoff_temp_socket: Option<PathBuf> = (sock != public_sock).then(|| sock.clone());
    if sock.exists() {
        let _ = std::fs::remove_file(&sock);
    }
    if let Some(parent) = sock.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !root_incarnation.still_matches(project_root) {
        let _ = std::fs::remove_file(&sock);
        anyhow::bail!(
            "project root {} was replaced during controller startup",
            project_root.display()
        );
    }
    let bootstrap = if let Some(generation) = controller_generation {
        write_bootstrap_with_options(
            project_root,
            sock.clone(),
            launch_mode,
            generation,
            handoff_state,
            previous_controller_pid,
        )?
    } else {
        write_bootstrap(project_root, launch_mode)?
    };
    let durable_project_root = bootstrap.project_root.clone();
    let runtime = ControllerRuntime::new_arc(bootstrap)?;
    // P4: rebuild every controller-acknowledged liveness fact before the socket
    // becomes visible. Sender frames may already have been pruned after their ACK;
    // the receiver journal is therefore the recycle authority, not a lease scan.
    restore_reliable_sync_liveness(&durable_project_root)?;
    // #s4b: install the OS process-exit watcher on the process-global editor-attachment
    // registry so the editor-attached authority (`authority_for_file`) can read pure
    // reactive state on the hot path and learn about editor **crashes** (which send no
    // `deregister`) from a bounded-latency liveness poller instead of a per-decision
    // filesystem lease read. A short-lived CLI hydrates from the durable receiver
    // journal and retained sender suffix instead of scanning lease sidecars.
    crate::process_exit_watcher::install_process_exit_watcher(durable_project_root);
    let name = sock.clone().to_fs_name::<GenericFilePath>()?;
    let listener = ListenerOptions::new()
        .name(name)
        .create_sync()
        .with_context(|| format!("failed to listen on {}", sock.display()))?;
    if !root_incarnation.still_matches(project_root) {
        drop(listener);
        let _ = std::fs::remove_file(&sock);
        anyhow::bail!(
            "project root {} was replaced while publishing the controller socket",
            project_root.display()
        );
    }
    listener
        .set_nonblocking(ListenerNonblockingMode::Accept)
        .context("failed to set project controller listener nonblocking")?;

    let should_stop = Arc::new(AtomicBool::new(false));
    let active_clients = Arc::new(AtomicUsize::new(0));
    let watchdog_threshold = stale_preparing_controller_threshold();
    let controller_launched_at = Instant::now();
    let recycle_grace = recycle_idle_grace();
    let mut recycle_stale_since: Option<Instant> = None;
    // #supresilience Part B — autonomous route-owned supervisor watchdog. Runs on
    // the controller's idle serve tick, throttled to `SUPERVISOR_WATCHDOG_INTERVAL`.
    // `watchdog_halt_notified` dedups the operator halt diagnostic per document so a
    // budget-exhausted supervisor is reported once, not every tick.
    let supervisor_watchdog_interval = supervisor_watchdog_interval();
    let mut supervisor_watchdog_last_run: Option<Instant> = None;
    let mut supervisor_watchdog_halt_notified: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    while !should_stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok(stream) => {
                let runtime = Arc::clone(&runtime);
                let should_stop = Arc::clone(&should_stop);
                let active_clients = Arc::clone(&active_clients);
                let sock = sock.clone();
                active_clients.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || {
                    if let Err(err) = serve_client(stream, &runtime, &should_stop, &sock) {
                        eprintln!("[controller] client error: {err}");
                    }
                    active_clients.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                if !project_root.exists() {
                    eprintln!(
                        "[controller] project root {} no longer exists; shutting down detached controller",
                        project_root.display()
                    );
                    should_stop.store(true, Ordering::SeqCst);
                    break;
                }
                // M1 (#stuckhandoff2) — self-watchdog / suicide timer. A controller
                // wedged in `Preparing`/`Promoted` past the staleness threshold (the
                // client driving the two-phase handoff died between `prepare_handoff`
                // and `promote_handoff`) self-terminates here, with no dependency on
                // an external reaper tick, the bootstrap PID record, or a later invoke
                // in *any* project. The live in-memory bootstrap reflects the latest
                // `prepare_handoff`/`promote_handoff` transition, so a healthy handoff
                // (which completes well under the threshold) never trips this.
                // The in-memory predicate catches a replacement still wedged in
                // `Preparing`. M1b additionally catches the dominant orphan shape
                // (#stuckhandoff2 reopen): a replacement that received
                // `promote_handoff` (flipping in-memory state to `Stable`, invisible
                // to the predicate) but whose client died before renaming its temp
                // socket onto the public path — detected structurally below.
                //
                // `#stuckhandoffselfheal`: for that promoted-but-stranded (`Stable`)
                // shape, COMPLETE the promotion here instead of suiciding. The
                // replacement already holds the authoritative generation; renaming its
                // own temp socket onto `public_sock` restores the canonical
                // `controller.sock` so clients resolving via the recorded `socket_path`
                // (e.g. finalize/commit) stop timing out and never fall back to an
                // out-of-band disk write the IPC snapshot cannot adopt (the drift root
                // cause). Only the `Preparing` wedge below still suicides.
                if let Some(temp) = handoff_temp_socket.clone()
                    && controller_replacement_should_self_promote(
                        &runtime,
                        &temp,
                        controller_launched_at.elapsed(),
                    )
                {
                    match controller_self_promote_to_public(&runtime, &temp, &public_sock) {
                        Ok(()) => {
                            handoff_temp_socket = None;
                            sock = public_sock.clone();
                            std::thread::sleep(CONNECT_POLL);
                            continue;
                        }
                        Err(err) => {
                            agent_doc_ops_log_io::log_op(
                                project_root,
                                &format!(
                                    "controller_self_promote_failed temp={} err={err}",
                                    temp.display()
                                ),
                            );
                        }
                    }
                }
                if controller_self_watchdog_should_suicide(
                    &runtime,
                    handoff_temp_socket.as_deref(),
                    controller_launched_at.elapsed(),
                    watchdog_threshold,
                ) {
                    controller_self_watchdog_suicide(&runtime, watchdog_threshold);
                    should_stop.store(true, Ordering::SeqCst);
                    break;
                }
                // R1/R2 (#ctlrecycle): recycle onto a freshly-installed binary (R1) or
                // on an operator `recycle` request (R2) once no dispatch is in flight,
                // debounced so a brief lull between queue items never triggers it. The
                // idle DB probe only runs when a recycle is actually wanted (rare), so
                // the common hot path stays an atomic load plus one binary `stat`.
                let wants_recycle_and_idle = controller_wants_recycle(&runtime)
                    && active_clients.load(Ordering::SeqCst) == 0
                    && controller_recycle_idle(&runtime);
                let (do_recycle, next_since) =
                    agent_doc_controller::recycle::recycle_debounce_decision(
                        wants_recycle_and_idle,
                        recycle_stale_since,
                        Instant::now(),
                        recycle_grace,
                    );
                recycle_stale_since = next_since;
                if do_recycle {
                    let reason = if runtime.recycle_forced() {
                        "operator_force_request"
                    } else if runtime.recycle_requested() {
                        "operator_request"
                    } else {
                        "stale_binary"
                    };
                    controller_self_recycle(&runtime, reason);
                    should_stop.store(true, Ordering::SeqCst);
                    break;
                }
                // #supresilience Part B — periodic dead-supervisor watchdog. Skipped
                // on a recycling iteration above (the controller is going away, so a
                // spawned replacement would be orphaned in a dying process).
                let watchdog_now = Instant::now();
                let watchdog_due = supervisor_watchdog_last_run
                    .map(|last| watchdog_now.duration_since(last) >= supervisor_watchdog_interval)
                    .unwrap_or(true);
                if watchdog_due {
                    supervisor_watchdog_last_run = Some(watchdog_now);
                    controller_supervisor_watchdog_tick(
                        &runtime,
                        &mut supervisor_watchdog_halt_notified,
                    );
                    // `#orphandrain`: sweep the documents the supervisor watchdog
                    // cannot help — those with no supervisor to revive.
                    controller_orphan_drain_tick(&runtime);
                }
                std::thread::sleep(CONNECT_POLL);
            }
            Err(err) => return Err(err).context("failed to accept project controller client"),
        }
    }
    let _ = std::fs::remove_file(&sock);
    Ok(())
}

const ORPHAN_DRAIN_BACKOFF_SCOPE: &str = "queue_orphan_drain_backoff";
const ORPHAN_DRAIN_BACKOFF_HOLDER: &str = "controller_orphan_drain";

/// `#orphandrain` — advance `queue: go` documents that have no supervisor.
///
/// The controller only decides and enqueues here. One bounded controller-owned
/// thread runs routes after this event-loop tick has returned, and an in-memory
/// per-document guard prevents overlapping work inside the process. The durable
/// dispatch fence is an atomic claim in `state.db`, surviving controller
/// restarts and excluding simultaneous controller processes.
struct OrphanDrainRouteWorker {
    sender: std::sync::mpsc::SyncSender<ControllerEditorRouteInvocation>,
    in_flight: Arc<Mutex<BTreeSet<PathBuf>>>,
}

static ORPHAN_DRAIN_ROUTE_WORKER: OnceLock<Result<OrphanDrainRouteWorker, String>> =
    OnceLock::new();

fn start_orphan_drain_route_worker() -> Result<OrphanDrainRouteWorker> {
    // One bounded controller-owned worker serializes autonomous recovery across
    // documents. This keeps route work off the RPC accept loop without letting
    // one detached process per document contend for global controller/tmux state.
    let (sender, receiver) = std::sync::mpsc::sync_channel::<ControllerEditorRouteInvocation>(1);
    let in_flight = Arc::new(Mutex::new(BTreeSet::<PathBuf>::new()));
    let worker_in_flight = Arc::clone(&in_flight);
    std::thread::Builder::new()
        .name("controller-orphan-drain-route".to_string())
        .spawn(move || {
            while let Ok(invocation) = receiver.recv() {
                let file = invocation.file.clone();
                let route_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    runtime_effects().and_then(|effects| effects.run_editor_route(invocation))
                }));
                match route_result {
                    Ok(Ok(result)) if result.exit_code == 0 => {
                        agent_doc_ops_log_io::log_op(
                            &file,
                            "controller_orphan_drain_worker_settled status=success",
                        );
                    }
                    Ok(Ok(result)) => agent_doc_ops_log_io::log_op(
                        &file,
                        &format!(
                            "controller_orphan_drain_worker_failed exit_code={} output={}",
                            result.exit_code,
                            result.output.replace('\n', "\\n")
                        ),
                    ),
                    Ok(Err(err)) => agent_doc_ops_log_io::log_op(
                        &file,
                        &format!("controller_orphan_drain_worker_failed error={err:#}"),
                    ),
                    Err(_) => agent_doc_ops_log_io::log_op(
                        &file,
                        "controller_orphan_drain_worker_panicked recovery=worker_continues",
                    ),
                }
                worker_in_flight.lock().remove(&file);
            }
        })
        .context("spawn controller orphan-drain route worker")?;
    Ok(OrphanDrainRouteWorker { sender, in_flight })
}

fn orphan_drain_route_worker() -> Result<&'static OrphanDrainRouteWorker> {
    match ORPHAN_DRAIN_ROUTE_WORKER
        .get_or_init(|| start_orphan_drain_route_worker().map_err(|err| format!("{err:#}")))
    {
        Ok(worker) => Ok(worker),
        Err(err) => anyhow::bail!("{err}"),
    }
}

fn enqueue_orphan_drain_route(invocation: ControllerEditorRouteInvocation) -> Result<bool> {
    use std::sync::mpsc::TrySendError;

    let worker = orphan_drain_route_worker()?;
    let file = invocation.file.clone();
    if !worker.in_flight.lock().insert(file.clone()) {
        return Ok(false);
    }
    match worker.sender.try_send(invocation) {
        Ok(()) => Ok(true),
        Err(TrySendError::Full(_)) => {
            worker.in_flight.lock().remove(&file);
            anyhow::bail!("controller orphan-drain route worker queue is full")
        }
        Err(TrySendError::Disconnected(_)) => {
            worker.in_flight.lock().remove(&file);
            anyhow::bail!("controller orphan-drain route worker disconnected")
        }
    }
}

/// A drainable head memoized against the revision it was parsed from
/// (`#orphandrainrevisiongate`).
///
/// Newtyped rather than a bare `Option<String>` so the two states a hit can
/// carry — "there is a head" and "there is provably no head" — stay distinct
/// from the miss that `orphan_drain_memoized_head` returns as `None`. Collapsing
/// them would make "no head this tick" indistinguishable from "not cached", and
/// the drain would re-materialize the document on every quiescent tick, which is
/// the whole cost being removed.
#[derive(Clone)]
pub(crate) struct OrphanDrainHead(Option<String>);

impl OrphanDrainHead {
    fn into_head(self) -> Option<String> {
        self.0
    }
}

/// Per-document `(revision, drainable head)` for the orphan-drain sweep.
///
/// Bounded by the number of documents with a live actor; an entry is replaced
/// whenever the revision moves, so it never grows with time.
fn orphan_drain_head_memo() -> &'static Mutex<
    std::collections::HashMap<PathBuf, (agent_doc_crdt_relay_io::CurrentRevision, OrphanDrainHead)>,
> {
    static MEMO: OnceLock<
        Mutex<
            std::collections::HashMap<
                PathBuf,
                (agent_doc_crdt_relay_io::CurrentRevision, OrphanDrainHead),
            >,
        >,
    > = OnceLock::new();
    MEMO.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn orphan_drain_memoized_head(
    file: &Path,
    revision: &agent_doc_crdt_relay_io::CurrentRevision,
) -> Option<OrphanDrainHead> {
    orphan_drain_head_memo()
        .lock()
        .get(file)
        .filter(|(cached, _)| cached == revision)
        .map(|(_, head)| head.clone())
}

fn orphan_drain_memoize_head(
    file: &Path,
    revision: agent_doc_crdt_relay_io::CurrentRevision,
    head: Option<String>,
) {
    orphan_drain_head_memo()
        .lock()
        .insert(file.to_path_buf(), (revision, OrphanDrainHead(head)));
}

fn controller_orphan_drain_tick(runtime: &Arc<ControllerRuntime>) {
    use agent_doc_controller::orphan_drain::{
        DEFAULT_MIN_DISPATCH_INTERVAL_SECS, OrphanDrainDecision, OrphanDrainObservation,
        orphan_drain_decision,
    };

    let bootstrap = match runtime.bootstrap_snapshot() {
        Ok(bootstrap) => bootstrap,
        Err(err) => {
            eprintln!("[controller] orphan drain: bootstrap snapshot unavailable: {err}");
            return;
        }
    };
    let project_root = bootstrap.project_root.clone();
    let conn = match open_state_db(&project_root) {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!(
                "[controller] orphan drain: failed to open state db in {}: {err}",
                project_root.display()
            );
            return;
        }
    };
    let store = match load_actor_store_from_db(&conn) {
        Ok(store) => store,
        Err(err) => {
            eprintln!(
                "[controller] orphan drain: failed to load actor store in {}: {err}",
                project_root.display()
            );
            return;
        }
    };
    let registry = match agent_doc_session_registry_io::load_in(&project_root) {
        Ok(registry) => registry,
        Err(err) => {
            eprintln!(
                "[controller] orphan drain: failed to load session registry in {}: {err}",
                project_root.display()
            );
            return;
        }
    };

    for record in store.values() {
        if record.pane_id.is_empty()
            || record.state == agent_doc_sqlite::state_store::ActorState::Closed
        {
            continue;
        }
        let Some(file) = registry
            .get(&record.document_id)
            .map(|entry| PathBuf::from(&entry.file))
            .filter(|file| !file.as_os_str().is_empty())
        else {
            continue;
        };

        // `#orphandrainrevisiongate`: this sweep runs on the 10s supervisor
        // watchdog tick and, unlike the other read sites fixed alongside it, it
        // genuinely needs the text — it parses the queue head out of it. What it
        // does not need is to re-materialize an unchanged document. Attributed
        // by pid on 2026-07-26: after the idle-watch gates landed, every
        // remaining quiescent-idle read was this loop, arriving as an isolated
        // triple (one per open document) every ten seconds from the controller
        // process, never through the RPC handler.
        //
        // The revision is read before the text, so a document that changes
        // between the two stores the older revision and simply misses next
        // tick; storing the revision observed after the text could pair a fresh
        // revision with a stale head. Local call — the controller owns the hub
        // in-process, so the gate costs a state-vector comparison.
        // The memo caches the *head*, and the loop body then runs exactly as
        // before. It deliberately does not `continue` on a hit: this sweep takes
        // ACTION on a drainable head, and a head that failed to dispatch last
        // tick must be retried on the next one. Skipping the iteration would
        // turn a read optimization into a dropped retry.
        let gate_revision = match agent_doc_crdt_relay_io::current_revision_for_file(&file) {
            Ok(revision @ agent_doc_crdt_relay_io::CurrentRevision::Current { .. }) => {
                Some(revision)
            }
            _ => None,
        };
        let memoized_head = gate_revision
            .as_ref()
            .and_then(|revision| orphan_drain_memoized_head(&file, revision));
        // Operator-visible live text is authoritative. Disk is consulted only
        // when no editor owns the document; an attached-but-pending replica
        // fails closed until the relay can supply a consistent cut.
        let content = match &memoized_head {
            // Unchanged revision: the head cannot have moved, so skip the
            // materialize + SHA + log entirely and reuse the parsed answer.
            Some(_) => String::new(),
            None => match agent_doc_crdt_relay_io::current_text_for_file_nonblocking(&file) {
                Ok(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }) => text,
                Ok(agent_doc_crdt_relay_io::CurrentText::Detached) => {
                    let Ok(content) = std::fs::read_to_string(&file) else {
                        continue;
                    };
                    content
                }
                Ok(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica)
                | Ok(agent_doc_crdt_relay_io::CurrentText::EditorSyncPending) => continue,
                Err(err) => {
                    agent_doc_ops_log_io::log_op(
                        &file,
                        &format!("controller_orphan_drain_authority_failed error={err:#}"),
                    );
                    continue;
                }
            },
        };
        let drainable = match memoized_head {
            Some(head) => head.into_head(),
            None => {
                let head = agent_doc_queue::queue_continuation::live_drainable_continuation_head(
                    &content,
                    agent_doc_queue::queue_continuation::DrainScope::Supervisor,
                );
                if let Some(revision) = gate_revision {
                    orphan_drain_memoize_head(&file, revision, head.clone());
                }
                head
            }
        };
        let document_hash = match agent_doc_fs::document_state_hash(&file) {
            Ok(hash) => hash,
            Err(err) => {
                agent_doc_ops_log_io::log_op(
                    &file,
                    &format!("controller_orphan_drain_hash_failed error={err:#}"),
                );
                continue;
            }
        };
        let now = controller_model_pressure_now_secs();
        let loop_owns_drain = match state_store::load_coordination_lease_from_db(
            &conn,
            agent_doc_lease::DRAIN_OWNER_SCOPE,
            &document_hash,
        ) {
            Ok(Some(lease)) => agent_doc_lease::timestamp_is_fresh(
                lease.heartbeat_secs,
                now,
                agent_doc_lease::drain_owner_ttl(),
            ),
            Ok(None) => false,
            Err(err) => {
                agent_doc_ops_log_io::log_op(
                    &file,
                    &format!("controller_orphan_drain_owner_read_failed error={err:#}"),
                );
                continue;
            }
        };
        let last_dispatch = match state_store::load_coordination_lease_from_db(
            &conn,
            ORPHAN_DRAIN_BACKOFF_SCOPE,
            &document_hash,
        ) {
            Ok(last_dispatch) => last_dispatch,
            Err(err) => {
                agent_doc_ops_log_io::log_op(
                    &file,
                    &format!("controller_orphan_drain_backoff_read_failed error={err:#}"),
                );
                continue;
            }
        };
        let observation = OrphanDrainObservation {
            // The supervisor-scope resolver owns activation (`queue: go`,
            // queue-tag `go`, and legacy active state) as well as drainability.
            queue_active: drainable.is_some(),
            has_drainable_head: drainable.is_some(),
            supervisor_alive: agent_doc_supervisor_io::process::supervisor_pid_for_doc(&file)
                .is_some(),
            loop_owns_drain,
            pane_busy: record.state != agent_doc_sqlite::state_store::ActorState::Ready,
            secs_since_last_dispatch: last_dispatch
                .map(|lease| now.saturating_sub(lease.heartbeat_secs)),
        };

        if orphan_drain_decision(observation, DEFAULT_MIN_DISPATCH_INTERVAL_SECS)
            != OrphanDrainDecision::Dispatch
        {
            continue;
        }

        let dispatch_claim = state_store::CoordinationLeaseRecord {
            scope_kind: ORPHAN_DRAIN_BACKOFF_SCOPE.to_string(),
            scope_id: document_hash,
            holder: ORPHAN_DRAIN_BACKOFF_HOLDER.to_string(),
            holder_pid: Some(std::process::id()),
            heartbeat_secs: now,
        };
        let eligible_at_or_before = now.saturating_sub(DEFAULT_MIN_DISPATCH_INTERVAL_SECS);
        match state_store::claim_coordination_lease_if_expired_in_db(
            &conn,
            &dispatch_claim,
            eligible_at_or_before,
        ) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(err) => {
                agent_doc_ops_log_io::log_op(
                    &file,
                    &format!("controller_orphan_drain_backoff_claim_failed error={err:#}"),
                );
                continue;
            }
        }

        let relative_path = file
            .strip_prefix(&project_root)
            .unwrap_or(&file)
            .to_string_lossy()
            .to_string();
        agent_doc_ops_log_io::log_op(
            &file,
            &format!(
                "controller_orphan_drain_dispatch file={} pane={} head={} reason=no_supervisor_idle_watch",
                file.display(),
                record.pane_id,
                drainable.as_deref().unwrap_or("unknown")
            ),
        );
        let invocation = ControllerEditorRouteInvocation {
            file: file.clone(),
            relative_path,
            pane: Some(record.pane_id.clone()),
            layout_args: Vec::new(),
            dispatch_only: true,
            plain_trigger: true,
            wait_for_ready_secs: None,
            force_disk: false,
            prune_before_lookup: false,
        };
        match enqueue_orphan_drain_route(invocation) {
            Ok(true) => {}
            Ok(false) => agent_doc_ops_log_io::log_op(
                &file,
                "controller_orphan_drain_dispatch_skipped reason=route_already_in_flight",
            ),
            Err(err) => {
                // Keep the durable claim even on enqueue failure. Releasing it
                // here would recreate the original restart/dispatch storm.
                agent_doc_ops_log_io::log_op(
                    &file,
                    &format!("controller_orphan_drain_dispatch_failed error={err:#}"),
                );
            }
        }
    }
}

/// Env override (seconds) for the [`controller_supervisor_watchdog_tick`] cadence.
const SUPERVISOR_WATCHDOG_INTERVAL_SECS_ENV: &str = "AGENT_DOC_SUPERVISOR_WATCHDOG_INTERVAL_SECS";
/// Default idle-tick cadence for the dead-supervisor watchdog.
const DEFAULT_SUPERVISOR_WATCHDOG_INTERVAL_SECS: u64 = 10;

fn supervisor_watchdog_interval() -> Duration {
    let secs = std::env::var(SUPERVISOR_WATCHDOG_INTERVAL_SECS_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_SUPERVISOR_WATCHDOG_INTERVAL_SECS);
    Duration::from_secs(secs)
}

/// #supresilience Part B — autonomous route-owned supervisor watchdog.
///
/// The project-controller daemon detects a route-owned supervisor that died and
/// restarts it by REUSING the existing operator replacement path
/// ([`handle_supervisor_replacement`] → `spawn_supervisor_replacement_worker` →
/// `drive_supervisor_replacement_background` → `cold_start_supervisor_replacement`) —
/// the same path `session restart-supervisor` / `admin recycle --force` drive. No
/// parallel spawn logic.
///
/// A supervisor is restarted ONLY when ALL hold:
/// - its recorded `supervisor_pid` is dead ([`process_is_alive`] is false),
/// - the document's actor session is not `Closed`,
/// - a live tmux pane still exists for it,
/// - its supervisor session log is still open (a hard crash leaves no close event;
///   this also dedups against a restart already recorded and in flight — recording
///   the loss below closes the log until the cold-start reopens it), AND
/// - it is within the restart budget ([`crash_policy::watchdog_restart_decision`]).
///
/// The self-ancestor guard skips any pid equal to this controller process so the
/// watchdog never tears down its own process.
///
/// Backoff shares ONE ledger with the route auto-start fallback: each restart
/// records a session-loss event, and the budget is the count of those events within
/// [`crash_policy::WATCHDOG_RESTART_WINDOW`]. After the cap the watchdog emits an
/// operator-visible diagnostic (once per document) instead of spawn-storming.
fn controller_supervisor_watchdog_tick(
    runtime: &Arc<ControllerRuntime>,
    halt_notified: &mut std::collections::HashSet<String>,
) {
    let bootstrap = match runtime.bootstrap_snapshot() {
        Ok(bootstrap) => bootstrap,
        Err(err) => {
            eprintln!("[controller] supervisor watchdog: bootstrap snapshot unavailable: {err}");
            return;
        }
    };
    let project_root = bootstrap.project_root.clone();
    let conn = match open_state_db(&project_root) {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!(
                "[controller] supervisor watchdog: failed to open state db in {}: {err}",
                project_root.display()
            );
            return;
        }
    };
    let store = match load_actor_store_from_db(&conn) {
        Ok(store) => store,
        Err(err) => {
            eprintln!(
                "[controller] supervisor watchdog: failed to load actor store in {}: {err}",
                project_root.display()
            );
            return;
        }
    };
    let registry = match agent_doc_session_registry_io::load_in(&project_root) {
        Ok(registry) => registry,
        Err(err) => {
            eprintln!(
                "[controller] supervisor watchdog: failed to load session registry in {}: {err}",
                project_root.display()
            );
            return;
        }
    };
    let tmux = tmux_router::Tmux::default_server();
    let window_secs = agent_doc_supervisor::crash_policy::WATCHDOG_RESTART_WINDOW.as_secs();

    for record in store.values() {
        let document_id = record.document_id.clone();
        // Gate: document actor session must not be Closed/ended.
        if record.state == agent_doc_sqlite::state_store::ActorState::Closed
            || record.pane_id.is_empty()
        {
            halt_notified.remove(&document_id);
            continue;
        }
        // Resolve the served file path from the durable registry metadata.
        let Some(file) = registry
            .get(&document_id)
            .map(|entry| PathBuf::from(&entry.file))
            .filter(|file| !file.as_os_str().is_empty())
        else {
            continue;
        };
        // Recorded supervisor pid for the authoritative generation.
        let Some(supervisor_pid) =
            load_supervisor_lease_from_db(&conn, &document_id, record.generation)
                .ok()
                .flatten()
                .and_then(|lease| lease.supervisor_pid)
        else {
            continue;
        };
        // Self-ancestor guard: never act on this controller's own process.
        if supervisor_pid == std::process::id() {
            continue;
        }
        // Gate: supervisor pid must actually be dead. A live supervisor is healthy —
        // clear any stale halt-notified marker so a future crash storm re-notifies.
        if process_is_alive(supervisor_pid) {
            halt_notified.remove(&document_id);
            continue;
        }
        // Gate: a live tmux pane must still exist (the pane-loss path is owned by
        // route/sync `record_session_loss`, not the crash watchdog).
        if !tmux.pane_alive(&record.pane_id) {
            continue;
        }
        // Gate + dedup: only act on an OPEN supervisor session log. A hard crash
        // leaves the latest session open (no close event); once we record the loss
        // below the log closes until the cold-start reopens it, so an in-flight
        // restart is not re-triggered on the next tick.
        let session_open = match agent_doc_supervisor_io::startup_miss::session_log_status(
            &file,
            &record.session_id,
        ) {
            Ok(Some(status)) => status.latest_session_open(),
            Ok(None) => false,
            Err(err) => {
                eprintln!(
                    "[controller] supervisor watchdog: failed to read session log for {}: {err}",
                    file.display()
                );
                false
            }
        };
        if !session_open {
            continue;
        }
        // Restart budget from the shared session-loss ledger.
        let prior_restarts =
            match agent_doc_supervisor_io::startup_miss::count_recent_session_loss_events(
                &file,
                &record.session_id,
                window_secs,
            ) {
                Ok(count) => count,
                Err(err) => {
                    eprintln!(
                        "[controller] supervisor watchdog: failed to read restart budget for {}: {err}",
                        file.display()
                    );
                    continue;
                }
            };
        match agent_doc_supervisor::crash_policy::watchdog_restart_decision(
            prior_restarts,
            agent_doc_supervisor::crash_policy::WATCHDOG_RESTART_CAP,
        ) {
            agent_doc_supervisor::crash_policy::WatchdogRestartDecision::Restart { attempt } => {
                // Record the loss into the shared ledger (also closes the session log,
                // dedup'ing in-flight restarts) before dispatching the replacement.
                if let Err(err) = agent_doc_supervisor_io::startup_miss::record_session_loss(
                    &file,
                    &record.session_id,
                    &record.pane_id,
                    "watchdog_dead_supervisor",
                    Some(record.window_id.as_str()),
                ) {
                    eprintln!(
                        "[controller] supervisor watchdog: failed to record session loss for {}: {err}",
                        file.display()
                    );
                }
                halt_notified.remove(&document_id);
                agent_doc_ops_log_io::log_op(
                    &file,
                    &format!(
                        "controller_supervisor_watchdog_restart document={document_id} session={} pane={} generation={} dead_pid={supervisor_pid} attempt={attempt} cap={} window_secs={window_secs}",
                        record.session_id,
                        record.pane_id,
                        record.generation,
                        agent_doc_supervisor::crash_policy::WATCHDOG_RESTART_CAP,
                    ),
                );
                let diagnostic_payload = serde_json::json!({
                    "force": false,
                    "mode": "continue",
                    "caller": "controller_supervisor_watchdog",
                    "dead_pid": supervisor_pid,
                    "attempt": attempt,
                })
                .to_string();
                let request = ControllerRequest {
                    command: "supervisor_replacement".to_string(),
                    file: Some(file.clone()),
                    session_id: None,
                    pane_id: None,
                    window_id: None,
                    generation: None,
                    state: Some("continue".to_string()),
                    caller: Some("controller_supervisor_watchdog".to_string()),
                    reason: Some("watchdog_dead_supervisor".to_string()),
                    supervisor_pid: None,
                    supervisor_socket: None,
                    command_kind: None,
                    diagnostic_payload: Some(diagnostic_payload),
                };
                if let Err(err) =
                    handle_supervisor_replacement(&bootstrap, Some(runtime.as_ref()), request)
                {
                    eprintln!(
                        "[controller] supervisor watchdog: replacement dispatch failed for {}: {err:#}",
                        file.display()
                    );
                    agent_doc_ops_log_io::log_op(
                        &file,
                        &format!(
                            "controller_supervisor_watchdog_restart_failed document={document_id} session={} dead_pid={supervisor_pid} error={err}",
                            record.session_id
                        ),
                    );
                }
            }
            agent_doc_supervisor::crash_policy::WatchdogRestartDecision::Halt {
                restarts_in_window,
            } => {
                // Operator-visible diagnostic, once per document until it recovers.
                if halt_notified.insert(document_id.clone()) {
                    let message = format!(
                        "controller_supervisor_watchdog_halted document={document_id} session={} pane={} generation={} dead_pid={supervisor_pid} restarts_in_window={restarts_in_window} cap={} window_secs={window_secs} reason=restart_budget_exhausted",
                        record.session_id,
                        record.pane_id,
                        record.generation,
                        agent_doc_supervisor::crash_policy::WATCHDOG_RESTART_CAP,
                    );
                    agent_doc_ops_log_io::log_op(&file, &message);
                    eprintln!(
                        "[controller] supervisor watchdog: STOP restarting {} — {} restarts within {}s exhausted the budget (cap={}); inspect the crash and run `agent-doc start {}` manually to recover",
                        file.display(),
                        restarts_in_window,
                        window_secs,
                        agent_doc_supervisor::crash_policy::WATCHDOG_RESTART_CAP,
                        file.display(),
                    );
                }
            }
        }
    }
}

/// M1/M1b (#stuckhandoff2) — runtime adapter for the pure controller watchdog policy.
/// Reads the controller's own live bootstrap snapshot and supplies wall-clock/socket
/// facts. A failed bootstrap snapshot is treated as "do not suicide" (the next
/// external reaper pass still covers it).
pub(crate) fn controller_self_watchdog_should_suicide(
    runtime: &ControllerRuntime,
    handoff_temp_socket: Option<&Path>,
    launched_elapsed: Duration,
    threshold: Duration,
) -> bool {
    let Ok(bootstrap) = runtime.bootstrap_snapshot() else {
        return false;
    };
    status::controller_watchdog_should_suicide(status::ControllerWatchdogFacts {
        handoff_state: bootstrap.handoff_state,
        handoff_started_at: bootstrap.handoff_started_at,
        now: timestamp_secs(),
        stale_after: threshold,
        is_handoff_replacement: handoff_temp_socket.is_some(),
        handoff_replacement_socket_exists: handoff_temp_socket.is_some_and(|temp| temp.exists()),
        launched_elapsed,
    })
}

/// `#stuckhandoffselfheal` — grace before a promoted-but-stranded replacement
/// completes its own promotion. Much shorter than the stale-preparing suicide
/// threshold: once a replacement is `Stable`, a *healthy* client renames within
/// milliseconds, so a few seconds unambiguously means the client died — while still
/// bounding the canonical `controller.sock` outage that makes clients time out.
const HANDOFF_SELF_PROMOTE_GRACE: Duration = Duration::from_secs(3);

/// `#stuckhandoffselfheal` — runtime adapter for the pure self-promote policy. Reads
/// the controller's live bootstrap snapshot and supplies wall-clock/socket facts. A
/// failed snapshot reads as "no" (the stranded/suicide watchdog still covers the wedge).
pub(crate) fn controller_replacement_should_self_promote(
    runtime: &ControllerRuntime,
    handoff_temp_socket: &Path,
    launched_elapsed: Duration,
) -> bool {
    let Ok(bootstrap) = runtime.bootstrap_snapshot() else {
        return false;
    };
    status::handoff_replacement_should_self_promote(
        true,
        handoff_temp_socket.exists(),
        bootstrap.handoff_state,
        launched_elapsed,
        HANDOFF_SELF_PROMOTE_GRACE,
    )
}

/// `#stuckhandoffselfheal` — perform the promotion the dead handoff client never
/// finished: atomically rename this replacement's temp handoff socket onto the
/// canonical public path, so the live listener (this process) becomes reachable at
/// `controller.sock` again. Mirrors the client-side promotion in
/// `handoff_stale_controller`. The bootstrap already records `socket_path = public`
/// (set at `promote_handoff`); re-persist it defensively so state and filesystem
/// agree, and log the self-promotion for closeout forensics.
pub(crate) fn controller_self_promote_to_public(
    runtime: &ControllerRuntime,
    handoff_temp_socket: &Path,
    public_sock: &Path,
) -> Result<()> {
    if public_sock.exists() {
        let _ = std::fs::remove_file(public_sock);
    }
    std::fs::rename(handoff_temp_socket, public_sock).with_context(|| {
        format!(
            "self-promote: failed to rename handoff socket {} onto public {}",
            handoff_temp_socket.display(),
            public_sock.display()
        )
    })?;
    let mut state = runtime.bootstrap.lock();
    state.socket_path = public_sock.to_path_buf();
    state.handoff_state = ControllerHandoffState::Stable;
    state.handoff_started_at = None;
    write_bootstrap_state(&state)?;
    agent_doc_ops_log_io::log_op(
        &state.project_root,
        &format!(
            "controller_self_promoted pid={} generation={} public_sock={} reason=client_died_before_rename",
            state.pid,
            state.controller_generation,
            public_sock.display()
        ),
    );
    Ok(())
}

/// M1 (#stuckhandoff2) — supersede the wedged in-memory + on-disk bootstrap with
/// `Failed` (so the next bind promotes a clean controller instead of re-adopting the
/// stuck generation) and log the self-reap. The caller flips `should_stop`, drops the
/// listener, and removes the socket; this process exits without ever needing `pkill`.
pub(crate) fn controller_self_watchdog_suicide(runtime: &ControllerRuntime, threshold: Duration) {
    let Ok(bootstrap) = runtime.bootstrap_snapshot() else {
        return;
    };
    let project_root = bootstrap.project_root.clone();
    let pid = bootstrap.pid;
    let generation = bootstrap.controller_generation;
    let age = timestamp_secs()
        .saturating_sub(bootstrap.handoff_started_at.unwrap_or_else(timestamp_secs));
    {
        let mut state = runtime.bootstrap.lock();
        state.handoff_state = ControllerHandoffState::Failed;
        state.handoff_started_at = None;
        // The per-project bootstrap state file is shared across controller
        // generations. Only supersede it with `Failed` while it still names THIS
        // controller (pid + generation). A stranded replacement (the post-
        // `promote_handoff` orphan from M1b) can be a stale generation a newer
        // clean controller already recorded on disk — clobbering that newer record
        // to `Failed` would churn the next bind into relaunching over a healthy
        // controller. When we no longer own the record, just exit (the process
        // dying is what stops the buffer race; the on-disk record stays correct).
        let owns_record = matches!(
            read_bootstrap(&project_root),
            Ok(Some(on_disk)) if on_disk.pid == pid && on_disk.controller_generation == generation
        );
        if owns_record && let Err(err) = write_bootstrap_state(&state) {
            eprintln!(
                "[controller] warning: self-watchdog failed to mark bootstrap failed pid={pid} generation={generation}: {err}"
            );
        }
    }
    agent_doc_ops_log_io::log_op(
        &project_root,
        &format!(
            "stale_preparing_controller_self_reaped pid={pid} generation={generation} age_secs={age} threshold_secs={} caller=self_watchdog",
            threshold.as_secs()
        ),
    );
    eprintln!(
        "[controller] self-watchdog: terminating controller wedged in preparing past {}s pid={pid} generation={generation} age_secs={age}",
        threshold.as_secs()
    );
}

/// `#ctlrecycle` R1/R2 — does this controller want to recycle onto a fresh binary?
/// True when an operator `recycle` RPC asked it to (R2) or its own binary is stale
/// after a `cargo install` (R1). Cheap: an atomic load, then one `stat` of the
/// install path only when no explicit request is pending.
pub(crate) fn controller_wants_recycle(runtime: &ControllerRuntime) -> bool {
    if runtime.recycle_requested() {
        return true;
    }
    match runtime.bootstrap_snapshot() {
        Ok(bootstrap) => {
            let current_binary = current_binary_identity().ok();
            status::process_binary_is_stale(
                bootstrap.controller_binary.as_ref(),
                current_binary.as_ref(),
            )
        }
        Err(_) => false,
    }
}

/// `#ctlrecycle` R1 — is the controller safe to recycle right now? Only when it is
/// `Stable` (not mid-handoff) AND no dispatch is in flight for ANY document, so a
/// recycle never interrupts an in-flight turn. Fail-closed on the idle proof: a
/// bootstrap-lock or DB error reads as "not idle" so we never recycle on uncertainty.
pub(crate) fn controller_recycle_idle(runtime: &ControllerRuntime) -> bool {
    let Ok(bootstrap) = runtime.bootstrap_snapshot() else {
        return false;
    };
    if bootstrap.handoff_state != ControllerHandoffState::Stable {
        return false;
    }
    // `#recycleforce`: a forced recycle (`agent-doc admin recycle --force`) is an
    // explicit operator override of the in-flight-dispatch deferral. Skip the
    // open-dispatch idle probe so the recycle takes effect at the next serve-loop
    // tick even mid-turn. We still require `Stable` above so a forced recycle never
    // lands mid-handoff (which would strand the replacement controller).
    if agent_doc_controller::recycle::force_overrides_in_flight_gate(
        runtime.recycle_forced(),
        bootstrap.handoff_state == ControllerHandoffState::Stable,
    ) {
        return true;
    }
    let Ok(conn) = open_state_db(&bootstrap.project_root) else {
        return false;
    };
    !state_store::has_any_open_in_flight_dispatch(&conn).unwrap_or(true)
}

/// `#ctlrecycle` R1/R2 — record the recycle and let the serve loop exit so the next
/// `connect_or_launch` relaunches the freshly-installed binary. State lives in
/// SQLite, so a coordinator exit loses nothing; the new controller adopts it. The
/// caller flips `should_stop`, drops the listener, and removes the socket.
pub(crate) fn controller_self_recycle(runtime: &ControllerRuntime, reason: &str) {
    let Ok(bootstrap) = runtime.bootstrap_snapshot() else {
        return;
    };
    let old_version = bootstrap
        .controller_binary
        .as_ref()
        .map(|id| id.version.clone())
        .unwrap_or_default();
    let new_version = current_binary_identity()
        .map(|id| id.version)
        .unwrap_or_default();
    agent_doc_ops_log_io::log_op(
        &bootstrap.project_root,
        &format!(
            "controller_self_recycled pid={} generation={} reason={reason} old_version={old_version} new_version={new_version}",
            bootstrap.pid, bootstrap.controller_generation
        ),
    );
    eprintln!(
        "[controller] recycling onto freshly-installed binary pid={} generation={} reason={reason}",
        bootstrap.pid, bootstrap.controller_generation
    );
}

fn shutdown_reason_allows_fresh_controller(reason: Option<&str>) -> bool {
    matches!(
        reason,
        Some("operator_shutdown" | "controller_restart" | "handoff_drop_guard" | "test_shutdown")
    )
}

fn controller_binary_is_stale_for_shutdown(bootstrap: &ControllerBootstrap) -> bool {
    let current_binary = current_binary_identity().ok();
    status::process_binary_is_stale(
        bootstrap.controller_binary.as_ref(),
        current_binary.as_ref(),
    )
}

pub(crate) fn serve_client(
    stream: interprocess::local_socket::Stream,
    runtime: &Arc<ControllerRuntime>,
    should_stop: &AtomicBool,
    sock: &Path,
) -> Result<()> {
    stream
        .set_recv_timeout(Some(CONTROLLER_IDLE_CLIENT_TIMEOUT))
        .context("failed to set project controller client read timeout")?;
    let (reader_half, mut writer_half) = stream.split();
    let mut reader = BufReader::new(reader_half);
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => Ok(()),
        Ok(_) => {
            let (response, request_should_stop) = handle_request_for_client(&line, runtime)?;
            writer_half.write_all(response.as_bytes())?;
            writer_half.write_all(b"\n")?;
            writer_half.flush()?;
            if request_should_stop {
                should_stop.store(true, Ordering::SeqCst);
                let _ = std::fs::remove_file(sock);
                let bootstrap = runtime.bootstrap.lock();
                if bootstrap.socket_path != sock {
                    let _ = std::fs::remove_file(&bootstrap.socket_path);
                }
            }
            Ok(())
        }
        Err(err) if is_timeout_error(&err) => {
            eprintln!(
                "[controller] closing idle client after {:.1}s without a complete request",
                CONTROLLER_IDLE_CLIENT_TIMEOUT.as_secs_f32()
            );
            Ok(())
        }
        Err(err) => Err(err).context("failed to read project controller request"),
    }
}

pub(crate) fn handle_request_for_client(
    line: &str,
    runtime: &Arc<ControllerRuntime>,
) -> Result<(String, bool)> {
    let mut request_should_stop = false;
    let response = match handle_request_locked(line, runtime, &mut request_should_stop) {
        Ok(response) => response,
        Err(err) => {
            eprintln!("[controller] request error: {err:#}");
            controller_envelope::<serde_json::Value>(Err(err))?
        }
    };
    Ok((response, request_should_stop))
}

#[cfg(any(test, feature = "test-support"))]
#[cfg_attr(feature = "test-support", allow(dead_code))]
pub(crate) fn handle_request(
    line: &str,
    bootstrap: &ControllerBootstrap,
    should_stop: &mut bool,
) -> Result<String> {
    handle_request_locked(
        line,
        &ControllerRuntime::new_arc(bootstrap.clone())?,
        should_stop,
    )
}

pub(crate) fn handle_request_locked(
    line: &str,
    runtime: &Arc<ControllerRuntime>,
    should_stop: &mut bool,
) -> Result<String> {
    // `#adturnscopehotloop`: memoize turn attribution for the life of this
    // request. `log_op` resolves a `turn=` id per line, and outside a scope that
    // resolution is `load_document_projection` — a SQLite ledger replay here in
    // the controller (`in_controller_request()` suppresses the IPC branch), or a
    // 5s-timeout controller round trip in every other process. `ops.log` carried
    // 12,700 lines per 20k-op window on this project, so the diagnostic label on
    // a log line was costing more than the operation it described. The memo the
    // `#adturnscope` doc comment describes already existed; `route` was its only
    // caller, and the two processes that emit almost all the lines — this one and
    // the supervisor idle watch — never opened one.
    //
    // A request cannot span two turns, so the scope is exact rather than a TTL
    // guess. Requests get their own thread and the scope is thread-local, so
    // concurrent documents keep separate memos.
    let _turn_attribution = agent_doc_ops_log_io::begin_turn_attribution_scope();
    let request_value = serde_json::from_str::<serde_json::Value>(line.trim())?;
    let request: ControllerRequest = serde_json::from_value(request_value.clone())?;
    // #af88 B enforcement: read the caller's stamped binary version (skew-safe;
    // absent from older clients). A value that differs from this controller's own
    // running version proves this controller is serving a different (stale) binary
    // than the freshly-invoked caller — used to refuse write-bearing commands.
    let client_binary_version = request_value
        .get("binary_version")
        .and_then(|b| b.as_str())
        .map(str::to_string);
    let client_binary_identity = request_value
        .get("binary_identity")
        .cloned()
        .and_then(|identity| serde_json::from_value::<ControllerBinaryIdentity>(identity).ok());
    let bootstrap_snapshot = runtime.bootstrap_snapshot()?;
    match request.command.as_str() {
        "coordination_claim" => {
            let scopes: Vec<String> = serde_json::from_str(
                request
                    .state
                    .as_deref()
                    .context("coordination_claim requires scopes")?,
            )
            .context("coordination_claim scopes must be a JSON string array")?;
            let owner_token = request
                .caller
                .as_deref()
                .context("coordination_claim requires owner token")?;
            let owner_pid = request
                .supervisor_pid
                .context("coordination_claim requires owner pid")?;
            let acquired = runtime.try_claim_coordination(&scopes, owner_token, owner_pid);
            Ok(serde_json::to_string(&CoordinationClaimResponse {
                acquired,
            })?)
        }
        "coordination_release" => {
            let scopes: Vec<String> = serde_json::from_str(
                request
                    .state
                    .as_deref()
                    .context("coordination_release requires scopes")?,
            )
            .context("coordination_release scopes must be a JSON string array")?;
            let owner_token = request
                .caller
                .as_deref()
                .context("coordination_release requires owner token")?;
            runtime.release_coordination(&scopes, owner_token);
            Ok(serde_json::to_string(
                &serde_json::json!({ "released": true }),
            )?)
        }
        "status" => Ok(serde_json::to_string(
            &status::controller_status_from_bootstrap(
                &controller_bootstrap_status_facts(&bootstrap_snapshot),
                true,
                discover_stale_duplicate_pids(
                    &bootstrap_snapshot.project_root,
                    Some(bootstrap_snapshot.pid),
                ),
                status::controller_freshness_status(controller_freshness_facts(
                    Some(bootstrap_snapshot.pid),
                    None,
                )),
                status::control_plane_status(
                    true,
                    control_plane_store_counts(&bootstrap_snapshot.project_root)?,
                    Some(runtime.memory_categories()?),
                ),
            ),
        )?),
        "handoff_status" => Ok(serde_json::to_string(
            &status::controller_status_from_bootstrap(
                &controller_bootstrap_status_facts(&bootstrap_snapshot),
                true,
                Vec::new(),
                status::controller_freshness_status(controller_freshness_facts(
                    Some(bootstrap_snapshot.pid),
                    None,
                )),
                status::default_control_plane_status(),
            ),
        )?),
        "prepare_handoff" => {
            let mut state = runtime.bootstrap.lock();
            let already_in_flight = matches!(
                state.handoff_state,
                ControllerHandoffState::Preparing | ControllerHandoffState::Promoted
            ) && state.handoff_started_at.is_some();
            state.handoff_state = ControllerHandoffState::Preparing;
            if !already_in_flight {
                state.handoff_started_at = Some(timestamp_secs());
            }
            write_bootstrap_state(&state)?;
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "abort_handoff" => {
            let mut state = runtime.bootstrap.lock();
            if matches!(
                state.handoff_state,
                ControllerHandoffState::Preparing | ControllerHandoffState::Promoted
            ) {
                state.handoff_state = ControllerHandoffState::Stable;
                state.handoff_started_at = None;
                write_bootstrap_state(&state)?;
                agent_doc_ops_log_io::log_op(
                    &state.project_root,
                    &format!(
                        "controller_handoff_aborted pid={} generation={}",
                        state.pid, state.controller_generation
                    ),
                );
            }
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "promote_handoff" => {
            let mut state = runtime.bootstrap.lock();
            state.socket_path = socket_path(&state.project_root);
            state.handoff_state = ControllerHandoffState::Stable;
            state.handoff_started_at = None;
            write_bootstrap_state(&state)?;
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "retire_after_handoff" => {
            {
                let mut state = runtime.bootstrap.lock();
                state.handoff_state = ControllerHandoffState::Retiring;
                write_bootstrap_state(&state)?;
            }
            *should_stop = true;
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "shutdown" => {
            let reason = request.reason.as_deref();
            let controller_is_stale = controller_binary_is_stale_for_shutdown(&bootstrap_snapshot);
            let caller_proves_newer_binary = reason == Some("stale_controller_replacement")
                && status::controller_binary_identity_is_newer(
                    client_binary_identity.as_ref(),
                    bootstrap_snapshot.controller_binary.as_ref(),
                );
            if !controller_is_stale
                && !caller_proves_newer_binary
                && !shutdown_reason_allows_fresh_controller(reason)
            {
                agent_doc_ops_log_io::log_op(
                    &bootstrap_snapshot.project_root,
                    &format!(
                        "controller_shutdown_refused pid={} generation={} request_reason={} reason=fresh_controller_requires_explicit_shutdown_reason",
                        bootstrap_snapshot.pid,
                        bootstrap_snapshot.controller_generation,
                        reason.unwrap_or("none"),
                    ),
                );
                anyhow::bail!(
                    "shutdown refused: controller is fresh and request omitted an accepted reason"
                );
            }
            *should_stop = true;
            agent_doc_ops_log_io::log_op(
                &bootstrap_snapshot.project_root,
                &format!(
                    "controller_shutdown_accepted pid={} generation={} request_reason={} controller_binary_stale={} caller_proves_newer_binary={}",
                    bootstrap_snapshot.pid,
                    bootstrap_snapshot.controller_generation,
                    reason.unwrap_or("none"),
                    controller_is_stale,
                    caller_proves_newer_binary,
                ),
            );
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "recycle" => {
            // R2 (#ctlrecycle): mark this controller to recycle at the next idle
            // boundary. Unlike `shutdown`, it does NOT stop immediately — the
            // serve-loop idle poll honors it only once no dispatch is in flight, so
            // an explicit recycle never interrupts an in-flight turn.
            runtime.request_recycle();
            agent_doc_ops_log_io::log_op(
                &bootstrap_snapshot.project_root,
                &format!(
                    "controller_recycle_requested pid={} generation={} reason={}",
                    bootstrap_snapshot.pid,
                    bootstrap_snapshot.controller_generation,
                    request.reason.as_deref().unwrap_or("operator_request"),
                ),
            );
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "recycle_force" => {
            // `#recycleforce`: mark this controller to recycle promptly, overriding
            // the in-flight-dispatch idle gate. Still acts at the serve-loop tick
            // (never mid-RPC), but `controller_recycle_idle` returns true while
            // forced, so the recycle is NOT deferred behind an open dispatch — an
            // explicit operator override that MAY interrupt an in-flight turn.
            runtime.request_recycle_force();
            agent_doc_ops_log_io::log_op(
                &bootstrap_snapshot.project_root,
                &format!(
                    "controller_recycle_requested pid={} generation={} reason={} forced=true",
                    bootstrap_snapshot.pid,
                    bootstrap_snapshot.controller_generation,
                    request
                        .reason
                        .as_deref()
                        .unwrap_or("operator_force_request"),
                ),
            );
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "supervisor_recycle_requested" => controller_envelope(handle_supervisor_recycle_requested(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "supervisor_recycle_started" => controller_envelope(handle_supervisor_recycle_started(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "supervisor_recycle_settled" => controller_envelope(handle_supervisor_recycle_settled(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "supervisor_recycle_status" => controller_envelope(runtime.supervisor_recycle_projection()),
        "supervisor_recycle_wait_settled" => controller_envelope(
            runtime.wait_for_supervisor_recycle_settle(SUPERVISOR_RECYCLE_SETTLE_WAIT),
        ),
        "state_event_append" => controller_envelope(handle_state_event_append(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "document_state_projection" => {
            // Read-only live-projection query (`#lazily-hot-path`): a client
            // inspects the authoritative in-memory projection instead of
            // replaying cold `state.db`. No fact is emitted.
            controller_envelope(handle_document_state_projection(runtime.as_ref(), request))
        }
        "record_owner_pane_wedge" => controller_envelope(handle_record_owner_pane_wedge(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "clear_owner_pane_wedge" => controller_envelope(handle_clear_owner_pane_wedge(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "closeout_owner_claim" => controller_envelope(handle_closeout_owner_claim(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "closeout_owner_release" => controller_envelope(handle_closeout_owner_release(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "command_plane_submit" => {
            // #af88 B enforcement: a command-plane submit persists facts (the
            // durable sink), so refuse it when the caller proves this controller
            // is serving a stale binary. Reuse `controller_binary_stale` so the
            // client's existing one-retry reconnect loop promotes the fresh
            // binary — same rule as `dispatch` / Compact Exchange.
            if let Some(client_version) =
                stale_mutating_client_binary(client_binary_version.as_deref())
            {
                agent_doc_ops_log_io::log_op(
                    &bootstrap_snapshot.project_root,
                    &format!(
                        "command_plane_submit_refused_client_binary_mismatch controller_version={} client_version={}",
                        identity_version(),
                        client_version
                    ),
                );
                anyhow::bail!(
                    "command_plane_submit refused: controller_binary_stale (running controller binary {} differs from caller {}; reconnect to promote the fresh binary)",
                    identity_version(),
                    client_version
                );
            }
            controller_envelope(handle_command_plane_submit(
                &bootstrap_snapshot,
                runtime.as_ref(),
                request,
            ))
        }
        "queue_context_clear_started" => controller_envelope(handle_queue_context_clear_started(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "queue_context_clear_deferred" => controller_envelope(handle_queue_context_clear_deferred(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "queue_context_clear_settled" => controller_envelope(handle_queue_context_clear_settled(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "queue_context_clear_status" => {
            controller_envelope(handle_queue_context_clear_status(runtime.as_ref(), request))
        }
        "queue_drain_stall_continuation_recorded" => {
            controller_envelope(handle_queue_drain_stall_continuation_recorded(
                &bootstrap_snapshot,
                runtime.as_ref(),
                request,
            ))
        }
        "queue_drain_stall_continuation_cleared" => {
            controller_envelope(handle_queue_drain_stall_continuation_cleared(
                &bootstrap_snapshot,
                runtime.as_ref(),
                request,
            ))
        }
        "queue_drain_stall_status" => {
            controller_envelope(handle_queue_drain_stall_status(runtime.as_ref(), request))
        }
        "route_submit_started" => controller_envelope(handle_route_submit_started(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "route_submit_settled" => controller_envelope(handle_route_submit_settled(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "route_submit_blocked" => controller_envelope(handle_route_submit_blocked(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "route_submit_status" => {
            controller_envelope(handle_route_submit_status(runtime.as_ref(), request))
        }
        "visible_write_commit_candidate_observed" => {
            controller_envelope(handle_visible_write_commit_candidate_observed(
                &bootstrap_snapshot,
                runtime.as_ref(),
                request,
            ))
        }
        "visible_write_commit_candidate_status" => {
            controller_envelope(handle_visible_write_commit_candidate_status(
                &bootstrap_snapshot,
                runtime.as_ref(),
                request,
            ))
        }
        "visible_write_commit_candidate_patch_status" => {
            controller_envelope(handle_visible_write_commit_candidate_patch_status(
                &bootstrap_snapshot,
                runtime.as_ref(),
                request,
            ))
        }
        "peer_replicas_missing" => controller_envelope(handle_peer_replicas_missing(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "delivery_convergence_await" => controller_envelope(handle_delivery_convergence_await(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "visible_write_commit_candidate_patch_await" => {
            controller_envelope(handle_visible_write_commit_candidate_patch_await(
                &bootstrap_snapshot,
                runtime.as_ref(),
                request,
            ))
        }
        "visible_write_materialized_carry_forward_observed" => {
            controller_envelope(handle_visible_write_materialized_carry_forward_observed(
                &bootstrap_snapshot,
                runtime.as_ref(),
                request,
            ))
        }
        "visible_write_materialized_carry_forward_status" => {
            controller_envelope(handle_visible_write_materialized_carry_forward_status(
                &bootstrap_snapshot,
                runtime.as_ref(),
                request,
            ))
        }
        "start_session" => controller_envelope(handle_start_session(
            &bootstrap_snapshot,
            Some(runtime.as_ref()),
            request,
        )),
        "register_supervisor" => controller_envelope(handle_register_supervisor(
            &bootstrap_snapshot,
            Some(runtime.as_ref()),
            request,
        )),
        "mark_lifecycle" => controller_envelope(handle_mark_lifecycle(
            &bootstrap_snapshot,
            Some(runtime.as_ref()),
            request,
        )),
        "supervisor_heartbeat" => controller_envelope(handle_supervisor_heartbeat(
            &bootstrap_snapshot,
            Some(runtime.as_ref()),
            request,
        )),
        "actor_binding" => controller_envelope(handle_actor_binding(
            &bootstrap_snapshot,
            Some(runtime.as_ref()),
            request,
        )),
        "dispatch" => {
            // #af88 B enforcement: refuse a session-write dispatch when the caller's
            // binary version differs from this controller's running version — the
            // controller is serving stale code. Reuse the `controller_binary_stale`
            // reason so the caller's existing `#ctlstalebin` one-retry loop reconnects
            // and `connect_or_launch` promotes the freshly-installed binary. This is
            // the wire-authoritative complement to the mtime-based check in
            // `handle_dispatch` (fires even when mtime/inode is ambiguous).
            if let Some(client_version) =
                stale_mutating_client_binary(client_binary_version.as_deref())
            {
                agent_doc_ops_log_io::log_op(
                    &bootstrap_snapshot.project_root,
                    &format!(
                        "dispatch_refused_client_binary_mismatch controller_version={} client_version={}",
                        identity_version(),
                        client_version
                    ),
                );
                anyhow::bail!(
                    "dispatch refused: controller_binary_stale (running controller binary {} differs from caller {}; reconnect to promote the fresh binary)",
                    identity_version(),
                    client_version
                );
            }
            controller_envelope(handle_dispatch(
                &bootstrap_snapshot,
                Some(runtime.as_ref()),
                request,
            ))
        }
        "session_status" => {
            controller_envelope(handle_session_status(&bootstrap_snapshot, request))
        }
        "inspect_actor" => controller_envelope(handle_inspect_actor(&bootstrap_snapshot, request)),
        "tmux_focus_state" => controller_envelope(handle_tmux_focus_state(&bootstrap_snapshot)),
        "tmux_layout_sync_state" => controller_envelope(handle_tmux_layout_sync_state(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "state_subscribe" => controller_envelope(handle_state_subscribe(runtime.as_ref(), request)),
        "retained_write_settlement" => {
            controller_envelope(handle_retained_write_settlement(runtime.as_ref(), request))
        }
        "reliable_sync" => controller_envelope(handle_reliable_sync(
            &bootstrap_snapshot.project_root,
            request,
        )),
        "reliable_sync_outbox" => controller_envelope(handle_reliable_sync_outbox(
            &bootstrap_snapshot.project_root,
            request,
        )),
        "reliable_sync_status" => {
            controller_envelope(handle_reliable_sync_status(&bootstrap_snapshot))
        }
        "focus_document_pane" => {
            controller_envelope(handle_focus_document_pane(&bootstrap_snapshot, request))
        }
        "sync_tmux_layout" => {
            controller_envelope(handle_sync_tmux_layout(&bootstrap_snapshot, request))
        }
        "queue_control" => controller_envelope(handle_queue_control(&bootstrap_snapshot, request)),
        "admin_control" => controller_envelope(handle_admin_control(&bootstrap_snapshot, request)),
        "projection_repair" => {
            controller_envelope(handle_projection_repair(&bootstrap_snapshot, request))
        }
        "attach_pane" => controller_envelope(handle_attach_pane(
            &bootstrap_snapshot,
            Some(runtime.as_ref()),
            request,
        )),
        "operator_command" => controller_envelope(handle_operator_command(
            &bootstrap_snapshot,
            Some(runtime.as_ref()),
            request,
        )),
        "supervisor_replacement" => controller_envelope(handle_supervisor_replacement(
            &bootstrap_snapshot,
            Some(runtime.as_ref()),
            request,
        )),
        "crdt_replica" => controller_envelope(handle_crdt_replica_rpc(
            &bootstrap_snapshot,
            Some(runtime.as_ref()),
            request,
        )),
        "editor_route" => {
            controller_envelope(handle_editor_route_rpc(&bootstrap_snapshot, request))
        }
        "editor_command_submit" => controller_envelope(handle_editor_command_submit_rpc(
            &bootstrap_snapshot,
            request,
        )),
        "editor_command_submit_async" => controller_envelope(
            handle_editor_command_submit_async_rpc(&bootstrap_snapshot, request),
        ),
        "editor_command_status" => controller_envelope(handle_editor_command_status_rpc(request)),
        "crdt_current_text" => {
            controller_envelope(handle_crdt_current_text_rpc(&bootstrap_snapshot, request))
        }
        "crdt_revision" => {
            controller_envelope(handle_crdt_revision_rpc(&bootstrap_snapshot, request))
        }
        "crdt_cp_write" => {
            controller_envelope(handle_crdt_cp_write_rpc(&bootstrap_snapshot, request))
        }
        "response_cell_add" => {
            controller_envelope(handle_response_cell_add_rpc(&bootstrap_snapshot, request))
        }
        "crdt_text_adopt" => {
            controller_envelope(handle_crdt_text_adopt_rpc(&bootstrap_snapshot, request))
        }
        "crdt_commit_barrier" => controller_envelope(handle_crdt_commit_barrier_rpc(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "commit_document" => controller_envelope(handle_commit_document_rpc(
            &bootstrap_snapshot,
            runtime.as_ref(),
            request,
        )),
        "compact_document" => {
            if let Some(client_version) =
                stale_mutating_client_binary(client_binary_version.as_deref())
            {
                anyhow::bail!(
                    "Compact Exchange refused: controller_binary_stale (running controller binary {} differs from caller {}; reconnect to promote the fresh binary)",
                    identity_version(),
                    client_version,
                );
            }
            controller_envelope(handle_compact_document_rpc(
                &bootstrap_snapshot,
                runtime.as_ref(),
                request,
            ))
        }
        "crdt_record_committed_baseline" => controller_envelope(
            handle_crdt_record_committed_baseline_rpc(&bootstrap_snapshot, request),
        ),
        "crdt_disk_projection_reconcile" => controller_envelope(
            handle_crdt_disk_projection_reconcile_rpc(&bootstrap_snapshot, request),
        ),
        "crdt_route_disk_change_signal" => controller_envelope(
            handle_crdt_route_disk_change_signal_rpc(&bootstrap_snapshot, request),
        ),
        "admin_operation" => {
            controller_envelope(handle_admin_operation(&bootstrap_snapshot, request))
        }
        other => Ok(serde_json::to_string(&serde_json::json!({
                "ok": false,
                "error": format!("unknown controller command: {other}")
        }))?),
    }
}

fn stale_mutating_client_binary(client_version: Option<&str>) -> Option<&str> {
    client_version.filter(|version| *version != identity_version())
}

pub(crate) fn controller_envelope<T: Serialize>(result: Result<T>) -> Result<String> {
    match result {
        Ok(data) => Ok(serde_json::to_string(&serde_json::json!({
            "ok": true,
            "data": data
        }))?),
        Err(err) => Ok(serde_json::to_string(&serde_json::json!({
            "ok": false,
            "error": format!("{err:#}")
        }))?),
    }
}

#[derive(Debug, Serialize)]
struct ControllerStateSubscribeResponse {
    document_hash: String,
    message: serde_json::Value,
    /// Durable `state_events.document_version` represented by this response.
    ///
    /// This is not the Lazily graph epoch: it remains monotonic across
    /// count-cap retention and is the cursor peers acknowledge on their next
    /// subscription.
    document_version: u64,
    /// Whether the preceding cursor supplied by this peer was accepted.
    peer_ack_recorded: bool,
}

#[derive(Debug, Default, Deserialize)]
struct ControllerStateSubscribePayload {
    document_hash: Option<String>,
    peer_pid: Option<u64>,
    editor_id: Option<String>,
    acked_version: Option<u64>,
}

fn handle_state_subscribe(
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<ControllerStateSubscribeResponse> {
    let file = request_file(&request)?;
    let payload = request
        .diagnostic_payload
        .as_deref()
        .and_then(|payload| serde_json::from_str::<ControllerStateSubscribePayload>(payload).ok())
        .unwrap_or_default();
    let document_hash = payload
        .document_hash
        .unwrap_or_else(|| agent_doc_hash::document_id_for_path(&file));
    let last_epoch = request.generation.unwrap_or(0);

    // Closeout writers still append state-backbone facts directly to the durable
    // store in a few recovery-oriented paths. Refresh before serving editor
    // state so the Project Controller projection remains the hot-path source and
    // editor integrations never need to inspect sidecar files.
    runtime.refresh_memory()?;
    // `#lzsync` 3B wire cutover (flipped): emit the canonical native lazily wire
    // (`IpcMessage` Snapshot/Delta, `NodeId`/`IpcValue`) instead of the bespoke
    // base64-`WireDelta` `WireSubscribe` JSON. The plugins fold this through a
    // generic lazily `GraphView` and layer their own `AgentDocProjection` on top;
    // there is intentionally no fallback (the plugin rewrites land in the same flip).
    // `build_delta` still produces the internal `WireDelta` producer form; the
    // state-wire bridge converts it here (fully porting `build_delta` to native is a
    // follow-up).
    let (wire, document_version) = runtime.state_subscribe(&document_hash, last_epoch)?;
    let message = serde_json::to_value(
        agent_doc_state_wire::lazily_convert::wire_subscribe_to_ipc_message(&wire)?,
    )
    .context("serialize native IpcMessage for state_subscribe")?;

    let live_registrations = controller_liveness_plane()
        .lock()
        .projection()
        .live_registrations(&document_hash);
    let project_root = runtime.bootstrap_snapshot()?.project_root;
    let state_db = agent_doc_sqlite::state_store::open_state_db(&project_root)?;

    // The cursor reports the PREVIOUS response, so it is safe only after the
    // editor successfully folded that snapshot/delta. Bind it to the exact
    // live PID-scoped registration; never let a synthetic socket client create
    // a retention peer. Failure to collect an ack must not make the read path
    // unavailable — a missing cursor contributes zero to the live-peer minimum.
    let peer_ack_recorded = match (
        payload.peer_pid,
        payload.editor_id.as_deref(),
        payload.acked_version.filter(|version| *version > 0),
    ) {
        (Some(peer_pid), Some(editor_id), Some(acked_version))
            if live_registrations.iter().any(|registration| {
                registration.pid == peer_pid && registration.editor_id == editor_id
            }) =>
        {
            match agent_doc_sqlite::state_store::record_state_event_peer_ack_in_db(
                &state_db,
                &document_hash,
                peer_pid,
                editor_id,
                acked_version,
            ) {
                Ok(_) => true,
                Err(err) => {
                    eprintln!(
                        "[agent-doc] warning: state replay ack rejected \
                         document_hash={document_hash} pid={peer_pid} editor_id={editor_id}: {err:#}"
                    );
                    false
                }
            }
        }
        _ => false,
    };
    let live_peers = live_registrations
        .iter()
        .map(|registration| (registration.pid, registration.editor_id.clone()))
        .collect::<Vec<_>>();
    match agent_doc_sqlite::state_store::prune_state_events_to_live_peer_watermark_in_db(
        &state_db,
        &document_hash,
        &live_peers,
    ) {
        Ok(outcome) if outcome.evicted_peer_rows > 0 || outcome.deleted_event_rows > 0 => {
            eprintln!(
                "[state-retention] document_hash={document_hash} \
                 live_peers={} evicted_peers={} minimum_acked_version={:?} deleted_events={}",
                outcome.live_peer_count,
                outcome.evicted_peer_rows,
                outcome.minimum_acked_version,
                outcome.deleted_event_rows,
            );
        }
        Ok(_) => {}
        Err(err) => {
            eprintln!(
                "[agent-doc] warning: live-peer state retention failed \
                 document_hash={document_hash}: {err:#}"
            );
        }
    }
    Ok(ControllerStateSubscribeResponse {
        document_hash,
        message,
        document_version,
        peer_ack_recorded,
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ControllerReliableSyncResponse {
    pub document_hash: String,
    /// Ack cursor the pushing plugin uses to prune / resume its outbox.
    pub ack_through: u64,
    /// Lazily accepted the frame into the authoritative plane.
    pub accepted: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ControllerReliableSyncOutboxPayload {
    document_hash: String,
    /// A raw Lazily frame to durably append. `None` means flush only.
    frame: Option<lazily::IpcMessage>,
    /// Flush the retained suffix after an optional append.
    flush: bool,
}

/// Ask the project controller to own the durable sender outbox for one raw
/// reliable-sync frame. The caller is deliberately stateless: epoch allocation,
/// append-before-apply, replay, and pruning all occur in the controller process.
pub fn enqueue_reliable_sync_frame(
    project_root: &Path,
    file: Option<&Path>,
    document_hash: &str,
    frame: lazily::IpcMessage,
    flush: bool,
) -> Result<ControllerReliableSyncResponse> {
    request_reliable_sync_outbox(
        project_root,
        file,
        ControllerReliableSyncOutboxPayload {
            document_hash: document_hash.to_string(),
            frame: Some(frame),
            flush,
        },
    )
}

/// Flush a controller-owned reliable-sync sender channel without appending.
pub fn flush_reliable_sync_channel(
    project_root: &Path,
    file: Option<&Path>,
    document_hash: &str,
) -> Result<ControllerReliableSyncResponse> {
    request_reliable_sync_outbox(
        project_root,
        file,
        ControllerReliableSyncOutboxPayload {
            document_hash: document_hash.to_string(),
            frame: None,
            flush: true,
        },
    )
}

fn request_reliable_sync_outbox(
    project_root: &Path,
    file: Option<&Path>,
    payload: ControllerReliableSyncOutboxPayload,
) -> Result<ControllerReliableSyncResponse> {
    let request = ControllerRequest {
        command: "reliable_sync_outbox".to_string(),
        file: file.map(Path::to_path_buf),
        session_id: None,
        pane_id: None,
        window_id: None,
        generation: None,
        state: None,
        caller: Some("reliable_sync_outbox_client".to_string()),
        reason: None,
        supervisor_pid: None,
        supervisor_socket: None,
        command_kind: None,
        diagnostic_payload: Some(serde_json::to_string(&payload)?),
    };
    request_controller::<ControllerReliableSyncResponse>(project_root, request)
}

/// Client side of the `reliable_sync` RPC: push one liveness envelope to the
/// controller and return its response (ack cursor + acceptance).
///
/// The plugin-push endpoint's [`agent_doc_reliable_sync_io::push::LivenessPushTransport`]
/// wraps this. `envelope` is an [`agent_doc_reliable_sync_io::encode_envelope`] value;
/// `epoch` is the outbox retention key.
pub fn push_reliable_sync_liveness(
    project_root: &Path,
    epoch: u64,
    envelope: &serde_json::Value,
) -> Result<ControllerReliableSyncResponse> {
    let request = ControllerRequest {
        command: "reliable_sync".to_string(),
        file: None,
        session_id: None,
        pane_id: None,
        window_id: None,
        generation: Some(epoch),
        state: None,
        caller: Some("reliable_sync_push".to_string()),
        reason: None,
        supervisor_pid: None,
        supervisor_socket: None,
        command_kind: None,
        diagnostic_payload: Some(envelope.to_string()),
    };
    request_controller::<ControllerReliableSyncResponse>(project_root, request)
}

/// Client side of the `reliable_sync` RPC that ALSO carries the document `file` so the
/// controller can route document-op / full-state-adopt frames (`#docop-plane` /
/// `#reattach-adopt`) to the right relay canonical (the liveness path is hash-keyed and
/// leaves `file` unset). `envelope` wraps the frame; `epoch` is the outbox retention key.
pub fn push_reliable_sync_frame_for_file(
    project_root: &Path,
    file: &Path,
    epoch: u64,
    envelope: &serde_json::Value,
) -> Result<ControllerReliableSyncResponse> {
    let request = ControllerRequest {
        command: "reliable_sync".to_string(),
        file: Some(file.to_path_buf()),
        session_id: None,
        pane_id: None,
        window_id: None,
        generation: Some(epoch),
        state: None,
        caller: Some("reliable_sync_frame_push".to_string()),
        reason: None,
        supervisor_pid: None,
        supervisor_socket: None,
        command_kind: None,
        diagnostic_payload: Some(envelope.to_string()),
    };
    request_controller::<ControllerReliableSyncResponse>(project_root, request)
}

/// [`agent_doc_reliable_sync_io::push::LivenessPushTransport`] over the controller
/// `reliable_sync` RPC — the production transport the plugin-push endpoint flushes
/// through. One per project root.
pub struct RpcLivenessPushTransport {
    project_root: std::path::PathBuf,
}

impl RpcLivenessPushTransport {
    pub fn new(project_root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
        }
    }
}

impl agent_doc_reliable_sync_io::push::LivenessPushTransport for RpcLivenessPushTransport {
    fn push(&self, _document_hash: &str, epoch: u64, envelope: &serde_json::Value) -> Result<u64> {
        let response = push_reliable_sync_liveness(&self.project_root, epoch, envelope)?;
        Ok(response.ack_through)
    }
}

/// Reliable-sync transport for document-scoped frames. Unlike liveness, the
/// request carries `file`, allowing the controller to fold the frame into the
/// correct relay canonical.
pub struct RpcDocumentPushTransport {
    project_root: std::path::PathBuf,
    file: std::path::PathBuf,
}

impl RpcDocumentPushTransport {
    pub fn new(
        project_root: impl Into<std::path::PathBuf>,
        file: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self {
            project_root: project_root.into(),
            file: file.into(),
        }
    }
}

impl agent_doc_reliable_sync_io::push::LivenessPushTransport for RpcDocumentPushTransport {
    fn push(&self, _document_hash: &str, epoch: u64, envelope: &serde_json::Value) -> Result<u64> {
        let response =
            push_reliable_sync_frame_for_file(&self.project_root, &self.file, epoch, envelope)?;
        Ok(response.ack_through)
    }
}

/// The reliable-sync liveness plane the controller feeds and reads (sidecar-retirement
/// Phase 3C + step 3). The global lives in `agent-doc-reliable-sync-io`
/// ([`agent_doc_reliable_sync_io::global_liveness_plane`]) so the same instance the
/// controller feeds is the one [`crdt_authority_for_file`] reads — the plane is the
/// sole live-editor metadata authority.
fn controller_liveness_plane()
-> &'static parking_lot::Mutex<agent_doc_reliable_sync_io::plane::ControllerLivenessPlane> {
    agent_doc_reliable_sync_io::global_liveness_plane()
}

fn retain_state_events_for_live_registrations(
    project_root: &Path,
    document_hash: &str,
    registrations: &[agent_doc_reliable_sync_io::liveness::EditorRegistration],
) -> Result<agent_doc_sqlite::state_store::StateEventWatermarkRetention> {
    let state_db = agent_doc_sqlite::state_store::open_state_db(project_root)?;
    let live_peers = registrations
        .iter()
        .map(|registration| (registration.pid, registration.editor_id.clone()))
        .collect::<Vec<_>>();
    agent_doc_sqlite::state_store::prune_state_events_to_live_peer_watermark_in_db(
        &state_db,
        document_hash,
        &live_peers,
    )
}

fn restored_reliable_sync_projects() -> &'static parking_lot::Mutex<BTreeSet<PathBuf>> {
    static RESTORED: std::sync::LazyLock<parking_lot::Mutex<BTreeSet<PathBuf>>> =
        std::sync::LazyLock::new(|| parking_lot::Mutex::new(BTreeSet::new()));
    &RESTORED
}

/// Rebuild the in-memory liveness plane from facts the receiver committed before
/// acknowledging them. This is intentionally idempotent: CRDT joins and cursor
/// maxima make duplicate hydration harmless.
fn restore_reliable_sync_liveness(project_root: &Path) -> Result<()> {
    let project_key = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    if restored_reliable_sync_projects()
        .lock()
        .contains(&project_key)
    {
        return Ok(());
    }

    let snapshot = agent_doc_sqlite::reliable_sync_inbox::load(
        &agent_doc_sqlite::state_store::state_db_path(project_root),
    )?;
    let batches = snapshot
        .liveness
        .iter()
        .map(|record| {
            serde_json::from_str::<Vec<agent_doc_reliable_sync_io::liveness::LivenessOp>>(
                &record.ops_json,
            )
            .with_context(|| {
                format!(
                    "decode durable reliable-sync liveness source={} epoch={}",
                    record.source_key, record.epoch
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let dead_open_pids = {
        let mut plane = controller_liveness_plane().lock();
        for batch in &batches {
            plane.restore_liveness(batch);
        }
        for cursor in &snapshot.cursors {
            plane.restore_cursor(&cursor.document_hash, cursor.ack_through);
        }
        plane
            .projection()
            .all_open_pids()
            .into_iter()
            .filter(|pid| {
                u32::try_from(*pid)
                    .ok()
                    .is_none_or(|pid| !agent_doc_reliable_sync_io::process_pid_is_live(pid))
            })
            .collect::<Vec<_>>()
    };
    restored_reliable_sync_projects().lock().insert(project_key);
    // A process can die while the controller is down, so its exit watcher never
    // gets a chance to publish Alive(false). Reconcile those durable Open facts
    // once at hydration and record the death through the same durable LWW path
    // as a live exit event. This prevents a dead IDE pid from retaining editor
    // authority forever after controller/IDE restart.
    for pid in dead_open_pids {
        record_reliable_sync_editor_exit(project_root, pid);
    }
    request_editor_replica_rebuild_after_restart(project_root);
    // Arm the Tier 2 reactive path from boot as well, so later registrations are
    // covered without another explicit call site.
    publish_editor_replica_rebuild_targets(project_root);
    Ok(())
}

/// `#ctrlkillreregister` — ask every surviving editor to rebuild its replica once the
/// controller is back.
///
/// Hydration above restores the durable liveness plane, so after a controller
/// restart the editor still reads as *registered*. But the relay hub that holds each
/// replica's CRDT membership is a **process-local** static: it died with the previous
/// controller and nothing rehydrates it. The document therefore resolves as
/// attached-with-missing-replica — an editor the plane says is live, with no replica
/// behind it — until the plugin happens to re-register on its own.
///
/// That gap is why killing the controller strands a live IDE: operators see the
/// document stop converging and reach for an IDE restart to force re-registration.
/// The controller already knows exactly who to ask, so ask them instead of waiting.
///
/// Best-effort and non-destructive: this only requests a refresh. An editor that
/// cannot be reached is left to the existing missing-replica recovery, and a failure
/// here must never block the controller from finishing startup — so failures are
/// logged, never propagated.
/// `#ctrlkillreregister` Tier 2 — the rebuild request as an **Effect over a signal**,
/// so it fires whenever an editor becomes registered-without-a-replica rather than
/// only at the restart instant.
///
/// Tier 1 is a one-shot call at boot: an editor that registers later, or reconnects
/// while the controller is up, hits the same missing-replica state with nobody left
/// to ask. That is the shape this codebase keeps getting burned by — a side effect
/// someone must remember to invoke at the right moment (`#stategraphjoin`).
///
/// Tier 3 (the editor pulling `peer_replicas_missing` about itself) is the better
/// mechanism and needs no push at all. This tier remains because it covers editors
/// whose plugin does not yet call that pull: the controller keeps them working
/// without either side being upgraded first. It is a compatibility layer with an
/// explicit retirement condition, not the destination.
///
/// The target set lives in a `Source` inside a [`ProcessScope`] — a real scope with a
/// process lifetime, not another private context — and the request is an `Effect`
/// reading it. Publishing is idempotent and an empty set does nothing, so callers
/// simply report the current truth whenever it may have changed.
struct EditorReplicaRebuildPlane {
    scope: agent_doc_state_backbone::ProcessScope,
    targets: lazily::Source<Vec<(String, u64, String)>>,
    /// Held so the effect is not disposed; it re-runs on every `targets` change.
    _effect: lazily::Effect,
}

fn editor_replica_rebuild_plane(project_root: &Path) -> &'static EditorReplicaRebuildPlane {
    static PLANE: std::sync::OnceLock<EditorReplicaRebuildPlane> = std::sync::OnceLock::new();
    PLANE.get_or_init(|| {
        let scope = agent_doc_state_backbone::ProcessScope::new();
        let targets: lazily::Source<Vec<(String, u64, String)>> = scope.ctx().source(Vec::new());
        let root = project_root.to_path_buf();
        // `Source` is `Copy`, so the `move` closure takes its own handle to the same
        // cell and `targets` stays usable below — no rebinding needed.
        let effect = scope.ctx().effect(move |ctx| {
            for (path, pid, editor_id) in ctx.get(&targets) {
                let file = std::path::PathBuf::from(&path);
                match agent_doc_crdt_relay_io::signal_crdt_replica_event(
                    &file,
                    agent_doc_crdt_relay_io::CrdtReplicaEventReason::AckRecoveryForceRefresh,
                    1,
                ) {
                    Ok(()) => agent_doc_ops_log_io::log_op(
                        &root,
                        &format!(
                            "editor_replica_rebuild_requested file={path} pid={pid} editor_id={editor_id} driver=effect"
                        ),
                    ),
                    // Never swallow: an editor we could not reach is exactly the one
                    // an operator will find stranded, so name it.
                    Err(err) => agent_doc_ops_log_io::log_op(
                        &root,
                        &format!(
                            "editor_replica_rebuild_failed file={path} pid={pid} editor_id={editor_id} error={}",
                            format!("{err:#}")
                                .replace('\n', " | ")
                                .chars()
                                .take(160)
                                .collect::<String>()
                        ),
                    ),
                }
            }
        });
        EditorReplicaRebuildPlane {
            scope,
            targets,
            _effect: effect,
        }
    })
}

/// Publish the current missing-replica set so the Tier 2 effect re-runs.
///
/// Idempotent: an unchanged set does not re-fire, and an empty set does nothing.
/// Called at controller start and whenever folded liveness ops may have changed the
/// registration set.
fn publish_editor_replica_rebuild_targets(project_root: &Path) {
    // The hub is process-local, so anything the plane says is registered but this
    // process cannot serve is stranded.
    let held: BTreeSet<String> = BTreeSet::new();
    let targets: Vec<(String, u64, String)> = controller_liveness_plane()
        .lock()
        .projection()
        .registrations_missing_replica(&held)
        .into_iter()
        .filter(|registration| !controller_serves_replica(&registration.path))
        .filter(|registration| !peer_repairs_itself(registration))
        .map(|registration| (registration.path, registration.pid, registration.editor_id))
        .collect();
    let plane = editor_replica_rebuild_plane(project_root);
    plane.scope.ctx().set(&plane.targets, targets);
}

/// Whether this registration's editor advertises the Tier 3 pull and therefore
/// repairs itself.
///
/// The capability travels on the registration, which is part of the same replicated
/// liveness plane the desired set is derived from — so the fan-out retires **per
/// peer**, from converged state, with no version handshake and no flag day. An old
/// plugin in one IDE keeps getting the compatibility push while a current one in the
/// next IDE does not.
fn peer_repairs_itself(
    registration: &agent_doc_reliable_sync_io::liveness::EditorRegistration,
) -> bool {
    agent_doc_document_realtime::editor_contract::has_capability(
        &registration.capabilities,
        agent_doc_document_realtime::editor_contract::PEER_REPLICA_PULL_CAPABILITY,
    )
}

fn request_editor_replica_rebuild_after_restart(project_root: &Path) {
    // Tier 3: ask the REPLICATED plane which registrations lack a replica here,
    // rather than re-deriving "everyone" and pushing at all of them. `held` is what
    // this process actually has — the hub is process-local, so after a restart it is
    // empty and the derivation names exactly the stranded set. The same derivation
    // run on the editor side lets it repair itself without being told.
    let held: BTreeSet<String> = BTreeSet::new();
    let registrations = controller_liveness_plane()
        .lock()
        .projection()
        .registrations_missing_replica(&held);
    let mut requested: BTreeSet<String> = BTreeSet::new();
    for registration in registrations {
        // The retirement condition (`#ctrlkillreregister`): a peer that pulls
        // `peer_replicas_missing` about itself repairs without being pushed at, so
        // pushing anyway is a delivery that can only fail, never help.
        if peer_repairs_itself(&registration) {
            agent_doc_ops_log_io::log_op(
                project_root,
                &format!(
                    "controller_restart_editor_replica_rebuild_skipped file={} pid={} editor_id={} reason=peer_replica_pull",
                    registration.path, registration.pid, registration.editor_id
                ),
            );
            continue;
        }
        if !requested.insert(registration.path.clone()) {
            continue;
        }
        let file = std::path::PathBuf::from(&registration.path);
        match agent_doc_crdt_relay_io::signal_crdt_replica_event(
            &file,
            agent_doc_crdt_relay_io::CrdtReplicaEventReason::AckRecoveryForceRefresh,
            1,
        ) {
            Ok(()) => agent_doc_ops_log_io::log_op(
                project_root,
                &format!(
                    "controller_restart_editor_replica_rebuild_requested file={} pid={} editor_id={}",
                    registration.path, registration.pid, registration.editor_id
                ),
            ),
            // Never swallow: an editor we could not reach is exactly the one an
            // operator will find stranded, so name it.
            Err(err) => agent_doc_ops_log_io::log_op(
                project_root,
                &format!(
                    "controller_restart_editor_replica_rebuild_failed file={} pid={} editor_id={} error={}",
                    registration.path,
                    registration.pid,
                    registration.editor_id,
                    format!("{err:#}")
                        .replace('\n', " | ")
                        .chars()
                        .take(160)
                        .collect::<String>()
                ),
            ),
        }
    }
}

/// Default-on liveness authority with a durable cold path. The hot path is one
/// in-memory CRDT projection read. A cold process replays the receiver journal
/// plus the sender's retained suffix; it never reconciles live-buffer sidecars or
/// consults a plugin-owner lease or live-buffer filesystem model.
pub fn reliable_sync_editor_live_for_file(file: &Path) -> bool {
    agent_doc_crdt_relay_io::reliable_sync_editor_live_for_file(file)
}

/// The hot-path CRDT authority for `file` (sidecar-retirement P3/P4). The
/// reliable-sync plane is **primary**: its `live_docs` decides `MultiReplica` /
/// `GitAuthoritative`, and a cold process first hydrates the receiver journal plus
/// retained sender suffix. Filesystem leases and live-buffer sidecars are not read.
/// This is the single authority entry shared by controller and write paths.
pub fn crdt_authority_for_file(
    file: &str,
) -> agent_doc_document_realtime::crdt_authority::CrdtAuthority {
    use agent_doc_document_realtime::crdt_authority::CrdtAuthority;
    if reliable_sync_editor_live_for_file(Path::new(file)) {
        CrdtAuthority::MultiReplica
    } else {
        CrdtAuthority::GitAuthoritative
    }
}

/// Status response for the `reliable_sync_status` diagnostic RPC. The Lazily
/// reliable-sync projection is the sole live-editor authority; no filesystem
/// live-buffer model participates in this response.
#[derive(Debug, Serialize, Deserialize)]
pub struct ControllerReliableSyncStatusResponse {
    /// Document hashes the plane derives as open (from pushed liveness frames).
    pub plane_open_docs: Vec<String>,
    /// Each plane-open hash resolved through a live editor registration.
    pub plane_open_paths: Vec<(String, Option<String>)>,
    /// Document hashes the plane derives as live (open minus the whole-editor-death cascade).
    pub plane_live_docs: Vec<String>,
    /// Live editor identity/version/capability registrations carried on the same
    /// monotone reliable-sync channel as open/close state.
    pub registrations: Vec<agent_doc_reliable_sync_io::liveness::EditorRegistration>,
    /// Volatile in-process registry, retained as a diagnostic only.
    pub registry_open_docs: Vec<String>,
    /// Per open document: the live editor pids the plane sees.
    pub per_doc_pids: Vec<(String, Vec<u64>)>,
}

/// Handle the `reliable_sync_status` diagnostic RPC from the controller-owned
/// reliable liveness projection.
fn handle_reliable_sync_status(
    _bootstrap: &ControllerBootstrap,
) -> Result<ControllerReliableSyncStatusResponse> {
    let plane = controller_liveness_plane().lock();
    let projection = plane.projection();
    let plane_open = projection.open_docs();
    let plane_live = projection.live_docs();
    let per_doc_pids: Vec<(String, Vec<u64>)> = plane_open
        .iter()
        .map(|doc| {
            (
                doc.clone(),
                projection
                    .open_pids(doc)
                    .into_iter()
                    .filter(|pid| projection.pid_alive(*pid))
                    .collect(),
            )
        })
        .collect();
    let registrations = projection.all_live_registrations();
    let plane_open_paths: Vec<(String, Option<String>)> = plane_open
        .iter()
        .map(|hash| {
            let path = registrations
                .iter()
                .filter(|registration| &registration.document_hash == hash)
                .max_by_key(|registration| {
                    (
                        registration.timestamp_ms,
                        registration.pid,
                        registration.editor_id.as_str(),
                    )
                })
                .map(|registration| registration.path.clone());
            (hash.clone(), path)
        })
        .collect();
    // Volatile in-memory registry — secondary diagnostic only.
    let registry_open: std::collections::BTreeSet<String> =
        agent_doc_document_realtime::editor_open_docs::editor_open_docs()
            .open_agent_docs()
            .into_iter()
            .map(|path| agent_doc_hash::document_id_for_path(std::path::Path::new(&path)))
            .collect();
    Ok(ControllerReliableSyncStatusResponse {
        plane_open_docs: plane_open.into_iter().collect(),
        plane_open_paths,
        plane_live_docs: plane_live.into_iter().collect(),
        registrations,
        registry_open_docs: registry_open.into_iter().collect(),
        per_doc_pids,
    })
}

/// Client side of the `reliable_sync_status` diagnostic RPC.
pub fn reliable_sync_status(project_root: &Path) -> Result<ControllerReliableSyncStatusResponse> {
    let request = ControllerRequest {
        command: "reliable_sync_status".to_string(),
        file: None,
        session_id: None,
        pane_id: None,
        window_id: None,
        generation: None,
        state: None,
        caller: Some("reliable_sync_status".to_string()),
        reason: None,
        supervisor_pid: None,
        supervisor_socket: None,
        command_kind: None,
        diagnostic_payload: None,
    };
    request_controller::<ControllerReliableSyncStatusResponse>(project_root, request)
}

/// Resolve live editor registrations for one document from the controller-owned
/// Lazily liveness projection. The result is ordered deterministically for
/// diagnostics; single-target selection explicitly orders by registration time.
pub fn live_editor_registrations_for_file(
    file: &Path,
) -> Result<Vec<agent_doc_reliable_sync_io::liveness::EditorRegistration>> {
    // Registration diagnostics are meaningful only when Lazily says an editor
    // replica is actually attached. Starting or polling a project controller
    // to prove an already-detached document adds a 45s retry ladder to repair
    // and can steal focus from unrelated operator activity.
    if !agent_doc_crdt_relay_io::crdt_authority_for_file(file).editor_attached() {
        return Ok(Vec::new());
    }
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .ok_or_else(|| anyhow::anyhow!("no agent-doc project root for {}", file.display()))?;
    let document_hash = agent_doc_hash::document_id_for_path(file);
    let mut registrations = if agent_doc_crdt_relay_io::embedded_relay_is_available_for_file(file) {
        controller_liveness_plane()
            .lock()
            .projection()
            .live_registrations(&document_hash)
    } else {
        reliable_sync_status(&project_root)?
            .registrations
            .into_iter()
            .filter(|registration| registration.document_hash == document_hash)
            .collect::<Vec<_>>()
    };
    registrations.sort();
    Ok(registrations)
}

pub fn live_editor_registration_for_file(
    file: &Path,
) -> Result<Option<agent_doc_reliable_sync_io::liveness::EditorRegistration>> {
    Ok(live_editor_registrations_for_file(file)?
        .into_iter()
        .max_by(|left, right| {
            (left.timestamp_ms, left.pid, left.editor_id.as_str()).cmp(&(
                right.timestamp_ms,
                right.pid,
                right.editor_id.as_str(),
            ))
        }))
}

/// Feed the shadow liveness plane the controller's own OS-exit-watcher death
/// signal (`#s4b`, Phase 3C): a dead editor `pid` writes `Alive{value:false}` at a
/// fresh stamp, so the derived live-doc aggregate cascades every doc that pid held
/// to not-live.
pub fn record_reliable_sync_editor_exit(project_root: &Path, pid: u64) {
    let wall_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);
    let op = agent_doc_reliable_sync_io::liveness::LivenessOp::Alive {
        pid,
        value: false,
        stamp: lazily::WireStamp {
            wall_time,
            logical: 0,
            peer: 0,
        },
    };
    let ops_json = match serde_json::to_string(&vec![op.clone()]) {
        Ok(value) => value,
        Err(error) => {
            eprintln!(
                "[reliable-sync] record_reliable_sync_editor_exit: serialize failed: {error}"
            );
            return;
        }
    };
    let source_key = format!("controller-alive:{pid}");
    if let Err(error) = agent_doc_sqlite::reliable_sync_inbox::record_local_liveness(
        &agent_doc_sqlite::state_store::state_db_path(project_root),
        &source_key,
        wall_time,
        &ops_json,
    ) {
        eprintln!(
            "[reliable-sync] record_reliable_sync_editor_exit: durable write failed: {error:#}"
        );
        return;
    }
    let affected_documents = {
        let mut plane = controller_liveness_plane().lock();
        let affected_documents = plane
            .projection()
            .all_live_registrations()
            .into_iter()
            .filter(|registration| registration.pid == pid)
            .map(|registration| registration.document_hash)
            .collect::<BTreeSet<_>>();
        plane.apply_local(&op);
        affected_documents
            .into_iter()
            .map(|document_hash| {
                let registrations = plane.projection().live_registrations(&document_hash);
                (document_hash, registrations)
            })
            .collect::<Vec<_>>()
    };
    for (document_hash, registrations) in affected_documents {
        match retain_state_events_for_live_registrations(
            project_root,
            &document_hash,
            &registrations,
        ) {
            Ok(outcome) if outcome.evicted_peer_rows > 0 || outcome.deleted_event_rows > 0 => {
                eprintln!(
                    "[state-retention] editor_exit_pid={pid} document_hash={document_hash} \
                     live_peers={} evicted_peers={} minimum_acked_version={:?} deleted_events={}",
                    outcome.live_peer_count,
                    outcome.evicted_peer_rows,
                    outcome.minimum_acked_version,
                    outcome.deleted_event_rows,
                );
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!(
                    "[reliable-sync] record_reliable_sync_editor_exit: \
                     state retention failed document_hash={document_hash}: {error:#}"
                );
            }
        }
    }
}

/// `plugin → controller` reliable-sync liveness push (`#lzsync`, Phase 3C).
///
/// The plugin sends the 3A `reliable_sync` envelope (from
/// `agent_doc_reliable_sync_io::encode_envelope`) as the `diagnostic_payload` and
/// the outbox epoch as `generation`. The frame folds into the
/// `ControllerLivenessPlane` and the returned
/// `ack_through` lets the plugin outbox prune / resume from the frontier.
struct InProcessReliableSyncTransport<'a> {
    project_root: &'a Path,
    file: Option<&'a Path>,
}

impl agent_doc_reliable_sync_io::push::LivenessPushTransport
    for InProcessReliableSyncTransport<'_>
{
    fn push(&self, _document_hash: &str, epoch: u64, envelope: &serde_json::Value) -> Result<u64> {
        let response = handle_reliable_sync(
            self.project_root,
            ControllerRequest {
                command: "reliable_sync".to_string(),
                file: self.file.map(Path::to_path_buf),
                session_id: None,
                pane_id: None,
                window_id: None,
                generation: Some(epoch),
                state: None,
                caller: Some("controller_owned_reliable_sync_outbox".to_string()),
                reason: None,
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: None,
                diagnostic_payload: Some(envelope.to_string()),
            },
        )?;
        Ok(response.ack_through)
    }
}

fn handle_reliable_sync_outbox(
    project_root: &Path,
    request: ControllerRequest,
) -> Result<ControllerReliableSyncResponse> {
    let payload: ControllerReliableSyncOutboxPayload = serde_json::from_str(
        request
            .diagnostic_payload
            .as_deref()
            .context("reliable_sync_outbox request missing diagnostic_payload")?,
    )
    .context("parse reliable_sync_outbox payload")?;
    let outbox = lazily::SqliteOutbox::open(
        &agent_doc_sqlite::state_store::state_db_path(project_root),
        payload.document_hash.clone(),
    )?;
    let acked_through = outbox.acked_through();
    let mut endpoint = agent_doc_reliable_sync_io::push::FramePushEndpoint::resuming(
        payload.document_hash.clone(),
        outbox,
        acked_through,
    );
    if let Some(frame) = payload.frame {
        endpoint.enqueue_frame(frame);
    }
    let ack_through = if payload.flush {
        endpoint
            .flush(&InProcessReliableSyncTransport {
                project_root,
                file: request.file.as_deref(),
            })?
            .acked_through
            .max(acked_through)
    } else {
        acked_through
    };
    Ok(ControllerReliableSyncResponse {
        document_hash: payload.document_hash,
        ack_through,
        accepted: true,
    })
}

fn handle_reliable_sync(
    project_root: &Path,
    request: ControllerRequest,
) -> Result<ControllerReliableSyncResponse> {
    let payload_str = request
        .diagnostic_payload
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("reliable_sync request missing diagnostic_payload"))?;
    let payload: serde_json::Value =
        serde_json::from_str(payload_str).context("parse reliable_sync envelope")?;
    let (document_hash, message) = agent_doc_reliable_sync_io::decode_envelope(&payload)
        .ok_or_else(|| {
            anyhow::anyhow!("reliable_sync payload is not a reliable-sync envelope")
        })??;
    let epoch = request.generation.unwrap_or(0);

    let liveness_ops =
        agent_doc_reliable_sync_io::liveness::decode_liveness_frame(&message).transpose()?;

    // `#docop-plane` P2b: a document-op frame folds into the relay canonical so a
    // connected editor's ops feed it even when its CRDT member registration lapsed
    // (`live_editors == 0`). Inert for liveness-only frames (no document-op node) and
    // when no path is supplied. A frame is idempotent + commutative only inside its
    // registered whole-document lineage; obsolete lineages are terminally quarantined
    // before the durable receiver cursor advances. Serving the fed canonical (removing
    // the frozen-canonical read) is the separate P3 authority flip — this only keeps the
    // canonical fed.
    // `#reattach-adopt` (bounded, runaway-safe): a TEXT-adopt frame rebuilds the canonical
    // from the editor's document text (O(text), self-echo-guarded). This is the path the
    // new one-shot-on-reattach plugin uses. Checked first.
    if let Some(file) = request.file.as_deref()
        && let Some(decoded) =
            agent_doc_reliable_sync_io::document_op::decode_text_adopt_frame(&message)
    {
        let text = decoded
            .with_context(|| format!("reliable_sync_text_adopt_malformed hash={document_hash}"))?;
        agent_doc_crdt_relay_io::adopt_editor_text_for_file(file, &text)
            .with_context(|| format!("reliable_sync_text_adopt_failed hash={document_hash}"))?;
    }

    // `#reattach-adopt`: a full-state adopt frame REPLACES the canonical with the
    // editor's authoritative lazily state (drops `#sy71`-class drift, lineage-intact),
    // as opposed to the fold path below which union-merges an incremental delta. Checked
    // first so an adopt frame never falls through to the fold. Inert until the FFI sends
    // an adopt frame on reattach.
    if let Some(file) = request.file.as_deref()
        && let Some(decoded) =
            agent_doc_reliable_sync_io::document_op::decode_full_state_adopt_frame(&message)
    {
        let ops = decoded.with_context(|| {
            format!("reliable_sync_full_state_adopt_malformed hash={document_hash}")
        })?;
        let full_state =
            agent_doc_merge::crdt_sync::encode_update_ops(&ops).with_context(|| {
                format!("reliable_sync_full_state_adopt_reencode_failed hash={document_hash}")
            })?;
        agent_doc_crdt_relay_io::adopt_editor_full_state_for_file(file, &full_state).with_context(
            || format!("reliable_sync_full_state_adopt_failed hash={document_hash}"),
        )?;
    }

    if let Some(file) = request.file.as_deref()
        && let Some(decoded) =
            agent_doc_reliable_sync_io::document_op::decode_document_op_frame(&message)
    {
        let batch = decoded.with_context(|| {
            format!("reliable_sync_document_op_frame_malformed hash={document_hash}")
        })?;
        let delta =
            agent_doc_merge::crdt_sync::encode_update_ops(&batch.ops).with_context(|| {
                format!("reliable_sync_document_op_reencode_failed hash={document_hash}")
            })?;
        agent_doc_crdt_relay_io::apply_document_op_delta_for_file_in_lineage(
            file,
            batch.lineage.as_deref(),
            &delta,
        )
        .with_context(|| format!("reliable_sync_document_op_fold_failed hash={document_hash}"))?;
    }

    // P4 receive durability boundary: every document-side effect above completes
    // before the receiver cursor advances. The liveness batch and cursor then
    // commit in one SQLite transaction before the ACK becomes observable. A crash
    // before this point leaves the sender frame retained; a crash after it can
    // rebuild the projection from the receiver journal.
    let liveness_ops_json = liveness_ops
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let ack_through = agent_doc_sqlite::reliable_sync_inbox::record_remote_frame(
        &agent_doc_sqlite::state_store::state_db_path(project_root),
        &document_hash,
        epoch,
        liveness_ops_json.as_deref(),
    )?;
    let folded_liveness = liveness_ops.is_some();
    {
        let mut plane = controller_liveness_plane().lock();
        if let Some(ops) = &liveness_ops {
            plane.restore_liveness(ops);
        }
        plane.restore_cursor(&document_hash, ack_through);
    }
    // `#ctrlkillreregister` Tier 2 — registrations just changed, so republish the
    // derived missing-replica set. This is what covers an editor that registers or
    // reconnects *after* the restart fan-out already ran; the effect no-ops when the
    // set is unchanged or empty. Done after the plane lock is released so the effect
    // never runs while holding it.
    if folded_liveness {
        publish_editor_replica_rebuild_targets(project_root);
    }

    Ok(ControllerReliableSyncResponse {
        document_hash,
        ack_through,
        accepted: true,
    })
}

pub(crate) fn actor_record_from_authority(
    bootstrap: &ControllerBootstrap,
    runtime: Option<&ControllerRuntime>,
    document_id: &str,
) -> Result<Option<agent_doc_sqlite::state_store::ActorRecord>> {
    if let Some(runtime) = runtime {
        runtime.actor_record(document_id)
    } else {
        load_actor_record(&bootstrap.project_root, document_id)
    }
}

pub(crate) fn refresh_runtime_after_actor_write(runtime: Option<&ControllerRuntime>) -> Result<()> {
    if let Some(runtime) = runtime {
        runtime.refresh_memory()?;
    }
    Ok(())
}

pub(crate) fn request_file(request: &ControllerRequest) -> Result<PathBuf> {
    request
        .file
        .clone()
        .context("controller request missing file")
}

pub(crate) fn request_string(value: &Option<String>, name: &str) -> Result<String> {
    value
        .clone()
        .with_context(|| format!("controller request missing {name}"))
}

pub(crate) fn request_u64(value: Option<u64>, name: &str) -> Result<u64> {
    value.with_context(|| format!("controller request missing {name}"))
}

fn closeout_owner_event_nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

pub(crate) fn handle_state_event_append(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<bool> {
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let event: agent_doc_state_backbone::StateEvent =
        serde_json::from_str(&payload_json).context("parse state actor append payload")?;
    let incoming_cycle = match &event.fact {
        agent_doc_state_backbone::StateFact::TurnIntentCheckpointed { cycle_id, .. }
        | agent_doc_state_backbone::StateFact::PreflightStarted { cycle_id, .. } => Some(cycle_id),
        _ => None,
    };
    if let Some(incoming_cycle) = incoming_cycle {
        let document = runtime.document_state_projection(event.document_hash())?;
        if document.as_ref().is_some_and(|document| {
            document.closeout.cycle_id.as_deref() != Some(incoming_cycle.as_str())
                && document.retained_captured_response_write().is_some()
        }) {
            anyhow::bail!(
                "state actor rejected cycle `{incoming_cycle}`: a retained document-write effect still owns the prior captured response"
            );
        }
    }
    append_apply_state_event(bootstrap, runtime, event)
}

/// Read-only live-projection query (`#lazily-hot-path`): return the controller's
/// authoritative in-memory [`DocumentStateProjection`] for the request file, so a
/// client inspects live state instead of replaying cold `state.db`. Counterpart
/// to `agent_doc_cycle_state_io::load_document_projection`, which prefers this
/// verb when a controller is live and falls back to `state.db` otherwise.
pub(crate) fn handle_document_state_projection(
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<Option<agent_doc_state_backbone::DocumentStateProjection>> {
    let file = request_file(&request)?;
    let document_hash = agent_doc_hash::document_id_for_path(&file);
    runtime.document_state_projection(&document_hash)
}

/// Shared core of closeout owner claim: decide the CAS from the live
/// in-memory projection (`decide_owner_claim`), emit the `CloseoutOwnerClaimed`
/// fact when acquired, and update the supervisor-recycle graph. Pure authority
/// logic reused by the bespoke `closeout_owner_claim` verb and the command-plane
/// `service_closeout_owner_claim` (`#lzdurablesink`).
fn run_closeout_owner_claim(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    file: &Path,
    claim: agent_doc_state_backbone::CloseoutOwnerClaimRequest,
) -> Result<agent_doc_state_backbone::CloseoutOwnerClaimOutcome> {
    use agent_doc_state_backbone::{CloseoutOwnerClaimOutcome, StateFact};
    let document_hash = agent_doc_hash::document_id_for_path(file);

    let (outcome, recycle) = {
        let mut memory = runtime.memory.lock();
        let current = memory
            .state_projection
            .document(&document_hash)
            .map(|document| document.closeout.clone())
            .unwrap_or_default();
        let current_owner_alive = current.owner.as_ref().and_then(|owner| {
            (owner.owner_id != claim.owner_id
                && owner.is_active_at(claim.now_secs)
                && claim.allow_dead_owner_takeover)
                .then(|| process_is_alive(owner.owner_pid))
        });
        // `#closeoutterminalreactive`: the same derived gate the reactive
        // `CloseoutGateState` exposes, read here so the live path and the graph
        // cannot drift. Its value is the *reason* the incumbent stopped
        // blocking, and a `lease_expired` reason is the stopgap firing — a
        // derived fact should have released this claim earlier and none did.
        // Logged as feedback rather than swallowed, because in a healthy system
        // this never appears.
        let displaced = agent_doc_state_backbone::closeout_gate::closeout_gate(
            current.cycle_id.as_deref(),
            current.owner.as_ref(),
            claim.now_secs,
            current_owner_alive,
            claim.allow_dead_owner_takeover,
        );
        if displaced.released_by_stopgap()
            && let Some(owner) = displaced.owner()
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "closeout_owner_released_by_stopgap file={} cycle_id={} owner_id={} owner_pid={} role={} reason=lease_expired",
                    file.display(),
                    owner.cycle_id,
                    owner.owner_id,
                    owner.owner_pid,
                    owner.role,
                ),
            );
        }
        let outcome = current.decide_owner_claim(&claim, current_owner_alive);
        if let CloseoutOwnerClaimOutcome::Acquired(owner) = &outcome {
            let event = agent_doc_state_backbone::StateEvent::new(
                format!(
                    "closeout-owner-claimed:{document_hash}:{}:{}:{}",
                    owner.cycle_id,
                    owner.owner_id,
                    closeout_owner_event_nonce()
                ),
                StateFact::CloseoutOwnerClaimed {
                    document_hash: document_hash.clone(),
                    cycle_id: owner.cycle_id.clone(),
                    owner_id: owner.owner_id.clone(),
                    owner_pid: owner.owner_pid,
                    role: owner.role.clone(),
                    claimed_secs: owner.claimed_secs,
                    expires_secs: owner.expires_secs,
                },
            );
            append_state_event(&bootstrap.project_root, &event)?;
            memory.state_ledger.append(event.clone());
            memory.state_projection.apply(&event);
        }
        let recycle = memory.state_projection.project_supervisor_recycle();
        (outcome, recycle)
    };
    runtime.supervisor_recycle_graph.set(recycle);
    runtime.supervisor_recycle_waiters.notify_all();

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "closeout_owner_actor_claim file={} owner_id={} expected_cycle={} outcome={outcome:?}",
            file.display(),
            claim.owner_id,
            claim.expected_cycle_id.as_deref().unwrap_or("current"),
        ),
    );
    Ok(outcome)
}

/// Shared core of closeout owner release: decide from the live projection
/// whether the caller still owns the cycle, emit `CloseoutOwnerReleased` when it
/// does, and update the supervisor-recycle graph. Reused by the bespoke verb
/// and the command-plane service.
fn run_closeout_owner_release(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    file: &Path,
    release: CloseoutOwnerReleaseRequest,
) -> Result<bool> {
    use agent_doc_state_backbone::StateFact;
    let document_hash = agent_doc_hash::document_id_for_path(file);

    let (released, recycle) = {
        let mut memory = runtime.memory.lock();
        let release_matches = memory
            .state_projection
            .document(&document_hash)
            .is_some_and(|document| {
                document
                    .closeout
                    .owner_release_matches(&release.cycle_id, &release.owner_id)
            });
        if release_matches {
            let event = agent_doc_state_backbone::StateEvent::new(
                format!(
                    "closeout-owner-released:{document_hash}:{}:{}:{}",
                    release.cycle_id,
                    release.owner_id,
                    closeout_owner_event_nonce()
                ),
                StateFact::CloseoutOwnerReleased {
                    document_hash: document_hash.clone(),
                    cycle_id: release.cycle_id.clone(),
                    owner_id: release.owner_id.clone(),
                    reason: release.reason.clone(),
                    released_secs: release.released_secs,
                },
            );
            append_state_event(&bootstrap.project_root, &event)?;
            memory.state_ledger.append(event.clone());
            memory.state_projection.apply(&event);
        }
        (
            release_matches,
            memory.state_projection.project_supervisor_recycle(),
        )
    };
    runtime.supervisor_recycle_graph.set(recycle);
    runtime.supervisor_recycle_waiters.notify_all();
    Ok(released)
}

pub(crate) fn handle_closeout_owner_claim(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::CloseoutOwnerClaimOutcome> {
    let file = request_file(&request)?;
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let claim: agent_doc_state_backbone::CloseoutOwnerClaimRequest =
        serde_json::from_str(&payload_json).context("parse closeout owner claim")?;
    run_closeout_owner_claim(bootstrap, runtime, &file, claim)
}

/// Command-plane service for a `closeout_owner_claim` (`#lzdurablesink`). Decodes
/// the [`lazily::CommandSubmit`], runs the shared claim core from the live
/// projection, and returns the typed outcome — the authority result for this
/// coordination CAS (Acquired / HeldByOther / CycleSuperseded).
fn service_closeout_owner_claim(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    submit: &lazily::CommandSubmit,
) -> Result<agent_doc_state_backbone::CloseoutOwnerClaimOutcome> {
    use super::command_plane::decode_closeout_owner_claim_payload;
    let payload = decode_closeout_owner_claim_payload(submit)?;
    let file = std::path::PathBuf::from(&payload.document_path);
    run_closeout_owner_claim(bootstrap, runtime, &file, payload.request)
}

pub(crate) fn handle_closeout_owner_release(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<bool> {
    let file = request_file(&request)?;
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let release: CloseoutOwnerReleaseRequest =
        serde_json::from_str(&payload_json).context("parse closeout owner release")?;
    run_closeout_owner_release(bootstrap, runtime, &file, release)
}

/// Owner-pane wedge counter RMW (`#lazily-hot-path`): the controller is the
/// SQLite authority for this runtime state, so the read-modify-write runs
/// server-side here. The client (`agent_doc_owner_pane_io::record`) calls this
/// when a controller is live and falls back to its own direct SQLite path only
/// for the actorless/bootstrap boundary. See `#recguard-wedge-escape`.
#[derive(Debug, Deserialize)]
struct OwnerPaneWedgePayload {
    document_hash: String,
    #[allow(dead_code)]
    head: String,
}

pub(crate) fn handle_record_owner_pane_wedge(
    bootstrap: &ControllerBootstrap,
    _runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<u32> {
    use agent_doc_turn::owner_pane_recursion::{
        OwnerPaneWedgeRecord, record_owner_pane_wedge_fire,
    };
    const OWNER_PANE_WEDGE_STATE_KIND: &str = "owner_pane_wedge";
    let canonical_path = request_file(&request)?;
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: OwnerPaneWedgePayload =
        serde_json::from_str(&payload_json).context("parse owner_pane_wedge payload")?;
    let mut conn = agent_doc_sqlite::state_store::open_state_db(&bootstrap.project_root)?;
    let tx = conn.transaction()?;
    let prior = agent_doc_sqlite::state_store::load_document_runtime_state_from_db(
        &tx,
        &payload.document_hash,
        OWNER_PANE_WEDGE_STATE_KIND,
    )?
    .and_then(|state| serde_json::from_str::<OwnerPaneWedgeRecord>(&state.payload_json).ok());
    let record = record_owner_pane_wedge_fire(prior.as_ref(), &payload.head);
    let count = record.count;
    agent_doc_sqlite::state_store::upsert_document_runtime_state_in_db(
        &tx,
        &agent_doc_sqlite::state_store::DocumentRuntimeStateRecord {
            document_hash: payload.document_hash,
            state_kind: OWNER_PANE_WEDGE_STATE_KIND.to_string(),
            canonical_path: canonical_path.to_string_lossy().into_owned(),
            payload_json: serde_json::to_string(&record)?,
            updated_at_ms: controller_now_ms(),
        },
    )?;
    tx.commit()?;
    Ok(count)
}

pub(crate) fn handle_clear_owner_pane_wedge(
    bootstrap: &ControllerBootstrap,
    _runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<bool> {
    const OWNER_PANE_WEDGE_STATE_KIND: &str = "owner_pane_wedge";
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: OwnerPaneWedgePayload =
        serde_json::from_str(&payload_json).context("parse owner_pane_wedge payload")?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&bootstrap.project_root)?;
    agent_doc_sqlite::state_store::clear_document_runtime_state_in_db(
        &conn,
        &payload.document_hash,
        OWNER_PANE_WEDGE_STATE_KIND,
    )?;
    Ok(true)
}

fn controller_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// Command-plane service for a `closeout_owner_release`. Decodes the submit and
/// runs the shared release core from the live projection; returns whether the
/// caller still owned (and so released) the cycle.
fn service_closeout_owner_release(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    submit: &lazily::CommandSubmit,
) -> Result<bool> {
    use super::command_plane::decode_closeout_owner_release_payload;
    let payload = decode_closeout_owner_release_payload(submit)?;
    let file = std::path::PathBuf::from(&payload.document_path);
    run_closeout_owner_release(
        bootstrap,
        runtime,
        &file,
        CloseoutOwnerReleaseRequest {
            cycle_id: payload.cycle_id,
            owner_id: payload.owner_id,
            reason: payload.reason,
            released_secs: payload.released_secs,
        },
    )
}

fn route_submit_event_id(kind: &str, document_hash: &str, submit_epoch: u64) -> String {
    format!("route-submit-{kind}-{document_hash}-{submit_epoch}")
}

fn queue_context_clear_event_id(kind: &str, document_hash: &str, clear_epoch: u64) -> String {
    format!("queue-context-clear-{kind}-{document_hash}-{clear_epoch}")
}

fn queue_context_clear_request_fields(
    request: &ControllerRequest,
) -> Result<(PathBuf, String, String, String, QueueContextClearPayload)> {
    let file = request_file(request)?;
    let target = request_string(&request.pane_id, "pane_id")?;
    let harness = request
        .command_kind
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let source = request
        .reason
        .clone()
        .unwrap_or_else(|| "context_clear".to_string());
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: QueueContextClearPayload =
        serde_json::from_str(&payload_json).context("parse queue context-clear payload")?;
    Ok((file, target, harness, source, payload))
}

fn queue_context_clear_projection(
    runtime: &ControllerRuntime,
    file: &Path,
) -> Result<agent_doc_state_backbone::QueueContextClearProjection> {
    let document_hash = agent_doc_hash::document_id_for_path(file);
    Ok(runtime
        .document_state_projection(&document_hash)?
        .map(|projection| projection.queue.context_clear)
        .unwrap_or_default())
}

pub(crate) fn handle_queue_context_clear_started(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::QueueContextClearProjection> {
    let (file, target, harness, source, payload) = queue_context_clear_request_fields(&request)?;
    let document_hash = agent_doc_hash::document_id_for_path(&file);
    let current = queue_context_clear_projection(runtime, &file)?;
    let clear_epoch = current.clear_epoch.saturating_add(1);
    let event = agent_doc_state_backbone::StateEvent::new(
        queue_context_clear_event_id("started", &document_hash, clear_epoch),
        agent_doc_state_backbone::StateFact::QueueContextClearStarted {
            document_hash,
            file: file.to_string_lossy().into_owned(),
            target: target.clone(),
            harness: harness.clone(),
            command: payload.command.clone(),
            source: Some(source.clone()),
            head_sha256: payload.head_sha256.clone(),
            head_bytes: payload.head_bytes,
            clear_epoch,
            marked_secs: timestamp_secs(),
        },
    );
    append_apply_state_event(bootstrap, runtime, event)?;
    let projection = queue_context_clear_projection(runtime, &file)?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "queue_context_clear_projection_started file={} target={} harness={} source={} clear_epoch={} phase={:?}",
            file.display(),
            target,
            harness,
            source,
            projection.clear_epoch,
            projection.phase
        ),
    );
    Ok(projection)
}

pub(crate) fn handle_queue_context_clear_deferred(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::QueueContextClearProjection> {
    let (file, target, harness, source, payload) = queue_context_clear_request_fields(&request)?;
    let document_hash = agent_doc_hash::document_id_for_path(&file);
    let current = queue_context_clear_projection(runtime, &file)?;
    let clear_epoch = current.clear_epoch.saturating_add(1);
    let event = agent_doc_state_backbone::StateEvent::new(
        queue_context_clear_event_id("deferred", &document_hash, clear_epoch),
        agent_doc_state_backbone::StateFact::QueueContextClearDeferred {
            document_hash,
            file: file.to_string_lossy().into_owned(),
            target: target.clone(),
            harness: harness.clone(),
            command: payload.command.clone(),
            source: Some(source.clone()),
            head_sha256: payload.head_sha256.clone(),
            head_bytes: payload.head_bytes,
            clear_epoch,
            marked_secs: timestamp_secs(),
        },
    );
    append_apply_state_event(bootstrap, runtime, event)?;
    let projection = queue_context_clear_projection(runtime, &file)?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "queue_context_clear_projection_deferred file={} target={} harness={} source={} clear_epoch={} phase={:?}",
            file.display(),
            target,
            harness,
            source,
            projection.clear_epoch,
            projection.phase
        ),
    );
    Ok(projection)
}

pub(crate) fn handle_queue_context_clear_settled(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::QueueContextClearProjection> {
    let (file, target, harness, source, payload) = queue_context_clear_request_fields(&request)?;
    let document_hash = agent_doc_hash::document_id_for_path(&file);
    let clear_epoch = request.generation.unwrap_or_else(|| {
        queue_context_clear_projection(runtime, &file).map_or(0, |p| p.clear_epoch)
    });
    let event = agent_doc_state_backbone::StateEvent::new(
        queue_context_clear_event_id("settled", &document_hash, clear_epoch),
        agent_doc_state_backbone::StateFact::QueueContextClearSettled {
            document_hash,
            file: file.to_string_lossy().into_owned(),
            target: target.clone(),
            harness: harness.clone(),
            command: payload.command.clone(),
            source: Some(source.clone()),
            clear_epoch,
            marked_secs: timestamp_secs(),
        },
    );
    append_apply_state_event(bootstrap, runtime, event)?;
    let projection = queue_context_clear_projection(runtime, &file)?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "queue_context_clear_projection_settled file={} target={} harness={} source={} clear_epoch={} phase={:?}",
            file.display(),
            target,
            harness,
            source,
            clear_epoch,
            projection.phase
        ),
    );
    Ok(projection)
}

pub(crate) fn handle_queue_context_clear_status(
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::QueueContextClearProjection> {
    let file = request_file(&request)?;
    queue_context_clear_projection(runtime, &file)
}

fn queue_drain_stall_event_id(kind: &str, document_hash: &str, stall_epoch: u64) -> String {
    format!("queue-drain-stall-{kind}-{document_hash}-{stall_epoch}")
}

fn queue_drain_stall_payload(request: &ControllerRequest) -> Result<QueueDrainStallPayload> {
    match request.diagnostic_payload.as_deref() {
        Some(payload_json) => {
            serde_json::from_str(payload_json).context("parse queue drain-stall payload")
        }
        None => Ok(QueueDrainStallPayload { cycle_id: None }),
    }
}

fn queue_drain_stall_projection(
    runtime: &ControllerRuntime,
    file: &Path,
) -> Result<agent_doc_state_backbone::QueueDrainStallProjection> {
    let document_hash = agent_doc_hash::document_id_for_path(file);
    Ok(runtime
        .document_state_projection(&document_hash)?
        .map(|projection| projection.queue.drain_stall)
        .unwrap_or_default())
}

pub(crate) fn handle_queue_drain_stall_continuation_recorded(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::QueueDrainStallProjection> {
    let file = request_file(&request)?;
    let payload = queue_drain_stall_payload(&request)?;
    let cycle_id = payload
        .cycle_id
        .with_context(|| "queue drain-stall record missing cycle_id")?;
    let document_hash = agent_doc_hash::document_id_for_path(&file);
    let current = queue_drain_stall_projection(runtime, &file)?;
    let stall_epoch = current.stall_epoch.saturating_add(1);
    let event = agent_doc_state_backbone::StateEvent::new(
        queue_drain_stall_event_id("recorded", &document_hash, stall_epoch),
        agent_doc_state_backbone::StateFact::QueueDrainStallContinuationRecorded {
            document_hash,
            file: file.to_string_lossy().into_owned(),
            cycle_id: cycle_id.clone(),
            stall_epoch,
            recorded_secs: timestamp_secs(),
        },
    );
    append_apply_state_event(bootstrap, runtime, event)?;
    let projection = queue_drain_stall_projection(runtime, &file)?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "queue_drain_stall_continuation_recorded file={} cycle_id={} stall_epoch={} phase={:?}",
            file.display(),
            cycle_id,
            projection.stall_epoch,
            projection.phase
        ),
    );
    Ok(projection)
}

pub(crate) fn handle_queue_drain_stall_continuation_cleared(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::QueueDrainStallProjection> {
    let file = request_file(&request)?;
    let document_hash = agent_doc_hash::document_id_for_path(&file);
    let stall_epoch = request.generation.unwrap_or_else(|| {
        queue_drain_stall_projection(runtime, &file).map_or(0, |p| p.stall_epoch)
    });
    let reason = request
        .reason
        .clone()
        .unwrap_or_else(|| "reconciled".to_string());
    let event = agent_doc_state_backbone::StateEvent::new(
        queue_drain_stall_event_id("cleared", &document_hash, stall_epoch),
        agent_doc_state_backbone::StateFact::QueueDrainStallContinuationCleared {
            document_hash,
            file: file.to_string_lossy().into_owned(),
            stall_epoch,
            cleared_secs: timestamp_secs(),
            reason: Some(reason.clone()),
        },
    );
    append_apply_state_event(bootstrap, runtime, event)?;
    let projection = queue_drain_stall_projection(runtime, &file)?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "queue_drain_stall_continuation_cleared file={} reason={} stall_epoch={} phase={:?}",
            file.display(),
            reason,
            stall_epoch,
            projection.phase
        ),
    );
    Ok(projection)
}

pub(crate) fn handle_queue_drain_stall_status(
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::QueueDrainStallProjection> {
    let file = request_file(&request)?;
    queue_drain_stall_projection(runtime, &file)
}

fn route_submit_request_fields(
    request: &ControllerRequest,
) -> Result<(PathBuf, String, String, String)> {
    let file = request_file(request)?;
    let pane = request_string(&request.pane_id, "pane_id")?;
    let harness = request
        .command_kind
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let reason = request
        .reason
        .clone()
        .unwrap_or_else(|| "route_submit".to_string());
    Ok((file, pane, harness, reason))
}

fn route_submit_projection(
    runtime: &ControllerRuntime,
    file: &Path,
) -> Result<agent_doc_state_backbone::RouteSubmitProjection> {
    let document_hash = agent_doc_hash::document_id_for_path(file);
    Ok(runtime
        .document_state_projection(&document_hash)?
        .map(|projection| projection.route.submit)
        .unwrap_or_default())
}

pub(crate) fn handle_route_submit_started(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::RouteSubmitProjection> {
    let (file, pane, harness, reason) = route_submit_request_fields(&request)?;
    let document_hash = agent_doc_hash::document_id_for_path(&file);
    let current = route_submit_projection(runtime, &file)?;
    let submit_epoch = current.submit_epoch.saturating_add(1);
    let event = agent_doc_state_backbone::StateEvent::new(
        route_submit_event_id("started", &document_hash, submit_epoch),
        agent_doc_state_backbone::StateFact::RouteSubmitStarted {
            document_hash: document_hash.clone(),
            pane_id: pane.clone(),
            harness: harness.clone(),
            reason: reason.clone(),
            submit_epoch,
            marked_secs: timestamp_secs(),
        },
    );
    append_apply_state_event(bootstrap, runtime, event)?;
    let projection = route_submit_projection(runtime, &file)?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "route_submit_projection_started file={} pane={} harness={} reason={} submit_epoch={} phase={:?}",
            file.display(),
            pane,
            harness,
            reason,
            projection.submit_epoch,
            projection.phase
        ),
    );
    Ok(projection)
}

pub(crate) fn handle_route_submit_settled(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::RouteSubmitProjection> {
    let (file, pane, harness, reason) = route_submit_request_fields(&request)?;
    let document_hash = agent_doc_hash::document_id_for_path(&file);
    let submit_epoch = request
        .generation
        .unwrap_or_else(|| route_submit_projection(runtime, &file).map_or(0, |p| p.submit_epoch));
    let event = agent_doc_state_backbone::StateEvent::new(
        route_submit_event_id("settled", &document_hash, submit_epoch),
        agent_doc_state_backbone::StateFact::RouteSubmitSettled {
            document_hash,
            pane_id: pane.clone(),
            harness: harness.clone(),
            reason: reason.clone(),
            submit_epoch,
            marked_secs: timestamp_secs(),
        },
    );
    append_apply_state_event(bootstrap, runtime, event)?;
    let projection = route_submit_projection(runtime, &file)?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "route_submit_projection_settled file={} pane={} harness={} reason={} submit_epoch={} phase={:?}",
            file.display(),
            pane,
            harness,
            reason,
            submit_epoch,
            projection.phase
        ),
    );
    Ok(projection)
}

pub(crate) fn handle_route_submit_blocked(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::RouteSubmitProjection> {
    let (file, pane, harness, reason) = route_submit_request_fields(&request)?;
    let document_hash = agent_doc_hash::document_id_for_path(&file);
    let current = route_submit_projection(runtime, &file)?;
    let submit_epoch = current.submit_epoch.saturating_add(1);
    let event = agent_doc_state_backbone::StateEvent::new(
        route_submit_event_id("blocked", &document_hash, submit_epoch),
        agent_doc_state_backbone::StateFact::RouteSubmitBlocked {
            document_hash,
            pane_id: pane.clone(),
            harness: harness.clone(),
            reason: reason.clone(),
            submit_epoch,
            marked_secs: timestamp_secs(),
        },
    );
    append_apply_state_event(bootstrap, runtime, event)?;
    let projection = route_submit_projection(runtime, &file)?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "route_submit_projection_blocked file={} pane={} harness={} reason={} submit_epoch={} phase={:?}",
            file.display(),
            pane,
            harness,
            reason,
            projection.submit_epoch,
            projection.phase
        ),
    );
    Ok(projection)
}

pub(crate) fn handle_route_submit_status(
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::RouteSubmitProjection> {
    let file = request_file(&request)?;
    route_submit_projection(runtime, &file)
}

fn supervisor_recycle_event_id(kind: &str, recycle_epoch: u64) -> String {
    format!("project-supervisor-recycle-{kind}-{recycle_epoch}")
}

fn visible_write_event_id(
    kind: &str,
    document_hash: &str,
    patch_id: &str,
    candidate_hash: &str,
) -> String {
    format!("visible-write-{kind}-{document_hash}-{patch_id}-{candidate_hash}")
}

fn visible_write_commit_candidate_events(
    document_hash: &str,
    payload: &VisibleWriteCommitCandidatePayload,
) -> (
    agent_doc_state_backbone::StateEvent,
    agent_doc_state_backbone::StateEvent,
    agent_doc_state_backbone::StateEvent,
) {
    let generation_event = agent_doc_state_backbone::StateEvent::new(
        visible_write_event_id(
            "generation",
            document_hash,
            &payload.patch_id,
            &payload.model_revision.to_string(),
        ),
        agent_doc_state_backbone::StateFact::OwnerGenerationChanged {
            document_hash: document_hash.to_string(),
            owner: agent_doc_state_backbone::StateOwner::EditorIpcBridge,
            generation: payload.model_revision,
        },
    );
    let applied_event = agent_doc_state_backbone::StateEvent::new(
        visible_write_event_id(
            "applied",
            document_hash,
            &payload.patch_id,
            &format!(
                "{}-{}",
                payload.commit_candidate_hash, payload.model_revision
            ),
        ),
        agent_doc_state_backbone::StateFact::EditorPatchApplied {
            document_hash: document_hash.to_string(),
            patch_id: payload.patch_id.clone(),
            actor_generation: payload.model_revision,
        },
    );
    let proof_event = agent_doc_state_backbone::StateEvent::new(
        visible_write_event_id(
            "candidate",
            document_hash,
            &payload.patch_id,
            &format!(
                "{}-{}",
                payload.commit_candidate_hash, payload.model_revision
            ),
        ),
        agent_doc_state_backbone::StateFact::VisibleWriteCommitCandidateObserved {
            document_hash: document_hash.to_string(),
            patch_id: payload.patch_id.clone(),
            model_revision: payload.model_revision,
            editor_visible_hash: payload.editor_visible_hash.clone(),
            commit_candidate_hash: payload.commit_candidate_hash.clone(),
            commit_candidate_content: Some(payload.commit_candidate_content.clone()),
            source: payload.source.clone(),
        },
    );
    (generation_event, applied_event, proof_event)
}

fn record_visible_write_commit_candidate_direct(
    project_root: &Path,
    canonical: &Path,
    document_hash: &str,
    payload: &VisibleWriteCommitCandidatePayload,
    controller_err: &anyhow::Error,
) -> Result<agent_doc_state_backbone::VisibleWriteCommitCandidateProjection> {
    let (generation_event, applied_event, proof_event) =
        visible_write_commit_candidate_events(document_hash, payload);
    let generation_inserted = append_state_event(project_root, &generation_event)?;
    let applied_inserted = append_state_event(project_root, &applied_event)?;
    let candidate_inserted = append_state_event(project_root, &proof_event)?;
    let projection = load_state_backbone_projection(project_root)?;
    let proof = visible_write_commit_candidate_from_projection(
        &projection,
        canonical,
        &payload.commit_candidate_hash,
    )
    .with_context(|| {
        format!(
            "durable visible write event did not fold for {} candidate={}",
            canonical.display(),
            payload.commit_candidate_hash
        )
    })?;
    agent_doc_ops_log_io::log_op(
        canonical,
        &format!(
            "visible_write_commit_candidate_durable_event_recorded file={} patch_id={} model_revision={} commit_candidate_hash={} source={} generation_inserted={} applied_inserted={} candidate_inserted={} authority=state_backbone recovery=controller_reconcile controller_error={}",
            canonical.display(),
            payload.patch_id,
            proof.model_revision,
            proof.commit_candidate_hash,
            proof.source,
            generation_inserted,
            applied_inserted,
            candidate_inserted,
            compact_controller_error(controller_err)
        ),
    );
    Ok(proof)
}

fn visible_write_commit_candidate_from_projection(
    projection: &agent_doc_state_backbone::StateBackboneProjection,
    file: &Path,
    commit_candidate_hash: &str,
) -> Option<agent_doc_state_backbone::VisibleWriteCommitCandidateProjection> {
    let document_hash = agent_doc_hash::document_id_for_path(file);
    projection
        .document(&document_hash)
        .and_then(|document| document.applied_visible_write_candidate(commit_candidate_hash))
        .cloned()
}

fn visible_write_commit_candidate_for_patch_from_projection(
    projection: &agent_doc_state_backbone::StateBackboneProjection,
    file: &Path,
    patch_id: &str,
) -> Option<agent_doc_state_backbone::VisibleWriteCommitCandidateProjection> {
    let document_hash = agent_doc_hash::document_id_for_path(file);
    projection
        .document(&document_hash)
        .and_then(|document| document.applied_visible_write_candidate_for_patch(patch_id))
        .cloned()
}

fn visible_write_materialized_carry_forward_from_projection(
    projection: &agent_doc_state_backbone::StateBackboneProjection,
    file: &Path,
    commit_candidate_hash: &str,
    file_content_hash: &str,
    live_buffer_hash: &str,
) -> Option<agent_doc_state_backbone::VisibleWriteMaterializedCarryForwardProjection> {
    let document_hash = agent_doc_hash::document_id_for_path(file);
    projection
        .document(&document_hash)
        .and_then(|document| {
            document.materialized_visible_write_carry_forward(
                commit_candidate_hash,
                file_content_hash,
                live_buffer_hash,
            )
        })
        .cloned()
}

fn append_apply_state_event(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    event: agent_doc_state_backbone::StateEvent,
) -> Result<bool> {
    let inserted = append_state_event(&bootstrap.project_root, &event)?;
    runtime.apply_state_event(&event)?;
    Ok(inserted)
}

/// Controller authority for a `command_plane_submit` request: route the lazily
/// [`CommandSubmit`] envelope by `(namespace, name)` to the domain service, run
/// the transition from the live Lazily projection, and return the terminal
/// [`lazily::CausalReceipt`]. This is the server half of the live
/// `CommandTransport` (`#lzdurablesink`, `command-plane-v1`): the wire contract
/// is `CommandSubmit` → terminal `CausalReceipt`, never a transport ACK.
/// Counterpart to [`super::command_plane::ControllerCommandTransport`].
fn handle_command_plane_submit(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<serde_json::Value> {
    use super::command_plane::NAMESPACE;
    // The `CommandSubmit` envelope rides `diagnostic_payload` — the same channel
    // `closeout_owner_claim`/`release` use for serialized structured payloads —
    // so the shared `ControllerRequest` shape is unchanged.
    let submit_json = request.diagnostic_payload.ok_or_else(|| {
        anyhow::anyhow!(
            "command_plane_submit requires a CommandSubmit envelope in diagnostic_payload"
        )
    })?;
    let submit: lazily::CommandSubmit =
        serde_json::from_str(&submit_json).context("decode command_plane_submit CommandSubmit")?;
    // The controller is the `agent-doc` namespace authority; refuse a foreign
    // namespace rather than silently routing an unknown payload schema.
    if submit.namespace != NAMESPACE {
        anyhow::bail!(
            "command_plane_submit refuses foreign namespace {:?} (want {NAMESPACE:?})",
            submit.namespace
        );
    }
    dispatch_command_plane_submit(bootstrap, runtime, &submit)
}

/// Route a command-plane submit to its domain service by `(namespace, name)`.
/// The response is the command's authority result, serialized as JSON:
/// - `closeout_advance` → a terminal [`lazily::CausalReceipt`] (transition
///   authority; Applied/Rejected).
/// - `closeout_owner_claim` → the typed `CloseoutOwnerClaimOutcome`
///   (coordination CAS result: Acquired/HeldByOther/CycleSuperseded).
/// - `closeout_owner_release` → `bool` (released / not-owner).
///
/// An unknown name fails closed as a terminal `rejected` receipt (with the
/// command id) so the client resolves instead of hanging on a non-terminal ACK.
fn dispatch_command_plane_submit(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    submit: &lazily::CommandSubmit,
) -> Result<serde_json::Value> {
    use super::command_plane::{
        CLOSEOUT_ADVANCE_NAME, CLOSEOUT_OWNER_CLAIM_NAME, CLOSEOUT_OWNER_RELEASE_NAME,
        CONTROLLER_TARGET, NAMESPACE,
    };
    let (ns, name) = (submit.namespace.as_str(), submit.name.as_str());
    // Compare by `==` (not const patterns): a `&str` const is not a structural
    // pattern, so matching on the const identifier would bind instead of compare.
    if ns == NAMESPACE && name == CLOSEOUT_ADVANCE_NAME {
        Ok(serde_json::to_value(service_closeout_advance(
            bootstrap, runtime, submit,
        ))?)
    } else if ns == NAMESPACE && name == CLOSEOUT_OWNER_CLAIM_NAME {
        Ok(serde_json::to_value(service_closeout_owner_claim(
            bootstrap, runtime, submit,
        )?)?)
    } else if ns == NAMESPACE && name == CLOSEOUT_OWNER_RELEASE_NAME {
        Ok(serde_json::to_value(service_closeout_owner_release(
            bootstrap, runtime, submit,
        )?)?)
    } else {
        Ok(serde_json::to_value(
            lazily::CausalReceipt::rejected(
                format!("{}:rcpt", submit.command_id),
                &submit.command_id,
                CONTROLLER_TARGET,
                submit.authority_generation,
            )
            .with_reason(format!(
                "command_plane_submit: unknown command name {name:?} in namespace {ns:?}"
            )),
        )?)
    }
}

/// Authority-side service for a `closeout_advance` command (`#lzdurablesink`).
/// Decodes the [`lazily::CommandSubmit`], decides the phase transition from the
/// live Lazily projection (no `state.db` replay), emits the phase fact(s) as the
/// durable sink via [`append_apply_state_event`], and returns the terminal
/// [`lazily::CausalReceipt`] — the command's terminal authority, never a
/// transport ACK. Counterpart to
/// [`super::command_plane::build_closeout_advance_submit`]; reachable from a
/// client through [`handle_command_plane_submit`] / the live
/// [`super::command_plane::ControllerCommandTransport`].
///
/// Every command resolves to a receipt: a decode failure, an unrouted event, or
/// a sink error is a `rejected` receipt (fail closed); an applied transition (or
/// an idempotent no-op at the requested phase) is `applied`.
pub(crate) fn service_closeout_advance(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    submit: &lazily::CommandSubmit,
) -> lazily::CausalReceipt {
    use super::command_plane::{CONTROLLER_TARGET, closeout_advance_receipt};
    let outcome = closeout_advance_outcome(bootstrap, runtime, submit);
    closeout_advance_receipt(
        submit,
        format!("{}:rcpt", submit.command_id),
        CONTROLLER_TARGET,
        submit.authority_generation,
        outcome,
    )
}

fn closeout_advance_outcome(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    submit: &lazily::CommandSubmit,
) -> Result<(), String> {
    use super::command_plane::{CloseoutPhaseEvent, decode_closeout_advance_payload};
    let payload = decode_closeout_advance_payload(submit).map_err(|e| format!("{e:#}"))?;
    let file = std::path::PathBuf::from(&payload.document_path);
    let document_hash = agent_doc_hash::document_id_for_path(&file);
    let document = runtime
        .document_state_projection(&document_hash)
        .map_err(|e| format!("{e:#}"))?;
    let current = match document.as_ref() {
        Some(d) => {
            agent_doc_cycle_state_io::reconstruct_cycle_state(d).map_err(|e| format!("{e:#}"))?
        }
        None => None,
    };
    // The next checkpoint sequence is the last recorded one + 1 (0 when none).
    let checkpoint_sequence = document
        .as_ref()
        .and_then(|d| d.closeout.turn_intent_checkpoint.as_ref())
        .map(|c| c.checkpoint_sequence.saturating_add(1))
        .unwrap_or(0);

    let event_label = payload.last_event_label();
    use agent_doc_cycle_state_io::CloseoutProjectionEvent;
    // Normalize every transition to `(state, checkpoint, phase-facts)` so the sink
    // loop is uniform. An empty decision (no checkpoint, no facts) is an idempotent
    // no-op that folds as `applied`.
    let (state, checkpoint, facts): (
        agent_doc_cycle_state_io::CycleState,
        bool,
        Vec<CloseoutProjectionEvent>,
    ) = match payload.event {
        CloseoutPhaseEvent::WriteApplied => {
            let (state, transitioned) = agent_doc_cycle_state_io::decide_write_applied(
                current,
                &file,
                &event_label,
                payload.snapshot_content.as_deref(),
                payload.file_content.as_deref(),
            );
            (
                state,
                transitioned,
                if transitioned {
                    vec![CloseoutProjectionEvent::WriteApplied]
                } else {
                    Vec::new()
                },
            )
        }
        CloseoutPhaseEvent::ResponseCaptured => {
            let Some(response_sha256) = payload.response_sha256.as_deref() else {
                return Err(
                    "closeout_advance: ResponseCaptured requires response_sha256".to_string(),
                );
            };
            let (state, transitioned) = agent_doc_cycle_state_io::decide_response_captured(
                current,
                &file,
                &event_label,
                payload.snapshot_content.as_deref(),
                payload.file_content.as_deref(),
                response_sha256,
                payload.cycle_id_hint.as_deref(),
            );
            (
                state,
                transitioned,
                if transitioned {
                    vec![CloseoutProjectionEvent::ResponseCaptured]
                } else {
                    Vec::new()
                },
            )
        }
        CloseoutPhaseEvent::Committed(_) => {
            let decision = agent_doc_cycle_state_io::decide_committed(
                current,
                &file,
                &event_label,
                payload.snapshot_content.as_deref(),
                payload.file_content.as_deref(),
            );
            (decision.state, decision.checkpoint, decision.facts)
        }
        CloseoutPhaseEvent::Abandoned => {
            let decision = agent_doc_cycle_state_io::decide_abandoned(
                current,
                &file,
                &event_label,
                payload.snapshot_content.as_deref(),
                payload.file_content.as_deref(),
            );
            (decision.state, decision.checkpoint, decision.facts)
        }
    };

    // An idempotent no-op folds as `applied`: the requested end-state already
    // holds and the sink emits nothing.
    if !checkpoint && facts.is_empty() {
        return Ok(());
    }

    if checkpoint {
        let checkpoint_event = agent_doc_cycle_state_io::build_turn_intent_checkpoint_event(
            &document_hash,
            checkpoint_sequence,
            &state,
        )
        .map_err(|e| format!("{e:#}"))?;
        append_apply_state_event(bootstrap, runtime, checkpoint_event)
            .map_err(|e| format!("{e:#}"))?;
    }
    for fact in &facts {
        if let Some(phase_event) =
            agent_doc_cycle_state_io::build_closeout_projection_event(&document_hash, &state, *fact)
        {
            append_apply_state_event(bootstrap, runtime, phase_event)
                .map_err(|e| format!("{e:#}"))?;
        }
    }
    Ok(())
}

fn append_and_apply_state_event(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    event: agent_doc_state_backbone::StateEvent,
) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
    append_apply_state_event(bootstrap, runtime, event)?;
    runtime.supervisor_recycle_projection()
}

pub(crate) fn handle_supervisor_recycle_requested(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
    let current = runtime.supervisor_recycle_projection()?;
    // The statechart only arms `Requested` from `Settled`; a request that races an
    // already-requested or in-flight recycle is a no-op (idempotent), so don't burn
    // an epoch or rewrite the reason.
    if !matches!(
        current.phase,
        agent_doc_state_backbone::SupervisorRecyclePhase::Settled
    ) {
        return Ok(current);
    }
    let recycle_epoch = current.recycle_epoch.saturating_add(1);
    let reason = request
        .reason
        .as_deref()
        .unwrap_or("supervisor_recycle_requested");
    let event = agent_doc_state_backbone::StateEvent::new(
        supervisor_recycle_event_id("requested", recycle_epoch),
        agent_doc_state_backbone::StateFact::SupervisorRecycleRequested {
            document_hash: agent_doc_state_backbone::PROJECT_SUPERVISOR_DOCUMENT_HASH.to_string(),
            reason: reason.to_string(),
            recycle_epoch,
            marked_secs: timestamp_secs(),
        },
    );
    let projection = append_and_apply_state_event(bootstrap, runtime, event)?;
    agent_doc_ops_log_io::log_op(
        &bootstrap.project_root,
        &format!(
            "supervisor_recycle_graph_requested reason={} recycle_epoch={} phase={:?}",
            reason, projection.recycle_epoch, projection.phase
        ),
    );
    Ok(projection)
}

pub(crate) fn handle_supervisor_recycle_started(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
    let current = runtime.supervisor_recycle_projection()?;
    let recycle_epoch = current.recycle_epoch.saturating_add(1);
    let reason = request
        .reason
        .as_deref()
        .unwrap_or("supervisor_recycle_started");
    let event = agent_doc_state_backbone::StateEvent::new(
        supervisor_recycle_event_id("started", recycle_epoch),
        agent_doc_state_backbone::StateFact::SupervisorRecycleStarted {
            document_hash: agent_doc_state_backbone::PROJECT_SUPERVISOR_DOCUMENT_HASH.to_string(),
            reason: reason.to_string(),
            recycle_epoch,
            marked_secs: timestamp_secs(),
        },
    );
    let projection = append_and_apply_state_event(bootstrap, runtime, event)?;
    agent_doc_ops_log_io::log_op(
        &bootstrap.project_root,
        &format!(
            "supervisor_recycle_graph_started reason={} recycle_epoch={} phase={:?}",
            reason, projection.recycle_epoch, projection.phase
        ),
    );
    Ok(projection)
}

pub(crate) fn handle_supervisor_recycle_settled(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
    let current = runtime.supervisor_recycle_projection()?;
    if matches!(
        current.phase,
        agent_doc_state_backbone::SupervisorRecyclePhase::Settled
    ) {
        return Ok(current);
    }
    let recycle_epoch = current.recycle_epoch;
    let reason = request
        .reason
        .as_deref()
        .unwrap_or("supervisor_recycle_settled");
    let event = agent_doc_state_backbone::StateEvent::new(
        supervisor_recycle_event_id("settled", recycle_epoch),
        agent_doc_state_backbone::StateFact::SupervisorRecycleSettled {
            document_hash: agent_doc_state_backbone::PROJECT_SUPERVISOR_DOCUMENT_HASH.to_string(),
            reason: reason.to_string(),
            recycle_epoch,
            marked_secs: timestamp_secs(),
        },
    );
    let projection = append_and_apply_state_event(bootstrap, runtime, event)?;
    agent_doc_ops_log_io::log_op(
        &bootstrap.project_root,
        &format!(
            "supervisor_recycle_graph_settled reason={} recycle_epoch={} phase={:?}",
            reason, projection.recycle_epoch, projection.phase
        ),
    );
    Ok(projection)
}

pub(crate) fn handle_visible_write_commit_candidate_observed(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::VisibleWriteCommitCandidateProjection> {
    let file = request_file(&request)?;
    let canonical = file.canonicalize().unwrap_or(file);
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: VisibleWriteCommitCandidatePayload = serde_json::from_str(&payload_json)
        .context("parse visible write commit candidate payload")?;
    let (generation_event, applied_event, proof_event) =
        visible_write_commit_candidate_events(&document_hash, &payload);
    append_apply_state_event(bootstrap, runtime, generation_event)?;
    append_apply_state_event(bootstrap, runtime, applied_event)?;
    append_apply_state_event(bootstrap, runtime, proof_event)?;
    let projection = runtime
        .document_state_projection(&document_hash)?
        .and_then(|document| {
            document
                .applied_visible_write_candidate(&payload.commit_candidate_hash)
                .cloned()
        })
        .with_context(|| {
            format!(
                "visible write commit candidate proof did not fold for {} candidate={}",
                canonical.display(),
                payload.commit_candidate_hash
            )
        })?;
    agent_doc_ops_log_io::log_op(
        &canonical,
        &format!(
            "visible_write_commit_candidate_observed file={} patch_id={} model_revision={} commit_candidate_hash={} source={}",
            canonical.display(),
            projection.patch_id,
            projection.model_revision,
            projection.commit_candidate_hash,
            projection.source
        ),
    );
    Ok(projection)
}

pub(crate) fn handle_visible_write_commit_candidate_status(
    _bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<VisibleWriteCommitCandidateStatus> {
    let file = request_file(&request)?;
    let canonical = file.canonicalize().unwrap_or(file);
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: serde_json::Value = serde_json::from_str(&payload_json)
        .context("parse visible write candidate status payload")?;
    let commit_candidate_hash = payload
        .get("commit_candidate_hash")
        .and_then(|value| value.as_str())
        .context("visible write candidate status missing commit_candidate_hash")?;
    Ok(VisibleWriteCommitCandidateStatus {
        proof: runtime
            .document_state_projection(&document_hash)?
            .and_then(|document| {
                document
                    .applied_visible_write_candidate(commit_candidate_hash)
                    .cloned()
            }),
    })
}

pub(crate) fn handle_visible_write_commit_candidate_patch_status(
    _bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<VisibleWriteCommitCandidatePatchStatus> {
    let file = request_file(&request)?;
    let canonical = file.canonicalize().unwrap_or(file);
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: serde_json::Value = serde_json::from_str(&payload_json)
        .context("parse visible write candidate patch status payload")?;
    let patch_id = payload
        .get("patch_id")
        .and_then(|value| value.as_str())
        .context("visible write candidate patch status missing patch_id")?;
    Ok(VisibleWriteCommitCandidatePatchStatus {
        proof: runtime
            .document_state_projection(&document_hash)?
            .and_then(|document| {
                document
                    .applied_visible_write_candidate_for_patch(patch_id)
                    .cloned()
            }),
    })
}

/// `#lazily-hot-path` W1 — bounded server-side await for a visible-write receipt.
///
/// The client supplies its own real deadline (`wait_ms`); the convergence wait is
/// legitimately long (a slow controller/editor can take far more than the default
/// RPC budget), so this is clamped to the controller ceiling rather than a global
/// hang budget. The projection read stays authoritative inside the wait, so the
/// answer is identical to `visible_write_commit_candidate_patch_status` — only the
/// arrival is pushed instead of polled.
pub(crate) fn handle_visible_write_commit_candidate_patch_await(
    _bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<VisibleWriteCommitCandidatePatchStatus> {
    let file = request_file(&request)?;
    let canonical = file.canonicalize().unwrap_or(file);
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: serde_json::Value = serde_json::from_str(&payload_json)
        .context("parse visible write candidate patch await payload")?;
    let patch_id = payload
        .get("patch_id")
        .and_then(|value| value.as_str())
        .context("visible write candidate patch await missing patch_id")?;
    let wait = payload
        .get("wait_ms")
        .and_then(|value| value.as_u64())
        .map(Duration::from_millis)
        .unwrap_or(CONTROLLER_VISIBLE_WRITE_AWAIT_MAX)
        .min(CONTROLLER_VISIBLE_WRITE_AWAIT_MAX);
    Ok(VisibleWriteCommitCandidatePatchStatus {
        proof: runtime.wait_for_visible_write_commit_candidate_patch(
            &document_hash,
            patch_id,
            wait,
        ),
    })
}

/// `#ctrlkillreregister` Tier 3 — answer a peer's own missing-replica question.
///
/// The editor is the actor that can create its replica, but the converged liveness
/// plane lives here, so the editor asks *about itself* and repairs rather than being
/// pushed a rebuild request. That inversion is the whole point: a push has to reach
/// the editor (the `1/4 endpoints` failure), while a pull is driven by the side that
/// is definitely alive — it just asked.
///
/// `held` is the set of document hashes the caller already has a replica for, so the
/// answer is exactly what it still needs and an up-to-date editor gets an empty list.
pub(crate) fn handle_peer_replicas_missing(
    _bootstrap: &ControllerBootstrap,
    _runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<Vec<agent_doc_reliable_sync_io::liveness::EditorRegistration>> {
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: serde_json::Value =
        serde_json::from_str(&payload_json).context("parse peer_replicas_missing payload")?;
    let pid = payload
        .get("pid")
        .and_then(serde_json::Value::as_u64)
        .context("peer_replicas_missing missing pid")?;
    let held: BTreeSet<String> = payload
        .get("held")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Ok(controller_liveness_plane()
        .lock()
        .projection()
        .peer_registrations_missing_replica(pid, &held)
        .into_iter()
        .filter(|registration| !controller_serves_replica(&registration.path))
        .collect())
}

/// Whether **this controller process** can currently serve `path`'s replica.
///
/// The relay hub is a process-local static, so this is the only fact that
/// distinguishes "registered" from "registered and actually backed". The caller's
/// `held` set cannot answer it: an editor holding a forwarder it believes is live
/// says nothing about whether the hub on this side survived, and after a controller
/// kill that belief is exactly what is wrong. So the derivation subtracts what this
/// process can serve, and the peer's `held` only suppresses documents it has already
/// decided not to hear about.
///
/// Without this the pull returned every live registration in steady state, which
/// would have made an editor re-register perfectly healthy documents on every
/// startup — and returned nothing useful after a restart, since a stranded editor
/// still lists its stale forwarders as `held`.
fn controller_serves_replica(path: &str) -> bool {
    agent_doc_crdt_relay_io::embedded_relay_is_available_for_file(std::path::Path::new(path))
}

/// `#lazily-hot-path` Theme A — bounded server-side await for delivery convergence.
///
/// The relay hub is process-local, so only the process hosting it can observe
/// convergence; every CLI-side consumer (compact's commit-observe and CRDT-merge
/// retries) would otherwise re-derive a *proxy* for it by re-reading disk and the
/// snapshot on its own timer. This publishes the fact once, from the owner: a single
/// in-memory poll here replaces N filesystem re-reads out there (rubric #3).
///
/// The hub exposes a witness, not a notification, so this polls it — but in memory,
/// in one process, and it returns the instant convergence lands.
pub(crate) fn handle_delivery_convergence_await(
    _bootstrap: &ControllerBootstrap,
    _runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<DeliveryConvergenceStatus> {
    let file = request_file(&request)?;
    let canonical = file.canonicalize().unwrap_or(file);
    let payload = request
        .diagnostic_payload
        .as_deref()
        .and_then(|payload| serde_json::from_str::<serde_json::Value>(payload).ok());
    let wait = payload
        .as_ref()
        .and_then(|payload| payload.get("wait_ms").and_then(serde_json::Value::as_u64))
        .map(Duration::from_millis)
        .unwrap_or(CONTROLLER_DELIVERY_CONVERGENCE_AWAIT_MAX)
        .min(CONTROLLER_DELIVERY_CONVERGENCE_AWAIT_MAX);
    let after_version = payload.as_ref().and_then(|payload| {
        payload
            .get("after_version")
            .and_then(serde_json::Value::as_u64)
    });
    let recovery = payload
        .as_ref()
        .and_then(|payload| payload.get("recovery"))
        .cloned()
        .filter(|value| !value.is_null())
        .and_then(|value| serde_json::from_value::<DeliveryConvergenceRecovery>(value).ok());
    Ok(
        await_local_delivery_convergence_change_for_file(
            &canonical,
            after_version,
            wait,
            recovery,
        )?
        .unwrap_or(DeliveryConvergenceStatus {
            observed: false,
            converged: false,
            version: 0,
            recovery_signal_observed: false,
            force_refresh_sent: recovery.is_some_and(|recovery| recovery.force_refresh_sent),
        }),
    )
}

pub(crate) fn handle_visible_write_materialized_carry_forward_observed(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<agent_doc_state_backbone::VisibleWriteMaterializedCarryForwardProjection> {
    let file = request_file(&request)?;
    let canonical = file.canonicalize().unwrap_or(file);
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: VisibleWriteMaterializedCarryForwardPayload = serde_json::from_str(&payload_json)
        .context("parse visible write materialized carry-forward payload")?;
    let event = agent_doc_state_backbone::StateEvent::new(
        format!(
            "visible-write-materialized-carry-forward-{document_hash}-{}-{}-{}",
            payload.commit_candidate_hash, payload.file_content_hash, payload.live_buffer_hash
        ),
        agent_doc_state_backbone::StateFact::VisibleWriteMaterializedCarryForwardObserved {
            document_hash: document_hash.clone(),
            model_revision: payload.model_revision,
            live_buffer_hash: payload.live_buffer_hash.clone(),
            file_content_hash: payload.file_content_hash.clone(),
            commit_candidate_hash: payload.commit_candidate_hash.clone(),
            source: payload.source.clone(),
        },
    );
    append_apply_state_event(bootstrap, runtime, event)?;
    let projection = runtime
        .document_state_projection(&document_hash)?
        .and_then(|document| {
            document
                .materialized_visible_write_carry_forward(
                    &payload.commit_candidate_hash,
                    &payload.file_content_hash,
                    &payload.live_buffer_hash,
                )
                .cloned()
        })
        .with_context(|| {
            format!(
                "visible write materialized carry-forward proof did not fold for {} candidate={}",
                canonical.display(),
                payload.commit_candidate_hash
            )
        })?;
    agent_doc_ops_log_io::log_op(
        &canonical,
        &format!(
            "visible_write_materialized_carry_forward_observed file={} model_revision={} commit_candidate_hash={} file_content_hash={} live_buffer_hash={} source={}",
            canonical.display(),
            projection.model_revision,
            projection.commit_candidate_hash,
            projection.file_content_hash,
            projection.live_buffer_hash,
            projection.source
        ),
    );
    Ok(projection)
}

pub(crate) fn handle_visible_write_materialized_carry_forward_status(
    _bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<VisibleWriteMaterializedCarryForwardStatus> {
    let file = request_file(&request)?;
    let canonical = file.canonicalize().unwrap_or(file);
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let payload: serde_json::Value = serde_json::from_str(&payload_json)
        .context("parse visible write materialized carry-forward status payload")?;
    let commit_candidate_hash = payload
        .get("commit_candidate_hash")
        .and_then(|value| value.as_str())
        .context("visible write materialized carry-forward status missing commit_candidate_hash")?;
    let file_content_hash = payload
        .get("file_content_hash")
        .and_then(|value| value.as_str())
        .context("visible write materialized carry-forward status missing file_content_hash")?;
    let live_buffer_hash = payload
        .get("live_buffer_hash")
        .and_then(|value| value.as_str())
        .context("visible write materialized carry-forward status missing live_buffer_hash")?;
    Ok(VisibleWriteMaterializedCarryForwardStatus {
        proof: runtime
            .document_state_projection(&document_hash)?
            .and_then(|document| {
                document
                    .materialized_visible_write_carry_forward(
                        commit_candidate_hash,
                        file_content_hash,
                        live_buffer_hash,
                    )
                    .cloned()
            }),
    })
}

pub(crate) fn handle_start_session(
    bootstrap: &ControllerBootstrap,
    runtime: Option<&ControllerRuntime>,
    request: ControllerRequest,
) -> Result<agent_doc_sqlite::state_store::ActorRecord> {
    let file = request_file(&request)?;
    let session_id = request_string(&request.session_id, "session_id")?;
    let pane_id = request_string(&request.pane_id, "pane_id")?;
    let window_id = request_string(&request.window_id, "window_id")?;
    let generation = request_u64(request.generation, "generation")?;
    let document_id = agent_doc_session_actor_io::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let harness = agent_doc_session_actor_io::detect_document_harness_in(
        &bootstrap.project_root,
        &document_id,
    );
    let record = agent_doc_sqlite::state_store::ActorRecord {
        document_id: document_id.clone(),
        session_id: session_id.clone(),
        generation,
        pane_id: pane_id.clone(),
        window_id: window_id.clone(),
        harness,
        state: agent_doc_sqlite::state_store::ActorState::Starting,
        last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
            caller: "start".to_string(),
            reason: "session_start".to_string(),
            timestamp: timestamp_secs(),
            prior_generation: generation.saturating_sub(1),
            new_generation: generation,
        },
    };
    ensure_start_session_pane_not_claimed_by_other_actor(
        &bootstrap.project_root,
        &record.document_id,
        &record.session_id,
        &record.pane_id,
    )?;
    let record = store_actor_record(
        &bootstrap.project_root,
        Some(generation.saturating_sub(1)),
        &record,
    )
    .with_context(|| {
        format!(
            "controller failed to start session actor for {}",
            file.display()
        )
    })?;
    refresh_runtime_after_actor_write(runtime)?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "controller_session_start session={} pane={} generation={} state={}",
            session_id,
            pane_id,
            record.generation,
            record.state.as_str()
        ),
    );
    Ok(record)
}

fn ensure_start_session_pane_not_claimed_by_other_actor(
    project_root: &Path,
    document_id: &str,
    session_id: &str,
    pane_id: &str,
) -> Result<()> {
    if pane_id.is_empty() {
        return Ok(());
    }
    let store = load_actor_store(project_root)?;
    let registry = agent_doc_session_registry_io::load_in(project_root).with_context(|| {
        format!(
            "failed to load session registry while checking start_session pane alias for {pane_id}"
        )
    })?;
    let conn = open_state_db(project_root)?;
    let now = timestamp_secs();
    let mut stale_aliases = Vec::new();
    for existing in store.values().filter(|record| {
        record.pane_id == pane_id
            && record.state != agent_doc_sqlite::state_store::ActorState::Closed
    }) {
        if document_ids_equivalent(project_root, &existing.document_id, document_id) {
            continue;
        }
        if existing.session_id == session_id {
            agent_doc_ops_log_io::log_op(
                Path::new(document_id),
                &format!(
                    "controller_start_session_same_session_pane_alias_admitted document_id={} stale_document_id={} session={} generation={} pane={} state={}",
                    document_id,
                    existing.document_id,
                    existing.session_id,
                    existing.generation,
                    pane_id,
                    existing.state.as_str()
                ),
            );
            continue;
        }
        if start_session_cross_document_alias_has_live_claim(
            project_root,
            &conn,
            &registry,
            existing,
            now,
        )? {
            anyhow::bail!(
                "refusing start_session cross-document actor pane alias: pane {} is already claimed by {} session={} generation={} state={}",
                pane_id,
                existing.document_id,
                existing.session_id,
                existing.generation,
                existing.state.as_str()
            );
        }
        stale_aliases.push(existing.clone());
    }
    for existing in stale_aliases {
        close_stale_start_session_pane_alias(project_root, document_id, pane_id, &existing)?;
    }
    Ok(())
}

fn start_session_cross_document_alias_has_live_claim(
    project_root: &Path,
    conn: &Connection,
    registry: &tmux_router::Registry,
    record: &agent_doc_sqlite::state_store::ActorRecord,
    now: u64,
) -> Result<bool> {
    let lease = load_supervisor_lease_from_db(conn, &record.document_id, record.generation)?;
    if lease.as_ref().is_some_and(|lease| {
        status::supervisor_lease_is_fresh_and_alive(
            lease.last_heartbeat,
            lease.supervisor_pid.is_some_and(process_is_alive),
            now,
            SUPERVISOR_LEASE_GUARD_STALE_AFTER,
        )
    }) {
        return Ok(true);
    }
    Ok(registry.iter().any(|(key, entry)| {
        document_ids_equivalent(project_root, key, &record.document_id)
            && entry.session_id == record.session_id
            && entry.pane == record.pane_id
            && (record.window_id.is_empty()
                || entry.window.is_empty()
                || entry.window == record.window_id)
            && entry.pid != 0
            && process_is_alive(entry.pid)
    }))
}

fn close_stale_start_session_pane_alias(
    project_root: &Path,
    owner_document_id: &str,
    pane_id: &str,
    existing: &agent_doc_sqlite::state_store::ActorRecord,
) -> Result<()> {
    let mut closed = existing.clone();
    closed.state = agent_doc_sqlite::state_store::ActorState::Closed;
    closed.pane_id.clear();
    closed.window_id.clear();
    closed.last_transition = agent_doc_sqlite::state_store::ActorLastTransition {
        caller: "start".to_string(),
        reason: format!("stale_cross_document_pane_alias owner={owner_document_id} pane={pane_id}"),
        timestamp: timestamp_secs(),
        prior_generation: existing.generation,
        new_generation: existing.generation,
    };
    store_actor_record(project_root, Some(existing.generation), &closed).with_context(|| {
        format!(
            "failed to close stale start_session pane alias for {} on {}",
            existing.document_id, pane_id
        )
    })?;
    agent_doc_ops_log_io::log_op(
        Path::new(&closed.document_id),
        &format!(
            "controller_start_session_closed_stale_cross_document_pane_alias document_id={} session={} generation={} pane={} owner={}",
            closed.document_id, closed.session_id, closed.generation, pane_id, owner_document_id
        ),
    );
    Ok(())
}

fn document_ids_equivalent(project_root: &Path, left: &str, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    if left == right {
        return true;
    }
    let left = agent_doc_session_actor_io::canonical_document_id_in(project_root, left);
    let right = agent_doc_session_actor_io::canonical_document_id_in(project_root, right);
    left == right
}

fn supervisor_report_matches_existing_lease(
    project_root: &Path,
    current: &agent_doc_sqlite::state_store::ActorRecord,
    request: &ControllerRequest,
) -> Result<bool> {
    let conn = open_state_db(project_root)?;
    let Some(lease) =
        load_supervisor_lease_from_db(&conn, &current.document_id, current.generation)?
    else {
        return Ok(false);
    };
    let same_pid =
        request.supervisor_pid.is_some() && request.supervisor_pid == lease.supervisor_pid;
    let same_socket = request.supervisor_socket.as_deref().is_some()
        && request.supervisor_socket.as_deref() == lease.supervisor_socket.as_deref();
    Ok(same_pid || same_socket)
}

fn replace_closed_actor_from_same_supervisor_report(
    bootstrap: &ControllerBootstrap,
    runtime: Option<&ControllerRuntime>,
    file: &Path,
    current: &agent_doc_sqlite::state_store::ActorRecord,
    request: &ControllerRequest,
) -> Result<Option<agent_doc_sqlite::state_store::ActorRecord>> {
    let session_id = request_string(&request.session_id, "session_id")?;
    let pane_id = request_string(&request.pane_id, "pane_id")?;
    let generation = request_u64(request.generation, "generation")?;
    let runtime_state = request
        .state
        .as_deref()
        .unwrap_or(agent_doc_sqlite::state_store::ActorState::Starting.as_str());
    if current.state != agent_doc_sqlite::state_store::ActorState::Closed
        || generation <= current.generation
        || current.pane_id != pane_id
        || !supervisor_report_matches_existing_lease(&bootstrap.project_root, current, request)?
    {
        return Ok(None);
    }
    let state = agent_doc_sqlite::state_store::ActorState::parse(runtime_state)
        .with_context(|| format!("unknown supervisor runtime state: {runtime_state}"))?;
    let replacement = agent_doc_sqlite::state_store::ActorRecord {
        document_id: current.document_id.clone(),
        session_id,
        generation,
        pane_id,
        window_id: current.window_id.clone(),
        harness: agent_doc_session_actor_io::detect_document_harness_in(
            &bootstrap.project_root,
            &current.document_id,
        ),
        state,
        last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
            caller: "supervisor".to_string(),
            reason: "same_supervisor_session_replaced".to_string(),
            timestamp: timestamp_secs(),
            prior_generation: current.generation,
            new_generation: generation,
        },
    };
    let replacement = store_actor_record(
        &bootstrap.project_root,
        Some(current.generation),
        &replacement,
    )?;
    refresh_runtime_after_actor_write(runtime)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "controller_supervisor_replaced_closed_session file={} prior_session={} new_session={} pane={} prior_generation={} new_generation={} state={}",
            file.display(),
            current.session_id,
            replacement.session_id,
            replacement.pane_id,
            current.generation,
            replacement.generation,
            replacement.state.as_str(),
        ),
    );
    Ok(Some(replacement))
}

pub(crate) fn handle_register_supervisor(
    bootstrap: &ControllerBootstrap,
    runtime: Option<&ControllerRuntime>,
    request: ControllerRequest,
) -> Result<agent_doc_sqlite::state_store::ActorRecord> {
    let file = request_file(&request)?;
    let session_id = request_string(&request.session_id, "session_id")?;
    let pane_id = request_string(&request.pane_id, "pane_id")?;
    let generation = request_u64(request.generation, "generation")?;
    let runtime_state = request
        .state
        .as_deref()
        .unwrap_or(agent_doc_sqlite::state_store::ActorState::Starting.as_str());
    let document_id = agent_doc_session_actor_io::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let mut record = actor_record_from_authority(bootstrap, runtime, &document_id)?
        .with_context(|| format!("missing actor record for supervisor {}", file.display()))?;
    if record.session_id != session_id
        || record.pane_id != pane_id
        || record.generation != generation
    {
        if let Some(replacement) = replace_closed_actor_from_same_supervisor_report(
            bootstrap, runtime, &file, &record, &request,
        )? {
            record = replacement;
        } else {
            anyhow::bail!(
                "stale supervisor registration for {}: requested session={} pane={} generation={}, current session={} pane={} generation={}",
                file.display(),
                session_id,
                pane_id,
                generation,
                record.session_id,
                record.pane_id,
                record.generation
            );
        }
    }
    upsert_supervisor_lease(
        &bootstrap.project_root,
        &record,
        request.supervisor_pid,
        request.supervisor_socket.as_deref(),
        runtime_state,
    )?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "controller_supervisor_registered session={} pane={} generation={} state={}",
            session_id, pane_id, generation, runtime_state
        ),
    );
    if let Ok(conn) = open_state_db(&bootstrap.project_root)
        && let Ok(Some(control)) =
            state_store::load_queue_control_from_db(&conn, "document", &document_id)
    {
        let _ = clear_superseded_stale_supervisor_pause(
            &bootstrap.project_root,
            &file,
            &document_id,
            &record,
            &control,
        );
    }
    Ok(record)
}

pub(crate) fn handle_mark_lifecycle(
    bootstrap: &ControllerBootstrap,
    runtime: Option<&ControllerRuntime>,
    request: ControllerRequest,
) -> Result<agent_doc_sqlite::state_store::ActorRecord> {
    let file = request_file(&request)?;
    let session_id = request_string(&request.session_id, "session_id")?;
    let pane_id = request_string(&request.pane_id, "pane_id")?;
    let generation = request_u64(request.generation, "generation")?;
    let state_raw = request_string(&request.state, "state")?;
    let state = agent_doc_sqlite::state_store::ActorState::parse(&state_raw)
        .with_context(|| format!("unknown lifecycle state: {state_raw}"))?;
    let caller = request_string(&request.caller, "caller")?;
    let reason = request_string(&request.reason, "reason")?;
    let document_id = agent_doc_session_actor_io::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let current = actor_record_from_authority(bootstrap, runtime, &document_id)?
        .with_context(|| format!("missing authoritative actor record for {document_id}"))?;
    if current.session_id != session_id {
        anyhow::bail!(
            "stale actor transition for {}: session {} no longer owns generation {} (current session {})",
            document_id,
            session_id,
            current.generation,
            current.session_id
        );
    }
    if current.generation != generation && current.pane_id != pane_id {
        anyhow::bail!(
            "stale actor transition for {}: generation {} pane {} no longer current (current generation {} pane {})",
            document_id,
            generation,
            pane_id,
            current.generation,
            current.pane_id
        );
    }
    if current.pane_id != pane_id {
        anyhow::bail!(
            "stale actor transition for {}: pane {} no longer owns generation {} (current pane {})",
            document_id,
            pane_id,
            current.generation,
            current.pane_id
        );
    }
    let mut record = current.clone();
    record.state = state;
    record.last_transition = agent_doc_sqlite::state_store::ActorLastTransition {
        caller: caller.clone(),
        reason: reason.clone(),
        timestamp: timestamp_secs(),
        prior_generation: current.generation,
        new_generation: current.generation,
    };
    let record = store_actor_record(&bootstrap.project_root, Some(current.generation), &record)?;
    let startup_miss_clear_reason = match state {
        agent_doc_sqlite::state_store::ActorState::Ready => Some("actor_ready"),
        agent_doc_sqlite::state_store::ActorState::Closed => Some("actor_closed"),
        _ => None,
    };
    if let Some(clear_reason) = startup_miss_clear_reason
        && let Ok(Some(miss)) = agent_doc_supervisor_io::startup_miss::load_startup_miss(&file)
    {
        match agent_doc_supervisor_io::startup_miss::clear_startup_miss(&file) {
            Ok(()) => agent_doc_ops_log_io::log_op(
                &file,
                &format!(
                    "controller_startup_miss_cleared_lifecycle file={} stale_pane={} actor_pane={} session={} state={} reason={}",
                    file.display(),
                    miss.pane_id,
                    pane_id,
                    session_id,
                    state.as_str(),
                    clear_reason
                ),
            ),
            Err(e) => eprintln!(
                "[controller] startup-miss clear on {} failed for {} (non-fatal): {e}",
                state.as_str(),
                file.display(),
            ),
        }
    }
    // #qflood: a transition to Ready means the turn finished, so any dispatch in
    // flight for this document is now consumed — release the in-flight marker so the
    // open-dispatch set stays accurate for the next busy episode's coalescing and for
    // restart recovery. A Ready projection alone is not sufficient to bypass an
    // open receipt; only this controller-owned lifecycle boundary consumes it.
    if matches!(state, agent_doc_sqlite::state_store::ActorState::Ready) {
        match open_state_db(&bootstrap.project_root)
            .and_then(|conn| state_store::mark_open_dispatches_consumed(&conn, &document_id))
        {
            Ok(released) if released > 0 => agent_doc_ops_log_io::log_op(
                &file,
                &format!(
                    "dispatch_in_flight_released document_id={} count={} reason=actor_ready",
                    document_id, released
                ),
            ),
            Ok(_) => {}
            Err(e) => {
                eprintln!("[controller] #qflood in-flight release on Ready failed (non-fatal): {e}")
            }
        }
    }
    refresh_runtime_after_actor_write(runtime)?;
    upsert_supervisor_lease(
        &bootstrap.project_root,
        &record,
        request.supervisor_pid,
        request.supervisor_socket.as_deref(),
        state.as_str(),
    )?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "controller_lifecycle session={} pane={} generation={} state={} caller={} reason={}",
            session_id,
            pane_id,
            generation,
            state.as_str(),
            caller,
            reason
        ),
    );
    Ok(record)
}

pub(crate) fn handle_supervisor_heartbeat(
    bootstrap: &ControllerBootstrap,
    runtime: Option<&ControllerRuntime>,
    request: ControllerRequest,
) -> Result<SupervisorLeaseStatus> {
    let file = request_file(&request)?;
    let session_id = request_string(&request.session_id, "session_id")?;
    let pane_id = request_string(&request.pane_id, "pane_id")?;
    let generation = request_u64(request.generation, "generation")?;
    let runtime_state = request
        .state
        .as_deref()
        .unwrap_or(agent_doc_sqlite::state_store::ActorState::Starting.as_str());
    let document_id = agent_doc_session_actor_io::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let mut record = actor_record_from_authority(bootstrap, runtime, &document_id)?
        .with_context(|| format!("missing actor record for supervisor {}", file.display()))?;
    if record.session_id != session_id
        || record.pane_id != pane_id
        || record.generation != generation
    {
        if let Some(replacement) = replace_closed_actor_from_same_supervisor_report(
            bootstrap, runtime, &file, &record, &request,
        )? {
            record = replacement;
        } else {
            anyhow::bail!(
                "stale supervisor heartbeat for {}: requested session={} pane={} generation={}, current session={} pane={} generation={}",
                file.display(),
                session_id,
                pane_id,
                generation,
                record.session_id,
                record.pane_id,
                record.generation
            );
        }
    }
    upsert_supervisor_lease(
        &bootstrap.project_root,
        &record,
        request.supervisor_pid,
        request.supervisor_socket.as_deref(),
        runtime_state,
    )?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "controller_supervisor_heartbeat session={} pane={} generation={} state={}",
            session_id, pane_id, generation, runtime_state
        ),
    );
    if let Ok(conn) = open_state_db(&bootstrap.project_root)
        && let Ok(Some(control)) =
            state_store::load_queue_control_from_db(&conn, "document", &document_id)
    {
        let _ = clear_superseded_stale_supervisor_pause(
            &bootstrap.project_root,
            &file,
            &document_id,
            &record,
            &control,
        );
    }
    load_supervisor_lease_from_db(
        &open_state_db(&bootstrap.project_root)?,
        &record.document_id,
        record.generation,
    )?
    .context("missing supervisor lease after heartbeat")
}

pub(crate) fn handle_actor_binding(
    bootstrap: &ControllerBootstrap,
    runtime: Option<&ControllerRuntime>,
    request: ControllerRequest,
) -> Result<ActorBindingResponse> {
    let file = request_file(&request)?;
    let document_id = agent_doc_session_actor_io::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let record = actor_record_from_authority(bootstrap, runtime, &document_id)?;
    Ok(match record {
        Some(record) => ActorBindingResponse {
            status: ActorBindingStatus::Bound,
            record: Some(record),
        },
        None => ActorBindingResponse {
            status: ActorBindingStatus::NotFound,
            record: None,
        },
    })
}

pub(crate) fn handle_dispatch(
    bootstrap: &ControllerBootstrap,
    runtime: Option<&ControllerRuntime>,
    request: ControllerRequest,
) -> Result<DispatchAuthorization> {
    let file = request_file(&request)?;
    let session_id = request_string(&request.session_id, "session_id")?;
    let pane_id = request_string(&request.pane_id, "pane_id")?;
    let generation = request_u64(request.generation, "generation")?;
    let command_kind = request_string(&request.command_kind, "command_kind")?;
    let diagnostic_payload = request
        .diagnostic_payload
        .as_deref()
        .unwrap_or_default()
        .to_string();
    let document_id = agent_doc_session_actor_io::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    // `#ctlstalebin` (#stuckhandoff2 follow-up): a controller whose own binary no
    // longer matches the installed agent-doc keeps running OLD code. `connect_or_launch`
    // hands cross-process callers to a fresh controller, but any dispatch that still
    // reaches this stale `handle_dispatch` (in-process co-host, handoff race) must be
    // refused so the stale process cannot keep driving session writes — the operator's
    // observed "old binary churns for ~1h until manual restart" failure. The caller
    // (`authorize_dispatch`) re-runs the dispatch once; the retry's `connect_or_launch`
    // promotes the freshly-installed binary, then the dispatch admits normally.
    let current_binary = current_binary_identity().ok();
    if status::process_binary_is_stale(
        bootstrap.controller_binary.as_ref(),
        current_binary.as_ref(),
    ) {
        let receipt = insert_dispatch_attempt_record(
            &bootstrap.project_root,
            ControllerDispatchReceiptInsert {
                document_id: &document_id,
                generation,
                command_kind: &command_kind,
                accepted_stage: None,
                failed_stage: Some("controller_binary_stale"),
                diagnostic_payload: &diagnostic_payload,
                result_status: ControllerDispatchResultStatus::Rejected,
                proof_scope: ControllerDispatchProofScope::AcceptedOnly,
                dispatch_start_proven: false,
            },
        )?;
        agent_doc_ops_log_io::log_op(
            &bootstrap.project_root,
            &format!(
                "dispatch_refused_stale_binary file={} generation={} receipt_id={}",
                file.display(),
                generation,
                receipt.receipt_id,
            ),
        );
        anyhow::bail!(
            "dispatch refused for {}: controller_binary_stale (running controller binary no longer matches the installed agent-doc; reconnect to promote the fresh binary)",
            file.display()
        );
    }
    // M2 (#stuckhandoff2): only a `Stable` controller is authoritative for mutating
    // dispatch admission. A controller still `Preparing` (mid-handoff, or wedged
    // because the client died before `promote_handoff`), `Promoted`, `Retiring`, or
    // `Failed` must refuse to admit dispatches that drive session writes — even
    // before M1's self-watchdog reaps it — so a wedged controller cannot corrupt the
    // worktree between wedge and reap (this also shrinks the `#pcwc` drift window).
    // Read-only status/inspect and the operator/handoff/admin control RPCs are
    // handled before `handle_dispatch` and stay available, so the operator can still
    // pause/shut down a non-authoritative controller and the handoff can still
    // promote it to `Stable`.
    if bootstrap.handoff_state != ControllerHandoffState::Stable {
        let receipt = insert_dispatch_attempt_record(
            &bootstrap.project_root,
            ControllerDispatchReceiptInsert {
                document_id: &document_id,
                generation,
                command_kind: &command_kind,
                accepted_stage: None,
                failed_stage: Some("controller_not_authoritative"),
                diagnostic_payload: &diagnostic_payload,
                result_status: ControllerDispatchResultStatus::Rejected,
                proof_scope: ControllerDispatchProofScope::AcceptedOnly,
                dispatch_start_proven: false,
            },
        )?;
        agent_doc_ops_log_io::log_op(
            &bootstrap.project_root,
            &format!(
                "dispatch_refused_non_stable_controller file={} handoff_state={:?} generation={} receipt_id={}",
                file.display(),
                bootstrap.handoff_state,
                generation,
                receipt.receipt_id,
            ),
        );
        anyhow::bail!(
            "dispatch refused for {}: controller not authoritative (handoff_state={:?})",
            file.display(),
            bootstrap.handoff_state
        );
    }
    let record = actor_record_from_authority(bootstrap, runtime, &document_id)?
        .with_context(|| format!("missing actor record for dispatch {}", file.display()))?;
    let mut queue_control = {
        let conn = open_state_db(&bootstrap.project_root)?;
        state_store::load_effective_queue_control_from_db(
            &conn,
            &document_id,
            &bootstrap.project_root.to_string_lossy(),
        )?
    };
    if let Some(control) = queue_control.as_ref()
        && repair_spent_preset_pause_before_dispatch(
            &bootstrap.project_root,
            &file,
            &document_id,
            control,
        )?
    {
        queue_control = None;
    }
    if let Some(control) = queue_control.as_ref()
        && clear_superseded_stale_supervisor_pause(
            &bootstrap.project_root,
            &file,
            &document_id,
            &record,
            control,
        )?
    {
        queue_control = None;
    }
    let queue_block_stage = queue_control.as_ref().and_then(|control| {
        if control.state == "paused" {
            // `#qpauserun`: a deliberate operator/admin `paused` queue control
            // suppresses UNATTENDED auto-dispatch (idle-watch / `/loop`
            // continuation), but an EXPLICIT operator reopen (JB `Run Agent Doc`
            // → route managed / dispatch-only reopen) must still start — the pause
            // is about auto-draining the queue, not about whether the operator can
            // run a cycle. Allowing the explicit reopen is one-shot: the pause row
            // stays in place, so future auto callers remain blocked until
            // `admin queue resume`. EXCEPTION: a stale-supervisor churn-stop pause
            // (`#jbrestale`) is NOT a deliberate operator pause — it still blocks
            // every caller so the route path restarts the stale supervisor and
            // re-dispatches once, rather than admitting a reopen against a stale
            // supervisor.
            let stale_supervisor_pause = pause_reason_is_stale_supervisor_churn_stop(
                control.reason.as_deref().unwrap_or(""),
            );
            if dispatch_command_kind_is_operator_reopen(&command_kind) && !stale_supervisor_pause {
                None
            } else {
                Some("queue_paused")
            }
        } else if control.state == "draining"
            && record.state != agent_doc_sqlite::state_store::ActorState::Ready
        {
            Some("actor_busy_draining")
        } else {
            None
        }
    });
    if let Some(stage) = queue_block_stage {
        let reason = queue_control
            .as_ref()
            .and_then(|control| control.reason.as_deref())
            .unwrap_or(stage);
        let file_path = if file.is_absolute() {
            file.to_path_buf()
        } else {
            bootstrap.project_root.join(&file)
        };
        let blocked_head = std::fs::read_to_string(&file_path)
            .ok()
            .and_then(|content| {
                agent_doc_queue::queue_heads::active_queue_head_text(&content)
                    .ok()
                    .flatten()
            });
        let trigger = dispatch_diagnostic_field(&diagnostic_payload, "harness").map(|harness| {
            agent_doc_harness::HarnessConfig::from_agent_name(harness)
                .trigger_command(&file.to_string_lossy())
        });
        let proof_fields = dispatch_blocked_proof_fields(DispatchBlockedProofFacts {
            stage,
            reason,
            blocked_head: blocked_head.as_deref(),
            trigger: trigger.as_deref(),
        });
        let blocked_diagnostic_payload =
            append_dispatch_proof_payload(&diagnostic_payload, &proof_fields);
        let receipt = insert_dispatch_attempt_record(
            &bootstrap.project_root,
            ControllerDispatchReceiptInsert {
                document_id: &document_id,
                generation: record.generation,
                command_kind: &command_kind,
                accepted_stage: None,
                failed_stage: Some(stage),
                diagnostic_payload: &blocked_diagnostic_payload,
                result_status: ControllerDispatchResultStatus::Blocked,
                proof_scope: ControllerDispatchProofScope::AcceptedOnly,
                dispatch_start_proven: false,
            },
        )?;
        if !proof_fields.is_empty() {
            agent_doc_ops_log_io::log_op(
                &file,
                &format!(
                    "dispatch_blocked_proof file={} failed_stage={} receipt_id={} {}",
                    file.display(),
                    stage,
                    receipt.receipt_id,
                    proof_fields
                ),
            );
        }
        let conn = open_state_db(&bootstrap.project_root)?;
        let _backpressure = state_store::insert_queue_backpressure_in_db(
            &conn,
            &state_store::QueueBackpressureInsert {
                document_id: &document_id,
                generation: Some(record.generation),
                command_kind: &command_kind,
                capacity_class: stage,
                reason,
                dispatch_receipt_id: Some(receipt.receipt_id),
            },
        )?;
        // `#jbrestale`: a `queue_paused` whose pause was written by the churn detector
        // because a STALE supervisor re-injected an already-answered/operator-verify
        // head is recoverable by recycling the supervisor — not a terminal operator
        // pause. Tag the bail with the restart-redirect marker + the named stale PID so
        // the route dispatch path can restart the supervisor once, lift this pause, and
        // re-dispatch instead of failing closed. A deliberate operator/spent-preset
        // pause does not classify and stays terminal.
        let recovery_suffix = if stage == "queue_paused"
            && pause_reason_is_stale_supervisor_churn_stop(reason)
        {
            let stale_pid = stale_supervisor_pid_from_pause_reason(reason).unwrap_or(0);
            let recovery = StaleQueuePauseRecovery::new(stale_pid);
            agent_doc_ops_log_io::log_op(
                &file,
                &format!(
                    "dispatch_queue_paused_stale_supervisor file={} stale_pid={} marker={} {} {}",
                    file.display(),
                    stale_pid,
                    DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER,
                    recovery.outcome.log_fields(),
                    agent_doc_flow::outcome::UserFacingOutcome::new(
                        agent_doc_flow::outcome::UserFacingOutcomeKind::RecoveredAndRetried,
                    )
                    .expect("static recovered-and-retried outcome is valid")
                    .log_fields(),
                ),
            );
            format!(" {DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER} stale_pid={stale_pid}")
        } else {
            String::new()
        };
        let proof_suffix = if proof_fields.is_empty() {
            String::new()
        } else {
            format!(" {proof_fields}")
        };
        anyhow::bail!(
            "dispatch blocked for {}: failed_stage={} reason={} receipt_id={}{}{}",
            file.display(),
            stage,
            reason,
            receipt.receipt_id,
            recovery_suffix,
            proof_suffix
        );
    }
    let mut failed_stage = None;
    let mut failure = None;
    let mut failure_status = ControllerDispatchResultStatus::Rejected;
    if record.session_id != session_id {
        failed_stage = Some("stale_session");
        failure = Some(format!(
            "dispatch rejected for {}: requested session {}, current session {}",
            file.display(),
            session_id,
            record.session_id
        ));
    } else if record.pane_id != pane_id {
        failed_stage = Some("stale_pane");
        failure = Some(format!(
            "dispatch rejected for {}: requested pane {}, current pane {}",
            file.display(),
            pane_id,
            record.pane_id
        ));
    } else if record.generation != generation {
        failed_stage = Some("stale_generation");
        // `#anw0` (#supkill-bg part 3): a racing dispatcher holding a superseded
        // generation should self-heal by retrying against the current generation
        // rather than failing closed — BUT only when the current generation is itself
        // dispatchable (a retry would actually be authorized). If the current actor is
        // Closed/Blocked a retry cannot help, so keep the terminal reject with no
        // redirect marker. `authorize_dispatch` consumes the marker and retries once.
        let current_dispatchable = !matches!(
            record.state,
            agent_doc_sqlite::state_store::ActorState::Blocked
                | agent_doc_sqlite::state_store::ActorState::Closed
        );
        if current_dispatchable {
            agent_doc_ops_log_io::log_op(
                &file,
                &format!(
                    "dispatch_stale_generation_redirect session={} pane={} prior_generation={} next_generation={} kind={}",
                    session_id, pane_id, generation, record.generation, command_kind
                ),
            );
            failure = Some(format!(
                "dispatch rejected for {}: requested generation {}, current generation {} ({} retry_generation={})",
                file.display(),
                generation,
                record.generation,
                DISPATCH_STALE_GENERATION_REDIRECT_MARKER,
                record.generation
            ));
        } else {
            failure = Some(format!(
                "dispatch rejected for {}: requested generation {}, current generation {}",
                file.display(),
                generation,
                record.generation
            ));
        }
    } else if matches!(
        record.state,
        agent_doc_sqlite::state_store::ActorState::Blocked
            | agent_doc_sqlite::state_store::ActorState::Closed
    ) {
        failed_stage = Some(record.state.as_str());
        failure_status = ControllerDispatchResultStatus::Blocked;
        failure = Some(format!(
            "dispatch rejected for {}: authoritative actor generation {} is {}",
            file.display(),
            generation,
            record.state.as_str()
        ));
    }

    if let Some(message) = failure {
        let stage = failed_stage.unwrap_or("rejected");
        let receipt = insert_dispatch_attempt_record(
            &bootstrap.project_root,
            ControllerDispatchReceiptInsert {
                document_id: &document_id,
                generation,
                command_kind: &command_kind,
                accepted_stage: None,
                failed_stage: Some(stage),
                diagnostic_payload: &diagnostic_payload,
                result_status: failure_status,
                proof_scope: ControllerDispatchProofScope::AcceptedOnly,
                dispatch_start_proven: false,
            },
        )?;
        anyhow::bail!("{message} receipt_id={}", receipt.receipt_id);
    }

    // #qflood: the durable unconsumed receipt is authoritative backpressure.
    // Coalesce a second dispatch even if the lossy actor projection briefly says
    // Ready (for example, an idle-pane reconcile racing composer submission).
    // The first dispatch has no open receipt and always passes; a genuine Ready
    // transition consumes the prior receipt, so the next cycle also passes.
    let conn = open_state_db(&bootstrap.project_root)?;
    let in_flight =
        state_store::has_open_in_flight_dispatch(&conn, &document_id, record.generation)?;
    let operator_driven = dispatch_command_kind_is_operator_reopen(&command_kind);
    if dispatch_should_coalesce_in_flight(in_flight, operator_driven) {
        let receipt = insert_dispatch_attempt_record(
            &bootstrap.project_root,
            ControllerDispatchReceiptInsert {
                document_id: &document_id,
                generation: record.generation,
                command_kind: &command_kind,
                accepted_stage: None,
                failed_stage: Some("coalesced_in_flight"),
                diagnostic_payload: &diagnostic_payload,
                result_status: ControllerDispatchResultStatus::Blocked,
                proof_scope: ControllerDispatchProofScope::AcceptedOnly,
                dispatch_start_proven: false,
            },
        )?;
        agent_doc_ops_log_io::log_op(
            &file,
            &format!(
                "dispatch_coalesced_in_flight session={} pane={} generation={} state={} kind={} receipt_id={} reason=in_flight_redispatch",
                session_id,
                pane_id,
                record.generation,
                record.state.as_str(),
                command_kind,
                receipt.receipt_id
            ),
        );
        anyhow::bail!(
            "dispatch coalesced for {}: a dispatch for generation {} is already in flight (#qflood); {} receipt_id={}",
            file.display(),
            record.generation,
            DISPATCH_COALESCED_IN_FLIGHT_MARKER,
            receipt.receipt_id
        );
    }

    let accepted_stage = match record.state {
        agent_doc_sqlite::state_store::ActorState::Ready => "ready",
        agent_doc_sqlite::state_store::ActorState::Starting => "starting_queue",
        agent_doc_sqlite::state_store::ActorState::Busy => "busy_queue",
        agent_doc_sqlite::state_store::ActorState::WaitingInput => "waiting_input_recovery",
        agent_doc_sqlite::state_store::ActorState::Blocked
        | agent_doc_sqlite::state_store::ActorState::Closed => {
            unreachable!("blocked/closed dispatch rejected above")
        }
    };
    let result_status = match record.state {
        agent_doc_sqlite::state_store::ActorState::Ready => {
            ControllerDispatchResultStatus::Accepted
        }
        agent_doc_sqlite::state_store::ActorState::Starting
        | agent_doc_sqlite::state_store::ActorState::Busy
        | agent_doc_sqlite::state_store::ActorState::WaitingInput => {
            ControllerDispatchResultStatus::Queued
        }
        agent_doc_sqlite::state_store::ActorState::Blocked
        | agent_doc_sqlite::state_store::ActorState::Closed => {
            unreachable!("blocked/closed dispatch rejected above")
        }
    };
    let receipt = insert_dispatch_attempt_record(
        &bootstrap.project_root,
        ControllerDispatchReceiptInsert {
            document_id: &document_id,
            generation: record.generation,
            command_kind: &command_kind,
            accepted_stage: Some(accepted_stage),
            failed_stage: None,
            diagnostic_payload: &diagnostic_payload,
            result_status,
            proof_scope: ControllerDispatchProofScope::AcceptedOnly,
            dispatch_start_proven: false,
        },
    )?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "controller_dispatch_accepted session={} pane={} generation={} state={} kind={} stage={} receipt_id={} proof_scope={}",
            session_id,
            pane_id,
            generation,
            record.state.as_str(),
            command_kind,
            accepted_stage,
            receipt.receipt_id,
            receipt.proof_scope.as_str()
        ),
    );
    Ok(DispatchAuthorization {
        record,
        accepted_stage: accepted_stage.to_string(),
        receipt,
    })
}

pub(crate) fn handle_session_status(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<SessionOperatorStatus> {
    let file = request_file(&request)?;
    let document_id = agent_doc_session_actor_io::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let conn = open_state_db(&bootstrap.project_root)?;
    load_session_operator_status_from_db(&conn, &document_id)
}

pub(crate) fn admin_target_record(
    bootstrap: &ControllerBootstrap,
    request: &ControllerRequest,
) -> Result<(
    String,
    Option<String>,
    Option<agent_doc_sqlite::state_store::ActorRecord>,
)> {
    let conn = open_state_db(&bootstrap.project_root)?;
    if let Some(file) = request.file.as_ref() {
        let document_id = agent_doc_session_actor_io::canonical_document_id_in(
            &bootstrap.project_root,
            &file.to_string_lossy(),
        );
        let record = load_actor_record_from_db(&conn, &document_id)?;
        return Ok((file.display().to_string(), Some(document_id), record));
    }
    if let Some(session_id) = request.session_id.as_deref() {
        let store = load_actor_store_from_db(&conn)?;
        let record = store
            .values()
            .find(|record| record.session_id == session_id)
            .cloned();
        let document_id = record.as_ref().map(|record| record.document_id.clone());
        return Ok((format!("session:{session_id}"), document_id, record));
    }
    if let Some(pane_id) = request.pane_id.as_deref() {
        let store = load_actor_store_from_db(&conn)?;
        let record = store
            .values()
            .find(|record| record.pane_id == pane_id)
            .cloned();
        let document_id = record.as_ref().map(|record| record.document_id.clone());
        return Ok((format!("pane:{pane_id}"), document_id, record));
    }
    anyhow::bail!("admin request requires a document, --session, or --pane target")
}

pub(crate) fn handle_inspect_actor(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<ControllerActorInspection> {
    let (target, document_id, record) = admin_target_record(bootstrap, &request)?;
    let conn = open_state_db(&bootstrap.project_root)?;
    let supervisor_lease = match record.as_ref() {
        Some(record) => {
            load_supervisor_lease_from_db(&conn, &record.document_id, record.generation)?
        }
        None => None,
    };
    let queue_head = match document_id.as_deref() {
        Some(document_id) => {
            state_store::load_queue_head_from_db(&conn, document_id, "agent:queue")?
        }
        None => None,
    };
    let queue_control = match document_id.as_deref() {
        Some(document_id) => state_store::load_effective_queue_control_from_db(
            &conn,
            document_id,
            &bootstrap.project_root.to_string_lossy(),
        )?,
        None => None,
    };
    let dispatch_attempts = match document_id.as_deref() {
        Some(document_id) => state_store::load_dispatch_attempts_from_db(&conn, document_id)?,
        None => Vec::new(),
    };
    let queue_backpressure = match document_id.as_deref() {
        Some(document_id) => state_store::load_queue_backpressure_from_db(&conn, document_id, 10)?,
        None => Vec::new(),
    };
    let admin_operations =
        state_store::load_admin_operations_from_db(&conn, document_id.as_deref(), 10)?;
    let projection_diagnostics = match document_id.as_deref() {
        Some(document_id) => state_store::load_projection_diagnostics_from_db(&conn, document_id)?,
        None => Vec::new(),
    };
    let projection_lag = projection_diagnostics
        .iter()
        .any(|diagnostic| diagnostic.retry_status.as_deref() != Some("completed"));
    let supervisor_pid = supervisor_lease
        .as_ref()
        .and_then(|lease| lease.supervisor_pid);
    Ok(ControllerActorInspection {
        target,
        document_id,
        record,
        supervisor_lease,
        freshness: Some(status::controller_freshness_status(
            controller_freshness_facts(Some(bootstrap.pid), supervisor_pid),
        )),
        queue_head,
        queue_control,
        queue_backpressure,
        projection_lag,
        dispatch_attempts,
        admin_operations,
        projection_diagnostics,
    })
}

fn inactive_tmux_focus_state(
    reason: &str,
    session_name: Option<String>,
    window_id: Option<String>,
    window_name: Option<String>,
    pane_id: Option<String>,
) -> ControllerTmuxFocusState {
    ControllerTmuxFocusState {
        active: false,
        reason: reason.to_string(),
        session_name,
        window_id,
        window_name,
        pane_id,
        document_id: None,
        record: None,
    }
}

fn tmux_focus_receipt(
    focused: bool,
    reason: &str,
    document_id: Option<String>,
    pane_id: Option<String>,
    session_name: Option<String>,
    window_id: Option<String>,
    window_name: Option<String>,
) -> ControllerTmuxFocusReceipt {
    ControllerTmuxFocusReceipt {
        focused,
        reason: reason.to_string(),
        document_id,
        pane_id,
        session_name,
        window_id,
        window_name,
    }
}

fn configured_tmux_session_for_project(project_root: &Path) -> Option<String> {
    let config_path = project_root.join(".agent-doc").join("config.toml");
    agent_doc_project_config_io::load_project_from(&config_path)
        .tmux_session
        .filter(|session| !session.trim().is_empty())
}

fn active_tmux_window_for_session(
    tmux: &tmux_router::Tmux,
    session_name: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    let window_id = tmux.active_window(session_name);
    let window_name = window_id
        .as_deref()
        .and_then(|window| agent_doc_tmux_io::target_window_name(tmux, window));
    let pane_id = tmux.active_pane(session_name);
    (window_id, window_name, pane_id)
}

fn resolve_agent_doc_window_id_for_session(
    tmux: &tmux_router::Tmux,
    session_name: &str,
) -> Option<String> {
    let listing = agent_doc_tmux_io::list_windows(
        tmux,
        Some(&format!("{session_name}:")),
        "#{window_id} #{window_name}",
    )
    .ok()?;
    listing.lines().find_map(|line| {
        let mut parts = line.splitn(2, ' ');
        match (parts.next(), parts.next()) {
            (Some(window_id), Some("agent-doc")) => Some(window_id.to_string()),
            _ => None,
        }
    })
}

fn canonical_layout_document_id(project_root: &Path, file: &str) -> String {
    let trimmed = file.trim();
    let path = if Path::new(trimmed).is_absolute() {
        PathBuf::from(trimmed)
    } else {
        project_root.join(trimmed)
    };
    path.canonicalize()
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

fn first_agent_doc_in_layout_column(project_root: &Path, column: &str) -> Option<String> {
    column.split(',').find_map(|raw| {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        let document_id = canonical_layout_document_id(project_root, raw);
        let content = std::fs::read_to_string(&document_id).ok()?;
        let (frontmatter, _) = agent_doc_frontmatter::frontmatter::parse(&content).ok()?;
        frontmatter.session.as_ref()?;
        Some(document_id)
    })
}

fn layout_sync_state_expected_documents(
    project_root: &Path,
    invocation: &ControllerTmuxLayoutSyncStateInvocation,
) -> Vec<String> {
    let saved_layout = load_layout_state(project_root).unwrap_or_default();
    let classified = agent_doc_tmux::classify_sync_layout_columns(&invocation.columns, |column| {
        first_agent_doc_in_layout_column(project_root, column)
    });
    let effective = agent_doc_tmux::apply_column_memory(&classified, &saved_layout);
    agent_doc_tmux::classify_sync_layout_columns(&effective.columns, |column| {
        first_agent_doc_in_layout_column(project_root, column)
    })
    .into_iter()
    .filter_map(|column| column.agent_doc)
    .collect()
}

fn layout_sync_state_actual_document_for_pane(
    project_root: &Path,
    tmux: &tmux_router::Tmux,
    actor_store: &BTreeMap<String, agent_doc_sqlite::state_store::ActorRecord>,
    pane_id: &str,
) -> String {
    if let Some(record) = actor_store
        .values()
        .find(|record| record.pane_id == pane_id)
    {
        return canonical_layout_document_id(project_root, &record.document_id);
    }
    active_pane_process_owner_document(tmux, pane_id, project_root).unwrap_or_default()
}

#[derive(Default)]
struct LayoutSyncStateTarget {
    session_name: Option<String>,
    window_id: Option<String>,
    window_name: Option<String>,
    focus: Option<String>,
}

fn layout_sync_state_report(
    synced: bool,
    reason: impl Into<String>,
    expected_documents: Vec<String>,
    actual_documents: Vec<String>,
    panes: Vec<String>,
    target: LayoutSyncStateTarget,
) -> ControllerTmuxLayoutSyncStateReport {
    ControllerTmuxLayoutSyncStateReport {
        synced,
        reason: reason.into(),
        expected_documents,
        actual_documents,
        panes,
        session_name: target.session_name,
        window_id: target.window_id,
        window_name: target.window_name,
        focus: target.focus,
    }
}

fn layout_sync_state_result(
    expected_documents: &[String],
    actual_documents: &[String],
) -> (bool, &'static str) {
    if expected_documents == actual_documents {
        (true, "synced")
    } else if expected_documents.len() != actual_documents.len() {
        (false, "pane_count_mismatch")
    } else {
        (false, "pane_order_mismatch")
    }
}

pub(crate) fn handle_tmux_layout_sync_state(
    bootstrap: &ControllerBootstrap,
    runtime: &ControllerRuntime,
    request: ControllerRequest,
) -> Result<ControllerTmuxLayoutSyncStateReport> {
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let invocation: ControllerTmuxLayoutSyncStateInvocation =
        serde_json::from_str(&payload_json).context("parse tmux layout sync state invocation")?;
    let expected_documents =
        layout_sync_state_expected_documents(&bootstrap.project_root, &invocation);
    let focus = invocation.focus.clone();
    if expected_documents.is_empty() {
        return Ok(layout_sync_state_report(
            false,
            "empty_layout_model",
            expected_documents,
            Vec::new(),
            Vec::new(),
            LayoutSyncStateTarget {
                window_id: invocation.window,
                focus,
                ..LayoutSyncStateTarget::default()
            },
        ));
    }

    let Some(configured_session) = configured_tmux_session_for_project(&bootstrap.project_root)
    else {
        return Ok(layout_sync_state_report(
            false,
            "missing_tmux_session",
            expected_documents,
            Vec::new(),
            Vec::new(),
            LayoutSyncStateTarget {
                window_id: invocation.window,
                focus,
                ..LayoutSyncStateTarget::default()
            },
        ));
    };
    let tmux = tmux_router::Tmux::default_server();
    if !tmux.session_alive(&configured_session) {
        return Ok(layout_sync_state_report(
            false,
            "tmux_session_not_alive",
            expected_documents,
            Vec::new(),
            Vec::new(),
            LayoutSyncStateTarget {
                session_name: Some(configured_session),
                window_id: invocation.window,
                focus,
                ..LayoutSyncStateTarget::default()
            },
        ));
    }

    let window_id = invocation
        .window
        .clone()
        .or_else(|| resolve_agent_doc_window_id_for_session(&tmux, &configured_session));
    let Some(window_id_value) = window_id.clone() else {
        return Ok(layout_sync_state_report(
            false,
            "missing_agent_doc_window",
            expected_documents,
            Vec::new(),
            Vec::new(),
            LayoutSyncStateTarget {
                session_name: Some(configured_session),
                focus,
                ..LayoutSyncStateTarget::default()
            },
        ));
    };
    let window_name = agent_doc_tmux_io::target_window_name(&tmux, &window_id_value);
    if window_name.as_deref() != Some("agent-doc") {
        return Ok(layout_sync_state_report(
            false,
            "target_window_not_agent_doc",
            expected_documents,
            Vec::new(),
            Vec::new(),
            LayoutSyncStateTarget {
                session_name: Some(configured_session),
                window_id: Some(window_id_value),
                window_name,
                focus,
            },
        ));
    }

    let panes = tmux
        .list_panes_ordered(&window_id_value)
        .unwrap_or_default();
    // Layout inspection is a hot read path. The controller's lazily-held actor
    // projection is authoritative here; SQLite and the registry are durable
    // effect sinks, not competing read models.
    let actor_store = runtime.actor_store_snapshot();
    let actual_documents = panes
        .iter()
        .map(|pane_id| {
            layout_sync_state_actual_document_for_pane(
                &bootstrap.project_root,
                &tmux,
                &actor_store,
                pane_id,
            )
        })
        .collect::<Vec<_>>();
    let (synced, reason) = layout_sync_state_result(&expected_documents, &actual_documents);
    Ok(layout_sync_state_report(
        synced,
        reason,
        expected_documents,
        actual_documents,
        panes,
        LayoutSyncStateTarget {
            session_name: Some(configured_session),
            window_id: Some(window_id_value),
            window_name,
            focus,
        },
    ))
}

pub(crate) fn handle_tmux_focus_state(
    bootstrap: &ControllerBootstrap,
) -> Result<ControllerTmuxFocusState> {
    let Some(session_name) = configured_tmux_session_for_project(&bootstrap.project_root) else {
        return Ok(inactive_tmux_focus_state(
            "missing_tmux_session",
            None,
            None,
            None,
            None,
        ));
    };
    let tmux = tmux_router::Tmux::default_server();
    if !tmux.session_alive(&session_name) {
        return Ok(inactive_tmux_focus_state(
            "tmux_session_not_alive",
            Some(session_name),
            None,
            None,
            None,
        ));
    }
    let (window_id, window_name, pane_id) = active_tmux_window_for_session(&tmux, &session_name);
    if window_name.as_deref() != Some("agent-doc") {
        return Ok(inactive_tmux_focus_state(
            "outside_agent_doc_window",
            Some(session_name),
            window_id,
            window_name,
            pane_id,
        ));
    }
    let Some(pane_id_value) = pane_id.clone() else {
        return Ok(inactive_tmux_focus_state(
            "missing_active_pane",
            Some(session_name),
            window_id,
            window_name,
            None,
        ));
    };
    let conn = open_state_db(&bootstrap.project_root)?;
    let record = load_actor_store_from_db(&conn)?
        .values()
        .find(|record| {
            record.pane_id == pane_id_value
                && !matches!(
                    record.state,
                    agent_doc_sqlite::state_store::ActorState::Blocked
                        | agent_doc_sqlite::state_store::ActorState::Closed
                )
        })
        .cloned();
    let process_owner_document = record
        .is_none()
        .then(|| active_pane_process_owner_document(&tmux, &pane_id_value, &bootstrap.project_root))
        .flatten();
    let document_id = record
        .as_ref()
        .map(|record| record.document_id.clone())
        .or(process_owner_document);
    Ok(ControllerTmuxFocusState {
        active: document_id.is_some(),
        reason: if record.is_some() {
            "focused_agent_doc_actor".to_string()
        } else if document_id.is_some() {
            "focused_live_process_owner".to_string()
        } else {
            "active_pane_unbound".to_string()
        },
        session_name: Some(session_name),
        window_id,
        window_name,
        pane_id,
        document_id,
        record,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusPaneCandidateDecision<'a> {
    Candidate {
        pane_id: &'a str,
        focused_reason: &'static str,
        not_alive_reason: &'static str,
    },
    Reject {
        reason: &'static str,
        pane_id: Option<&'a str>,
    },
}

/// Reconcile the durable actor/registry projections against a pane whose
/// current process tree proves ownership of the selected document. Focus is a
/// non-mutating UI handoff, so a closed/blocked projection must not hide a
/// still-running route-owned agent. The proof wins only when it is exact; the
/// I/O adapter below rejects dead, bare-shell, and cross-document pane reuse.
fn decide_focus_pane_candidate<'a>(
    actor: Option<(&'a str, bool)>,
    registry_pane: Option<&'a str>,
    proven_live_owner: Option<&'a str>,
) -> FocusPaneCandidateDecision<'a> {
    if let Some(owner) = proven_live_owner {
        if actor.is_some_and(|(pane, focusable)| focusable && pane == owner) {
            return FocusPaneCandidateDecision::Candidate {
                pane_id: owner,
                focused_reason: "focused_agent_doc_actor",
                not_alive_reason: "actor_pane_not_alive",
            };
        }
        return FocusPaneCandidateDecision::Candidate {
            pane_id: owner,
            focused_reason: "focused_live_process_owner",
            not_alive_reason: "live_owner_pane_not_alive",
        };
    }

    if let Some((pane_id, focusable)) = actor {
        if !focusable {
            return FocusPaneCandidateDecision::Reject {
                reason: "actor_not_focusable",
                pane_id: Some(pane_id),
            };
        }
        return FocusPaneCandidateDecision::Candidate {
            pane_id,
            focused_reason: "focused_agent_doc_actor",
            not_alive_reason: "actor_pane_not_alive",
        };
    }

    if let Some(pane_id) = registry_pane {
        return FocusPaneCandidateDecision::Candidate {
            pane_id,
            focused_reason: "focused_durable_registry",
            not_alive_reason: "registry_pane_not_alive",
        };
    }

    FocusPaneCandidateDecision::Reject {
        reason: "missing_actor_record",
        pane_id: None,
    }
}

/// A `Blocked`/`Closed` *durable* actor projection must not veto a pure,
/// non-mutating UI focus handoff when the pane's live process tree still exactly
/// owns this document (a finished/blocked session whose pane is still open and
/// showing that document). Focus is non-mutating and the alive/visible guards
/// still apply downstream. Ownership is proven from the *live process tree*
/// (`pane_process_owner_document`), never the stale durable state, so a pane
/// reused by a different document stays refused (`actor_not_focusable`) and
/// cross-document focus steal cannot happen. Returns the rescued pane when the
/// reject is safe to override.
fn focus_reject_rescued_by_live_pane_owner<'a>(
    reject_reason: &str,
    pane_id: Option<&'a str>,
    pane_process_owner_document: Option<&str>,
    document_id: &str,
) -> Option<&'a str> {
    if reject_reason != "actor_not_focusable" {
        return None;
    }
    let pane = pane_id?;
    (pane_process_owner_document == Some(document_id)).then_some(pane)
}

fn current_document_session_id(
    canonical: &Path,
    actor_record: Option<&agent_doc_sqlite::state_store::ActorRecord>,
    registry_entry: Option<&tmux_router::RegistryEntry>,
) -> Option<String> {
    let frontmatter_session = std::fs::read_to_string(canonical).ok().and_then(|content| {
        let (frontmatter, _) = agent_doc_frontmatter::frontmatter::parse(&content).ok()?;
        frontmatter.session
    });
    frontmatter_session
        .or_else(|| registry_entry.map(|entry| entry.session_id.clone()))
        .or_else(|| actor_record.map(|record| record.session_id.clone()))
        .filter(|session_id| !session_id.trim().is_empty())
}

fn process_tree_exactly_owns_document(
    tmux: &tmux_router::Tmux,
    pane_id: &str,
    file: &Path,
) -> bool {
    let Some(pane_pid) = agent_doc_tmux_io::pane_pid(tmux, pane_id) else {
        return false;
    };
    agent_doc_process_owner_io::process_tree_has_agent_doc_owner_for_file(
        &pane_pid.to_string(),
        &file.to_string_lossy(),
    )
}

fn session_log_owner_signals_prove_focus(
    latest_session_open: bool,
    pane_owns_live_agent: bool,
    process_tree_exactly_owns_document: bool,
) -> bool {
    latest_session_open && pane_owns_live_agent && process_tree_exactly_owns_document
}

fn proven_session_log_focus_owner(
    tmux: &tmux_router::Tmux,
    file: &Path,
    session_id: &str,
) -> Option<String> {
    let status = agent_doc_supervisor_io::startup_miss::session_log_status(file, session_id)
        .ok()
        .flatten()?;
    let latest_session_open = status.latest_session_open();
    let pane_id = status.latest_start_pane?;
    if !session_log_owner_signals_prove_focus(
        latest_session_open,
        agent_doc_supervisor_process::session_liveness::pane_owns_live_agent(tmux, &pane_id),
        process_tree_exactly_owns_document(tmux, &pane_id, file),
    ) {
        return None;
    }
    Some(pane_id)
}

fn active_pane_process_owner_document(
    tmux: &tmux_router::Tmux,
    pane_id: &str,
    project_root: &Path,
) -> Option<String> {
    if !agent_doc_supervisor_process::session_liveness::pane_owns_live_agent(tmux, pane_id) {
        return None;
    }
    let pane_pid = agent_doc_tmux_io::pane_pid(tmux, pane_id)?;
    let raw_document =
        agent_doc_process_owner_io::process_tree_agent_doc_owner_document(&pane_pid.to_string())?;
    let raw_path = PathBuf::from(raw_document);
    let candidate = if raw_path.is_absolute() {
        raw_path
    } else {
        project_root.join(raw_path)
    };
    let canonical = candidate.canonicalize().ok()?;
    let owner_root = agent_doc_project_root_io::project_root_containing(&canonical)?;
    if owner_root != project_root {
        return None;
    }
    Some(agent_doc_session_actor_io::canonical_document_id_in(
        project_root,
        &canonical.to_string_lossy(),
    ))
}

pub(crate) fn handle_focus_document_pane(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<ControllerTmuxFocusReceipt> {
    let requested_file = request_file(&request)?;
    let canonical = canonical_controller_request_file(bootstrap, &requested_file);
    let document_id = agent_doc_session_actor_io::canonical_document_id_in(
        &bootstrap.project_root,
        &canonical.to_string_lossy(),
    );
    let conn = open_state_db(&bootstrap.project_root)?;
    let actor_record = load_actor_record_from_db(&conn, &document_id)?;
    let registry_entry =
        agent_doc_session_registry_io::lookup_file_entry_in(&bootstrap.project_root, &canonical)?;
    let tmux = tmux_router::Tmux::default_server();
    let session_id =
        current_document_session_id(&canonical, actor_record.as_ref(), registry_entry.as_ref());
    let proven_live_owner = session_id
        .as_deref()
        .and_then(|session_id| proven_session_log_focus_owner(&tmux, &canonical, session_id));
    let actor = actor_record.as_ref().map(|record| {
        let focusable = !matches!(
            record.state,
            agent_doc_sqlite::state_store::ActorState::Blocked
                | agent_doc_sqlite::state_store::ActorState::Closed
        );
        (record.pane_id.as_str(), focusable)
    });
    let decision = decide_focus_pane_candidate(
        actor,
        registry_entry.as_ref().map(|entry| entry.pane.as_str()),
        proven_live_owner.as_deref(),
    );
    let (pane_id, focused_reason, not_alive_reason) = match decision {
        FocusPaneCandidateDecision::Candidate {
            pane_id,
            focused_reason,
            not_alive_reason,
        } => (pane_id.to_string(), focused_reason, not_alive_reason),
        FocusPaneCandidateDecision::Reject { reason, pane_id } => {
            let pane_owner_document = pane_id.and_then(|pane| {
                active_pane_process_owner_document(&tmux, pane, &bootstrap.project_root)
            });
            if let Some(pane) = focus_reject_rescued_by_live_pane_owner(
                reason,
                pane_id,
                pane_owner_document.as_deref(),
                &document_id,
            ) {
                (
                    pane.to_string(),
                    "focused_live_process_owner",
                    "live_owner_pane_not_alive",
                )
            } else {
                return Ok(tmux_focus_receipt(
                    false,
                    reason,
                    Some(document_id),
                    pane_id.map(ToOwned::to_owned),
                    None,
                    None,
                    None,
                ));
            }
        }
    };
    if pane_id.is_empty() || !tmux.pane_alive(&pane_id) {
        return Ok(tmux_focus_receipt(
            false,
            not_alive_reason,
            Some(document_id),
            Some(pane_id),
            None,
            None,
            None,
        ));
    }
    let session_name = tmux
        .pane_session(&pane_id)
        .ok()
        .or_else(|| configured_tmux_session_for_project(&bootstrap.project_root));
    let Some(session_name_value) = session_name.clone() else {
        return Ok(tmux_focus_receipt(
            false,
            "missing_tmux_session",
            Some(document_id),
            Some(pane_id),
            None,
            None,
            None,
        ));
    };
    let (window_id, window_name, _active_pane) =
        active_tmux_window_for_session(&tmux, &session_name_value);
    if window_name.as_deref() != Some("agent-doc") {
        return Ok(tmux_focus_receipt(
            false,
            "outside_agent_doc_window",
            Some(document_id),
            Some(pane_id),
            Some(session_name_value),
            window_id,
            window_name,
        ));
    }
    let pane_window = tmux.pane_window(&pane_id).ok();
    if pane_window.as_deref() != window_id.as_deref() {
        return Ok(tmux_focus_receipt(
            false,
            "actor_pane_not_visible",
            Some(document_id),
            Some(pane_id),
            Some(session_name_value),
            window_id,
            window_name,
        ));
    }
    tmux.select_pane(&pane_id)?;
    Ok(tmux_focus_receipt(
        true,
        focused_reason,
        Some(document_id),
        Some(pane_id),
        Some(session_name_value),
        window_id,
        window_name,
    ))
}

pub(crate) fn handle_sync_tmux_layout(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<ControllerTmuxLayoutSyncReceipt> {
    if bootstrap.handoff_state != ControllerHandoffState::Stable {
        anyhow::bail!(
            "sync_tmux_layout refused: controller not authoritative (handoff_state={:?})",
            bootstrap.handoff_state
        );
    }
    let payload_json = request_string(&request.diagnostic_payload, "diagnostic_payload")?;
    let invocation: ControllerTmuxLayoutSyncInvocation =
        serde_json::from_str(&payload_json).context("parse sync tmux layout invocation")?;
    runtime_effects()?.sync_tmux_layout(&bootstrap.project_root, invocation)
}

pub(crate) fn rejected_admin_receipt(
    bootstrap: &ControllerBootstrap,
    operation_kind: &str,
    document_id: Option<&str>,
    failed_stage: &str,
    diagnostic_payload: &str,
    observed_generation: Option<u64>,
    current_generation: Option<u64>,
) -> Result<ControllerAdminReceipt> {
    let mut receipt = insert_admin_operation_record(
        &bootstrap.project_root,
        operation_kind,
        document_id,
        "rejected",
        Some(diagnostic_payload),
    )?;
    receipt.failed_stage = Some(failed_stage.to_string());
    receipt.unblock_hint =
        Some("inspect the actor and retry with the current observed generation".to_string());
    receipt.observed_generation = observed_generation;
    receipt.current_generation = current_generation;
    Ok(receipt)
}

pub(crate) fn require_observed_generation(
    bootstrap: &ControllerBootstrap,
    operation_kind: &str,
    document_id: Option<&str>,
    record: Option<&agent_doc_sqlite::state_store::ActorRecord>,
    observed_generation: Option<u64>,
    diagnostic_payload: &str,
) -> Result<Option<ControllerAdminReceipt>> {
    let Some(record) = record else {
        return Ok(None);
    };
    match observed_generation {
        None => rejected_admin_receipt(
            bootstrap,
            operation_kind,
            document_id,
            "missing_observed_generation",
            diagnostic_payload,
            None,
            Some(record.generation),
        )
        .map(Some),
        Some(observed) if observed != record.generation => rejected_admin_receipt(
            bootstrap,
            operation_kind,
            document_id,
            "stale_generation",
            diagnostic_payload,
            Some(observed),
            Some(record.generation),
        )
        .map(Some),
        Some(_) => Ok(None),
    }
}

pub(crate) fn handle_queue_control(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<ControllerAdminReceipt> {
    let action = request_string(&request.state, "queue control action")?;
    let control_state = match action.as_str() {
        "pause" | "paused" => "paused",
        "resume" | "resumed" => "resumed",
        "drain" | "draining" => "draining",
        other => anyhow::bail!("unknown queue control action: {other}"),
    };
    let operation_kind = format!("queue_{control_state}");
    let reason = request
        .reason
        .as_deref()
        .unwrap_or("operator queue control");
    let item_id = request.diagnostic_payload.as_deref();
    let diagnostic_payload = match item_id {
        Some(item_id) => format!("reason={reason} item_id={item_id}"),
        None => format!("reason={reason}"),
    };
    let (_target, document_id, record) = if request.file.is_some() {
        admin_target_record(bootstrap, &request)?
    } else {
        (bootstrap.project_root.display().to_string(), None, None)
    };
    if let Some(rejected) = require_observed_generation(
        bootstrap,
        &operation_kind,
        document_id.as_deref(),
        record.as_ref(),
        request.generation,
        &diagnostic_payload,
    )? {
        return Ok(rejected);
    }
    let receipt = insert_admin_operation_record(
        &bootstrap.project_root,
        &operation_kind,
        document_id.as_deref(),
        "accepted",
        Some(&diagnostic_payload),
    )?;
    let conn = open_state_db(&bootstrap.project_root)?;
    let scope_kind = if document_id.is_some() {
        "document"
    } else {
        "project"
    };
    let project_scope = bootstrap.project_root.to_string_lossy();
    let scope_id = document_id.as_deref().unwrap_or(project_scope.as_ref());
    let control = state_store::upsert_queue_control_in_db(
        &conn,
        &state_store::QueueControlInsert {
            scope_kind,
            scope_id,
            state: control_state,
            reason: Some(reason),
            operation_receipt_id: Some(receipt.receipt_id),
        },
    )?;
    let mut receipt = receipt;
    receipt.diagnostic_payload = Some(format!(
        "{diagnostic_payload} scope_kind={} scope_id={} control_receipt_id={}",
        control.scope_kind, control.scope_id, control.receipt_id
    ));
    Ok(receipt)
}

pub(crate) fn handle_admin_control(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<ControllerAdminReceipt> {
    let action = request_string(&request.state, "admin control action")?;
    let operation_kind = format!("admin_{action}");
    let reason = request
        .reason
        .as_deref()
        .unwrap_or("operator admin control");
    let diagnostic_payload = format!("reason={reason}");
    let (_target, document_id, record) = admin_target_record(bootstrap, &request)?;
    let Some(record) = record else {
        return rejected_admin_receipt(
            bootstrap,
            &operation_kind,
            document_id.as_deref(),
            "missing_actor",
            &diagnostic_payload,
            request.generation,
            None,
        );
    };
    if let Some(rejected) = require_observed_generation(
        bootstrap,
        &operation_kind,
        Some(&record.document_id),
        Some(&record),
        request.generation,
        &diagnostic_payload,
    )? {
        return Ok(rejected);
    }
    let receipt = insert_admin_operation_record(
        &bootstrap.project_root,
        &operation_kind,
        Some(&record.document_id),
        "accepted",
        Some(&diagnostic_payload),
    )?;
    let mut next = record.clone();
    match action.as_str() {
        "reap" => {
            next.state = agent_doc_sqlite::state_store::ActorState::Closed;
            next.pane_id.clear();
            next.window_id.clear();
            next.last_transition = agent_doc_sqlite::state_store::ActorLastTransition {
                caller: "admin".to_string(),
                reason: format!("manual_reap {reason} receipt_id={}", receipt.receipt_id),
                timestamp: timestamp_secs(),
                prior_generation: record.generation,
                new_generation: record.generation,
            };
        }
        "handoff" => {
            let to_pane = request_string(&request.pane_id, "to pane")?;
            next.generation = record.generation.saturating_add(1);
            next.pane_id = to_pane;
            next.state = agent_doc_sqlite::state_store::ActorState::Ready;
            next.last_transition = agent_doc_sqlite::state_store::ActorLastTransition {
                caller: "admin".to_string(),
                reason: format!("manual_handoff {reason} receipt_id={}", receipt.receipt_id),
                timestamp: timestamp_secs(),
                prior_generation: record.generation,
                new_generation: record.generation.saturating_add(1),
            };
        }
        other => anyhow::bail!("unknown admin control action: {other}"),
    }
    store_actor_record(&bootstrap.project_root, Some(record.generation), &next)?;
    Ok(receipt)
}

pub(crate) fn handle_projection_repair(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<ControllerAdminReceipt> {
    let projection = request.state.as_deref().unwrap_or("all").trim().to_string();
    let reason = request
        .reason
        .as_deref()
        .unwrap_or("operator projection repair");
    let diagnostic_payload = format!("projection={projection} reason={reason}");
    let (_target, document_id, record) = if request.file.is_some() {
        admin_target_record(bootstrap, &request)?
    } else {
        (bootstrap.project_root.display().to_string(), None, None)
    };
    if let Some(rejected) = require_observed_generation(
        bootstrap,
        "projection_repair",
        document_id.as_deref(),
        record.as_ref(),
        request.generation,
        &diagnostic_payload,
    )? {
        return Ok(rejected);
    }
    let receipt = insert_admin_operation_record(
        &bootstrap.project_root,
        "projection_repair",
        document_id.as_deref(),
        "accepted",
        Some(&diagnostic_payload),
    )?;
    repair_projection_from_controller_state(
        &bootstrap.project_root,
        &projection,
        document_id.as_deref(),
    )?;
    Ok(receipt)
}

pub(crate) fn repair_projection_from_controller_state(
    project_root: &Path,
    projection: &str,
    _document_id: Option<&str>,
) -> Result<()> {
    match projection {
        "all" | "layout" => {
            load_layout_state(project_root)?;
        }
        other => anyhow::bail!("unknown projection repair target: {other}"),
    }
    Ok(())
}

pub(crate) fn handle_attach_pane(
    bootstrap: &ControllerBootstrap,
    runtime: Option<&ControllerRuntime>,
    request: ControllerRequest,
) -> Result<agent_doc_sqlite::state_store::ActorRecord> {
    let file = request_file(&request)?;
    let session_id = request_string(&request.session_id, "session_id")?;
    let pane_id = request_string(&request.pane_id, "pane_id")?;
    let window_id = request_string(&request.window_id, "window_id")?;
    let document_id = agent_doc_session_actor_io::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let prior = actor_record_from_authority(bootstrap, runtime, &document_id)?;
    let prior_generation = prior.as_ref().map(|record| record.generation).unwrap_or(0);
    let generation = prior
        .as_ref()
        .filter(|record| record.pane_id == pane_id)
        .map(|record| record.generation)
        .unwrap_or_else(|| prior_generation.saturating_add(1).max(1));
    let harness = prior
        .as_ref()
        .map(|record| record.harness.clone())
        .filter(|harness| !harness.trim().is_empty())
        .unwrap_or_else(|| {
            agent_doc_session_actor_io::detect_document_harness_in(
                &bootstrap.project_root,
                &document_id,
            )
        });
    let record = agent_doc_sqlite::state_store::ActorRecord {
        document_id: document_id.clone(),
        session_id: session_id.clone(),
        generation,
        pane_id: pane_id.clone(),
        window_id: window_id.clone(),
        harness,
        state: agent_doc_sqlite::state_store::ActorState::Ready,
        last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
            caller: request.caller.as_deref().unwrap_or("session").to_string(),
            reason: request
                .reason
                .as_deref()
                .unwrap_or("manual_attach")
                .to_string(),
            timestamp: timestamp_secs(),
            prior_generation,
            new_generation: generation,
        },
    };
    let record = store_actor_record(&bootstrap.project_root, Some(prior_generation), &record)?;
    refresh_runtime_after_actor_write(runtime)?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "controller_attach_pane session={} pane={} generation={} state={}",
            session_id,
            pane_id,
            record.generation,
            record.state.as_str()
        ),
    );
    Ok(record)
}

pub(crate) fn handle_operator_command(
    bootstrap: &ControllerBootstrap,
    runtime: Option<&ControllerRuntime>,
    request: ControllerRequest,
) -> Result<DispatchAuthorization> {
    let file = request_file(&request)?;
    let command_kind = request_string(&request.command_kind, "command_kind")?;
    let diagnostic_payload = request
        .diagnostic_payload
        .as_deref()
        .unwrap_or_default()
        .to_string();
    let document_id = agent_doc_session_actor_io::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let Some(record) = actor_record_from_authority(bootstrap, runtime, &document_id)? else {
        let receipt = insert_dispatch_attempt_record(
            &bootstrap.project_root,
            ControllerDispatchReceiptInsert {
                document_id: &document_id,
                generation: 0,
                command_kind: &command_kind,
                accepted_stage: None,
                failed_stage: Some("missing_actor"),
                diagnostic_payload: &diagnostic_payload,
                result_status: ControllerDispatchResultStatus::Rejected,
                proof_scope: ControllerDispatchProofScope::AcceptedOnly,
                dispatch_start_proven: false,
            },
        )?;
        anyhow::bail!(
            "operator command `{}` rejected for {}: stage=missing_actor receipt_id={}",
            command_kind,
            file.display(),
            receipt.receipt_id
        );
    };
    // A `Closed` or `Blocked` actor is a valid target for recovery commands.
    // `session_clear` / `session_interrupt_clear` reset a closed/blocked session,
    // and `session_restart` SUPERSEDES it — blue/green drain-and-supersede
    // (#supkill-bg): a restart must not fail closed against a dead/closed or
    // stuck/blocked generation. The whole point of restart is to replace the
    // superseded generation with the next one, so a `Closed` actor is exactly when
    // restart is meaningful (the operator's `session restart-supervisor` hit
    // `generation N is closed` here). A `Blocked` actor (starting-timeout) is the
    // same kind of stuck state: rejecting the recovery command on exactly the
    // state that needs recovery is self-defeating (#clear-blocked-actor). Only
    // non-recovery commands on a `Closed` or `Blocked` actor still reject.
    let is_recovery_command = matches!(
        command_kind.as_str(),
        "session_clear" | "session_interrupt_clear" | "session_restart"
    );
    let recovers_terminal_actor = is_recovery_command
        && matches!(
            record.state,
            agent_doc_sqlite::state_store::ActorState::Closed
                | agent_doc_sqlite::state_store::ActorState::Blocked
        );
    if !recovers_terminal_actor
        && matches!(
            record.state,
            agent_doc_sqlite::state_store::ActorState::Blocked
                | agent_doc_sqlite::state_store::ActorState::Closed
        )
    {
        let failed_stage = record.state.as_str();
        let receipt = insert_dispatch_attempt_record(
            &bootstrap.project_root,
            ControllerDispatchReceiptInsert {
                document_id: &document_id,
                generation: record.generation,
                command_kind: &command_kind,
                accepted_stage: None,
                failed_stage: Some(failed_stage),
                diagnostic_payload: &diagnostic_payload,
                result_status: ControllerDispatchResultStatus::Blocked,
                proof_scope: ControllerDispatchProofScope::AcceptedOnly,
                dispatch_start_proven: false,
            },
        )?;
        anyhow::bail!(
            "operator command `{}` rejected for {}: generation {} is {} receipt_id={}",
            command_kind,
            file.display(),
            record.generation,
            failed_stage,
            receipt.receipt_id
        );
    }
    let accepted_stage = format!("operator_{}", record.state.as_str());
    let receipt = insert_dispatch_attempt_record(
        &bootstrap.project_root,
        ControllerDispatchReceiptInsert {
            document_id: &document_id,
            generation: record.generation,
            command_kind: &command_kind,
            accepted_stage: Some(&accepted_stage),
            failed_stage: None,
            diagnostic_payload: &diagnostic_payload,
            result_status: ControllerDispatchResultStatus::Accepted,
            proof_scope: ControllerDispatchProofScope::AcceptedOnly,
            dispatch_start_proven: false,
        },
    )?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "controller_operator_command_accepted kind={} session={} pane={} generation={} stage={} receipt_id={} proof_scope={}",
            command_kind,
            record.session_id,
            record.pane_id,
            record.generation,
            accepted_stage,
            receipt.receipt_id,
            receipt.proof_scope.as_str()
        ),
    );
    // #supkill-bg blue/green redirect proof: a `session_restart` that authorizes
    // against a `Closed` or `Blocked` actor is the supersede signal — record the
    // prior (closed/blocked) generation and the next generation the restart drains
    // toward so racing dispatch / log forensics can see the "superseded -> retry
    // against N+1" redirect instead of the old `generation N is closed` hard
    // reject.
    if command_kind == "session_restart"
        && matches!(
            record.state,
            agent_doc_sqlite::state_store::ActorState::Closed
                | agent_doc_sqlite::state_store::ActorState::Blocked
        )
    {
        agent_doc_ops_log_io::log_op(
            &file,
            &format!(
                "supervisor_restart_supersede file={} action=supersede_{}_actor prior_generation={} next_generation={} receipt_id={} caller=operator",
                file.display(),
                record.state.as_str(),
                record.generation,
                record.generation.saturating_add(1),
                receipt.receipt_id,
            ),
        );
    }
    Ok(DispatchAuthorization {
        record,
        accepted_stage,
        receipt,
    })
}

#[cfg(not(any(test, feature = "test-support")))]
const SUPERVISOR_REPLACEMENT_WAIT_SECS_ENV: &str = "AGENT_DOC_SUPERVISOR_REPLACEMENT_WAIT_SECS";
#[cfg(not(any(test, feature = "test-support")))]
const DEFAULT_SUPERVISOR_REPLACEMENT_WAIT_SECS: u64 = 20;
#[cfg(not(any(test, feature = "test-support")))]
const SUPERVISOR_REPLACEMENT_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
struct SupervisorReplacementWork {
    project_root: PathBuf,
    file: PathBuf,
    session_id: String,
    pane_id: String,
    generation: u64,
    mode: String,
    force: bool,
    operator_receipt_id: u64,
}

#[cfg(not(any(test, feature = "test-support")))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SupervisorReplacementIpcStatus {
    Accepted,
    Dead,
    Failed,
}

#[cfg(any(test, not(feature = "test-support")))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SupervisorReplacementPaneStartDecision {
    PreserveExisting,
    AutoStartNew,
    /// The pane runs THIS document's own harness. Quit it, then start under a
    /// supervisor — that is literally what "Restart Agent" means.
    RestartLiveHarness,
    /// The pane runs something else. Never touch it.
    BlockLiveNonShell,
}

/// `#restartlivepane`: refusing every live non-shell pane made "Restart Agent"
/// impossible on the one pane it is always aimed at — the document's own agent.
/// The operator explicitly asked to restart THIS document's harness, and
/// `--resume` (`#restartresume`) means relaunching costs the process, not the
/// conversation. So a pane running the document's own harness is restartable.
///
/// The guard that matters is kept, and narrowed rather than widened: a live pane
/// running anything ELSE is still untouchable. That covers a build, an editor, an
/// ssh session, or a harness bound to a different document — killing any of those
/// destroys unrelated work the operator never offered up.
///
/// Matching is deliberately EXACT against the resolved harness binary, not
/// `HarnessConfig::process_names` (which includes `node` and `agent-doc`): a
/// substring match there would make any Node process look like a restartable
/// agent. Related: `#bare-foreign-session-guard` — proving ownership, never
/// assuming it.
#[cfg(any(test, not(feature = "test-support")))]
fn supervisor_replacement_pane_start_decision(
    pane_alive: bool,
    current_command: Option<&str>,
    document_harness_binary: Option<&str>,
) -> SupervisorReplacementPaneStartDecision {
    if !pane_alive {
        return SupervisorReplacementPaneStartDecision::AutoStartNew;
    }
    let current_command = current_command
        .map(str::trim)
        .filter(|command| !command.is_empty());
    if current_command.is_some_and(agent_doc_tmux::pane_current_command_is_bare_shell) {
        return SupervisorReplacementPaneStartDecision::PreserveExisting;
    }
    let runs_own_harness = match (current_command, document_harness_binary) {
        (Some(command), Some(harness)) => {
            let harness = harness.trim();
            !harness.is_empty() && command.eq_ignore_ascii_case(harness)
        }
        _ => false,
    };
    if runs_own_harness {
        SupervisorReplacementPaneStartDecision::RestartLiveHarness
    } else {
        SupervisorReplacementPaneStartDecision::BlockLiveNonShell
    }
}

pub(crate) fn handle_supervisor_replacement(
    bootstrap: &ControllerBootstrap,
    runtime: Option<&ControllerRuntime>,
    request: ControllerRequest,
) -> Result<SupervisorReplacementReceipt> {
    let file = request_file(&request)?;
    let parsed = parse_supervisor_replacement_request(SupervisorReplacementRequestFields {
        state: request.state.as_deref(),
        reason: request.reason.as_deref(),
        diagnostic_payload: request.diagnostic_payload.as_deref(),
    })?;
    let mode = parsed.mode.as_str().to_string();
    let force = parsed.force;
    let authorization = handle_operator_command(
        bootstrap,
        runtime,
        ControllerRequest {
            command: "operator_command".to_string(),
            file: Some(file.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: request.caller.clone(),
            reason: request.reason.clone(),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("session_restart".to_string()),
            diagnostic_payload: Some(format!(
                "session supervisor replacement background mode={mode} force={force}"
            )),
        },
    )?;
    let record = authorization.record.clone();
    let work = SupervisorReplacementWork {
        project_root: bootstrap.project_root.clone(),
        file: file.clone(),
        session_id: record.session_id.clone(),
        pane_id: record.pane_id.clone(),
        generation: record.generation,
        mode: mode.clone(),
        force,
        operator_receipt_id: authorization.receipt.receipt_id,
    };
    let background_started = spawn_supervisor_replacement_worker(work)?;
    agent_doc_ops_log_io::log_op(
        &file,
        &format!(
            "controller_supervisor_replacement_accepted mode={} force={} session={} pane={} generation={} stage={} receipt_id={} background_started={}",
            mode,
            force,
            record.session_id,
            record.pane_id,
            record.generation,
            authorization.accepted_stage,
            authorization.receipt.receipt_id,
            background_started
        ),
    );
    Ok(SupervisorReplacementReceipt {
        record,
        accepted_stage: authorization.accepted_stage,
        operator_receipt: authorization.receipt,
        background_started,
        mode,
        force,
        session_id: authorization.record.session_id,
        pane_id: authorization.record.pane_id,
        generation: authorization.record.generation,
    })
}

#[cfg(any(test, feature = "test-support"))]
fn spawn_supervisor_replacement_worker(work: SupervisorReplacementWork) -> Result<bool> {
    agent_doc_ops_log_io::log_op(
        &work.file,
        &format!(
            "controller_supervisor_replacement_background_stub mode={} force={} session={} pane={} generation={} receipt_id={} project_root={}",
            work.mode,
            work.force,
            work.session_id,
            work.pane_id,
            work.generation,
            work.operator_receipt_id,
            work.project_root.display()
        ),
    );
    Ok(false)
}

#[cfg(not(any(test, feature = "test-support")))]
fn spawn_supervisor_replacement_worker(work: SupervisorReplacementWork) -> Result<bool> {
    std::thread::Builder::new()
        .name("agent-doc-supervisor-replacement".to_string())
        .spawn(move || {
            if let Err(err) = drive_supervisor_replacement_background(work.clone()) {
                agent_doc_ops_log_io::log_op(
                    &work.file,
                    &format!(
                        "controller_supervisor_replacement_background_failed session={} pane={} generation={} receipt_id={} error={err:?}",
                        work.session_id, work.pane_id, work.generation, work.operator_receipt_id
                    ),
                );
            }
        })
        .context("failed to spawn supervisor replacement background worker")?;
    Ok(true)
}

#[cfg(not(any(test, feature = "test-support")))]
fn drive_supervisor_replacement_background(work: SupervisorReplacementWork) -> Result<()> {
    let initial_pid = agent_doc_supervisor_io::process::supervisor_pid_for_doc(&work.file);
    let initial_host_stale = host_supervisor_stale_warning_for_doc(&work.file).is_some();
    let socket = agent_doc_supervisor_io::ipc::socket_path(&work.project_root, &work.session_id);
    agent_doc_ops_log_io::log_op(
        &work.file,
        &format!(
            "controller_supervisor_replacement_background_started mode={} force={} session={} pane={} generation={} receipt_id={} initial_pid={} initial_host_stale={} socket={}",
            work.mode,
            work.force,
            work.session_id,
            work.pane_id,
            work.generation,
            work.operator_receipt_id,
            initial_pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "none".to_string()),
            initial_host_stale,
            socket.display()
        ),
    );

    let ipc_status = request_supervisor_replacement_ipc(&work, &socket);
    let needs_escalation = match ipc_status {
        SupervisorReplacementIpcStatus::Accepted => work.force || initial_host_stale,
        SupervisorReplacementIpcStatus::Dead => true,
        SupervisorReplacementIpcStatus::Failed => work.force || initial_host_stale,
    };
    if !needs_escalation {
        agent_doc_ops_log_io::log_op(
            &work.file,
            &format!(
                "controller_supervisor_replacement_background_completed stage=ipc_only mode={} session={} pane={} generation={} receipt_id={} reason=fresh_supervisor_restart_accepted",
                work.mode, work.session_id, work.pane_id, work.generation, work.operator_receipt_id
            ),
        );
        return Ok(());
    }

    if ipc_status == SupervisorReplacementIpcStatus::Accepted
        && wait_for_supervisor_replacement_completion(&work.file, initial_pid, initial_host_stale)
    {
        agent_doc_ops_log_io::log_op(
            &work.file,
            &format!(
                "controller_supervisor_replacement_background_completed stage=ipc_reexec mode={} session={} pane={} generation={} receipt_id={} initial_host_stale={}",
                work.mode,
                work.session_id,
                work.pane_id,
                work.generation,
                work.operator_receipt_id,
                initial_host_stale
            ),
        );
        return Ok(());
    }

    agent_doc_ops_log_io::log_op(
        &work.file,
        &format!(
            "controller_supervisor_replacement_background_escalating mode={} force={} session={} pane={} generation={} receipt_id={} ipc_status={ipc_status:?} initial_host_stale={}",
            work.mode,
            work.force,
            work.session_id,
            work.pane_id,
            work.generation,
            work.operator_receipt_id,
            initial_host_stale
        ),
    );
    let kill_outcome = agent_doc_supervisor_io::selfkill::drive_supervisor_kill(
        &work.file,
        agent_doc_supervisor_io::selfkill::selfkill_grace(),
        false,
    )?;
    agent_doc_ops_log_io::log_op(
        &work.file,
        &format!(
            "controller_supervisor_replacement_kill_outcome mode={} session={} pane={} generation={} receipt_id={} outcome={kill_outcome:?}",
            work.mode, work.session_id, work.pane_id, work.generation, work.operator_receipt_id
        ),
    );
    reap_dead_supervisor_socket(&work.file, &socket);
    let pane = cold_start_supervisor_replacement(&work)?;
    agent_doc_ops_log_io::log_op(
        &work.file,
        &format!(
            "controller_supervisor_replacement_background_completed stage=cold_start mode={} session={} pane={} generation={} receipt_id={} replacement_pane={}",
            work.mode,
            work.session_id,
            work.pane_id,
            work.generation,
            work.operator_receipt_id,
            pane
        ),
    );
    Ok(())
}

#[cfg(not(any(test, feature = "test-support")))]
fn request_supervisor_replacement_ipc(
    work: &SupervisorReplacementWork,
    socket: &Path,
) -> SupervisorReplacementIpcStatus {
    if matches!(
        agent_doc_supervisor_io::ipc::probe_socket(socket),
        agent_doc_supervisor_io::ipc::SocketLiveness::Dead
    ) {
        agent_doc_ops_log_io::log_op(
            &work.file,
            &format!(
                "controller_supervisor_replacement_ipc_dead session={} pane={} generation={} receipt_id={} socket={}",
                work.session_id,
                work.pane_id,
                work.generation,
                work.operator_receipt_id,
                socket.display()
            ),
        );
        return SupervisorReplacementIpcStatus::Dead;
    }
    match agent_doc_supervisor_io::ipc::send_command(
        socket,
        &agent_doc_supervisor::ipc_protocol::IpcMethod::Restart {
            mode: work.mode.clone(),
        },
    ) {
        Ok(response) if response.ok => {
            agent_doc_ops_log_io::log_op(
                &work.file,
                &format!(
                    "controller_supervisor_replacement_ipc_accepted mode={} session={} pane={} generation={} receipt_id={} socket={}",
                    work.mode,
                    work.session_id,
                    work.pane_id,
                    work.generation,
                    work.operator_receipt_id,
                    socket.display()
                ),
            );
            SupervisorReplacementIpcStatus::Accepted
        }
        Ok(response) => {
            agent_doc_ops_log_io::log_op(
                &work.file,
                &format!(
                    "controller_supervisor_replacement_ipc_failed mode={} session={} pane={} generation={} receipt_id={} error={}",
                    work.mode,
                    work.session_id,
                    work.pane_id,
                    work.generation,
                    work.operator_receipt_id,
                    response
                        .error
                        .unwrap_or_else(|| "supervisor restart request failed".to_string())
                ),
            );
            SupervisorReplacementIpcStatus::Failed
        }
        Err(err) => {
            let status = if matches!(
                agent_doc_supervisor_io::ipc::probe_socket(socket),
                agent_doc_supervisor_io::ipc::SocketLiveness::Dead
            ) {
                SupervisorReplacementIpcStatus::Dead
            } else {
                SupervisorReplacementIpcStatus::Failed
            };
            agent_doc_ops_log_io::log_op(
                &work.file,
                &format!(
                    "controller_supervisor_replacement_ipc_error mode={} session={} pane={} generation={} receipt_id={} status={status:?} error={err:?}",
                    work.mode,
                    work.session_id,
                    work.pane_id,
                    work.generation,
                    work.operator_receipt_id
                ),
            );
            status
        }
    }
}

#[cfg(not(any(test, feature = "test-support")))]
fn wait_for_supervisor_replacement_completion(
    file: &Path,
    initial_pid: Option<u32>,
    initial_host_stale: bool,
) -> bool {
    let deadline = Instant::now() + supervisor_replacement_wait_timeout();
    while Instant::now() < deadline {
        let current_pid = agent_doc_supervisor_io::process::supervisor_pid_for_doc(file);
        if let (Some(initial), Some(current)) = (initial_pid, current_pid)
            && initial != current
        {
            return true;
        }
        if initial_pid.is_some() && current_pid.is_none() {
            return true;
        }
        if initial_host_stale && host_supervisor_stale_warning_for_doc(file).is_none() {
            return true;
        }
        std::thread::sleep(SUPERVISOR_REPLACEMENT_POLL_INTERVAL);
    }
    false
}

#[cfg(not(any(test, feature = "test-support")))]
fn supervisor_replacement_wait_timeout() -> Duration {
    let secs = std::env::var(SUPERVISOR_REPLACEMENT_WAIT_SECS_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_SUPERVISOR_REPLACEMENT_WAIT_SECS);
    Duration::from_secs(secs)
}

#[cfg(not(any(test, feature = "test-support")))]
fn reap_dead_supervisor_socket(file: &Path, socket: &Path) {
    if !matches!(
        agent_doc_supervisor_io::ipc::probe_socket(socket),
        agent_doc_supervisor_io::ipc::SocketLiveness::Dead
    ) || !socket.exists()
    {
        return;
    }
    match std::fs::remove_file(socket) {
        Ok(()) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "controller_supervisor_replacement_reaped_stale_socket socket={}",
                socket.display()
            ),
        ),
        Err(err) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "controller_supervisor_replacement_reap_stale_socket_failed socket={} error={err}",
                socket.display()
            ),
        ),
    }
}

/// Quit a live harness pane back to its shell so a supervisor can relaunch it.
///
/// Returns `false` if the pane never becomes a bare shell, so the caller can fail
/// closed instead of typing a shell command into a live agent.
#[cfg(not(any(test, feature = "test-support")))]
fn quit_live_harness_pane_to_shell(
    tmux: &tmux_router::Tmux,
    pane_id: &str,
    harness_binary: &str,
) -> bool {
    // `#restartbusyquit`: a mid-turn harness ignores `C-d`, so interrupt first.
    // Detected from the live capture rather than assumed, because interrupting an
    // IDLE composer is not always harmless (Codex `C-g`, stray `C-c`).
    let busy = agent_doc_tmux_io::capture_pane_with_ansi(tmux, pane_id)
        .or_else(|_| agent_doc_tmux_io::capture_pane(tmux, pane_id))
        .ok()
        .is_some_and(|captured| {
            agent_doc_harness::HarnessConfig::from_agent_name(harness_binary)
                .dispatch_blocker_reason(&captured)
                .is_some()
        });
    let plan = agent_doc_harness::operator_quit_key_plan_for_state(harness_binary, busy);
    for (index, key) in plan.iter().enumerate() {
        if index > 0 {
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        if tmux.send_keys_raw(pane_id, key).is_err() {
            return false;
        }
    }
    // Harness shutdown is not instant (flushing state, writing the session log),
    // so poll rather than sleeping one guessed interval.
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        if !tmux.pane_alive(pane_id) {
            // The pane closed outright; the caller's cold-start path handles this.
            return false;
        }
        if agent_doc_tmux_io::target_current_command(tmux, pane_id)
            .as_deref()
            .map(str::trim)
            .is_some_and(agent_doc_tmux::pane_current_command_is_bare_shell)
        {
            return true;
        }
    }
    false
}

#[cfg(not(any(test, feature = "test-support")))]
fn cold_start_supervisor_replacement(work: &SupervisorReplacementWork) -> Result<String> {
    let tmux = tmux_router::Tmux::default_server();
    // `#restartresume`: `restart-supervisor` defaults to continue-mode (`--fresh`
    // is the opt-out), but a replacement that has to (re)launch the harness was
    // dropping that mode and starting a brand-new conversation. That is the worst
    // possible moment to discard context: the operator is recovering, not starting
    // over. BOTH launch branches below must carry it — the preserve-existing-pane
    // branch is the COMMON one (a pane whose agent already exited is a bare shell),
    // so fixing only the cold-start branch leaves the usual path still lossy.
    let resume = (work.mode != "fresh").then_some(agent_doc_harness::ResumeRequest::Latest);
    if !work.pane_id.trim().is_empty() {
        let pane_alive = tmux.pane_alive(&work.pane_id);
        let current_command = if pane_alive {
            agent_doc_tmux_io::target_current_command(&tmux, &work.pane_id)
        } else {
            None
        };
        let document_harness = std::fs::read_to_string(&work.file)
            .ok()
            .and_then(|content| agent_doc_harness::document_harness_from_content(&content))
            .map(|name| agent_doc_harness::HarnessConfig::from_agent_name(&name).binary)
            .unwrap_or_else(|| agent_doc_harness::HarnessConfig::claude().binary);
        if matches!(
            supervisor_replacement_pane_start_decision(
                pane_alive,
                current_command.as_deref(),
                Some(document_harness.as_str()),
            ),
            SupervisorReplacementPaneStartDecision::RestartLiveHarness
        ) {
            // Quit the document's own harness so the pane falls back to its
            // shell, then take the normal preserve-existing start path below.
            // Bounded and fail-closed: if the pane does not become a shell we
            // refuse exactly as before rather than typing into a live agent.
            agent_doc_ops_log_io::log_op(
                &work.file,
                &format!(
                    "controller_supervisor_replacement_quitting_live_harness harness={} session={} pane={} generation={} receipt_id={}",
                    document_harness,
                    work.session_id,
                    work.pane_id,
                    work.generation,
                    work.operator_receipt_id,
                ),
            );
            if !quit_live_harness_pane_to_shell(&tmux, &work.pane_id, &document_harness) {
                agent_doc_ops_log_io::log_op(
                    &work.file,
                    &format!(
                        "controller_supervisor_replacement_preserve_pane_blocked mode={} session={} pane={} generation={} receipt_id={} reason=live_harness_quit_timeout harness={} (a mid-turn harness that ignores the quit keys reports here; see #restartbusyquit)",
                        work.mode,
                        work.session_id,
                        work.pane_id,
                        work.generation,
                        work.operator_receipt_id,
                        document_harness,
                    ),
                );
                anyhow::bail!(
                    "the {document_harness} session in pane {} did not exit to a shell, so the replacement supervisor was not started; quit it manually and retry",
                    work.pane_id
                );
            }
        }
        match supervisor_replacement_pane_start_decision(
            pane_alive,
            // Re-read: the quit above may have turned this into a bare shell.
            agent_doc_tmux_io::target_current_command(&tmux, &work.pane_id).as_deref(),
            Some(document_harness.as_str()),
        ) {
            SupervisorReplacementPaneStartDecision::PreserveExisting => {
                let agent_doc_bin = agent_doc_supervisor_process::agent_doc_start_bin();
                let project_root = agent_doc_project_root_io::project_root_containing(&work.file)
                    .or_else(|| work.file.parent().map(Path::to_path_buf))
                    .context(
                        "replacement supervisor document must have a project root or parent",
                    )?;
                let stderr_log =
                    agent_doc_supervisor_process::start_command::route_owned_stderr_log_path(
                        &project_root,
                    );
                let stderr_log_dir = stderr_log
                    .parent()
                    .context("replacement supervisor stderr path must include a logs directory")?;
                std::fs::create_dir_all(stderr_log_dir).with_context(|| {
                    format!(
                        "failed to prepare replacement supervisor stderr directory {}",
                        stderr_log_dir.display()
                    )
                })?;
                let start_cmd =
                    agent_doc_supervisor_process::start_command::route_owned_start_command_with_options(
                        &agent_doc_bin,
                        &work.file,
                        &agent_doc_supervisor_process::start_command::RouteOwnedStartOptions {
                            reap_policy:
                                agent_doc_supervisor::route_owned::RouteOwnedReapPolicy::Auto,
                            stderr_log: Some(&stderr_log),
                            resume: resume.clone(),
                        },
                    );
                agent_doc_ops_log_io::log_op(
                    &work.file,
                    &format!(
                        "controller_supervisor_replacement_preserve_pane mode={} resume={} session={} pane={}",
                        work.mode,
                        if resume.is_some() {
                            "continue"
                        } else {
                            "fresh"
                        },
                        work.session_id,
                        work.pane_id,
                    ),
                );
                agent_doc_tmux_io::input_diag::log_text_submit(
                    agent_doc_tmux_io::input_diag::InputDiagSink::new(
                        Some(&work.file),
                        agent_doc_ops_log_io::log_op,
                    ),
                    "controller.supervisor_replacement.cold_start_preserve_pane",
                    &format!("pane:{}", work.pane_id),
                    &start_cmd,
                    None,
                    "route_owned_start_enter",
                    "Enter",
                );
                agent_doc_tmux_io::send_submitted_text_logged(
                    &tmux,
                    &work.pane_id,
                    &start_cmd,
                    agent_doc_tmux_io::input_diag::InputDiagSink::new(
                        None,
                        agent_doc_ops_log_io::log_op,
                    ),
                    "sessions.send_submitted_text",
                )
                .with_context(|| {
                    format!(
                        "failed to submit replacement supervisor start command into pane {}",
                        work.pane_id
                    )
                })?;
                return Ok(work.pane_id.clone());
            }
            SupervisorReplacementPaneStartDecision::BlockLiveNonShell => {
                let current_command = current_command
                    .as_deref()
                    .map(str::trim)
                    .filter(|command| !command.is_empty())
                    .unwrap_or("unknown");
                agent_doc_ops_log_io::log_op(
                    &work.file,
                    &format!(
                        "controller_supervisor_replacement_preserve_pane_blocked mode={} session={} pane={} generation={} receipt_id={} reason=live_non_shell_pane current_command={}",
                        work.mode,
                        work.session_id,
                        work.pane_id,
                        work.generation,
                        work.operator_receipt_id,
                        current_command
                    ),
                );
                // Do NOT advertise `--force` here: this decision takes no force
                // flag (`--force` only affects the earlier IPC-escalation choice),
                // so suggesting it sends the operator in a circle. `#restartlivepane`
                // already auto-restarts a pane running the document's OWN harness;
                // reaching this arm means the pane is running something else, and
                // the honest advice is to free it or point the document elsewhere.
                anyhow::bail!(
                    "refusing to start a replacement supervisor in pane {}: it is running `{current_command}`, which is not {}'s harness. Quit that program (or claim the document to another pane with `agent-doc claim {}`) and retry.",
                    work.pane_id,
                    work.file.display(),
                    work.file.display()
                );
            }
            SupervisorReplacementPaneStartDecision::RestartLiveHarness => {
                // The quit above reported the pane back at a shell, so re-reading
                // it must not still show the harness. If it does, something else
                // reclaimed the pane between the two reads — fail closed rather
                // than quitting twice or typing into whatever is now running.
                anyhow::bail!(
                    "pane {} is still running {document_harness} after it reported exiting; refusing to start a replacement supervisor into it",
                    work.pane_id
                );
            }
            SupervisorReplacementPaneStartDecision::AutoStartNew => {}
        }
    }
    let file_str = work.file.to_string_lossy().to_string();
    // `#restartresume`: `restart-supervisor` defaults to continue-mode (`--fresh`
    // is the opt-out), but when there is no live supervisor to restart we escalate
    // to a COLD START — and a cold start with no resume intent silently downgrades
    // that promise to a brand-new conversation. That is the worst possible moment
    // to discard context: the supervisor is already dead, so the operator is
    // recovering, not starting over. Carry the mode into the cold start.
    agent_doc_ops_log_io::log_op(
        &work.file,
        &format!(
            "controller_supervisor_replacement_cold_start mode={} resume={} session={} pane={}",
            work.mode,
            if resume.is_some() {
                "continue"
            } else {
                "fresh"
            },
            work.session_id,
            work.pane_id,
        ),
    );
    runtime_effects()?
        .route_auto_start(&tmux, &work.file, &work.session_id, &file_str, None, resume)
        .with_context(|| {
            format!(
                "failed to cold-start replacement supervisor for {}",
                work.file.display()
            )
        })
}

pub(crate) fn handle_admin_operation(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<ControllerAdminReceipt> {
    let operation_kind = request_string(&request.command_kind, "command_kind")?;
    let status = request.state.as_deref().unwrap_or("accepted");
    let document_id = request.file.as_ref().map(|file| {
        agent_doc_session_actor_io::canonical_document_id_in(
            &bootstrap.project_root,
            &file.to_string_lossy(),
        )
    });
    insert_admin_operation_record(
        &bootstrap.project_root,
        &operation_kind,
        document_id.as_deref(),
        status,
        request.diagnostic_payload.as_deref(),
    )
}

pub fn run_status(root: Option<&Path>, ensure: bool) -> Result<()> {
    let project_root = agent_doc_project_root_io::project_root_from_arg(root)?;
    if ensure {
        ensure_controller_running(&project_root, LaunchMode::Lazy)?;
    }
    println!("{}", serde_json::to_string_pretty(&status(&project_root)?)?);
    Ok(())
}

pub fn run_serve(
    root: Option<&Path>,
    launch_mode: &str,
    listen_socket: Option<&Path>,
    controller_generation: Option<u64>,
    previous_controller_pid: Option<u32>,
    handoff_state: &str,
) -> Result<()> {
    let project_root = agent_doc_project_root_io::project_root_from_arg(root)?;
    let closed_fds = sanitize_controller_serve_inherited_fds();
    if closed_fds > 0 {
        agent_doc_ops_log_io::log_op(
            &project_root,
            &format!("controller_serve_inherited_fds_closed count={closed_fds}"),
        );
    }
    serve_with_options(
        &project_root,
        LaunchMode::parse(launch_mode)?,
        listen_socket.map(Path::to_path_buf),
        controller_generation,
        previous_controller_pid,
        status::parse_handoff_state(handoff_state)?,
    )
}

pub fn run_launch_detached(
    root: Option<&Path>,
    launch_mode: &str,
    listen_socket: Option<&Path>,
    controller_generation: Option<u64>,
    previous_controller_pid: Option<u32>,
    handoff_state: &str,
) -> Result<()> {
    let project_root = agent_doc_project_root_io::project_root_from_arg(root)?;
    launch_detached_at(
        &project_root,
        LaunchMode::parse(launch_mode)?,
        listen_socket,
        controller_generation,
        previous_controller_pid,
        status::parse_handoff_state(handoff_state)?,
    )
}

#[cfg(all(unix, not(test)))]
fn sanitize_controller_serve_inherited_fds() -> usize {
    let fds = controller_serve_inherited_fds_to_close()
        .unwrap_or_else(|| (3..inherited_fd_close_limit()).collect());
    let mut closed = 0usize;
    for fd in fds {
        if fd <= 2 {
            continue;
        }
        if unsafe { libc::close(fd) } == 0 {
            closed += 1;
        }
    }
    closed
}

#[cfg(all(unix, not(test)))]
fn controller_serve_inherited_fds_to_close() -> Option<Vec<i32>> {
    let entries = std::fs::read_dir("/proc/self/fd").ok()?;
    let mut fds = Vec::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(fd) = name.parse::<i32>() else {
            continue;
        };
        if fd > 2 {
            fds.push(fd);
        }
    }
    Some(fds)
}

#[cfg(any(not(unix), test))]
fn sanitize_controller_serve_inherited_fds() -> usize {
    0
}

pub fn run_shutdown(root: Option<&Path>) -> Result<()> {
    let project_root = agent_doc_project_root_io::project_root_from_arg(root)?;
    println!(
        "{}",
        request_with_reason(&project_root, "shutdown", "operator_shutdown")?
    );
    Ok(())
}

/// Outcome of a controller restart (`#cpcrestart`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct ControllerRestartOutcome {
    /// Controller PIDs verified same-project and reaped (SIGTERM → SIGKILL).
    pub reaped_pids: Vec<u32>,
    /// Whether a bounded graceful `shutdown` RPC was accepted before the reap.
    pub graceful: bool,
    /// Whether the caller forced the signal reap without the graceful RPC.
    pub force: bool,
}

/// `#cpcrestart` — restart/recycle the current project controller out-of-band.
///
/// The CP is the live state authority; a restart is only for cold-start recovery
/// (recycle onto a new binary, or recover from a bug that wedged it). Because a
/// spin-wedged controller stops servicing RPCs (`shutdown` times out), the reap
/// signals the controller PID(s) directly (SIGTERM → 750ms → SIGKILL, via
/// [`reap_verified_controller_pid`], which is guarded by
/// [`is_same_project_controller_pid`] so it never touches this process or another
/// project's controller). Route-owned document state is durably checkpointed
/// first so the fresh controller rebuilds its model from disk. With
/// `--launch-mode lazy` the next request relaunches a fresh controller, so no
/// explicit relaunch is issued here.
///
/// `force == false` first attempts a bounded graceful `shutdown` RPC (clean exit
/// when the controller is healthy); `force == true` skips straight to the signal
/// reap — the correct path for a spin-wedged / RPC-unreachable controller.
pub fn force_restart_controller(
    project_root: &Path,
    force: bool,
) -> Result<ControllerRestartOutcome> {
    // Best-effort durable snapshot so the fresh controller recovers in-flight
    // route-owned document state. Never block the restart on a checkpoint failure.
    if let Err(err) =
        checkpoint_route_owned_documents_for_project(project_root, "controller_restart_request")
    {
        eprintln!("[controller] restart: durable checkpoint warning: {err:#}");
    }

    let mut graceful = false;
    if !force {
        // A healthy controller exits cleanly here; a wedged one times out and we
        // fall through to the signal reap below (bounded inside `request`).
        graceful = request_with_reason(project_root, "shutdown", "controller_restart").is_ok();
    }

    let self_pid = std::process::id();
    let mut reaped = Vec::new();
    for pid in crate::process::project_controller_pids(project_root) {
        if pid == self_pid {
            continue;
        }
        reap_verified_controller_pid(project_root, pid, 0);
        if !process_is_alive(pid) {
            reaped.push(pid);
        }
    }
    reaped.sort_unstable();
    reaped.dedup();

    agent_doc_ops_log_io::log_op(
        project_root,
        &format!(
            "controller_restart_requested project_root={} force={} graceful={} reaped={:?} (#cpcrestart)",
            project_root.display(),
            force,
            graceful,
            reaped,
        ),
    );

    Ok(ControllerRestartOutcome {
        reaped_pids: reaped,
        graceful,
        force,
    })
}

pub fn run_restart(root: Option<&Path>, force: bool) -> Result<()> {
    let project_root = agent_doc_project_root_io::project_root_from_arg(root)?;
    let outcome = force_restart_controller(&project_root, force)?;
    println!("{}", serde_json::to_string_pretty(&outcome)?);
    if outcome.reaped_pids.is_empty() && !outcome.graceful {
        println!(
            "[controller] no live project controller to restart; a lazy launcher starts a fresh one on the next request"
        );
    } else {
        println!(
            "[controller] restarted: reaped {} controller pid(s); a fresh controller launches on the next request",
            outcome.reaped_pids.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    // `rusqlite` is a dev-dependency: these tests open the controller state DB
    // directly to assert the schema/rows the seam writes. `Connection` is the
    // `state_store` re-export already in scope via `super::*`.
    use agent_doc_sqlite::state_store::{load_actor_transitions_from_db, sqlite_i64};
    use lazily::DurableOutbox;

    #[test]
    fn the_editor_ack_profile_reports_legs_it_can_derive_and_omits_the_rest() {
        // #ackeditorstamps: the whole point is attribution, so each leg must be
        // separately readable — a total would not distinguish a late delivery
        // from a slow apply from a slow receipt.
        let profile = render_editor_ack_profile(Some(1_000), Some(1_400), Some(1_450), 1_600)
            .expect("fully stamped ack renders a profile");
        assert_eq!(
            profile,
            "received_to_applied=400ms applied_to_receipt=50ms receipt_to_observed=150ms received_to_observed=600ms"
        );

        // A self-echo ACK never crosses the buffer, so its middle stamp is absent.
        // The legs that touch it must be omitted, NOT computed against 0 — an
        // epoch-relative delta would report a ~55-year apply.
        let profile = render_editor_ack_profile(Some(1_000), None, Some(1_050), 1_100)
            .expect("partially stamped ack still bounds the editor half");
        assert_eq!(
            profile, "receipt_to_observed=50ms received_to_observed=100ms",
            "unstamped endpoints must drop their legs, not fabricate them"
        );

        // An un-updated replica sends no stamps at all: emit nothing rather than
        // a line whose only content is the controller's own clock.
        assert!(render_editor_ack_profile(None, None, None, 1_100).is_none());

        // Two processes, two clocks. A negative leg is real information about
        // skew; clamping it to 0 would read as "instant" and mislead the next
        // person profiling this path.
        let profile = render_editor_ack_profile(Some(2_000), Some(1_000), None, 3_000)
            .expect("skewed stamps still render");
        assert!(
            profile.contains("received_to_applied=skew"),
            "backwards leg must be named, not clamped: {profile}"
        );
    }

    #[test]
    fn native_reload_policy_requires_an_explicit_safe_adapter() {
        assert_eq!(
            editor_native_reload_policy("vscode-123-uuid", &[]),
            EditorNativeReloadPolicy::HotReload
        );
        assert_eq!(
            editor_native_reload_policy(
                "jetbrains-123-uuid",
                &["native_hot_reload_generation_v1".to_string()],
            ),
            EditorNativeReloadPolicy::HotReload
        );
        assert_eq!(
            editor_native_reload_policy("jetbrains-123-uuid", &[]),
            EditorNativeReloadPolicy::RestartRequired
        );
        assert_eq!(
            editor_native_reload_policy("", &[]),
            EditorNativeReloadPolicy::RestartRequired
        );
        assert_eq!(
            editor_native_reload_policy("future-editor-123", &[]),
            EditorNativeReloadPolicy::RestartRequired
        );
    }

    /// `#ctrlkillreregister` — the Tier 1 fan-out retires **per peer**, off the same
    /// replicated registration set the desired set is derived from.
    ///
    /// The point of asserting it here is that there is no flag day and no version
    /// handshake: a current plugin stops being pushed at the moment its registration
    /// converges, while an old plugin in another IDE keeps the compatibility push.
    #[test]
    fn restart_fan_out_skips_peers_that_pull_their_own_missing_replicas() {
        let registration =
            |capabilities: Vec<String>| agent_doc_reliable_sync_io::liveness::EditorRegistration {
                document_hash: "doc".into(),
                pid: 42,
                path: "/proj/plan.md".into(),
                editor_id: "jetbrains-42".into(),
                editor_kind: "jetbrains".into(),
                editor_version: "0.2.283".into(),
                capabilities,
                timestamp_ms: 1,
            };

        assert!(
            !peer_repairs_itself(&registration(vec![])),
            "a plugin predating the pull still needs the compatibility push"
        );
        assert!(
            !peer_repairs_itself(&registration(vec![
                "operator_text_authority_v1".to_string()
            ])),
            "an unrelated capability must not retire the push"
        );
        assert!(
            peer_repairs_itself(&registration(vec![
                "operator_text_authority_v1".to_string(),
                agent_doc_document_realtime::editor_contract::PEER_REPLICA_PULL_CAPABILITY
                    .to_string(),
            ])),
            "a peer that asks about itself must not also be pushed at"
        );
    }

    /// The pull's answer is scoped by what THIS controller can serve, not by what the
    /// asking editor believes it holds.
    ///
    /// A stranded editor lists its stale forwarders as `held`, so a `held`-only
    /// derivation returns nothing exactly when the editor most needs an answer. With
    /// no hub for the path, the controller cannot serve it and the registration is
    /// reported.
    #[test]
    fn controller_serves_replica_is_false_without_a_process_local_hub() {
        assert!(
            !controller_serves_replica("/nonexistent/never-registered.md"),
            "a path this process holds no relay for is not served here"
        );
    }

    #[test]
    fn editor_route_layout_args_preserve_empty_column_placeholders() {
        let args = vec![
            "--col".to_string(),
            String::new(),
            "--col".to_string(),
            "/repo/acadian-take-home.md".to_string(),
            "--focus".to_string(),
            "/repo/acadian-take-home.md".to_string(),
        ];

        assert_eq!(validate_editor_route_layout_args(&args).unwrap(), args);
    }

    #[test]
    fn editor_route_layout_args_reject_other_empty_values() {
        let empty_focus = vec!["--focus".to_string(), String::new()];
        assert!(validate_editor_route_layout_args(&empty_focus).is_err());

        let whitespace_column = vec!["--col".to_string(), "   ".to_string()];
        assert!(validate_editor_route_layout_args(&whitespace_column).is_err());
    }

    #[test]
    fn mutating_rpc_binary_guard_covers_dispatch_and_compact_callers() {
        let current = identity_version();
        assert_eq!(stale_mutating_client_binary(None), None);
        assert_eq!(stale_mutating_client_binary(Some(&current)), None);
        assert_eq!(
            stale_mutating_client_binary(Some("definitely-stale")),
            Some("definitely-stale")
        );
    }

    #[test]
    fn controller_model_pressure_state_quiets_all_project_idle_watchers() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let first = dir.path().join("first.md");
        let second = dir.path().join("second.md");
        std::fs::write(&first, "# first\n").unwrap();
        std::fs::write(&second, "# second\n").unwrap();

        assert!(!controller_model_pressure_cooldown_active_for_doc(&first));
        record_controller_model_pressure(
            dir.path(),
            &first,
            "idle_watch_test",
            "controller_model_backpressure",
        );

        assert!(controller_model_pressure_cooldown_active_for_doc(&first));
        assert!(controller_model_pressure_cooldown_active_for_doc(&second));
        let deadline = read_controller_model_pressure_deadline(dir.path())
            .unwrap()
            .unwrap();
        assert!(deadline >= controller_model_pressure_now_secs());
    }

    #[test]
    fn controller_model_pressure_state_is_not_extended_on_every_retry() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "# doc\n").unwrap();
        let retained = controller_model_pressure_now_secs() + 25;
        let conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
            &conn,
            CONTROLLER_MODEL_PRESSURE_STATE_KEY,
            &retained.to_string(),
            controller_model_pressure_now_secs() * 1000,
        )
        .unwrap();

        record_controller_model_pressure(dir.path(), &doc, "retry", "still busy");

        assert_eq!(
            read_controller_model_pressure_deadline(dir.path()).unwrap(),
            Some(retained),
            "active state must not churn the ledger and ops.log on every retry"
        );
    }

    #[test]
    fn realtime_steering_event_carries_the_full_crdt_aggregate() {
        let document = |tail: &str| {
            format!(
                "---\nagent_doc_format: template\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- agent:boundary:base -->\n{tail}<!-- /agent:exchange -->\n"
            )
        };
        let baseline = document("");
        let current = document("❯ First live edit\n\n❯ Second live edit\n");
        let event = realtime_steering_event_for_text("doc", "cycle", &baseline, &current);

        let agent_doc_state_backbone::StateFact::RealtimeSteeringObserved {
            cycle_id,
            steering,
            content_hash,
            ..
        } = &event.fact
        else {
            panic!("expected realtime steering fact");
        };
        assert_eq!(cycle_id, "cycle");
        assert_eq!(steering.count, 2);
        assert!(steering.verbatim.as_deref().is_some_and(
            |body| body.contains("First live edit") && body.contains("Second live edit")
        ));
        assert_eq!(content_hash, &agent_doc_hash::content_hash(&current));
        assert!(event.event_id.ends_with(content_hash));
    }

    #[test]
    fn supervisor_auto_install_waits_for_clean_committed_checkout() {
        let dir = tempfile::TempDir::new().unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init"]);
        git(&["config", "user.email", "agent-doc-test@example.invalid"]);
        git(&["config", "user.name", "agent-doc test"]);
        std::fs::write(dir.path().join("source.rs"), "stable\n").unwrap();
        git(&["add", "source.rs"]);
        git(&["commit", "-m", "stable"]);

        assert!(supervisor_auto_install_worktree_clean(dir.path()).unwrap());

        std::fs::write(dir.path().join("source.rs"), "typing\n").unwrap();
        assert!(!supervisor_auto_install_worktree_clean(dir.path()).unwrap());

        git(&["restore", "source.rs"]);
        std::fs::write(dir.path().join("new-source.rs"), "uncommitted\n").unwrap();
        assert!(!supervisor_auto_install_worktree_clean(dir.path()).unwrap());
    }

    fn test_controller_binary_identity() -> ControllerBinaryIdentity {
        ControllerBinaryIdentity {
            path: PathBuf::from("/tmp/agent-doc"),
            version: "test".to_string(),
            len: 123,
            modified_secs: 456,
            modified_nanos: 789,
        }
    }

    fn active_controller_status_with_handoff_state(
        handoff_state: ControllerHandoffState,
    ) -> ControllerStatus {
        ControllerStatus {
            active: true,
            project_root: PathBuf::from("/tmp/project"),
            socket_path: PathBuf::from("/tmp/project/.agent-doc/controller.sock"),
            launch_mode: Some(LaunchMode::Lazy),
            bootstrap_epoch: Some(1),
            pid: Some(42),
            controller_binary: Some(test_controller_binary_identity()),
            controller_generation: Some(7),
            handoff_state: Some(handoff_state),
            handoff_started_at: Some(1),
            previous_controller_pid: None,
            stale_duplicate_pids: Vec::new(),
            freshness: None,
            control_plane: status::default_control_plane_status(),
        }
    }

    fn command_submit_request_for_test(
        file: Option<PathBuf>,
        name: &str,
        payload_type: &str,
        payload: serde_json::Value,
        command_id: &str,
    ) -> ControllerRequest {
        let payload_bytes = payload.to_string().into_bytes();
        let submit = lazily::CommandSubmit {
            command_id: command_id.to_string(),
            causation_id: command_id.to_string(),
            source: "test-plugin".to_string(),
            target: "project-controller".to_string(),
            namespace: "agent-doc".to_string(),
            name: name.to_string(),
            authority_generation: 0,
            idempotency_key: format!("test:{name}"),
            deadline_ms: 5_000,
            policy: lazily::CommandPolicy {
                dedupe: lazily::DedupePolicy::SameIdempotencyKey,
                supersede: true,
                cancel_on_preempt: true,
            },
            payload_type: payload_type.to_string(),
            payload_hash: "sha256:test".to_string(),
            payload: lazily::IpcValue::Inline(payload_bytes),
            required_features: vec!["causal-receipts".to_string(), "command-events".to_string()],
        };
        let message = lazily::CommandMessage::CommandSubmit(Box::new(submit));
        ControllerRequest {
            command: "editor_command_submit".to_string(),
            file,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(serde_json::to_string(&message).unwrap()),
        }
    }

    #[test]
    fn command_submit_dispatches_sync_tmux_layout_payload() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let bootstrap = test_bootstrap(&dir);
        let request = command_submit_request_for_test(
            None,
            "sync_tmux_layout",
            "agent-doc.sync_tmux_layout.v1",
            serde_json::json!({
                "project_root": dir.path().display().to_string(),
                "columns": ["tasks/one.md,tasks/two.md"],
                "focus": null,
                "no_autostart": false,
                "exact_visible": true,
                "caller_kind": "manual"
            }),
            "cmd-sync",
        );

        let response = handle_editor_command_submit_rpc(&bootstrap, request).unwrap();
        assert_eq!(response["exit_code"], 0);
        assert_eq!(response["payload"]["reason"], "test_runtime");
        assert_eq!(
            response["payload"]["columns"][0],
            "tasks/one.md,tasks/two.md"
        );
        assert_eq!(response["projection"]["commands"][0]["status"], "applied");
        assert_eq!(response["projection"]["commands"][0]["terminal"], true);
    }

    #[test]
    fn command_submit_selected_text_uses_the_plain_editor_route() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "# Plan\n").unwrap();
        let bootstrap = test_bootstrap(&dir);
        let selected = "Preserve this\n  exact selection  ";
        let request = command_submit_request_for_test(
            Some(file),
            "editor_route",
            "agent-doc.editor_route.v1",
            serde_json::json!({
                "relative_path": "plan.md",
                "dispatch_only": true,
                "plain_trigger": true,
                "layout_args": [],
                "selected_text": selected,
                "steering_id": "steer-exact-1",
            }),
            "cmd-steer",
        );

        let response = handle_editor_command_submit_rpc(&bootstrap, request).unwrap();
        assert_eq!(response["exit_code"], 0);
        assert!(
            response["payload"]["output"]
                .as_str()
                .unwrap()
                .contains("test editor route accepted")
        );
        assert!(response["payload"]["steering"].is_null());
        assert_eq!(response["projection"]["commands"][0]["status"], "applied");
    }

    #[test]
    fn async_command_submit_admits_sync_tmux_layout_without_terminal_wait() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let bootstrap = test_bootstrap(&dir);
        let request = command_submit_request_for_test(
            None,
            "sync_tmux_layout",
            "agent-doc.sync_tmux_layout.v1",
            serde_json::json!({
                "project_root": dir.path().display().to_string(),
                "columns": ["tasks/one.md"],
                "focus": null,
                "no_autostart": false,
                "exact_visible": true,
                "caller_kind": "manual"
            }),
            "cmd-sync-async",
        );

        let response = handle_editor_command_submit_async_rpc(&bootstrap, request).unwrap();
        assert_eq!(response["exit_code"], 0);
        assert_eq!(response["payload"]["accepted"], true);
        assert_eq!(response["payload"]["command"], "sync_tmux_layout");
        assert_eq!(response["projection"]["commands"][0]["status"], "accepted");
        assert_eq!(response["projection"]["commands"][0]["terminal"], false);
        assert!(response["receipt"].is_null());

        // The test-support worker runs inline, so the completion channel is
        // already terminal by the time admission returns.
        let status = handle_editor_command_status_rpc(ControllerRequest {
            command: "editor_command_status".to_string(),
            file: None,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(
                serde_json::json!({ "command_id": "cmd-sync-async" }).to_string(),
            ),
        })
        .unwrap();
        assert_eq!(status["exit_code"], 0);
        assert_eq!(status["projection"]["commands"][0]["status"], "applied");
        assert_eq!(status["projection"]["commands"][0]["terminal"], true);
        assert!(!status["receipt"].is_null());
    }

    #[test]
    fn async_command_submit_rejects_unsupported_editor_commands_before_admission() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let bootstrap = test_bootstrap(&dir);
        let request = command_submit_request_for_test(
            None,
            "unknown_command",
            "agent-doc.unknown_command.v1",
            serde_json::json!({}),
            "cmd-unknown-async",
        );

        let err = handle_editor_command_submit_async_rpc(&bootstrap, request).unwrap_err();
        assert!(format!("{err:#}").contains("unsupported async editor command"));
    }

    #[test]
    fn async_command_submit_accepts_well_formed_editor_route_payload() {
        let request = command_submit_request_for_test(
            Some(PathBuf::from("plan.md")),
            "editor_route",
            "agent-doc.editor_route.v1",
            serde_json::json!({
                "relative_path": "plan.md",
                "dispatch_only": true,
                "plain_trigger": true,
                "wait_for_ready_secs": 15,
                "layout_args": [],
                "attempt_id": "attempt-1",
                "route_key": "route-1"
            }),
            "cmd-route-async",
        );
        let (submit, payload_json) = parse_editor_command_submit_request(&request).unwrap();
        validate_async_editor_command_payload(&submit, &payload_json).unwrap();
    }

    #[test]
    fn editor_command_status_rejects_unknown_completion_id() {
        let request = ControllerRequest {
            command: "editor_command_status".to_string(),
            file: None,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(
                serde_json::json!({ "command_id": "cmd-does-not-exist" }).to_string(),
            ),
        };
        let err = handle_editor_command_status_rpc(request).unwrap_err();
        assert!(format!("{err:#}").contains("unknown or expired async editor command"));
    }

    #[test]
    fn command_submit_dispatches_focus_document_pane_payload() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let doc = dir.path().join("tasks/one.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: one\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let request = command_submit_request_for_test(
            Some(doc.clone()),
            "focus_document_pane",
            "agent-doc.focus_document_pane.v1",
            serde_json::json!({
                "project_root": dir.path().display().to_string(),
                "document_path": doc.display().to_string(),
                "no_promotion": true,
                "active_window_guard": true
            }),
            "cmd-focus",
        );

        let response = handle_editor_command_submit_rpc(&bootstrap, request).unwrap();
        assert_eq!(response["exit_code"], 0);
        assert_eq!(response["payload"]["focused"], false);
        assert_eq!(response["payload"]["reason"], "missing_actor_record");
        assert_eq!(response["projection"]["commands"][0]["status"], "applied");
    }

    #[test]
    fn focus_candidate_uses_exact_live_owner_when_actor_projection_is_closed() {
        assert_eq!(
            decide_focus_pane_candidate(Some(("%stale", false)), None, Some("%live")),
            FocusPaneCandidateDecision::Candidate {
                pane_id: "%live",
                focused_reason: "focused_live_process_owner",
                not_alive_reason: "live_owner_pane_not_alive",
            },
        );
    }

    #[test]
    fn focus_candidate_uses_exact_live_owner_when_actor_and_registry_are_missing() {
        assert_eq!(
            decide_focus_pane_candidate(None, None, Some("%busy-live")),
            FocusPaneCandidateDecision::Candidate {
                pane_id: "%busy-live",
                focused_reason: "focused_live_process_owner",
                not_alive_reason: "live_owner_pane_not_alive",
            },
        );
    }

    #[test]
    fn focus_candidate_reconciles_stale_active_actor_to_new_live_owner() {
        assert_eq!(
            decide_focus_pane_candidate(Some(("%old", true)), Some("%old"), Some("%new")),
            FocusPaneCandidateDecision::Candidate {
                pane_id: "%new",
                focused_reason: "focused_live_process_owner",
                not_alive_reason: "live_owner_pane_not_alive",
            },
        );
    }

    #[test]
    fn focus_candidate_still_refuses_closed_actor_without_exact_live_proof() {
        assert_eq!(
            decide_focus_pane_candidate(Some(("%closed", false)), Some("%stale"), None),
            FocusPaneCandidateDecision::Reject {
                reason: "actor_not_focusable",
                pane_id: Some("%closed"),
            },
        );
    }

    #[test]
    fn actor_not_focusable_reject_is_rescued_when_live_pane_still_owns_the_document() {
        // A Blocked/Closed durable actor whose pane's live process tree still
        // owns the same document is a valid non-mutating focus target — the
        // finished/blocked session's pane is still open and showing that doc.
        assert_eq!(
            focus_reject_rescued_by_live_pane_owner(
                "actor_not_focusable",
                Some("%59"),
                Some("doc-bugs2"),
                "doc-bugs2",
            ),
            Some("%59"),
        );
    }

    #[test]
    fn actor_not_focusable_reject_stands_when_live_pane_owns_a_different_document() {
        // A pane reused by a different document must NOT be focus-stolen.
        assert_eq!(
            focus_reject_rescued_by_live_pane_owner(
                "actor_not_focusable",
                Some("%59"),
                Some("doc-other"),
                "doc-bugs2",
            ),
            None,
        );
        // No live owner proof at all (bare shell / dead) also stands.
        assert_eq!(
            focus_reject_rescued_by_live_pane_owner(
                "actor_not_focusable",
                Some("%59"),
                None,
                "doc-bugs2"
            ),
            None,
        );
        // Only `actor_not_focusable` is rescuable; other rejects are untouched.
        assert_eq!(
            focus_reject_rescued_by_live_pane_owner(
                "missing_actor_record",
                Some("%59"),
                Some("doc-bugs2"),
                "doc-bugs2",
            ),
            None,
        );
    }

    #[test]
    fn session_log_focus_proof_rejects_bare_shell_and_cross_document_reuse() {
        assert!(!session_log_owner_signals_prove_focus(true, false, true));
        assert!(!session_log_owner_signals_prove_focus(true, true, false));
        assert!(!session_log_owner_signals_prove_focus(false, true, true));
        assert!(session_log_owner_signals_prove_focus(true, true, true));
    }

    #[test]
    fn tmux_layout_sync_state_result_detects_swapped_panes() {
        let expected = vec![
            "/repo/tasks/left.md".to_string(),
            "/repo/tasks/right.md".to_string(),
        ];
        let actual = vec![
            "/repo/tasks/right.md".to_string(),
            "/repo/tasks/left.md".to_string(),
        ];

        assert_eq!(
            layout_sync_state_result(&expected, &actual),
            (false, "pane_order_mismatch")
        );
    }

    #[test]
    fn tmux_layout_sync_state_result_detects_missing_or_extra_panes() {
        let expected = vec![
            "/repo/tasks/left.md".to_string(),
            "/repo/tasks/right.md".to_string(),
        ];
        let actual = vec!["/repo/tasks/left.md".to_string()];

        assert_eq!(
            layout_sync_state_result(&expected, &actual),
            (false, "pane_count_mismatch")
        );
    }

    #[test]
    fn tmux_layout_sync_state_result_accepts_matching_model() {
        let expected = vec![
            "/repo/tasks/left.md".to_string(),
            "/repo/tasks/right.md".to_string(),
        ];

        assert_eq!(
            layout_sync_state_result(&expected, &expected),
            (true, "synced")
        );
    }

    #[test]
    fn tmux_layout_sync_state_rpc_reports_missing_tmux_session_with_model() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let doc = dir.path().join("tasks/left.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: left-session\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let response = handle_request(
            &(serde_json::json!({
                "command": "tmux_layout_sync_state",
                "diagnostic_payload": serde_json::to_string(&ControllerTmuxLayoutSyncStateInvocation {
                    columns: vec![doc.display().to_string()],
                    window: None,
                    focus: Some(doc.display().to_string()),
                }).unwrap()
            })
            .to_string()
                + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&response).unwrap();

        assert_eq!(envelope["ok"], true);
        assert_eq!(envelope["data"]["synced"], false);
        assert_eq!(envelope["data"]["reason"], "missing_tmux_session");
        assert_eq!(
            envelope["data"]["expected_documents"][0].as_str(),
            Some(doc.canonicalize().unwrap().to_string_lossy().as_ref())
        );
    }

    #[test]
    fn command_submit_unknown_agent_doc_command_is_terminal_rejection() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let bootstrap = test_bootstrap(&dir);
        let request = command_submit_request_for_test(
            None,
            "unknown_command",
            "agent-doc.unknown_command.v1",
            serde_json::json!({}),
            "cmd-unknown",
        );

        let response = handle_editor_command_submit_rpc(&bootstrap, request).unwrap();
        assert_eq!(response["exit_code"], 1);
        assert!(
            response["output"]
                .as_str()
                .unwrap()
                .contains("unsupported agent-doc command")
        );
        assert_eq!(response["projection"]["commands"][0]["status"], "rejected");
        assert_eq!(response["projection"]["commands"][0]["terminal"], true);
    }

    #[test]
    fn active_controller_adoption_requires_stable_handoff_state() {
        let current_binary = test_controller_binary_identity();
        let stable = active_controller_status_with_handoff_state(ControllerHandoffState::Stable);
        assert!(active_controller_status_is_adoptable(
            &stable,
            Some(&current_binary)
        ));

        for state in [
            ControllerHandoffState::Preparing,
            ControllerHandoffState::Promoted,
            ControllerHandoffState::Retiring,
            ControllerHandoffState::Failed,
        ] {
            let status = active_controller_status_with_handoff_state(state);
            assert!(
                !active_controller_status_is_adoptable(&status, Some(&current_binary)),
                "non-stable active controller must not be adopted: {state:?}"
            );
            assert_eq!(
                active_controller_status_non_stable_handoff_state(&status),
                Some(state)
            );
        }
    }

    #[test]
    fn active_controller_adoption_only_rejects_provably_newer_caller_binary() {
        let current_binary = test_controller_binary_identity();
        let stable = active_controller_status_with_handoff_state(ControllerHandoffState::Stable);
        assert!(active_controller_status_is_adoptable(
            &stable,
            Some(&current_binary)
        ));
        assert!(
            active_controller_status_is_adoptable(&stable, None),
            "callers that cannot resolve their own binary must adopt a stable controller"
        );
        assert!(!active_controller_status_needs_binary_replacement(
            &stable, None
        ));

        let missing_recorded_binary = ControllerStatus {
            controller_binary: None,
            ..stable.clone()
        };
        assert!(
            active_controller_status_is_adoptable(&missing_recorded_binary, Some(&current_binary)),
            "legacy/stale status records without binary identity are not proof of mismatch"
        );
        assert!(!active_controller_status_needs_binary_replacement(
            &missing_recorded_binary,
            Some(&current_binary)
        ));

        let mut changed_binary = current_binary.clone();
        changed_binary.modified_nanos = changed_binary.modified_nanos.wrapping_add(1);
        assert!(
            !active_controller_status_is_adoptable(&stable, Some(&changed_binary)),
            "known stale binary identity must still trigger replacement"
        );
        assert!(active_controller_status_needs_binary_replacement(
            &stable,
            Some(&changed_binary)
        ));

        let mut older_binary = current_binary.clone();
        older_binary.modified_secs -= 1;
        assert!(
            active_controller_status_is_adoptable(&stable, Some(&older_binary)),
            "an older caller must adopt a newer stable controller"
        );
        assert!(!active_controller_status_needs_binary_replacement(
            &stable,
            Some(&older_binary)
        ));
    }

    #[test]
    fn sync_tmux_layout_refuses_non_authoritative_controller() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut bootstrap = test_bootstrap(&dir);
        bootstrap.handoff_state = ControllerHandoffState::Preparing;
        bootstrap.handoff_started_at = Some(timestamp_secs().saturating_sub(60));
        let request = ControllerRequest {
            command: "sync_tmux_layout".to_string(),
            file: None,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(
                serde_json::to_string(&ControllerTmuxLayoutSyncInvocation {
                    columns: vec!["tasks/a.md".to_string()],
                    window: None,
                    focus: Some("tasks/a.md".to_string()),
                    no_autostart: false,
                    exact_visible: false,
                })
                .unwrap(),
            ),
        };

        let err = handle_sync_tmux_layout(&bootstrap, request).unwrap_err();
        assert!(
            format!("{err:#}").contains("controller not authoritative"),
            "sync must fail closed on non-stable handoff state: {err:#}"
        );
    }

    #[test]
    fn fresh_controller_refuses_bare_shutdown_from_stale_client() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let err = handle_request(
            &(serde_json::json!({ "command": "shutdown" }).to_string() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap_err();

        assert!(!should_stop);
        assert!(
            format!("{err:#}").contains("shutdown refused"),
            "fresh controller must reject unreasoned shutdown: {err:#}"
        );
    }

    #[test]
    fn fresh_controller_accepts_explicit_operator_shutdown() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let response = handle_request(
            &(serde_json::json!({
                "command": "shutdown",
                "reason": "operator_shutdown"
            })
            .to_string()
                + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();

        assert!(should_stop);
        assert!(response.contains("\"ok\":true"), "{response}");
    }

    #[test]
    fn fresh_controller_accepts_stale_replacement_from_provably_newer_caller() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut newer_binary = bootstrap.controller_binary.clone().unwrap();
        newer_binary.modified_secs += 1;
        let mut should_stop = false;

        let response = handle_request(
            &(serde_json::json!({
                "command": "shutdown",
                "reason": "stale_controller_replacement",
                "binary_identity": newer_binary,
            })
            .to_string()
                + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();

        assert!(should_stop);
        assert!(response.contains("\"ok\":true"), "{response}");
    }

    #[test]
    fn fresh_controller_refuses_stale_replacement_from_older_caller() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut older_binary = bootstrap.controller_binary.clone().unwrap();
        older_binary.modified_secs -= 1;
        let mut should_stop = false;

        let err = handle_request(
            &(serde_json::json!({
                "command": "shutdown",
                "reason": "stale_controller_replacement",
                "binary_identity": older_binary,
            })
            .to_string()
                + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap_err();

        assert!(!should_stop);
        assert!(format!("{err:#}").contains("shutdown refused"), "{err:#}");
    }

    #[test]
    fn stale_controller_accepts_bare_shutdown_for_replacement() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut bootstrap = test_bootstrap(&dir);
        let stale_binary = bootstrap.controller_binary.as_mut().unwrap();
        stale_binary.modified_nanos = stale_binary.modified_nanos.wrapping_add(1);
        let mut should_stop = false;

        let response = handle_request(
            &(serde_json::json!({ "command": "shutdown" }).to_string() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();

        assert!(should_stop);
        assert!(response.contains("\"ok\":true"), "{response}");
    }

    #[test]
    fn force_restart_controller_with_no_running_controller_is_a_noop_ok() {
        // `#cpcrestart`: with no project controller running, a force restart must
        // succeed as a no-op (nothing reaped, no graceful RPC) — the lazy launcher
        // will start a fresh controller on the next request. Never errors so an
        // operator recovery path can't itself fail closed.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let outcome = force_restart_controller(dir.path(), true).unwrap();
        assert!(
            outcome.reaped_pids.is_empty(),
            "no controller serves this fresh temp project, so nothing is reaped"
        );
        assert!(!outcome.graceful, "force skips the graceful shutdown RPC");
        assert!(outcome.force);
    }

    /// `#lzdurablesink` Phase 3 architecture enforcement: the command-plane
    /// authority surface (`command_plane.rs`) must stay pure — no file-lock
    /// primitives and no persistence query/CAS. The authority decides from the
    /// live Lazily projection and sinks via `append_apply_state_event`; it must
    /// never hold a file lock or replay storage to arbitrate a transition
    /// (`#lazily-hot-path`). The legitimate file-TOCTOU flock lives in
    /// `agent_doc_fs::acquire_doc_lock`, not here. This test reads the module's
    /// own source so a future regression (an import of `fs2`, a `lock_exclusive`
    /// call, a `state_store` query) fails the build.
    #[test]
    fn command_plane_authority_surface_has_no_file_lock_or_persistence_query() {
        let source = include_str!("command_plane.rs");
        for forbidden in [
            "lock_exclusive(",
            "acquire_doc_lock(",
            "fs2::",
            "use fs2",
            "open_state_db(",
            "state_store::load",
            "load_state_events",
            ".flock(",
        ] {
            assert!(
                !source.contains(forbidden),
                "command_plane.rs must not reference `{forbidden}` — the command-plane \
                 authority surface stays free of file-lock and persistence-query primitives"
            );
        }
    }

    #[test]
    fn command_plane_submit_dispatch_reaches_closeout_authority_and_returns_receipt() {
        // `#lzdurablesink` live transport (server half): a `command_plane_submit`
        // controller request carries a `CommandSubmit` envelope, the dispatch
        // routes it by `(namespace, name)` to `service_closeout_advance`, the
        // authority decides from the live Lazily projection, sinks the fact, and
        // the terminal `CausalReceipt` (applied) comes back in the response
        // envelope — never a transport ACK. This is the in-process proof that
        // `service_closeout_advance` is now reachable from a client request.
        use super::command_plane::{
            CloseoutAdvancePayload, CloseoutPhaseEvent, build_closeout_advance_submit,
        };

        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/dispatch.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();

        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();

        let submit = build_closeout_advance_submit(
            "cmd-dispatch-1",
            "cycle_state",
            "doc:cycle:write_applied:body",
            1,
            CloseoutAdvancePayload {
                document_path: doc.to_string_lossy().to_string(),
                event: CloseoutPhaseEvent::WriteApplied,
                event_label: None,
                reason: None,
                snapshot_content: None,
                file_content: Some("body".to_string()),
                response_sha256: None,
                cycle_id_hint: None,
            },
        )
        .unwrap();

        // Frame the submit exactly as the live `ControllerCommandTransport` does:
        // serialized into the request's `diagnostic_payload`.
        let request =
            ControllerRequest::command_plane_submit(serde_json::to_string(&submit).unwrap());
        let line = serde_json::to_string(&request).unwrap();
        let mut should_stop = false;
        let response = handle_request_locked(&line, &runtime, &mut should_stop).unwrap();

        let envelope: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(envelope["ok"], true, "envelope should be ok: {envelope}");
        let receipt: lazily::CausalReceipt =
            serde_json::from_value(envelope["data"].clone()).unwrap();
        assert_eq!(receipt.outcome, lazily::ReceiptOutcome::Applied);
        assert_eq!(receipt.causation_id, submit.command_id);

        // The durable sink fired through the dispatched authority path: the live
        // projection now observes WriteApplied.
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let phase = runtime
            .document_state_projection(&document_hash)
            .unwrap()
            .and_then(|d| d.closeout.phase);
        assert_eq!(phase, Some(agent_doc_turn::CyclePhase::WriteApplied));
    }

    #[test]
    fn command_plane_submit_unknown_name_fails_closed_as_rejected_receipt() {
        // Every command resolves to a terminal receipt: an unknown command name
        // fails closed as a `rejected` receipt (with the command id) so the client
        // resolves instead of hanging on a non-terminal ACK.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();

        let mut submit = super::command_plane::build_supervisor_recycle_submit(
            "cmd-unknown-1",
            "cycle_state",
            "root:unknown",
            "x",
            1,
        )
        .unwrap();
        // Same namespace, but a name no service is wired to yet.
        submit.name = "nonexistent_op".to_string();
        let request =
            ControllerRequest::command_plane_submit(serde_json::to_string(&submit).unwrap());
        let line = serde_json::to_string(&request).unwrap();
        let mut should_stop = false;
        let response = handle_request_locked(&line, &runtime, &mut should_stop).unwrap();

        let envelope: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(envelope["ok"], true, "envelope should be ok: {envelope}");
        let receipt: lazily::CausalReceipt =
            serde_json::from_value(envelope["data"].clone()).unwrap();
        assert_eq!(receipt.outcome, lazily::ReceiptOutcome::Rejected);
        assert_eq!(receipt.causation_id, submit.command_id);
        assert!(
            receipt
                .reason
                .as_deref()
                .unwrap()
                .contains("nonexistent_op")
        );
    }

    #[test]
    fn command_plane_submit_foreign_namespace_refused() {
        // The controller is the `agent-doc` namespace authority; a foreign
        // namespace is refused at the envelope boundary (envelope error), not
        // routed to a domain service.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();

        let mut submit = super::command_plane::build_supervisor_recycle_submit(
            "cmd-foreign-1",
            "cycle_state",
            "root:foreign",
            "x",
            1,
        )
        .unwrap();
        submit.namespace = "not-agent-doc".to_string();
        let request =
            ControllerRequest::command_plane_submit(serde_json::to_string(&submit).unwrap());
        let line = serde_json::to_string(&request).unwrap();
        let mut should_stop = false;
        let response = handle_request_locked(&line, &runtime, &mut should_stop).unwrap();

        let envelope: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(
            envelope["ok"], false,
            "foreign namespace must be refused: {envelope}"
        );
        assert!(
            envelope["error"]
                .as_str()
                .unwrap()
                .contains("foreign namespace")
        );
    }

    #[test]
    fn closeout_advance_authority_decides_from_live_projection_and_sinks_fact() {
        // `#lzdurablesink`: the authority services a closeout_advance CommandSubmit
        // from its live Lazily projection — no state.db replay — advances the pure
        // phase machine, emits the fact(s) as the durable sink, and returns a
        // terminal CausalReceipt (applied). With no prior state the machine
        // synthesizes a PreflightStarted cycle and advances it to WriteApplied.
        use super::command_plane::{
            CloseoutAdvancePayload, CloseoutPhaseEvent, build_closeout_advance_submit,
        };

        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/authority.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();

        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();
        let bootstrap = runtime.bootstrap_snapshot().unwrap();

        let submit = build_closeout_advance_submit(
            "cmd-authority-1",
            "cycle_state",
            "doc:cycle:write_applied:body",
            1,
            CloseoutAdvancePayload {
                document_path: doc.to_string_lossy().to_string(),
                event: CloseoutPhaseEvent::WriteApplied,
                event_label: None,
                reason: None,
                snapshot_content: None,
                file_content: Some("body".to_string()),
                response_sha256: None,
                cycle_id_hint: None,
            },
        )
        .unwrap();

        let receipt = service_closeout_advance(&bootstrap, &runtime, &submit);
        assert_eq!(receipt.outcome, lazily::ReceiptOutcome::Applied);

        // The durable sink fired: the live projection now observes WriteApplied.
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let phase = runtime
            .document_state_projection(&document_hash)
            .unwrap()
            .and_then(|d| d.closeout.phase);
        assert_eq!(phase, Some(agent_doc_turn::CyclePhase::WriteApplied));
    }

    #[test]
    fn closeout_advance_authority_routes_committed_and_abandoned() {
        // All four transitions route through the authority on the command plane.
        // Seed WriteApplied, then advance Committed(CommitSuccess); also verify
        // Abandoned on a fresh open cycle reaches the Abandoned phase. Each step
        // returns an applied CausalReceipt and the live projection advances.
        use super::command_plane::{
            CloseoutAdvancePayload, CloseoutPhaseEvent, CommitObservation,
            build_closeout_advance_submit,
        };

        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/routed.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();
        let bootstrap = runtime.bootstrap_snapshot().unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let mk = |command_id: &str, event: CloseoutPhaseEvent, reason: Option<&str>| {
            build_closeout_advance_submit(
                command_id,
                "cycle_state",
                format!("doc:cycle:{command_id}"),
                1,
                CloseoutAdvancePayload {
                    document_path: doc.to_string_lossy().to_string(),
                    event,
                    event_label: None,
                    reason: reason.map(str::to_string),
                    snapshot_content: None,
                    file_content: Some("body".to_string()),
                    response_sha256: None,
                    cycle_id_hint: None,
                },
            )
            .unwrap()
        };

        // PreflightStarted → WriteApplied → Committed.
        let receipt = service_closeout_advance(
            &bootstrap,
            &runtime,
            &mk("wa", CloseoutPhaseEvent::WriteApplied, None),
        );
        assert_eq!(receipt.outcome, lazily::ReceiptOutcome::Applied);
        let receipt = service_closeout_advance(
            &bootstrap,
            &runtime,
            &mk(
                "cm",
                CloseoutPhaseEvent::Committed(CommitObservation::CommitSuccess),
                None,
            ),
        );
        assert_eq!(receipt.outcome, lazily::ReceiptOutcome::Applied);
        let phase = runtime
            .document_state_projection(&document_hash)
            .unwrap()
            .and_then(|d| d.closeout.phase);
        assert_eq!(phase, Some(agent_doc_turn::CyclePhase::Committed));

        // A fresh document with an open cycle can be abandoned through the authority.
        let dir2 = tempfile::TempDir::new().unwrap();
        let doc2 = dir2.path().join("tasks/abandon.md");
        std::fs::create_dir_all(doc2.parent().unwrap()).unwrap();
        std::fs::write(&doc2, "body").unwrap();
        let runtime2 = ControllerRuntime::new_arc(test_bootstrap(&dir2)).unwrap();
        let bootstrap2 = runtime2.bootstrap_snapshot().unwrap();
        let submit = build_closeout_advance_submit(
            "ab",
            "cycle_state",
            "doc:cycle:ab",
            1,
            CloseoutAdvancePayload {
                document_path: doc2.to_string_lossy().to_string(),
                event: CloseoutPhaseEvent::Abandoned,
                event_label: None,
                reason: Some("stalled_preflight".to_string()),
                snapshot_content: None,
                file_content: None,
                response_sha256: None,
                cycle_id_hint: None,
            },
        )
        .unwrap();
        let receipt = service_closeout_advance(&bootstrap2, &runtime2, &submit);
        assert_eq!(receipt.outcome, lazily::ReceiptOutcome::Applied);
        let phase2 = runtime2
            .document_state_projection(&agent_doc_hash::document_id_for_path(&doc2))
            .unwrap()
            .and_then(|d| d.closeout.phase);
        assert_eq!(phase2, Some(agent_doc_turn::CyclePhase::Abandoned));
    }

    #[test]
    fn controller_client_handler_errors_return_error_envelope() {
        let dir = tempfile::TempDir::new().unwrap();
        let bootstrap = test_bootstrap(&dir);
        let runtime = ControllerRuntime::new_arc(bootstrap).unwrap();
        let request = serde_json::json!({
            "command": "register_supervisor",
            "file": "missing.md",
            "session_id": "session-1",
            "pane_id": "%1",
            "generation": 1,
            "state": "ready",
            "supervisor_pid": 1234,
            "supervisor_socket": "/tmp/missing.sock"
        });

        let (response, should_stop) =
            handle_request_for_client(&(request.to_string() + "\n"), &runtime).unwrap();
        let envelope: ControllerEnvelope<serde_json::Value> =
            serde_json::from_str(&response).unwrap();

        assert!(!should_stop);
        assert!(!envelope.ok);
        assert!(envelope.data.is_none());
        let error = envelope
            .error
            .expect("error envelope should include detail");
        assert!(
            error.contains("missing actor record for supervisor missing.md"),
            "unexpected error envelope: {error}"
        );
    }

    #[test]
    fn controller_transport_drop_retries_once_and_logs() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let doc = dir.path().join("tasks/retry.md");
        std::fs::write(&doc, "Body\n").unwrap();
        let mut attempts = 0;

        let result = retry_controller_transport_drop(&doc, "actor_binding", || {
            attempts += 1;
            if attempts == 1 {
                Err(std::io::Error::from(ErrorKind::ConnectionReset))
                    .context("failed to read project controller response")
            } else {
                Ok("ok")
            }
        })
        .unwrap();

        assert_eq!(result, "ok");
        assert_eq!(attempts, 2, "transport drop must retry exactly once");
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("controller_rpc_transport_retry command=actor_binding"),
            "transport retry must be visible in ops.log:\n{ops_log}"
        );
        assert!(ops_log.contains("failed to read project controller response"));
    }

    #[test]
    fn controller_command_errors_do_not_retry_as_transport_drops() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let doc = dir.path().join("tasks/no-retry.md");
        std::fs::write(&doc, "Body\n").unwrap();
        let mut attempts = 0;

        let err = retry_controller_transport_drop::<&str>(&doc, "dispatch", || {
            attempts += 1;
            anyhow::bail!("project controller command `dispatch` failed: queue_paused")
        })
        .unwrap_err();

        assert_eq!(attempts, 1, "controller rejections must not be retried");
        assert!(format!("{err:#}").contains("queue_paused"));
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            !ops_log.contains("controller_rpc_transport_retry"),
            "ordinary controller command errors must not log transport retry:\n{ops_log}"
        );
    }

    #[test]
    fn crdt_current_text_projection_recovery_is_limited_to_transport_or_publish_failures() {
        let timeout =
            anyhow::anyhow!("timed out after 120.0s waiting for project controller response");
        assert!(controller_current_text_error_allows_projection_recovery(
            &timeout
        ));

        let closed = anyhow::anyhow!("project controller closed connection without a response");
        assert!(controller_current_text_error_allows_projection_recovery(
            &closed
        ));

        let command_error = anyhow::anyhow!(
            "project controller command `crdt_current_text` failed: malformed payload"
        );
        assert!(!controller_current_text_error_allows_projection_recovery(
            &command_error
        ));

        let stale_editor_publish = anyhow::anyhow!(
            "project controller command `crdt_current_text` failed: document model startup/reconciliation failed for /tmp/tasks/doc.md: Lazily-current request over editor_ipc failed: IPC receipt rejected: {{\"type\":\"receipt\",\"status\":\"rejected\"}}; disk remained non-authoritative and was not read as a fallback"
        );
        assert!(controller_current_text_error_allows_projection_recovery(
            &stale_editor_publish
        ));
    }

    #[test]
    fn crdt_current_text_timeout_allows_slow_controller_reply() {
        assert!(
            CONTROLLER_CRDT_CURRENT_TEXT_TIMEOUT >= Duration::from_secs(120),
            "current-text recovery can queue behind large controller store work; \
             the client timeout must stay above the observed 30s response tail"
        );
    }

    #[test]
    fn crdt_current_text_rpc_reads_relay_without_publish_recovery() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let doc = dir.path().join("tasks/current-text.md");
        std::fs::write(&doc, "Body\n").unwrap();
        let canonical = doc.canonicalize().unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        controller_liveness_plane().lock().restore_liveness(&[
            agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid: std::process::id().into(),
                tag: "test-editor-current-text".into(),
            },
        ]);
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let response = handle_request(
            &(serde_json::json!({
                "command": "crdt_current_text",
                "file": canonical,
                "diagnostic_payload": serde_json::json!({
                    "source": "test_controller_current_text_pure_read"
                }).to_string()
            })
            .to_string()
                + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&response).unwrap();

        assert_eq!(envelope.get("ok").and_then(|ok| ok.as_bool()), Some(true));
        assert_eq!(
            envelope
                .get("data")
                .and_then(|data| data.get("status"))
                .and_then(|status| status.as_str()),
            Some("editor_attached_model_missing")
        );
        assert!(!should_stop);
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            !ops_log.contains("document_model_ensure_start"),
            "controller current-text RPC must not initiate editor publish recovery:\n{ops_log}"
        );
        assert!(
            ops_log.contains("controller_crdt_current_text")
                && ops_log.contains("status=editor_attached_model_missing"),
            "controller should log the pure relay-read result:\n{ops_log}"
        );
    }

    #[test]
    fn crdt_current_text_rpc_does_not_promote_projection_when_editor_is_attached() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let doc = dir.path().join("tasks/current-text-recover.md");
        std::fs::write(&doc, "Body\n").unwrap();
        let canonical = doc.canonicalize().unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        controller_liveness_plane().lock().restore_liveness(&[
            agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid: std::process::id().into(),
                tag: "test-editor-current-text-recover".into(),
            },
        ]);
        let mut prior = agent_doc_document_realtime::crdt_relay::RelayHub::new(1);
        let editor =
            agent_doc_document_realtime::crdt_relay::mint_client_id("intellij:controller-recover");
        prior.register(editor).unwrap();
        prior
            .apply_local(editor, 0, 0, "controller projection")
            .unwrap();
        agent_doc_snapshot_io::checkpoint_crdt_recovery_projection(
            &canonical,
            &prior.projection_bytes(),
            "test:controller-current-text",
        )
        .unwrap();

        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        let response = handle_request(
            &(serde_json::json!({
                "command": "crdt_current_text",
                "file": canonical,
                "diagnostic_payload": serde_json::json!({
                    "source": "test_controller_current_text_recover",
                    "recover_projection": true
                }).to_string()
            })
            .to_string()
                + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&response).unwrap();

        assert_eq!(envelope.get("ok").and_then(|ok| ok.as_bool()), Some(true));
        assert_eq!(
            envelope
                .get("data")
                .and_then(|data| data.get("status"))
                .and_then(|status| status.as_str()),
            Some("editor_attached_model_missing")
        );
        assert_eq!(
            envelope
                .get("data")
                .and_then(|data| data.get("text"))
                .and_then(|text| text.as_str()),
            None
        );
        assert!(!should_stop);
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            !ops_log.contains("crdt_current_text_projection_recovered")
                && ops_log.contains("status=editor_attached_model_missing"),
            "an attached editor must republish; the controller must not promote a durable projection:\n{ops_log}"
        );
    }

    #[test]
    fn controller_closed_without_response_is_retryable_transport_drop() {
        let err = anyhow::anyhow!("project controller closed connection without a response");
        assert!(controller_transport_drop_is_retryable(&err));

        let command_err = anyhow::anyhow!(
            "project controller command `dispatch` failed: failed_stage=queue_paused"
        );
        assert!(!controller_transport_drop_is_retryable(&command_err));
    }

    #[test]
    fn visible_write_commit_candidate_direct_durable_event_reconciles_without_controller() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let doc = dir.path().join("tasks/session.md");
        let content = "before\n### Re: done\n";
        std::fs::write(&doc, content).unwrap();
        let canonical = doc.canonicalize().unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        let commit_candidate_hash = visible_write_commit_candidate_hash(content);
        let payload = VisibleWriteCommitCandidatePayload {
            patch_id: "patch-direct-durable".to_string(),
            model_revision: 7,
            editor_visible_hash: commit_candidate_hash.clone(),
            commit_candidate_hash: commit_candidate_hash.clone(),
            commit_candidate_content: content.to_string(),
            source: "test_direct_durable".to_string(),
        };

        let proof = record_visible_write_commit_candidate_direct(
            dir.path(),
            &canonical,
            &document_hash,
            &payload,
            &anyhow::anyhow!("controller unavailable"),
        )
        .unwrap();

        assert_eq!(proof.patch_id, "patch-direct-durable");
        assert_eq!(proof.model_revision, 7);
        assert_eq!(proof.commit_candidate_hash, commit_candidate_hash);

        let reconciled =
            visible_write_commit_candidate_for_patch_file(&canonical, "patch-direct-durable")
                .expect("durable lazily event should reconcile without a live controller");
        assert_eq!(
            reconciled.commit_candidate_hash,
            proof.commit_candidate_hash
        );
        assert_eq!(
            reconciled.commit_candidate_content.as_deref(),
            Some(content)
        );

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("visible_write_commit_candidate_durable_event_recorded"));
        assert!(ops_log.contains("authority=state_backbone"));
        assert!(ops_log.contains("recovery=controller_reconcile"));
    }

    #[test]
    fn closeout_owner_cas_is_serialized_by_live_lazily_projection() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let doc = dir.path().join("tasks/session.md");
        std::fs::write(&doc, "body\n").unwrap();
        let cycle = agent_doc_cycle_state_io::start_preflight(&doc, Some("body\n"), Some("body\n"))
            .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let runtime = ControllerRuntime::new(bootstrap.clone()).unwrap();

        let claim_request = |owner_id: &str| ControllerRequest {
            command: "closeout_owner_claim".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("test".to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(
                serde_json::to_string(&CloseoutOwnerClaimRequest {
                    expected_cycle_id: Some(cycle.cycle_id.clone()),
                    owner_id: owner_id.to_string(),
                    owner_pid: std::process::id(),
                    role: "test_closeout".to_string(),
                    now_secs: 10,
                    lease_secs: 30,
                    allow_dead_owner_takeover: true,
                })
                .unwrap(),
            ),
        };

        let first =
            handle_closeout_owner_claim(&bootstrap, &runtime, claim_request("owner-1")).unwrap();
        assert!(matches!(
            first,
            CloseoutOwnerClaimOutcome::Acquired(CloseoutOwnerProjection {
                ref owner_id,
                ..
            }) if owner_id == "owner-1"
        ));
        let second =
            handle_closeout_owner_claim(&bootstrap, &runtime, claim_request("owner-2")).unwrap();
        assert!(matches!(
            second,
            CloseoutOwnerClaimOutcome::HeldByOther(CloseoutOwnerProjection {
                ref owner_id,
                ..
            }) if owner_id == "owner-1"
        ));

        let released = handle_closeout_owner_release(
            &bootstrap,
            &runtime,
            ControllerRequest {
                command: "closeout_owner_release".to_string(),
                file: Some(doc.clone()),
                session_id: None,
                pane_id: None,
                window_id: None,
                generation: None,
                state: None,
                caller: Some("test".to_string()),
                reason: Some("finished".to_string()),
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: None,
                diagnostic_payload: Some(
                    serde_json::to_string(&CloseoutOwnerReleaseRequest {
                        cycle_id: cycle.cycle_id.clone(),
                        owner_id: "owner-1".to_string(),
                        reason: "finished".to_string(),
                        released_secs: 11,
                    })
                    .unwrap(),
                ),
            },
        )
        .unwrap();
        assert!(released);
        assert!(matches!(
            handle_closeout_owner_claim(&bootstrap, &runtime, claim_request("owner-2")).unwrap(),
            CloseoutOwnerClaimOutcome::Acquired(CloseoutOwnerProjection {
                ref owner_id,
                ..
            }) if owner_id == "owner-2"
        ));
    }

    /// `#restartstderrbleed` — the auto-install child must NOT inherit the
    /// supervisor's fd1 (the agent pane). Prove that `make install`-style output
    /// on BOTH stdout and stderr is redirected to the supervisor-log target fd,
    /// `#restartbleednonroute`: the auto-install child must land on a real LOG
    /// file, not fd2. fd2 is off-pane only while `SupervisorStderrRedirect` is
    /// active; on a non-route-owned TUI (or a route-owned supervisor whose
    /// redirect fell back to inactive) fd2 IS the agent pane, and `make
    /// install` output bleeds into the live session.
    #[cfg(unix)]
    #[test]
    fn auto_install_stderr_log_is_opened_off_fd2_when_a_project_root_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let file = auto_install_stderr_log_file(dir.path())
            .expect("a project root must yield an appendable stderr log");

        use std::os::fd::AsRawFd;
        assert!(
            file.as_raw_fd() > 2,
            "the child must dup a real log fd, never the inherited fd2/pane"
        );

        let path =
            agent_doc_supervisor_process::start_command::route_owned_stderr_log_path(dir.path());
        assert!(
            path.exists(),
            "opening must create the log: {}",
            path.display()
        );

        // Appending, not truncating — a recycle must not discard prior output.
        use std::io::Write;
        let mut file = file;
        file.write_all(b"first\n").unwrap();
        drop(file);
        let again = auto_install_stderr_log_file(dir.path()).unwrap();
        drop(again);
        assert!(
            std::fs::read_to_string(&path).unwrap().contains("first"),
            "reopening must append, not truncate"
        );
    }

    /// Outside a project there is no log to open, so the caller keeps the fd2
    /// plan rather than discarding build output entirely.
    #[cfg(unix)]
    #[test]
    fn auto_install_stderr_log_is_none_without_a_project_root() {
        let dir = tempfile::tempdir().unwrap();
        assert!(auto_install_stderr_log_file(dir.path()).is_none());
    }

    /// never left on the parent's stdout where it would corrupt the agent TUI.
    #[cfg(unix)]
    #[test]
    fn auto_install_child_stdio_redirects_stdout_off_the_pane() {
        use std::io::Read;
        use std::os::fd::AsRawFd;

        let tmp = tempfile::NamedTempFile::new().expect("temp log file");
        let log = std::fs::OpenOptions::new()
            .write(true)
            .open(tmp.path())
            .expect("open temp log for write");

        // Wire the probe child exactly as the auto-install path does, but at a
        // caller-controlled target fd (a temp file standing in for the redirected
        // supervisor-stderr.log) so the test never perturbs the harness fds.
        let (stdin, stdout, stderr) = auto_install_child_stdio_from_plan(
            agent_doc_supervisor::auto_install_stdio::auto_install_child_stdio_plan_to_fd(
                log.as_raw_fd(),
            ),
        );
        let status = std::process::Command::new("sh")
            .args(["-c", "echo BLEED_STDOUT; echo BLEED_STDERR 1>&2"])
            .stdin(stdin)
            .stdout(stdout)
            .stderr(stderr)
            .status()
            .expect("spawn probe child");
        assert!(status.success(), "probe child exited non-zero: {status}");

        let mut captured = String::new();
        std::fs::File::open(tmp.path())
            .expect("reopen temp log")
            .read_to_string(&mut captured)
            .expect("read temp log");

        // The child's stdout was routed to the log target (a dup of the
        // supervisor stderr), NOT inherited onto fd1 / the pane.
        assert!(
            captured.contains("BLEED_STDOUT"),
            "child stdout must land on the supervisor-log target, not the agent pane: {captured:?}"
        );
        assert!(
            captured.contains("BLEED_STDERR"),
            "child stderr must land on the supervisor-log target: {captured:?}"
        );
    }

    #[test]
    fn auto_install_should_retry_while_attempts_remain() {
        use agent_doc_supervisor::config::auto_install_should_retry;

        // `#autoinstallretry`: retry the first attempts, give up on the last so the
        // caller can fall back to operator refresh.
        assert!(auto_install_should_retry(1, 3), "attempt 1 of 3 retries");
        assert!(auto_install_should_retry(2, 3), "attempt 2 of 3 retries");
        assert!(
            !auto_install_should_retry(3, 3),
            "final attempt does not retry"
        );
        assert!(
            !auto_install_should_retry(4, 3),
            "past the cap never retries"
        );
        // A single-attempt policy never retries.
        assert!(!auto_install_should_retry(1, 1));
    }
    use rusqlite::params;
    use std::collections::BTreeMap;

    #[test]
    fn duplicate_scan_only_matches_same_project_controller_args() {
        use agent_doc_controller::command_line::same_project_controller_args_match_project_root;

        let dir = tempfile::TempDir::new().unwrap();
        let args = vec![
            "/home/user/.cargo/bin/agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            dir.path().display().to_string(),
        ];
        assert!(same_project_controller_args_match_project_root(
            &args,
            dir.path()
        ));

        let shell_sentinel = vec![
            "sh".to_string(),
            "-c".to_string(),
            "sleep 30; :".to_string(),
            dir.path().join("agent-doc").display().to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            dir.path().display().to_string(),
        ];
        assert!(same_project_controller_args_match_project_root(
            &shell_sentinel,
            dir.path()
        ));

        let other_dir = tempfile::TempDir::new().unwrap();
        assert!(!same_project_controller_args_match_project_root(
            &args,
            other_dir.path()
        ));

        let non_controller = vec![
            "agent-doc".to_string(),
            "preflight".to_string(),
            dir.path().join("task.md").display().to_string(),
        ];
        assert!(!same_project_controller_args_match_project_root(
            &non_controller,
            dir.path()
        ));

        let tmux_launcher = vec![
            "tmux".to_string(),
            "new-session".to_string(),
            "agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            dir.path().display().to_string(),
        ];
        assert!(!same_project_controller_args_match_project_root(
            &tmux_launcher,
            dir.path()
        ));
    }
    #[test]
    fn force_overrides_in_flight_gate_only_when_forced_and_stable() {
        use agent_doc_controller::recycle::force_overrides_in_flight_gate;

        // `#recycleforce`: the force bypass of the in-flight-dispatch idle gate fires
        // only when the operator asked for force AND the controller is `Stable`.
        assert!(force_overrides_in_flight_gate(true, true));
        // Not forced → never bypass (default defer-at-idle behavior unchanged).
        assert!(!force_overrides_in_flight_gate(false, true));
        // Forced but mid-handoff (not Stable) → do NOT bypass; a forced recycle must
        // not strand a half-promoted replacement controller.
        assert!(!force_overrides_in_flight_gate(true, false));
        assert!(!force_overrides_in_flight_gate(false, false));
    }

    #[test]
    fn recycle_force_rpc_sets_forced_flag_while_plain_recycle_does_not() {
        // `#recycleforce`: the `recycle_force` RPC sets BOTH the want-recycle and the
        // forced flags; the plain `recycle` RPC sets only want-recycle (so its idle
        // gate is preserved).
        let dir = tempfile::TempDir::new().unwrap();
        let bootstrap = test_bootstrap(&dir);
        let runtime = ControllerRuntime::new_arc(bootstrap).unwrap();
        let mut should_stop = false;

        // Plain recycle: want-recycle set, force NOT set.
        let response = handle_request_locked(
            &(serde_json::json!({ "command": "recycle" }).to_string() + "\n"),
            &runtime,
            &mut should_stop,
        )
        .unwrap();
        assert!(response.contains("\"ok\":true"));
        assert!(runtime.recycle_requested());
        assert!(!runtime.recycle_forced());
        assert!(
            !should_stop,
            "recycle defers to the serve loop, never stops mid-RPC"
        );

        // Forced recycle: force flag now set too.
        let response = handle_request_locked(
            &(serde_json::json!({ "command": "recycle_force" }).to_string() + "\n"),
            &runtime,
            &mut should_stop,
        )
        .unwrap();
        assert!(response.contains("\"ok\":true"));
        assert!(runtime.recycle_requested());
        assert!(runtime.recycle_forced());
        assert!(
            !should_stop,
            "forced recycle still defers to the serve-loop tick, never stops mid-RPC"
        );
    }

    #[test]
    fn controller_status_reports_startup_binary_identity() {
        let dir = tempfile::TempDir::new().unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let response = handle_request(
            &(serde_json::json!({ "command": "status" }).to_string() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let status: ControllerStatus = serde_json::from_str(&response).unwrap();

        assert!(status.active);
        assert_eq!(status.controller_binary, bootstrap.controller_binary);
        let current_binary = current_binary_identity().unwrap();
        assert!(status::controller_binary_identity_matches(
            status.controller_binary.as_ref(),
            Some(&current_binary)
        ));
        let freshness = status
            .freshness
            .as_ref()
            .expect("controller status should expose binary freshness proof");
        assert_eq!(freshness.controller.pid, Some(bootstrap.pid));
        assert!(freshness.installed_binary.is_some());
    }

    #[test]
    fn controller_handoff_status_uses_lightweight_control_plane() {
        let dir = tempfile::TempDir::new().unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let response = handle_request(
            &(serde_json::json!({ "command": "handoff_status" }).to_string() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let status: ControllerStatus = serde_json::from_str(&response).unwrap();

        assert!(status.active);
        assert_eq!(status.pid, Some(bootstrap.pid));
        assert_eq!(
            status.controller_generation,
            Some(bootstrap.controller_generation)
        );
        assert!(status.stale_duplicate_pids.is_empty());
        assert_eq!(status.control_plane.store_actor.state, "unknown");
        assert_eq!(status.control_plane.store_actor.owned_items, 0);
        assert!(status.control_plane.store_actor.categories.is_empty());
    }

    #[test]
    fn controller_client_response_read_times_out() {
        let dir = tempfile::TempDir::new().unwrap();
        let sock = socket_path(dir.path());
        std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
        let name = sock.clone().to_fs_name::<GenericFilePath>().unwrap();
        let listener = ListenerOptions::new().name(name).create_sync().unwrap();
        // The peer accepts and then never answers, holding the connection open
        // until the client has given up. Parking on a channel rather than
        // sleeping a multiple of the deadline keeps the test's runtime equal to
        // the deadline itself instead of a multiple of it.
        let (release, released) = std::sync::mpsc::channel::<()>();
        let handle = std::thread::spawn(move || {
            let _stream = listener.accept().unwrap();
            let _ = released.recv();
        });

        let started = Instant::now();
        let err = request(dir.path(), "status").unwrap_err();
        let elapsed = started.elapsed();
        drop(release);
        handle.join().unwrap();

        // Bound the read against the deadline itself, not a hard-coded wall
        // clock: the previous `< 2s` literal silently became an equality with
        // the deadline the moment that constant moved, turning a boundedness
        // check into a latency assertion the runner could fail.
        assert!(
            elapsed < CONTROLLER_RPC_TIMEOUT * 2,
            "the read must be bounded by the deadline, not by the silent peer: {elapsed:?}"
        );
        assert!(
            err.to_string().contains("timed out") || format!("{err:#}").contains("timed out"),
            "{err:#}"
        );
    }
    #[test]
    fn idle_controller_client_does_not_block_later_status_request() {
        let dir = tempfile::TempDir::new().unwrap();
        let project_root = dir.path().to_path_buf();
        let server_root = project_root.clone();
        let handle = std::thread::spawn(move || serve(&server_root, LaunchMode::Lazy).unwrap());
        wait_for_test_controller(&project_root);

        let idle_stream = connect(&project_root).unwrap();
        let response = request(&project_root, "status").unwrap();
        let status: ControllerStatus = serde_json::from_str(&response).unwrap();
        assert!(status.active);
        assert_eq!(status.project_root, project_root);

        drop(idle_stream);
        let shutdown = request_with_reason(&project_root, "shutdown", "test_shutdown").unwrap();
        assert!(shutdown.contains("\"ok\":true"), "{shutdown}");
        handle.join().unwrap();
    }

    #[test]
    fn controller_client_connection_is_one_request() {
        let dir = tempfile::TempDir::new().unwrap();
        let project_root = dir.path().to_path_buf();
        let server_root = project_root.clone();
        let handle = std::thread::spawn(move || serve(&server_root, LaunchMode::Lazy).unwrap());
        wait_for_test_controller(&project_root);

        let stream = connect(&project_root).unwrap();
        let (reader_half, mut writer_half) = stream.split();
        let mut reader = BufReader::new(reader_half);
        writer_half
            .write_all(b"{\"command\":\"status\"}\n")
            .unwrap();
        writer_half.flush().unwrap();
        let mut response = String::new();
        reader.read_line(&mut response).unwrap();
        let status: ControllerStatus = serde_json::from_str(response.trim()).unwrap();
        assert!(status.active);

        let mut second = String::new();
        let closed = reader.read_line(&mut second).unwrap();
        assert_eq!(closed, 0, "controller should close after one request");

        let next = request(&project_root, "status").unwrap();
        let next_status: ControllerStatus = serde_json::from_str(&next).unwrap();
        assert!(next_status.active);
        let shutdown = request_with_reason(&project_root, "shutdown", "test_shutdown").unwrap();
        assert!(shutdown.contains("\"ok\":true"), "{shutdown}");
        handle.join().unwrap();
    }

    #[test]
    fn mark_write_applied_routes_through_command_plane_when_controller_live() {
        // `#lzdurablesink` / `#lazily-hot-path`: with a live controller, the
        // `mark_write_applied` chokepoint submits a `closeout_advance` command
        // over the command plane instead of the local load→decide→save→append
        // path. The controller authority decides from the live projection, sinks
        // the fact, and returns a terminal applied receipt; the client reads back
        // the advanced cycle state. End-to-end proof through the real controller
        // socket (not `handle_request_locked` in-process).
        let dir = tempfile::TempDir::new().unwrap();
        let project_root = dir.path().to_path_buf();
        std::fs::create_dir_all(project_root.join("tasks")).unwrap();
        let doc = project_root.join("tasks/live.md");
        std::fs::write(&doc, "body\n").unwrap();

        let server_root = project_root.clone();
        let handle = std::thread::spawn(move || serve(&server_root, LaunchMode::Lazy).unwrap());
        wait_for_test_controller(&project_root);

        let state = agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            None,
            Some("body\n"),
        )
        .expect("mark_write_applied through the command plane");
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::WriteApplied);

        // The durable sink fired through the controller authority: a fresh
        // client read reconstructs WriteApplied from the sunk facts.
        let readback = agent_doc_cycle_state_io::load(&doc)
            .expect("read back cycle state")
            .expect("a cycle state exists");
        assert_eq!(readback.phase, agent_doc_turn::CyclePhase::WriteApplied);

        let shutdown = request_with_reason(&project_root, "shutdown", "test_shutdown").unwrap();
        assert!(shutdown.contains("\"ok\":true"), "{shutdown}");
        handle.join().unwrap();
    }

    #[test]
    fn mark_committed_and_abandoned_route_through_command_plane_when_controller_live() {
        // The remaining closeout transitions route through the command plane:
        // write_applied → committed (the caller's free-text label canonicalizes
        // to the typed CommitObservation on the wire), and abandoned (the
        // descriptive reason rides the `reason` field; the authority stamps it
        // back as last_event).
        let dir = tempfile::TempDir::new().unwrap();
        let project_root = dir.path().to_path_buf();
        std::fs::create_dir_all(project_root.join("tasks")).unwrap();
        let doc = project_root.join("tasks/live.md");
        std::fs::write(&doc, "body\n").unwrap();

        let server_root = project_root.clone();
        let handle = std::thread::spawn(move || serve(&server_root, LaunchMode::Lazy).unwrap());
        wait_for_test_controller(&project_root);

        agent_doc_cycle_state_io::mark_write_applied(&doc, "write_applied", None, Some("body\n"))
            .expect("write_applied through the command plane");
        let committed =
            agent_doc_cycle_state_io::mark_committed(&doc, "commit_success", None, Some("body\n"))
                .expect("mark_committed through the command plane");
        assert_eq!(committed.phase, agent_doc_turn::CyclePhase::Committed);
        // The command plane carries the typed CommitObservation, so the canonical
        // label is stamped back as last_event.
        assert_eq!(committed.last_event, "commit_success");

        // `mark_committed` routes through the command plane (the caller's label
        // rides the event_label payload field). A re-commit on an already-
        // committed cycle refreshes and keeps the stable label, so "repair_applied"
        // folds onto the existing "commit_success".
        let refreshed =
            agent_doc_cycle_state_io::mark_committed(&doc, "repair_applied", None, Some("body\n"))
                .expect("mark_committed (re-commit) through the command plane");
        assert_eq!(refreshed.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(
            refreshed.last_event, "commit_success",
            "re-commit refreshes onto the existing stable commit label"
        );

        // Abandon on a fresh open cycle: the descriptive reason round-trips as last_event.
        let doc2 = project_root.join("tasks/abandon.md");
        std::fs::write(&doc2, "body\n").unwrap();
        let abandoned = agent_doc_cycle_state_io::mark_abandoned(
            &doc2,
            "stalled_preflight",
            None,
            Some("body\n"),
        )
        .expect("mark_abandoned through the command plane");
        assert_eq!(abandoned.phase, agent_doc_turn::CyclePhase::Abandoned);
        assert_eq!(abandoned.last_event, "stalled_preflight");

        let shutdown = request_with_reason(&project_root, "shutdown", "test_shutdown").unwrap();
        assert!(shutdown.contains("\"ok\":true"), "{shutdown}");
        handle.join().unwrap();
    }

    #[test]
    fn closeout_owner_claim_and_release_route_through_command_plane_dispatch() {
        // `#lzdurablesink` todo (3): the command-plane dispatch routes
        // closeout_owner_claim / closeout_owner_release and returns the typed CAS
        // result (Acquired / HeldByOther), not a coarse receipt — the authority
        // result for a coordination op is preserved. The live-socket transport is
        // already proven by the mark_write_applied integration test, so this
        // exercises the dispatch + service in-process (mirroring the CAS test).
        use super::command_plane::{
            CloseoutOwnerClaimPayload, CloseoutOwnerReleasePayload,
            build_closeout_owner_claim_submit, build_closeout_owner_release_submit,
        };
        use agent_doc_state_backbone::{CloseoutOwnerClaimOutcome, CloseoutOwnerClaimRequest};

        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/owner-cp.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body\n").unwrap();
        let cycle = agent_doc_cycle_state_io::start_preflight(&doc, Some("body\n"), Some("body\n"))
            .unwrap();

        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();
        let bootstrap = runtime.bootstrap_snapshot().unwrap();

        let claim_submit = |owner_id: &str| {
            build_closeout_owner_claim_submit(
                format!("closeout-owner-claim:{owner_id}"),
                format!("closeout-owner-claim:{owner_id}"),
                0,
                CloseoutOwnerClaimPayload {
                    document_path: doc.to_string_lossy().to_string(),
                    request: CloseoutOwnerClaimRequest {
                        expected_cycle_id: Some(cycle.cycle_id.clone()),
                        owner_id: owner_id.to_string(),
                        owner_pid: std::process::id(),
                        role: "test_closeout".to_string(),
                        now_secs: 10,
                        lease_secs: 30,
                        allow_dead_owner_takeover: true,
                    },
                },
            )
            .unwrap()
        };

        let first = serde_json::from_value::<CloseoutOwnerClaimOutcome>(
            dispatch_command_plane_submit(&bootstrap, runtime.as_ref(), &claim_submit("owner-1"))
                .unwrap(),
        )
        .unwrap();
        assert!(matches!(
            first,
            CloseoutOwnerClaimOutcome::Acquired(ref p) if p.owner_id == "owner-1"
        ));
        let second = serde_json::from_value::<CloseoutOwnerClaimOutcome>(
            dispatch_command_plane_submit(&bootstrap, runtime.as_ref(), &claim_submit("owner-2"))
                .unwrap(),
        )
        .unwrap();
        assert!(matches!(
            second,
            CloseoutOwnerClaimOutcome::HeldByOther(ref p) if p.owner_id == "owner-1"
        ));

        // Release owner-1 over the command plane, then owner-2 can claim.
        let release = build_closeout_owner_release_submit(
            "closeout-owner-release",
            "closeout-owner-release",
            0,
            CloseoutOwnerReleasePayload {
                document_path: doc.to_string_lossy().to_string(),
                cycle_id: cycle.cycle_id.clone(),
                owner_id: "owner-1".to_string(),
                reason: "finished".to_string(),
                released_secs: 11,
            },
        )
        .unwrap();
        let released: bool = serde_json::from_value::<bool>(
            dispatch_command_plane_submit(&bootstrap, runtime.as_ref(), &release).unwrap(),
        )
        .unwrap();
        assert!(released);
        let reclaimed = serde_json::from_value::<CloseoutOwnerClaimOutcome>(
            dispatch_command_plane_submit(&bootstrap, runtime.as_ref(), &claim_submit("owner-2"))
                .unwrap(),
        )
        .unwrap();
        assert!(matches!(
            reclaimed,
            CloseoutOwnerClaimOutcome::Acquired(ref p) if p.owner_id == "owner-2"
        ));
    }

    #[test]
    fn detached_controller_exits_when_temp_project_root_is_removed() {
        let dir = tempfile::TempDir::new().unwrap();
        let project_root = dir.path().to_path_buf();
        let server_root = project_root.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let result = serve(&server_root, LaunchMode::Lazy).map_err(|err| err.to_string());
            let _ = tx.send(result);
        });
        wait_for_test_controller(&project_root);

        drop(dir);

        let result = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("controller should exit after its temp project root is removed");
        assert_eq!(result, Ok(()));
        handle.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn controller_startup_rejects_a_recreated_project_root_incarnation() {
        let dir = tempfile::TempDir::new().unwrap();
        let project_root = dir.path().to_path_buf();
        let incarnation = ProjectRootIncarnation::capture(&project_root).unwrap();

        drop(dir);
        std::fs::create_dir_all(&project_root).unwrap();

        assert!(
            !incarnation.still_matches(&project_root),
            "a detached child must distinguish the caller's deleted TempDir from a same-path directory it recreated"
        );
        std::fs::remove_dir(&project_root).unwrap();
    }

    #[test]
    fn run_status_ensure_does_not_hold_idle_controller_stream() {
        let dir = tempfile::TempDir::new().unwrap();
        let project_root = dir.path().to_path_buf();
        let server_root = project_root.clone();
        let handle = std::thread::spawn(move || serve(&server_root, LaunchMode::Lazy).unwrap());
        wait_for_test_controller(&project_root);

        let idle_stream = connect(&project_root).unwrap();
        let started = Instant::now();
        run_status(Some(&project_root), true).unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "controller status --ensure should complete without holding an idle stream"
        );

        drop(idle_stream);
        let shutdown = request_with_reason(&project_root, "shutdown", "test_shutdown").unwrap();
        assert!(shutdown.contains("\"ok\":true"), "{shutdown}");
        handle.join().unwrap();
    }

    #[test]
    fn public_controller_serve_skips_when_authoritative_public_controller_exists() {
        let dir = tempfile::TempDir::new().unwrap();
        let project_root = dir.path().to_path_buf();
        let server_root = project_root.clone();
        let handle = std::thread::spawn(move || serve(&server_root, LaunchMode::Lazy).unwrap());
        wait_for_test_controller(&project_root);
        let before = status(&project_root).unwrap();
        assert!(before.active);

        let started = Instant::now();
        serve_with_options(
            &project_root,
            LaunchMode::Lazy,
            None,
            None,
            None,
            ControllerHandoffState::Stable,
        )
        .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "second public serve should skip instead of replacing the active controller"
        );

        let after = status(&project_root).unwrap();
        assert!(after.active);
        assert_eq!(after.pid, before.pid);
        let ops_log = std::fs::read_to_string(project_root.join(".agent-doc/logs/ops.log"))
            .unwrap_or_default();
        assert!(
            ops_log.contains("controller_public_launch_skipped_existing_authoritative"),
            "skip proof marker missing:\n{ops_log}"
        );

        let shutdown = request_with_reason(&project_root, "shutdown", "test_shutdown").unwrap();
        assert!(shutdown.contains("\"ok\":true"), "{shutdown}");
        handle.join().unwrap();
    }

    /// `#ctrlrespawnenoent` — the ENOENT retry budget must span a real install,
    /// not just an atomic rename.
    ///
    /// The original schedule was 5 x 100ms. A `make install` rebuilds and copies
    /// a ~70MB binary, so the path can be absent for seconds: every attempt was
    /// exhausted inside 500ms, every agent-doc command on the project failed, and
    /// recovery needed a hand-started `controller serve`.
    #[test]
    fn launch_enoent_retry_budget_spans_a_real_install() {
        let schedule = launch_enoent::retry_schedule_ms();

        assert!(
            schedule.first().is_some_and(|first| *first <= 100),
            "the common microsecond rename must still resolve on a fast first retry, got {schedule:?}"
        );
        let total = *schedule.last().expect("schedule must retry at least once");
        assert!(
            total >= 10_000,
            "the budget must span a realistic install window; got {total}ms across {} retries",
            schedule.len()
        );
        assert!(
            total < launch_enoent::LAUNCH_ENOENT_TOTAL_BUDGET.as_millis(),
            "the schedule must stay inside its own bound"
        );
        // The regression: the old 5 x 100ms schedule capped out here.
        assert!(
            total > 500,
            "500ms is the window that failed in production; got {total}ms"
        );
    }

    #[test]
    fn controller_serve_reaps_stale_socket_file_before_binding() {
        let dir = tempfile::TempDir::new().unwrap();
        let project_root = dir.path().to_path_buf();
        let stale_socket = socket_path(&project_root);
        std::fs::create_dir_all(stale_socket.parent().unwrap()).unwrap();
        std::fs::write(&stale_socket, []).unwrap();
        assert!(stale_socket.is_file());

        let server_root = project_root.clone();
        let server = std::thread::spawn(move || serve(&server_root, LaunchMode::Lazy).unwrap());
        wait_for_test_controller(&project_root);

        let controller_status = status(&project_root).unwrap();
        assert!(controller_status.active);
        assert!(stale_socket.exists());
        assert!(
            !stale_socket.is_file(),
            "the stale regular file must be replaced by a live socket"
        );

        let shutdown = request_with_reason(&project_root, "shutdown", "test_shutdown").unwrap();
        assert!(shutdown.contains("\"ok\":true"), "{shutdown}");
        server.join().unwrap();
    }

    #[test]
    fn connect_or_launch_adopts_controller_published_during_launch_claim_contention() {
        // #suprecyclelock / #1j8q: a self-recycled supervisor can re-run `start`
        // while another project-root launcher still owns the bootstrap endpoint.
        // If that holder publishes a healthy controller before the waiter gives
        // up, the waiter must connect to it instead of surfacing os-error-11.
        let dir = tempfile::TempDir::new().unwrap();
        let project_root = dir.path().to_path_buf();
        let held_claim = LaunchClaim::acquire(&project_root).unwrap();

        // `#ctrliotestflake`: this used to race wall-clock. The waiter was given a
        // fixed 150ms lock wait and the publish was timed with a bare 25ms sleep,
        // so the adoption marker only appeared when the controller happened to be
        // published inside that window — too early and the caller's PRE-lock
        // status check adopts with no marker, too late (a loaded machine) and the
        // waiter fails outright. Order the phases explicitly instead: the caller
        // blocks on a lock wait long enough that it never expires, the publish
        // happens only after the caller signals it is about to contend, and
        // releasing `held_claim` is what lets the waiter through. It then adopts
        // via the `phase=acquired` marker path rather than the timeout one, with
        // no timing budget to blow.
        let (entering_tx, entering_rx) = std::sync::mpsc::channel::<()>();
        let caller_root = project_root.clone();
        let caller = std::thread::spawn(move || {
            entering_tx.send(()).unwrap();
            let stream = connect_or_launch_with_claim_wait(
                &caller_root,
                LaunchMode::Lazy,
                Duration::from_secs(30),
            )?;
            drop(stream);
            Ok::<(), anyhow::Error>(())
        });

        // The caller is about to take the pre-lock status check (which must find
        // no controller) and then block on the held lock.
        entering_rx.recv().unwrap();
        let server_root = project_root.clone();
        let server = std::thread::spawn(move || {
            let started = Instant::now();
            loop {
                match serve(&server_root, LaunchMode::Lazy) {
                    Ok(()) => return Ok::<(), anyhow::Error>(()),
                    Err(err)
                        if err.to_string().contains("database is locked")
                            && started.elapsed() < Duration::from_secs(5) =>
                    {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Err(err) => return Err(err),
                }
            }
        });
        wait_for_test_controller(&project_root);

        // Only now may the contended waiter proceed: it waited, so it must adopt
        // the controller published while it was blocked.
        drop(held_claim);
        let result = caller.join().unwrap();
        assert!(
            result.is_ok(),
            "contended waiter should adopt the published controller, not fail: {:?}",
            result.err().map(|err| err.to_string())
        );

        let ops_log = std::fs::read_to_string(project_root.join(".agent-doc/logs/ops.log"))
            .unwrap_or_default();
        assert!(
            ops_log.contains("controller_launch_claim_waiter_adopted_published_controller"),
            "contended adoption proof marker missing:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("controller launch already in progress"),
            "the historical launch-contention error must not be logged:\n{ops_log}"
        );

        let shutdown = request_with_reason(&project_root, "shutdown", "test_shutdown").unwrap();
        assert!(shutdown.contains("\"ok\":true"), "{shutdown}");
        server.join().unwrap().unwrap();
    }

    #[test]
    fn controller_session_operator_status_reports_history_and_command_stages() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/operator.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-operator\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-operator",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-operator",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let operator_command = ControllerRequest {
            command: "operator_command".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("session_clear".to_string()),
            diagnostic_payload: Some("test operator command".to_string()),
        };
        let response = handle_request(
            &(serde_json::to_string(&operator_command).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let authorization = envelope.data.unwrap();
        assert_eq!(authorization.accepted_stage, "operator_ready");
        assert!(authorization.receipt.receipt_id > 0);
        assert_eq!(
            authorization.receipt.status,
            ControllerDispatchResultStatus::Accepted
        );
        assert_eq!(
            authorization.receipt.proof_scope,
            ControllerDispatchProofScope::AcceptedOnly
        );

        let status = ControllerRequest {
            command: "session_status".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&status).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<SessionOperatorStatus> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let status = envelope.data.unwrap();
        assert_eq!(
            status.record.unwrap().state,
            agent_doc_sqlite::state_store::ActorState::Ready
        );
        assert_eq!(status.transitions.len(), 2);
        let attempt = status.dispatch_attempts.last().unwrap();
        assert_eq!(attempt.receipt_id, authorization.receipt.receipt_id);
        assert_eq!(attempt.accepted_stage.as_deref(), Some("operator_ready"));
        assert_eq!(attempt.result_status.as_deref(), Some("accepted"));
        assert_eq!(attempt.proof_scope.as_deref(), Some("accepted_only"));
        assert!(!attempt.dispatch_start_proven);
    }

    #[test]
    fn supervisor_replacement_records_restart_and_returns_background_receipt() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/restart-supervisor.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-restart\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-restart",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-restart",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let request = ControllerRequest {
            command: "supervisor_replacement".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: Some("continue".to_string()),
            caller: Some("session".to_string()),
            reason: Some("operator_request".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(serde_json::json!({"force": false}).to_string()),
        };
        let response = handle_request(
            &(serde_json::to_string(&request).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<SupervisorReplacementReceipt> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok, "{:?}", envelope.error);
        let receipt = envelope.data.unwrap();
        assert_eq!(receipt.accepted_stage, "operator_ready");
        assert_eq!(receipt.operator_receipt.command_kind, "session_restart");
        assert_eq!(
            receipt.operator_receipt.status,
            ControllerDispatchResultStatus::Accepted
        );
        assert_eq!(receipt.session_id, "session-restart");
        assert_eq!(receipt.pane_id, "%41");
        assert_eq!(receipt.generation, 1);
        assert_eq!(receipt.mode, "continue");
        assert!(!receipt.force);
        assert!(
            !receipt.background_started,
            "unit tests use the no-spawn background stub"
        );

        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("controller_supervisor_replacement_accepted"),
            "acceptance marker missing:\n{ops_log}"
        );
        assert!(
            ops_log.contains("controller_supervisor_replacement_background_stub"),
            "test background stub marker missing:\n{ops_log}"
        );
    }

    #[test]
    fn supervisor_replacement_preserves_only_bare_shell_panes() {
        assert_eq!(
            supervisor_replacement_pane_start_decision(false, None, Some("claude")),
            SupervisorReplacementPaneStartDecision::AutoStartNew
        );
        for shell in ["sh", "bash", "zsh", "-zsh", "fish"] {
            assert_eq!(
                supervisor_replacement_pane_start_decision(true, Some(shell), Some("claude")),
                SupervisorReplacementPaneStartDecision::PreserveExisting,
                "{shell} should be safe for shell-command cold start"
            );
        }
        assert_eq!(
            supervisor_replacement_pane_start_decision(true, None, Some("claude")),
            SupervisorReplacementPaneStartDecision::BlockLiveNonShell,
            "unknown command on a live pane is not safe for prompt injection"
        );
    }

    /// `#restartlivepane`: a pane running THIS document's own harness is what
    /// "Restart Agent" is always aimed at. Refusing it made the command
    /// impossible to use on its own target.
    #[test]
    fn supervisor_replacement_restarts_a_pane_running_its_own_harness() {
        for harness in ["claude", "codex", "opencode"] {
            assert_eq!(
                supervisor_replacement_pane_start_decision(true, Some(harness), Some(harness)),
                SupervisorReplacementPaneStartDecision::RestartLiveHarness,
                "{harness} pane bound to a {harness} document must be restartable"
            );
        }
    }

    /// The guard that matters is NARROWED, not removed. A live pane running
    /// anything other than this document's own harness stays untouchable —
    /// killing a build, an editor, an ssh session, or a harness bound to a
    /// DIFFERENT document destroys work the operator never offered up.
    #[test]
    fn supervisor_replacement_still_refuses_panes_that_are_not_our_harness() {
        for foreign in [
            "vim",
            "nvim",
            "ssh",
            "cargo",
            "make",
            "node",
            "agent-doc",
            "",
        ] {
            assert_eq!(
                supervisor_replacement_pane_start_decision(true, Some(foreign), Some("claude")),
                SupervisorReplacementPaneStartDecision::BlockLiveNonShell,
                "{foreign:?} must not be quit or receive route-owned start text"
            );
        }
        // A codex pane must not be quit just because a claude document asked.
        assert_eq!(
            supervisor_replacement_pane_start_decision(true, Some("codex"), Some("claude")),
            SupervisorReplacementPaneStartDecision::BlockLiveNonShell,
            "a pane running a DIFFERENT harness is someone else's session"
        );
        // `node` is in claude's `process_names`; matching on that list instead of
        // the exact binary would make any Node process look restartable.
        assert_eq!(
            supervisor_replacement_pane_start_decision(true, Some("node"), Some("claude")),
            SupervisorReplacementPaneStartDecision::BlockLiveNonShell,
            "process_names substring matching must not be used here"
        );
        // Unknown document harness proves nothing, so it cannot authorise a quit.
        assert_eq!(
            supervisor_replacement_pane_start_decision(true, Some("claude"), None),
            SupervisorReplacementPaneStartDecision::BlockLiveNonShell,
            "an unresolved document harness must not authorise quitting a live pane"
        );
    }

    #[test]
    fn host_supervisor_is_stale_compares_running_inode_against_installed_inode() {
        use agent_doc_supervisor::config::host_supervisor_is_stale;

        // #fccsupwarn2 — staleness is identity (inode), NOT process start time. A
        // supervisor that hot-reloaded onto the fresh binary via in-place `execve`
        // preserves its process start time but remaps the inode, so it must read FRESH.
        let installed_inode = 4242u64;

        // Supervisor maps a DIFFERENT inode (original launch never re-exec'd; the old,
        // now-unlinked binary) → STALE.
        assert!(host_supervisor_is_stale(
            Some(installed_inode + 1),
            installed_inode
        ));

        // Supervisor maps the SAME inode as the install (fresh launch OR in-place
        // execve hot-reload) → FRESH. This is the case the start-time heuristic got
        // wrong: a re-exec'd supervisor that runs current code.
        assert!(!host_supervisor_is_stale(
            Some(installed_inode),
            installed_inode
        ));

        // Running inode unknown (non-Linux / unreadable `/proc/<pid>/exe`) → fail-open,
        // NOT stale, so a read error can never spam the warning.
        assert!(!host_supervisor_is_stale(None, installed_inode));
    }

    #[test]
    fn queue_boundary_self_recycle_makes_stale_content_ours_refusal_ineligible() {
        use agent_doc_supervisor::config::host_supervisor_is_stale;
        use agent_doc_supervisor::lifecycle::{SupervisorRecycleAction, supervisor_recycle_action};

        let installed_inode = 4242u64;
        assert!(
            host_supervisor_is_stale(Some(installed_inode + 1), installed_inode),
            "an old mapped inode is the prerequisite for stale content_ours refusal"
        );

        assert_eq!(
            supervisor_recycle_action(
                /* stale */ true, /* auto_recycle */ true, /* turn_boundary */ true,
                /* head_pending */ true, /* explicit_admin */ false,
                /* write_wedged */ false, /* editor_delivery_stale */ false,
                /* reexec_failed */ false, /* cycle_open */ false,
            ),
            SupervisorRecycleAction::RecycleImmediate,
            "a stale supervisor with a pending queue head must self-recycle before the next item"
        );

        assert!(
            !host_supervisor_is_stale(Some(installed_inode), installed_inode),
            "after supervisor_binary_stale_self_recycled maps the installed inode, stale-supervisor content_ours refusal is no longer eligible"
        );
    }

    #[test]
    fn controller_status_reports_single_process_control_plane_runtime() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/control-plane.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-control-plane\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let start = ControllerRequest {
            command: "start_session".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-control-plane".to_string()),
            pane_id: Some("%77".to_string()),
            window_id: Some("@7".to_string()),
            generation: Some(1),
            state: None,
            caller: Some("start".to_string()),
            reason: Some("session_start".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&start).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<agent_doc_sqlite::state_store::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);

        let register = ControllerRequest {
            command: "register_supervisor".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-control-plane".to_string()),
            pane_id: Some("%77".to_string()),
            window_id: None,
            generation: Some(1),
            state: Some("starting".to_string()),
            caller: None,
            reason: None,
            supervisor_pid: Some(4242),
            supervisor_socket: Some("supervisor.sock".to_string()),
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&register).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<agent_doc_sqlite::state_store::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);

        let dispatch = ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-control-plane".to_string()),
            pane_id: Some("%77".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("managed_reopen".to_string()),
            diagnostic_payload: Some("control-plane status test".to_string()),
        };
        let response = handle_request(
            &(serde_json::to_string(&dispatch).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);

        let doc_id = doc.to_string_lossy().to_string();
        record_projection_diagnostic(
            dir.path(),
            "controller-state",
            &doc_id,
            "test controller-state lag",
        );
        let conn = open_state_db(dir.path()).unwrap();
        state_store::upsert_queue_head_in_db(
            &conn,
            &doc_id,
            "agent:queue",
            Some("ctrlplane-storeactor"),
            "do [#ctrlplane-storeactor]",
            "selected",
        )
        .unwrap();
        state_store::upsert_document_cycle_state_in_db(
            &conn,
            &doc_id,
            "cycle-control-plane",
            "preflight_started",
            Some("ctrlplane-storeactor"),
            None,
        )
        .unwrap();
        drop(conn);
        let mut conn = open_state_db(dir.path()).unwrap();
        state_store::commit_session_actor_closeout_in_db(
            &mut conn,
            &state_store::SessionActorCloseoutCommit {
                document_id: &doc_id,
                cycle_id: "cycle-control-plane",
                cycle_state: "committed",
                queue_name: "agent:queue",
                queue_head_id: Some("ctrlplane-storeactor"),
                queue_head_prompt: Some("do [#ctrlplane-storeactor]"),
                queue_head_state: "consumed",
                response_commit: Some("commit-control-plane"),
                mutations: vec![state_store::SessionActorCloseoutMutation {
                    item_id: "ctrlplane-storeactor",
                    mutation_kind: "backlog_completion",
                    status: "done",
                }],
            },
        )
        .unwrap();
        state_store::insert_admin_operation_in_db(
            &conn,
            "projection_repair",
            Some(&doc_id),
            "accepted",
            Some("control-plane status test"),
        )
        .unwrap();
        state_store::insert_crash_recovery_marker_in_db(
            &conn,
            "startup_reconcile",
            Some(&doc_id),
            Some(1),
            "pending",
            Some("control-plane status test"),
        )
        .unwrap();
        state_store::store_layout_state_in_db(&conn, DEFAULT_LAYOUT_SCOPE, &["@7".to_string()])
            .unwrap();

        let status = ControllerRequest {
            command: "status".to_string(),
            file: None,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&status).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let status: ControllerStatus = serde_json::from_str(&response).unwrap();

        assert!(status.active);
        assert_eq!(
            status.control_plane.process_model,
            "project_scoped_single_process"
        );
        assert_eq!(status.control_plane.external_boundary, "controller_ipc");
        assert_eq!(status.control_plane.state_authority, ".agent-doc/state.db");
        assert_eq!(
            status.control_plane.projection_authority,
            "cold_recovery_output"
        );
        assert_eq!(status.control_plane.dispatch_actor.owned_items, 1);
        assert_eq!(
            status
                .control_plane
                .store_actor
                .categories
                .get("queue_heads"),
            Some(&1)
        );
        assert_eq!(
            status
                .control_plane
                .store_actor
                .categories
                .get("document_cycles"),
            Some(&1)
        );
        assert_eq!(
            status
                .control_plane
                .store_actor
                .categories
                .get("pending_mutations"),
            Some(&1)
        );
        assert_eq!(
            status
                .control_plane
                .store_actor
                .categories
                .get("admin_operations"),
            Some(&1)
        );
        assert_eq!(
            status
                .control_plane
                .store_actor
                .categories
                .get("crash_recovery_markers"),
            Some(&1)
        );
        assert_eq!(
            status
                .control_plane
                .store_actor
                .categories
                .get("layout_states"),
            Some(&1)
        );
        assert_eq!(status.control_plane.session_actors.owned_items, 4);
        assert_eq!(status.control_plane.supervisor_adapters.owned_items, 1);
        assert!(status.control_plane.projection_workers.owned_items >= 1);
        assert!(status.control_plane.store_actor.owned_items >= 11);
    }

    #[cfg(unix)]
    #[test]
    fn detached_controller_command_starts_a_new_process_session() {
        let mut command = Command::new("sleep");
        command
            .arg("1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        close_inherited_fds_on_exec(&mut command);
        let mut child = command.spawn().expect("spawn detached-session probe");
        let pid = child.id() as libc::pid_t;
        let sid = unsafe { libc::getsid(pid) };
        assert_eq!(sid, pid, "detached daemon must own a new process session");
        child.kill().expect("stop detached-session probe");
        child.wait().expect("reap detached-session probe");
    }

    #[test]
    fn controller_runtime_refreshes_memory_after_write_through_commit() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/memory-auth.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-memory-auth\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let runtime = ControllerRuntime::new_arc(bootstrap).unwrap();
        let mut should_stop = false;
        let doc_id = doc.to_string_lossy().to_string();

        assert!(runtime.actor_record(&doc_id).unwrap().is_none());

        let start = ControllerRequest {
            command: "start_session".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-memory-auth".to_string()),
            pane_id: Some("%88".to_string()),
            window_id: Some("@8".to_string()),
            generation: Some(1),
            state: None,
            caller: Some("start".to_string()),
            reason: Some("session_start".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request_locked(
            &(serde_json::to_string(&start).unwrap() + "\n"),
            &runtime,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<agent_doc_sqlite::state_store::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);

        let memory_record = runtime.actor_record(&doc_id).unwrap().unwrap();
        assert_eq!(memory_record.session_id, "session-memory-auth");
        assert_eq!(memory_record.pane_id, "%88");

        let status = ControllerRequest {
            command: "status".to_string(),
            file: None,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request_locked(
            &(serde_json::to_string(&status).unwrap() + "\n"),
            &runtime,
            &mut should_stop,
        )
        .unwrap();
        let status: ControllerStatus = serde_json::from_str(&response).unwrap();
        assert_eq!(
            status
                .control_plane
                .session_actors
                .categories
                .get("actor_records"),
            Some(&1)
        );
        assert_eq!(
            status
                .control_plane
                .session_actors
                .categories
                .get("write_through_sqlite"),
            Some(&1)
        );
        assert_eq!(
            status
                .control_plane
                .session_actors
                .categories
                .get("map_backend_std_btree_map"),
            Some(&1)
        );
    }
    #[test]
    fn typed_controller_decode_reports_missing_data_with_command_and_raw_envelope() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/missing-data.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let request = ControllerRequest {
            command: "session_status".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };

        let err = decode_controller_response::<SessionOperatorStatus>(
            dir.path(),
            &request,
            r#"{"ok":true}"#,
        )
        .expect_err("typed controller response without data must fail");

        let message = err.to_string();
        assert!(message.contains("command `session_status`"));
        assert!(message.contains(r#"{"ok":true}"#));
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("controller_response_missing_data command=session_status"));
    }
    #[test]
    fn reaper_terminates_wedged_same_project_controller_and_marks_failed() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let sentinel = spawn_controller_sentinel(dir.path());
        let pid = sentinel.id();
        assert!(
            is_same_project_controller_pid(dir.path(), pid),
            "sentinel must present a matching `controller serve --project-root` cmdline"
        );
        let old = timestamp_secs() - 600;
        write_preparing_bootstrap(dir.path(), pid, Some(old));

        let (reaped, kept) =
            terminate_stale_preparing_controllers(dir.path(), Duration::from_secs(45), false)
                .unwrap();
        assert_eq!((reaped, kept), (1, 0));

        // The live wedged process must be dead (the critical difference from the
        // projection reaper). The sentinel is a child of this test, so a killed
        // process lingers as a zombie (with `/proc/<pid>` still present) until we
        // `wait()` it — poll `try_wait` instead of `process_is_alive`.
        let status = wait_for_test_child_exit(
            sentinel,
            Duration::from_secs(2),
            "wedged sentinel pid must be reaped",
        );
        assert!(
            !status.success(),
            "sentinel must be signal-terminated, not exit cleanly: {status:?}"
        );

        // The record must be superseded with `Failed` so the next bind promotes fresh.
        let after = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(after.handoff_state, ControllerHandoffState::Failed);
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("stale_preparing_controller_reaped pid="));
        assert!(ops_log.contains("caller=gc"));
    }
    #[test]
    fn orphan_reaper_reaps_aged_preparing_sentinel() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let sentinel = spawn_preparing_controller_sentinel(dir.path());
        let pid = sentinel.id();
        // Let the process age past a zero threshold (start age is /proc dir mtime).
        std::thread::sleep(Duration::from_millis(1100));

        let (reaped, kept) =
            reap_orphaned_preparing_controllers(dir.path(), Duration::from_secs(0), false).unwrap();
        assert_eq!((reaped, kept), (1, 0));

        // The live orphan must actually be terminated (the whole point vs. the
        // record-scoped reaper). The sentinel is our child, so a killed process
        // lingers as a zombie until `wait()` — poll `try_wait`.
        let status = wait_for_test_child_exit(
            sentinel,
            Duration::from_secs(2),
            "aged preparing orphan must be reaped",
        );
        assert!(
            !status.success(),
            "orphan must be signal-terminated: {status:?}"
        );

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains(&format!("orphaned_preparing_controller_reaped pid={pid}")));
    }
    #[test]
    fn qflood_coalesces_only_auto_in_flight_redispatch() {
        // The flood: an AUTO re-dispatch while the same cycle's dispatch is still in
        // flight (unconsumed) — suppress it so the trigger does not pile into the
        // busy pane.
        assert!(dispatch_should_coalesce_in_flight(true, false));
        // Operator dispatch always passes, even mid-flight (explicit intent must not
        // be blocked by auto-drain backpressure).
        assert!(!dispatch_should_coalesce_in_flight(true, true));
        // Nothing in flight (prior consumed / new cycle) → always admit.
        assert!(!dispatch_should_coalesce_in_flight(false, false));
        assert!(!dispatch_should_coalesce_in_flight(false, true));
    }
    #[test]
    fn qflood2_coalesce_marker_survives_ipc_wrapping() {
        // The route caller only sees the controller error as a string across IPC,
        // wrapped by `request_controller`. The benign-coalesce classifier must still
        // recognise it through that wrapping (the same pattern #ctlstalebin uses).
        let wrapped = format!(
            "project controller command `dispatch` failed: dispatch coalesced for x: a dispatch for generation 66 is already in flight (#qflood); {} receipt_id=3310",
            DISPATCH_COALESCED_IN_FLIGHT_MARKER
        );
        assert!(agent_doc_controller::dispatch::dispatch_error_is_coalesced(
            &wrapped
        ));
        // A real failure (e.g. a paused queue) must NOT be swallowed as success.
        assert!(
            !agent_doc_controller::dispatch::dispatch_error_is_coalesced(
                "project controller command `dispatch` failed: dispatch blocked for x: failed_stage=queue_paused"
            )
        );
    }
    #[test]
    fn qflood_coalesces_in_flight_despite_ready_projection_drift_and_releases_on_ready() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/qflood.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-qf\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        agent_doc_session_actor_io::record_session_start_direct(&doc, "session-qf", "%41", "@1", 1)
            .unwrap();
        // Actor actively running a turn (mid-turn / pane busy).
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-qf",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Busy,
            "supervisor",
            "turn_started",
        )
        .unwrap();
        let document_id = agent_doc_session_actor_io::canonical_document_id_in(
            dir.path(),
            &doc.to_string_lossy(),
        );
        let bootstrap = test_bootstrap(&dir);
        let dispatch = || ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-qf".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("projection_repair".to_string()),
            diagnostic_payload: Some("qflood test".to_string()),
        };

        // First dispatch while Busy: nothing in flight yet ⇒ admitted (queued),
        // recording the in-flight marker. The first dispatch of a turn is never lost.
        handle_dispatch(&bootstrap, None, dispatch()).expect("first busy dispatch must queue");
        let conn = open_state_db(dir.path()).unwrap();
        assert!(
            state_store::has_open_in_flight_dispatch(&conn, &document_id, 1).unwrap(),
            "the first busy dispatch must be in flight"
        );

        // Simulate the exact race from Run Agent Doc: a lossy pane reconciler
        // projects Ready before the command has established/finished its turn.
        // This direct state write intentionally bypasses the controller's genuine
        // Ready boundary, so the durable receipt remains open.
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-qf",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
            "test",
            "premature_idle_projection",
        )
        .unwrap();
        assert!(
            state_store::has_open_in_flight_dispatch(&conn, &document_id, 1).unwrap(),
            "projection drift must not consume the controller-owned receipt"
        );

        // An explicit operator reopen is never swallowed by stale same-generation
        // backpressure. This is the JetBrains `Run Agent Doc` path: the click must
        // reach the pane even while an older receipt remains open.
        let operator_dispatch = || ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-qf".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("dispatch_only_reopen".to_string()),
            diagnostic_payload: Some("operator Run Agent Doc".to_string()),
        };
        handle_dispatch(&bootstrap, None, operator_dispatch())
            .expect("operator reopen must bypass stale in-flight coalescing");

        // A non-operator re-fire while the receipt is in flight ⇒ coalesced
        // (bail), even though
        // the actor projection says Ready. It cannot pile another trigger into
        // the pane.
        let err = handle_dispatch(&bootstrap, None, dispatch()).unwrap_err();
        assert!(
            format!("{err:#}").contains("coalesced"),
            "a redundant in-flight re-dispatch must coalesce: {err:#}"
        );
        // `#qflood2`: the coalesce bail must carry the stable machine marker so route
        // callers can recognise the benign dedup across the IPC boundary and report
        // deduped-success instead of an exit-1 failure.
        assert!(
            agent_doc_controller::dispatch::dispatch_error_is_coalesced(&format!("{err:#}")),
            "a coalesce bail must be classifiable as deduped-success: {err:#}"
        );
        assert!(
            !agent_doc_controller::dispatch::dispatch_error_is_coalesced(
                "dispatch blocked for x: failed_stage=queue_paused"
            ),
            "an unrelated dispatch failure must NOT classify as a benign coalesce"
        );
        let coalesced: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dispatch_attempts WHERE failed_stage = 'coalesced_in_flight'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(coalesced, 1, "the coalesced re-dispatch must be recorded");

        // A genuine controller-owned Ready boundary releases the in-flight marker
        // so the next turn dispatches cleanly.
        let mark_ready = ControllerRequest {
            command: "mark_lifecycle".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-qf".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: Some("ready".to_string()),
            caller: Some("supervisor".to_string()),
            reason: Some("prompt_ready".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        handle_mark_lifecycle(&bootstrap, None, mark_ready).expect("mark ready");
        assert!(
            !state_store::has_open_in_flight_dispatch(&conn, &document_id, 1).unwrap(),
            "the Ready transition must release the in-flight marker"
        );
    }
    #[test]
    fn anw0_stale_generation_redirect_marker_classifies_and_survives_ipc_wrapping() {
        // The route caller only sees the controller error as a string across IPC,
        // wrapped by `request_controller`. The redirect classifier must recognise the
        // marker through that wrapping and extract the retry generation (mirrors the
        // #qflood2 marker contract).
        let wrapped = format!(
            "project controller command `dispatch` failed: dispatch rejected for x: requested generation 1, current generation 2 ({} retry_generation=2) receipt_id=42",
            DISPATCH_STALE_GENERATION_REDIRECT_MARKER
        );
        assert_eq!(
            dispatch_error_stale_generation_redirect_target(&wrapped),
            Some(2),
            "a redirect bail must yield its retry generation across IPC wrapping"
        );
        // A terminal stale_generation reject (current actor Closed/Blocked) carries no
        // marker, so it must stay terminal — never trigger a self-heal retry.
        assert_eq!(
            dispatch_error_stale_generation_redirect_target(
                "dispatch rejected for x: requested generation 1, current generation 2 receipt_id=42"
            ),
            None,
            "a marker-less stale_generation reject must stay terminal"
        );
        // An unrelated failure (e.g. a paused queue) is never a redirect.
        assert_eq!(
            dispatch_error_stale_generation_redirect_target(
                "dispatch blocked for x: failed_stage=queue_paused"
            ),
            None
        );
    }
    #[test]
    fn anw0_stale_generation_redirect_emitted_only_when_current_dispatchable() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/anw0.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-anw0\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-anw0",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let stale_dispatch = || ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-anw0".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            // Requested generation 0 is superseded by the live generation 1.
            generation: Some(0),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("managed_reopen".to_string()),
            diagnostic_payload: Some("anw0 redirect test".to_string()),
        };

        // Current generation (1) is Ready ⇒ a retry would be authorized ⇒ structured
        // redirect with the marker + retry target pointing at the current generation.
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-anw0",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();
        let err = handle_dispatch(&bootstrap, None, stale_dispatch()).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("requested generation 0"), "{msg}");
        assert_eq!(
            dispatch_error_stale_generation_redirect_target(&msg),
            Some(1),
            "a dispatchable current generation must emit a retry-against-N+1 redirect: {msg}"
        );

        // Current generation (1) is Closed ⇒ a retry cannot help ⇒ terminal reject with
        // NO redirect marker, so racing dispatch does not loop against a dead actor.
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-anw0",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Closed,
            "supervisor",
            "superseded",
        )
        .unwrap();
        let err = handle_dispatch(&bootstrap, None, stale_dispatch()).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("requested generation 0"), "{msg}");
        assert_eq!(
            dispatch_error_stale_generation_redirect_target(&msg),
            None,
            "a closed current generation must stay a terminal stale_generation reject: {msg}"
        );
    }
    #[test]
    fn anw0_racing_dispatch_self_heals_against_new_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/anw0heal.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-anw0h\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-anw0h",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-anw0h",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        // A racing dispatcher still holds the superseded generation 0. `authorize_dispatch`
        // must consume the redirect and retry ONCE against the live generation 1, landing
        // the dispatch instead of failing closed.
        let auth = authorize_dispatch(
            dir.path(),
            DispatchRequest {
                file: doc.clone(),
                session_id: "session-anw0h".to_string(),
                pane_id: "%41".to_string(),
                generation: 0,
                command_kind: "managed_reopen".to_string(),
                diagnostic_payload: "anw0 self-heal".to_string(),
            },
        )
        .expect("a racing dispatch must self-heal by retrying against the current generation");
        assert_eq!(
            auth.record.generation, 1,
            "the self-heal retry must land on the current (N+1) generation"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("dispatch_stale_generation_redirect"),
            "the first stale dispatch must leave redirect proof:\n{ops_log}"
        );
        assert!(
            ops_log.contains("dispatch_retry_after_stale_generation"),
            "authorize_dispatch must leave retry proof:\n{ops_log}"
        );
        assert!(
            !ops_log.contains(" is closed"),
            "redirectable stale dispatch must not degrade into a terminal generation-closed proof:\n{ops_log}"
        );
    }
    #[test]
    fn resolve_supervisor_auto_recycle_precedence() {
        use agent_doc_supervisor::config::resolve_supervisor_auto_recycle as r;

        // `#suprecyclequeue` precedence: env > frontmatter > project config > default.
        // Env truthy force-enables regardless of frontmatter / project config.
        assert!(r(Some("1"), Some(false), Some(false)));
        assert!(r(Some("true"), None, None));
        assert!(r(Some(" ON "), Some(false), Some(false)));
        // Env falsey force-disables regardless of frontmatter / project opt-in.
        assert!(!r(Some("0"), Some(true), Some(true)));
        assert!(!r(Some("off"), Some(true), Some(true)));
        // Env unset / unrecognized → frontmatter decides over project config.
        assert!(r(None, Some(true), Some(false)));
        assert!(!r(None, Some(false), Some(true)));
        assert!(r(Some("garbage"), Some(true), Some(false)));
        // Frontmatter absent → project config decides.
        assert!(r(None, None, Some(true)));
        assert!(!r(None, None, Some(false)));
        // `#supselfheal` — nothing set anywhere → default ON (turn-boundary
        // blue/green self-recycle is the hands-off self-heal). Opt out via a
        // falsey env/frontmatter/project knob (asserted above).
        assert!(r(None, None, None));
        assert!(r(Some(""), None, None));
    }
    #[test]
    fn resolve_agent_change_restart_precedence() {
        use agent_doc_supervisor::config::resolve_agent_change_restart as r;

        // Env wins.
        assert!(r(Some("on"), Some(false), Some(false)));
        assert!(!r(Some("off"), Some(true), Some(true)));
        // Unrecognized env → fall through to frontmatter.
        assert!(r(Some("garbage"), Some(true), Some(false)));
        // Frontmatter over project.
        assert!(r(None, Some(true), Some(false)));
        assert!(!r(None, Some(false), Some(true)));
        // Project decides when frontmatter absent.
        assert!(r(None, None, Some(true)));
        assert!(!r(None, None, Some(false)));
        // `#agentreloadrestart` — nothing set → default ON.
        assert!(r(None, None, None));
        assert!(r(Some(""), None, None));
    }
    #[test]
    fn pause_reason_stale_supervisor_churn_stop_classification() {
        use agent_doc_controller::dispatch::pause_reason_is_stale_supervisor_churn_stop as c;
        // `#jbrestale` live-repro churn-stop reason → recoverable by recycle.
        assert!(c(
            "churn-stop: do[#c2b6] operator-verify head re-injected by stale supervisor pid1368698 (pre-0.34.0); needs operator recycle, not agent drain"
        ));
        // Explicit discriminators (case-insensitive).
        assert!(c("supervisor_binary_stale pane=%25"));
        assert!(c("re-injected by Stale Supervisor PID 42"));
        assert!(c(
            "#qchurn no-op churn: go-mode re-injecting :pushpin: [#qchurn] each idle boundary; all heads are undrainable under stale host supervisor pid 2715614; zero drainable work"
        ));
        assert!(c(
            "stale route-owned supervisor (pid 968752) replaying already-answered/archived #advance-review queue item"
        ));
        // `churn-stop` + the recycle remedy with no other signature still recovers.
        assert!(c("churn-stop: repeated injection; needs operator recycle"));
        // Deliberate spent-preset pause → NOT a stale-supervisor recovery (must fail closed).
        assert!(!c(
            "advance-review preset head is spent (backlog added + both features shipped); pausing so the go-queue does not re-trigger advance-review. Operator can clear the '- #advance-review' line"
        ));
        // Plain operator pause → not recoverable.
        assert!(!c("operator paused for manual review"));
        // A churn-stop with neither a stale signature nor the recycle remedy → not recoverable.
        assert!(!c("churn-stop: repeated no-op closeouts"));
    }
    #[test]
    fn stale_supervisor_pid_extraction() {
        use agent_doc_controller::dispatch::stale_supervisor_pid_from_pause_reason as p;
        assert_eq!(
            p("re-injected by stale supervisor pid1368698 (pre-0.34.0)"),
            Some(1368698)
        );
        assert_eq!(
            p("stale supervisor pid 2825163; needs operator recycle"),
            Some(2825163)
        );
        assert_eq!(
            p("undrainable under stale host supervisor pid 2715614; zero drainable work"),
            Some(2715614)
        );
        assert_eq!(p("stale supervisor (no pid named)"), None);
        assert_eq!(p("supervisor_binary_stale pane=%25"), None);
    }
    #[test]
    fn dispatch_error_supervisor_restart_redirect_classification() {
        use agent_doc_controller::dispatch::{
            DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER, DispatchRecoveryOutcomeClass,
            stale_queue_pause_pid_from_dispatch_error as r,
            stale_queue_pause_recovery_from_dispatch_error as recover,
        };
        // `#jbrestale`: a queue_paused bail tagged with the restart-redirect marker is
        // recoverable; the named stale pid is extracted for the proof line.
        let tagged = "project controller command `dispatch` failed: dispatch blocked for x.md: failed_stage=queue_paused reason=churn-stop: stale supervisor pid1368698; needs operator recycle receipt_id=42 supervisor_restart_redirect stale_pid=1368698";
        assert_eq!(r(tagged), Some(1368698));
        let tagged_recovery = recover(tagged).unwrap();
        assert_eq!(tagged_recovery.stale_pid, 1368698);
        assert_eq!(
            tagged_recovery.outcome.class,
            DispatchRecoveryOutcomeClass::Recoverable
        );
        assert_eq!(tagged_recovery.outcome.invariant_id, "stale_queue_pause");
        assert_eq!(
            tagged_recovery.outcome.proof_marker,
            DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER
        );
        assert_eq!(
            tagged_recovery.outcome.next_action,
            "restart_supervisor_once_and_retry"
        );
        // Marker present but no pid named → recoverable with pid 0 (proof line still emits).
        assert_eq!(
            r(
                "dispatch blocked: failed_stage=queue_paused reason=supervisor_binary_stale receipt_id=7 supervisor_restart_redirect stale_pid=0"
            ),
            Some(0)
        );
        // Legacy stale supervisors predate the redirect marker. Route must still recover
        // the stale-supervisor churn-stop instead of surfacing the JB Run Agent Doc error.
        assert_eq!(
            r(
                "project controller command `dispatch` failed: dispatch blocked for tasks/agent-doc/agent-doc-bugs2.md: failed_stage=queue_paused reason=#qchurn no-op churn: go-mode re-injecting :pushpin: [#qchurn] each idle boundary; all 16 heads are clean-session/operator-verify and undrainable under stale host supervisor pid 2715614; zero drainable work. Pausing to stop the flood until restart onto fresh binary. receipt_id=3649 blocked_head_bytes=132"
            ),
            Some(2715614)
        );
        // No marker → terminal (a deliberate operator/spent-preset pause never restarts).
        assert_eq!(
            r(
                "dispatch blocked for x.md: failed_stage=queue_paused reason=advance-review preset head is spent receipt_id=9"
            ),
            None
        );
        // A coalesce / stale-generation failure must not be misread as a restart redirect.
        assert_eq!(
            r("dispatch coalesced for x.md (#qflood); receipt_id=3"),
            None
        );
        assert!(recover("dispatch coalesced for x.md (#qflood); receipt_id=3").is_none());
        assert_eq!(
            r(
                "dispatch rejected: requested generation 1, current generation 2 (stale_generation_redirect retry_generation=2)"
            ),
            None
        );
    }
    #[test]
    fn schedule_stale_supervisor_cp_recycle_marks_doc_for_idle_recycle() {
        // `#fccsupwarn4`: a preflight-proven stale route-owned supervisor should
        // schedule the safe idle-boundary recycle automatically, not just tell the
        // operator to run `admin recycle` by hand.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(
            &file,
            "---\nagent_doc_supervisor_auto_recycle: false\n---\nbody\n",
        )
        .unwrap();

        assert!(
            agent_doc_supervisor_io::recycle_request::read_recycle_request(&file.to_string_lossy())
                .is_none(),
            "no request before a stale turn stage schedules one"
        );

        assert!(
            !agent_doc_supervisor_io::config::supervisor_auto_recycle_enabled(&file),
            "test must prove stale recycling overrides the proactive recycle opt-out"
        );
        let status = schedule_stale_supervisor_cp_recycle(&file, "finalize_start");
        assert!(
            status.contains("requested project_root="),
            "schedule status should prove the request was written: {status}"
        );
        assert!(
            status.contains("editor_replica_reregister=requested"),
            "schedule status should prove editor replica re-registration was requested: {status}"
        );

        let request =
            agent_doc_supervisor_io::recycle_request::read_recycle_request(&file.to_string_lossy())
                .expect("recycle-request present after stale-stage scheduling");
        assert_eq!(
            request.reason,
            agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_STALE_SUPERVISOR_TURN_STAGE
        );

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("stale_supervisor_cp_recycle_requested")
                && ops_log.contains("source=finalize_start")
                && ops_log.contains("reason=supervisor_binary_stale"),
            "ops log should record the automatic recycle request:\n{ops_log}"
        );
    }

    #[test]
    fn schedule_stale_editor_replica_cp_recycle_reports_no_live_registration_when_none_exists() {
        // `#mrnh` / `#ghosteditorliveness`: with no live editor registration there is
        // nothing to re-register, and the old unit-form signal reported a phantom
        // `editor_replica_reregister=requested` that never converged. The counted
        // outcome must instead report `no_live_registration` so session-check stops
        // implying an automatic recovery is pending.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body\n").unwrap();

        let status =
            schedule_stale_editor_replica_cp_recycle(&file, "session_check_terminal_convergence");
        assert!(
            status.contains("reason=no_live_editor")
                && status.contains("editor_replica_reregister=no_live_registration"),
            "with no live editor the status must be honest, not a phantom request: {status}"
        );
        assert!(
            !status.contains("editor_replica_reregister=requested"),
            "must not imply a re-registration was requested when there is no editor: {status}"
        );
        assert!(
            agent_doc_supervisor_io::recycle_request::read_recycle_request(&file.to_string_lossy())
                .is_none(),
            "a successfully published editor repair must not hot-reload a healthy supervisor"
        );
        assert!(!dir.path().join(".agent-doc/crdt-replica-events").exists());
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("stale_editor_replica_recovery_requested")
                && ops_log.contains("source=session_check_terminal_convergence")
                && ops_log.contains("action=reregister_editor_replica")
                && ops_log.contains("reason=editor_authority_unavailable_or_diverged"),
            "ops log should record editor authority recovery:\n{ops_log}"
        );
    }

    #[test]
    fn schedule_supervisor_recycle_marks_served_doc() {
        // `#turnsaferecycle` Goal 1: an install fan-out marks a served route-owned
        // document so its supervisor recycles at the next idle boundary.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();

        assert!(
            agent_doc_supervisor_io::recycle_request::read_recycle_request(&file.to_string_lossy())
                .is_none(),
            "no request before scheduling"
        );

        agent_doc_supervisor_io::recycle_request::request_recycle_for_doc(
            &file,
            agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_INSTALL_FANOUT,
        )
        .unwrap();

        let request =
            agent_doc_supervisor_io::recycle_request::read_recycle_request(&file.to_string_lossy())
                .expect("recycle-request present after scheduling");
        assert_eq!(
            request.reason,
            agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_INSTALL_FANOUT
        );
    }

    #[test]
    fn recycle_supervisors_all_projects_force_enumerates_without_error() {
        // `#turnsaferecycle` Goal 1: the /proc supervisor fan-out is fail-open and
        // returns a (marked, skipped) tally even when no route-owned supervisor is
        // running in this test environment.
        let (marked, skipped) = recycle_supervisors_all_projects_force(false).unwrap();
        // Fail-open: the tally is well-formed; no route-owned supervisor is expected
        // in the test environment, but the walk must not panic or error.
        let _total = marked + skipped;
    }

    #[test]
    fn recycle_debounce_decision_requires_continuous_idle_grace() {
        use agent_doc_controller::recycle::recycle_debounce_decision;

        // `#ctlrecycle` foundation: a recycle fires only after "wants-recycle AND
        // idle" holds continuously for the grace window, and any busy blip resets it.
        let grace = Duration::from_secs(5);
        let t0 = Instant::now();
        // Not idle-and-stale → no recycle, timer cleared.
        assert_eq!(
            recycle_debounce_decision(false, Some(t0), t0, grace),
            (false, None)
        );
        // First observation arms the timer but does not recycle yet.
        let (do_recycle, since) = recycle_debounce_decision(true, None, t0, grace);
        assert!(!do_recycle);
        assert_eq!(since, Some(t0));
        // Before the grace elapses → still no recycle, timer preserved.
        let t_mid = t0 + Duration::from_secs(2);
        assert_eq!(
            recycle_debounce_decision(true, since, t_mid, grace),
            (false, Some(t0))
        );
        // After the grace elapses while continuously idle-and-stale → recycle.
        let t_late = t0 + Duration::from_secs(6);
        assert_eq!(
            recycle_debounce_decision(true, since, t_late, grace),
            (true, Some(t0))
        );
        // A busy blip between samples resets the timer (no recycle, cleared).
        assert_eq!(
            recycle_debounce_decision(false, since, t_late, grace),
            (false, None)
        );
    }
    #[test]
    fn source_newer_than_installed_binary_strict() {
        use agent_doc_supervisor::config::source_newer_than_installed_binary;

        // `#supautoinstall`: only a STRICTLY newer source triggers an install; equal
        // timestamps (a just-installed binary / clock granularity) read as not-newer so a
        // boundary tie never re-builds.
        assert!(source_newer_than_installed_binary(101, 100));
        assert!(!source_newer_than_installed_binary(100, 100));
        assert!(!source_newer_than_installed_binary(99, 100));
    }

    #[test]
    fn resolve_supervisor_auto_install_default_on() {
        use agent_doc_supervisor::config::resolve_supervisor_auto_install;

        // `#supautoinstall`: default ON (mirrors recycle); env > frontmatter > project >
        // built-in ON. Safe to default ON because the build only fires for an agent-doc
        // dogfood session document (crate-root resolves).
        // Env override wins both ways, regardless of frontmatter/project.
        assert!(resolve_supervisor_auto_install(
            Some("1"),
            Some(false),
            Some(false)
        ));
        assert!(resolve_supervisor_auto_install(Some(" ON "), None, None));
        assert!(!resolve_supervisor_auto_install(
            Some("0"),
            Some(true),
            Some(true)
        ));
        assert!(!resolve_supervisor_auto_install(Some("off"), None, None));
        // Unrecognized env falls through to frontmatter, then project.
        assert!(!resolve_supervisor_auto_install(
            Some("maybe"),
            Some(false),
            None
        ));
        assert!(resolve_supervisor_auto_install(
            None,
            Some(true),
            Some(false)
        ));
        assert!(!resolve_supervisor_auto_install(None, None, Some(false)));
        // Absent everywhere → built-in default ON.
        assert!(resolve_supervisor_auto_install(None, None, None));
    }

    #[test]
    fn dogfood_crate_root_rejects_unrelated_superproject_docs() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let crate_root = root.join("src/agent-doc");
        std::fs::create_dir_all(crate_root.join("specs")).unwrap();
        std::fs::write(
            crate_root.join("Cargo.toml"),
            "[package]\nname = \"agent-doc\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();

        let write_doc = |relative: &str| {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "---\nagent_doc_format: template\n---\n").unwrap();
            path
        };

        let dogfood_doc = write_doc("tasks/agent-doc/agent-doc-bugs2.md");
        assert_eq!(
            dogfood_agent_doc_crate_root(&dogfood_doc),
            Some(crate_root.clone())
        );

        let legacy_root_doc = write_doc("tasks/agent-doc-bugs.md");
        assert_eq!(
            dogfood_agent_doc_crate_root(&legacy_root_doc),
            Some(crate_root.clone())
        );

        let software_agent_doc = write_doc("tasks/software/agent-doc.md");
        assert_eq!(
            dogfood_agent_doc_crate_root(&software_agent_doc),
            Some(crate_root.clone())
        );

        let source_doc = write_doc("src/agent-doc/specs/supervisor.md");
        assert_eq!(
            dogfood_agent_doc_crate_root(&source_doc),
            Some(crate_root.clone())
        );

        let efs_doc = write_doc("tasks/professional/sampleportal.md");
        assert!(
            dogfood_agent_doc_crate_root(&efs_doc).is_none(),
            "unrelated project sessions must not inherit agent-doc auto-install"
        );

        let lazily_doc = write_doc("tasks/software/lazily-rs.md");
        assert!(
            dogfood_agent_doc_crate_root(&lazily_doc).is_none(),
            "sibling software sessions must not inherit agent-doc auto-install"
        );
    }

    #[test]
    fn handoff_drop_guard_aborted_handoff_sends_shutdown_and_logs() {
        // An aborted handoff (guard dropped before `complete`) must tell the
        // half-launched replacement on the temp socket to shut down, and record it.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let temp_sock = dir
            .path()
            .join(".agent-doc")
            .join("controller-handoff.sock");

        // Stand up a one-shot listener standing in for the Preparing replacement so
        // we can prove the exact `shutdown` command crosses the socket. Binding on
        // this thread before spawning means the guard's connect always succeeds.
        let name = temp_sock.clone().to_fs_name::<GenericFilePath>().unwrap();
        let listener = ListenerOptions::new().name(name).create_sync().unwrap();
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let server = std::thread::spawn(move || {
            let stream = listener.accept().unwrap();
            let (reader_half, mut writer_half) = stream.split();
            let mut reader = BufReader::new(reader_half);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            // Respond so the guard's bounded `request_path` read returns promptly.
            writer_half.write_all(b"{\"ok\":true}\n").unwrap();
            writer_half.flush().unwrap();
            tx.send(line).unwrap();
        });

        {
            let _guard = HandoffDropGuard::new(dir.path(), &temp_sock);
            // Dropped here without `complete()` ⇒ abort path fires.
        }

        let received = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("aborted drop-guard must send a request to the replacement");
        assert!(
            received.contains("\"command\":\"shutdown\""),
            "aborted handoff must send shutdown, got: {received}"
        );
        server.join().unwrap();

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("handoff_drop_guard_aborted_handoff_shutdown"));
        assert!(ops_log.contains(&format!("temp_sock={}", temp_sock.display())));
    }
    #[test]
    fn handoff_drop_guard_completed_handoff_does_not_shut_down() {
        // The success path calls `complete()`: a promoted, now-authoritative
        // controller must never be shut down or logged as an aborted handoff.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let temp_sock = dir
            .path()
            .join(".agent-doc")
            .join("controller-handoff.sock");
        {
            let mut guard = HandoffDropGuard::new(dir.path(), &temp_sock);
            guard.complete();
            // Dropped here after `complete()` ⇒ shutdown branch must be skipped.
        }
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            !ops_log.contains("handoff_drop_guard_aborted_handoff_shutdown"),
            "a completed handoff must not log an aborted shutdown"
        );
    }
    #[test]
    fn controller_serve_project_root_from_args_rejects_non_controllers() {
        use agent_doc_controller::command_line::controller_serve_project_root_from_args;

        // `controller serve` window present but no `--project-root`.
        assert_eq!(
            controller_serve_project_root_from_args(&[
                "/bin/agent-doc".to_string(),
                "controller".to_string(),
                "serve".to_string(),
            ]),
            None
        );
        // An agent-doc invocation that is not `controller serve`.
        assert_eq!(
            controller_serve_project_root_from_args(&[
                "/bin/agent-doc".to_string(),
                "status".to_string(),
                "--project-root".to_string(),
                "/x".to_string(),
            ]),
            None
        );
        // Not an agent-doc process at all (no arg ends with `agent-doc`).
        assert_eq!(
            controller_serve_project_root_from_args(&[
                "sleep".to_string(),
                "controller".to_string(),
                "serve".to_string(),
                "--project-root".to_string(),
                "/x".to_string(),
            ]),
            None
        );
    }

    #[test]
    fn start_after_pane_move_against_up_to_date_actor_bumps_past_cas() {
        // Regression for the live `controller failed to start session actor`
        // wall: a session whose controller actor has already caught up to the
        // latest generation (the healthy steady state) is re-started from a
        // launcher pane that differs from the registry's stale pane. The start
        // path must mint a NEW generation (`infer + 1`) so the controller's
        // unconditional `start_generation - 1` CAS holds. Handing the
        // un-incremented current generation — the old no-bump
        // `infer_latest_generation` branch — fails closed with
        // `compare-and-swap failed: expected N-1, found N`.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/efs.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: efs\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let doc_id = agent_doc_session_actor_io::canonical_document_id_in(
            &bootstrap.project_root,
            &doc.to_string_lossy(),
        );

        // Seed an up-to-date controller actor at generation 83 on the OLD pane
        // %33 (mirrors the migrated-but-registry-lagging live state).
        let seeded = agent_doc_sqlite::state_store::ActorRecord {
            document_id: doc_id.clone(),
            session_id: "efs".to_string(),
            generation: 83,
            pane_id: "%33".to_string(),
            window_id: "@9".to_string(),
            harness: agent_doc_session_actor_io::detect_document_harness_in(
                &bootstrap.project_root,
                &doc_id,
            ),
            state: agent_doc_sqlite::state_store::ActorState::Ready,
            last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
                caller: "supervisor".to_string(),
                reason: "idle_pane_reconcile".to_string(),
                timestamp: 1,
                prior_generation: 82,
                new_generation: 83,
            },
        };
        store_actor_record(&bootstrap.project_root, None, &seeded).unwrap();

        // The start path now unconditionally takes the `next_generation` branch:
        // it infers 83 from the up-to-date controller actor and returns 84.
        let generations = agent_doc_session_actor_io::next_generation(&doc, "efs").unwrap();
        assert_eq!(generations.prior_generation, 83);
        assert_eq!(generations.new_generation, 84);

        let start_at = |generation: u64| ControllerRequest {
            command: "start_session".to_string(),
            file: Some(doc.clone()),
            session_id: Some("efs".to_string()),
            pane_id: Some("%44".to_string()),
            window_id: Some("@150".to_string()),
            generation: Some(generation),
            state: None,
            caller: Some("start".to_string()),
            reason: Some("session_start".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };

        // Contract guard: the OLD no-bump value (the un-incremented current
        // generation 83) is exactly what failed closed. The controller wraps the
        // inner CAS bail in the user-visible "failed to start session actor"
        // context, so assert against the full error chain.
        let stale = handle_start_session(&bootstrap, None, start_at(83)).unwrap_err();
        assert!(
            format!("{stale:#}").contains("compare-and-swap failed"),
            "un-bumped start must fail the CAS: {stale:#}"
        );

        // The bumped generation the fix now produces satisfies the CAS
        // (expected prior 83 == found 83) and re-asserts ownership on pane %44.
        let record = handle_start_session(&bootstrap, None, start_at(generations.new_generation))
            .expect("bumped-generation start must pass the CAS");
        assert_eq!(record.generation, 84);
        assert_eq!(record.pane_id, "%44");
    }

    fn ready_actor_for_start_alias_test(
        bootstrap: &ControllerBootstrap,
        document_id: &str,
        session_id: &str,
        generation: u64,
        pane_id: &str,
        window_id: &str,
    ) -> agent_doc_sqlite::state_store::ActorRecord {
        agent_doc_sqlite::state_store::ActorRecord {
            document_id: document_id.to_string(),
            session_id: session_id.to_string(),
            generation,
            pane_id: pane_id.to_string(),
            window_id: window_id.to_string(),
            harness: agent_doc_session_actor_io::detect_document_harness_in(
                &bootstrap.project_root,
                document_id,
            ),
            state: agent_doc_sqlite::state_store::ActorState::Ready,
            last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
                caller: "supervisor".to_string(),
                reason: "idle".to_string(),
                timestamp: 1,
                prior_generation: generation.saturating_sub(1),
                new_generation: generation,
            },
        }
    }

    fn start_session_request_for_alias_test(
        file: &Path,
        session_id: &str,
        pane_id: &str,
        window_id: &str,
        generation: u64,
    ) -> ControllerRequest {
        ControllerRequest {
            command: "start_session".to_string(),
            file: Some(file.to_path_buf()),
            session_id: Some(session_id.to_string()),
            pane_id: Some(pane_id.to_string()),
            window_id: Some(window_id.to_string()),
            generation: Some(generation),
            state: None,
            caller: Some("start".to_string()),
            reason: Some("session_start".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        }
    }

    fn save_registry_entry_for_alias_test(
        project_root: &Path,
        document_id: &str,
        session_id: &str,
        pane_id: &str,
        window_id: &str,
        file: &Path,
        pid: u32,
    ) {
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            document_id.to_string(),
            tmux_router::RegistryEntry {
                pane: pane_id.to_string(),
                pid,
                cwd: project_root.to_string_lossy().to_string(),
                started: "test".to_string(),
                session_id: session_id.to_string(),
                file: file.to_string_lossy().to_string(),
                window: window_id.to_string(),
                supervisor_instance_id: "test-supervisor".to_string(),
            },
        );
        agent_doc_session_registry_io::save_in(project_root, &registry).unwrap();
    }

    #[test]
    fn start_session_closes_stale_cross_document_pane_alias_without_live_claim() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let owner_doc = dir.path().join("tasks/owner.md");
        let candidate_doc = dir.path().join("tasks/candidate.md");
        std::fs::write(
            &owner_doc,
            "---\nagent_doc_session: owner\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        std::fs::write(
            &candidate_doc,
            "---\nagent_doc_session: candidate\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let owner_doc_id = agent_doc_session_actor_io::canonical_document_id_in(
            &bootstrap.project_root,
            &owner_doc.to_string_lossy(),
        );
        let candidate_doc_id = agent_doc_session_actor_io::canonical_document_id_in(
            &bootstrap.project_root,
            &candidate_doc.to_string_lossy(),
        );

        let owner =
            ready_actor_for_start_alias_test(&bootstrap, &owner_doc_id, "owner", 1131, "%4", "@3");
        store_actor_record(&bootstrap.project_root, None, &owner).unwrap();

        let record = handle_start_session(
            &bootstrap,
            None,
            start_session_request_for_alias_test(&candidate_doc, "candidate", "%4", "@3", 1),
        )
        .expect("stale cross-document pane alias should be closed before start");

        assert_eq!(record.document_id, candidate_doc_id);
        assert_eq!(record.pane_id, "%4");
        assert_eq!(
            record.state,
            agent_doc_sqlite::state_store::ActorState::Starting
        );

        let store = load_actor_store(&bootstrap.project_root).unwrap();
        let owner_after = store
            .get(&owner_doc_id)
            .expect("stale owner should remain as closed history");
        assert_eq!(
            owner_after.state,
            agent_doc_sqlite::state_store::ActorState::Closed
        );
        assert!(owner_after.pane_id.is_empty());
        assert!(owner_after.window_id.is_empty());
        assert_eq!(owner_after.last_transition.caller, "start");
        assert!(
            owner_after
                .last_transition
                .reason
                .contains("stale_cross_document_pane_alias"),
            "owner transition should record stale-alias repair: {}",
            owner_after.last_transition.reason
        );
    }

    #[test]
    fn start_session_closes_stale_alias_when_registry_pid_is_dead() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let owner_doc = dir.path().join("tasks/owner.md");
        let candidate_doc = dir.path().join("tasks/candidate.md");
        std::fs::write(
            &owner_doc,
            "---\nagent_doc_session: owner\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        std::fs::write(
            &candidate_doc,
            "---\nagent_doc_session: candidate\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let owner_doc_id = agent_doc_session_actor_io::canonical_document_id_in(
            &bootstrap.project_root,
            &owner_doc.to_string_lossy(),
        );

        let owner =
            ready_actor_for_start_alias_test(&bootstrap, &owner_doc_id, "owner", 7, "%44", "@9");
        store_actor_record(&bootstrap.project_root, None, &owner).unwrap();
        save_registry_entry_for_alias_test(
            &bootstrap.project_root,
            &owner_doc_id,
            "owner",
            "%44",
            "@9",
            &owner_doc,
            u32::MAX,
        );

        handle_start_session(
            &bootstrap,
            None,
            start_session_request_for_alias_test(&candidate_doc, "candidate", "%44", "@10", 1),
        )
        .expect("dead durable registry entry should not keep stale pane alias live");

        let store = load_actor_store(&bootstrap.project_root).unwrap();
        assert_eq!(
            store.get(&owner_doc_id).unwrap().state,
            agent_doc_sqlite::state_store::ActorState::Closed
        );
    }

    #[test]
    fn start_session_refuses_cross_document_alias_with_live_registry_entry() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let owner_doc = dir.path().join("tasks/owner.md");
        let candidate_doc = dir.path().join("tasks/candidate.md");
        std::fs::write(
            &owner_doc,
            "---\nagent_doc_session: owner\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        std::fs::write(
            &candidate_doc,
            "---\nagent_doc_session: candidate\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let owner_doc_id = agent_doc_session_actor_io::canonical_document_id_in(
            &bootstrap.project_root,
            &owner_doc.to_string_lossy(),
        );
        let candidate_doc_id = agent_doc_session_actor_io::canonical_document_id_in(
            &bootstrap.project_root,
            &candidate_doc.to_string_lossy(),
        );

        let owner =
            ready_actor_for_start_alias_test(&bootstrap, &owner_doc_id, "owner", 7, "%44", "@9");
        store_actor_record(&bootstrap.project_root, None, &owner).unwrap();
        save_registry_entry_for_alias_test(
            &bootstrap.project_root,
            &owner_doc_id,
            "owner",
            "%44",
            "@9",
            &owner_doc,
            std::process::id(),
        );

        let err = handle_start_session(
            &bootstrap,
            None,
            start_session_request_for_alias_test(&candidate_doc, "candidate", "%44", "@10", 1),
        )
        .unwrap_err();
        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("cross-document actor pane alias"),
            "live durable registry entry must block cross-document pane takeover: {rendered}"
        );

        let store = load_actor_store(&bootstrap.project_root).unwrap();
        assert_eq!(
            store.get(&owner_doc_id).unwrap().state,
            agent_doc_sqlite::state_store::ActorState::Ready
        );
        assert!(
            !store.contains_key(&candidate_doc_id),
            "candidate start must not replace a registry-backed live owner"
        );
    }

    #[test]
    fn start_session_refuses_nonclosed_cross_document_pane_alias() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let owner_doc = dir.path().join("tasks/owner.md");
        let candidate_doc = dir.path().join("tasks/candidate.md");
        std::fs::write(
            &owner_doc,
            "---\nagent_doc_session: owner\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        std::fs::write(
            &candidate_doc,
            "---\nagent_doc_session: candidate\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let owner_doc_id = agent_doc_session_actor_io::canonical_document_id_in(
            &bootstrap.project_root,
            &owner_doc.to_string_lossy(),
        );
        let candidate_doc_id = agent_doc_session_actor_io::canonical_document_id_in(
            &bootstrap.project_root,
            &candidate_doc.to_string_lossy(),
        );

        let owner = agent_doc_sqlite::state_store::ActorRecord {
            document_id: owner_doc_id.clone(),
            session_id: "owner".to_string(),
            generation: 7,
            pane_id: "%44".to_string(),
            window_id: "@9".to_string(),
            harness: agent_doc_session_actor_io::detect_document_harness_in(
                &bootstrap.project_root,
                &owner_doc_id,
            ),
            state: agent_doc_sqlite::state_store::ActorState::Ready,
            last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
                caller: "supervisor".to_string(),
                reason: "idle".to_string(),
                timestamp: 1,
                prior_generation: 6,
                new_generation: 7,
            },
        };
        store_actor_record(&bootstrap.project_root, None, &owner).unwrap();
        upsert_supervisor_lease(
            &bootstrap.project_root,
            &owner,
            Some(std::process::id()),
            None,
            "ready",
        )
        .unwrap();

        let err = handle_start_session(
            &bootstrap,
            None,
            ControllerRequest {
                command: "start_session".to_string(),
                file: Some(candidate_doc.clone()),
                session_id: Some("candidate".to_string()),
                pane_id: Some("%44".to_string()),
                window_id: Some("@10".to_string()),
                generation: Some(1),
                state: None,
                caller: Some("start".to_string()),
                reason: Some("session_start".to_string()),
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: None,
                diagnostic_payload: None,
            },
        )
        .unwrap_err();
        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("cross-document actor pane alias"),
            "start_session must refuse a pane claimed by another document: {rendered}"
        );

        let store = load_actor_store(&bootstrap.project_root).unwrap();
        assert_eq!(
            store.get(&owner_doc_id).unwrap().state,
            agent_doc_sqlite::state_store::ActorState::Ready
        );
        assert!(
            !store.contains_key(&candidate_doc_id),
            "candidate start must not evict or replace the existing owner"
        );
    }

    #[test]
    fn start_session_allows_same_document_pane_alias_from_legacy_document_key() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks/professional")).unwrap();
        let doc = dir.path().join("tasks/professional/sampleportal.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: efs\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let canonical_doc_id = agent_doc_session_actor_io::canonical_document_id_in(
            &bootstrap.project_root,
            &doc.to_string_lossy(),
        );
        let legacy_doc_id = "tasks/professional/sampleportal.md".to_string();

        let legacy_actor = agent_doc_sqlite::state_store::ActorRecord {
            document_id: legacy_doc_id.clone(),
            session_id: "efs".to_string(),
            generation: 1131,
            pane_id: "%4".to_string(),
            window_id: "@3".to_string(),
            harness: agent_doc_session_actor_io::detect_document_harness_in(
                &bootstrap.project_root,
                &legacy_doc_id,
            ),
            state: agent_doc_sqlite::state_store::ActorState::Ready,
            last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
                caller: "sync".to_string(),
                reason: "prompt_ready".to_string(),
                timestamp: 1,
                prior_generation: 1130,
                new_generation: 1131,
            },
        };
        store_actor_record(&bootstrap.project_root, None, &legacy_actor).unwrap();

        let record = handle_start_session(
            &bootstrap,
            None,
            ControllerRequest {
                command: "start_session".to_string(),
                file: Some(doc.clone()),
                session_id: Some("efs".to_string()),
                pane_id: Some("%4".to_string()),
                window_id: Some("@3".to_string()),
                generation: Some(1132),
                state: None,
                caller: Some("start".to_string()),
                reason: Some("session_start".to_string()),
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: None,
                diagnostic_payload: None,
            },
        )
        .expect("start_session must accept an equivalent same-document pane alias");

        assert_eq!(record.document_id, canonical_doc_id);
        assert_eq!(record.generation, 1132);
        assert_eq!(
            record.state,
            agent_doc_sqlite::state_store::ActorState::Starting
        );
        assert_eq!(record.pane_id, "%4");

        let store = load_actor_store(&bootstrap.project_root).unwrap();
        let legacy_after = store
            .get(&legacy_doc_id)
            .expect("legacy same-document alias should remain as closed history");
        assert_eq!(
            legacy_after.state,
            agent_doc_sqlite::state_store::ActorState::Closed
        );
        assert!(legacy_after.pane_id.is_empty());
        assert!(legacy_after.window_id.is_empty());
        let canonical_after = store
            .get(&canonical_doc_id)
            .expect("canonical document actor should be stored");
        assert_eq!(
            canonical_after.state,
            agent_doc_sqlite::state_store::ActorState::Starting
        );
        assert_eq!(canonical_after.pane_id, "%4");
    }

    #[test]
    fn start_session_trims_invisible_same_document_alias_drift_before_live_claim_check() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks/professional")).unwrap();
        let doc = dir.path().join("tasks/professional/sampleportal.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: efs\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let canonical_doc_id = agent_doc_session_actor_io::canonical_document_id_in(
            &bootstrap.project_root,
            &doc.to_string_lossy(),
        );
        let dirty_doc_id = format!("{canonical_doc_id}\n");

        let dirty_actor = ready_actor_for_start_alias_test(
            &bootstrap,
            &dirty_doc_id,
            "efs-old",
            1131,
            "%4",
            "@3",
        );
        store_actor_record(&bootstrap.project_root, None, &dirty_actor).unwrap();
        save_registry_entry_for_alias_test(
            &bootstrap.project_root,
            &dirty_doc_id,
            "efs-old",
            "%4",
            "@3",
            &doc,
            std::process::id(),
        );

        let record = handle_start_session(
            &bootstrap,
            None,
            start_session_request_for_alias_test(&doc, "efs", "%4", "@3", 1132),
        )
        .expect("invisible same-document id drift must not block start_session");

        assert_eq!(record.document_id, canonical_doc_id);
        assert_eq!(record.generation, 1132);
        assert_eq!(
            record.state,
            agent_doc_sqlite::state_store::ActorState::Starting
        );

        let store = load_actor_store(&bootstrap.project_root).unwrap();
        assert_eq!(
            store.get(&dirty_doc_id).unwrap().state,
            agent_doc_sqlite::state_store::ActorState::Closed
        );
    }

    #[test]
    fn start_session_allows_same_session_pane_alias_after_restart_key_drift() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks/professional")).unwrap();
        let doc = dir.path().join("tasks/professional/sampleportal.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: efs\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let canonical_doc_id = agent_doc_session_actor_io::canonical_document_id_in(
            &bootstrap.project_root,
            &doc.to_string_lossy(),
        );
        let stale_doc_key = format!("{canonical_doc_id}.stale-restart-key");

        let stale_actor =
            ready_actor_for_start_alias_test(&bootstrap, &stale_doc_key, "efs", 1131, "%4", "@3");
        store_actor_record(&bootstrap.project_root, None, &stale_actor).unwrap();
        save_registry_entry_for_alias_test(
            &bootstrap.project_root,
            &stale_doc_key,
            "efs",
            "%4",
            "@3",
            &doc,
            std::process::id(),
        );

        let record = handle_start_session(
            &bootstrap,
            None,
            start_session_request_for_alias_test(&doc, "efs", "%4", "@3", 1132),
        )
        .expect("same-session same-pane restart must not be treated as cross-document");

        assert_eq!(record.document_id, canonical_doc_id);
        assert_eq!(record.generation, 1132);
        assert_eq!(record.pane_id, "%4");
        assert_eq!(
            record.state,
            agent_doc_sqlite::state_store::ActorState::Starting
        );

        let store = load_actor_store(&bootstrap.project_root).unwrap();
        assert_eq!(
            store.get(&stale_doc_key).unwrap().state,
            agent_doc_sqlite::state_store::ActorState::Closed
        );
    }

    #[test]
    fn start_session_reopens_closed_same_document_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/closed-efs.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: closed-efs\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let doc_id = agent_doc_session_actor_io::canonical_document_id_in(
            &bootstrap.project_root,
            &doc.to_string_lossy(),
        );

        let closed = agent_doc_sqlite::state_store::ActorRecord {
            document_id: doc_id.clone(),
            session_id: "closed-efs".to_string(),
            generation: 1096,
            pane_id: String::new(),
            window_id: String::new(),
            harness: agent_doc_session_actor_io::detect_document_harness_in(
                &bootstrap.project_root,
                &doc_id,
            ),
            state: agent_doc_sqlite::state_store::ActorState::Closed,
            last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
                caller: "sync".to_string(),
                reason: "stale_dead_pane_actor".to_string(),
                timestamp: 1,
                prior_generation: 1096,
                new_generation: 1096,
            },
        };
        store_actor_record(&bootstrap.project_root, None, &closed).unwrap();

        let record = handle_start_session(
            &bootstrap,
            None,
            ControllerRequest {
                command: "start_session".to_string(),
                file: Some(doc.clone()),
                session_id: Some("closed-efs".to_string()),
                pane_id: Some("%162".to_string()),
                window_id: Some("@79".to_string()),
                generation: Some(1097),
                state: None,
                caller: Some("start".to_string()),
                reason: Some("session_start".to_string()),
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: None,
                diagnostic_payload: None,
            },
        )
        .expect("start_session must reopen a closed same-document generation");

        assert_eq!(record.generation, 1097);
        assert_eq!(
            record.state,
            agent_doc_sqlite::state_store::ActorState::Starting
        );
        assert_eq!(record.pane_id, "%162");
    }

    fn reliable_sync_env_lock() -> parking_lot::MutexGuard<'static, ()> {
        static LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
        LOCK.lock()
    }

    fn reliable_sync_open_request(document_hash: &str) -> ControllerRequest {
        use agent_doc_reliable_sync_io::liveness::{LivenessOp, encode_liveness_frame};
        // A valid 3A reliable-sync envelope carrying one liveness Open op.
        let frame = encode_liveness_frame(&[LivenessOp::Open {
            document_hash: document_hash.into(),
            pid: 100,
            tag: "t1".into(),
        }])
        .expect("encode liveness frame");
        let envelope = agent_doc_reliable_sync_io::encode_envelope(document_hash, &frame)
            .expect("encode envelope");
        ControllerRequest {
            command: "reliable_sync".to_string(),
            file: None,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: Some(5),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(envelope.to_string()),
        }
    }

    #[test]
    fn reliable_sync_handler_folds_frame() {
        let _env = reliable_sync_env_lock();
        // The authoritative plane folds the inbound liveness frame and advances
        // the durable ACK cursor; there is no disabled compatibility mode.
        let dir = tempfile::tempdir().unwrap();
        let resp = handle_reliable_sync(dir.path(), reliable_sync_open_request("docwire-on"))
            .expect("handler ok");
        assert!(resp.accepted);
        assert_eq!(resp.document_hash, "docwire-on");
        // Folded ⇒ the per-channel ack cursor advanced past the initial 0.
        assert!(resp.ack_through >= 1);
    }

    /// Build the three state events that record a visible-write receipt, the same
    /// way the observed/direct recording paths do.
    fn visible_write_receipt_events(
        document_hash: &str,
        patch_id: &str,
        content: &str,
    ) -> Vec<agent_doc_state_backbone::StateEvent> {
        let payload = VisibleWriteCommitCandidatePayload {
            patch_id: patch_id.to_string(),
            model_revision: 7,
            editor_visible_hash: visible_write_commit_candidate_hash(content),
            commit_candidate_hash: visible_write_commit_candidate_hash(content),
            commit_candidate_content: content.to_string(),
            source: "test_receipt".to_string(),
        };
        let (generation, applied, proof) =
            visible_write_commit_candidate_events(document_hash, &payload);
        vec![generation, applied, proof]
    }

    /// `#lazily-hot-path` W1 — a receipt that already folded answers without waiting.
    #[test]
    fn visible_write_receipt_await_returns_a_receipt_that_already_folded() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("receipt-present.md");
        std::fs::write(&file, "# receipt\n").unwrap();
        let bootstrap = test_bootstrap(&dir);
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        for event in visible_write_receipt_events(&document_hash, "patch-present", "visible text") {
            append_state_event(&bootstrap.project_root, &event).unwrap();
        }
        let runtime = ControllerRuntime::new(bootstrap).unwrap();

        let started = Instant::now();
        let proof = runtime
            .wait_for_visible_write_commit_candidate_patch(
                &document_hash,
                "patch-present",
                Duration::from_secs(30),
            )
            .expect("an already-folded receipt must answer immediately");

        assert_eq!(proof.patch_id, "patch-present");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "an already-folded receipt must not wait on the deadline"
        );
    }

    /// `#lazily-hot-path` W1 — THE property this primitive exists for: the waiter is
    /// woken by the append that records the fact, not by its own timer. A waiter that
    /// silently degraded to polling-until-deadline would still return the receipt, so
    /// the assertion is on elapsed time against a deliberately long deadline.
    #[test]
    fn visible_write_receipt_await_wakes_on_the_recording_append() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("receipt-pushed.md");
        std::fs::write(&file, "# receipt\n").unwrap();
        let bootstrap = test_bootstrap(&dir);
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        let runtime = ControllerRuntime::new_arc(bootstrap).unwrap();

        let recorder = Arc::clone(&runtime);
        let recorded_hash = document_hash.clone();
        let recorder = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            for event in visible_write_receipt_events(&recorded_hash, "patch-pushed", "pushed text")
            {
                recorder.apply_state_event(&event).unwrap();
            }
        });

        let started = Instant::now();
        let proof = runtime
            .wait_for_visible_write_commit_candidate_patch(
                &document_hash,
                "patch-pushed",
                Duration::from_secs(30),
            )
            .expect("the recording append must wake the waiter");
        let elapsed = started.elapsed();
        recorder.join().unwrap();

        assert_eq!(proof.patch_id, "patch-pushed");
        assert!(
            elapsed < Duration::from_secs(10),
            "waiter must be woken by the append, not by its own deadline (elapsed {elapsed:?})"
        );
    }

    /// `#lazily-hot-path` W1 — the await is bounded by the caller's deadline and
    /// fails open (no receipt) instead of hanging, so a caller can still fall back to
    /// its authoritative read.
    #[test]
    fn visible_write_receipt_await_is_bounded_by_the_caller_deadline() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("receipt-absent.md");
        std::fs::write(&file, "# receipt\n").unwrap();
        let bootstrap = test_bootstrap(&dir);
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        let runtime = ControllerRuntime::new(bootstrap).unwrap();

        let started = Instant::now();
        let proof = runtime.wait_for_visible_write_commit_candidate_patch(
            &document_hash,
            "patch-never-recorded",
            Duration::from_millis(120),
        );

        assert!(proof.is_none(), "no receipt was ever recorded");
        assert!(
            started.elapsed() >= Duration::from_millis(120),
            "the await must consume the caller's deadline rather than spinning"
        );
    }

    /// The await command answers with the same proof shape as the status command, so
    /// swapping a poll for a push cannot change what the caller observes.
    #[test]
    fn visible_write_receipt_await_command_matches_the_status_command() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("receipt-command.md");
        std::fs::write(&file, "# receipt\n").unwrap();
        let bootstrap = test_bootstrap(&dir);
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        for event in visible_write_receipt_events(&document_hash, "patch-cmd", "command text") {
            append_state_event(&bootstrap.project_root, &event).unwrap();
        }
        let runtime = ControllerRuntime::new(bootstrap.clone()).unwrap();

        let request_for = |command: &str, payload: serde_json::Value| {
            let mut request = empty_controller_request(command);
            request.file = Some(file.clone());
            request.diagnostic_payload = Some(payload.to_string());
            request
        };

        let status = handle_visible_write_commit_candidate_patch_status(
            &bootstrap,
            &runtime,
            request_for(
                "visible_write_commit_candidate_patch_status",
                serde_json::json!({ "patch_id": "patch-cmd" }),
            ),
        )
        .expect("status handler ok");
        let awaited = handle_visible_write_commit_candidate_patch_await(
            &bootstrap,
            &runtime,
            request_for(
                "visible_write_commit_candidate_patch_await",
                serde_json::json!({ "patch_id": "patch-cmd", "wait_ms": 30_000 }),
            ),
        )
        .expect("await handler ok");

        assert_eq!(
            status.proof.map(|proof| proof.commit_candidate_hash),
            awaited.proof.map(|proof| proof.commit_candidate_hash),
        );

        // An absent receipt with a zero wait answers `None` immediately instead of
        // falling back to the controller ceiling.
        let started = Instant::now();
        let missing = handle_visible_write_commit_candidate_patch_await(
            &bootstrap,
            &runtime,
            request_for(
                "visible_write_commit_candidate_patch_await",
                serde_json::json!({ "patch_id": "patch-absent", "wait_ms": 0 }),
            ),
        )
        .expect("await handler ok");
        assert!(missing.proof.is_none());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    /// `#lazily-hot-path` Theme A — a controller that hosts no hub for the document
    /// must say "not observed" immediately, not wait out the deadline and not claim
    /// convergence. This is the failure the `Option` return exists to prevent: a
    /// waiter told `converged` about a document nobody can see would stop waiting for
    /// a delivery that is still in flight somewhere else.
    #[test]
    fn delivery_convergence_await_reports_unobserved_without_burning_the_deadline() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("no-hub.md");
        std::fs::write(&file, "# no hub\n").unwrap();
        let bootstrap = test_bootstrap(&dir);
        let runtime = ControllerRuntime::new(bootstrap.clone()).unwrap();

        let mut request = empty_controller_request("delivery_convergence_await");
        request.file = Some(file.clone());
        request.diagnostic_payload = Some(serde_json::json!({ "wait_ms": 30_000 }).to_string());

        let started = Instant::now();
        let status = handle_delivery_convergence_await(&bootstrap, &runtime, request)
            .expect("await handler ok");

        assert!(
            !status.observed,
            "this process hosts no hub for the document"
        );
        assert!(
            !status.converged,
            "an unobservable document must never be reported as converged"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "an absent hub is known immediately; it must not wait out the deadline"
        );
    }

    /// Carry-forward guardrail for this migrated seam (`#lazily-hot-path`): once a
    /// coordination fact has a Lazily cell, its decision site reads the in-memory
    /// projection and must not re-derive the answer from durable storage. Without
    /// this, a later "just reload it here to be safe" edit silently reintroduces the
    /// per-waiter ledger fold this primitive exists to delete.
    #[test]
    fn visible_write_receipt_await_decides_from_lazily_state_not_durable_storage() {
        let source = include_str!("../project_controller.rs");
        let start = source
            .find("fn wait_for_visible_write_commit_candidate_patch(")
            .expect("the visible-write receipt await must exist");
        let body = &source[start..];
        let end = body
            .find("\n    fn ")
            .expect("the await must be followed by another method");
        let body = &body[..end];

        assert!(
            body.contains("state_projection"),
            "the await must decide from the in-memory Lazily projection"
        );
        for forbidden in [
            "open_state_db",
            "load_state_backbone_projection",
            "load_state_event_ledger",
            "flock",
        ] {
            assert!(
                !body.contains(forbidden),
                "visible-write receipt await must not arbitrate from `{forbidden}` \
                 (#lazily-hot-path: durable storage is a sink, not a decision authority)"
            );
        }
    }

    #[test]
    fn state_subscribe_records_only_the_previously_applied_live_peer_cursor() {
        let _env = reliable_sync_env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("peer-ack.md");
        std::fs::write(&file, "# peer ack\n").unwrap();
        let bootstrap = test_bootstrap(&dir);
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        for (cycle, current) in [("cycle-1", "one"), ("cycle-2", "two")] {
            let event = realtime_steering_event_for_text(&document_hash, cycle, "", current);
            append_state_event(&bootstrap.project_root, &event).unwrap();
        }
        let runtime = ControllerRuntime::new(bootstrap.clone()).unwrap();

        let registration = agent_doc_reliable_sync_io::liveness::EditorRegistration {
            document_hash: document_hash.clone(),
            pid: 100,
            path: file.to_string_lossy().into_owned(),
            editor_id: "jetbrains-100-fmgc".to_string(),
            editor_kind: "jetbrains".to_string(),
            editor_version: "test".to_string(),
            capabilities: vec![],
            timestamp_ms: 1,
        };
        {
            let mut plane = controller_liveness_plane().lock();
            *plane = agent_doc_reliable_sync_io::plane::ControllerLivenessPlane::new();
            plane.restore_liveness(&[
                agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                    document_hash: document_hash.clone(),
                    pid: 100,
                    tag: "open-fmgc".to_string(),
                },
                agent_doc_reliable_sync_io::liveness::LivenessOp::Register(registration),
            ]);
        }

        let subscribe = |acked_version| {
            let mut request = empty_controller_request("state_subscribe");
            request.file = Some(file.clone());
            request.generation = Some(0);
            request.diagnostic_payload = Some(
                serde_json::json!({
                    "document_hash": document_hash,
                    "peer_pid": 100,
                    "editor_id": "jetbrains-100-fmgc",
                    "acked_version": acked_version,
                })
                .to_string(),
            );
            handle_state_subscribe(&runtime, request).unwrap()
        };

        let delivered = subscribe(0);
        assert_eq!(delivered.document_version, 2);
        assert!(
            !delivered.peer_ack_recorded,
            "the response being delivered is not acknowledged optimistically"
        );
        let acknowledged = subscribe(delivered.document_version);
        assert!(acknowledged.peer_ack_recorded);

        let conn = agent_doc_sqlite::state_store::open_state_db(&bootstrap.project_root).unwrap();
        let rows = agent_doc_sqlite::state_store::load_state_event_peer_acks_from_db(
            &conn,
            &document_hash,
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].acked_version, 2);
        assert_eq!(rows[0].registration_pid, 100);
        let remaining_versions: Vec<i64> = conn
            .prepare(
                "SELECT document_version FROM state_events \
                 WHERE document_hash = ?1 ORDER BY document_version",
            )
            .unwrap()
            .query_map([&document_hash], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(
            remaining_versions,
            vec![2],
            "the controller wires the exact live registration into delete-below retention"
        );
        let cold_runtime = ControllerRuntime::new(bootstrap.clone()).unwrap();
        let (cold_wire, cold_version) = cold_runtime.state_subscribe(&document_hash, 0).unwrap();
        let mut cold_message = serde_json::to_value(
            agent_doc_state_wire::lazily_convert::wire_subscribe_to_ipc_message(&cold_wire)
                .unwrap(),
        )
        .unwrap();
        let mut expected_message = acknowledged.message.clone();
        for message in [&mut cold_message, &mut expected_message] {
            if let Some(snapshot) = message
                .get_mut("Snapshot")
                .and_then(serde_json::Value::as_object_mut)
            {
                snapshot.remove("epoch");
            }
        }
        assert_eq!(cold_version, 2);
        assert_eq!(
            cold_message, expected_message,
            "the retained minimum row must cold-rebuild the same graph content; \
             the Lazily epoch is process-local and intentionally restarts"
        );

        record_reliable_sync_editor_exit(&bootstrap.project_root, 100);
        assert!(
            agent_doc_sqlite::state_store::load_state_event_peer_acks_from_db(
                &conn,
                &document_hash,
            )
            .unwrap()
            .is_empty(),
            "the durable OS-exit liveness transition evicts the crashed registration's ack"
        );

        *controller_liveness_plane().lock() =
            agent_doc_reliable_sync_io::plane::ControllerLivenessPlane::new();
    }

    #[test]
    fn controller_owned_outbox_persists_then_flushes_without_plugin_state() {
        let _env = reliable_sync_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let document_hash = "controller-owned-outbox";
        let frame = agent_doc_reliable_sync_io::liveness::encode_liveness_frame(&[
            agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash: document_hash.to_string(),
                pid: 101,
                tag: "controller-owner".to_string(),
            },
        ])
        .unwrap();
        let request = |frame, flush| ControllerRequest {
            command: "reliable_sync_outbox".to_string(),
            file: None,
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: Some("test".to_string()),
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: Some(
                serde_json::to_string(&ControllerReliableSyncOutboxPayload {
                    document_hash: document_hash.to_string(),
                    frame,
                    flush,
                })
                .unwrap(),
            ),
        };

        let enqueued = handle_reliable_sync_outbox(dir.path(), request(Some(frame), false))
            .expect("controller durably enqueues raw frame");
        assert_eq!(enqueued.ack_through, 0);
        let outbox = lazily::SqliteOutbox::open(
            &agent_doc_sqlite::state_store::state_db_path(dir.path()),
            document_hash.to_string(),
        )
        .unwrap();
        assert_eq!(outbox.retained_epochs(), vec![1]);
        drop(outbox);

        let flushed = handle_reliable_sync_outbox(dir.path(), request(None, true))
            .expect("controller reopens and flushes its durable channel");
        assert_eq!(flushed.ack_through, 1);
        let outbox = lazily::SqliteOutbox::open(
            &agent_doc_sqlite::state_store::state_db_path(dir.path()),
            document_hash.to_string(),
        )
        .unwrap();
        assert!(outbox.retained_epochs().is_empty());
        assert_eq!(outbox.acked_through(), 1);
    }

    #[test]
    fn reliable_sync_ack_is_receiver_durable_and_stale_delivery_cannot_regress_it() {
        let _env = reliable_sync_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let document_hash = "docwire-durable-receiver";
        let mut newest = reliable_sync_open_request(document_hash);
        newest.generation = Some(12);
        let first = handle_reliable_sync(dir.path(), newest).expect("newest frame");
        assert_eq!(first.ack_through, 12);

        let mut stale = reliable_sync_open_request(document_hash);
        stale.generation = Some(3);
        let replay = handle_reliable_sync(dir.path(), stale).expect("stale replay");
        assert_eq!(replay.ack_through, 12, "receiver ACK must be monotone");

        let snapshot = agent_doc_sqlite::reliable_sync_inbox::load(
            &agent_doc_sqlite::state_store::state_db_path(dir.path()),
        )
        .expect("durable receiver snapshot");
        let mut recycled = agent_doc_reliable_sync_io::plane::ControllerLivenessPlane::recycle();
        for record in &snapshot.liveness {
            let ops: Vec<agent_doc_reliable_sync_io::liveness::LivenessOp> =
                serde_json::from_str(&record.ops_json).unwrap();
            recycled.restore_liveness(&ops);
        }
        for cursor in &snapshot.cursors {
            recycled.restore_cursor(&cursor.document_hash, cursor.ack_through);
        }

        assert_eq!(recycled.ack_cursor(document_hash), 12);
        assert!(recycled.projection().tracks_document(document_hash));
        assert!(recycled.projection().live_docs().contains(document_hash));
    }

    #[test]
    fn cold_authority_reader_rehydrates_receiver_journal_without_a_lease() {
        let _env = reliable_sync_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(
            &file,
            "---\nagent_doc_session: durable-cold-read\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        let ops = vec![agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
            document_hash: document_hash.clone(),
            pid: 987_654,
            tag: "durable-open".into(),
        }];
        agent_doc_sqlite::reliable_sync_inbox::record_remote_frame(
            &agent_doc_sqlite::state_store::state_db_path(dir.path()),
            &document_hash,
            4,
            Some(&serde_json::to_string(&ops).unwrap()),
        )
        .unwrap();

        assert!(reliable_sync_editor_live_for_file(&file));
        assert_eq!(
            agent_doc_reliable_sync_io::plane_editor_live_for_path(&file.to_string_lossy()),
            Some(true)
        );
    }

    #[test]
    fn crdt_authority_for_file_reads_reliable_sync_projection_without_sidecar() {
        let _env = reliable_sync_env_lock();
        use agent_doc_document_realtime::crdt_authority::CrdtAuthority;
        use agent_doc_reliable_sync_io::liveness::{LivenessOp, encode_liveness_frame};
        // Seed the process-global plane with an Open for a specific path's hash.
        let live_path = "/tmp/agent-doc-authority-test/open-doc.md";
        let live_hash = agent_doc_hash::document_id_for_path(std::path::Path::new(live_path));
        let frame = encode_liveness_frame(&[LivenessOp::Open {
            document_hash: live_hash.clone(),
            pid: 4242,
            tag: "t".into(),
        }])
        .expect("encode liveness frame");
        {
            let mut plane = controller_liveness_plane().lock();
            plane.ingest(&live_hash, 1, &frame).expect("ingest");
        }
        // The reliable-sync plane is authoritative: the open doc is MultiReplica...
        assert_eq!(
            crdt_authority_for_file(live_path),
            CrdtAuthority::MultiReplica
        );
        // ...and a different path the plane does not hold is GitAuthoritative.
        // No lease or live-buffer sidecar is consulted.
        assert_eq!(
            crdt_authority_for_file("/tmp/agent-doc-authority-test/other-doc.md"),
            CrdtAuthority::GitAuthoritative
        );
    }

    #[test]
    fn commit_document_via_controller_is_none_for_headless_document() {
        let _env = reliable_sync_env_lock();
        use agent_doc_document_realtime::crdt_authority::CrdtAuthority;
        // `#cp-commit`: a headless document has no live editor authority to defer
        // to, so the CLI commits locally — `commit_document_via_controller` must
        // short-circuit to `Ok(None)` WITHOUT touching a controller socket or
        // launching one. A fresh temp path is absent from Lazily liveness and
        // deterministically resolves as GitAuthoritative.
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("headless.md");
        std::fs::write(&doc, "# doc\n").unwrap();
        assert_eq!(
            crdt_authority_for_file(&doc.to_string_lossy()),
            CrdtAuthority::GitAuthoritative
        );
        assert_eq!(commit_document_via_controller(&doc, false).unwrap(), None);
        assert_eq!(commit_document_via_controller(&doc, true).unwrap(), None);
    }

    #[test]
    fn commit_document_consumes_the_stream_that_proved_controller_liveness() {
        let _env = reliable_sync_env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let doc = dir.path().join("atomic-controller-stream.md");
        std::fs::write(&doc, "# doc\n").unwrap();
        let canonical = doc.canonicalize().unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        let frame = agent_doc_reliable_sync_io::liveness::encode_liveness_frame(&[
            agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash: document_hash.clone(),
                pid: u64::from(std::process::id()),
                tag: "commit-stream-test".into(),
            },
        ])
        .unwrap();
        controller_liveness_plane()
            .lock()
            .ingest(&document_hash, 1, &frame)
            .unwrap();

        let sock = socket_path(dir.path());
        std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
        let name = sock.clone().to_fs_name::<GenericFilePath>().unwrap();
        let listener = ListenerOptions::new().name(name).create_sync().unwrap();
        let server = std::thread::spawn(move || {
            let stream = listener.accept().unwrap();
            let (reader_half, mut writer_half) = stream.split();
            let mut request = String::new();
            BufReader::new(reader_half).read_line(&mut request).unwrap();
            assert!(
                request.contains("\"command\":\"commit_document\""),
                "{request}"
            );
            writer_half
                .write_all(
                    b"{\"ok\":true,\"data\":{\"did_commit\":true,\"vcs_refresh_signaled\":true}}\n",
                )
                .unwrap();
            writer_half.flush().unwrap();
        });

        let outcome = commit_document_via_controller(&doc, false)
            .unwrap()
            .expect("live editor should delegate to the existing controller");
        assert!(outcome.did_commit);
        assert_eq!(outcome.vcs_refresh_signaled, Some(true));
        server.join().unwrap();
    }

    #[test]
    fn commit_document_payload_round_trips_and_defaults_false() {
        let json = serde_json::to_string(&ControllerCommitDocumentPayload {
            authoritative_compaction: true,
        })
        .unwrap();
        let back: ControllerCommitDocumentPayload = serde_json::from_str(&json).unwrap();
        assert!(back.authoritative_compaction);
        // An absent field defaults to false so an older client's payload stays valid.
        let legacy: ControllerCommitDocumentPayload = serde_json::from_str("{}").unwrap();
        assert!(!legacy.authoritative_compaction);
    }

    #[test]
    fn reliable_sync_status_projects_plane_open_set_without_sidecar_oracle() {
        let _env = reliable_sync_env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let bootstrap = ControllerBootstrap {
            project_root: dir.path().to_path_buf(),
            socket_path: socket_path(dir.path()),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: current_binary_identity().ok(),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        // Fold an Open frame so the shadow plane derives one open doc (pid 100).
        handle_reliable_sync(
            &bootstrap.project_root,
            reliable_sync_open_request("docwire-status"),
        )
        .expect("fold");
        let status = handle_reliable_sync_status(&bootstrap).expect("status ok");
        assert!(
            status
                .plane_open_docs
                .contains(&"docwire-status".to_string())
        );
        // Absent death signal ⇒ pid presumed alive ⇒ the doc is also live.
        assert!(
            status
                .plane_live_docs
                .contains(&"docwire-status".to_string())
        );
        let pids = status
            .per_doc_pids
            .iter()
            .find(|(d, _)| d == "docwire-status")
            .map(|(_, p)| p.clone())
            .unwrap_or_default();
        assert!(pids.contains(&100));
        // `#wsflake2`: scope the "no registration" claim to THIS document. The
        // liveness plane is process-global and shared by every test in this
        // crate, so asserting the whole list is empty really asserts that no
        // other test has registered yet — true when the crate runs alone, and
        // false once CPU contention (a full `cargo test --workspace`) reorders
        // the threads. The property under test is that the open set projects
        // without a registration oracle for this document, which this states
        // directly and which no sibling test can invalidate.
        assert!(
            !status
                .registrations
                .iter()
                .any(
                    |registration| registration.document_hash == "docwire-status"
                        || registration.path.contains("docwire-status")
                ),
            "this document must project without a registration oracle; got {:?}",
            status.registrations
        );
    }
}
