//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

impl agent_doc_supervisor_process_io::SupervisorProcessIoState for SupervisorShared {
    fn transition_actor_ready_for_prompt(&self) {
        self.transition_actor_state(
            agent_doc_sqlite::state_store::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        );
    }

    fn clear_suppress_stale_ctrl_d_until_prompt(&self) {
        self.suppress_stale_ctrl_d_until_prompt
            .store(false, Ordering::Relaxed);
    }

    fn suppress_stale_ctrl_d_until_prompt(&self) -> bool {
        self.suppress_stale_ctrl_d_until_prompt
            .load(Ordering::Relaxed)
    }

    fn prompt_visible_once(&self) -> bool {
        self.prompt_visible_once.load(Ordering::Relaxed)
    }
}

impl agent_doc_supervisor_io::ipc::SupervisorInjectDeliveryState for SupervisorShared {
    fn inject_pane_id(&self) -> Option<String> {
        self.inject_pane.clone()
    }

    fn harness_binary(&self) -> String {
        self.current_harness()
    }

    fn write_child_pty(&self, bytes: &[u8]) -> Result<(), String> {
        let guard = self.inject_writer.lock();
        match guard.as_ref() {
            Some(writer_arc) => {
                let mut writer = writer_arc.lock();
                writer
                    .write_all_blocking(bytes)
                    .map_err(|err| format!("write error: {err}"))
            }
            None => Err("no active session".to_string()),
        }
    }

    fn begin_prompt_dispatch(
        &self,
        source: &str,
        bytes: &str,
    ) -> agent_doc_supervisor_io::ipc::PromptDispatchAdmission {
        self.begin_prompt_dispatch_projection(source, bytes)
    }

    fn clear_prompt_dispatch_on_failure(&self, key: &str) {
        self.clear_prompt_dispatch_projection_on_failure(key);
    }

    fn begin_turn_steering(
        &self,
        steering_id: &str,
        _bytes: &str,
    ) -> agent_doc_supervisor_io::ipc::PromptDispatchAdmission {
        const TURN_STEERING_ADMISSION_CAPACITY: usize = 256;
        let mut admissions = self.turn_steering_admissions.lock();
        if admissions.iter().any(|current| current == steering_id) {
            return agent_doc_supervisor_io::ipc::PromptDispatchAdmission::Duplicate {
                key: steering_id.to_string(),
            };
        }
        if admissions.len() >= TURN_STEERING_ADMISSION_CAPACITY {
            admissions.pop_front();
        }
        admissions.push_back(steering_id.to_string());
        agent_doc_supervisor_io::ipc::PromptDispatchAdmission::Accepted {
            key: steering_id.to_string(),
        }
    }

    fn clear_turn_steering_on_failure(&self, steering_id: &str) {
        self.turn_steering_admissions
            .lock()
            .retain(|current| current != steering_id);
    }
}

impl agent_doc_supervisor_io::ipc::SupervisorIpcLifecycleState for SupervisorShared {
    fn actor_waiting_input(&self) -> bool {
        *self.actor_state.lock() == Some(agent_doc_sqlite::state_store::ActorState::WaitingInput)
    }

    fn transition_actor_busy(&self, caller: &str, reason: &str) {
        self.transition_actor_state(
            agent_doc_sqlite::state_store::ActorState::Busy,
            caller,
            reason,
        );
    }

    fn transition_actor_waiting_input(&self, caller: &str, reason: &str) {
        self.transition_actor_state(
            agent_doc_sqlite::state_store::ActorState::WaitingInput,
            caller,
            reason,
        );
    }

    fn set_restart_mode(&self, mode: String) {
        *self.restart_mode.lock() = mode;
    }

    fn set_restart_requested(&self, requested: bool) {
        self.restart_requested.store(requested, Ordering::Relaxed);
    }

    fn binary_stale(&self) -> bool {
        self.refresh_binary_stale()
    }

    fn set_restart_reexec(&self, reexec: bool) {
        self.restart_reexec.store(reexec, Ordering::Relaxed);
    }

    fn set_stop_requested(&self, requested: bool) {
        self.stop_requested.store(requested, Ordering::Relaxed);
    }

