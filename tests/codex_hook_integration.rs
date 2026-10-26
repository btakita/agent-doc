use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use tempfile::TempDir;

/// `#integrationtesttargets`: this target drives the real `agent-doc` CLI, and
/// a controller launch by that CLI is production behavior, so every spawned
/// command opts in with this reason.
const CONTROLLER_LAUNCH_REASON: &str = "the Codex hook CLI reaches the project controller during closeout; that launch is production behavior";

fn agent_doc() -> Command {
    let mut cmd = cargo_bin_cmd!("agent-doc");
    // `#stophookdeadlineflake`: these tests assert what the Stop hook does, not
    // its wall-clock budget. Under a loaded machine (load average ~100) the
    // real closeout work exceeded the 45s budget and the hook failed closed
    // (`continue:false`), flaking `codex_hook_cli_replays_plain_final_answer_
    // after_repeated_auto_queue_stop` and `codex_hook_cli_resumes_original_
    // capture_over_editor_convergence_block`. Wait for the worker's completion
    // event instead; the budget itself is unit-tested in agent-doc-codex-stop-io.
    cmd.env("AGENT_DOC_CODEX_STOP_HOOK_DEADLINE", "worker_completion");
    cmd.env(
        agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
        CONTROLLER_LAUNCH_REASON,
    );
    cmd
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
        .stdout(predicate::str::contains(
            "[session-check] steering pending:",
        ))
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
        .stdout(predicate::str::contains(
            "[session-check] steering pending:",
        ));
}

/// `#codexsteerinterrupt`: the `#steerinterruptexit` fixture inside a bound
/// Codex session. `steering` is written to disk after the commit; when
/// `editor_typed` is set it is also captured as the operator's editor op
/// against the committed baseline (editor-op provenance).
fn codex_committed_cycle_with_post_commit_drift(
    steering: &str,
    editor_typed: bool,
) -> (TempDir, PathBuf) {
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
    let close = "<!-- /agent:exchange -->";
    if editor_typed {
        agent_doc_op_capture_io::record_editor_op(
            &doc,
            &agent_doc_hash::content_hash(committed),
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: committed.find(close).unwrap(),
                text: steering.to_string(),
            },
        )
        .unwrap();
    }
    fs::write(
        &doc,
        committed.replacen(close, &format!("{steering}{close}"), 1),
    )
    .unwrap();
    agent_doc_codex_hook_io::record_external_prompt_for_file(
        &doc,
        "codex-session",
        &format!("agent-doc {}", doc.display()),
    )
    .unwrap();
    (tmp, doc)
}

#[test]
fn session_check_cli_reports_editor_proven_codex_post_commit_steering_as_pending() {
    // `#codexsteerinterrupt` (a): with CODEX_THREAD_ID bound to the document,
    // the active-harness-session drift check reported this operator prompt as
    // INTERRUPTED (exit 1) before the steering-pending path ran. Editor-op
    // provenance routes it to `steering pending`: exit 0, and 2 (work owed)
    // under the Codex final gate.
    let (tmp, doc) =
        codex_committed_cycle_with_post_commit_drift("\n❯ Also check the CI run.\n", true);
    agent_doc()
        .current_dir(tmp.path())
        .env("CODEX_THREAD_ID", "codex-session")
        .env("AGENT_DOC_SESSION_CHECK_SETTLE_SECS", "0")
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "[session-check] steering pending:",
        ))
        .stdout(predicate::str::contains("❯ Also check the CI run."))
        .stdout(predicate::str::contains(
            "queue_continuation_required=false steering_pending=true",
        ))
        .stdout(predicate::str::contains("INTERRUPTED").not());

    agent_doc()
        .current_dir(tmp.path())
        .env("CODEX_THREAD_ID", "codex-session")
        .env("AGENT_DOC_SESSION_CHECK_SETTLE_SECS", "0")
        .args(["session-check", doc.to_str().unwrap(), "--codex-final-gate"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains(
            "[session-check] steering pending:",
        ));
}

