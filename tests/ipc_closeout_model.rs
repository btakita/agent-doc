use agent_doc_hash::content_hash;
use agent_doc_turn::closeout_recovery::{
    CloseoutRecoveryDecision, CloseoutRecoveryDecisionInput, CloseoutRecoveryState,
    closeout_recovery_decision_from_state,
};
use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use proptest::prelude::*;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;

const TEST_EDITOR_ID: &str = "jetbrains-test-editor";
// The watcher starts before the `agent-doc finalize` subprocess reaches the
// file-IPC write. Under nextest load the subprocess can spend more than 10s in
// setup/preflight before the patch exists, even though delivery is immediate
// once the patch is written. Keep this below nextest's 60s slow-test period.
const FILE_IPC_WATCHER_TIMEOUT: Duration = Duration::from_secs(45);

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
        ) => CloseoutTransition::RejectBeforeCommit,
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

fn expected_closeout_recovery_decision(
    state: CloseoutRecoveryState,
    prompt_context_available: bool,
    proof_available: bool,
) -> &'static str {
    if prompt_context_available {
        return "queue_prompt_for_after_closeout";
    }
    if state == CloseoutRecoveryState::Clean {
        return "already_committed";
    }
    if proof_available
        && matches!(
            state,
            CloseoutRecoveryState::MissingResponseBody
                | CloseoutRecoveryState::UnsafeUserContentDrift
        )
    {
        return "retire_stale_capture";
    }
    match state {
        CloseoutRecoveryState::Clean => "already_committed",
        CloseoutRecoveryState::DirectResponsePatchback
        | CloseoutRecoveryState::BoundaryOnlyDrift
        | CloseoutRecoveryState::NestedParentPointerStale
        | CloseoutRecoveryState::OpenEmptyPreflight
        | CloseoutRecoveryState::QueueMetadataDrift => "replay_safe",
        CloseoutRecoveryState::SidecarVisibleDrift => "reset_sidecars_from_visible",
        CloseoutRecoveryState::OpenCycle
        | CloseoutRecoveryState::MissingResponseBody
        | CloseoutRecoveryState::EscapedTemplatePatch
        | CloseoutRecoveryState::UnsafeUserContentDrift => "blocked",
    }
}

