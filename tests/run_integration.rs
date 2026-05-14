use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::Duration;
use tempfile::TempDir;

fn agent_doc() -> Command {
    cargo_bin_cmd!("agent-doc")
}

fn init_git_repo(root: &Path, tracked: &Path) {
    fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/pending")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/locks")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/state/cycles")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/captures")).unwrap();
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

fn write_mock_agent(root: &Path, response: &str) -> PathBuf {
    let script = root.join("mock-agent.sh");
    let payload = serde_json::json!({
        "result": response,
        "session_id": "sess-123"
    })
    .to_string();
    fs::write(
        &script,
        format!("#!/bin/sh\ncat >/dev/null\ncat <<'JSON'\n{payload}\nJSON\n"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
    }
    script
}

fn write_mock_streaming_agent(root: &Path) -> PathBuf {
    let script = root.join("mock-streaming-agent.sh");
    fs::write(
        &script,
        "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"<!-- patch:exchange -->\\n### Re: orchestrate streaming — gpt-5\\n\"}]}}'\nsleep 1\nprintf '%s\\n' '{\"type\":\"result\",\"result\":\"<!-- patch:exchange -->\\n### Re: orchestrate streaming — gpt-5\\n\\nImplemented and verified.\\n\\nVerification:\\n- `cargo test`\\n<!-- /patch:exchange -->\\n\",\"session_id\":\"sess-stream\"}'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
    }
    script
}

fn write_sleeping_agent(root: &Path) -> PathBuf {
    let script = root.join("mock-sleeping-agent.sh");
    fs::write(&script, "#!/bin/sh\ncat >/dev/null\nsleep 30\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
    }
    script
}

fn write_marker_agent(root: &Path, response: &str) -> (PathBuf, PathBuf) {
    let script = root.join("mock-marker-agent.sh");
    let marker = root.join("mock-agent-called");
    let payload = serde_json::json!({
        "result": response,
        "session_id": "sess-marker"
    })
    .to_string();
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf called > '{}'\ncat >/dev/null\ncat <<'JSON'\n{payload}\nJSON\n",
            marker.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
    }
    (script, marker)
}

fn write_delayed_mock_agent(root: &Path, response: &str, delay_secs: u64) -> PathBuf {
    let script = root.join("mock-delayed-agent.sh");
    let payload = serde_json::json!({
        "result": response,
        "session_id": "sess-delayed"
    })
    .to_string();
    fs::write(
        &script,
        format!("#!/bin/sh\ncat >/dev/null\nsleep {delay_secs}\ncat <<'JSON'\n{payload}\nJSON\n"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
    }
    script
}

fn write_config(root: &Path, script: &Path) -> PathBuf {
    let config_root = root.join("config");
    let agent_doc_dir = config_root.join("agent-doc");
    fs::create_dir_all(&agent_doc_dir).unwrap();
    fs::write(
        agent_doc_dir.join("config.toml"),
        format!(
            "default_agent = \"mock\"\n\n[agents.mock]\ncommand = \"{}\"\nargs = []\n",
            script.display()
        ),
    )
    .unwrap();
    config_root
}

fn write_claude_config(root: &Path, script: &Path) -> PathBuf {
    let config_root = root.join("config");
    let agent_doc_dir = config_root.join("agent-doc");
    fs::create_dir_all(&agent_doc_dir).unwrap();
    fs::write(
        agent_doc_dir.join("config.toml"),
        format!(
            "default_agent = \"claude\"\n\n[agents.claude]\ncommand = \"{}\"\nargs = []\n",
            script.display()
        ),
    )
    .unwrap();
    config_root
}

fn template_doc() -> String {
    "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n❯ Please reply\n<!-- /agent:exchange -->\n\n## Pending\n\n<!-- agent:pending -->\n<!-- /agent:pending -->\n".to_string()
}

fn append_doc() -> String {
    "---\nagent_doc_format: append\nagent_doc_write: merge\n---\n\n# Session\n\n## User\n\nPlease reply\n".to_string()
}

fn read_cycle_phase(root: &Path) -> String {
    let state_dir = root.join(".agent-doc/state/cycles");
    let entry = fs::read_dir(&state_dir)
        .unwrap()
        .next()
        .expect("expected cycle state file")
        .unwrap();
    let value: Value = serde_json::from_str(&fs::read_to_string(entry.path()).unwrap()).unwrap();
    value["phase"].as_str().unwrap().to_string()
}

fn read_cycle_state(root: &Path) -> Value {
    let state_dir = root.join(".agent-doc/state/cycles");
    let entry = fs::read_dir(&state_dir)
        .unwrap()
        .next()
        .expect("expected cycle state file")
        .unwrap();
    serde_json::from_str(&fs::read_to_string(entry.path()).unwrap()).unwrap()
}

fn seed_snapshot(root: &Path, doc: &Path) {
    let canonical = doc.canonicalize().unwrap();
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let hash = hex::encode(hasher.finalize());
    let snapshot = root.join(".agent-doc/snapshots").join(format!("{hash}.md"));
    fs::write(snapshot, fs::read_to_string(doc).unwrap()).unwrap();
}

fn template_doc_with_model() -> String {
    "---\nagent_doc_format: template\nagent_doc_write: crdt\nmodel: gpt-5\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n❯ Please reply\n<!-- /agent:exchange -->\n\n## Pending\n\n<!-- agent:pending -->\n<!-- /agent:pending -->\n".to_string()
}

#[test]
fn run_template_mode_writes_inside_exchange_and_commits() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, template_doc()).unwrap();
    init_git_repo(tmp.path(), &doc);

    let script = write_mock_agent(
        tmp.path(),
        "<!-- patch:exchange -->\n### Re: topic — gpt-5\nbody\n<!-- /patch:exchange -->\n",
    );
    let config_root = write_config(tmp.path(), &script);

    agent_doc()
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .args(["run", doc.to_str().unwrap()])
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    let response_pos = content.find("### Re: topic — gpt-5").unwrap();
    let exchange_end = content.find("<!-- /agent:exchange -->").unwrap();
    assert!(
        response_pos < exchange_end,
        "response should stay inside exchange"
    );
    assert!(
        !content[exchange_end..].contains("### Re: topic — gpt-5"),
        "response should not be appended after exchange"
    );
    assert!(content.contains("resume: sess-123"));

    let head = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    let head_blob = String::from_utf8_lossy(&head.stdout);
    assert!(head_blob.contains("### Re: topic — gpt-5"));
    assert!(head_blob.contains("resume: sess-123"));
    assert_eq!(read_cycle_phase(tmp.path()), "committed");
}