#[test]
fn session_check_cli_still_interrupts_codex_console_replay_without_editor_provenance() {
    // `#codexsteerinterrupt` (b): console output replayed into the document as
    // a fake prompt carries no editor-op provenance, so the active-harness
    // check still reports INTERRUPTED (exit 1), with or without the final gate.
    let (tmp, doc) = codex_committed_cycle_with_post_commit_drift(
        "\n• Ran agent-doc session-check session.md\n  └ [session-check] ok\n",
        false,
    );
    for extra in [None, Some("--codex-final-gate")] {
        let mut cmd = agent_doc();
        cmd.current_dir(tmp.path())
            .env("CODEX_THREAD_ID", "codex-session")
            .env("AGENT_DOC_SESSION_CHECK_SETTLE_SECS", "0")
            .args(["session-check", doc.to_str().unwrap()]);
        if let Some(flag) = extra {
            cmd.arg(flag);
        }
        cmd.assert()
            .code(1)
            .stdout(predicate::str::contains("steering pending").not())
            .stdout(predicate::str::contains(
                "active harness session changed this document",
            ));
    }
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

    // Deterministic cost oracle (independent of host speed): the repair's own
    // strict closeout must be consumed as the terminal receipt. A stale
    // pre-repair document memo used to make the receipt never match, so the
    // hook ran a second full closeout inside its wall-clock budget, and under
    // load that redundant pass is where the budget expired.
    let ops = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops.contains("codex_stop_repeated_queue_recovery_success source=strict_replay_receipt"),
        "repeated-stop recovery must reuse the strict replay receipt instead of re-running closeout:\n{ops}"
    );
    // Count completed closeouts by their terminal proof, which every closeout
    // records. `closeout_latency` is only logged for a closeout slower than
    // 250ms; it counted one here only while each CLI closeout shut down the
    // project controller first (`#integrationtesttargets`).
    assert_eq!(
        ops.matches("terminal_closeout_proof_recorded ").count(),
        1,
        "exactly one closeout must run for one recovered response:\n{ops}"
    );
    // `#integrationtesttargets`: the CLI adopts the project's launched
    // controller instead of displacing it with an in-process actor, so no
    // shutdown is requested at all, let alone refused.
    assert!(
        !ops.contains("controller_shutdown_refused"),
        "test actor shutdown was refused by a fresh controller:\n{ops}"
    );
    assert!(
        !ops.contains("controller_shutdown_accepted"),
        "a CLI closeout must reuse the project controller, not shut it down:\n{ops}"
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

/// `#fixrunturn` (infra.md, 2026-10-12): Run Agent Doc typed the trigger into a
/// busy Codex pane and Codex steered it into the turn while that turn's own
/// finalize had the cycle at `write_applied`. The hook used to run a fresh
/// preflight, which refused ("previous cycle ... is still `write_applied`") and
/// stamped a refusal receipt that fenced the Stop hook's queue continuation.
#[test]
fn codex_hook_cli_coalesces_same_turn_repeat_into_in_flight_closeout() {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);
    let submit_payload = json!({
        "session_id": "closeout-repeat-session",
        "turn_id": "closeout-repeat-turn",
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
    let admitted = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
    let content = fs::read_to_string(&doc).unwrap();
    agent_doc_cycle_state_io::mark_response_captured(
        &doc,
        "response_captured",
        Some(&content),
        Some(&content),
        "sha-closeout-in-flight",
        None,
    )
    .unwrap();
    agent_doc_cycle_state_io::mark_write_applied(
        &doc,
        "write_applied",
        Some(&content),
        Some(&content),
    )
    .unwrap();

    let repeated = agent_doc()
        .current_dir(tmp.path())
        .args(["hook", "codex-user-prompt-submit"])
        .write_stdin(submit_payload.to_string())
        .assert()
        .success();
    let hook_output: serde_json::Value = serde_json::from_slice(&repeated.get_output().stdout)
        .expect("coalesced repeat stdout must be exactly one valid JSON document");
    let context = hook_output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("coalesced repeat must carry additionalContext");
    assert!(context.contains("\"reused_admission\": true"), "{context}");
    assert!(
        context.contains("\"kind\": \"same_turn_repeat_closeout_in_flight\""),
        "{context}"
    );
    assert!(
        context.contains("\"cycle_phase\": \"write_applied\""),
        "{context}"
    );
    assert!(
        context.contains("cycle contract (preflight already ran"),
        "{context}"
    );
    assert!(
        context.contains("Do NOT start a second response"),
        "{context}"
    );
    assert!(
        !context.contains("cycle contract UNAVAILABLE"),
        "the overlapping trigger must not be refused: {context}"
    );
    assert!(
        !context.contains("execute its unresolved work now"),
        "the in-pane admission directive would start a second response over the closeout: {context}"
    );

    // The in-flight cycle is untouched and the exact-thread binding keeps its
    // admission, so the Stop hook still owns the continuation.
    let after = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
    assert_eq!(after.cycle_id, admitted.cycle_id);
    assert_eq!(after.phase.as_str(), "write_applied");
    let state = agent_doc_codex_hook_io::load_state(tmp.path(), "closeout-repeat-session")
        .unwrap()
        .unwrap();
    assert_eq!(state.preflight_admitted, Some(true));
    assert_eq!(state.last_turn_id, "closeout-repeat-turn");
    let ops_log =
        fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
    assert!(
        ops_log.contains("codex_same_turn_repeat_coalesced_into_closeout"),
        "the coalescing decision must be observable in ops.log"
    );
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

// ---------------------------------------------------------------------------
// GH #125 (`#chatprompt`): chat-originated prompts end to end.
// ---------------------------------------------------------------------------

fn codex_submit(root: &Path, session: &str, turn: &str, prompt: &str) -> String {
    let payload = json!({
        "session_id": session,
        "turn_id": turn,
        "cwd": root.display().to_string(),
        "prompt": prompt,
    });
    let submit = agent_doc()
        .current_dir(root)
        .args(["hook", "codex-user-prompt-submit"])
        .write_stdin(payload.to_string())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&submit.get_output().stdout).to_string();
    if stdout.trim().is_empty() {
        return String::new();
    }
    let output: serde_json::Value =
        serde_json::from_str(&stdout).expect("hook stdout must be one JSON document");
    output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// The preflight JSON inside an admitted hook context.
fn admitted_contract(context: &str) -> serde_json::Value {
    let start = context
        .find("{\n")
        .unwrap_or_else(|| panic!("no contract JSON in hook context: {context}"));
    let end = context
        .find("\n[agent-doc] cycle contract")
        .unwrap_or_else(|| panic!("no contract seal in hook context: {context}"));
    serde_json::from_str(&context[start..end])
        .unwrap_or_else(|err| panic!("contract is not JSON ({err}): {context}"))
}

fn respond(root: &Path, doc: &Path, response: &str) -> String {
    let out = agent_doc()
        .current_dir(root)
        .args(["respond", doc.to_str().unwrap()])
        .write_stdin(response.to_string())
        .assert()
        .success();
    String::from_utf8_lossy(&out.get_output().stderr).to_string()
}

fn session_check_stderr(root: &Path, doc: &Path) -> String {
    let out = agent_doc()
        .current_dir(root)
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
    String::from_utf8_lossy(&out.get_output().stderr).to_string()
}

fn head_document(root: &Path) -> String {
    let out = ProcessCommand::new("git")
        .current_dir(root)
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}

const CHAT: &str = "what changed in the last release?";
/// `agent_doc_prompt_lines::CHAT_PROMPT_RECORD_PREFIX`, as SKILL.md documents it.
const CHAT_RECORD_PREFIX: &str = "> **Chat prompt (#chatprompt):**";

/// Admit and close out the first trigger so the session is bound to `doc`.
fn bound_session_with_committed_cycle(session: &str) -> (TempDir, PathBuf) {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);
    let context = codex_submit(
        tmp.path(),
        session,
        "turn-1",
        &format!("agent-doc {}", doc.display()),
    );
    assert!(
        context.contains("cycle contract (preflight already ran"),
        "{context}"
    );
    respond(
        tmp.path(),
        &doc,
        "<!-- patch:exchange -->\n### Re: Please reply — gpt-5\n\nReplied.\n<!-- /patch:exchange -->\n",
    );
    session_check_stderr(tmp.path(), &doc);
    (tmp, doc)
}

/// GH #125 gaps 1 and 4: a chat prompt reaches the next trigger's contract as
/// a real turn, and a closeout that never records it warns with the repair.
/// The repair (gap 5) is a pure chat turn: `respond` with the canonical record
/// line on a committed cycle, no trigger, no open preflight cycle.
#[test]
fn chat_prompt_is_carried_by_the_next_contract_and_unrecorded_closeout_warns() {
    let (tmp, doc) = bound_session_with_committed_cycle("chat-session-1");
    let root = tmp.path();

    let context = codex_submit(root, "chat-session-1", "turn-2", CHAT);
    assert!(
        context.starts_with(agent_doc_prompt_contract::chat_prompt::CHAT_PROMPT_MARKER),
        "{context}"
    );

    // Gap 1: the trigger's contract carries the chat prompt; it is not idle.
    let context = codex_submit(
        root,
        "chat-session-1",
        "turn-3",
        &format!("agent-doc {}", doc.display()),
    );
    assert!(
        context.starts_with(agent_doc_prompt_contract::chat_prompt::UNRECORDED_CHAT_PROMPTS_MARKER),
        "{context}"
    );
    let contract = admitted_contract(&context);
    assert_eq!(contract["no_changes"], false, "{contract:#}");
    assert_eq!(contract["chat_prompts"], json!([CHAT]), "{contract:#}");
    assert_eq!(
        contract["diff_type"],
        agent_doc_prompt_contract::chat_prompt::CHAT_PROMPT_DIFF_TYPE
    );
    assert!(
        contract["user_intent_prompt_changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|change| change["text"] == CHAT),
        "{contract:#}"
    );
    let cycle = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
    assert!(cycle.is_open());
    assert_eq!(cycle.chat_prompts, vec![CHAT.to_string()]);

    // Gap 4: the response does not record the chat prompt -> a warning with the
    // repair, never a failed closeout.
    respond(
        root,
        &doc,
        "<!-- patch:exchange -->\n### Re: release — gpt-5\n\nNothing new.\n<!-- /patch:exchange -->\n",
    );
    let stderr = session_check_stderr(root, &doc);
    assert!(
        stderr.contains(
            agent_doc_prompt_contract::chat_prompt::UNRECORDED_CHAT_PROMPT_CLOSEOUT_MARKER
        ),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("chat_prompt: {CHAT:?}")),
        "{stderr}"
    );

    // Gap 5: the documented repair on a committed cycle with no trigger.
    let record = format!("{CHAT_RECORD_PREFIX} {CHAT}");
    let stderr = respond(
        root,
        &doc,
        &format!(
            "<!-- patch:exchange -->\n{record}\n\n### Re: release (chat) — gpt-5\n\nRecorded the chat turn.\n<!-- /patch:exchange -->\n"
        ),
    );
    assert!(stderr.contains("auto-reopened a fresh cycle"), "{stderr}");
    let head = head_document(root);
    assert!(
        head.contains(&record),
        "the record must be committed:\n{head}"
    );
    let stderr = session_check_stderr(root, &doc);
    assert!(
        !stderr.contains(
            agent_doc_prompt_contract::chat_prompt::UNRECORDED_CHAT_PROMPT_CLOSEOUT_MARKER
        ),
        "{stderr}"
    );
}

/// GH #125 gaps 3 and 5: a pure chat turn (no `agent-doc <FILE>` trigger, no
/// open preflight cycle) records the prompt by putting the canonical record at
/// the top of its `patch:exchange` response; `respond` commits it, and the
/// ledger forgets the prompt so no later trigger re-reports it.
#[test]
fn a_pure_chat_turn_records_its_prompt_through_respond_and_clears_the_ledger() {
    let (tmp, doc) = bound_session_with_committed_cycle("chat-session-2");
    let root = tmp.path();

    let context = codex_submit(root, "chat-session-2", "turn-2", CHAT);
    assert!(
        context.starts_with(agent_doc_prompt_contract::chat_prompt::CHAT_PROMPT_MARKER),
        "{context}"
    );
    let before = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
    assert!(!before.is_open(), "a pure chat turn has no open cycle");

    let record = format!("{CHAT_RECORD_PREFIX} {CHAT}");
    respond(
        root,
        &doc,
        &format!(
            "<!-- patch:exchange -->\n{record}\n\n### Re: release — gpt-5\n\nNothing new since 1.2.\n<!-- /patch:exchange -->\n"
        ),
    );
    let head = head_document(root);
    assert!(head.contains(&record), "{head}");
    assert!(head.contains("Nothing new since 1.2."), "{head}");
    let stderr = session_check_stderr(root, &doc);
    assert!(
        !stderr.contains(
            agent_doc_prompt_contract::chat_prompt::UNRECORDED_CHAT_PROMPT_CLOSEOUT_MARKER
        ),
        "{stderr}"
    );

    // Gap 3: the next trigger neither re-reports nor carries the prompt.
    let context = codex_submit(
        root,
        "chat-session-2",
        "turn-3",
        &format!("agent-doc {}", doc.display()),
    );
    assert!(
        !context.contains(agent_doc_prompt_contract::chat_prompt::UNRECORDED_CHAT_PROMPTS_MARKER),
        "{context}"
    );
    let contract = admitted_contract(&context);
    assert!(contract.get("chat_prompts").is_none(), "{contract:#}");
}

// ---------------------------------------------------------------------------
// The no-argument entrypoint runs on the pane's claim.
// ---------------------------------------------------------------------------

/// Bind `pane` to `doc` in the project registry, as `agent-doc claim` does.
fn claim_pane(root: &Path, doc: &Path, pane: &str) {
    let mut registry = agent_doc_session_registry_io::load_in(root).unwrap_or_default();
    let file = doc.strip_prefix(root).unwrap().display().to_string();
    agent_doc_session_registry::insert_registry_entry(
        root,
        &mut registry,
        agent_doc_session_registry::RegistryEntryFields {
            session_id: "pane-claim-session",
            pane_id: pane,
            file: &file,
            pid: std::process::id(),
            cwd: &root.display().to_string(),
            started: "2026-10-26T00:00:00Z",
            window: "@1",
            supervisor_instance_id: "",
        },
    );
    agent_doc_session_registry_io::save_in(root, &registry).unwrap();
}

fn submit_with_pane(
    root: &Path,
    hook: &str,
    session: &str,
    prompt: &str,
    pane: Option<&str>,
) -> String {
    let payload = json!({
        "session_id": session,
        "turn_id": "turn-1",
        "cwd": root.display().to_string(),
        "prompt": prompt,
    });
    let mut command = agent_doc();
    command
        .current_dir(root)
        .args(["hook", hook])
        .write_stdin(payload.to_string());
    match pane {
        Some(pane) => command.env("TMUX_PANE", pane),
        None => command.env_remove("TMUX_PANE"),
    };
    let submit = command.assert().success();
    let output: serde_json::Value = serde_json::from_slice(&submit.get_output().stdout)
        .expect("UserPromptSubmit stdout must be exactly one valid JSON document");
    output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("a bare trigger must never be answered with silence")
        .to_string()
}

#[test]
fn a_bare_codex_trigger_admits_and_seals_the_claimed_pane_document() {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);
    claim_pane(tmp.path(), &doc, "%42");

    let context = submit_with_pane(
        tmp.path(),
        "codex-user-prompt-submit",
        "bare-codex-session",
        "agent-doc",
        Some("%42"),
    );
    assert!(
        context.starts_with("[agent-doc] no FILE argument: this turn's FILE is `session.md`"),
        "the resolved FILE must be named before the contract: {context}"
    );
    assert!(context.contains("pane %42"), "{context}");
    admitted_contract(&context);
    assert!(!context.contains("cycle contract UNAVAILABLE"), "{context}");

    // The same hook opened the cycle on the claimed document and bound the
    // session to it, so the Stop hook can find it again.
    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("INTERRUPTED"));
    let state = agent_doc_codex_hook_io::load_state(tmp.path(), "bare-codex-session")
        .unwrap()
        .expect("the bare trigger must bind the session");
    assert_eq!(
        fs::canonicalize(&state.doc_path).unwrap(),
        fs::canonicalize(&doc).unwrap()
    );
    assert_eq!(state.preflight_admitted, Some(true));
}