fn doc_hash(doc: &Path) -> String {
    let canonical = doc.canonicalize().unwrap();
    content_hash(canonical.to_string_lossy().as_ref())
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
                    Some(agent_doc_template::PatchBlock::new(name, content))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let unmatched = payload
        .get("unmatched")
        .and_then(Value::as_str)
        .unwrap_or("");
    let after = agent_doc_template_io::apply_patches(&before, &patches, unmatched, file).ok()?;
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
            CloseoutTransition::RejectBeforeCommit,
        ),
        (
            IpcTransport::File,
            Some(CloseoutHazard::BadAckContent),
            CloseoutTransition::RejectBeforeCommit,
        ),
        (
            IpcTransport::File,
            Some(CloseoutHazard::PartialResponseMaterialization),
            CloseoutTransition::RejectBeforeCommit,
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
    fn closeout_recovery_decision_table_preserves_priority_and_source_state(
        state_index in 0usize..CloseoutRecoveryState::ALL.len(),
        prompt_context_available in any::<bool>(),
        proof_index in 0usize..3,
    ) {
        let state = CloseoutRecoveryState::ALL[state_index];
        let blocker_reason = (proof_index == 2).then_some("route blocked by open closeout");
        let proof = (proof_index > 0).then_some("visible response superseded capture");
        let decision = closeout_recovery_decision_from_state(
            state,
            CloseoutRecoveryDecisionInput {
                prompt_context_available,
                blocker_reason,
                stale_capture_supersession_proof: proof,
            },
            Some("agent-doc recover session.md"),
        );
        let expected = expected_closeout_recovery_decision(
            state,
            prompt_context_available,
            proof.is_some(),
        );

        prop_assert_eq!(
            decision.as_str(),
            expected,
            "state={:?} prompt_context_available={} proof={:?}",
            state,
            prompt_context_available,
            proof
        );
        prop_assert_eq!(
            decision.state(),
            if expected == "already_committed" { None } else { Some(state) },
            "decision must preserve the source recovery state unless no recovery remains"
        );

        match &decision {
            CloseoutRecoveryDecision::AlreadyCommitted => {
                prop_assert!(!prompt_context_available);
                prop_assert_eq!(state, CloseoutRecoveryState::Clean);
            }
            CloseoutRecoveryDecision::ReplaySafe { command, .. }
            | CloseoutRecoveryDecision::ResetSidecarsFromVisible { command, .. } => {
                prop_assert!(command.contains("session.md"));
            }
            CloseoutRecoveryDecision::RetireStaleCapture { proof: actual, .. } => {
                prop_assert_eq!(Some(actual.as_str()), proof);
            }
            CloseoutRecoveryDecision::QueuePromptForAfterCloseout { reason, .. } => {
                prop_assert!(prompt_context_available);
                prop_assert_eq!(reason.as_str(), blocker_reason.unwrap_or(state.as_str()));
            }
            CloseoutRecoveryDecision::Blocked {
                missing_proof,
                recommended,
                ..
            } => {
                prop_assert!(!missing_proof.is_empty());
                prop_assert!(recommended.contains("session.md"));
            }
        }
    }

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
fn file_ipc_missing_lazily_receipt_fails_closed_before_commit() {
    let (tmp, doc, baseline, original) = setup_project(true);
    let root = tmp.path();
    let agent_doc_dir = root.join(".agent-doc");
    let patches_dir = agent_doc_dir.join("patches");
    let ack_dir = agent_doc_dir.join("ack-content");
    let corrupt_marker = "bad-ack-content-only";
    let corrupt_ack = format!("### Re: bad ack - gpt-5\n{corrupt_marker}\n");
    let response = response_text("bad ack");
    let initial_head = head_blob(root);

    let watcher = std::thread::spawn(move || {
        let started = Instant::now();
        while started.elapsed() < FILE_IPC_WATCHER_TIMEOUT {
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

    run_finalize_expect(root, &doc, &baseline, &response, 1);
    assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");

    let visible = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        original, visible,
        "legacy ack-content must not be repaired through a direct document write"
    );
    let head = head_blob(root);
    assert_eq!(
        initial_head, head,
        "missing lazily receipt must fail before committing any response"
    );
    assert!(
        !head.contains(corrupt_marker),
        "legacy ack-content sidecar must not become the committed document:\n{head}"
    );
    let snapshot = fs::read_to_string(snapshot_path(root, &doc)).unwrap();
    assert!(
        !snapshot.contains(&response_body("bad ack")),
        "missing lazily receipt retry must not save the fallback response snapshot:\n{snapshot}"
    );
    assert!(
        !snapshot.contains(corrupt_marker),
        "legacy ack-content sidecar must not become the saved snapshot:\n{snapshot}"
    );
    let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("ipc_proof_insufficient")
            && ops_log.contains("invariant=no_lazily_visible_write_receipt"),
        "missing lazily receipt should fail before snapshot/commit with a proof diagnostic:\n{ops_log}"
    );
    assert!(
        ops_log.contains("recovery=retry_without_disk_write"),
        "missing lazily receipt should request an editor retry without a direct disk write:\n{ops_log}"
    );
}

#[test]
fn file_ipc_partial_response_materialization_fails_closed_before_commit() {
    let (tmp, doc, baseline, _original) = setup_project(true);
    let root = tmp.path();
    let agent_doc_dir = root.join(".agent-doc");
    let patches_dir = agent_doc_dir.join("patches");
    let ack_dir = agent_doc_dir.join("ack-content");
    let response = response_text("partial response");
    let partial_marker = "partial-only-response-heading";
    let initial_head = head_blob(root);

    let doc_for_watcher = doc.clone();
    let watcher = std::thread::spawn(move || {
        let started = Instant::now();
        while started.elapsed() < FILE_IPC_WATCHER_TIMEOUT {
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

    run_finalize_expect(root, &doc, &baseline, &response, 1);
    assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");

    let visible = fs::read_to_string(&doc).unwrap();
    assert!(
        visible.contains(partial_marker),
        "editor-visible partial materialization should remain for retry:\n{visible}"
    );
    assert!(
        !visible.contains(&response_body("partial response")),
        "partial materialization must not be completed by a direct disk write:\n{visible}"
    );
    let head = head_blob(root);
    assert_eq!(
        initial_head, head,
        "partial response materialization must fail before committing"
    );
    assert!(
        !head.contains(partial_marker),
        "partial response materialization must not reach the commit:\n{head}"
    );
    let snapshot = fs::read_to_string(snapshot_path(root, &doc)).unwrap();
    assert!(
        !snapshot.contains(&response_body("partial response")),
        "partial materialization retry must not save the fallback response snapshot:\n{snapshot}"
    );
    assert!(
        !snapshot.contains(partial_marker),
        "partial response materialization must not become the saved snapshot:\n{snapshot}"
    );
    let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("ipc_materialization_missing_response"),
        "partial materialization should fail before snapshot/commit:\n{ops_log}"
    );
    assert!(
        ops_log.contains("recovery=retry_without_disk_write"),
        "partial materialization should request an editor retry without a direct disk write:\n{ops_log}"
    );
}

#[test]
fn socket_ipc_post_block_prompt_drift_commits_ack_authority_snapshot() {
    let (tmp, doc, baseline, _original) = setup_project(true);
    let root = tmp.path();
    let agent_doc_dir = root.join(".agent-doc");
    let live_note = "Typing below exchange during closeout. #next-steps";
    let current_with_note = fs::read_to_string(&doc)
        .unwrap()
        .replace("<!--\n-->", &format!("<!--\n{live_note}\n-->"));
    fs::write(&doc, &current_with_note).unwrap();
    record_operator_buffer(&doc, &current_with_note);

    let seen_payload = Arc::new(Mutex::new(None::<Value>));
    let seen_for_listener = seen_payload.clone();
    let doc_for_listener = doc.clone();
    let listener_root = root.to_path_buf();
    let ack_dir = agent_doc_dir.join("ack-content");
    let server = std::thread::spawn(move || {
        agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
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
            record_operator_buffer(&doc_for_listener, &after_apply);
            fs::write(ack_dir.join(format!("{id}.md")), &after_apply).ok()?;
            *seen_for_listener.lock().ok()? = Some(payload);
            Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
        })
        .ok();
    });
    for _ in 0..100 {
        if agent_doc_ipc_io::is_listener_active(root) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        agent_doc_ipc_io::is_listener_active(root),
        "fake socket listener did not start"
    );

    run_finalize_expect(
        root,
        &doc,
        &baseline,
        &response_text("socket prompt drift"),
        0,
    );

    let _ = fs::remove_file(agent_doc_ipc_io::socket_path(root));
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
    assert_eq!(
        head.matches(live_note).count(),
        1,
        "ACK-proven post-block prompt drift should be committed exactly once:\n{head}"
    );
    let visible = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        visible.matches(live_note).count(),
        1,
        "post-block prompt drift should remain visible exactly once:\n{visible}"
    );
    let snapshot = fs::read_to_string(snapshot_path(root, &doc)).unwrap();
    assert!(snapshot.contains(&response_body("socket prompt drift")));
    assert_eq!(
        snapshot.matches(live_note).count(),
        1,
        "ACK-proven snapshot should preserve post-block prompt drift exactly once:\n{snapshot}"
    );
    let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("ipc_snapshot_adoption_blocked")
            && ops_log.contains("reason=live_prompt_drift_after_preflight"),
        "socket ACK prompt drift should block snapshot adoption:\n{ops_log}"
    );
}

