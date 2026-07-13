use agent_doc_hash::content_hash;
use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;

const TEST_EDITOR_ID: &str = "jetbrains-test-editor";

fn agent_doc() -> Command {
    cargo_bin_cmd!("agent-doc")
}

fn doc_hash(doc: &Path) -> String {
    let canonical = doc.canonicalize().unwrap();
    content_hash(canonical.to_string_lossy().as_ref())
}

fn record_operator_buffer(file: &Path, content: &str) {
    let file_key = file.to_string_lossy();
    agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
        file_key.as_ref(),
        content,
        TEST_EDITOR_ID,
        "jetbrains",
        "test",
        &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
    )
    .unwrap();
}

fn record_visible_write_receipt(
    file: &Path,
    replica: &ProjectControllerReplica,
    patch_id: &str,
    content: &str,
) {
    let file_key = file.to_string_lossy();
    let _ = agent_doc_debounce::record_live_buffer_synced_content_for_editor_with_capabilities(
        file_key.as_ref(),
        content,
        TEST_EDITOR_ID,
        "jetbrains",
        "test",
        &[
            agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
            agent_doc_debounce::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY,
        ],
    );
    publish_editor_text_via_project_controller(file, replica, content);
    let _ =
        agent_doc_controller_io::project_controller::record_visible_write_commit_candidate_for_file(
            file,
            patch_id,
            content,
            "test_live_ipc_race",
        );
}

fn seed_reliable_sync_open(doc: &Path, tag: &str) {
    let project_root = agent_doc_fs::find_project_root(doc).expect("test project root");
    let document_hash = agent_doc_hash::document_id_for_path(doc);
    let ops = vec![agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
        document_hash: document_hash.clone(),
        pid: std::process::id().into(),
        tag: tag.to_string(),
    }];
    agent_doc_sqlite::reliable_sync_inbox::record_remote_frame(
        &project_root
            .join(".agent-doc")
            .join("reliable_sync_outbox.db"),
        &document_hash,
        1,
        Some(&serde_json::to_string(&ops).unwrap()),
    )
    .expect("seed durable reliable-sync Open fact");
}

fn seed_legacy_editor_endpoint(doc: &Path, editor_id: &str) {
    assert!(
        agent_doc_plugin_owner::try_acquire_plugin_owner(
            doc.to_str().unwrap(),
            editor_id,
            std::process::id(),
        ),
        "test setup should acquire a live targeted editor endpoint"
    );
}