#[test]
fn a_bare_claude_trigger_admits_the_claimed_pane_document() {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);
    claim_pane(tmp.path(), &doc, "%43");

    let context = submit_with_pane(
        tmp.path(),
        "preflight-user-prompt-submit",
        "bare-claude-session",
        "/agent-doc",
        Some("%43"),
    );
    assert!(
        context.starts_with("[agent-doc] no FILE argument: this turn's FILE is `session.md`"),
        "{context}"
    );
    admitted_contract(&context);
}

#[test]
fn a_bare_trigger_without_a_pane_claim_refuses_by_name_and_opens_nothing() {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);
    claim_pane(tmp.path(), &doc, "%42");

    for (pane, needle) in [
        (None, "`$TMUX_PANE` is unset"),
        (Some("%99"), "pane %99 claims no document"),
    ] {
        for (hook, prompt) in [
            ("codex-user-prompt-submit", "agent-doc"),
            ("preflight-user-prompt-submit", "/agent-doc"),
        ] {
            let context = submit_with_pane(tmp.path(), hook, "refused-session", prompt, pane);
            assert!(
                context.starts_with("[agent-doc] cycle contract UNAVAILABLE"),
                "{hook} {pane:?}: {context}"
            );
            assert!(context.contains(needle), "{hook} {pane:?}: {context}");
            assert!(
                !context.contains("cycle contract (preflight already ran"),
                "{hook} {pane:?}: a refusal must never carry the success seal"
            );
        }
    }
    assert!(
        agent_doc_cycle_state_io::load(&doc).unwrap().is_none(),
        "a refused bare trigger must not open a cycle on any document"
    );
}

