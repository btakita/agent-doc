use agent_doc_hash::content_hash;
use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
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
fn finalize_file_ipc_commits_ack_proven_live_queue_drift() {
    let tmp = TempDir::new().unwrap();
    let agent_doc_dir = tmp.path().join(".agent-doc");
    for subdir in [
        "ack-content",
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
    record_operator_buffer(&doc, &current_with_queue);

    let seen_payload = Arc::new(Mutex::new(None::<Value>));
    let patches_dir = agent_doc_dir.join("patches");
    let ack_dir = agent_doc_dir.join("ack-content");
    let doc_for_watcher = doc.clone();
    let seen_for_watcher = seen_payload.clone();
    let watcher = std::thread::spawn(move || {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(3) {
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
                let Some(patch_content) = payload
                    .get("patches")
                    .and_then(Value::as_array)
                    .and_then(|patches| patches.first())
                    .and_then(|patch| patch.get("content"))
                    .and_then(Value::as_str)
                else {
                    fs::remove_file(path).unwrap();
                    continue;
                };
                let current = fs::read_to_string(&doc_for_watcher).unwrap();
                let after_plugin_apply = current.replace(
                    "<!-- agent:boundary:abcd1234 -->",
                    &format!("{patch_content}<!-- agent:boundary:abcd1234 -->"),
                );
                fs::write(&doc_for_watcher, &after_plugin_apply).unwrap();
                record_operator_buffer(&doc_for_watcher, &after_plugin_apply);
                fs::write(ack_dir.join(format!("{patch_id}.md")), after_plugin_apply).unwrap();
                *seen_for_watcher.lock().unwrap() = Some(payload);
                fs::remove_file(path).unwrap();
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    });

    let response = "<!-- patch:exchange -->\n### Re: live queue IPC race — gpt-5\nChanged paths: src/agent-doc/specs/07-closeout-commands.md, src/agent-doc/tests/live_ipc_race_integration.rs.\nCommands: cargo test finalize_file_ipc_commits_ack_proven_live_queue_drift.\nVerification: passed.\nCommit: deferred to the test harness.\nPush: deferred to the test harness.\nConfidence: high.\n<!-- /patch:exchange -->\n";

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
            "--stream",
        ])
        .write_stdin(response)
        .assert()
        .success();
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
    assert!(head.contains("### Re: live queue IPC race — gpt-5"));
    assert_eq!(
        head.matches(live_queue_prompt).count(),
        1,
        "ACK-proven live queue prompt should be committed exactly once:\n{head}"
    );

    let snapshot = fs::read_to_string(snapshot_path(tmp.path(), &doc)).unwrap();
    assert!(snapshot.contains("### Re: live queue IPC race — gpt-5"));
    assert_eq!(
        snapshot.matches(live_queue_prompt).count(),
        1,
        "ACK-proven snapshot should preserve the live queue prompt exactly once:\n{snapshot}"
    );

    let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("ipc_snapshot_adoption_blocked")
            && ops_log.contains("reason=live_prompt_drift_after_preflight"),
        "IPC snapshot adoption should explicitly block queue drift absorption:\n{ops_log}"
    );
    assert!(
        !ops_log.contains("snapshot_absorb"),
        "commit staging must not silently absorb the live queue prompt after IPC blocked it:\n{ops_log}"
    );
}

#[test]
fn finalize_commits_ack_proven_cycle_1779845677327_scratch_directives() {
    let tmp = TempDir::new().unwrap();
    let agent_doc_dir = tmp.path().join(".agent-doc");
    for subdir in [
        "ack-content",
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
    record_operator_buffer(&doc, &current_with_scratch);

    let seen_payload = Arc::new(Mutex::new(None::<Value>));
    let patches_dir = agent_doc_dir.join("patches");
    let ack_dir = agent_doc_dir.join("ack-content");
    let doc_for_watcher = doc.clone();
    let seen_for_watcher = seen_payload.clone();
    let watcher = std::thread::spawn(move || {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(3) {
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
                let Some(patch_content) = payload
                    .get("patches")
                    .and_then(Value::as_array)
                    .and_then(|patches| patches.first())
                    .and_then(|patch| patch.get("content"))
                    .and_then(Value::as_str)
                else {
                    fs::remove_file(path).unwrap();
                    continue;
                };
                let current = fs::read_to_string(&doc_for_watcher).unwrap();
                let after_plugin_apply = current.replace(
                    "<!-- agent:boundary:17798456:cycle1779 -->",
                    &format!("{patch_content}<!-- agent:boundary:17798456:cycle1779 -->"),
                );
                fs::write(&doc_for_watcher, &after_plugin_apply).unwrap();
                record_operator_buffer(&doc_for_watcher, &after_plugin_apply);
                fs::write(ack_dir.join(format!("{patch_id}.md")), after_plugin_apply).unwrap();
                *seen_for_watcher.lock().unwrap() = Some(payload);
                fs::remove_file(path).unwrap();
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    });

    let response = "<!-- patch:exchange -->\n### Re: cycle 1779845677327 IPC race — gpt-5\nChanged paths: src/agent-doc/tests/live_ipc_race_integration.rs.\nCommands: cargo test finalize_commits_ack_proven_cycle_1779845677327_scratch_directives.\nVerification: passed.\nCommit: deferred to the test harness.\nPush: deferred to the test harness.\nConfidence: high.\n<!-- /patch:exchange -->\n";

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
            "--stream",
        ])
        .write_stdin(response)
        .assert()
        .success();
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
    assert!(head.contains("### Re: cycle 1779845677327 IPC race — gpt-5"));
    assert_eq!(
        head.matches(scratch_prompt).count(),
        1,
        "ACK-proven scratch prompt should be committed exactly once:\n{head}"
    );

    let snapshot = fs::read_to_string(snapshot_path(tmp.path(), &doc)).unwrap();
    assert!(snapshot.contains("### Re: cycle 1779845677327 IPC race — gpt-5"));
    assert_eq!(
        snapshot.matches(scratch_prompt).count(),
        1,
        "ACK-proven snapshot should preserve scratch prompt text exactly once:\n{snapshot}"
    );

    let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("ipc_snapshot_adoption_blocked")
            && ops_log.contains("reason=live_prompt_drift_after_preflight"),
        "IPC snapshot adoption should explicitly record live prompt drift:\n{ops_log}"
    );
    assert!(
        !ops_log.contains("snapshot_absorb"),
        "commit staging must not silently absorb the live scratch comment after IPC blocked it:\n{ops_log}"
    );
}