/// `#exch-intermix`: a long `### Re:` response, big enough that adopting it over
/// a fragmented (response-lost) visible file is a real stale-snapshot wedge.
fn long_response_body() -> String {
    concat!(
        "### Re: live prompt drift recovery - gpt-5\n\n",
        "Implemented and verified the auto-recovery for the live_prompt_drift wedge.\n",
        "This response body is intentionally long so the adopted content_ours snapshot\n",
        "is well over the stale-snapshot-reset-drift threshold relative to the\n",
        "fragmented visible file that lost the response. That makes the commit-time\n",
        "guard a genuine wedge instead of a benign small delta.\n"
    )
    .to_string()
}

fn wedge_content_ours(original: &str) -> String {
    apply_patch_at_boundary(original, &long_response_body())
}

/// Set up the closeout wedge: snapshot adopted `content_ours` (baseline + the long
/// response) while the visible file lost the response, and the cycle carries the
/// `ipc_snapshot_adoption_blocked` flag from the drift guard.
fn wedge_cycle_state(doc: &Path, content_ours: &str, fragmented: &str) {
    agent_doc_cycle_state_io::start_preflight(doc, Some(content_ours), Some(fragmented)).unwrap();
    agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(doc).unwrap();
}

#[test]
fn commit_auto_recovers_live_prompt_drift_wedge() {
    // The benign wedge: response safe in the adopted snapshot, visible file
    // fragmented, no disk-only user prompt. The commit must auto-recover (write
    // the snapshot to disk + commit the response) instead of failing closed as a
    // "manual cleanup".
    let (tmp, doc, _baseline, original) = setup_project(false);
    let root = tmp.path();

    let content_ours = wedge_content_ours(&original);
    // Visible file lost the response (only the committed baseline remains).
    fs::write(&doc, &original).unwrap();
    fs::write(snapshot_path(root, &doc), &content_ours).unwrap();
    wedge_cycle_state(&doc, &content_ours, &original);

    agent_doc()
        .current_dir(root)
        .args(["commit", doc.to_str().unwrap()])
        .assert()
        .success();

    let head = head_blob(root);
    assert!(
        head.contains("Implemented and verified the auto-recovery"),
        "auto-recovered commit must land the response in HEAD:\n{head}"
    );
    let visible = fs::read_to_string(&doc).unwrap();
    assert!(
        visible.contains("Implemented and verified the auto-recovery"),
        "auto-recovery must restore the response to the visible working tree:\n{visible}"
    );
    let ops_log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("live_prompt_drift_auto_recovered"),
        "auto-recovery must leave an observable ops.log marker:\n{ops_log}"
    );
}