#[test]
fn resolve_pane_document_cli_prints_the_claim_and_fails_closed_otherwise() {
    let (tmp, doc) = setup_template_doc();
    claim_pane(tmp.path(), &doc, "%42");

    agent_doc()
        .current_dir(tmp.path())
        .env("TMUX_PANE", "%42")
        .arg("document")
        .assert()
        .success()
        .stdout("session.md\n");
    agent_doc()
        .current_dir(tmp.path())
        .env_remove("TMUX_PANE")
        .args(["document", "--pane", "%42", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"document\":\"session.md\""));

    agent_doc()
        .current_dir(tmp.path())
        .env_remove("TMUX_PANE")
        .arg("document")
        .assert()
        .failure()
        .stderr(predicate::str::contains("`$TMUX_PANE` is unset"));
    agent_doc()
        .current_dir(tmp.path())
        .args(["document", "--pane", "%99", "--json"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"kind\":\"unclaimed\""));

    let other = tmp.path().join("other.md");
    fs::write(&other, template_doc_content()).unwrap();
    let mut registry = agent_doc_session_registry_io::load_in(tmp.path()).unwrap();
    agent_doc_session_registry::insert_registry_entry(
        tmp.path(),
        &mut registry,
        agent_doc_session_registry::RegistryEntryFields {
            session_id: "other-session",
            pane_id: "%42",
            file: "other.md",
            pid: std::process::id(),
            cwd: &tmp.path().display().to_string(),
            started: "2026-10-26T00:00:01Z",
            window: "@1",
            supervisor_instance_id: "",
        },
    );
    agent_doc_session_registry_io::save_in(tmp.path(), &registry).unwrap();
    agent_doc()
        .current_dir(tmp.path())
        .args(["document", "--pane", "%42"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("refusing to guess"));
}

/// Run one hook-admitted turn end to end for `prompt` in a fresh fixture whose
/// pane `%42` claims `session.md`: UserPromptSubmit, the session binding, and
/// the harness's Stop hook on an unanswered turn. Paths are normalised to
/// `<ROOT>` so two fixtures can be compared.
struct HookTurn {
    contract_keys: BTreeSet<String>,
    contract_fields: serde_json::Value,
    context_after_notice: String,
    preflight_admitted: Option<bool>,
    bound_doc: PathBuf,
    stop: String,
}

fn run_hook_turn(submit_hook: &str, stop_hook: &str, prompt: &str) -> HookTurn {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);
    claim_pane(tmp.path(), &doc, "%42");
    let root = tmp.path().display().to_string();
    let normalise = |text: &str| text.replace(&root, "<ROOT>");

    let context = submit_with_pane(
        tmp.path(),
        submit_hook,
        "parity-session",
        prompt,
        Some("%42"),
    );
    let contract = admitted_contract(&context);
    let contract_keys = contract
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let contract_fields = json!({
        "no_changes": contract["no_changes"],
        "diff_type": contract["diff_type"],
        "user_intent_prompt_changes": contract["user_intent_prompt_changes"],
        "chat_prompts": contract.get("chat_prompts"),
    });
    let context_after_notice = context
        .strip_prefix("[agent-doc] no FILE argument: ")
        .map(|rest| rest.split_once('\n').unwrap().1.to_string())
        .unwrap_or_else(|| context.clone());
    // The contract body itself carries per-run ids and timestamps; what must
    // match is everything around it: the seal and the harness directive.
    let seal_onward = context_after_notice
        .find("\n[agent-doc] cycle contract")
        .map(|at| normalise(&context_after_notice[at..]))
        .unwrap();

    let state = agent_doc_codex_hook_io::load_state(tmp.path(), "parity-session")
        .unwrap()
        .expect("the turn must bind the session for its Stop hook");
    let bound_doc = fs::canonicalize(&state.doc_path)
        .unwrap()
        .strip_prefix(fs::canonicalize(tmp.path()).unwrap())
        .unwrap()
        .to_path_buf();

    let stop_payload = json!({
        "session_id": "parity-session",
        "turn_id": "turn-1",
        "cwd": root,
        "last_assistant_message": "Stopping without a response.",
        "stop_hook_active": false,
    });
    let stop = agent_doc()
        .current_dir(tmp.path())
        .env("TMUX_PANE", "%42")
        .args(["hook", stop_hook])
        .write_stdin(stop_payload.to_string())
        .assert()
        .success();
    let stop = normalise(&String::from_utf8_lossy(&stop.get_output().stdout));

    HookTurn {
        contract_keys,
        contract_fields,
        context_after_notice: seal_onward,
        preflight_admitted: state.preflight_admitted,
        bound_doc,
        stop,
    }
}