    fn set_stop_agent_requested(&self, requested: bool) {
        self.stop_agent_requested
            .store(requested, Ordering::Relaxed);
    }

    fn kill_child_for_ipc(&self) {
        self.kill_child();
    }

    fn wake_restart_prompt(&self) -> Result<(), String> {
        let pane = self
            .inject_pane
            .as_deref()
            .ok_or_else(|| "supervisor restart prompt has no owned pane".to_string())?;
        tmux_router::Tmux::default_server()
            .send_keys(pane, "")
            .map_err(|err| format!("failed to wake supervisor restart prompt on {pane}: {err:#}"))
    }
}

impl agent_doc_supervisor_io::ipc::SupervisorIpcSnapshotState for SupervisorShared {
    fn supervisor_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    fn supervisor_state_label(&self) -> String {
        let state = self.supervisor_state.lock();
        state.as_str().to_string()
    }

    fn actor_state_label(&self) -> Option<String> {
        self.actor_state
            .lock()
            .map(|state| state.as_str().to_string())
    }

    fn actor_session_id(&self) -> Option<String> {
        self.actor_runtime
            .as_ref()
            .map(|runtime| runtime.session_id.clone())
    }

    fn actor_pane_id(&self) -> Option<String> {
        self.actor_runtime
            .as_ref()
            .map(|runtime| runtime.pane_id.clone())
    }

    fn actor_generation(&self) -> Option<u64> {
        self.actor_runtime
            .as_ref()
            .map(|runtime| runtime.generation)
    }

    fn current_harness(&self) -> String {
        SupervisorShared::current_harness(self)
    }

    fn actor_file(&self) -> Option<String> {
        self.actor_runtime
            .as_ref()
            .map(|runtime| runtime.file.display().to_string())
    }

    fn editor_authority_snapshot(&self) -> Option<serde_json::Value> {
        let file = self.actor_runtime.as_ref()?.file.clone();
        let display = file.display().to_string();
        // `#idlewatchtransitionrevision`: a *status* snapshot needs two fields —
        // `live_editors` and `delivery_converged` — and used to obtain them by
        // materializing the entire document, SHA-256ing it, and appending an
        // `ops.log` line with its length and hash, then discarding the text
        // (`..`). One supervisor exists per open document, and this snapshot is
        // served on a status poll, so on this project it was three
        // whole-document materializations every ten seconds, forever, at idle.
        //
        // The compact revision carries both fields and drives no commit barrier.
        // `CurrentRevision` has no `EditorSyncPending`: that state exists only
        // because the text path can find the barrier not ready, and it arrives
        // here as `Current { delivery_converged: false }`, which already reports
        // `in_flight: true`.
        Some(
            match agent_doc_crdt_relay_io::current_revision_for_file(&file) {
                Ok(agent_doc_crdt_relay_io::CurrentRevision::Detached) => serde_json::json!({
                    "file": display,
                    "authority": "detached",
                    "in_flight": false,
                }),
                Ok(agent_doc_crdt_relay_io::CurrentRevision::Current {
                    live_editors,
                    delivery_converged,
                    ..
                }) => serde_json::json!({
                    "file": display,
                    "authority": "lazily_current",
                    "live_editors": live_editors,
                    "delivery_converged": delivery_converged,
                    "in_flight": !delivery_converged,
                }),
                Ok(agent_doc_crdt_relay_io::CurrentRevision::EditorAttachedMissingReplica) => {
                    serde_json::json!({
                        "file": display,
                        "authority": "editor_attached_missing_replica",
                        "in_flight": true,
                    })
                }
                Err(error) => serde_json::json!({
                    "file": display,
                    "authority": "unavailable",
                    "in_flight": true,
                    "error": error.to_string(),
                }),
            },
        )
    }

    fn restart_count(&self) -> u32 {
        self.restart_count.load(Ordering::Relaxed)
    }

    fn cwd_source(&self) -> &'static str {
        self.cwd_source
    }

    fn supervisor_pid(&self) -> u32 {
        self.supervisor_pid
    }

    fn supervisor_instance_id(&self) -> String {
        self.supervisor_instance_id.clone()
    }

    fn child_pid(&self) -> u32 {
        self.child_pid.load(Ordering::Relaxed)
    }
}

