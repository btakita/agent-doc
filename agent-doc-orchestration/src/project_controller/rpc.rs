//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_controller::dispatch::{
    DISPATCH_COALESCED_IN_FLIGHT_MARKER, DISPATCH_STALE_GENERATION_REDIRECT_MARKER,
    DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER, StaleQueuePauseRecovery,
    append_dispatch_proof_payload, dispatch_command_kind_is_operator_reopen,
    dispatch_diagnostic_field, dispatch_error_stale_generation_redirect_target,
    dispatch_should_coalesce_in_flight, pause_reason_is_stale_supervisor_churn_stop,
    spent_preset_id_from_pause_reason, stale_supervisor_pid_from_pause_reason,
};
use agent_doc_controller::status;

pub(crate) fn connect(project_root: &Path) -> Result<interprocess::local_socket::Stream> {
    connect_path(&socket_path(project_root))
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
    let stream = connect_path(path)?;
    stream
        .set_recv_timeout(Some(CONTROLLER_RPC_TIMEOUT))
        .context("failed to set project controller response timeout")?;
    let (reader_half, mut writer_half) = stream.split();
    let mut request = serde_json::to_string(&serde_json::json!({ "command": command }))?;
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
    match reader.read_line(response) {
        Ok(0) => anyhow::bail!("project controller closed connection without a response"),
        Ok(_) => Ok(()),
        Err(err) if is_timeout_error(&err) => anyhow::bail!(
            "timed out after {:.1}s waiting for project controller response",
            CONTROLLER_RPC_TIMEOUT.as_secs_f32()
        ),
        Err(err) => Err(err).context("failed to read project controller response"),
    }
}

pub(crate) fn request_controller<T: DeserializeOwned>(
    project_root: &Path,
    request: ControllerRequest,
) -> Result<T> {
    let stream = connect_or_launch(project_root, LaunchMode::Lazy)?;
    stream
        .set_recv_timeout(Some(CONTROLLER_RPC_TIMEOUT))
        .context("failed to set project controller response timeout")?;
    let (reader_half, mut writer_half) = stream.split();
    let mut raw = serde_json::to_string(&request)?;
    raw.push('\n');
    writer_half.write_all(raw.as_bytes())?;
    writer_half.flush()?;

    let mut reader = BufReader::new(reader_half);
    let mut response = String::new();
    read_controller_response_line(&mut reader, &mut response)?;
    decode_controller_response(project_root, &request, response.trim())
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
                    crate::ops_log::log_op(
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
        return handle_mark_lifecycle(
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
        );
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
        return handle_supervisor_heartbeat(
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
        );
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
        let document_id =
            crate::session_actor::canonical_document_id_in(project_root, &file.to_string_lossy());
        return load_actor_record(project_root, &document_id);
    }

    #[cfg(not(any(test, feature = "test-support")))]
    {
        let response: ActorBindingResponse = request_controller(
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
        )?;
        match response.status {
            ActorBindingStatus::Bound => response.record.map(Some).with_context(|| {
                format!(
                    "project controller command `actor_binding` returned bound status without record for {}",
                    file.display()
                )
            }),
            ActorBindingStatus::NotFound => Ok(None),
        }
    }
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
    crate::ops_log::log_op(
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
                    crate::ops_log::log_op(
                        &request.file,
                        &format!(
                            "dispatch_retry_after_stale_generation file={} prior_generation={} next_generation={}",
                            request.file.display(),
                            request.generation,
                            target
                        ),
                    );
                    return handle_dispatch(&bootstrap, None, dispatch_request(target));
                }
                return Err(err);
            }
            ok => return ok,
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
        match request_controller::<DispatchAuthorization>(project_root, controller_request.clone())
        {
            Err(err) if err.to_string().contains("controller_binary_stale") => {
                crate::ops_log::log_op(
                    &request.file,
                    &format!(
                        "dispatch_retry_after_stale_binary file={}",
                        request.file.display()
                    ),
                );
                request_controller(project_root, controller_request)
            }
            Err(err)
                if dispatch_error_stale_generation_redirect_target(&err.to_string()).is_some() =>
            {
                // `#anw0`: the dispatch lost the supersede race against a newer
                // dispatchable generation. Retry exactly once against the redirect
                // target so racing dispatch self-heals instead of failing closed.
                let target =
                    dispatch_error_stale_generation_redirect_target(&err.to_string()).unwrap();
                crate::ops_log::log_op(
                    &request.file,
                    &format!(
                        "dispatch_retry_after_stale_generation file={} next_generation={}",
                        request.file.display(),
                        target
                    ),
                );
                let mut redirected = controller_request.clone();
                redirected.generation = Some(target);
                request_controller(project_root, redirected)
            }
            other => other,
        }
    }
}

pub fn session_operator_status(project_root: &Path, file: &Path) -> Result<SessionOperatorStatus> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let document_id =
            crate::session_actor::canonical_document_id_in(project_root, &file.to_string_lossy());
        let mut conn = open_state_db(project_root)?;
        migrate_legacy_actor_projection(project_root, &mut conn)?;
        return load_session_operator_status_from_db(&conn, &document_id);
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
        return handle_inspect_actor(
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
        );
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
        return handle_queue_control(
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
        );
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
        return handle_admin_control(
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
        );
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
        return handle_admin_control(
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
        );
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
        return handle_projection_repair(
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
        );
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
        return handle_operator_command(
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
        );
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

pub(crate) fn controller_status_matches_current_binary(status: &ControllerStatus) -> Result<bool> {
    Ok(status.controller_binary.as_ref() == Some(&current_binary_identity()?))
}

/// `#ctlstalebin` (#stuckhandoff2 follow-up) — is the SERVING controller's own
/// recorded binary stale relative to the freshly-installed agent-doc binary?
///
/// `connect_or_launch` already hands a *cross-process* caller off to a fresh
/// controller when the active controller's binary no longer matches (the common
/// `cargo install` recycle path). This predicate is the dispatch-admission backstop
/// for the residual gap: a dispatch that still reaches a stale controller's
/// `handle_dispatch` — an in-process co-hosted call, or a narrow handoff race —
/// must be refused so the stale process cannot keep driving session writes (the
/// "already-running supervisor uses the OLD binary until restarted" churn class).
///
/// True only when both identities resolve and differ. Any stat/resolution error is
/// treated as "not stale" so a transiently-unreadable binary path can never block a
/// live dispatch (fail-open — staleness is a recycle hint, never a hard stop).
pub(crate) fn controller_binary_is_stale(bootstrap: &ControllerBootstrap) -> bool {
    process_binary_is_stale(bootstrap.controller_binary.as_ref())
}

/// `#ctlrecycle` — process-type-agnostic generalization of [`controller_binary_is_stale`].
/// Compares a long-lived process's RECORDED launch identity against the
/// freshly-installed binary (`current_binary_identity` stats the install *path*, so
/// it reflects a `cargo install` even though this process still runs the old mapped
/// inode). Shared by the controller serve loop (R1) and the `start` supervisor (R3).
/// Fail-open: a missing recorded identity or any stat error reads as "not stale".
pub(crate) fn process_binary_is_stale(recorded: Option<&ControllerBinaryIdentity>) -> bool {
    let Some(recorded) = recorded else {
        return false;
    };
    match current_binary_identity() {
        Ok(current) => recorded != &current,
        Err(_) => false,
    }
}