#[test]
fn bare_path_alias_uses_same_template_safe_path() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, template_doc()).unwrap();
    init_git_repo(tmp.path(), &doc);

    let script = write_mock_agent(
        tmp.path(),
        "<!-- patch:exchange -->\n### Re: bare path — gpt-5\nbody\n<!-- /patch:exchange -->\n",
    );
    let config_root = write_config(tmp.path(), &script);

    agent_doc()
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .arg(doc.to_str().unwrap())
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    let response_pos = content.find("### Re: bare path — gpt-5").unwrap();
    let exchange_end = content.find("<!-- /agent:exchange -->").unwrap();
    assert!(
        response_pos < exchange_end,
        "bare path should stay inside exchange"
    );
}

#[test]
fn run_times_out_agent_child_and_marks_recoverable_preflight() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, template_doc()).unwrap();
    init_git_repo(tmp.path(), &doc);

    let script = write_sleeping_agent(tmp.path());
    let config_root = write_config(tmp.path(), &script);

    agent_doc()
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .env("AGENT_DOC_RUN_AGENT_TIMEOUT_SECS", "2")
        .env("AGENT_DOC_RUN_HEARTBEAT_SECS", "1")
        .args(["run", doc.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "[run] heartbeat phase=child_agent_wait",
        ))
        .stderr(predicate::str::contains("timed out after waiting 2s"));

    let state = read_cycle_state(tmp.path());
    assert_eq!(state["phase"].as_str().unwrap(), "preflight_started");
    assert!(
        state["last_event"]
            .as_str()
            .unwrap()
            .contains("direct_invocation_timeout")
    );
}

#[test]
fn run_stops_before_child_dispatch_when_precommit_consumes_stale_repair_diff() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    let stale_snapshot = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n",
        "old body\n",
        "<!-- /agent:exchange -->\n",
    );
    fs::write(&doc, stale_snapshot).unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc);

    let committed_repair = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n",
        "old body\n",
        "### Re: repaired — gpt-5\n",
        "already committed body\n",
        "<!-- /agent:exchange -->\n",
    );
    fs::write(&doc, committed_repair).unwrap();
    ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["add", "session.md"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["commit", "-m", "manual repair", "--no-verify"])
        .status()
        .unwrap();

    let (script, marker) = write_marker_agent(
        tmp.path(),
        "<!-- patch:exchange -->\n### Re: should not run — gpt-5\nbody\n<!-- /patch:exchange -->\n",
    );
    let config_root = write_config(tmp.path(), &script);

    agent_doc()
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .args(["run", doc.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no child-agent dispatch"))
        .stderr(predicate::str::contains("agent-doc write --commit"));

    assert!(
        !marker.exists(),
        "pre-commit repair consumed the diff; child agent must not be invoked"
    );
    let content = fs::read_to_string(&doc).unwrap();
    assert!(!content.contains("should not run"));
    assert_eq!(read_cycle_phase(tmp.path()), "committed");
}

#[test]
fn run_heartbeats_are_visible_and_persisted_while_child_is_waiting() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, template_doc()).unwrap();
    init_git_repo(tmp.path(), &doc);

    let script = write_delayed_mock_agent(
        tmp.path(),
        "<!-- patch:exchange -->\n### Re: delayed — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        3,
    );
    let config_root = write_config(tmp.path(), &script);
    let bin = std::env::var("CARGO_BIN_EXE_agent-doc").unwrap();

    let mut child = ProcessCommand::new(bin)
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .env("AGENT_DOC_RUN_AGENT_TIMEOUT_SECS", "10")
        .env("AGENT_DOC_RUN_HEARTBEAT_SECS", "1")
        .args(["run", doc.to_str().unwrap()])
        .spawn()
        .unwrap();

    let mut saw_persisted_heartbeat = false;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if let Ok(state) = std::panic::catch_unwind(|| read_cycle_state(tmp.path()))
            && state["last_event"]
                .as_str()
                .unwrap_or("")
                .contains("run_heartbeat phase=child_agent_wait")
        {
            saw_persisted_heartbeat = true;
            break;
        }
    }

    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(
        saw_persisted_heartbeat,
        "expected run heartbeat to update cycle progress while the child was still running"
    );

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: delayed — gpt-5"));
    assert_eq!(read_cycle_phase(tmp.path()), "committed");
}