fn start_project_controller(root: &Path) -> Child {
    let mut controller = ProcessCommand::new(env!("CARGO_BIN_EXE_agent-doc"))
        .args([
            "controller",
            "serve",
            "--project-root",
            root.to_str().unwrap(),
            "--launch-mode",
            "managed",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start project controller subprocess");
    for _ in 0..100 {
        if agent_doc_controller_io::project_controller::status(root)
            .is_ok_and(|status| status.active)
        {
            return controller;
        }
        if controller.try_wait().ok().flatten().is_some() {
            panic!("project controller exited before becoming active");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = controller.kill();
    let _ = controller.wait();
    panic!("project controller did not start");
}

fn stop_project_controller(root: &Path, controller: &mut Child) {
    let shutdown = agent_doc_controller_io::project_controller::run_shutdown(Some(root));
    for _ in 0..100 {
        if controller.try_wait().ok().flatten().is_some() {
            assert!(
                shutdown.is_ok(),
                "project controller shutdown failed: {shutdown:?}"
            );
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = controller.kill();
    let _ = controller.wait();
    assert!(
        shutdown.is_ok(),
        "project controller shutdown failed: {shutdown:?}"
    );
}

struct ProjectControllerReplica {
    project_root: PathBuf,
    identity: String,
    client_id: u64,
    bootstrap: Vec<u8>,
}

fn register_editor_replica_via_project_controller(doc: &Path) -> ProjectControllerReplica {
    let project_root = agent_doc_fs::find_project_root(doc).expect("test project root");
    let identity = format!("{TEST_EDITOR_ID}:{}", doc.display());
    let registered = agent_doc_controller_io::project_controller::request_crdt_replica_for_test(
        &project_root,
        doc,
        serde_json::json!({
            "method": "replica_register",
            "identity": identity,
            "source": "live_ipc_race_integration_test"
        }),
    )
    .expect("register test editor through project controller");
    let client_id = registered
        .get("client_id")
        .and_then(Value::as_u64)
        .expect("project controller replica registration should return client_id");
    let bootstrap = BASE64_STANDARD
        .decode(
            registered
                .get("bootstrap_b64")
                .and_then(Value::as_str)
                .expect("project controller replica registration should return bootstrap_b64"),
        )
        .expect("decode project controller replica bootstrap");
    ProjectControllerReplica {
        project_root,
        identity,
        client_id,
        bootstrap,
    }
}

fn publish_editor_text_via_project_controller(
    doc: &Path,
    registered: &ProjectControllerReplica,
    content: &str,
) {
    let replica = agent_doc_merge::crdt_sync::ReplicaState::from_encoded(
        registered.client_id,
        &registered.bootstrap,
    )
    .expect("decode test editor relay bootstrap");
    let replica_text = replica.text();
    replica.apply_local_edit(0, replica_text.len() as u32, content);
    let updated = agent_doc_controller_io::project_controller::request_crdt_replica_for_test(
        &registered.project_root,
        doc,
        serde_json::json!({
            "method": "replica_update",
            "identity": registered.identity,
            "source": "live_ipc_race_integration_test",
            "update_b64": BASE64_STANDARD.encode(replica.encode_state())
        }),
    )
    .expect("publish test editor update through project controller");
    assert!(
        updated
            .get("canonical_len")
            .and_then(Value::as_u64)
            .is_some(),
        "project controller should accept test editor update: {updated}"
    );
}

fn apply_first_component_patch(current: &str, payload: &Value) -> Option<String> {
    let patch = payload
        .get("patches")
        .and_then(Value::as_array)
        .and_then(|patches| patches.first())?;
    let name = patch.get("component")?.as_str()?;
    let replacement = patch.get("content")?.as_str()?;
    let components = agent_doc_element::element::parse(current).ok()?;
    let target = components.iter().find(|component| component.name == name)?;
    Some(target.replace_content(current, replacement))
}

fn session_stream_doc_content() -> String {
    "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- agent:boundary:abcd1234 -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n".to_string()
}

fn cycle_1779845677327_doc_content() -> String {
    "---\nagent_doc_session: cycle-1779845677327\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\npending_done_guard: off\nprompt_presets:\n  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push\n---\n\n<!-- agent:exchange -->\n❯ do [#liveipcrace]\n<!-- agent:boundary:17798456:cycle1779 -->\n<!-- /agent:exchange -->\n\n###\n\n<!--\n-->\n\n<!-- agent:queue auto -->\ndispatch #spec-test-build-install-commit-push\n- do [#liveipcrace]\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n- [ ] [#liveipcrace] Reproduce and fix the live post-exchange typing/full-document IPC corruption path.\n<!-- /agent:backlog -->\n".to_string()
}

fn init_git_repo(root: &Path, tracked: &Path) {
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["init"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["config", "user.email", "test@example.com"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["config", "user.name", "Test User"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["add", tracked.file_name().unwrap().to_str().unwrap()])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["commit", "-m", "initial", "--no-verify"])
        .status()
        .unwrap();
}

fn write_baseline(root: &Path, content: &str) -> PathBuf {
    let baseline = root.join("baseline.md");
    fs::write(&baseline, content).unwrap();
    baseline
}

fn head_blob(root: &Path) -> String {
    let output = ProcessCommand::new("git")
        .current_dir(root)
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn snapshot_path(root: &Path, doc: &Path) -> PathBuf {
    root.join(".agent-doc/snapshots")
        .join(format!("{}.md", doc_hash(doc)))
}

#[test]
fn finalize_file_ipc_commits_response_without_absorbing_visible_write_live_queue_drift() {
    let tmp = TempDir::new().unwrap();
    let agent_doc_dir = tmp.path().join(".agent-doc");
    for subdir in [
        "live-buffer",
        "crdt",
        "logs",
        "patches",
        "pre-response",
        "snapshots",
        "state/cycles",
    ] {
        fs::create_dir_all(agent_doc_dir.join(subdir)).unwrap();
    }
    let doc = tmp.path().join("session.md");
    let original = session_stream_doc_content();
    fs::write(&doc, &original).unwrap();
    init_git_repo(tmp.path(), &doc);
    let baseline = write_baseline(tmp.path(), &original);

    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();
    for entry in fs::read_dir(agent_doc_dir.join("patches"))
        .unwrap()
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            fs::remove_file(path).unwrap();
        }
    }

    let live_queue_prompt = "- do #liveipcrace. #spec-test-build-install-commit-push";
    let current_with_queue = fs::read_to_string(&doc).unwrap().replace(
        "<!-- agent:queue -->\n<!-- /agent:queue -->",
        &format!("<!-- agent:queue -->\n{live_queue_prompt}\n<!-- /agent:queue -->"),
    );
    fs::write(&doc, &current_with_queue).unwrap();
    seed_reliable_sync_open(&doc, TEST_EDITOR_ID);
    seed_legacy_editor_endpoint(&doc, TEST_EDITOR_ID);
    record_operator_buffer(&doc, &current_with_queue);
    let mut controller = start_project_controller(tmp.path());
    let controller_replica = register_editor_replica_via_project_controller(&doc);

    let seen_payload = Arc::new(Mutex::new(None::<Value>));
    let patches_dir = agent_doc_dir.join("patches");
    let doc_for_watcher = doc.clone();
    let seen_for_watcher = seen_payload.clone();
    let watcher = std::thread::spawn(move || {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
            let entries = match fs::read_dir(&patches_dir) {
                Ok(entries) => entries,
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let text = match fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                };
                let payload: Value = match serde_json::from_str(&text) {
                    Ok(payload) => payload,
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                };
                let patch_id = payload
                    .get("patch_id")
                    .and_then(Value::as_str)
                    .unwrap()
                    .to_string();
                let current = fs::read_to_string(&doc_for_watcher).unwrap();
                let Some(after_plugin_apply) = apply_first_component_patch(&current, &payload)
                else {
                    fs::remove_file(path).unwrap();
                    continue;
                };
                fs::write(&doc_for_watcher, &after_plugin_apply).unwrap();
                record_visible_write_receipt(
                    &doc_for_watcher,
                    &controller_replica,
                    &patch_id,
                    &after_plugin_apply,
                );
                *seen_for_watcher.lock().unwrap() = Some(payload);
                fs::remove_file(path).unwrap();
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    });

    let response = "<!-- patch:exchange -->\n### Re: live queue IPC race — gpt-5\nChanged paths: src/agent-doc/specs/07-closeout-commands.md, src/agent-doc/tests/live_ipc_race_integration.rs.\nCommands: cargo test finalize_file_ipc_commits_response_without_absorbing_visible_write_live_queue_drift.\nVerification: passed.\nCommit: deferred to the test harness.\nPush: deferred to the test harness.\nConfidence: high.\n<!-- /patch:exchange -->\n";

    let output = agent_doc()
        .current_dir(tmp.path())
        .env("AGENT_DOC_FILE_IPC_TIMEOUT_MS", "20000")
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
            "--stream",
        ])
        .write_stdin(response)
        .output()
        .unwrap();
    stop_project_controller(tmp.path(), &mut controller);
    assert!(output.status.success(), "finalize failed: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("editor IPC repair did not prove visible state"),
        "finalize should not require repair when the lazily receipt proves response delivery:\n{stderr}"
    );
    assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");

    let payload = seen_payload
        .lock()
        .unwrap()
        .clone()
        .expect("watcher should capture the IPC payload");
    assert!(
        payload.get("fullContent").is_none(),
        "template response IPC must stay component-scoped while the queue is being edited: {payload}"
    );

    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        content.matches(live_queue_prompt).count(),
        1,
        "live queue prompt should remain visible exactly once for the next cycle:\n{content}"
    );
    assert_eq!(
        content
            .matches("### Re: live queue IPC race — gpt-5")
            .count(),
        1,
        "IPC closeout must not duplicate the response heading:\n{content}"
    );

    let head = head_blob(tmp.path());
    assert_eq!(
        head.matches("### Re: live queue IPC race — gpt-5").count(),
        1,
        "closeout must commit the response exactly once:\n{head}"
    );
    assert_eq!(
        head.matches(live_queue_prompt).count(),
        1,
        "content_ours closeout must preserve the live queue prompt exactly once:\n{head}"
    );

    let snapshot = fs::read_to_string(snapshot_path(tmp.path(), &doc)).unwrap();
    assert_eq!(
        snapshot
            .matches("### Re: live queue IPC race — gpt-5")
            .count(),
        1,
        "snapshot must save the response exactly once:\n{snapshot}"
    );
    assert_eq!(
        snapshot.matches(live_queue_prompt).count(),
        1,
        "content_ours snapshot must preserve the live queue prompt exactly once:\n{snapshot}"
    );

    let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("live_prompt_drift_semantic_merged")
            || ops_log.contains("live_prompt_drift_forward_merged")
            || ops_log.contains("live_prompt_drift_agent_target_not_snapshot_authority")
            || ops_log.contains("live_prompt_drift_visible_write_component_reconciled")
            || ops_log.contains("live_prompt_drift_visible_write_reconciled_merge")
            || (ops_log.contains("out_of_band_write")
                && ops_log.contains("write_authority action=routed transport=write_queue")),
        "IPC snapshot adoption should log visible-write reconciliation:\n{ops_log}"
    );
    assert!(
        !ops_log.contains("snapshot_absorb"),
        "commit staging must not silently absorb the live queue prompt outside visible-write proof:\n{ops_log}"
    );
}

#[test]
fn finalize_commits_response_with_visible_write_cycle_1779845677327_scratch_directives() {
    let tmp = TempDir::new().unwrap();
    let agent_doc_dir = tmp.path().join(".agent-doc");
    for subdir in [
        "live-buffer",
        "crdt",
        "logs",
        "patches",
        "pre-response",
        "snapshots",
        "state/cycles",
    ] {
        fs::create_dir_all(agent_doc_dir.join(subdir)).unwrap();
    }
    let doc = tmp.path().join("session.md");
    let original = cycle_1779845677327_doc_content();
    fs::write(&doc, &original).unwrap();
    init_git_repo(tmp.path(), &doc);
    let baseline = write_baseline(tmp.path(), &original);

    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();
    for entry in fs::read_dir(agent_doc_dir.join("patches"))
        .unwrap()
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            fs::remove_file(path).unwrap();
        }
    }

    let scratch_prompt = "The duplicate content corrupting document and duplicate prompt issues happened yet again. Reproduce bugs with tests first that fail and fix the implementation.";
    let scratch_directive = "#spec-test-build-install-commit-push";
    let scratch_dispatch = "dispatch #spec-test-build-install-commit-push";
    let scratch_comment =
        format!("<!--\n{scratch_prompt}\n{scratch_directive}\n---\n{scratch_dispatch}\n-->");
    let current_with_scratch = fs::read_to_string(&doc)
        .unwrap()
        .replace("<!--\n-->", &scratch_comment);
    fs::write(&doc, &current_with_scratch).unwrap();
    seed_reliable_sync_open(&doc, TEST_EDITOR_ID);
    seed_legacy_editor_endpoint(&doc, TEST_EDITOR_ID);
    record_operator_buffer(&doc, &current_with_scratch);
    let mut controller = start_project_controller(tmp.path());
    let controller_replica = register_editor_replica_via_project_controller(&doc);

    let seen_payload = Arc::new(Mutex::new(None::<Value>));
    let patches_dir = agent_doc_dir.join("patches");
    let doc_for_watcher = doc.clone();
    let seen_for_watcher = seen_payload.clone();
    let watcher = std::thread::spawn(move || {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
            let entries = match fs::read_dir(&patches_dir) {
                Ok(entries) => entries,
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let text = match fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                };
                let payload: Value = match serde_json::from_str(&text) {
                    Ok(payload) => payload,
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                };
                let patch_id = payload
                    .get("patch_id")
                    .and_then(Value::as_str)
                    .unwrap()
                    .to_string();
                let current = fs::read_to_string(&doc_for_watcher).unwrap();
                let Some(after_plugin_apply) = apply_first_component_patch(&current, &payload)
                else {
                    fs::remove_file(path).unwrap();
                    continue;
                };
                fs::write(&doc_for_watcher, &after_plugin_apply).unwrap();
                record_visible_write_receipt(
                    &doc_for_watcher,
                    &controller_replica,
                    &patch_id,
                    &after_plugin_apply,
                );
                *seen_for_watcher.lock().unwrap() = Some(payload);
                fs::remove_file(path).unwrap();
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    });

    let response = "<!-- patch:exchange -->\n### Re: cycle 1779845677327 IPC race — gpt-5\nChanged paths: src/agent-doc/tests/live_ipc_race_integration.rs.\nCommands: cargo test finalize_commits_response_with_visible_write_cycle_1779845677327_scratch_directives.\nVerification: passed.\nCommit: deferred to the test harness.\nPush: deferred to the test harness.\nConfidence: high.\n<!-- /patch:exchange -->\n";

    let output = agent_doc()
        .current_dir(tmp.path())
        .env("AGENT_DOC_FILE_IPC_TIMEOUT_MS", "20000")
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
            "--stream",
        ])
        .write_stdin(response)
        .output()
        .unwrap();
    stop_project_controller(tmp.path(), &mut controller);
    assert!(output.status.success(), "finalize failed: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("editor IPC repair did not prove visible state"),
        "finalize should not require repair when the lazily receipt proves response delivery:\n{stderr}"
    );
    assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");

    let payload = seen_payload
        .lock()
        .unwrap()
        .clone()
        .expect("watcher should capture the IPC payload");
    assert!(
        payload.get("fullContent").is_none(),
        "cycle 1779845677327 response IPC must stay component-scoped while scratch directives are being edited: {payload}"
    );

    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        content.matches(scratch_prompt).count(),
        1,
        "live scratch prompt should remain visible exactly once:\n{content}"
    );
    assert_eq!(
        content.matches(scratch_dispatch).count(),
        2,
        "queue dispatch plus live scratch dispatch should both survive without duplicate loss:\n{content}"
    );
    assert_eq!(
        content.matches(&scratch_comment).count(),
        1,
        "the live scratch comment should remain intact exactly once, including the prompt preset directive:\n{content}"
    );
    assert_eq!(
        content
            .matches("### Re: cycle 1779845677327 IPC race — gpt-5")
            .count(),
        1,
        "IPC closeout must not duplicate the response heading:\n{content}"
    );

    let head = head_blob(tmp.path());
    assert_eq!(
        head.matches("### Re: cycle 1779845677327 IPC race — gpt-5")
            .count(),
        1,
        "closeout must commit the response exactly once:\n{head}"
    );
    assert_eq!(
        head.matches(scratch_prompt).count(),
        1,
        "response-bearing visible-write closeout must preserve post-exchange scratch prompt in HEAD:\n{head}"
    );

    let snapshot = fs::read_to_string(snapshot_path(tmp.path(), &doc)).unwrap();
    assert_eq!(
        snapshot
            .matches("### Re: cycle 1779845677327 IPC race — gpt-5")
            .count(),
        1,
        "snapshot must save the response exactly once:\n{snapshot}"
    );
    assert_eq!(
        snapshot.matches(scratch_prompt).count(),
        1,
        "response-bearing visible-write snapshot must preserve post-exchange scratch prompt drift:\n{snapshot}"
    );

    let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("live_prompt_drift_semantic_merged")
            || ops_log.contains("live_prompt_drift_forward_merged")
            || ops_log.contains("live_prompt_drift_agent_target_not_snapshot_authority")
            || ops_log.contains("live_prompt_drift_visible_write_component_reconciled")
            || ops_log.contains("live_prompt_drift_visible_write_reconciled_merge")
            || ops_log.contains("live_prompt_drift_visible_write_authority_preserved"),
        "IPC snapshot adoption should log visible-write reconciliation:\n{ops_log}"
    );
    assert!(
        !ops_log.contains("snapshot_absorb"),
        "commit staging must not silently absorb the live scratch comment outside visible-write proof:\n{ops_log}"
    );
}
