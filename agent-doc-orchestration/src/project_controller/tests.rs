    use super::*;
    // `rusqlite` is a dev-dependency: these tests open the controller state DB
    // directly to assert the schema/rows the seam writes. `Connection` is the
    // `state_store` re-export already in scope via `super::*`.
    use agent_doc_sqlite::state_store::{load_actor_transitions_from_db, sqlite_i64};
    use rusqlite::params;
    use std::collections::BTreeMap;

    #[test]
    fn controller_paths_are_project_local() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(
            socket_path(dir.path()),
            dir.path().join(".agent-doc/controller.sock")
        );
        assert_eq!(
            launch_lock_path(dir.path()),
            dir.path().join(".agent-doc/locks/controller-launch.lock")
        );
        assert_eq!(
            state_path(dir.path()),
            dir.path().join(".agent-doc/controller-state.json")
        );
    }

    fn actor_record(
        document_id: &str,
        pane: &str,
        window: &str,
    ) -> crate::session_actor::ActorRecord {
        crate::session_actor::ActorRecord {
            document_id: document_id.to_string(),
            session_id: "session-1".to_string(),
            generation: 1,
            pane_id: pane.to_string(),
            window_id: window.to_string(),
            harness: "codex".to_string(),
            state: crate::session_actor::ActorState::Starting,
            last_transition: crate::session_actor::ActorLastTransition {
                caller: "start".to_string(),
                reason: "session_start".to_string(),
                timestamp: 10,
                prior_generation: 0,
                new_generation: 1,
            },
        }
    }

    #[test]
    fn actor_store_writes_sqlite_before_actor_projection() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let record = actor_record(&document_id, "%41", "@1");

        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let stored: String = conn
            .query_row(
                "SELECT pane_id FROM documents WHERE document_id = ?1",
                params![document_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, "%41");

        let projection: BTreeMap<String, crate::session_actor::ActorRecord> = serde_json::from_str(
            &std::fs::read_to_string(actor_projection_path(dir.path())).unwrap(),
        )
        .unwrap();
        assert_eq!(projection.get(&record.document_id).unwrap(), &record);
    }

    #[test]
    fn sessions_projection_reconciles_existing_registry_entry() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let mut registry = crate::sessions::SessionRegistry::new();
        registry.insert(
            document_id.clone(),
            crate::sessions::SessionEntry {
                pane: "%old".to_string(),
                pid: 123,
                cwd: dir.path().to_string_lossy().to_string(),
                started: "1".to_string(),
                session_id: "old-session".to_string(),
                file: document_id.clone(),
                window: "@old".to_string(),
                supervisor_instance_id: "supervisor-1".to_string(),
            },
        );
        crate::sessions::save_in(dir.path(), &registry).unwrap();

        let record = actor_record(&document_id, "%51", "@2");
        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let projected = crate::sessions::load_in(dir.path()).unwrap();
        let entry = projected.get(&document_id).unwrap();
        assert_eq!(entry.pane, "%51");
        assert_eq!(entry.window, "@2");
        assert_eq!(entry.session_id, "session-1");
        assert_eq!(entry.pid, 123);
        assert_eq!(entry.supervisor_instance_id, "supervisor-1");
    }

    #[test]
    fn sessions_projection_removes_displaced_cross_document_owner() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc_a = dir.path().join("tasks/a.md");
        let doc_b = dir.path().join("tasks/b.md");
        std::fs::create_dir_all(doc_a.parent().unwrap()).unwrap();
        std::fs::write(&doc_a, "a").unwrap();
        std::fs::write(&doc_b, "b").unwrap();
        let document_a = doc_a.to_string_lossy().to_string();
        let document_b = doc_b.to_string_lossy().to_string();
        let mut registry = crate::sessions::SessionRegistry::new();
        registry.insert(
            document_a.clone(),
            crate::sessions::SessionEntry {
                pane: "%70".to_string(),
                pid: 100,
                cwd: dir.path().to_string_lossy().to_string(),
                started: "1".to_string(),
                session_id: "session-a".to_string(),
                file: document_a.clone(),
                window: "@7".to_string(),
                supervisor_instance_id: "supervisor-a".to_string(),
            },
        );
        registry.insert(
            document_b.clone(),
            crate::sessions::SessionEntry {
                pane: "%old".to_string(),
                pid: 200,
                cwd: dir.path().to_string_lossy().to_string(),
                started: "2".to_string(),
                session_id: "session-b".to_string(),
                file: document_b.clone(),
                window: "@old".to_string(),
                supervisor_instance_id: "supervisor-b".to_string(),
            },
        );
        crate::sessions::save_in(dir.path(), &registry).unwrap();

        let mut record_a = actor_record(&document_a, "%70", "@7");
        record_a.session_id = "session-a".to_string();
        store_actor_record(dir.path(), Some(0), &record_a).unwrap();
        let mut record_b = actor_record(&document_b, "%70", "@7");
        record_b.session_id = "session-b".to_string();
        store_actor_record(dir.path(), Some(0), &record_b).unwrap();

        let projected = crate::sessions::load_in(dir.path()).unwrap();
        assert!(
            !projected.contains_key(&document_a),
            "displaced document must not remain in sessions.json"
        );
        let entry_b = projected.get(&document_b).unwrap();
        assert_eq!(entry_b.pane, "%70");
        assert_eq!(entry_b.window, "@7");
        assert_eq!(entry_b.session_id, "session-b");
    }

    #[test]
    fn sessions_projection_creates_missing_registry_entry_from_controller_state() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let record = actor_record(&document_id, "%61", "@3");

        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let projected = crate::sessions::load_in(dir.path()).unwrap();
        let entry = projected.get(&document_id).unwrap();
        assert_eq!(entry.pane, "%61");
        assert_eq!(entry.window, "@3");
        assert_eq!(entry.session_id, "session-1");
        assert_eq!(entry.file, document_id);
        assert_eq!(entry.cwd, dir.path().to_string_lossy());

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let diagnostics: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projection_diagnostics WHERE projection = 'sessions.json'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(diagnostics, 0);
    }

    #[test]
    fn sessions_projection_failure_records_generation_hash_retry_metadata() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/sessions.json")).unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let record = actor_record(&document_id, "%61", "@3");

        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let (source_generation, intended_hash, retry_status, message): (
            i64,
            String,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT source_generation, intended_hash, retry_status, message \
                 FROM projection_diagnostics \
                 WHERE projection = 'sessions.json' \
                 ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(source_generation, 1);
        assert!(!intended_hash.is_empty());
        assert_eq!(retry_status, "retry_pending");
        assert!(message.contains("failed to write projection"));
    }

    #[test]
    fn layout_state_migrates_legacy_projection_to_sqlite() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(
            layout_projection_path(dir.path()),
            serde_json::to_string(&vec!["tasks/a.md".to_string(), "tasks/b.md".to_string()])
                .unwrap(),
        )
        .unwrap();

        let loaded = load_layout_state(dir.path()).unwrap();

        assert_eq!(loaded, vec!["tasks/a.md", "tasks/b.md"]);
        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let stored: String = conn
            .query_row(
                "SELECT columns_json FROM layout_states WHERE scope = ?1",
                params![DEFAULT_LAYOUT_SCOPE],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&stored).unwrap(),
            loaded
        );
    }

    #[test]
    fn layout_state_prefers_sqlite_over_drifted_projection() {
        let dir = tempfile::TempDir::new().unwrap();
        store_layout_state(dir.path(), &["tasks/current.md".to_string()]).unwrap();
        std::fs::write(
            layout_projection_path(dir.path()),
            serde_json::to_string(&vec!["tasks/stale.md".to_string()]).unwrap(),
        )
        .unwrap();

        let loaded = load_layout_state(dir.path()).unwrap();

        assert_eq!(loaded, vec!["tasks/current.md"]);
    }

    #[test]
    fn singleton_launch_lock_rejects_concurrent_holder() {
        let dir = tempfile::TempDir::new().unwrap();
        let first = LaunchLock::acquire(dir.path()).unwrap();
        let second = LaunchLock::acquire(dir.path());
        assert!(second.is_err());
        drop(first);
        assert!(LaunchLock::acquire(dir.path()).is_ok());
    }

    #[test]
    fn bootstrap_state_round_trips_launch_mode_and_epoch() {
        let dir = tempfile::TempDir::new().unwrap();
        let written = write_bootstrap(dir.path(), LaunchMode::Lazy).unwrap();
        let read = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(read.launch_mode, LaunchMode::Lazy);
        assert_eq!(read.bootstrap_epoch, written.bootstrap_epoch);
        assert_eq!(read.controller_binary, written.controller_binary);
        assert_eq!(
            read.controller_binary,
            Some(current_binary_identity().unwrap())
        );
        assert_eq!(read.controller_generation, written.controller_generation);
        assert_eq!(read.handoff_state, ControllerHandoffState::Stable);
        assert_eq!(
            read.socket_path,
            dir.path().join(".agent-doc/controller.sock")
        );
    }

    #[test]
    fn handoff_bootstrap_persists_generation_previous_pid_and_temp_socket() {
        let dir = tempfile::TempDir::new().unwrap();
        let temp_sock = dir.path().join(".agent-doc/controller-handoff-test.sock");
        let written = write_bootstrap_with_options(
            dir.path(),
            temp_sock.clone(),
            LaunchMode::Lazy,
            7,
            ControllerHandoffState::Preparing,
            Some(1234),
        )
        .unwrap();
        let read = read_bootstrap(dir.path()).unwrap().unwrap();

        assert_eq!(written.controller_generation, 7);
        assert_eq!(read.controller_generation, 7);
        assert_eq!(read.socket_path, temp_sock);
        assert_eq!(read.handoff_state, ControllerHandoffState::Preparing);
        assert_eq!(read.previous_controller_pid, Some(1234));
        assert!(read.handoff_started_at.is_some());
    }

    #[test]
    fn prepare_and_promote_handoff_update_controller_state() {
        let dir = tempfile::TempDir::new().unwrap();
        let bootstrap = write_bootstrap_with_options(
            dir.path(),
            dir.path().join(".agent-doc/controller-handoff-test.sock"),
            LaunchMode::Lazy,
            2,
            ControllerHandoffState::Preparing,
            Some(111),
        )
        .unwrap();
        let mut should_stop = false;

        let prepare = handle_request(
            &(serde_json::json!({ "command": "prepare_handoff" }).to_string() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        assert!(prepare.contains("\"ok\":true"), "{prepare}");
        let preparing = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(preparing.handoff_state, ControllerHandoffState::Preparing);

        let promote = handle_request(
            &(serde_json::json!({ "command": "promote_handoff" }).to_string() + "\n"),
            &preparing,
            &mut should_stop,
        )
        .unwrap();
        assert!(promote.contains("\"ok\":true"), "{promote}");
        let promoted = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(promoted.socket_path, socket_path(dir.path()));
        assert_eq!(promoted.handoff_state, ControllerHandoffState::Stable);
        assert_eq!(promoted.handoff_started_at, None);
    }

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

    fn wait_for_test_controller(project_root: &Path) {
        let started = Instant::now();
        loop {
            if connect(project_root).is_ok() {
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "test controller did not start"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn missing_or_changed_controller_binary_identity_is_stale() {
        let current = current_binary_identity().unwrap();
        let missing = ControllerStatus {
            active: true,
            project_root: PathBuf::from("/tmp/project"),
            socket_path: PathBuf::from("/tmp/project/.agent-doc/controller.sock"),
            launch_mode: Some(LaunchMode::Lazy),
            bootstrap_epoch: Some(1),
            pid: Some(2),
            controller_binary: None,
            controller_generation: Some(1),
            handoff_state: Some(ControllerHandoffState::Stable),
            handoff_started_at: None,
            previous_controller_pid: None,
            stale_duplicate_pids: Vec::new(),
            control_plane: default_control_plane_status(),
        };
        assert!(!controller_status_matches_current_binary(&missing).unwrap());

        let mut changed = current.clone();
        changed.modified_nanos = changed.modified_nanos.wrapping_add(1);
        let stale = ControllerStatus {
            controller_binary: Some(changed),
            ..missing
        };
        assert!(!controller_status_matches_current_binary(&stale).unwrap());

        let fresh = ControllerStatus {
            controller_binary: Some(current),
            ..stale
        };
        assert!(controller_status_matches_current_binary(&fresh).unwrap());
    }

    #[test]
    fn controller_binary_resolution_prefers_existing_current_exe() {
        let dir = tempfile::TempDir::new().unwrap();
        let current = dir.path().join("current-agent-doc");
        let path_bin_dir = dir.path().join("bin");
        let path_bin = path_bin_dir.join("agent-doc");
        std::fs::create_dir_all(&path_bin_dir).unwrap();
        std::fs::write(&current, "current").unwrap();
        std::fs::write(&path_bin, "path").unwrap();

        let resolved = resolve_agent_doc_binary_from_env(
            Some(current.clone()),
            Some(OsString::from("agent-doc")),
            Some(path_bin_dir.into_os_string()),
            dir.path(),
        )
        .unwrap();

        assert_eq!(resolved, current);
    }

    #[test]
    fn controller_binary_resolution_falls_back_to_path_when_current_exe_is_missing() {
        let dir = tempfile::TempDir::new().unwrap();
        let path_bin_dir = dir.path().join("bin");
        let path_bin = path_bin_dir.join("agent-doc");
        std::fs::create_dir_all(&path_bin_dir).unwrap();
        std::fs::write(&path_bin, "path").unwrap();

        let resolved = resolve_agent_doc_binary_from_env(
            Some(dir.path().join("deleted-agent-doc")),
            Some(OsString::from("agent-doc")),
            Some(path_bin_dir.into_os_string()),
            dir.path(),
        )
        .unwrap();

        assert_eq!(resolved, path_bin);
    }

    fn test_bootstrap(dir: &tempfile::TempDir) -> ControllerBootstrap {
        ControllerBootstrap {
            project_root: dir.path().to_path_buf(),
            socket_path: socket_path(dir.path()),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 123,
            pid: 456,
            controller_binary: Some(current_binary_identity().unwrap()),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        }
    }

    #[test]
    fn controller_start_register_and_lifecycle_update_actor_and_lease() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-controller\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let start = ControllerRequest {
            command: "start_session".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-controller".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: Some("@1".to_string()),
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
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let record = envelope.data.unwrap();
        assert_eq!(record.state, crate::session_actor::ActorState::Starting);

        let register = ControllerRequest {
            command: "register_supervisor".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-controller".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: Some("starting".to_string()),
            caller: None,
            reason: None,
            supervisor_pid: Some(999),
            supervisor_socket: Some("/tmp/agent-doc-test.sock".to_string()),
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&register).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);

        let lifecycle = ControllerRequest {
            command: "mark_lifecycle".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-controller".to_string()),
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
        let response = handle_request(
            &(serde_json::to_string(&lifecycle).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let record = envelope.data.unwrap();
        assert_eq!(record.state, crate::session_actor::ActorState::Ready);
        assert_eq!(record.last_transition.reason, "prompt_ready");

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let (pid, socket, runtime_state): (i64, String, String) = conn
            .query_row(
                "SELECT supervisor_pid, supervisor_socket, runtime_state FROM supervisor_leases WHERE document_id = ?1 AND generation = 1",
                params![doc.to_string_lossy().to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(pid, 999);
        assert_eq!(socket, "/tmp/agent-doc-test.sock");
        assert_eq!(runtime_state, "ready");
    }

    #[test]
    fn controller_supervisor_heartbeat_refreshes_stale_lease_without_actor_transition() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/heartbeat.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-heartbeat\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);

        crate::session_actor::record_session_start_direct(
            &doc,
            "session-heartbeat",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        let record = crate::session_actor::transition_state_direct(
            &doc,
            "session-heartbeat",
            "%41",
            Some(1),
            crate::session_actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();
        upsert_supervisor_lease(
            dir.path(),
            &record,
            Some(999),
            Some("/tmp/old.sock"),
            "starting",
        )
        .unwrap();

        let heartbeat = ControllerRequest {
            command: "supervisor_heartbeat".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-heartbeat".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: Some("ready".to_string()),
            caller: None,
            reason: None,
            supervisor_pid: Some(1001),
            supervisor_socket: Some("/tmp/new.sock".to_string()),
            command_kind: None,
            diagnostic_payload: None,
        };
        let lease = handle_supervisor_heartbeat(&bootstrap, None, heartbeat).unwrap();
        assert_eq!(lease.runtime_state.as_deref(), Some("ready"));
        assert_eq!(lease.supervisor_pid, Some(1001));
        assert_eq!(lease.supervisor_socket.as_deref(), Some("/tmp/new.sock"));

        let transitions = load_actor_transitions_from_db(
            &Connection::open(state_db_path(dir.path())).unwrap(),
            &doc.to_string_lossy(),
        )
        .unwrap();
        assert_eq!(
            transitions.len(),
            2,
            "heartbeat must not create an actor transition"
        );
    }

    #[test]
    fn gc_closes_stale_starting_actor_without_fresh_supervisor_lease() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/stale-starting.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-stale-starting\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let record = crate::session_actor::record_session_start_direct(
            &doc,
            "session-stale-starting",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        let old = timestamp_secs() - 7200;
        Connection::open(state_db_path(dir.path()))
            .unwrap()
            .execute(
                "UPDATE actor_transitions SET timestamp = ?1 WHERE new_generation = 1",
                params![sqlite_i64(old, "old timestamp").unwrap()],
            )
            .unwrap();

        let (closed, kept) =
            close_stale_starting_actors(dir.path(), Duration::from_secs(3600), false).unwrap();
        assert_eq!(closed, 1);
        assert_eq!(kept, 0);

        let updated = load_actor_record(dir.path(), &record.document_id)
            .unwrap()
            .unwrap();
        assert_eq!(updated.state, crate::session_actor::ActorState::Closed);
        assert_eq!(updated.last_transition.caller, "gc");
        assert_eq!(updated.last_transition.reason, "stale_starting_actor");
    }

    #[test]
    fn gc_keeps_stale_starting_actor_with_fresh_supervisor_lease() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/live-starting.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-live-starting\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let record = crate::session_actor::record_session_start_direct(
            &doc,
            "session-live-starting",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        upsert_supervisor_lease(
            dir.path(),
            &record,
            Some(std::process::id()),
            Some("/tmp/live-starting.sock"),
            "starting",
        )
        .unwrap();
        let old = timestamp_secs() - 7200;
        Connection::open(state_db_path(dir.path()))
            .unwrap()
            .execute(
                "UPDATE actor_transitions SET timestamp = ?1 WHERE new_generation = 1",
                params![sqlite_i64(old, "old timestamp").unwrap()],
            )
            .unwrap();

        let (closed, kept) =
            close_stale_starting_actors(dir.path(), Duration::from_secs(3600), false).unwrap();
        assert_eq!(closed, 0);
        assert_eq!(kept, 1);

        let updated = load_actor_record(dir.path(), &record.document_id)
            .unwrap()
            .unwrap();
        assert_eq!(updated.state, crate::session_actor::ActorState::Starting);
    }

    #[test]
    fn gc_closes_stale_starting_actor_with_stale_heartbeat_even_when_pid_is_alive() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/stuck-starting.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-stuck-starting\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let record = crate::session_actor::record_session_start_direct(
            &doc,
            "session-stuck-starting",
            "%42",
            "@1",
            1,
        )
        .unwrap();
        upsert_supervisor_lease(
            dir.path(),
            &record,
            Some(std::process::id()),
            Some("/tmp/stuck-starting.sock"),
            "starting",
        )
        .unwrap();
        let old = timestamp_secs() - 7200;
        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        conn.execute(
            "UPDATE actor_transitions SET timestamp = ?1 WHERE new_generation = 1",
            params![sqlite_i64(old, "old transition timestamp").unwrap()],
        )
        .unwrap();
        conn.execute(
            "UPDATE supervisor_leases SET last_heartbeat = ?1 WHERE document_id = ?2 AND generation = 1",
            params![
                sqlite_i64(old, "old heartbeat timestamp").unwrap(),
                record.document_id
            ],
        )
        .unwrap();

        let (closed, kept) =
            close_stale_starting_actors(dir.path(), Duration::from_secs(3600), false).unwrap();
        assert_eq!(closed, 1);
        assert_eq!(kept, 0);

        let updated = load_actor_record(dir.path(), &record.document_id)
            .unwrap()
            .unwrap();
        assert_eq!(updated.state, crate::session_actor::ActorState::Closed);
        assert_eq!(updated.last_transition.reason, "stale_starting_actor");
    }

    #[test]
    fn normal_path_actor_cleanup_records_calling_surface() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/preflight-stale-starting.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-preflight-stale\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let record = crate::session_actor::record_session_start_direct(
            &doc,
            "session-preflight-stale",
            "%51",
            "@1",
            1,
        )
        .unwrap();
        let old = timestamp_secs() - 7200;
        Connection::open(state_db_path(dir.path()))
            .unwrap()
            .execute(
                "UPDATE actor_transitions SET timestamp = ?1 WHERE new_generation = 1",
                params![sqlite_i64(old, "old timestamp").unwrap()],
            )
            .unwrap();

        let (closed, kept) = close_stale_starting_actors_for_caller(
            dir.path(),
            Duration::from_secs(3600),
            false,
            "preflight",
        )
        .unwrap();
        assert_eq!(closed, 1);
        assert_eq!(kept, 0);

        let updated = load_actor_record(dir.path(), &record.document_id)
            .unwrap()
            .unwrap();
        assert_eq!(updated.state, crate::session_actor::ActorState::Closed);
        assert_eq!(updated.last_transition.caller, "preflight");
        assert_eq!(updated.last_transition.reason, "stale_starting_actor");
    }

    #[test]
    fn controller_lifecycle_rejects_stale_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/stale.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-stale\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        crate::session_actor::record_session_start_direct(&doc, "session-stale", "%41", "@1", 1)
            .unwrap();
        crate::session_actor::project_binding_in(
            dir.path(),
            &doc.to_string_lossy(),
            "session-stale",
            "%42",
            "@2",
            "sync",
            "recover_owner",
        )
        .unwrap();

        let lifecycle = ControllerRequest {
            command: "mark_lifecycle".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-stale".to_string()),
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
        let response = handle_request(
            &(serde_json::to_string(&lifecycle).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(!envelope.ok);
        assert!(envelope.error.unwrap().contains("no longer current"));
    }

    #[test]
    fn controller_lifecycle_allows_same_pane_stale_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/same-pane.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-same\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        crate::session_actor::record_session_start_direct(&doc, "session-same", "%41", "@1", 1)
            .unwrap();
        crate::session_actor::record_session_start_direct(&doc, "session-same", "%41", "@1", 2)
            .unwrap();

        let lifecycle = ControllerRequest {
            command: "mark_lifecycle".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-same".to_string()),
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
        let response = handle_request(
            &(serde_json::to_string(&lifecycle).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(
            envelope.ok,
            "same-pane stale generation should succeed: {:?}",
            envelope.error
        );
        assert_eq!(
            envelope.data.unwrap().state,
            crate::session_actor::ActorState::Ready
        );
    }

    #[test]
    fn controller_actor_binding_and_dispatch_use_authoritative_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/route.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-route\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        crate::session_actor::record_session_start_direct(&doc, "session-route", "%41", "@1", 1)
            .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-route",
            "%41",
            Some(1),
            crate::session_actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let binding = ControllerRequest {
            command: "actor_binding".to_string(),
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
            &(serde_json::to_string(&binding).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ActorBindingResponse> =
            serde_json::from_str(&response).unwrap();
        let binding = envelope.data.unwrap();
        assert_eq!(binding.status, ActorBindingStatus::Bound);
        assert_eq!(binding.record.unwrap().pane_id, "%41");

        let dispatch = ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-route".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("managed_reopen".to_string()),
            diagnostic_payload: Some("test dispatch".to_string()),
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
        let authorization = envelope.data.unwrap();
        assert_eq!(authorization.accepted_stage, "ready");
        assert!(authorization.receipt.receipt_id > 0);
        assert_eq!(
            authorization.receipt.status,
            ControllerDispatchResultStatus::Accepted
        );
        assert_eq!(
            authorization.receipt.proof_scope,
            ControllerDispatchProofScope::AcceptedOnly
        );
        assert!(!authorization.receipt.dispatch_start_proven);

        let stale = ControllerRequest {
            generation: Some(0),
            ..dispatch
        };
        let response = handle_request(
            &(serde_json::to_string(&stale).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(!envelope.ok);
        let error = envelope.error.unwrap();
        assert!(error.contains("requested generation 0"));
        assert!(error.contains("receipt_id="));

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let accepted: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dispatch_attempts WHERE accepted_stage = 'ready'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let failed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dispatch_attempts WHERE failed_stage = 'stale_generation'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let typed_accepted: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dispatch_attempts WHERE result_status = 'accepted' AND proof_scope = 'accepted_only' AND dispatch_start_proven = 0",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let typed_rejected: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dispatch_attempts WHERE result_status = 'rejected' AND proof_scope = 'accepted_only' AND dispatch_start_proven = 0 AND failed_stage = 'stale_generation'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(accepted, 1);
        assert_eq!(failed, 1);
        assert_eq!(typed_accepted, 1);
        assert_eq!(typed_rejected, 1);
    }

    #[test]
    fn controller_actor_binding_absent_is_typed_not_found() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/no-binding.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-route\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let binding = ControllerRequest {
            command: "actor_binding".to_string(),
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
            &(serde_json::to_string(&binding).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ActorBindingResponse> =
            serde_json::from_str(&response).unwrap();
        assert!(
            envelope.ok,
            "actor_binding not_found should not be an error"
        );
        let binding = envelope.data.unwrap();
        assert_eq!(binding.status, ActorBindingStatus::NotFound);
        assert!(binding.record.is_none());
    }

    #[test]
    fn controller_admin_operation_returns_durable_receipt() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/admin.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-admin\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let admin = ControllerRequest {
            command: "admin_operation".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: Some("accepted".to_string()),
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("preflight".to_string()),
            diagnostic_payload: Some("admin receipt test".to_string()),
        };
        let response = handle_request(
            &(serde_json::to_string(&admin).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ControllerAdminReceipt> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let receipt = envelope.data.unwrap();
        assert!(receipt.receipt_id > 0);
        assert_eq!(receipt.operation_kind, "preflight");
        assert_eq!(receipt.status, "accepted");
        let document_id = doc.to_string_lossy().to_string();
        assert_eq!(receipt.document_id.as_deref(), Some(document_id.as_str()));

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let stored: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM admin_operations WHERE id = ?1 AND operation_kind = 'preflight' AND status = 'accepted'",
                params![sqlite_i64(receipt.receipt_id, "admin receipt id").unwrap()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, 1);
    }

    #[test]
    fn controller_queue_control_rejects_stale_generation_and_blocks_dispatch_when_paused() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/queue-control.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-queue\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        crate::session_actor::record_session_start_direct(&doc, "session-queue", "%41", "@1", 1)
            .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-queue",
            "%41",
            Some(1),
            crate::session_actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let stale_pause = ControllerRequest {
            command: "queue_control".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: Some(0),
            state: Some("pause".to_string()),
            caller: Some("admin".to_string()),
            reason: Some("test stale generation".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("pause".to_string()),
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&stale_pause).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ControllerAdminReceipt> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let receipt = envelope.data.unwrap();
        assert_eq!(receipt.status, "rejected");
        assert_eq!(receipt.failed_stage.as_deref(), Some("stale_generation"));
        assert_eq!(receipt.observed_generation, Some(0));
        assert_eq!(receipt.current_generation, Some(1));

        let conn = open_state_db(dir.path()).unwrap();
        let controls: i64 = conn
            .query_row("SELECT COUNT(*) FROM queue_controls", [], |row| row.get(0))
            .unwrap();
        assert_eq!(controls, 0, "stale queue control must not mutate state");

        let pause = ControllerRequest {
            generation: Some(1),
            reason: Some("operator pause".to_string()),
            ..stale_pause
        };
        let response = handle_request(
            &(serde_json::to_string(&pause).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ControllerAdminReceipt> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let receipt = envelope.data.unwrap();
        assert_eq!(receipt.status, "accepted");

        let dispatch = ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-queue".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("managed_reopen".to_string()),
            diagnostic_payload: Some("paused dispatch test".to_string()),
        };
        let response = handle_request(
            &(serde_json::to_string(&dispatch).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(!envelope.ok);
        assert!(
            envelope
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("failed_stage=queue_paused")
        );

        let conn = open_state_db(dir.path()).unwrap();
        let document_id =
            crate::session_actor::canonical_document_id_in(dir.path(), &doc.to_string_lossy());
        let failed_stage: String = conn
            .query_row(
                "SELECT failed_stage FROM dispatch_attempts WHERE document_id = ?1 ORDER BY id DESC LIMIT 1",
                params![&document_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(failed_stage, "queue_paused");
        let backpressure: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM queue_backpressure WHERE document_id = ?1 AND capacity_class = 'queue_paused'",
                params![&document_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(backpressure, 1);

        let inspect = ControllerRequest {
            command: "inspect_actor".to_string(),
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
            &(serde_json::to_string(&inspect).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ControllerActorInspection> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let inspection = envelope.data.unwrap();
        assert_eq!(
            inspection
                .queue_control
                .as_ref()
                .map(|control| control.state.as_str()),
            Some("paused")
        );
        let pressure = inspection.queue_backpressure.last().unwrap();
        assert_eq!(pressure.capacity_class, "queue_paused");
        assert_eq!(pressure.command_kind, "managed_reopen");
        assert_eq!(pressure.generation, Some(1));
        assert!(pressure.dispatch_receipt_id.is_some());
        let pressure_json = serde_json::to_value(pressure).unwrap();
        assert_eq!(pressure_json["capacity_class"], "queue_paused");
        assert_eq!(pressure_json["generation"].as_u64(), pressure.generation);
        assert!(
            inspection
                .admin_operations
                .iter()
                .any(|operation| operation.operation_kind == "queue_paused"
                    && operation.status == "accepted")
        );
    }

    #[test]
    fn controller_admin_handoff_and_reap_require_observed_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/admin-control.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-admin-control\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        crate::session_actor::record_session_start_direct(
            &doc,
            "session-admin-control",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-admin-control",
            "%41",
            Some(1),
            crate::session_actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let stale_handoff = ControllerRequest {
            command: "admin_control".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: Some("%42".to_string()),
            window_id: None,
            generation: Some(0),
            state: Some("handoff".to_string()),
            caller: Some("admin".to_string()),
            reason: Some("test stale handoff".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("handoff".to_string()),
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&stale_handoff).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ControllerAdminReceipt> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let receipt = envelope.data.unwrap();
        assert_eq!(receipt.status, "rejected");
        assert_eq!(receipt.failed_stage.as_deref(), Some("stale_generation"));

        let handoff = ControllerRequest {
            generation: Some(1),
            reason: Some("test accepted handoff".to_string()),
            ..stale_handoff
        };
        let response = handle_request(
            &(serde_json::to_string(&handoff).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ControllerAdminReceipt> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        assert_eq!(envelope.data.unwrap().status, "accepted");

        let document_id =
            crate::session_actor::canonical_document_id_in(dir.path(), &doc.to_string_lossy());
        let record = load_actor_record(dir.path(), &document_id)
            .unwrap()
            .unwrap();
        assert_eq!(record.pane_id, "%42");
        assert_eq!(record.generation, 2);

        let reap = ControllerRequest {
            command: "admin_control".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: Some(2),
            state: Some("reap".to_string()),
            caller: Some("admin".to_string()),
            reason: Some("test reap".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("reap".to_string()),
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&reap).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ControllerAdminReceipt> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        assert_eq!(envelope.data.unwrap().status, "accepted");

        let record = load_actor_record(dir.path(), &document_id)
            .unwrap()
            .unwrap();
        assert_eq!(record.state, crate::session_actor::ActorState::Closed);
        assert!(record.pane_id.is_empty());
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
            crate::session_actor::ActorState::Ready,
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
            crate::session_actor::ActorState::Ready
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
    fn session_actor_closeout_persists_queue_head_cycle_and_pending_mutations() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/session-closeout.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let content = "---\nagent_doc_session: session-closeout\nagent: codex\n---\n\
agent:queue\n\
- do [#ctrlplane-sessionactor]\n";
        std::fs::write(&doc, content).unwrap();

        let state =
            crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::record_active_queue_heads(
            &doc,
            &["do [#ctrlplane-sessionactor]".to_string()],
        )
        .unwrap();
        crate::cycle_state::record_pending_done_ids(&doc, &["ctrlplane-sessionactor".to_string()])
            .unwrap();
        crate::cycle_state::record_pending_gated_ids(&doc, &["held-item".to_string()]).unwrap();
        crate::cycle_state::record_pending_kept_open_ids(&doc, &["later-item".to_string()])
            .unwrap();
        crate::cycle_state::record_reaped_pending_ids(&doc, &["stale-item".to_string()]).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(content), Some(content))
            .unwrap();

        assert!(persist_session_actor_closeout(&doc).unwrap());

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let document_id =
            crate::session_actor::canonical_document_id_in(dir.path(), &doc.to_string_lossy());
        let cycle: (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT state, queue_head_id, response_commit FROM document_cycles WHERE document_id = ?1 AND cycle_id = ?2",
                params![&document_id, &state.cycle_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(cycle.0, "committed");
        assert_eq!(cycle.1.as_deref(), Some("ctrlplane-sessionactor"));
        assert!(cycle.2.is_some());

        let queue: (Option<String>, String, String) = conn
            .query_row(
                "SELECT head_id, prompt, state FROM queue_heads WHERE document_id = ?1 AND queue_name = 'agent:queue'",
                params![&document_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(queue.0.as_deref(), Some("ctrlplane-sessionactor"));
        assert_eq!(queue.1, "do [#ctrlplane-sessionactor]");
        assert_eq!(queue.2, "consumed");

        let mutations: Vec<(String, String)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT item_id, status FROM pending_mutations WHERE document_id = ?1 AND cycle_id = ?2 ORDER BY item_id",
                )
                .unwrap();
            stmt.query_map(params![&document_id, &state.cycle_id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
        };
        assert_eq!(
            mutations,
            vec![
                ("ctrlplane-sessionactor".to_string(), "done".to_string()),
                ("held-item".to_string(), "gated".to_string()),
                ("later-item".to_string(), "kept_open".to_string()),
                ("stale-item".to_string(), "reaped".to_string()),
            ]
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
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
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
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
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
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
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
    fn controller_restart_recovery_rebuilds_memory_and_repairs_projections() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/restart.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-restart\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let mut record = actor_record(&document_id, "%88", "@8");
        record.session_id = "session-restart".to_string();
        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let conn = open_state_db(dir.path()).unwrap();
        state_store::upsert_supervisor_lease_in_db(
            &conn,
            &record,
            Some(std::process::id()),
            Some("/tmp/supervisor.sock"),
            "ready",
        )
        .unwrap();
        state_store::insert_dispatch_attempt_in_db(
            &conn,
            &state_store::DispatchAttemptInsert {
                document_id: &document_id,
                generation: record.generation,
                command_kind: "managed_reopen",
                accepted_stage: Some("ready"),
                failed_stage: None,
                diagnostic_payload: "restart recovery test",
                result_status: "accepted",
                proof_scope: "accepted_only",
                dispatch_start_proven: false,
            },
        )
        .unwrap();
        state_store::upsert_document_cycle_state_in_db(
            &conn,
            &document_id,
            "cycle-restart",
            "preflight_started",
            Some("ctrlplane-crashrecover"),
            None,
        )
        .unwrap();
        state_store::store_layout_state_in_db(
            &conn,
            DEFAULT_LAYOUT_SCOPE,
            &["tasks/restart.md".to_string()],
        )
        .unwrap();
        drop(conn);

        std::fs::write(actor_projection_path(dir.path()), "{}").unwrap();
        let _ = std::fs::remove_file(crate::sessions::registry_path_in(dir.path()));
        let _ = std::fs::remove_file(layout_projection_path(dir.path()));

        let mut bootstrap = test_bootstrap(&dir);
        bootstrap.controller_generation = 2;
        let runtime = ControllerRuntime::new(bootstrap).unwrap();

        let memory_record = runtime.actor_record(&document_id).unwrap().unwrap();
        assert_eq!(memory_record.pane_id, "%88");
        assert_eq!(memory_record.session_id, "session-restart");

        let actor_projection: BTreeMap<String, crate::session_actor::ActorRecord> =
            serde_json::from_str(
                &std::fs::read_to_string(actor_projection_path(dir.path())).unwrap(),
            )
            .unwrap();
        assert_eq!(actor_projection.get(&document_id).unwrap(), &record);

        let sessions_projection = crate::sessions::load_in(dir.path()).unwrap();
        let entry = sessions_projection.get(&document_id).unwrap();
        assert_eq!(entry.pane, "%88");
        assert_eq!(entry.window, "@8");
        assert_eq!(entry.session_id, "session-restart");

        let layout_projection: Vec<String> = serde_json::from_str(
            &std::fs::read_to_string(layout_projection_path(dir.path())).unwrap(),
        )
        .unwrap();
        assert_eq!(layout_projection, vec!["tasks/restart.md"]);

        let conn = open_state_db(dir.path()).unwrap();
        let marker_count = |kind: &str, status: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM crash_recovery_markers WHERE marker_kind = ?1 AND status = ?2",
                params![kind, status],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(marker_count("supervisor_lease_reconcile", "reattached"), 1);
        assert_eq!(marker_count("dispatch_receipt_reconcile", "retryable"), 1);
        assert_eq!(marker_count("open_closeout_preserved", "preserved"), 1);
        assert_eq!(marker_count("controller_restart_reconcile", "completed"), 1);
        let cycle_state: String = conn
            .query_row(
                "SELECT state FROM document_cycles WHERE document_id = ?1 AND cycle_id = 'cycle-restart'",
                params![document_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cycle_state, "preflight_started");
    }

    #[test]
    fn controller_session_clear_accepts_closed_actor_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/closed-clear.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-closed-clear\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        crate::session_actor::record_session_start_direct(
            &doc,
            "session-closed-clear",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-closed-clear",
            "%41",
            Some(1),
            crate::session_actor::ActorState::Closed,
            "supervisor",
            "cycle_committed",
        )
        .unwrap();

        let clear = ControllerRequest {
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
            diagnostic_payload: Some("test clear closed actor".to_string()),
        };
        let response = handle_request(
            &(serde_json::to_string(&clear).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(
            envelope.ok,
            "session_clear should accept closed actors: {:?}",
            envelope.error
        );
        assert_eq!(envelope.data.unwrap().accepted_stage, "operator_closed");

        let restart = ControllerRequest {
            command_kind: Some("session_restart".to_string()),
            ..clear
        };
        let response = handle_request(
            &(serde_json::to_string(&restart).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(!envelope.ok);
        assert!(envelope.error.unwrap().contains("generation 1 is closed"));
    }

    #[test]
    fn controller_attach_pane_creates_manual_attach_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/attach.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-attach\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        crate::session_actor::record_session_start_direct(&doc, "session-attach", "%41", "@1", 1)
            .unwrap();
        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let diagnostics_before_attach: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projection_diagnostics WHERE projection = 'sessions.json'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        let attach = ControllerRequest {
            command: "attach_pane".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-attach".to_string()),
            pane_id: Some("%42".to_string()),
            window_id: Some("@2".to_string()),
            generation: None,
            state: None,
            caller: Some("session".to_string()),
            reason: Some("manual_attach".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&attach).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let record = envelope.data.unwrap();
        assert_eq!(record.pane_id, "%42");
        assert_eq!(record.window_id, "@2");
        assert_eq!(record.generation, 2);
        assert_eq!(record.last_transition.caller, "session");
        assert_eq!(record.last_transition.reason, "manual_attach");
        let diagnostics_after_attach: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projection_diagnostics WHERE projection = 'sessions.json'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            diagnostics_after_attach, diagnostics_before_attach,
            "controller attach should not add a projection diagnostic before the caller updates sessions.json"
        );
    }

    #[test]
    fn controller_mark_lifecycle_resolves_relative_path_via_project_root() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/relative.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-relative\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let start = ControllerRequest {
            command: "start_session".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-relative".to_string()),
            pane_id: Some("%51".to_string()),
            window_id: Some("@1".to_string()),
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
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);

        let relative = std::path::PathBuf::from("tasks/relative.md");
        let lifecycle = ControllerRequest {
            command: "mark_lifecycle".to_string(),
            file: Some(relative),
            session_id: Some("session-relative".to_string()),
            pane_id: Some("%51".to_string()),
            window_id: None,
            generation: Some(1),
            state: Some("ready".to_string()),
            caller: Some("route".to_string()),
            reason: Some("dispatch_ready_prompt".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&lifecycle).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok, "mark_lifecycle with relative path failed");
        assert_eq!(
            envelope.data.unwrap().state,
            crate::session_actor::ActorState::Ready
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

    // ── Stuck-`Preparing` controller reaper (#kqr6 / #sjwm / #stuckhandoff) ──

    #[test]
    fn preparing_controller_staleness_truth_table() {
        let stale_after = Duration::from_secs(45);
        let now = 10_000u64;
        // Preparing + old handoff_started_at + no fresh lease ⇒ reap.
        assert!(preparing_controller_is_stale(
            ControllerHandoffState::Preparing,
            Some(now - 100),
            now,
            stale_after,
        ));
        // Promoted-but-never-finalized + old ⇒ reap.
        assert!(preparing_controller_is_stale(
            ControllerHandoffState::Promoted,
            Some(now - 100),
            now,
            stale_after,
        ));
        // Within threshold ⇒ keep (healthy mid-handoff).
        assert!(!preparing_controller_is_stale(
            ControllerHandoffState::Preparing,
            Some(now - 5),
            now,
            stale_after,
        ));
        // Exactly at threshold ⇒ keep (strictly greater-than is stale).
        assert!(!preparing_controller_is_stale(
            ControllerHandoffState::Preparing,
            Some(now - 45),
            now,
            stale_after,
        ));
        // Stable / Retiring / Failed ⇒ never stale even when old.
        for state in [
            ControllerHandoffState::Stable,
            ControllerHandoffState::Retiring,
            ControllerHandoffState::Failed,
        ] {
            assert!(!preparing_controller_is_stale(
                state,
                Some(now - 100),
                now,
                stale_after,
            ));
        }
        // No handoff_started_at ⇒ never stale.
        assert!(!preparing_controller_is_stale(
            ControllerHandoffState::Preparing,
            None,
            now,
            stale_after,
        ));
    }

    fn write_preparing_bootstrap(
        project_root: &Path,
        pid: u32,
        handoff_started_at: Option<u64>,
    ) -> ControllerBootstrap {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid,
            controller_binary: None,
            controller_generation: 1004,
            handoff_state: ControllerHandoffState::Preparing,
            handoff_started_at,
            previous_controller_pid: Some(1002),
        };
        write_bootstrap_state(&bootstrap).unwrap();
        bootstrap
    }

    #[test]
    fn reaper_keeps_fresh_preparing_bootstrap() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        // Fresh handoff_started_at (just now) ⇒ healthy mid-handoff, keep.
        write_preparing_bootstrap(dir.path(), std::process::id(), Some(timestamp_secs()));
        let (reaped, kept) = terminate_stale_preparing_controllers(
            dir.path(),
            Duration::from_secs(45),
            false,
        )
        .unwrap();
        assert_eq!((reaped, kept), (0, 1));
        let after = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(after.handoff_state, ControllerHandoffState::Preparing);
    }

    #[test]
    fn reaper_skips_pid_that_is_not_a_same_project_controller() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        // Our own test pid is alive but is NOT an `agent-doc controller serve`
        // process, so the cmdline gate must refuse to kill it and keep the record.
        let old = timestamp_secs() - 600;
        write_preparing_bootstrap(dir.path(), std::process::id(), Some(old));
        let (reaped, kept) = terminate_stale_preparing_controllers(
            dir.path(),
            Duration::from_secs(45),
            false,
        )
        .unwrap();
        assert_eq!((reaped, kept), (0, 1));
        let after = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(
            after.handoff_state,
            ControllerHandoffState::Preparing,
            "a non-controller pid must never be killed or marked Failed"
        );
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("stale_preparing_controller_reaped_skipped"));
        assert!(ops_log.contains("reason=not_same_project_controller"));
    }

    #[test]
    fn reaper_dry_run_reports_without_killing() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut sentinel = spawn_controller_sentinel(dir.path());
        let pid = sentinel.id();
        let old = timestamp_secs() - 600;
        write_preparing_bootstrap(dir.path(), pid, Some(old));

        let (reaped, kept) = terminate_stale_preparing_controllers(
            dir.path(),
            Duration::from_secs(45),
            true, // dry-run
        )
        .unwrap();
        assert_eq!((reaped, kept), (1, 0));
        assert!(process_is_alive(pid), "dry-run must not kill the sentinel");
        let after = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(after.handoff_state, ControllerHandoffState::Preparing);

        let _ = sentinel.kill();
        let _ = sentinel.wait();
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

        let (reaped, kept) = terminate_stale_preparing_controllers(
            dir.path(),
            Duration::from_secs(45),
            false,
        )
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
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("stale_preparing_controller_reaped pid="));
        assert!(ops_log.contains("caller=gc"));
    }

    // ---- M1 (#stuckhandoff2): controller self-watchdog ----

    fn runtime_for_bootstrap(bootstrap: ControllerBootstrap) -> ControllerRuntime {
        // Construct directly (bypassing `ControllerRuntime::new`'s restart-recovery /
        // state-DB load) so the self-watchdog predicate is exercised in isolation.
        ControllerRuntime {
            bootstrap: Mutex::new(bootstrap),
            memory: Mutex::new(ControllerMemoryState {
                actor_store: BTreeMap::new(),
                map_backend: "std_btree_map",
            }),
            recycle_requested: AtomicBool::new(false),
        }
    }

    fn preparing_runtime_bootstrap(
        project_root: &Path,
        handoff_state: ControllerHandoffState,
        handoff_started_at: Option<u64>,
    ) -> ControllerBootstrap {
        ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: None,
            controller_generation: 7,
            handoff_state,
            handoff_started_at,
            previous_controller_pid: None,
        }
    }

    #[test]
    fn self_watchdog_keeps_fresh_preparing() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let runtime = runtime_for_bootstrap(preparing_runtime_bootstrap(
            dir.path(),
            ControllerHandoffState::Preparing,
            Some(timestamp_secs()),
        ));
        assert!(
            !controller_self_watchdog_should_suicide(&runtime, Duration::from_secs(45)),
            "a controller mid-handoff (fresh handoff_started_at) must not self-terminate"
        );
    }

    #[test]
    fn self_watchdog_keeps_stable() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let runtime = runtime_for_bootstrap(preparing_runtime_bootstrap(
            dir.path(),
            ControllerHandoffState::Stable,
            None,
        ));
        assert!(
            !controller_self_watchdog_should_suicide(&runtime, Duration::from_secs(0)),
            "a Stable controller must never self-terminate, even at a zero threshold"
        );
    }

    #[test]
    fn self_watchdog_suicides_and_marks_failed_on_stale_preparing() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let stale = timestamp_secs().saturating_sub(600);
        let bootstrap = preparing_runtime_bootstrap(
            dir.path(),
            ControllerHandoffState::Preparing,
            Some(stale),
        );
        write_bootstrap_state(&bootstrap).unwrap();
        let runtime = runtime_for_bootstrap(bootstrap);

        assert!(
            controller_self_watchdog_should_suicide(&runtime, Duration::from_secs(45)),
            "a controller wedged in Preparing past the threshold must self-terminate"
        );
        controller_self_watchdog_suicide(&runtime, Duration::from_secs(45));

        // On-disk bootstrap superseded with Failed so the next bind promotes fresh.
        let after = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(after.handoff_state, ControllerHandoffState::Failed);
        assert_eq!(after.handoff_started_at, None);
        // In-memory bootstrap mirrors the transition.
        assert_eq!(
            runtime.bootstrap_snapshot().unwrap().handoff_state,
            ControllerHandoffState::Failed
        );
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("stale_preparing_controller_self_reaped pid="));
        assert!(ops_log.contains("caller=self_watchdog"));
    }

    // ---- M3 (#stuckhandoff2): orphaned-preparing process-scan reaper ----

    #[test]
    fn process_start_age_secs_reports_for_self() {
        assert!(
            process_start_age_secs(std::process::id()).is_some(),
            "process start age must resolve from /proc for a live pid"
        );
    }

    #[test]
    fn orphan_reaper_keeps_fresh_preparing_sentinel() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut sentinel = spawn_preparing_controller_sentinel(dir.path());
        let pid = sentinel.id();
        assert!(cmdline_has_preparing_handoff(pid));

        // Just-launched (age ~0s) ⇒ inside a healthy handoff window ⇒ keep.
        let (reaped, kept) =
            reap_orphaned_preparing_controllers(dir.path(), Duration::from_secs(45), false)
                .unwrap();
        assert_eq!((reaped, kept), (0, 1));
        assert!(process_is_alive(pid), "a fresh preparing sentinel must be kept");

        let _ = sentinel.kill();
        let _ = sentinel.wait();
    }

    #[test]
    fn orphan_reaper_ignores_non_preparing_controller() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut sentinel = spawn_controller_sentinel(dir.path());
        let pid = sentinel.id();
        assert!(!cmdline_has_preparing_handoff(pid));

        // No `--handoff-state preparing` ⇒ not an orphaned handoff ⇒ never scanned.
        let (reaped, kept) =
            reap_orphaned_preparing_controllers(dir.path(), Duration::from_secs(0), false)
                .unwrap();
        assert_eq!((reaped, kept), (0, 0));
        assert!(process_is_alive(pid), "a plain controller must never be reaped here");

        let _ = sentinel.kill();
        let _ = sentinel.wait();
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
            reap_orphaned_preparing_controllers(dir.path(), Duration::from_secs(0), false)
                .unwrap();
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
        assert!(!status.success(), "orphan must be signal-terminated: {status:?}");

        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("orphaned_preparing_controller_reaped pid="));
    }

    // ---- #qflood: in-flight dispatch coalescing decision ----

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
            crate::session_actor::ActorState::Busy,
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

    // ---- M2 (#stuckhandoff2): non-Stable controller refuses dispatch ----

    #[test]
    fn dispatch_refused_when_controller_not_stable() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/m2-gate.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-m2\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        crate::session_actor::record_session_start_direct(&doc, "session-m2", "%41", "@1", 1)
            .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-m2",
            "%41",
            Some(1),
            crate::session_actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let dispatch_request = || ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-m2".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("managed_reopen".to_string()),
            diagnostic_payload: Some("m2 gate test".to_string()),
        };

        // A controller wedged in Preparing (client died before promote_handoff) is
        // non-authoritative: it must refuse to admit the dispatch.
        let preparing = ControllerBootstrap {
            handoff_state: ControllerHandoffState::Preparing,
            handoff_started_at: Some(timestamp_secs()),
            ..test_bootstrap(&dir)
        };
        let err = handle_dispatch(&preparing, None, dispatch_request()).unwrap_err();
        assert!(
            format!("{err:#}").contains("controller not authoritative"),
            "a Preparing controller must refuse dispatch admission: {err:#}"
        );

        // The refusal is recorded as a rejection receipt + ops-log line for forensics.
        let conn = open_state_db(dir.path()).unwrap();
        let refused: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dispatch_attempts WHERE failed_stage = 'controller_not_authoritative'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(refused, 1, "non-Stable dispatch refusal must record a receipt");
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("dispatch_refused_non_stable_controller"));

        // The identical dispatch on a Stable controller passes the authority gate —
        // it may proceed to admit (or fail for an unrelated reason), but never for
        // non-authority.
        let stable = test_bootstrap(&dir); // handoff_state: Stable
        if let Err(err) = handle_dispatch(&stable, None, dispatch_request()) {
            assert!(
                !format!("{err:#}").contains("controller not authoritative"),
                "a Stable controller must not be refused for authority: {err:#}"
            );
        }
    }

    #[test]
    fn autonomous_idle_queue_continuation_refused_when_controller_not_stable() {
        // M2 worktree-write gate (#stuckhandoff2 / #fcc0e): the supervisor's
        // self-driving queue drain is the autonomous worktree-write driver a wedged
        // `Preparing` controller would otherwise use to corrupt the tree between
        // wedge and M1 self-reap — it issues a `dispatch` with
        // `command_kind=idle_queue_continuation` (no external client), so it is the
        // exact path the dispatch-admission gate must refuse on a non-Stable
        // controller. This proves that gate covers the AUTONOMOUS driver, not just
        // operator/route dispatches — the worktree-write protection M2 promises.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/m2-idle.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-idle\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        crate::session_actor::record_session_start_direct(&doc, "session-idle", "%61", "@1", 1)
            .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-idle",
            "%61",
            Some(1),
            crate::session_actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let idle_continuation = || ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-idle".to_string()),
            pane_id: Some("%61".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("idle_queue_continuation".to_string()),
            diagnostic_payload: Some("autonomous queue drain".to_string()),
        };

        let preparing = ControllerBootstrap {
            handoff_state: ControllerHandoffState::Preparing,
            handoff_started_at: Some(timestamp_secs()),
            ..test_bootstrap(&dir)
        };
        let err = handle_dispatch(&preparing, None, idle_continuation()).unwrap_err();
        assert!(
            format!("{err:#}").contains("controller not authoritative"),
            "a Preparing controller must refuse the autonomous idle-queue worktree-write driver: {err:#}"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("dispatch_refused_non_stable_controller"),
            "the autonomous-driver refusal must be logged for forensics:\n{ops_log}"
        );

        // A Stable controller is never refused for authority on the same driver.
        let stable = test_bootstrap(&dir);
        if let Err(err) = handle_dispatch(&stable, None, idle_continuation()) {
            assert!(
                !format!("{err:#}").contains("controller not authoritative"),
                "a Stable controller must not be refused for authority: {err:#}"
            );
        }
    }

    #[test]
    fn dispatch_refused_when_controller_binary_stale() {
        // `#ctlstalebin` (#stuckhandoff2 follow-up): a Stable controller whose own
        // recorded binary no longer matches the installed agent-doc must refuse
        // dispatch admission, so a stale (old-binary) controller cannot keep driving
        // session writes between a `cargo install` and the next handoff — the
        // operator's observed "old binary churns until manual restart" failure. The
        // refusal records a `controller_binary_stale` receipt + ops-log line so the
        // recycle backstop is provable from the logs.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/stale-bin.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-sb\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        crate::session_actor::record_session_start_direct(&doc, "session-sb", "%51", "@1", 1)
            .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-sb",
            "%51",
            Some(1),
            crate::session_actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let dispatch_request = || ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-sb".to_string()),
            pane_id: Some("%51".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("idle_queue_continuation".to_string()),
            diagnostic_payload: Some("stale binary test".to_string()),
        };

        // Stable handoff state, but the recorded binary is an old/different build.
        let stale = ControllerBootstrap {
            controller_binary: Some(ControllerBinaryIdentity {
                path: PathBuf::from("/nonexistent/old-agent-doc"),
                version: "0.0.0-stale".to_string(),
                len: 1,
                modified_secs: 1,
                modified_nanos: 0,
            }),
            ..test_bootstrap(&dir)
        };
        let err = handle_dispatch(&stale, None, dispatch_request()).unwrap_err();
        assert!(
            format!("{err:#}").contains("controller_binary_stale"),
            "a stale-binary controller must refuse dispatch admission: {err:#}"
        );

        let conn = open_state_db(dir.path()).unwrap();
        let refused: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dispatch_attempts WHERE failed_stage = 'controller_binary_stale'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            refused, 1,
            "stale-binary dispatch refusal must record a receipt"
        );
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("dispatch_refused_stale_binary"));

        // A current-binary Stable controller is never refused for staleness (it may
        // admit, or fail for an unrelated reason, but never `controller_binary_stale`).
        let current = test_bootstrap(&dir);
        if let Err(err) = handle_dispatch(&current, None, dispatch_request()) {
            assert!(
                !format!("{err:#}").contains("controller_binary_stale"),
                "a current-binary controller must not be refused for staleness: {err:#}"
            );
        }
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
    fn recycle_debounce_decision_requires_continuous_idle_grace() {
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

    // ---- M4 (#stuckhandoff2): client handoff drop-guard ----

    #[test]
    fn handoff_drop_guard_aborted_handoff_sends_shutdown_and_logs() {
        // An aborted handoff (guard dropped before `complete`) must tell the
        // half-launched replacement on the temp socket to shut down, and record it.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let temp_sock = dir.path().join(".agent-doc").join("controller-handoff.sock");

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

        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("handoff_drop_guard_aborted_handoff_shutdown"));
        assert!(ops_log.contains(&format!("temp_sock={}", temp_sock.display())));
    }

    #[test]
    fn handoff_drop_guard_completed_handoff_does_not_shut_down() {
        // The success path calls `complete()`: a promoted, now-authoritative
        // controller must never be shut down or logged as an aborted handoff.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let temp_sock = dir.path().join(".agent-doc").join("controller-handoff.sock");
        {
            let mut guard = HandoffDropGuard::new(dir.path(), &temp_sock);
            guard.complete();
            // Dropped here after `complete()` ⇒ shutdown branch must be skipped.
        }
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log"))
            .unwrap_or_default();
        assert!(
            !ops_log.contains("handoff_drop_guard_aborted_handoff_shutdown"),
            "a completed handoff must not log an aborted shutdown"
        );
    }

    // ---- M5 (#stuckhandoff2): cross-project orphaned-preparing sweep ----

    #[test]
    fn controller_serve_project_root_from_args_extracts_root_for_any_project() {
        // The cmdline shape a sentinel/real controller presents in `/proc`, for a
        // project root that is NOT the caller's — the breadth M5 adds over the
        // per-project reaper.
        let args = vec![
            "/some/bin/agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            "/home/me/work/boost-client".to_string(),
            "--handoff-state".to_string(),
            "preparing".to_string(),
        ];
        assert_eq!(
            controller_serve_project_root_from_args(&args),
            Some(PathBuf::from("/home/me/work/boost-client"))
        );
    }

    #[test]
    fn controller_serve_project_root_from_args_rejects_non_controllers() {
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
    #[ignore = "global /proc preparing-controller sweep: would reap the per-project M3 \
                sentinel tests' processes under nextest concurrency. Runs in the \
                `make check` --ignored leg, where it is the only such sweeper."]
    fn all_projects_reaper_reaps_aged_cross_project_preparing_sentinel() {
        // The all-projects API takes no project_root: it must discover this wedged
        // Preparing controller purely from `/proc` and reap it keyed to its OWN root.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut sentinel = spawn_preparing_controller_sentinel(dir.path());
        let pid = sentinel.id();
        assert!(cmdline_has_preparing_handoff(pid));
        assert_eq!(
            controller_serve_project_root(pid).as_deref(),
            Some(dir.path()),
            "the sweep must recover the sentinel's own --project-root from /proc"
        );
        // Age past a zero threshold (start age = /proc dir mtime).
        std::thread::sleep(Duration::from_millis(1100));

        let (reaped, _kept) = reap_orphaned_preparing_controllers_all_projects(
            Duration::from_secs(0),
            false,
            "test",
        )
        .unwrap();
        assert!(
            reaped >= 1,
            "cross-project sweep must reap the aged preparing sentinel"
        );

        // The live orphan must actually be terminated. The sentinel is our child, so
        // a killed process lingers as a zombie until `wait()` — poll `try_wait`.
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
        let status = exit.expect("aged cross-project preparing orphan must be reaped");
        assert!(!status.success(), "orphan must be signal-terminated: {status:?}");

        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("orphaned_preparing_controller_reaped_cross_project pid="));
        assert!(ops_log.contains("caller=test"));
    }

    /// Spawn a long-sleep sentinel whose `/proc/<pid>/cmdline` matches the
    /// `agent-doc controller serve --project-root <root>` shape
    /// `is_same_project_controller_pid` checks, without exec-collapsing the
    /// shell (the `; :` keeps `sh` resident). The trailing positional params
    /// after the `-c` script name become `$0..$N` and are ignored by `sleep`.
    fn spawn_controller_sentinel(project_root: &Path) -> std::process::Child {
        let argv0 = project_root.join("agent-doc");
        let child = Command::new("sh")
            .arg("-c")
            .arg("sleep 30; :")
            .arg(argv0.to_string_lossy().to_string())
            .arg("controller")
            .arg("serve")
            .arg("--project-root")
            .arg(project_root.to_string_lossy().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn controller sentinel");
        // `/proc/<pid>/cmdline` is not populated the instant `spawn` returns; wait
        // until the sentinel presents the matching controller cmdline so the
        // reaper's `is_same_project_controller_pid` gate sees it deterministically.
        let pid = child.id();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2)
            && !is_same_project_controller_pid(project_root, pid)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        child
    }

    /// Like [`spawn_controller_sentinel`] but the cmdline also carries
    /// `--handoff-state preparing`, mirroring a replacement controller launched
    /// mid-handoff that wedged because the client never sent `promote_handoff`.
    fn spawn_preparing_controller_sentinel(project_root: &Path) -> std::process::Child {
        let argv0 = project_root.join("agent-doc");
        let child = Command::new("sh")
            .arg("-c")
            .arg("sleep 30; :")
            .arg(argv0.to_string_lossy().to_string())
            .arg("controller")
            .arg("serve")
            .arg("--project-root")
            .arg(project_root.to_string_lossy().to_string())
            .arg("--handoff-state")
            .arg("preparing")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn preparing controller sentinel");
        let pid = child.id();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2)
            && !(is_same_project_controller_pid(project_root, pid)
                && cmdline_has_preparing_handoff(pid))
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        child
    }