#[test]
fn codex_bare_run_inside_owning_pane_fails_before_nested_dispatch() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(
        &doc,
        "---\nagent_doc_session: session-recursive\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n❯ Please reply\n<!-- /agent:exchange -->\n",
    )
    .unwrap();
    init_git_repo(tmp.path(), &doc);
    fs::write(
        tmp.path().join(".agent-doc/sessions.json"),
        format!(
            "{{\n  \"session-recursive\": {{\n    \"pane\": \"%77\",\n    \"pid\": 123,\n    \"cwd\": \"{}\",\n    \"started\": \"2026-05-10T00:00:00Z\",\n    \"session_id\": \"session-recursive\",\n    \"file\": \"{}\",\n    \"window\": \"@7\",\n    \"supervisor_instance_id\": \"test-supervisor\"\n  }}\n}}\n",
            tmp.path().display(),
            doc.display()
        ),
    )
    .unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .env("CODEX_SESSION", "codex-session")
        .env("TMUX_PANE", "%77")
        .arg(doc.to_str().unwrap())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "recursive direct invocation would deadlock",
        ));

    let state = read_cycle_state(tmp.path());
    assert_eq!(state["phase"].as_str().unwrap(), "preflight_started");
    assert!(
        state["last_event"]
            .as_str()
            .unwrap()
            .contains("recursive_direct_invocation_blocked")
    );
}

#[test]
fn orchestrate_handles_already_open_preflight_cycle_for_first_step() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, template_doc_with_model()).unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc);

    let edited = fs::read_to_string(&doc).unwrap().replace(
        "❯ Please reply\n",
        "❯ Please reply\n\nSynchronous orchestra:\n",
    );
    fs::write(&doc, edited).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();

    assert_eq!(read_cycle_phase(tmp.path()), "preflight_started");

    let script = write_mock_agent(
        tmp.path(),
        "<!-- patch:exchange -->\n### Re: orchestrate step — gpt-5\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /patch:exchange -->\n",
    );
    let config_root = write_config(tmp.path(), &script);

    agent_doc()
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .args([
            "orchestrate",
            doc.to_str().unwrap(),
            "--mode",
            "sequential",
            "--task",
            "do #opcc. update spec + tests. build + install for local testing. commit + push",
        ])
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains(
        "❯ do #opcc. update spec + tests. build + install for local testing. commit + push"
    ));
    assert!(content.contains("### Re: orchestrate step — gpt-5"));
    assert_eq!(read_cycle_phase(tmp.path()), "committed");
}

#[test]
fn orchestrate_streams_step_patchback_before_finalize() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, template_doc_with_model()).unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc);

    let script = write_mock_streaming_agent(tmp.path());
    let config_root = write_claude_config(tmp.path(), &script);
    let bin = std::env::var("CARGO_BIN_EXE_agent-doc").unwrap();

    let mut child = ProcessCommand::new(bin)
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .args([
            "orchestrate",
            doc.to_str().unwrap(),
            "--mode",
            "sequential",
            "--task",
            "do #4qja. update spec + tests. build + install for local testing. commit + push",
        ])
        .spawn()
        .unwrap();

    let mut saw_partial = false;
    for _ in 0..60 {
        std::thread::sleep(Duration::from_millis(100));
        let content = fs::read_to_string(&doc).unwrap();
        let has_heading = content.contains("### Re: orchestrate streaming — gpt-5");
        let has_full_body = content.contains("Implemented and verified.");
        if has_heading && !has_full_body {
            saw_partial = true;
            assert!(
                child.try_wait().unwrap().is_none(),
                "partial streamed patchback should land before orchestrate exits"
            );
            break;
        }
    }

    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(
        saw_partial,
        "expected partial streamed patchback before finalize"
    );

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains(
        "❯ do #4qja. update spec + tests. build + install for local testing. commit + push"
    ));
    assert!(content.contains("### Re: orchestrate streaming — gpt-5"));
    assert!(content.contains("Implemented and verified."));
    assert_eq!(
        content
            .matches("### Re: orchestrate streaming — gpt-5")
            .count(),
        1,
        "streamed step response should not be duplicated by finalize"
    );
}

