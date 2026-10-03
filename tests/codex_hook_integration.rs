use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use tempfile::TempDir;

fn agent_doc() -> Command {
    cargo_bin_cmd!("agent-doc")
}

fn template_doc_content() -> String {
    "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:pending -->\n<!-- /agent:pending -->\n".to_string()
}

fn setup_template_doc() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, template_doc_content()).unwrap();
    (tmp, doc)
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

fn track_codex_session(root: &Path, doc: &Path) {
    agent_doc_codex_hook_io::apply_user_prompt_submit(
        &agent_doc_codex_hook_io::UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: root.display().to_string(),
            prompt: format!("agent-doc {}", doc.display()),
        },
    )
    .unwrap();
}

fn auto_queue_doc_content_with_prompts(prompts: &[&str]) -> String {
    let queue = prompts
        .iter()
        .map(|prompt| format!("- {prompt}\n"))
        .collect::<String>();
    format!(
        "---\nagent_doc_session: testsid\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n### Re: prior — gpt-5\n\nDone.\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue auto go -->\n{queue}<!-- /agent:queue -->\n"
    )
}

fn auto_queue_doc_content() -> String {
    auto_queue_doc_content_with_prompts(&["do [#seopdp] deploy product page"])
}

#[test]
fn session_check_codex_final_gate_blocks_on_active_auto_queue() {
    // #codex-auto-queue-stalled-final-gate: a clean document that still owes an
    // `agent:queue auto` continuation reports `queue_continuation_required=true`,
    // exits 0 in default mode, and exits nonzero under `--codex-final-gate`.
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, auto_queue_doc_content()).unwrap();
    // Commit so the working tree matches HEAD — a clean cycle with no open
    // preflight state (running preflight here would rewrite frontmatter and open
    // a cycle, defeating the clean-document scenario under test).
    init_git_repo(tmp.path(), &doc);

    // Default mode: clean cycle is OK (exit 0) but surfaces the typed detail.
    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("queue_continuation_required=true"))
        .stdout(predicate::str::contains("do [#seopdp] deploy product page"));

    // Strict Codex final gate: continuation required → nonzero exit.
    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap(), "--codex-final-gate"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("queue_continuation_required=true"));
}

/// A committed cycle whose live document carries an operator prompt added
/// after the commit (`#steerinterruptexit`).
fn committed_cycle_with_post_commit_steering() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
    let doc = tmp.path().join("session.md");
    let committed = "---\nagent_doc_session: sid\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange patch=append -->\n### Re: done — gpt-5\n\nCompleted.\n<!-- /agent:exchange -->\n";
    fs::write(&doc, committed).unwrap();
    init_git_repo(tmp.path(), &doc);
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &doc,
        committed,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();
    agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
    agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
        &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
        &doc,
        "commit_success",
        Some(committed),
        Some(committed),
    )
    .unwrap();
    fs::write(
        &doc,
        committed.replace(
            "Completed.\n",
            "Completed.\n\n❯ Also check the CI run for the release.\n",
        ),
    )
    .unwrap();
    (tmp, doc)
}

#[test]
fn session_check_cli_reports_post_commit_steering_as_pending_with_exit_zero() {
    // `#steerinterruptexit`: steering after a committed cycle is not a failure.
    // The CLI exits 0, names it `steering pending`, lists the item verbatim
    // with its dispatch, and defers queue continuation behind it.
    let (tmp, doc) = committed_cycle_with_post_commit_steering();
    let out = agent_doc()
        .current_dir(tmp.path())
        .env("AGENT_DOC_SESSION_CHECK_SETTLE_SECS", "0")
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("[session-check] steering pending:"))
        .stdout(predicate::str::contains(
            "dispatch=address_now source=exchange change=added",
        ))
        .stdout(predicate::str::contains(
            "❯ Also check the CI run for the release.",
        ))
        .stdout(predicate::str::contains(
            "queue_continuation_required=false steering_pending=true",
        ))
        .stdout(predicate::str::contains("INTERRUPTED").not());
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_ascii_lowercase();
    assert!(
        !stdout.contains("current turn"),
        "steering is the next cycle's input, not this turn's: {stdout}"
    );

    // The strict Codex final gate still holds the final answer for it, with
    // the gate's "work owed" code, not the failure code.
    agent_doc()
        .current_dir(tmp.path())
        .env("AGENT_DOC_SESSION_CHECK_SETTLE_SECS", "0")
        .args(["session-check", doc.to_str().unwrap(), "--codex-final-gate"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("[session-check] steering pending:"));
}

