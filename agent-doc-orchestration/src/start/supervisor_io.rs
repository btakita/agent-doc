//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_supervisor::ipc_protocol::IpcResponse;

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

    fn harness_binary(&self) -> &str {
        &self.harness_binary
    }

    fn write_child_pty(&self, bytes: &[u8]) -> Result<(), String> {
        let guard = self.inject_writer.lock().unwrap();
        match guard.as_ref() {
            Some(writer_arc) => {
                let mut writer = writer_arc.lock().unwrap();
                writer
                    .write_all_blocking(bytes)
                    .map_err(|err| format!("write error: {err}"))
            }
            None => Err("no active session".to_string()),
        }
    }
}

impl agent_doc_supervisor_io::ipc::SupervisorIpcLifecycleState for SupervisorShared {
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
        *self.restart_mode.lock().unwrap() = mode;
    }

    fn set_restart_requested(&self, requested: bool) {
        self.restart_requested.store(requested, Ordering::Relaxed);
    }

    fn binary_stale(&self) -> bool {
        self.binary_stale.load(Ordering::Relaxed)
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
}

impl agent_doc_supervisor_io::ipc::SupervisorIpcSnapshotState for SupervisorShared {
    fn supervisor_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    fn supervisor_state_label(&self) -> String {
        let state = self.supervisor_state.lock().unwrap();
        state.as_str().to_string()
    }