#[test]
fn commit_fails_closed_when_drift_carries_disk_only_user_prompt() {
    // Negative case: the same wedge shape, but the visible file also carries a
    // genuine new user prompt the snapshot never saw. Auto-recovery must NOT fire
    // (it would silently drop the user prompt); the commit fails closed instead.
    let (tmp, doc, _baseline, original) = setup_project(false);
    let root = tmp.path();

    let content_ours = wedge_content_ours(&original);
    // Visible file lost the response but gained a real user prompt typed after
    // preflight, inside the exchange component (not a comment region).
    let fragmented = original.replace(
        "\u{276f} Please reply\n",
        "\u{276f} Please reply\n\u{276f} do #typed-after-preflight\n",
    );
    fs::write(&doc, &fragmented).unwrap();
    fs::write(snapshot_path(root, &doc), &content_ours).unwrap();
    wedge_cycle_state(&doc, &content_ours, &fragmented);

    agent_doc()
        .current_dir(root)
        .args(["commit", doc.to_str().unwrap()])
        .assert()
        .failure();

    let ops_log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        !ops_log.contains("live_prompt_drift_auto_recovered"),
        "a disk-only user prompt must block auto-recovery (fail closed):\n{ops_log}"
    );
    let visible = fs::read_to_string(&doc).unwrap();
    assert!(
        visible.contains("do #typed-after-preflight"),
        "the genuine user prompt must remain on disk for the next cycle:\n{visible}"
    );
}

#[test]
fn file_ipc_timeout_retains_patch_before_retry() {
    let (tmp, doc, baseline, _original) = setup_project(true);
    let root = tmp.path();
    let initial_head = head_blob(root);

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
        .failure()
        .stderr(predicates::str::contains(
            "recovery=retry_without_disk_write",
        ));

    let head = head_blob(root);
    assert_eq!(
        initial_head, head,
        "IPC timeout must fail before committing a response"
    );
    let visible = fs::read_to_string(&doc).unwrap();
    assert!(
        !visible.contains(&response_body("stale patch replay")),
        "IPC timeout must not write the response directly to the document"
    );

    let patch_jsons = fs::read_dir(root.join(".agent-doc/patches"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    assert!(
        !patch_jsons.is_empty(),
        "IPC timeout should retain file IPC payloads for editor retry"
    );

    let claimed_entries = fs::read_dir(root.join(".agent-doc/claimed-patches"))
        .unwrap()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    assert!(
        claimed_entries.is_empty(),
        "IPC timeout should not claim an uncommitted payload"
    );
    let ops_log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("recovery=retry_without_disk_write"),
        "IPC timeout should request a retry without direct document write:\n{ops_log}"
    );
}