#[test]
fn codex_hook_cli_replays_plain_final_answer_after_repeated_auto_queue_stop() {
    // Reproduces the sampleorders shape: a clean template/CRDT Codex
    // session doc has an active auto queue, the first Stop hook asks Codex to
    // continue in-pane, and the second Stop hook receives a plain final answer
    // for the same queue head. The answer must be written into agent:exchange.
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(
        &doc,
        auto_queue_doc_content_with_prompts(&[
            "do [#seopdp] deploy product page",
            "do [#smoke] verify checkout",
        ]),
    )
    .unwrap();
    init_git_repo(tmp.path(), &doc);

    // This fixture exercises repeated Stop recovery from an already-clean
    // queue document, so bind the session without starting a new admission
    // cycle. Codex entrypoint admission is covered separately below.
    track_codex_session(tmp.path(), &doc);

    let first_stop = json!({
        "session_id": "codex-session",
        "turn_id": "turn-1",
        "cwd": tmp.path().display().to_string(),
        "last_assistant_message": "First stop only requests continuation.",
        "stop_hook_active": false,
    });

    agent_doc()
        .current_dir(tmp.path())
        .args(["hook", "codex-stop"])
        .write_stdin(first_stop.to_string())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"decision\":\"block\""))
        .stdout(predicate::str::contains("do [#seopdp] deploy product page"));

    let second_stop = json!({
        "session_id": "codex-session",
        "turn_id": "turn-1",
        "cwd": tmp.path().display().to_string(),
        "last_assistant_message": "Completed the queue task.\n\nVerification: reproduced the sampleorders Codex stop-hook shape.",
        "stop_hook_active": true,
    });

    agent_doc()
        .current_dir(tmp.path())
        .args(["hook", "codex-stop"])
        .write_stdin(second_stop.to_string())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"decision\":\"block\""))
        .stdout(predicate::str::contains(
            "recovered the previous queue response",
        ))
        .stdout(predicate::str::contains("do [#smoke] verify checkout"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: do [#seopdp] deploy product page — gpt-5"));
    assert!(content.contains("Completed the queue task."));
    assert!(content.contains("Verification: reproduced the sampleorders Codex stop-hook shape."));
    assert!(
        !content.contains("- do [#seopdp] deploy product page"),
        "completed head should not remain live:\n{content}"
    );
    assert!(content.contains("- do [#smoke] verify checkout"));

    let log = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["log", "--oneline", "-1"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&log.stdout).contains("agent-doc(session):"),
        "expected repeated-stop recovery commit, got: {}",
        String::from_utf8_lossy(&log.stdout)
    );
}

#[test]
fn codex_hook_cli_auto_closes_open_cycle_after_user_prompt_submit() {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);

    let submit_payload = json!({
        "session_id": "codex-session",
        "turn_id": "turn-1",
        "cwd": tmp.path().display().to_string(),
        "prompt": format!("agent-doc {}", doc.display()),
    });

    let submit = agent_doc()
        .current_dir(tmp.path())
        .args(["hook", "codex-user-prompt-submit"])
        .write_stdin(submit_payload.to_string())
        .assert()
        .success();
    let hook_output: serde_json::Value = serde_json::from_slice(&submit.get_output().stdout)
        .expect("UserPromptSubmit stdout must be exactly one valid JSON document");
    assert_eq!(
        hook_output["hookSpecificOutput"]["hookEventName"],
        "UserPromptSubmit"
    );
    let additional_context = hook_output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("hook response must carry additionalContext");
    assert!(
        additional_context
            .contains("[agent-doc] cycle contract (preflight already ran in the binary;")
    );
    assert!(
        additional_context
            .contains("Codex in-pane admission: continue this response cycle in the current turn"),
        "an admitted Codex trigger must forbid recursive shell reinvocation: {additional_context}"
    );
    let contract_end = additional_context
        .find("\n[agent-doc] cycle contract")
        .expect("contract marker must seal the captured preflight JSON");
    serde_json::from_str::<serde_json::Value>(&additional_context[..contract_end])
        .expect("captured preflight contract must remain valid JSON");

    // The same hook invocation must also have opened the cycle. This proves
    // Codex admission is not merely an injected marker or tracking-only state.
    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("INTERRUPTED"));

    let stop_payload = json!({
        "session_id": "codex-session",
        "turn_id": "turn-1",
        "cwd": tmp.path().display().to_string(),
        "last_assistant_message": "<!-- patch:exchange -->\n### Re: hook proof — gpt-5\nHook closeout body.\n<!-- /patch:exchange -->\n",
        "stop_hook_active": false,
    });

    agent_doc()
        .current_dir(tmp.path())
        .args(["hook", "codex-stop"])
        .write_stdin(stop_payload.to_string())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"continue\":true"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: hook proof — gpt-5"));

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();

    let log = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["log", "--oneline", "-1"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&log.stdout).contains("agent-doc(session):"),
        "expected auto-close commit, got: {}",
        String::from_utf8_lossy(&log.stdout)
    );

    let session_state_dir = tmp.path().join(".agent-doc/codex-hooks/sessions");
    if session_state_dir.exists() {
        let remaining: Vec<_> = fs::read_dir(&session_state_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .collect();
        assert!(
            remaining.is_empty(),
            "session hook state should be cleared after successful stop auto-close"
        );
    }
}