    fn actor_state_label(&self) -> Option<String> {
        self.actor_state
            .lock()
            .unwrap()
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

    fn actor_file(&self) -> Option<String> {
        self.actor_runtime
            .as_ref()
            .map(|runtime| runtime.file.display().to_string())
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

    fn handle_replica_register(&self, file: &str, identity: &str) -> IpcResponse {
        agent_doc_supervisor_crdt_io::handle_replica_register(file, identity)
    }

    fn handle_replica_deregister(&self, file: &str, identity: &str) -> IpcResponse {
        agent_doc_supervisor_crdt_io::handle_replica_deregister(file, identity)
    }

    fn handle_replica_update(&self, file: &str, identity: &str, update_b64: &str) -> IpcResponse {
        agent_doc_supervisor_crdt_io::handle_replica_update(file, identity, update_b64)
    }

    fn handle_replica_pull(&self, file: &str, identity: &str) -> IpcResponse {
        agent_doc_supervisor_crdt_io::handle_replica_pull(file, identity)
    }

    fn handle_replica_ack(
        &self,
        file: &str,
        identity: &str,
        patch_id: &str,
        generation: u64,
    ) -> IpcResponse {
        agent_doc_supervisor_crdt_io::handle_replica_ack(file, identity, patch_id, generation)
    }

    fn handle_replica_awareness(
        &self,
        file: &str,
        identity: &str,
        awareness_b64: &str,
    ) -> IpcResponse {
        agent_doc_supervisor_crdt_io::handle_replica_awareness(file, identity, awareness_b64)
    }

    fn handle_crdt_checkpoint(&self, file: &str, source: &str) -> IpcResponse {
        agent_doc_supervisor_crdt_io::handle_crdt_checkpoint(file, source)
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
    use tempfile::TempDir;
    use tmux_router::IsolatedTmux;
    // --- `#crdtauth5` end-to-end fan-out over the NEW IPC path -------------------

    /// A throwaway tracked document under a temp project root so `doc_hash` /
    /// authority lease resolution work against a real path.
    fn crdt_temp_doc(name: &str) -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, format!("# {name}\n\nbody\n")).unwrap();
        (dir, path)
    }

    /// Send a `#crdtauth5` replica IPC method over a REAL supervisor socket and
    /// return the parsed response — the production handler routes it through the
    /// per-document `crdt_relay_host` hub.
    fn crdt_send(sock: &std::path::Path, method: &IpcMethod) -> IpcResponse {
        agent_doc_supervisor_io::ipc::send_command(sock, method).expect("send crdt ipc")
    }

    #[test]
    fn crdtauth5_end_to_end_fan_out_over_the_ipc_path() {
        use agent_doc_merge::crdt_sync::ReplicaState;
        use base64::{Engine as _, engine::general_purpose::STANDARD as B64};

        let (_dir, doc) = crdt_temp_doc("fanout.md");
        let project_root = doc.parent().unwrap().to_path_buf();
        let file_str = doc.display().to_string();

        // Make the document editor-attached (MultiReplica): seed a live owner
        // lease for the CURRENT pid so `authority_for_file` resolves MultiReplica.
        agent_doc_test_support::seed_live_plugin_owner_lease(&file_str);
        assert!(
            agent_doc_plugin_owner::crdt_authority::authority_for_file(&file_str).editor_attached(),
            "test setup: the document must be editor-attached"
        );

        // Stand up the REAL supervisor IPC socket with the production handler.
        let shared = Arc::new(SupervisorShared::new(
            "test",
            "crdtauth5-instance".to_string(),
        ));
        let shared_for_ipc = shared.clone();
        let session_id = "crdtauth5-session";
        let mut ipc = agent_doc_supervisor_io::ipc::SupervisorIpc::start(
            &project_root,
            session_id,
            move |method| {
                agent_doc_supervisor_io::ipc::handle_supervisor_ipc(method, shared_for_ipc.as_ref())
            },
        )
        .expect("start supervisor ipc");
        std::thread::sleep(std::time::Duration::from_millis(50));
        let sock = agent_doc_supervisor_io::ipc::socket_path(&project_root, session_id);

        // Editor A and Editor B each register over the socket. The supervisor
        // hub mints their client-ids and returns the canonical bootstrap state.
        let reg_a = crdt_send(
            &sock,
            &IpcMethod::ReplicaRegister {
                file: file_str.clone(),
                identity: "intellij:A".into(),
            },
        );
        assert!(reg_a.ok, "register A: {reg_a:?}");
        let a_id = reg_a.data.as_ref().unwrap()["client_id"].as_u64().unwrap();
        let a_bootstrap = B64
            .decode(
                reg_a.data.as_ref().unwrap()["bootstrap_b64"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();

        let reg_b = crdt_send(
            &sock,
            &IpcMethod::ReplicaRegister {
                file: file_str.clone(),
                identity: "vscode:B".into(),
            },
        );
        assert!(reg_b.ok, "register B: {reg_b:?}");
        let b_id = reg_b.data.as_ref().unwrap()["client_id"].as_u64().unwrap();
        assert_ne!(a_id, b_id, "distinct editors mint distinct client-ids");

        // Editor A's FFI node (a real ReplicaState bound to the minted id) makes a
        // LOCAL edit and encodes the delta against the canonical bootstrap state.
        let editor_a = ReplicaState::from_encoded(a_id, &a_bootstrap).unwrap();
        editor_a.apply_local_edit(0, 0, "FROM-A");
        let a_delta = editor_a.diff(&ReplicaState::new(0).state_vector()).unwrap();

        // Editor A broadcasts its update OVER THE IPC PATH. The supervisor hub
        // integrates canonical and fans the delta out to editor B's hub-side mirror.
        let upd = crdt_send(
            &sock,
            &IpcMethod::ReplicaUpdate {
                file: file_str.clone(),
                identity: "intellij:A".into(),
                update_b64: B64.encode(&a_delta),
            },
        );
        assert!(upd.ok, "replica update: {upd:?}");
        let data = upd.data.as_ref().unwrap();
        assert_eq!(data["origin"].as_u64().unwrap(), a_id);
        let targets = data["targets"].as_array().unwrap();
        assert_eq!(
            targets.len(),
            1,
            "the update fans out to the one other replica (B)"
        );
        assert_eq!(targets[0]["client_id"].as_u64().unwrap(), b_id);

        // Until B applies + ACKs the queued fan-out delivery, the live delivery cut
        // is not converged and materialization must not be considered safe.
        assert!(
            !agent_doc_crdt_relay_io::commit_barrier_for_file(&doc),
            "unacked live fan-out delivery blocks the materialization barrier"
        );

        // B pulls its own pending delivery, applies it to its FFI node, then ACKs it.
        let editor_b = ReplicaState::from_encoded(b_id, &a_bootstrap).unwrap();
        let pull = crdt_send(
            &sock,
            &IpcMethod::ReplicaPull {
                file: file_str.clone(),
                identity: "vscode:B".into(),
            },
        );
        assert!(pull.ok, "replica pull: {pull:?}");
        let pulled = pull.data.as_ref().unwrap()["updates"].as_array().unwrap();
        assert_eq!(pulled.len(), 1, "B owns one pending delivery");
        assert_eq!(pulled[0]["target"].as_u64().unwrap(), b_id);
        let patch_id = pulled[0]["patch_id"].as_str().unwrap().to_string();
        let generation = pulled[0]["generation"].as_u64().unwrap();
        let to_b = B64
            .decode(pulled[0]["update_b64"].as_str().unwrap())
            .unwrap();
        editor_b.apply_update(&to_b).unwrap();
        assert!(
            editor_b.text().contains("FROM-A"),
            "replica B received A's op over the IPC fan-out path: {:?}",
            editor_b.text()
        );
        let ack = crdt_send(
            &sock,
            &IpcMethod::ReplicaAck {
                file: file_str.clone(),
                identity: "vscode:B".into(),
                patch_id,
                generation,
            },
        );
        assert!(ack.ok, "replica ack: {ack:?}");
        assert!(
            ack.data.as_ref().unwrap()["acknowledged"]
                .as_bool()
                .unwrap(),
            "the target ack clears the pending delivery"
        );

        // The commit barrier then captures a consistent cut INCLUDING the fanned-out
        // ops: the canonical replica holds A's edit and every live delivery is ACKed.
        assert!(agent_doc_crdt_relay_io::commit_barrier_for_file(&doc));
        agent_doc_crdt_relay_io::with_hub(&doc, |hub| {
            assert!(
                hub.canonical_text().contains("FROM-A"),
                "the commit barrier cut holds the fanned-out op"
            );
        })
        .unwrap();

        // Deregister B over the socket; the hub drops its mirror.
        let dereg = crdt_send(
            &sock,
            &IpcMethod::ReplicaDeregister {
                file: file_str.clone(),
                identity: "vscode:B".into(),
            },
        );
        assert!(dereg.ok && dereg.data.as_ref().unwrap()["removed"].as_bool().unwrap());

        ipc.stop();
    }

    #[test]
    fn crdtauth5_detached_path_refuses_replica_register_and_allocates_no_hub() {
        // A document with NO live editor (Detached / GitAuthoritative) must refuse
        // the new replica family and allocate no hub — the headless control-plane
        // path is unchanged.
        let (_dir, doc) = crdt_temp_doc("detached.md");
        let project_root = doc.parent().unwrap().to_path_buf();
        let file_str = doc.display().to_string();
        // No lease seeded → authority is GitAuthoritative.
        assert!(
            !agent_doc_plugin_owner::crdt_authority::authority_for_file(&file_str)
                .editor_attached(),
            "test setup: the document must be detached"
        );

        let shared = Arc::new(SupervisorShared::new(
            "test",
            "detached-instance".to_string(),
        ));
        let shared_for_ipc = shared.clone();
        let session_id = "crdtauth5-detached";
        let mut ipc = agent_doc_supervisor_io::ipc::SupervisorIpc::start(
            &project_root,
            session_id,
            move |method| {
                agent_doc_supervisor_io::ipc::handle_supervisor_ipc(method, shared_for_ipc.as_ref())
            },
        )
        .expect("start supervisor ipc");
        std::thread::sleep(std::time::Duration::from_millis(50));
        let sock = agent_doc_supervisor_io::ipc::socket_path(&project_root, session_id);

        let reg = crdt_send(
            &sock,
            &IpcMethod::ReplicaRegister {
                file: file_str.clone(),
                identity: "intellij:detached".into(),
            },
        );
        assert!(!reg.ok, "the detached path refuses replica register");
        assert!(
            reg.error
                .as_deref()
                .unwrap_or_default()
                .contains("not editor-attached"),
            "{reg:?}"
        );
        // No hub was allocated for the detached document.
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let allocated = agent_doc_crdt_relay_io::hub_is_allocated_for_test(&hash);
        assert!(
            !allocated,
            "the detached path must not allocate a relay hub"
        );

        ipc.stop();
    }

    #[test]
    fn handle_ipc_inject_normalizes_submit_newline_before_writing() {
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        let written = Arc::new(Mutex::new(Vec::new()));
        *shared.inject_writer.lock().unwrap() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(
            Box::new(RecordingWriter(written.clone())),
        ))));

        let response = agent_doc_supervisor_io::ipc::handle_supervisor_ipc(
            IpcMethod::Inject {
                bytes: "agent-doc tasks/software/tsift.md\n".to_string(),
            },
            shared.as_ref(),
        );

        assert!(response.ok);
        assert_eq!(
            written.lock().unwrap().as_slice(),
            b"agent-doc tasks/software/tsift.md\r"
        );
    }