impl agent_doc_supervisor_io::ipc::SupervisorIpcHandlerState for SupervisorShared {
    fn capability_dispatch_blocker(&self) -> Option<String> {
        SupervisorShared::capability_dispatch_blocker(self)
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use agent_doc_config::Config;
    use agent_doc_frontmatter::frontmatter::Frontmatter;
    use agent_doc_hooks_io::fire_doc_hooks;
    use agent_doc_project_config_io as project_config_io;
    use agent_doc_supervisor::ipc_protocol::IpcMethod;
    use std::collections::HashMap;
    use tmux_router::IsolatedTmux;

    #[test]
    fn restart_ipc_refreshes_stale_binary_before_reexec_decision() {
        let mut stale_launch =
            agent_doc_controller_io::project_controller::current_binary_identity()
                .expect("current binary identity");
        stale_launch.len = stale_launch.len.saturating_add(1);
        let mut shared = SupervisorShared::with_actor_runtime(
            "test",
            "stale-restart-instance".to_string(),
            Some(stale_launch),
            "claude",
            None,
            None,
            None,
        );
        shared.supervisor_pid = 0;

        assert!(!shared.binary_stale.load(Ordering::Relaxed));
        agent_doc_supervisor_io::ipc::request_supervisor_restart(&shared, "continue".to_string())
            .expect("request restart");

        assert!(shared.binary_stale.load(Ordering::Relaxed));
        assert!(shared.restart_requested.load(Ordering::Relaxed));
        assert!(shared.restart_reexec.load(Ordering::Relaxed));
    }

    #[test]
    fn restart_ipc_keeps_fresh_binary_on_relaunch_path() {
        let fresh_launch = agent_doc_controller_io::project_controller::current_binary_identity()
            .expect("current binary identity");
        let mut shared = SupervisorShared::with_actor_runtime(
            "test",
            "fresh-restart-instance".to_string(),
            Some(fresh_launch),
            "claude",
            None,
            None,
            None,
        );
        shared.supervisor_pid = 0;

        agent_doc_supervisor_io::ipc::request_supervisor_restart(&shared, "continue".to_string())
            .expect("request restart");

        assert!(!shared.binary_stale.load(Ordering::Relaxed));
        assert!(shared.restart_requested.load(Ordering::Relaxed));
        assert!(!shared.restart_reexec.load(Ordering::Relaxed));
    }

    #[test]
    fn handle_ipc_inject_normalizes_submit_newline_before_writing() {
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        let written = Arc::new(Mutex::new(Vec::new()));
        *shared.inject_writer.lock() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(Box::new(
            RecordingWriter(written.clone()),
        )))));

        let response = agent_doc_supervisor_io::ipc::handle_supervisor_ipc(
            IpcMethod::Inject {
                bytes: "agent-doc tasks/software/tsift.md\n".to_string(),
            },
            shared.as_ref(),
        );

        assert!(response.ok);
        assert_eq!(
            written.lock().as_slice(),
            b"agent-doc tasks/software/tsift.md\r"
        );
    }

    #[test]
    fn handle_ipc_inject_suppresses_duplicate_before_ready_projection_clears() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("projection.md");
        std::fs::write(&doc, "# projection\n").unwrap();
        let runtime = SessionActorRuntime {
            project_root: dir.path().to_path_buf(),
            file: doc,
            session_id: "projection-session".to_string(),
            pane_id: "%1".to_string(),
            generation: 9,
        };
        let shared = Arc::new(SupervisorShared::with_actor_runtime(
            "test",
            "projection-instance".to_string(),
            None,
            "claude",
            Some(runtime),
            Some(agent_doc_sqlite::state_store::ActorState::Ready),
            None,
        ));
        let written = Arc::new(Mutex::new(Vec::new()));
        *shared.inject_writer.lock() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(Box::new(
            RecordingWriter(written.clone()),
        )))));

        let bytes = "agent-doc tasks/software/tsift.md\n";
        let first = agent_doc_supervisor_io::ipc::deliver_supervisor_inject(
            shared.as_ref(),
            bytes,
            "ipc_inject",
        );
        let duplicate = agent_doc_supervisor_io::ipc::deliver_supervisor_inject(
            shared.as_ref(),
            bytes,
            "ipc_inject",
        );

        assert_eq!(
            first.unwrap(),
            agent_doc_supervisor_io::ipc::SupervisorInjectDeliveryOutcome::Delivered
        );
        assert_eq!(
            duplicate.unwrap(),
            agent_doc_supervisor_io::ipc::SupervisorInjectDeliveryOutcome::DuplicateSuppressed
        );
        assert!(shared.prompt_dispatch_grace_active(std::time::Duration::from_secs(15)));
        assert_eq!(
            written.lock().as_slice(),
            b"agent-doc tasks/software/tsift.md\r"
        );

        *shared.prompt_dispatch_projection.lock() = None;
        let after_ready = agent_doc_supervisor_io::ipc::deliver_supervisor_inject(
            shared.as_ref(),
            bytes,
            "ipc_inject",
        );
        assert_eq!(
            after_ready.unwrap(),
            agent_doc_supervisor_io::ipc::SupervisorInjectDeliveryOutcome::Delivered
        );
        assert_eq!(
            written.lock().as_slice(),
            b"agent-doc tasks/software/tsift.md\ragent-doc tasks/software/tsift.md\r"
        );
    }

    #[test]
    fn live_turn_steering_socket_delivers_exact_selection_once_and_keeps_turn_busy() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("steering.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: steering-session\n---\n\n# Steering\n",
        )
        .unwrap();
        let runtime = SessionActorRuntime {
            project_root: dir.path().to_path_buf(),
            file: doc,
            session_id: "steering-session".to_string(),
            pane_id: "%31".to_string(),
            generation: 12,
        };
        let shared = Arc::new(SupervisorShared::with_actor_runtime(
            "test",
            "steering-instance".to_string(),
            None,
            "codex",
            Some(runtime),
            Some(agent_doc_sqlite::state_store::ActorState::Busy),
            None,
        ));
        let written = Arc::new(Mutex::new(Vec::new()));
        *shared.inject_writer.lock() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(Box::new(
            RecordingWriter(written.clone()),
        )))));
        let handler_state = shared.clone();
        let mut ipc = agent_doc_supervisor_io::ipc::SupervisorIpc::start(
            dir.path(),
            "steering-session",
            move |method| {
                agent_doc_supervisor_io::ipc::handle_supervisor_ipc(method, handler_state.as_ref())
            },
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        let socket = agent_doc_supervisor_io::ipc::socket_path(dir.path(), "steering-session");
        let selection = "Keep this line\n  and these spaces  ";
        let method = IpcMethod::Steer {
            steering_id: "selection-1".to_string(),
            bytes: selection.to_string(),
        };

        let first = agent_doc_supervisor_io::ipc::send_command(&socket, &method).unwrap();
        let retry = agent_doc_supervisor_io::ipc::send_command(&socket, &method).unwrap();

        assert!(first.ok, "{first:?}");
        assert_eq!(first.data.as_ref().unwrap()["kind"], "turn_steering_ack");
        assert_eq!(first.data.as_ref().unwrap()["outcome"], "delivered");
        assert_eq!(first.data.as_ref().unwrap()["n"], selection.len());
        assert!(retry.ok, "{retry:?}");
        assert_eq!(retry.data.as_ref().unwrap()["outcome"], "duplicate");
        assert_eq!(retry.data.as_ref().unwrap()["n"], 0);
        assert_eq!(
            written.lock().as_slice(),
            agent_doc_supervisor::input::normalize_supervisor_inject_bytes(selection)
        );
        assert_eq!(
            *shared.actor_state.lock(),
            Some(agent_doc_sqlite::state_store::ActorState::Busy),
            "steering must not finish or restart the active turn"
        );
        ipc.stop();
    }

    #[test]
    fn turn_steering_rejects_when_no_active_turn_exists() {
        let shared = Arc::new(SupervisorShared::new(
            "test",
            "idle-steering-instance".to_string(),
        ));
        let written = Arc::new(Mutex::new(Vec::new()));
        *shared.inject_writer.lock() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(Box::new(
            RecordingWriter(written.clone()),
        )))));

        let response = agent_doc_supervisor_io::ipc::handle_supervisor_ipc(
            IpcMethod::Steer {
                steering_id: "idle-selection".to_string(),
                bytes: "must not become a new trigger".to_string(),
            },
            shared.as_ref(),
        );

        assert!(!response.ok);
        assert!(
            response
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("active busy turn")
        );
        assert!(written.lock().is_empty());
    }

    #[test]
    fn handle_ipc_inject_allows_pending_capability_proof() {
        // `#capproofbg`: a *pending* managed-capability proof no longer blocks
        // dispatch. The `Inject` is delivered immediately (here to a recording PTY
        // writer) while the proof runs in the background; only a proven FAILURE
        // gates the inject (`handle_ipc_inject_rejects_failed_capability_proof`).
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        shared.set_capability_proof_gate(CapabilityProofGate::Pending, None);
        let written = Arc::new(Mutex::new(Vec::new()));
        *shared.inject_writer.lock() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(Box::new(
            RecordingWriter(written.clone()),
        )))));
        let response = agent_doc_supervisor_io::ipc::handle_supervisor_ipc(
            IpcMethod::Inject {
                bytes: "agent-doc tasks/software/tsift.md\n".to_string(),
            },
            shared.as_ref(),
        );

        assert!(response.ok, "{response:?}");
        assert_eq!(
            written.lock().as_slice(),
            b"agent-doc tasks/software/tsift.md\r"
        );
    }
    #[test]
    fn handle_ipc_inject_rejects_failed_capability_proof() {
        // The dispatch gate still fails closed on a proven proof FAILURE — a
        // failed proof must remain visible and block dispatch (`#tsiftmdcrash`).
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        shared.set_capability_proof_gate(
            CapabilityProofGate::Failed,
            Some("network denied".to_string()),
        );
        let response = agent_doc_supervisor_io::ipc::handle_supervisor_ipc(
            IpcMethod::Inject {
                bytes: "agent-doc tasks/software/tsift.md\n".to_string(),
            },
            shared.as_ref(),
        );

        assert!(!response.ok);
        assert!(
            response
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("capability proof failed"),
            "{response:?}"
        );
    }
    #[test]
    fn handle_ipc_clear_bypasses_failed_capability_proof() {
        // #codex-capability-proof-unrecoverable: with the gate `Failed`, an
        // `Inject` is refused by the dispatch gate but `Clear` is delivered
        // (here to a recording PTY writer) without the gate error.
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        shared.set_capability_proof_gate(
            CapabilityProofGate::Failed,
            Some("network denied".to_string()),
        );

        let inject = agent_doc_supervisor_io::ipc::handle_supervisor_ipc(
            IpcMethod::Inject {
                bytes: "/clear".to_string(),
            },
            shared.as_ref(),
        );
        assert!(!inject.ok);
        assert!(
            inject
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("capability proof failed"),
            "{inject:?}"
        );

        let written = Arc::new(Mutex::new(Vec::new()));
        *shared.inject_writer.lock() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(Box::new(
            RecordingWriter(written.clone()),
        )))));
        let clear = agent_doc_supervisor_io::ipc::handle_supervisor_ipc(
            IpcMethod::Clear {
                bytes: "/clear".to_string(),
            },
            shared.as_ref(),
        );
        assert!(clear.ok, "clear must bypass the dispatch gate: {clear:?}");
        // Delivery matches the Inject path: trailing-newline normalization only,
        // no spurious CR added when the control text has none.
        assert_eq!(written.lock().as_slice(), b"/clear");
    }
    #[test]
    fn handle_ipc_stop_bypasses_failed_capability_proof() {
        // Stopping a session is recovery, not dispatch: it must succeed even
        // when the capability proof failed.
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        shared.set_capability_proof_gate(
            CapabilityProofGate::Failed,
            Some("network denied".to_string()),
        );
        let response = agent_doc_supervisor_io::ipc::handle_supervisor_ipc(
            IpcMethod::Stop { graceful: false },
            shared.as_ref(),
        );
        assert!(response.ok, "{response:?}");
        assert!(shared.stop_requested.load(Ordering::Relaxed));
    }
}