#[test]
fn codex_hook_cli_reuses_exact_same_turn_admission() {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);
    let submit_payload = json!({
        "session_id": "same-turn-session",
        "turn_id": "same-turn-id",
        "cwd": tmp.path().display().to_string(),
        "prompt": format!("agent-doc {}", doc.display()),
    });

    agent_doc()
        .current_dir(tmp.path())
        .args(["hook", "codex-user-prompt-submit"])
        .write_stdin(submit_payload.to_string())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "cycle contract (preflight already ran",
        ));
    let before = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();

    let repeated = agent_doc()
        .current_dir(tmp.path())
        .args(["hook", "codex-user-prompt-submit"])
        .write_stdin(submit_payload.to_string())
        .assert()
        .success();
    let hook_output: serde_json::Value = serde_json::from_slice(&repeated.get_output().stdout)
        .expect("same-turn reuse stdout must be exactly one valid JSON document");
    let context = hook_output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("same-turn reuse must carry additionalContext");
    assert!(context.contains("\"reused_admission\": true"));
    assert!(context.contains("\"kind\": \"same_turn_repeat\""));
    assert!(context.contains("cycle contract (preflight already ran"));
    assert!(!context.contains("cycle contract UNAVAILABLE"));

    let after = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
    assert_eq!(after.cycle_id, before.cycle_id);
    assert_eq!(after.last_event, "preflight_started");
    let state = agent_doc_codex_hook_io::load_state(tmp.path(), "same-turn-session")
        .unwrap()
        .unwrap();
    assert_eq!(state.preflight_admitted, Some(true));
}

