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

fn auto_queue_doc_content() -> String {
    "---\nagent_doc_session: testsid\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n### Re: prior — gpt-5\n\nDone.\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue auto -->\n- do [#seopdp] deploy product page\n<!-- /agent:queue -->\n".to_string()
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
        .args([
            "session-check",
            doc.to_str().unwrap(),
            "--codex-final-gate",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("queue_continuation_required=true"));
}

#[test]
fn codex_hook_cli_auto_closes_open_cycle_after_user_prompt_submit() {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("INTERRUPTED"));

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
        .success();

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
fn codex_hook_cli_blocks_transcript_shaped_last_assistant_message() {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();

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
        .success();

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
