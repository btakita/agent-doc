use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use proptest::prelude::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn agent_doc() -> Command {
    cargo_bin_cmd!("agent-doc")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IpcTransport {
    Socket,
    File,
    DirectDisk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SnapshotSource {
    AckContent,
    ContentOurs,
    FileRead,
    DirectDisk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseoutHazard {
    BadAckContent,
    PromptDrift,
    PartialResponseMaterialization,
    StalePatchReplay,
    PostBlockSnapshotAbsorb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseoutTransition {
    Commit(SnapshotSource),
    Fallback(SnapshotSource),
    RejectBeforeCommit,
}

fn model_transition(transport: IpcTransport, hazard: Option<CloseoutHazard>) -> CloseoutTransition {
    match (transport, hazard) {
        (_, Some(CloseoutHazard::StalePatchReplay)) => CloseoutTransition::RejectBeforeCommit,
        (
            IpcTransport::Socket | IpcTransport::File,
            Some(CloseoutHazard::BadAckContent | CloseoutHazard::PartialResponseMaterialization),
        ) => CloseoutTransition::Fallback(SnapshotSource::ContentOurs),
        (
            IpcTransport::Socket | IpcTransport::File,
            Some(CloseoutHazard::PromptDrift | CloseoutHazard::PostBlockSnapshotAbsorb),
        ) => CloseoutTransition::Commit(SnapshotSource::ContentOurs),
        (IpcTransport::Socket | IpcTransport::File, None) => {
            CloseoutTransition::Commit(SnapshotSource::AckContent)
        }
        (IpcTransport::DirectDisk, Some(CloseoutHazard::BadAckContent)) => {
            CloseoutTransition::Commit(SnapshotSource::DirectDisk)
        }
        (IpcTransport::DirectDisk, Some(CloseoutHazard::PartialResponseMaterialization)) => {
            CloseoutTransition::Fallback(SnapshotSource::DirectDisk)
        }
        (
            IpcTransport::DirectDisk,
            Some(CloseoutHazard::PromptDrift | CloseoutHazard::PostBlockSnapshotAbsorb),
        ) => CloseoutTransition::Commit(SnapshotSource::ContentOurs),
        (IpcTransport::DirectDisk, None) => CloseoutTransition::Commit(SnapshotSource::DirectDisk),
    }
}

fn unsafe_snapshot_source(hazard: CloseoutHazard, source: SnapshotSource) -> bool {
    match hazard {
        CloseoutHazard::BadAckContent | CloseoutHazard::PartialResponseMaterialization => {
            matches!(
                source,
                SnapshotSource::AckContent | SnapshotSource::FileRead
            )
        }
        CloseoutHazard::PromptDrift | CloseoutHazard::PostBlockSnapshotAbsorb => {
            matches!(
                source,
                SnapshotSource::AckContent | SnapshotSource::FileRead
            )
        }
        CloseoutHazard::StalePatchReplay => true,
    }
}

fn doc_hash(doc: &Path) -> String {
    let canonical = doc.canonicalize().unwrap();
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    hex::encode(hasher.finalize())
}

fn session_doc_content() -> String {
    concat!(
        "---\n",
        "agent_doc_session: test-session\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "agent: codex\n",
        "model: gpt-5\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "\u{276f} Please reply\n",
        "<!-- agent:boundary:abcd1234 -->\n",
        "<!-- /agent:exchange -->\n\n",
        "###\n",
        "<!--\n",
        "-->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "<!-- /agent:backlog -->\n"
    )
    .to_string()
}

fn response_text(topic: &str) -> String {
    format!(
        "<!-- patch:exchange -->\n### Re: {topic} - gpt-5\n\nBody for {topic}.\n<!-- /patch:exchange -->\n"
    )
}

fn response_body(topic: &str) -> String {
    format!("### Re: {topic} - gpt-5\n\nBody for {topic}.\n")
}

fn setup_project(with_patches_dir: bool) -> (TempDir, PathBuf, PathBuf, String) {
    let tmp = TempDir::new().unwrap();
    let agent_doc_dir = tmp.path().join(".agent-doc");
    for subdir in [
        "ack-content",
        "claimed-patches",
        "crdt",
        "logs",
        "pre-response",
        "snapshots",
        "state/cycles",
    ] {
        fs::create_dir_all(agent_doc_dir.join(subdir)).unwrap();
    }
    if with_patches_dir {
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
    }

    let doc = tmp.path().join("session.md");
    let original = session_doc_content();
    fs::write(&doc, &original).unwrap();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();
    clear_patch_jsons(&agent_doc_dir.join("patches"));

    let baseline = tmp.path().join("baseline.md");
    fs::write(&baseline, &original).unwrap();
    (tmp, doc, baseline, original)
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
        .args(["config", "commit.gpgsign", "false"])
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

fn clear_patch_jsons(patches_dir: &Path) {
    let Ok(entries) = fs::read_dir(patches_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            fs::remove_file(path).unwrap();
        }
    }
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

fn patch_id(payload: &Value) -> Option<String> {
    payload
        .get("patch_id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn apply_patch_at_boundary(current: &str, patch: &str) -> String {
    current.replace(
        "<!-- agent:boundary:abcd1234 -->",
        &format!("{patch}<!-- agent:boundary:abcd1234 -->"),
    )
}

fn apply_payload_to_file(payload: &Value, file: &Path) -> Option<String> {
    let before = fs::read_to_string(file).ok()?;
    let patches = payload
        .get("patches")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let name = item
                        .get("component")
                        .or_else(|| item.get("name"))
                        .and_then(Value::as_str)?;
                    let content = item.get("content").and_then(Value::as_str)?;
                    Some(agent_doc::template::PatchBlock::new(name, content))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let unmatched = payload
        .get("unmatched")
        .and_then(Value::as_str)
        .unwrap_or("");
    let after = agent_doc::template::apply_patches(&before, &patches, unmatched, file).ok()?;
    fs::write(file, &after).ok()?;
    Some(after)
}

fn run_finalize_expect(root: &Path, doc: &Path, baseline: &Path, response: &str, code: i32) {
    agent_doc()
        .current_dir(root)
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
            "--stream",
        ])
        .write_stdin(response)
        .assert()
        .code(code);
}

#[test]
fn ipc_closeout_transition_table_forbids_unsafe_commits() {
    assert!(
        unsafe_snapshot_source(CloseoutHazard::BadAckContent, SnapshotSource::FileRead),
        "file-read snapshots are unsafe when ACK content is known bad"
    );

    let cases = [
        (
            IpcTransport::Socket,
            Some(CloseoutHazard::BadAckContent),
            CloseoutTransition::Fallback(SnapshotSource::ContentOurs),
        ),
        (
            IpcTransport::File,
            Some(CloseoutHazard::BadAckContent),
            CloseoutTransition::Fallback(SnapshotSource::ContentOurs),
        ),
        (
            IpcTransport::File,
            Some(CloseoutHazard::PartialResponseMaterialization),
            CloseoutTransition::Fallback(SnapshotSource::ContentOurs),
        ),
        (
            IpcTransport::Socket,
            Some(CloseoutHazard::PromptDrift),
            CloseoutTransition::Commit(SnapshotSource::ContentOurs),
        ),
        (
            IpcTransport::File,
            Some(CloseoutHazard::PostBlockSnapshotAbsorb),
            CloseoutTransition::Commit(SnapshotSource::ContentOurs),
        ),
        (
            IpcTransport::File,
            Some(CloseoutHazard::StalePatchReplay),
            CloseoutTransition::RejectBeforeCommit,
        ),
        (
            IpcTransport::DirectDisk,
            Some(CloseoutHazard::PostBlockSnapshotAbsorb),
            CloseoutTransition::Commit(SnapshotSource::ContentOurs),
        ),
        (
            IpcTransport::DirectDisk,
            None,
            CloseoutTransition::Commit(SnapshotSource::DirectDisk),
        ),
    ];

    for (transport, hazard, expected) in cases {
        let actual = model_transition(transport, hazard);
        assert_eq!(actual, expected);
        if let Some(hazard) = hazard {
            match actual {
                CloseoutTransition::Commit(source) | CloseoutTransition::Fallback(source) => {
                    assert!(
                        !unsafe_snapshot_source(hazard, source),
                        "{transport:?} {hazard:?} selected unsafe snapshot source {source:?}"
                    );
                }
                CloseoutTransition::RejectBeforeCommit => {}
            }
        }
    }
}

proptest! {
    #[test]
    fn ipc_closeout_state_model_never_commits_unsafe_snapshot_source(
        transport_index in 0usize..3,
        hazard_index in 0usize..5,
    ) {
        let transport = match transport_index {
            0 => IpcTransport::Socket,
            1 => IpcTransport::File,
            _ => IpcTransport::DirectDisk,
        };
        let hazard = match hazard_index {
            0 => CloseoutHazard::BadAckContent,
            1 => CloseoutHazard::PromptDrift,
            2 => CloseoutHazard::PartialResponseMaterialization,
            3 => CloseoutHazard::StalePatchReplay,
            _ => CloseoutHazard::PostBlockSnapshotAbsorb,
        };

        match model_transition(transport, Some(hazard)) {
            CloseoutTransition::Commit(source) | CloseoutTransition::Fallback(source) => {
                prop_assert!(
                    !unsafe_snapshot_source(hazard, source),
                    "{transport:?} {hazard:?} selected unsafe snapshot source {source:?}"
                );
            }
            CloseoutTransition::RejectBeforeCommit => {}
        }
    }
}

#[test]
fn file_ipc_bad_ack_content_falls_back_before_commit() {
    let (tmp, doc, baseline, _original) = setup_project(true);
    let root = tmp.path();
    let agent_doc_dir = root.join(".agent-doc");
    let patches_dir = agent_doc_dir.join("patches");
    let ack_dir = agent_doc_dir.join("ack-content");
    let corrupt_marker = "bad-ack-content-only";
    let corrupt_ack = format!("### Re: bad ack - gpt-5\n{corrupt_marker}\n");
    let response = response_text("bad ack");

    let watcher = std::thread::spawn(move || {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(3) {
            let Ok(entries) = fs::read_dir(&patches_dir) else {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let text = fs::read_to_string(&path).unwrap();
                let payload: Value = serde_json::from_str(&text).unwrap();
                let Some(id) = patch_id(&payload) else {
                    continue;
                };
                fs::write(ack_dir.join(format!("{id}.md")), &corrupt_ack).unwrap();
                fs::remove_file(path).unwrap();
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    });

    run_finalize_expect(root, &doc, &baseline, &response, 75);
    assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");

    let head = head_blob(root);
    assert!(head.contains(&response_body("bad ack")));
    assert!(
        !head.contains(corrupt_marker),
        "bad ack-content sidecar must not become the committed document:\n{head}"
    );
    let snapshot = fs::read_to_string(snapshot_path(root, &doc)).unwrap();
    assert!(snapshot.contains(&response_body("bad ack")));
    assert!(
        !snapshot.contains(corrupt_marker),
        "bad ack-content sidecar must not become the saved snapshot:\n{snapshot}"
    );
    let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("ipc_materialization_missing_response"),
        "bad ACK should fall back before snapshot/commit with a materialization diagnostic:\n{ops_log}"
    );
}

#[test]
fn file_ipc_partial_response_materialization_falls_back_before_commit() {
    let (tmp, doc, baseline, _original) = setup_project(true);
    let root = tmp.path();
    let agent_doc_dir = root.join(".agent-doc");
    let patches_dir = agent_doc_dir.join("patches");
    let ack_dir = agent_doc_dir.join("ack-content");
    let response = response_text("partial response");
    let partial_marker = "partial-only-response-heading";

    let doc_for_watcher = doc.clone();
    let watcher = std::thread::spawn(move || {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(3) {
            let Ok(entries) = fs::read_dir(&patches_dir) else {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let text = fs::read_to_string(&path).unwrap();
                let payload: Value = serde_json::from_str(&text).unwrap();
                let Some(id) = patch_id(&payload) else {
                    continue;
                };
                let partial = apply_patch_at_boundary(
                    &fs::read_to_string(&doc_for_watcher).unwrap(),
                    &format!("### Re: partial response - gpt-5\n{partial_marker}\n"),
                );
                fs::write(&doc_for_watcher, &partial).unwrap();
                fs::write(ack_dir.join(format!("{id}.md")), &partial).unwrap();
                fs::remove_file(path).unwrap();
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    });

    run_finalize_expect(root, &doc, &baseline, &response, 75);
    assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");

    let head = head_blob(root);
    assert!(head.contains(&response_body("partial response")));
    assert!(
        !head.contains(partial_marker),
        "partial response materialization must not reach the commit:\n{head}"
    );
    let snapshot = fs::read_to_string(snapshot_path(root, &doc)).unwrap();
    assert!(snapshot.contains(&response_body("partial response")));
    assert!(
        !snapshot.contains(partial_marker),
        "partial response materialization must not become the saved snapshot:\n{snapshot}"
    );
    let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("ipc_materialization_missing_response"),
        "partial materialization should fall back before snapshot/commit:\n{ops_log}"
    );
}

#[test]
fn socket_ipc_post_block_prompt_drift_uses_content_ours_snapshot() {
    let (tmp, doc, baseline, _original) = setup_project(true);
    let root = tmp.path();
    let agent_doc_dir = root.join(".agent-doc");
    let live_note = "Typing below exchange during closeout. #next-steps";
    let current_with_note = fs::read_to_string(&doc)
        .unwrap()
        .replace("<!--\n-->", &format!("<!--\n{live_note}\n-->"));
    fs::write(&doc, &current_with_note).unwrap();

    let seen_payload = Arc::new(Mutex::new(None::<Value>));
    let seen_for_listener = seen_payload.clone();
    let doc_for_listener = doc.clone();
    let listener_root = root.to_path_buf();
    let ack_dir = agent_doc_dir.join("ack-content");
    let server = std::thread::spawn(move || {
        agent_doc::ipc_socket::start_listener(&listener_root, move |msg| {
            let payload: Value = serde_json::from_str(msg).ok()?;
            let Some(id) = patch_id(&payload) else {
                return Some(serde_json::json!({"type": "ack"}).to_string());
            };
            let after_apply = apply_payload_to_file(&payload, &doc_for_listener).or_else(|| {
                let patch = payload
                    .get("patches")
                    .and_then(Value::as_array)
                    .and_then(|patches| patches.first())
                    .and_then(|patch| patch.get("content"))
                    .and_then(Value::as_str)?;
                let current = fs::read_to_string(&doc_for_listener).ok()?;
                let after = apply_patch_at_boundary(&current, patch);
                fs::write(&doc_for_listener, &after).ok()?;
                Some(after)
            })?;
            fs::write(ack_dir.join(format!("{id}.md")), &after_apply).ok()?;
            *seen_for_listener.lock().ok()? = Some(payload);
            Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
        })
        .ok();
    });
    for _ in 0..100 {
        if agent_doc::ipc_socket::is_listener_active(root) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        agent_doc::ipc_socket::is_listener_active(root),
        "fake socket listener did not start"
    );

    run_finalize_expect(
        root,
        &doc,
        &baseline,
        &response_text("socket prompt drift"),
        0,
    );

    let _ = fs::remove_file(agent_doc::ipc_socket::socket_path(root));
    drop(server);

    let payload = seen_payload
        .lock()
        .unwrap()
        .clone()
        .expect("socket listener should capture IPC payload");
    assert!(
        payload.get("fullContent").is_none(),
        "socket closeout must stay component-scoped during live prompt drift: {payload}"
    );

    let head = head_blob(root);
    assert!(head.contains(&response_body("socket prompt drift")));
    assert!(
        !head.contains(live_note),
        "post-block prompt drift must not be absorbed into the commit:\n{head}"
    );
    let visible = fs::read_to_string(&doc).unwrap();
    assert!(
        visible.contains(live_note),
        "post-block prompt drift should remain visible for the next cycle:\n{visible}"
    );
    let snapshot = fs::read_to_string(snapshot_path(root, &doc)).unwrap();
    assert!(snapshot.contains(&response_body("socket prompt drift")));
    assert!(
        !snapshot.contains(live_note),
        "snapshot must stay on content_ours when socket ACK includes live prompt drift:\n{snapshot}"
    );
    let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("ipc_snapshot_adoption_blocked")
            && ops_log.contains("reason=live_prompt_drift_after_preflight"),
        "socket ACK prompt drift should block snapshot adoption:\n{ops_log}"
    );
}

#[test]
fn file_ipc_timeout_claims_stale_patch_before_commit() {
    let (tmp, doc, baseline, _original) = setup_project(true);
    let root = tmp.path();

    agent_doc()
        .current_dir(root)
        .args([
            "write",
            doc.to_str().unwrap(),
            "--stream",
            "--commit",
            "--baseline-file",
            baseline.to_str().unwrap(),
        ])
        .write_stdin(response_text("stale patch replay"))
        .assert()
        .code(75);

    let head = head_blob(root);
    assert!(head.contains(&response_body("stale patch replay")));

    let patch_jsons = fs::read_dir(root.join(".agent-doc/patches"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    assert!(
        patch_jsons.is_empty(),
        "timeout fallback must remove stale file IPC payloads before they can replay"
    );

    let claimed_entries = fs::read_dir(root.join(".agent-doc/claimed-patches"))
        .unwrap()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    assert!(
        !claimed_entries.is_empty(),
        "timeout fallback should claim removed file IPC payloads before commit closeout"
    );
}