/// The operator requirement, per hook harness: the bare entrypoint behaves
/// exactly like `<entrypoint> <FILE>`: same admitted contract shape, same seal
/// and directive, same session binding, and the same Stop-hook decision on the
/// turn.
#[test]
fn a_bare_trigger_matches_the_file_trigger_through_admission_and_stop() {
    for (harness, submit_hook, stop_hook, bare, with_file) in [
        (
            "codex",
            "codex-user-prompt-submit",
            "codex-stop",
            "agent-doc",
            "agent-doc session.md",
        ),
        (
            "claude",
            "preflight-user-prompt-submit",
            "claude-stop",
            "/agent-doc",
            "/agent-doc session.md",
        ),
    ] {
        let bare_turn = run_hook_turn(submit_hook, stop_hook, bare);
        let file_turn = run_hook_turn(submit_hook, stop_hook, with_file);
        assert_eq!(
            bare_turn.contract_keys, file_turn.contract_keys,
            "{harness}"
        );
        assert_eq!(
            bare_turn.contract_fields, file_turn.contract_fields,
            "{harness}"
        );
        assert_eq!(
            bare_turn.context_after_notice, file_turn.context_after_notice,
            "{harness}: seal and directive must match"
        );
        if harness == "codex" {
            assert_eq!(bare_turn.preflight_admitted, Some(true), "{harness}");
        }
        assert_eq!(
            bare_turn.preflight_admitted, file_turn.preflight_admitted,
            "{harness}"
        );
        assert_eq!(
            bare_turn.bound_doc,
            PathBuf::from("session.md"),
            "{harness}"
        );
        assert_eq!(bare_turn.bound_doc, file_turn.bound_doc, "{harness}");
        assert_eq!(
            bare_turn.stop, file_turn.stop,
            "{harness}: the Stop hook must decide the bare turn as it does the FILE turn"
        );
    }
}