#[test]
fn codex_hook_cli_wraps_admission_failure_in_one_json_document() {
    let tmp = TempDir::new().unwrap();
    let missing = tmp.path().join("missing.md");
    let submit_payload = json!({
        "session_id": "codex-session-failure",
        "turn_id": "turn-1",
        "cwd": tmp.path().display().to_string(),
        "prompt": format!("agent-doc {}", missing.display()),
    });

    let submit = agent_doc()
        .current_dir(tmp.path())
        .args(["hook", "codex-user-prompt-submit"])
        .write_stdin(submit_payload.to_string())
        .assert()
        .success();
    let hook_output: serde_json::Value = serde_json::from_slice(&submit.get_output().stdout)
        .expect("admission-failure stdout must be exactly one valid JSON document");
    assert_eq!(
        hook_output["hookSpecificOutput"]["hookEventName"],
        "UserPromptSubmit"
    );
    let additional_context = hook_output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("admission failure must carry additionalContext");
    assert!(additional_context.contains("[agent-doc] cycle contract UNAVAILABLE"));
    assert!(
        additional_context.contains("did not resolve to a file")
            || additional_context.contains("failed to canonicalize"),
        "missing-document admission must explain the path-resolution failure: {additional_context}"
    );
    let stderr = String::from_utf8_lossy(&submit.get_output().stderr);
    assert!(
        stderr.contains("[agent-doc] preflight hook failed")
            || stderr.contains("[agent-doc] Codex session tracking failed"),
        "missing-document admission must be reported to the operator: {stderr}"
    );
}

#[test]
fn codex_hook_cli_leaves_a_pasted_transcript_that_begins_with_a_trigger_alone() {
    // `#pastetriggeradmit`: an operator pasted a transcript whose first line was
    // `agent-doc tasks/fpe.md` (a document under another project root). The
    // hook resolved it against its own cwd, failed to canonicalize, and told the
    // agent admission was refused. A multi-line prompt whose document does not
    // resolve is operator content, not an invocation.
    let tmp = TempDir::new().unwrap();
    let submit_payload = json!({
        "session_id": "codex-session-paste",
        "turn_id": "turn-1",
        "cwd": tmp.path().display().to_string(),
        "prompt": "agent-doc tasks/fpe.md\n\u{2022} Ran git status --short\n  \u{2514} M tasks/fpe.md\n",
    });

    let submit = agent_doc()
        .current_dir(tmp.path())
        .args(["hook", "codex-user-prompt-submit"])
        .write_stdin(submit_payload.to_string())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&submit.get_output().stdout);
    assert!(
        !stdout.contains("cycle contract UNAVAILABLE"),
        "a pasted transcript must not be reported as a refused admission: {stdout}"
    );
    assert!(
        !stdout.contains("failed to canonicalize"),
        "a pasted transcript must not be resolved as a document binding: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&submit.get_output().stderr);
    assert!(
        !stderr.contains("Codex session tracking failed"),
        "tracking must not try to bind the pasted path: {stderr}"
    );
}