    #[test]
    fn handle_ipc_state_includes_editor_sync_for_actor_file() {
        let (_dir, doc) = crdt_temp_doc("state-editor-sync.md");
        let project_root = doc.parent().unwrap().to_path_buf();
        let file_str = doc.display().to_string();
        agent_doc_debounce::document_changed_with_content_for_editor(
            &file_str,
            "disk plus unsaved editor text",
            Some("jetbrains:state"),
        );
        let runtime = SessionActorRuntime {
            project_root,
            file: doc.clone(),
            session_id: "state-session".to_string(),
            pane_id: "%1".to_string(),
            generation: 7,
        };
        let shared = Arc::new(SupervisorShared::with_actor_runtime(
            "test",
            "state-instance".to_string(),
            "claude",
            Some(runtime),
            Some(agent_doc_sqlite::state_store::ActorState::Ready),
            None,
        ));

        let response =
            agent_doc_supervisor_io::ipc::handle_supervisor_ipc(IpcMethod::State, shared.as_ref());
        assert!(response.ok, "{response:?}");
        let data = response.data.expect("state data");
        let sync = data.get("editor_sync").expect("editor_sync field");
        assert_eq!(sync["file"], file_str);
        assert_eq!(sync["statuses"][0]["edit_epoch"], 1);
        assert_eq!(sync["statuses"][0]["in_flight"], true);
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
        *shared.inject_writer.lock().unwrap() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(
            Box::new(RecordingWriter(written.clone())),
        ))));
        let response = agent_doc_supervisor_io::ipc::handle_supervisor_ipc(
            IpcMethod::Inject {
                bytes: "agent-doc tasks/software/tsift.md\n".to_string(),
            },
            shared.as_ref(),
        );

        assert!(response.ok, "{response:?}");
        assert_eq!(
            written.lock().unwrap().as_slice(),
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
        *shared.inject_writer.lock().unwrap() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(
            Box::new(RecordingWriter(written.clone())),
        ))));
        let clear = agent_doc_supervisor_io::ipc::handle_supervisor_ipc(
            IpcMethod::Clear {
                bytes: "/clear".to_string(),
            },
            shared.as_ref(),
        );
        assert!(clear.ok, "clear must bypass the dispatch gate: {clear:?}");
        // Delivery matches the Inject path: trailing-newline normalization only,
        // no spurious CR added when the control text has none.
        assert_eq!(written.lock().unwrap().as_slice(), b"/clear");
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