/// `#fccsupwarn` — read-only operator WARN message when the LIVE controller/supervisor
/// hosting this document is serving a stale agent-doc binary (a fresh `cargo install`
/// hasn't been picked up). Pure over a [`ControllerStatus`] so it is unit-testable and
/// has no IO; the caller (preflight / session-check) loads the status and surfaces the
/// message. Returns `None` for an inactive controller (nothing is hosting), a status
/// with no recorded launch identity, or a fresh binary — so the warning only fires for
/// the exact "already-running supervisor uses the OLD binary until restarted" churn
/// class that silently produces `#fcc0` / `#ipcdrift` File Cache Conflict dialogs.
pub(crate) fn supervisor_stale_warning_message(status: &ControllerStatus) -> Option<String> {
    if !status.active {
        return None;
    }
    if !process_binary_is_stale(status.controller_binary.as_ref()) {
        return None;
    }
    let pid = status
        .pid
        .map(|pid| format!(" (pid {pid})"))
        .unwrap_or_default();
    let launched = status
        .controller_binary
        .as_ref()
        .map(|id| format!(" launched as {}", id.version))
        .unwrap_or_default();
    Some(format!(
        "the live session controller/supervisor{pid}{launched} is running a STALE agent-doc binary \
         (a newer build is installed) — restart it so the latest fixes take effect: \
         `agent-doc admin recycle` (recycles at the next idle boundary) or \
         `agent-doc session restart-supervisor <FILE>`. A stale supervisor silently keeps \
         producing File Cache Conflict / IPC-drift dialogs (#fcc0/#ipcdrift)."
    ))
}

/// `#fccsupwarn3` — user-facing warning for a stale route-owned host supervisor.
/// Point at the routine non-destructive refresh: idle-boundary recycle or normal
/// file-scoped restart. (`#recycledeadlock`: the prior "avoid force/discard recovery
/// for stale-binary refresh" sentence was dropped — the recycle now self-heals the
/// open-cycle deadlock that used to make force/discard the only escape, so a blanket
/// warning against it was both unnecessary and misleading.)
pub(crate) fn host_supervisor_stale_warning_message(supervisor_pid: u32) -> String {
    format!(
        "the route-owned host supervisor (pid {supervisor_pid}) serving this document is mapping \
         a STALE agent-doc binary while a newer build is installed, so it can keep producing File \
         Cache Conflict / IPC-drift dialogs (#fcc0/#ipcdrift). Refresh it without discarding the \
         live turn: `agent-doc admin recycle` (recycles at the next idle boundary) or \
         `agent-doc session restart-supervisor <FILE>` (refuses busy panes)."
    )
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
    let project_root = agent_doc_fs::find_project_root(file)?;
    let record = authoritative_actor_binding(&project_root, file)
        .ok()
        .flatten()?;
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
    let installed_inode = inode_of_path(&current_binary_identity().ok()?.path)?;
    let running_inode = running_exe_inode_for_pid(supervisor_pid);
    if !agent_doc_supervisor::config::host_supervisor_is_stale(running_inode, installed_inode) {
        return None;
    }
    Some(host_supervisor_stale_warning_message(supervisor_pid))
}

/// `#fccsupwarn`/`#fccsupwarn2` — IO wrapper: resolve the live processes hosting `file`
/// and return a stale-binary warning if EITHER the lazy controller OR the route-owned
/// host supervisor is serving a stale build. The controller check (`#fccsupwarn`) covers
/// the in-process / handoff path; the host-supervisor check (`#fccsupwarn2`) covers the
/// separate long-lived `agent-doc start --route-owned` process that actually writes the
/// document and is the common silent-stale offender. Fail-open — a missing project root,
/// an unreachable controller, a missing lease, or any stat error yields `None` so the
/// read-only check can never block a cycle.
pub(crate) fn stale_supervisor_warning_for_doc(file: &Path) -> Option<String> {
    let project_root = agent_doc_fs::find_project_root(file)?;
    if let Ok(status) = status(&project_root)
        && let Some(message) = supervisor_stale_warning_message(&status)
    {
        return Some(message);
    }
    host_supervisor_stale_warning_for_doc(file)
}

/// `#ctlrecycle` — idle grace before a stale/recycle-requested process actually
/// recycles. A process must observe "wants-recycle AND idle" continuously for this
/// long so a brief lull between queue items never triggers a recycle. Override with
/// `AGENT_DOC_RECYCLE_IDLE_GRACE_SECS`.
pub(crate) fn recycle_idle_grace() -> Duration {
    let secs = std::env::var(RECYCLE_IDLE_GRACE_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_RECYCLE_IDLE_GRACE_SECS);
    Duration::from_secs(secs)
}

/// `#ctlrecycle` R3 / `#suprecyclequeue` — is supervisor auto-recycle enabled for the
/// supervisor hosting `doc`? Reads the env override, the document's frontmatter, and
/// its project config, then resolves via `agent_doc_supervisor::config`. Default ON
/// (`#supselfheal`); opt out with a falsey env/frontmatter/project knob.
pub(crate) fn supervisor_auto_recycle_enabled(doc: &std::path::Path) -> bool {
    let env = std::env::var(SUPERVISOR_AUTO_RECYCLE_ENV).ok();
    let frontmatter = std::fs::read_to_string(doc).ok().and_then(|content| {
        agent_doc_frontmatter::frontmatter::parse(&content)
            .ok()
            .and_then(|(fm, _)| fm.supervisor_auto_recycle)
    });
    let project =
        agent_doc_project_config_io::load_project_for_doc(doc).agent_doc_supervisor_auto_recycle;
    agent_doc_supervisor::config::resolve_supervisor_auto_recycle(
        env.as_deref(),
        frontmatter,
        project,
    )
}

/// `#agentreloadrestart` — is agent-change-restart enabled for the supervisor
/// hosting `doc`? Reads env `AGENT_DOC_AGENT_CHANGE_RESTART`, the document's
/// frontmatter, and its project config. Default ON.
pub(crate) fn agent_change_restart_enabled(doc: &std::path::Path) -> bool {
    let env = std::env::var(AGENT_CHANGE_RESTART_ENV).ok();
    let frontmatter = std::fs::read_to_string(doc).ok().and_then(|content| {
        agent_doc_frontmatter::frontmatter::parse(&content)
            .ok()
            .and_then(|(fm, _)| fm.agent_change_restart)
    });
    let project =
        agent_doc_project_config_io::load_project_for_doc(doc).agent_doc_agent_change_restart;
    agent_doc_supervisor::config::resolve_agent_change_restart(env.as_deref(), frontmatter, project)
}

/// `#supautoinstall` — is supervisor auto-install enabled for the supervisor hosting `doc`?
/// Reads the env override, the document's frontmatter, and its project config, then resolves
/// via `agent_doc_supervisor::config`. Default ON; opt out with a falsey
/// env/frontmatter/project knob. (Never fires for a non-dogfooding document regardless.)
pub(crate) fn supervisor_auto_install_enabled(doc: &std::path::Path) -> bool {
    let env = std::env::var(SUPERVISOR_AUTO_INSTALL_ENV).ok();
    let frontmatter = std::fs::read_to_string(doc).ok().and_then(|content| {
        agent_doc_frontmatter::frontmatter::parse(&content)
            .ok()
            .and_then(|(fm, _)| fm.supervisor_auto_install)
    });
    let project =
        agent_doc_project_config_io::load_project_for_doc(doc).agent_doc_supervisor_auto_install;
    agent_doc_supervisor::config::resolve_supervisor_auto_install(
        env.as_deref(),
        frontmatter,
        project,
    )
}