#[test]
fn codex_hook_cli_resumes_original_capture_over_editor_convergence_block() {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);
    let content = fs::read_to_string(&doc).unwrap();
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &doc,
        &content,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();
    agent_doc_cycle_state_io::start_preflight(&doc, Some(&content), Some(&content)).unwrap();
    let retained_response = "<!-- patch:exchange -->\n### Re: retained — gpt-5\nRetained patch.\n<!-- /patch:exchange -->\n";
    agent_doc_repair_io::pending::save_pending(&doc, retained_response).unwrap();
    agent_doc_cycle_state_io::record_editor_convergence_required(
        &doc,
        "try_editor_converge",
        "send_failed",
        Some("patch-retained"),
        Some("editor_endpoint=live"),
    )
    .unwrap();

    // Preserve the deliberately staged convergence state while binding it to
    // Codex; the combined entrypoint's admission behavior is tested above.
    track_codex_session(tmp.path(), &doc);

    let stop_payload = json!({
        "session_id": "codex-session",
        "turn_id": "turn-1",
        "cwd": tmp.path().display().to_string(),
        "last_assistant_message": "<!-- patch:exchange -->\n### Re: stale stop payload — gpt-5\nThis must not replace the retained editor retry patch.\n<!-- /patch:exchange -->\n",
        "stop_hook_active": false,
    });
    agent_doc()
        .current_dir(tmp.path())
        .args(["hook", "codex-stop"])
        .write_stdin(stop_payload.to_string())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"continue\":true"));

    let committed = fs::read_to_string(&doc).unwrap();
    assert!(
        committed.contains("### Re: retained — gpt-5"),
        "Stop hook must commit the original retained capture:\n{committed}"
    );
    assert!(
        !committed.contains("stale stop payload"),
        "Stop hook must not replace the retained capture with its stale assistant payload:\n{committed}"
    );
    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn codex_hook_cli_blocks_transcript_shaped_last_assistant_message() {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);

    let submit_payload = json!({
        "session_id": "codex-session",
        "turn_id": "turn-1",
        "cwd": tmp.path().display().to_string(),
        "prompt": format!("agent-doc {}", doc.display()),
    });

    agent_doc()
        .current_dir(tmp.path())
        .args(["hook", "codex-user-prompt-submit"])
        .write_stdin(submit_payload.to_string())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "preflight already ran in the binary",
        ));

    let transcript_payload = concat!(
        "<!-- agent:exchange patch=append -->\n",
        "❯ Please reply\n",
        "### Re: hook proof — gpt-5\n",
        "Hook closeout body.\n",
        "<!-- agent:boundary:1234abcd -->\n",
        "<!-- /agent:exchange -->\n",
    );
    let stop_payload = json!({
        "session_id": "codex-session",
        "turn_id": "turn-1",
        "cwd": tmp.path().display().to_string(),
        "last_assistant_message": transcript_payload,
        "stop_hook_active": false,
    });

    agent_doc()
        .current_dir(tmp.path())
        .args(["hook", "codex-stop"])
        .write_stdin(stop_payload.to_string())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"decision\":\"block\""))
        .stdout(predicate::str::contains("refused to replay"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("### Re: hook proof — gpt-5"),
        "transcript-shaped payload should not be replayed into the document"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("INTERRUPTED"));

    let blocked_dir = tmp.path().join(".agent-doc/codex-hooks/blocked-stop");
    let blocked: Vec<_> = fs::read_dir(&blocked_dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .collect();
    assert_eq!(
        blocked.len(),
        1,
        "expected one blocked-stop diagnostic capture"
    );
    let blocked_payload = fs::read_to_string(blocked[0].path()).unwrap();
    assert!(blocked_payload.contains("agent:exchange"));
    assert!(blocked_payload.contains("Hook closeout body."));
}

#[test]
fn codex_hook_cli_refused_admission_preserves_previous_cycle() {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);
    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();
    let submit = json!({
        "session_id": "refused-session", "turn_id": "refused-turn",
        "cwd": tmp.path().display().to_string(),
        "prompt": format!("agent-doc {}", doc.display()),
    });
    agent_doc()
        .current_dir(tmp.path())
        .args(["hook", "codex-user-prompt-submit"])
        .write_stdin(submit.to_string())
        .assert()
        .success()
        .stdout(predicate::str::contains("cycle contract UNAVAILABLE"));
    let before = fs::read_to_string(&doc).unwrap();
    let stop = json!({
        "session_id": "refused-session", "turn_id": "refused-turn",
        "cwd": tmp.path().display().to_string(),
        "last_assistant_message": "Preflight refused; content remains retained.",
        "stop_hook_active": true,
    });
    agent_doc()
        .current_dir(tmp.path())
        .args(["hook", "codex-stop"])
        .write_stdin(stop.to_string())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"continue\":true"));
    assert_eq!(fs::read_to_string(&doc).unwrap(), before);
    assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
}
