use agent_doc_hash::content_hash;
use agent_doc_turn::closeout_recovery::{
    CloseoutRecoveryDecision, CloseoutRecoveryDecisionInput, CloseoutRecoveryState,
    closeout_recovery_decision_from_state,
};
use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use proptest::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
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
    VisibleWrite,
    ContentOurs,
    FileRead,
    DirectDisk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseoutHazard {
    MissingVisibleWriteReceipt,
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
            Some(
                CloseoutHazard::MissingVisibleWriteReceipt
                | CloseoutHazard::PartialResponseMaterialization,
            ),
        ) => CloseoutTransition::RejectBeforeCommit,
        (
            IpcTransport::Socket | IpcTransport::File,
            Some(CloseoutHazard::PromptDrift | CloseoutHazard::PostBlockSnapshotAbsorb),
        ) => CloseoutTransition::Commit(SnapshotSource::ContentOurs),
        (IpcTransport::Socket | IpcTransport::File, None) => {
            CloseoutTransition::Commit(SnapshotSource::VisibleWrite)
        }
        (IpcTransport::DirectDisk, Some(CloseoutHazard::MissingVisibleWriteReceipt)) => {
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
        CloseoutHazard::MissingVisibleWriteReceipt
        | CloseoutHazard::PartialResponseMaterialization => {
            matches!(
                source,
                SnapshotSource::VisibleWrite | SnapshotSource::FileRead
            )
        }
        CloseoutHazard::PromptDrift | CloseoutHazard::PostBlockSnapshotAbsorb => {
            matches!(
                source,
                SnapshotSource::VisibleWrite | SnapshotSource::FileRead
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
            CloseoutRecoveryState::OpenCycle
                | CloseoutRecoveryState::MissingResponseBody
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
        CloseoutRecoveryState::RecoveryProjectionVisibleDrift => {
            "refresh_recovery_projections_from_visible"
        }
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

fn setup_project(with_patches_dir: bool) -> (TempDir, PathBuf, String) {
    let tmp = TempDir::new().unwrap();
    let agent_doc_dir = tmp.path().join(".agent-doc");
    for subdir in [
        "ack-content",
        "claimed-patches",
        "crdt",
        "logs",
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
    (tmp, doc, original)
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

fn apply_patch_at_boundary(current: &str, patch: &str) -> String {
    current.replace(
        "<!-- agent:boundary:abcd1234 -->",
        &format!("{patch}<!-- agent:boundary:abcd1234 -->"),
    )
}

#[test]
fn ipc_closeout_transition_table_forbids_unsafe_commits() {
    assert!(
        unsafe_snapshot_source(
            CloseoutHazard::MissingVisibleWriteReceipt,
            SnapshotSource::FileRead
        ),
        "file-read snapshots are unsafe when visible-write proof is missing"
    );

    let cases = [
        (
            IpcTransport::Socket,
            Some(CloseoutHazard::MissingVisibleWriteReceipt),
            CloseoutTransition::RejectBeforeCommit,
        ),
        (
            IpcTransport::File,
            Some(CloseoutHazard::MissingVisibleWriteReceipt),
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
            | CloseoutRecoveryDecision::RefreshRecoveryProjectionsFromVisible {
                command,
                ..
            } => {
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
            0 => CloseoutHazard::MissingVisibleWriteReceipt,
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
fn commit_does_not_promote_recovery_projection_without_retained_intent() {
    // A response that exists only in a cold snapshot is not a current-document
    // intent. Commit may close the clean visible state, but it must not promote
    // the recovery projection into the document or HEAD.
    let (tmp, doc, original) = setup_project(false);
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
        !head.contains("Implemented and verified the auto-recovery"),
        "commit must not promote a snapshot-only response into HEAD:\n{head}"
    );
    let visible = fs::read_to_string(&doc).unwrap();
    assert!(
        !visible.contains("Implemented and verified the auto-recovery"),
        "commit must keep Lazily/disk current rather than replace it from recovery state:\n{visible}"
    );
    assert!(
        fs::read_to_string(snapshot_path(root, &doc))
            .unwrap()
            .contains("Implemented and verified the auto-recovery"),
        "cold recovery evidence may remain available without becoming authority"
    );
}

#[test]
fn commit_rebases_response_over_newer_visible_user_prompt() {
    // The visible file carries a genuine new user prompt the response snapshot
    // never saw. Semantic response recovery appends only the missing response
    // cell over that newer cut, so neither side needs to be discarded.
    let (tmp, doc, original) = setup_project(false);
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
    agent_doc_capture_io::capture_response_with_current_content(
        &doc,
        &long_response_body(),
        &fragmented,
    )
    .unwrap();

    agent_doc()
        .current_dir(root)
        .args(["write", "--commit", doc.to_str().unwrap()])
        .assert()
        .success();

    let ops_log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("repair_replay_via_strict_write"),
        "captured-intent replay through the strict write state machine must remain observable:\n{ops_log}"
    );
    let visible = fs::read_to_string(&doc).unwrap();
    assert!(
        visible.contains("do #typed-after-preflight"),
        "the genuine user prompt must survive semantic response recovery:\n{visible}"
    );
    assert!(
        visible.contains("Implemented and verified the auto-recovery"),
        "the missing response must land beside the newer prompt:\n{visible}"
    );
    let head = head_blob(root);
    assert!(
        head.contains("do #typed-after-preflight")
            && head.contains("Implemented and verified the auto-recovery"),
        "commit must preserve the prompt and response together:\n{head}"
    );
}

#[test]
fn file_ipc_capability_with_editor_absent_skips_queue_and_writes_directly() {
    let (tmp, doc, _original) = setup_project(true);
    let root = tmp.path();
    let initial_head = head_blob(root);

    agent_doc()
        .current_dir(root)
        .args(["write", doc.to_str().unwrap(), "--stream", "--commit"])
        .write_stdin(response_text("stale patch replay"))
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "reliable-sync reports the editor absent",
        ));

    let head = head_blob(root);
    assert_ne!(
        initial_head, head,
        "editor-absent direct write should commit the response"
    );
    let visible = fs::read_to_string(&doc).unwrap();
    assert!(
        visible.contains(&response_body("stale patch replay")),
        "editor-absent direct write should materialize the response:\n{visible}"
    );

    let patch_jsons = fs::read_dir(root.join(".agent-doc/patches"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    assert!(
        patch_jsons.is_empty(),
        "editor-absent direct write must not queue a file IPC payload"
    );

    let ops_log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("write_ipc_editor_absent_disk_authority")
            && ops_log.contains("reason=reliable_sync_editor_absent"),
        "editor absence should select direct disk authority before IPC:\n{ops_log}"
    );
}