#[test]
fn orchestrate_accepts_clean_plain_template_response() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, template_doc_with_model()).unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc);

    let plain_response = "### Re: clean orchestrate closeout — gpt-5\n\nImplemented and verified.\n\nVerification:\n- `cargo test`\n";
    let script = write_mock_agent(tmp.path(), plain_response);
    let config_root = write_config(tmp.path(), &script);

    agent_doc()
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .args([
            "orchestrate",
            doc.to_str().unwrap(),
            "--mode",
            "sequential",
            "--task",
            "do #orplain1. update spec + tests. build + install for local testing. commit + push",
        ])
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains(
        "❯ do #orplain1. update spec + tests. build + install for local testing. commit + push"
    ));
    assert!(content.contains("### Re: clean orchestrate closeout — gpt-5"));
    assert!(content.contains("Implemented and verified."));
    assert_eq!(
        content
            .matches("### Re: clean orchestrate closeout — gpt-5")
            .count(),
        1,
        "plain orchestrate closeout should be synthesized once into exchange"
    );
    assert_eq!(read_cycle_phase(tmp.path()), "committed");
}

#[test]
fn orchestrate_rejects_raw_template_transcript_response() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, template_doc_with_model()).unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc);

    let raw_response = "❯ do #orfmt1. update spec + tests. build + install for local testing. commit + push\n### Re: malformed closeout — gpt-5\nImplemented and verified.\n\nVerification:\n- `cargo test`\nCommit / push:\n- `abc1234`\nThis raw transcript must not be synthesized.\n";
    let script = write_mock_agent(tmp.path(), raw_response);
    let config_root = write_config(tmp.path(), &script);

    agent_doc()
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .args([
            "orchestrate",
            doc.to_str().unwrap(),
            "--mode",
            "sequential",
            "--task",
            "do #orfmt1. update spec + tests. build + install for local testing. commit + push",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("transcript prompt lines"));

    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        content
            .matches("❯ do #orfmt1. update spec + tests. build + install for local testing. commit + push")
            .count(),
        1,
        "raw orchestrate transcript must not be replayed into exchange"
    );
    assert!(
        !content.contains("This raw transcript must not be synthesized."),
        "malformed raw content should not be written to the document"
    );
}

#[test]
fn run_append_mode_keeps_inline_response_shape() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, append_doc()).unwrap();
    init_git_repo(tmp.path(), &doc);

    let script = write_mock_agent(tmp.path(), "Append answer.");
    let config_root = write_config(tmp.path(), &script);

    agent_doc()
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .args(["run", doc.to_str().unwrap()])
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("## Assistant\n\nAppend answer."));
    assert!(content.contains("resume: sess-123"));
    assert_eq!(read_cycle_phase(tmp.path()), "committed");
}

#[test]
fn interrupted_run_leaves_write_applied_and_preflight_finishes_commit() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, template_doc()).unwrap();
    init_git_repo(tmp.path(), &doc);

    let script = write_mock_agent(
        tmp.path(),
        "<!-- patch:exchange -->\n### Re: interrupted closeout — gpt-5\nbody\n<!-- /patch:exchange -->\n",
    );
    let config_root = write_config(tmp.path(), &script);

    agent_doc()
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .env("AGENT_DOC_TEST_ABORT_AFTER_RUN_WRITE_APPLIED", "1")
        .args(["run", doc.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "test abort after run write_applied",
        ));

    let content_after_abort = fs::read_to_string(&doc).unwrap();
    assert!(
        content_after_abort.contains("### Re: interrupted closeout — gpt-5"),
        "response should already be in the document after the simulated abort"
    );
    assert_eq!(read_cycle_phase(tmp.path()), "write_applied");

    agent_doc()
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();

    let repaired = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        repaired
            .matches("### Re: interrupted closeout — gpt-5")
            .count(),
        1,
        "preflight recovery should finish the pending commit without duplicating the response"
    );
    assert_eq!(read_cycle_phase(tmp.path()), "committed");

    let head = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    let head_blob = String::from_utf8_lossy(&head.stdout);
    assert!(
        head_blob.contains("### Re: interrupted closeout — gpt-5"),
        "HEAD should contain the recovered response"
    );
}