/// `#supautoinstall` — resolve the agent-doc crate source root for a DOGFOODING session
/// (an agent-doc session editing agent-doc's own source). A superproject may contain
/// `src/agent-doc` while also hosting unrelated project documents; those documents must not
/// inherit dogfood build/install policy just because the crate is nearby.
pub(crate) fn dogfood_agent_doc_crate_root(file: &Path) -> Option<PathBuf> {
    let file = file.canonicalize().ok()?;
    let project_root = agent_doc_fs::find_project_root(&file)?;
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

/// `#supautoinstall` — newest mtime (unix secs) among the crate's build inputs: a bounded
/// recursive walk of `.rs`/`.toml`/`.md` files under `crate_root`, skipping build/VCS/cache
/// dirs (`target`, `.git`, `.agent-doc`, `node_modules`, `.tsift`, `build`, `dist`). Used to
/// detect "a finalize committed a source edit but the binary has not been rebuilt yet".
/// Fail-open `None` when nothing is readable.
pub(crate) fn newest_crate_source_mtime_secs(crate_root: &Path) -> Option<u64> {
    fn walk(dir: &Path, newest: &mut u64) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if matches!(
                    name.as_ref(),
                    "target" | ".git" | ".agent-doc" | "node_modules" | ".tsift" | "build" | "dist"
                ) {
                    continue;
                }
                walk(&entry.path(), newest);
            } else if file_type.is_file() {
                let path = entry.path();
                let ext_ok = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| matches!(e, "rs" | "toml" | "md"))
                    .unwrap_or(false);
                if !ext_ok {
                    continue;
                }
                if let Ok(secs) = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .map_err(anyhow::Error::from)
                    .and_then(|m| Ok(m.duration_since(UNIX_EPOCH)?.as_secs()))
                {
                    *newest = (*newest).max(secs);
                }
            }
        }
    }
    let mut newest = 0u64;
    walk(crate_root, &mut newest);
    (newest > 0).then_some(newest)
}

/// `#supautoinstall` — run the dogfood local install for agent-doc's own source
/// from `crate_root`. Runs IN THE SUPERVISOR at an idle boundary (never the
/// finalize client mid-cycle), which is what root-fixes the mid-session-install
/// drift. After it succeeds the installed binary is newer than the running
/// supervisor process, so the existing `process_binary_is_stale` recycle path
/// hot-reloads onto it. Returns `Err` naming the failed step.
pub(crate) fn run_supervisor_auto_install(crate_root: &Path) -> Result<()> {
    run_supervisor_auto_install_with_retry(
        crate_root,
        AUTO_INSTALL_MAX_ATTEMPTS,
        Duration::from_secs(AUTO_INSTALL_RETRY_BACKOFF_SECS),
    )
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

/// Run the auto-install sequence ONCE through `make install`. The Makefile owns
/// the local-dev profile, incremental target dir, linker selection, and cdylib
/// install flags. The target is idempotent, so retrying it is safe.
fn run_auto_install_steps_once(crate_root: &Path) -> Result<()> {
    let steps: [(&str, &[&str]); 1] = [("make", &["install"])];
    for (program, args) in steps {
        let status = std::process::Command::new(program)
            .args(args)
            .current_dir(crate_root)
            .status()
            .with_context(|| format!("failed to spawn `{program} {}`", args.join(" ")))?;
        if !status.success() {
            anyhow::bail!(
                "auto-install step `{program} {}` failed with status {status}",
                args.join(" ")
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
    crate::ops_log::log_op(file, reason);
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
    let stale_pid_dead_after_reboot =
        stale_pid_dead && queue_pause_predates_current_boot(control.updated_at);
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

fn queue_pause_predates_current_boot(updated_at: u64) -> bool {
    system_boot_timestamp_secs().is_some_and(|boot| updated_at < boot)
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

    let outcome = crate::write::consume_queue_prompt_force_disk(file).with_context(|| {
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

    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let Ok(pid) = name.parse::<u32>() else {
                continue;
            };
            if Some(pid) == authoritative_pid || pid == std::process::id() {
                continue;
            }
            if is_same_project_controller_pid(project_root, pid) {
                pids.insert(pid);
            }
        }
    }

    pids.retain(|pid| {
        Some(*pid) != authoritative_pid && *pid != std::process::id() && process_is_alive(*pid)
    });
    pids.into_iter().collect()
}

pub(crate) fn is_same_project_controller_pid(project_root: &Path, pid: u32) -> bool {
    let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let args: Vec<String> = cmdline
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8_lossy(arg).to_string())
        .collect();
    args_match_same_project_controller(&args, project_root)
}

pub(crate) fn args_match_same_project_controller(args: &[String], project_root: &Path) -> bool {
    let Some(raw_root) =
        agent_doc_controller::command_line::controller_serve_project_root_from_args(args)
    else {
        return false;
    };
    canonical_path_for_compare(&raw_root) == canonical_path_for_compare(project_root)
}

pub(crate) fn canonical_path_for_compare(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn process_is_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

pub(crate) fn system_boot_timestamp_secs() -> Option<u64> {
    let uptime_secs = std::fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .parse::<f64>()
        .ok()?;
    if !uptime_secs.is_finite() || uptime_secs.is_sign_negative() {
        return None;
    }
    Some(timestamp_secs().saturating_sub(uptime_secs.floor() as u64))
}

pub(crate) fn reap_verified_controller_pid(project_root: &Path, pid: u32, generation: u64) {
    if pid == std::process::id() || !is_same_project_controller_pid(project_root, pid) {
        return;
    }
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(750) {
        if !process_is_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    if is_same_project_controller_pid(project_root, pid) {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(pid.to_string())
            .status();
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
    let _ = request(project_root, "shutdown");
    let start = Instant::now();
    while start.elapsed() < CONNECT_WAIT {
        if connect(project_root).is_err() {
            return;
        }
        std::thread::sleep(CONNECT_POLL);
    }
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
    // #stuckhandoff2: process-scan reap orphaned `Preparing` zombies in this root so
    // `admin recycle` clears the wedged-preparing class immediately instead of
    // relying on M1's later self-watchdog tick or the next gc/connect. The shared
    // threshold spares a healthy young handoff (including one a recycle just
    // launched). Runs regardless of the recycle RPC outcome.
    let _ = reap_orphaned_preparing_controllers_for_caller(
        project_root,
        stale_preparing_controller_threshold(),
        false,
        "recycle",
    );
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
    let self_pid = std::process::id();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Ok((0, 0));
    };
    let mut roots: BTreeSet<PathBuf> = BTreeSet::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if pid == self_pid {
            continue;
        }
        if let Some(root) = controller_serve_project_root(pid) {
            roots.insert(canonical_path_for_compare(&root));
        }
    }
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

/// M4 (#stuckhandoff2) — client handoff drop-guard. The two-phase handoff is
/// driven by the invoking client: it launches a replacement controller in
/// `Preparing`, then promotes it to `Stable` via `promote_handoff`. If the client
/// is interrupted or an RPC fails between those two steps (a `?` early-return /
/// panic in `handoff_stale_controller`), the half-launched replacement is left
/// wedged in `Preparing` forever — the exact orphan M1's self-watchdog and the
/// M3/M5 reapers exist to clean up *after the fact*. This guard prevents the wedge
/// at the source: on drop without a completed promotion it tells that replacement
/// (still listening on the temp socket) to `shutdown`, so an aborted handoff never
/// leaves a `Preparing` controller behind. The success path calls
/// [`HandoffDropGuard::complete`] after the socket rename + reap so a promoted,
/// now-authoritative controller is never shut down.
pub(crate) struct HandoffDropGuard<'a> {
    project_root: &'a Path,
    temp_sock: &'a Path,
    completed: bool,
}

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

impl Drop for HandoffDropGuard<'_> {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        // Aborted before promotion: best-effort shutdown of the half-launched
        // replacement so it never lingers in `Preparing`. If the temp socket is
        // already gone (the abort happened before the replacement came up) the
        // request fails harmlessly and M1's self-watchdog remains the backstop.
        let _ = request_path(self.temp_sock, "shutdown");
        crate::ops_log::log_op(
            self.project_root,
            &format!(
                "handoff_drop_guard_aborted_handoff_shutdown temp_sock={}",
                self.temp_sock.display()
            ),
        );
    }
}

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
    let _temp_stream = wait_for_controller_path(&temp_sock)?;
    let replacement_status: ControllerStatus = serde_json::from_str(
        &request_path(&temp_sock, "status").context("failed to read replacement status")?,
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
    connect_or_launch_with_lock_wait(project_root, launch_mode, LAUNCH_LOCK_WAIT)
}

fn connect_or_launch_with_lock_wait(
    project_root: &Path,
    launch_mode: LaunchMode,
    launch_lock_wait: Duration,
) -> Result<interprocess::local_socket::Stream> {
    if let Ok(active_status) = status(project_root)
        && active_status.active
        && controller_status_matches_current_binary(&active_status).unwrap_or(false)
    {
        reap_stale_duplicate_controllers(
            project_root,
            active_status.pid,
            active_status.controller_generation.unwrap_or(1),
        );
        return connect(project_root);
    }

    // Block (bounded) on launch-lock contention instead of failing fast: another
    // launcher (concurrent start, sibling document, or a just-execve'd self-recycle
    // racing its predecessor) is mid-launch on the shared project-root lock, and the
    // double-checked `status` + `connect` below adopts whatever it publishes
    // (#suprecyclelock). Only a genuinely wedged holder returns an error — and even
    // then, adopt a live matching controller it may have published before wedging.
    let launch_lock = match LaunchLock::acquire_blocking(project_root, launch_lock_wait) {
        Ok(lock) => lock,
        Err(err) => {
            if let Ok(active_status) = status(project_root)
                && active_status.active
                && controller_status_matches_current_binary(&active_status).unwrap_or(false)
            {
                log_launch_lock_waiter_adopted(project_root, &active_status, "timeout");
                reap_stale_duplicate_controllers(
                    project_root,
                    active_status.pid,
                    active_status.controller_generation.unwrap_or(1),
                );
                return connect(project_root);
            }
            return Err(err);
        }
    };
    let waited_on_launch_lock = launch_lock.waited();
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
        && controller_status_matches_current_binary(&active_status).unwrap_or(false)
    {
        if waited_on_launch_lock {
            log_launch_lock_waiter_adopted(project_root, &active_status, "acquired");
        }
        reap_stale_duplicate_controllers(
            project_root,
            active_status.pid,
            active_status.controller_generation.unwrap_or(1),
        );
        return connect(project_root);
    }
    if connect(project_root).is_ok() {
        if let Ok(old_status) = status(project_root)
            && old_status.active
        {
            return handoff_stale_controller(project_root, launch_mode, old_status);
        }
        shutdown_stale_controller(project_root);
    }

    launch_detached(project_root, launch_mode)?;
    wait_for_controller(project_root)
}

fn log_launch_lock_waiter_adopted(
    project_root: &Path,
    active_status: &ControllerStatus,
    phase: &str,
) {
    crate::ops_log::log_op(
        project_root,
        &format!(
            "controller_launch_lock_waiter_adopted_published_controller phase={} pid={} generation={}",
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
    let exe = current_agent_doc_binary()?;
    let mut command = Command::new(exe);
    command
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
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to launch project controller")?;
    Ok(())
}

pub(crate) fn wait_for_controller(
    project_root: &Path,
) -> Result<interprocess::local_socket::Stream> {
    wait_for_controller_path(&socket_path(project_root))
}

pub(crate) fn wait_for_controller_path(path: &Path) -> Result<interprocess::local_socket::Stream> {
    let start = Instant::now();
    loop {
        if let Ok(stream) = connect_path(path) {
            return Ok(stream);
        }
        if start.elapsed() >= CONNECT_WAIT {
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

pub(crate) fn serve_with_options(
    project_root: &Path,
    launch_mode: LaunchMode,
    listen_socket: Option<PathBuf>,
    controller_generation: Option<u64>,
    previous_controller_pid: Option<u32>,
    handoff_state: ControllerHandoffState,
) -> Result<()> {
    let public_sock = socket_path(project_root);
    let sock = listen_socket.unwrap_or_else(|| public_sock.clone());
    // M1b (#stuckhandoff2 reopen): a controller launched on a non-public socket is
    // a handoff *replacement* (`controller-handoff-*` temp socket from
    // `handoff_stale_controller`). It becomes authoritative only when its client
    // renames that temp socket onto the public path; until then it is a candidate
    // for the structural stranded-replacement watchdog below. The initial
    // controller serves directly on the public socket, so this stays `None`.
    let handoff_temp_socket: Option<PathBuf> = (sock != public_sock).then(|| sock.clone());
    if sock.exists() {
        let _ = std::fs::remove_file(&sock);
    }
    if let Some(parent) = sock.parent() {
        std::fs::create_dir_all(parent)?;
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
    let name = sock.clone().to_fs_name::<GenericFilePath>()?;
    let listener = ListenerOptions::new()
        .name(name)
        .create_sync()
        .with_context(|| format!("failed to listen on {}", sock.display()))?;
    listener
        .set_nonblocking(ListenerNonblockingMode::Accept)
        .context("failed to set project controller listener nonblocking")?;

    let runtime = Arc::new(ControllerRuntime::new(bootstrap)?);
    let should_stop = Arc::new(AtomicBool::new(false));
    let watchdog_threshold = stale_preparing_controller_threshold();
    let controller_launched_at = Instant::now();
    let recycle_grace = recycle_idle_grace();
    let mut recycle_stale_since: Option<Instant> = None;
    while !should_stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok(stream) => {
                let runtime = Arc::clone(&runtime);
                let should_stop = Arc::clone(&should_stop);
                let sock = sock.clone();
                std::thread::spawn(move || {
                    if let Err(err) = serve_client(stream, &runtime, &should_stop, &sock) {
                        eprintln!("[controller] client error: {err}");
                    }
                });
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
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
                if controller_self_watchdog_should_suicide(&runtime, watchdog_threshold)
                    || controller_handoff_replacement_is_stranded(
                        handoff_temp_socket.as_deref(),
                        controller_launched_at.elapsed(),
                        watchdog_threshold,
                    )
                {
                    controller_self_watchdog_suicide(&runtime, watchdog_threshold);
                    should_stop.store(true, Ordering::SeqCst);
                    break;
                }
                // R1/R2 (#ctlrecycle): recycle onto a freshly-installed binary (R1) or
                // on an operator `recycle` request (R2) once no dispatch is in flight,
                // debounced so a brief lull between queue items never triggers it. The
                // idle DB probe only runs when a recycle is actually wanted (rare), so
                // the common hot path stays an atomic load plus one binary `stat`.
                let wants_recycle_and_idle =
                    controller_wants_recycle(&runtime) && controller_recycle_idle(&runtime);
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
                std::thread::sleep(CONNECT_POLL);
            }
            Err(err) => return Err(err).context("failed to accept project controller client"),
        }
    }
    let _ = std::fs::remove_file(&sock);
    Ok(())
}

/// M1 (#stuckhandoff2) — pure predicate: should the serving controller self-terminate?
/// Reads the controller's own live bootstrap snapshot and applies the same staleness
/// rule the external reaper uses. Side-effect free for deterministic unit tests; a
/// poisoned bootstrap lock is treated as "do not suicide" (the next external reaper
/// pass still covers it).
pub(crate) fn controller_self_watchdog_should_suicide(
    runtime: &ControllerRuntime,
    threshold: Duration,
) -> bool {
    let Ok(bootstrap) = runtime.bootstrap_snapshot() else {
        return false;
    };
    preparing_controller_is_stale(
        bootstrap.handoff_state,
        bootstrap.handoff_started_at,
        timestamp_secs(),
        threshold,
    )
}

/// M1b (#stuckhandoff2 reopen) — structural self-watchdog for a promoted-but-stranded
/// handoff *replacement*. The in-memory predicate above only sees `Preparing`/`Promoted`,
/// but `promote_handoff` flips a replacement straight to `Stable` (`handoff_started_at`
/// cleared) the instant the client asks — so a client that dies *after* `promote_handoff`
/// but *before* `std::fs::rename(temp_sock → public_sock)` (`handoff_stale_controller`)
/// leaves a `Stable`-in-memory controller stranded on its temp socket, invisible to the
/// predicate. That was the dominant orphan the slow gc/M5 cmdline sweep cleaned up at
/// 7–21 minutes while M1 logged nothing.
///
/// Detect it structurally, independent of in-memory `handoff_state`: a replacement was
/// launched on a `controller-handoff-*` temp socket (`handoff_temp_socket`); a *completed*
/// handoff removes that path via the promote rename, so a temp socket that still exists
/// past the threshold proves the promotion never finished. The `launched_elapsed >
/// threshold` guard spares a healthy young handoff (which completes the rename in well
/// under a second).
pub(crate) fn controller_handoff_replacement_is_stranded(
    handoff_temp_socket: Option<&Path>,
    launched_elapsed: Duration,
    threshold: Duration,
) -> bool {
    let Some(temp) = handoff_temp_socket else {
        return false;
    };
    launched_elapsed > threshold && temp.exists()
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
    if let Ok(mut state) = runtime.bootstrap.lock() {
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
    crate::ops_log::log_op(
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
        Ok(bootstrap) => process_binary_is_stale(bootstrap.controller_binary.as_ref()),
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
    crate::ops_log::log_op(
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
    loop {
        match reader.read_line(&mut line) {
            Ok(0) => return Ok(()),
            Ok(_) => {
                let mut request_should_stop = false;
                let response = handle_request_locked(&line, runtime, &mut request_should_stop)?;
                writer_half.write_all(response.as_bytes())?;
                writer_half.write_all(b"\n")?;
                writer_half.flush()?;
                line.clear();
                if request_should_stop {
                    should_stop.store(true, Ordering::SeqCst);
                    let _ = std::fs::remove_file(sock);
                    if let Ok(bootstrap) = runtime.bootstrap.lock()
                        && bootstrap.socket_path != sock
                    {
                        let _ = std::fs::remove_file(&bootstrap.socket_path);
                    }
                    return Ok(());
                }
            }
            Err(err) if is_timeout_error(&err) => {
                eprintln!(
                    "[controller] closing idle client after {:.1}s without a complete request",
                    CONTROLLER_IDLE_CLIENT_TIMEOUT.as_secs_f32()
                );
                return Ok(());
            }
            Err(err) => return Err(err).context("failed to read project controller request"),
        }
    }
}

pub(crate) fn is_timeout_error(err: &std::io::Error) -> bool {
    matches!(err.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock)
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
        &Arc::new(ControllerRuntime::new(bootstrap.clone())?),
        should_stop,
    )
}

pub(crate) fn handle_request_locked(
    line: &str,
    runtime: &Arc<ControllerRuntime>,
    should_stop: &mut bool,
) -> Result<String> {
    let request: ControllerRequest = serde_json::from_str(line.trim())?;
    let bootstrap_snapshot = runtime.bootstrap_snapshot()?;
    match request.command.as_str() {
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
        "prepare_handoff" => {
            let mut state = runtime
                .bootstrap
                .lock()
                .map_err(|_| anyhow::anyhow!("controller bootstrap lock poisoned"))?;
            state.handoff_state = ControllerHandoffState::Preparing;
            state.handoff_started_at = Some(timestamp_secs());
            write_bootstrap_state(&state)?;
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "promote_handoff" => {
            let mut state = runtime
                .bootstrap
                .lock()
                .map_err(|_| anyhow::anyhow!("controller bootstrap lock poisoned"))?;
            state.socket_path = socket_path(&state.project_root);
            state.handoff_state = ControllerHandoffState::Stable;
            state.handoff_started_at = None;
            write_bootstrap_state(&state)?;
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "retire_after_handoff" => {
            {
                let mut state = runtime
                    .bootstrap
                    .lock()
                    .map_err(|_| anyhow::anyhow!("controller bootstrap lock poisoned"))?;
                state.handoff_state = ControllerHandoffState::Retiring;
                write_bootstrap_state(&state)?;
            }
            *should_stop = true;
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "shutdown" => {
            *should_stop = true;
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "recycle" => {
            // R2 (#ctlrecycle): mark this controller to recycle at the next idle
            // boundary. Unlike `shutdown`, it does NOT stop immediately — the
            // serve-loop idle poll honors it only once no dispatch is in flight, so
            // an explicit recycle never interrupts an in-flight turn.
            runtime.request_recycle();
            crate::ops_log::log_op(
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
            crate::ops_log::log_op(
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
        "dispatch" => controller_envelope(handle_dispatch(
            &bootstrap_snapshot,
            Some(runtime.as_ref()),
            request,
        )),
        "session_status" => {
            controller_envelope(handle_session_status(&bootstrap_snapshot, request))
        }
        "inspect_actor" => controller_envelope(handle_inspect_actor(&bootstrap_snapshot, request)),
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
        "admin_operation" => {
            controller_envelope(handle_admin_operation(&bootstrap_snapshot, request))
        }
        other => Ok(serde_json::to_string(&serde_json::json!({
                "ok": false,
                "error": format!("unknown controller command: {other}")
        }))?),
    }
}

pub(crate) fn controller_envelope<T: Serialize>(result: Result<T>) -> Result<String> {
    match result {
        Ok(data) => Ok(serde_json::to_string(&serde_json::json!({
            "ok": true,
            "data": data
        }))?),
        Err(err) => Ok(serde_json::to_string(&serde_json::json!({
            "ok": false,
            "error": err.to_string()
        }))?),
    }
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

fn dispatch_blocked_proof_fields(
    project_root: &Path,
    file: &Path,
    stage: &str,
    reason: &str,
    diagnostic_payload: &str,
) -> String {
    let mut fields = Vec::new();
    fields.push(dispatch_blocked_user_facing_outcome_fields(stage, reason));
    let file_path = if file.is_absolute() {
        file.to_path_buf()
    } else {
        project_root.join(file)
    };
    if let Ok(content) = std::fs::read_to_string(&file_path)
        && let Ok(Some(head)) = agent_doc_queue::queue_heads::active_queue_head_text(&content)
    {
        fields.push(format!("blocked_head_bytes={}", head.len()));
        fields.push(format!(
            "blocked_head_sha256={}",
            agent_doc_hash::content_hash(&head)
        ));
    }
    if let Some(harness) = dispatch_diagnostic_field(diagnostic_payload, "harness") {
        let trigger = crate::harness::HarnessConfig::from_agent_name(harness)
            .trigger_command(&file.to_string_lossy());
        fields.push(format!("trigger_bytes={}", trigger.len()));
        fields.push(format!(
            "trigger_sha256={}",
            agent_doc_hash::content_hash(&trigger)
        ));
    }
    fields.join(" ")
}

fn dispatch_blocked_user_facing_outcome_fields(stage: &str, reason: &str) -> String {
    use crate::flow::outcome::{UserFacingOutcome, UserFacingOutcomeKind as Kind};

    let lower = reason.to_ascii_lowercase();
    let outcome = if stage == "actor_busy_draining" {
        UserFacingOutcome::new(Kind::QueuedBehindOwner)
    } else if stage == "queue_paused" && pause_reason_is_stale_supervisor_churn_stop(reason) {
        UserFacingOutcome::new(Kind::RecoveredAndRetried)
    } else if lower.contains("file cache conflict")
        || lower.contains("component conflict")
        || lower.contains("typed_component_drift")
    {
        UserFacingOutcome::new(Kind::RealComponentConflict)
    } else if lower.contains("zero drainable")
        || lower.contains("no drainable")
        || lower.contains("undrainable")
    {
        UserFacingOutcome::new(Kind::NoDrainableWork)
    } else if lower.contains("operator-verify")
        || lower.contains("operator proof")
        || lower.contains("manual review")
    {
        UserFacingOutcome::new(Kind::DeferredForOperatorProof)
    } else {
        UserFacingOutcome::with_unblocker(
            Kind::BlockedWithExactUnblocker,
            "resume_or_clear_queue_control",
        )
    };

    outcome
        .expect("static dispatch blocked user-facing outcome fields are valid")
        .log_fields()
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
    let document_id = crate::session_actor::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let harness =
        crate::session_actor::detect_document_harness_in(&bootstrap.project_root, &document_id);
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
    let _ = project_sessions_projection_for_actor(&bootstrap.project_root, &record.document_id);
    crate::ops_log::log_op(
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
        harness: crate::session_actor::detect_document_harness_in(
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
    let _ =
        project_sessions_projection_for_actor(&bootstrap.project_root, &replacement.document_id);
    crate::ops_log::log_op(
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
    let document_id = crate::session_actor::canonical_document_id_in(
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
    crate::ops_log::log_op(
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
    let document_id = crate::session_actor::canonical_document_id_in(
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
    // #qflood: a transition to Ready means the turn finished, so any dispatch in
    // flight for this document is now consumed — release the in-flight marker so the
    // open-dispatch set stays accurate for the next busy episode's coalescing and for
    // restart recovery. Stall-safety does not depend on this (a turn always starts
    // from a Ready dispatch, which never coalesces); it keeps the table honest.
    if matches!(state, agent_doc_sqlite::state_store::ActorState::Ready) {
        match open_state_db(&bootstrap.project_root)
            .and_then(|conn| state_store::mark_open_dispatches_consumed(&conn, &document_id))
        {
            Ok(released) if released > 0 => crate::ops_log::log_op(
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
    crate::ops_log::log_op(
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
    let document_id = crate::session_actor::canonical_document_id_in(
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
    crate::ops_log::log_op(
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
    let document_id = crate::session_actor::canonical_document_id_in(
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
    let document_id = crate::session_actor::canonical_document_id_in(
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
    if controller_binary_is_stale(bootstrap) {
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
        crate::ops_log::log_op(
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
        crate::ops_log::log_op(
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
        let proof_fields = dispatch_blocked_proof_fields(
            &bootstrap.project_root,
            &file,
            stage,
            reason,
            &diagnostic_payload,
        );
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
            crate::ops_log::log_op(
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
            crate::ops_log::log_op(
                &file,
                &format!(
                    "dispatch_queue_paused_stale_supervisor file={} stale_pid={} marker={} {} {}",
                    file.display(),
                    stale_pid,
                    DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER,
                    recovery.outcome.log_fields(),
                    crate::flow::outcome::UserFacingOutcome::new(
                        crate::flow::outcome::UserFacingOutcomeKind::RecoveredAndRetried,
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
            crate::ops_log::log_op(
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

    // #qflood: coalesce a redundant in-flight re-dispatch. While the actor is
    // actively running a turn (Busy) and a dispatch for this cycle is already in
    // flight (accepted, not yet consumed), an AUTO re-fire (route auto-start on a
    // file-change save, idle-queue continuation, `/loop` tick) must not pile another
    // trigger into the busy pane. The first dispatch of a turn comes from a Ready
    // actor and is never coalesced, so this can never stall the queue — it only
    // suppresses the redundant re-fire; the next dispatch once the actor is Ready
    // submits cleanly. Backpressure, never a queue stop.
    if matches!(
        record.state,
        agent_doc_sqlite::state_store::ActorState::Busy
    ) {
        let conn = open_state_db(&bootstrap.project_root)?;
        let in_flight =
            state_store::has_open_in_flight_dispatch(&conn, &document_id, record.generation)?;
        if dispatch_should_coalesce_in_flight(in_flight, false) {
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
            crate::ops_log::log_op(
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
    crate::ops_log::log_op(
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
    let document_id = crate::session_actor::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let mut conn = open_state_db(&bootstrap.project_root)?;
    migrate_legacy_actor_projection(&bootstrap.project_root, &mut conn)?;
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
    let mut conn = open_state_db(&bootstrap.project_root)?;
    migrate_legacy_actor_projection(&bootstrap.project_root, &mut conn)?;
    if let Some(file) = request.file.as_ref() {
        let document_id = crate::session_actor::canonical_document_id_in(
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
    let mut conn = open_state_db(&bootstrap.project_root)?;
    migrate_legacy_actor_projection(&bootstrap.project_root, &mut conn)?;
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
    project_sessions_projection_for_actor(&bootstrap.project_root, &next.document_id)
        .with_context(|| {
            format!(
                "failed to repair sessions projection for {}",
                next.document_id
            )
        })?;
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
    document_id: Option<&str>,
) -> Result<()> {
    match projection {
        "all" => {
            emit_actor_projection(project_root)?;
            repair_sessions_projection(project_root, document_id)?;
            emit_layout_projection(project_root)?;
        }
        "actors" | "session-actors" | "session-actors.json" => {
            emit_actor_projection(project_root)?;
        }
        "sessions" | "sessions.json" => {
            repair_sessions_projection(project_root, document_id)?;
        }
        "layout" | "last_layout" | "last_layout.json" => {
            emit_layout_projection(project_root)?;
        }
        other => anyhow::bail!("unknown projection repair target: {other}"),
    }
    Ok(())
}

pub(crate) fn repair_sessions_projection(
    project_root: &Path,
    document_id: Option<&str>,
) -> Result<()> {
    if let Some(document_id) = document_id {
        return project_sessions_projection_for_actor(project_root, document_id);
    }
    let store = load_actor_store(project_root)?;
    for document_id in store.keys() {
        project_sessions_projection_for_actor(project_root, document_id)?;
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
    let document_id = crate::session_actor::canonical_document_id_in(
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
            crate::session_actor::detect_document_harness_in(&bootstrap.project_root, &document_id)
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
    crate::ops_log::log_op(
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
    let document_id = crate::session_actor::canonical_document_id_in(
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
    // A `Closed` actor is a valid target for recovery commands. `session_clear` /
    // `session_interrupt_clear` reset a closed session, and `session_restart`
    // SUPERSEDES it — blue/green drain-and-supersede (#supkill-bg): a restart must
    // not fail closed against a dead/closed generation. The whole point of restart
    // is to replace the superseded generation with the next one, so a `Closed`
    // actor is exactly when restart is meaningful (the operator's
    // `session restart-supervisor` hit `generation N is closed` here). Only
    // `Blocked` (and non-recovery commands on a `Closed` actor) still reject.
    let recovers_closed_actor = matches!(
        command_kind.as_str(),
        "session_clear" | "session_interrupt_clear" | "session_restart"
    ) && record.state
        == agent_doc_sqlite::state_store::ActorState::Closed;
    if matches!(
        record.state,
        agent_doc_sqlite::state_store::ActorState::Blocked
    ) || (record.state == agent_doc_sqlite::state_store::ActorState::Closed
        && !recovers_closed_actor)
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
    crate::ops_log::log_op(
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
    // against a `Closed` actor is the supersede signal — record the prior (closed)
    // generation and the next generation the restart drains toward so racing
    // dispatch / log forensics can see the "superseded -> retry against N+1"
    // redirect instead of the old `generation N is closed` hard reject.
    if command_kind == "session_restart"
        && record.state == agent_doc_sqlite::state_store::ActorState::Closed
    {
        crate::ops_log::log_op(
            &file,
            &format!(
                "supervisor_restart_supersede file={} action=supersede_closed_actor prior_generation={} next_generation={} receipt_id={} caller=operator",
                file.display(),
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

pub(crate) fn handle_admin_operation(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<ControllerAdminReceipt> {
    let operation_kind = request_string(&request.command_kind, "command_kind")?;
    let status = request.state.as_deref().unwrap_or("accepted");
    let document_id = request.file.as_ref().map(|file| {
        crate::session_actor::canonical_document_id_in(
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

pub fn project_root_from_arg(root: Option<&Path>) -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let start = match root {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => cwd.join(path),
        None => cwd,
    };
    let start = start.canonicalize().unwrap_or(start);
    agent_doc_fs::find_project_root(&start)
        .or_else(|| {
            if start.join(".git").exists() || start.join(".agent-doc").exists() {
                Some(start.to_path_buf())
            } else {
                None
            }
        })
        .with_context(|| format!("no project root found from {}", start.display()))
}

pub fn run_status(root: Option<&Path>, ensure: bool) -> Result<()> {
    let project_root = project_root_from_arg(root)?;
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
    let project_root = project_root_from_arg(root)?;
    serve_with_options(
        &project_root,
        LaunchMode::parse(launch_mode)?,
        listen_socket.map(Path::to_path_buf),
        controller_generation,
        previous_controller_pid,
        status::parse_handoff_state(handoff_state)?,
    )
}

pub fn run_shutdown(root: Option<&Path>) -> Result<()> {
    let project_root = project_root_from_arg(root)?;
    println!("{}", request(&project_root, "shutdown")?);
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
        let dir = tempfile::TempDir::new().unwrap();
        let args = vec![
            "/home/user/.cargo/bin/agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            dir.path().display().to_string(),
        ];
        assert!(args_match_same_project_controller(&args, dir.path()));

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
        assert!(args_match_same_project_controller(
            &shell_sentinel,
            dir.path()
        ));

        let other_dir = tempfile::TempDir::new().unwrap();
        assert!(!args_match_same_project_controller(&args, other_dir.path()));

        let non_controller = vec![
            "agent-doc".to_string(),
            "preflight".to_string(),
            dir.path().join("task.md").display().to_string(),
        ];
        assert!(!args_match_same_project_controller(
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
        assert!(!args_match_same_project_controller(
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
        let runtime = Arc::new(ControllerRuntime::new(bootstrap).unwrap());
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
        assert!(controller_status_matches_current_binary(&status).unwrap());
        let freshness = status
            .freshness
            .as_ref()
            .expect("controller status should expose binary freshness proof");
        assert_eq!(freshness.controller.pid, Some(bootstrap.pid));
        assert!(freshness.installed_binary.is_some());
    }
    #[test]
    fn controller_client_response_read_times_out() {
        let dir = tempfile::TempDir::new().unwrap();
        let sock = socket_path(dir.path());
        std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
        let name = sock.clone().to_fs_name::<GenericFilePath>().unwrap();
        let listener = ListenerOptions::new().name(name).create_sync().unwrap();
        let handle = std::thread::spawn(move || {
            let _stream = listener.accept().unwrap();
            std::thread::sleep(CONTROLLER_RPC_TIMEOUT * 2);
        });

        let started = Instant::now();
        let err = request(dir.path(), "status").unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "controller request should fail within the bounded timeout"
        );
        assert!(
            err.to_string().contains("timed out") || format!("{err:#}").contains("timed out"),
            "{err:#}"
        );
        handle.join().unwrap();
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
        let shutdown = request(&project_root, "shutdown").unwrap();
        assert!(shutdown.contains("\"ok\":true"), "{shutdown}");
        handle.join().unwrap();
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
        let shutdown = request(&project_root, "shutdown").unwrap();
        assert!(shutdown.contains("\"ok\":true"), "{shutdown}");
        handle.join().unwrap();
    }

    #[test]
    fn connect_or_launch_adopts_controller_published_during_launch_lock_contention() {
        // #suprecyclelock / #1j8q: a self-recycled supervisor can re-run `start`
        // while another project-root launcher still owns controller-launch.lock.
        // If that holder publishes a healthy controller before the waiter gives
        // up, the waiter must connect to it instead of surfacing os-error-11.
        let dir = tempfile::TempDir::new().unwrap();
        let project_root = dir.path().to_path_buf();
        let held_lock = LaunchLock::acquire(&project_root).unwrap();

        let caller_root = project_root.clone();
        let caller = std::thread::spawn(move || {
            let stream = connect_or_launch_with_lock_wait(
                &caller_root,
                LaunchMode::Lazy,
                Duration::from_millis(150),
            )?;
            drop(stream);
            Ok::<(), anyhow::Error>(())
        });

        std::thread::sleep(Duration::from_millis(25));
        let server_root = project_root.clone();
        let server = std::thread::spawn(move || serve(&server_root, LaunchMode::Lazy).unwrap());
        wait_for_test_controller(&project_root);

        let result = caller.join().unwrap();
        assert!(
            result.is_ok(),
            "contended waiter should adopt the published controller, not fail: {:?}",
            result.err().map(|err| err.to_string())
        );
        drop(held_lock);

        let ops_log = std::fs::read_to_string(project_root.join(".agent-doc/logs/ops.log"))
            .unwrap_or_default();
        assert!(
            ops_log.contains("controller_launch_lock_waiter_adopted_published_controller"),
            "contended adoption proof marker missing:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("controller launch already in progress"),
            "the historical launch-lock error must not be logged:\n{ops_log}"
        );

        let shutdown = request(&project_root, "shutdown").unwrap();
        assert!(shutdown.contains("\"ok\":true"), "{shutdown}");
        server.join().unwrap();
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
        crate::session_actor::record_session_start_direct(&doc, "session-operator", "%41", "@1", 1)
            .unwrap();
        crate::session_actor::transition_state_direct(
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
    fn supervisor_stale_warning_message_fires_only_for_active_stale_host() {
        // #fccsupwarn — the read-only WARN must fire exactly for a live controller
        // running a stale binary, and stay silent otherwise (fail-open).
        let dir = tempfile::TempDir::new().unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        let response = handle_request(
            &(serde_json::json!({ "command": "status" }).to_string() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let mut status: ControllerStatus = serde_json::from_str(&response).unwrap();

        // Fresh, active controller — no warning.
        assert!(status.active);
        assert!(supervisor_stale_warning_message(&status).is_none());

        // Stale launch identity on an active host → warning fires with the recycle hint.
        let mut stale = current_binary_identity().unwrap();
        stale.len = stale.len.wrapping_add(1);
        status.controller_binary = Some(stale);
        let msg = supervisor_stale_warning_message(&status)
            .expect("an active host on a stale binary must warn");
        assert!(msg.contains("STALE"), "message: {msg}");
        assert!(msg.contains("admin recycle"), "message: {msg}");
        assert!(!msg.contains("--force"), "message: {msg}");
        assert!(!msg.contains("interrupt-clear"), "message: {msg}");

        // Inactive controller (nothing hosting) → no warning even if stale.
        status.active = false;
        assert!(supervisor_stale_warning_message(&status).is_none());

        // Active but no recorded launch identity → fail-open, no warning.
        status.active = true;
        status.controller_binary = None;
        assert!(supervisor_stale_warning_message(&status).is_none());
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
                /* write_wedged */ false, /* reexec_failed */ false,
                /* cycle_open */ false,
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
    fn host_supervisor_stale_warning_message_uses_non_destructive_refresh() {
        // #fccsupwarn3 — a routine stale-binary refresh must never tell an agent to
        // discard a live turn. Force/interrupt-clear remain explicit wedged-owner
        // hatches, not stale-supervisor freshness guidance.
        let msg = host_supervisor_stale_warning_message(166599);
        assert!(msg.contains("pid 166599"), "message: {msg}");
        assert!(msg.contains("STALE"), "message: {msg}");
        assert!(msg.contains("agent-doc admin recycle"), "message: {msg}");
        assert!(
            msg.contains("agent-doc session restart-supervisor <FILE>"),
            "message: {msg}"
        );
        assert!(msg.contains("idle boundary"), "message: {msg}");
        assert!(msg.contains("refuses busy panes"), "message: {msg}");
        assert!(!msg.contains("--force"), "message: {msg}");
        assert!(!msg.contains("interrupt-clear"), "message: {msg}");
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
            "session-actors.json",
            &doc_id,
            "test projection lag",
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
            "compatibility_output"
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
        let runtime = Arc::new(ControllerRuntime::new(bootstrap).unwrap());
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
        let mut sentinel = spawn_controller_sentinel(dir.path());
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
        let start = Instant::now();
        let mut exit = None;
        while start.elapsed() < Duration::from_secs(2) {
            match sentinel.try_wait().unwrap() {
                Some(status) => {
                    exit = Some(status);
                    break;
                }
                None => std::thread::sleep(Duration::from_millis(25)),
            }
        }
        let status = exit.expect("wedged sentinel pid must be reaped");
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
        let mut sentinel = spawn_preparing_controller_sentinel(dir.path());
        let pid = sentinel.id();
        // Let the process age past a zero threshold (start age is /proc dir mtime).
        std::thread::sleep(Duration::from_millis(1100));

        let (reaped, kept) =
            reap_orphaned_preparing_controllers(dir.path(), Duration::from_secs(0), false).unwrap();
        assert_eq!((reaped, kept), (1, 0));

        // The live orphan must actually be terminated (the whole point vs. the
        // record-scoped reaper). The sentinel is our child, so a killed process
        // lingers as a zombie until `wait()` — poll `try_wait`.
        let start = Instant::now();
        let mut exit = None;
        while start.elapsed() < Duration::from_secs(2) {
            match sentinel.try_wait().unwrap() {
                Some(status) => {
                    exit = Some(status);
                    break;
                }
                None => std::thread::sleep(Duration::from_millis(25)),
            }
        }
        let status = exit.expect("aged preparing orphan must be reaped");
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
    fn qflood_coalesces_busy_in_flight_redispatch_and_releases_on_ready() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/qflood.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-qf\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        crate::session_actor::record_session_start_direct(&doc, "session-qf", "%41", "@1", 1)
            .unwrap();
        // Actor actively running a turn (mid-turn / pane busy).
        crate::session_actor::transition_state_direct(
            &doc,
            "session-qf",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Busy,
            "supervisor",
            "turn_started",
        )
        .unwrap();
        let document_id =
            crate::session_actor::canonical_document_id_in(dir.path(), &doc.to_string_lossy());
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
            command_kind: Some("managed_reopen".to_string()),
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

        // Re-fire while still Busy and in flight ⇒ coalesced (bail), not piled into
        // the pane as another trigger.
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

        // Actor returns to Ready (turn finished): the in-flight marker is released so
        // the next turn dispatches cleanly.
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
        crate::session_actor::record_session_start_direct(&doc, "session-anw0", "%41", "@1", 1)
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
        crate::session_actor::transition_state_direct(
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
        crate::session_actor::transition_state_direct(
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
        crate::session_actor::record_session_start_direct(&doc, "session-anw0h", "%41", "@1", 1)
            .unwrap();
        crate::session_actor::transition_state_direct(
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
    fn process_binary_is_stale_matches_and_differs() {
        // `#ctlrecycle` foundation. No recorded identity → never stale (fail-open).
        assert!(!process_binary_is_stale(None));
        // The freshly-installed identity matches itself → not stale.
        let current = current_binary_identity().unwrap();
        assert!(!process_binary_is_stale(Some(&current)));
        // A different recorded identity (an old build) → stale.
        let stale = ControllerBinaryIdentity {
            path: current.path.clone(),
            version: "0.0.0-stale".to_string(),
            len: current.len.wrapping_add(1),
            modified_secs: current.modified_secs.wrapping_add(1),
            modified_nanos: 0,
        };
        assert!(process_binary_is_stale(Some(&stale)));
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
        let doc_id = crate::session_actor::canonical_document_id_in(
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
            harness: crate::session_actor::detect_document_harness_in(
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
        let generations = crate::session_actor::next_generation(&doc, "efs").unwrap();
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
}
