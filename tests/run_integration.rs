use agent_doc_controller_io::project_controller;
use agent_doc_cycle_state_io as cycle_state;
use agent_doc_hash::content_hash;
use agent_doc_state_backbone as state_backbone;
use agent_doc_turn::turn_scope::{Address, TurnScope};
use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value;
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

fn write_counting_queue_agent(root: &Path) -> (PathBuf, PathBuf) {
    let script = root.join("mock-counting-queue-agent.sh");
    let counter = root.join("mock-counting-queue-agent.count");
    fs::write(
        &script,
        format!(
            r#"#!/bin/sh
cat >/dev/null
count_file='{counter}'
n=0
if [ -f "$count_file" ]; then
  n="$(cat "$count_file")"
fi
n=$((n + 1))
printf '%s' "$n" > "$count_file"
printf '{{"result":"<!-- patch:exchange -->\\n### Re: queue item %s — gpt-5\\n\\nImplemented and verified.\\n\\nVerification:\\n- `cargo test`\\n<!-- /patch:exchange -->\\n","session_id":"sess-%s"}}\n' "$n" "$n"
"#,
            counter = counter.display()
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
    (script, counter)
}

fn write_arg_logging_queue_agent(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let script = root.join("mock-arg-logging-queue-agent.sh");
    let counter = root.join("mock-arg-logging-queue-agent.count");
    let args_log = root.join("mock-arg-logging-queue-agent.args");
    fs::write(
        &script,
        format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> '{args_log}'
cat >/dev/null
count_file='{counter}'
n=0
if [ -f "$count_file" ]; then
  n="$(cat "$count_file")"
fi
n=$((n + 1))
printf '%s' "$n" > "$count_file"
printf '{{"result":"<!-- patch:exchange -->\\n### Re: queue item %s — gpt-5\\n\\nImplemented and verified.\\n<!-- /patch:exchange -->\\n","session_id":"sess-%s"}}\n' "$n" "$n"
"#,
            args_log = args_log.display(),
            counter = counter.display()
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
    (script, counter, args_log)
}

fn write_mock_streaming_agent(root: &Path) -> (PathBuf, PathBuf) {
    let script = root.join("mock-streaming-agent.sh");
    let release = root.join("mock-streaming-agent.release");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"<!-- patch:exchange -->\\n### Re: orchestrate streaming — gpt-5\\n\"}}]}}}}'\ni=0\nwhile [ ! -f '{release}' ] && [ \"$i\" -lt 300 ]; do\n  i=$((i + 1))\n  sleep 0.1\ndone\nprintf '%s\\n' '{{\"type\":\"result\",\"result\":\"<!-- patch:exchange -->\\n### Re: orchestrate streaming — gpt-5\\n\\nImplemented and verified.\\n\\nVerification:\\n- `cargo test`\\n<!-- /patch:exchange -->\\n\",\"session_id\":\"sess-stream\"}}'\n",
            release = release.display()
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
    (script, release)
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
    "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n❯ Please reply\n<!-- /agent:exchange -->\n\n## Pending\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n".to_string()
}

fn template_doc_with_session(session_id: &str) -> String {
    template_doc().replacen(
        "---\n",
        &format!("---\nagent_doc_session: {session_id}\n"),
        1,
    )
}

fn run_heartbeat_persisted(root: &Path, doc: &Path, session_id: &str) -> bool {
    if let Ok(Some(state)) = cycle_state::load(doc)
        && state
            .last_event
            .contains("run_heartbeat phase=child_agent_wait")
    {
        return true;
    }
    let log_path = root
        .join(".agent-doc/logs")
        .join(format!("{session_id}.log"));
    fs::read_to_string(log_path).is_ok_and(|log| {
        log.contains("document_cycle") && log.contains("run_heartbeat phase=child_agent_wait")
    })
}

fn append_doc() -> String {
    "---\nagent_doc_format: append\nagent_doc_write: merge\n---\n\n# Session\n\n## User\n\nPlease reply\n".to_string()
}

fn read_cycle_phase(file: &Path) -> String {
    read_cycle_state(file).phase.as_str().to_string()
}

fn read_cycle_state(file: &Path) -> cycle_state::CycleState {
    cycle_state::load(file)
        .unwrap()
        .expect("expected cycle state file")
}

fn assert_terminal_closeout_proof(_root: &Path, doc: &Path) {
    let proof = cycle_state::load_latest_terminal_closeout_proof(doc)
        .unwrap()
        .expect("expected typed terminal closeout proof projection");
    assert!(proof.last_event.contains("commit"));
    assert!(proof.did_commit);
    assert_eq!(proof.file_hash, proof.snapshot_hash);
    assert_eq!(proof.snapshot_hash, proof.head_hash);
    assert_eq!(proof.agreement, "file_snapshot_head");
}

fn seed_snapshot(root: &Path, doc: &Path) {
    let canonical = doc.canonicalize().unwrap();
    let hash = content_hash(canonical.to_string_lossy().as_ref());
    let snapshot = root.join(".agent-doc/snapshots").join(format!("{hash}.md"));
    fs::write(snapshot, fs::read_to_string(doc).unwrap()).unwrap();
}

fn record_selected_queue_head(root: &Path, doc: &Path, content: &str, prompt_text: &str) {
    let node_key = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
        .unwrap()
        .into_iter()
        .find(|node| !node.item.struck)
        .expect("queue head should have a node key")
        .node_key;
    let document_hash = agent_doc_fs::document_state_hash(&doc.canonicalize().unwrap()).unwrap();
    let event = state_backbone::StateEvent::new(
        format!("test-selected-queue-head:{node_key}"),
        state_backbone::StateFact::QueueHeadSelected {
            document_hash,
            node_key,
            backlog_id: None,
            prompt_text: Some(prompt_text.to_string()),
            drainable: true,
            hosting_epoch: None,
        },
    );
    project_controller::append_state_event(root, &event).unwrap();
}

fn template_doc_with_model() -> String {
    "---\nagent_doc_format: template\nagent_doc_write: crdt\nmodel: gpt-5\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n❯ Please reply\n<!-- /agent:exchange -->\n\n## Pending\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n".to_string()
}

fn active_auto_queue_doc() -> String {
    "---\nagent_doc_format: template\nagent_doc_write: crdt\nagent: mock\nmodel: gpt-5\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue auto go -->\n- do #fix1\n- do #fix2\n- do #fix3\n<!-- /agent:queue -->\n\n## Pending\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n".to_string()
}

fn active_large_auto_queue_doc_with_resume() -> String {
    let exchange_lines = (0..170)
        .map(|idx| format!("prior exchange line {idx}"))
        .collect::<Vec<_>>()
        .join("\n");
    // `#nm1x-codex-clear-parity`: the run-path accretion-driven fresh-session
    // decision is now gated on the `agent_doc_queue_context_reset` opt-in (off by
    // default, product-wide). This fixture exercises the fresh-session behavior,
    // so it must explicitly opt in; without it the run path never starts a fresh
    // agent session before the next queue head.
    format!(
        "---\nagent_doc_format: template\nagent_doc_write: crdt\nagent: mock\nmodel: gpt-5\nresume: old-session\nqueue_active: true\nagent_doc_queue_context_reset: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{exchange_lines}\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue auto go -->\n- do #fix1\n- do #fix2\n<!-- /agent:queue -->\n\n## Pending\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n"
    )
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
        .args(["run", "--force-disk", doc.to_str().unwrap()])
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
    assert_eq!(read_cycle_phase(&doc), "committed");
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
        // Bare `agent-doc <FILE>` is a harness-native alias. Simulate Codex
        // explicitly so this positive path is deterministic in plain CI shells.
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE")
        .env_remove("CLAUDE_CODE_SESSION")
        .env_remove("OPENCODE")
        .env_remove("OPENCODE_CLIENT")
        .env_remove("CODEX")
        .env_remove("CODEX_CLI")
        .env_remove("CODEX_THREAD_ID")
        .env("CODEX_SESSION", "codex-session")
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
fn run_auto_queue_continues_until_drained() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    let active_doc = active_auto_queue_doc();
    fs::write(&doc, &active_doc).unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc);
    record_selected_queue_head(tmp.path(), &doc, &active_doc, "do #fix1");

    let (script, counter) = write_counting_queue_agent(tmp.path());
    let config_root = write_config(tmp.path(), &script);

    agent_doc()
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .args(["run", "--force-disk", doc.to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "[run] active queue head synthesized as prompt diff",
        ))
        .stderr(predicate::str::contains(
            "[queue] queue continuation: completed 1 item(s); launching next prompt: \"do #fix2\"",
        ))
        .stderr(predicate::str::contains(
            "[queue] queue continuation: completed 2 item(s); launching next prompt: \"do #fix3\"",
        ))
        .stderr(predicate::str::contains("[queue] drained"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("### Re: queue item 1 — gpt-5"),
        "first queue response should be written"
    );
    assert!(
        content.contains("### Re: queue item 2 — gpt-5"),
        "second queue response should be written"
    );
    assert!(
        content.contains("### Re: queue item 3 — gpt-5"),
        "third queue response should be written"
    );
    assert!(
        content.contains("queue: stop"),
        "queue should clear active state after all prompts are consumed"
    );
    assert!(!content.contains("agent:queue auto"));
    // #queue-prompt-echo-in-response: consumed prompts are drained from the queue
    // but embedded (blockquoted) into their response blocks.
    let queue_section = queue_section_of(&content);
    assert!(
        !queue_section.contains("do #fix1"),
        "queue:\n{queue_section}"
    );
    assert!(
        !queue_section.contains("do #fix2"),
        "queue:\n{queue_section}"
    );
    assert!(
        !queue_section.contains("do #fix3"),
        "queue:\n{queue_section}"
    );
    assert!(content.contains("> do #fix1"), "response echo:\n{content}");
    assert!(content.contains("> do #fix2"), "response echo:\n{content}");
    assert!(content.contains("> do #fix3"), "response echo:\n{content}");
    assert_eq!(fs::read_to_string(counter).unwrap(), "3");
}

#[test]
fn run_auto_queue_starts_fresh_backend_session_after_accretion_threshold() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    let active_doc = active_large_auto_queue_doc_with_resume();
    fs::write(&doc, &active_doc).unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc);
    record_selected_queue_head(tmp.path(), &doc, &active_doc, "do #fix1");

    let (script, counter, args_log) = write_arg_logging_queue_agent(tmp.path());
    let config_root = write_config(tmp.path(), &script);

    let assert = agent_doc()
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .args(["run", "--force-disk", doc.to_str().unwrap()])
        .assert()
        .success();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("queue continuation will start a fresh agent session"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("[run] queue context reset: starting a fresh agent session"),
        "stderr:\n{stderr}"
    );

    let args = fs::read_to_string(&args_log).unwrap();
    let arg_lines: Vec<&str> = args.lines().collect();
    assert_eq!(arg_lines.len(), 2, "args log:\n{args}");
    assert!(
        arg_lines[0].contains("--resume old-session"),
        "first queue dispatch should resume the existing backend session: {args}"
    );
    assert!(
        !arg_lines[1].contains("--resume"),
        "second queue dispatch should not resume after context reset: {args}"
    );
    assert!(
        arg_lines[1].contains("--fork-session"),
        "fresh queue dispatch should fork a new backend session: {args}"
    );
    assert_eq!(fs::read_to_string(counter).unwrap(), "2");
}

fn queue_section_of(content: &str) -> String {
    content
        .split_once("<!-- agent:queue")
        .and_then(|(_, rest)| rest.split_once("<!-- /agent:queue -->"))
        .map(|(body, _)| body.to_string())
        .unwrap_or_default()
}

fn active_persisted_queue_doc() -> String {
    // Persisted-active queue: `queue_active: true` but the opening tag is plain
    // `<!-- agent:queue -->` (no `go`). `#active-queue-persisted-no-continue`.
    "---\nagent_doc_format: template\nagent_doc_write: crdt\nagent: mock\nmodel: gpt-5\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n- do #fix1\n- do #fix2\n- do #fix3\n<!-- /agent:queue -->\n\n## Pending\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n".to_string()
}

#[test]
fn run_persisted_active_plain_queue_without_go_does_not_continue() {
    // `#active-queue-persisted-no-continue`: an already-active queue without
    // explicit `go` mode is inert. `queue_active: true` is persisted state, not a
    // continuation signal by itself.
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    let active_doc = active_persisted_queue_doc();
    fs::write(&doc, &active_doc).unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc);
    record_selected_queue_head(tmp.path(), &doc, &active_doc, "do #fix1");

    let (script, counter) = write_counting_queue_agent(tmp.path());
    let config_root = write_config(tmp.path(), &script);

    let assert = agent_doc()
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .args(["run", "--force-disk", doc.to_str().unwrap()])
        .assert()
        .success();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        !stderr.contains("[run] active queue head synthesized as prompt diff"),
        "plain persisted queue must not synthesize a queue prompt:\n{stderr}"
    );

    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(content, active_doc);
    let queue_section = queue_section_of(&content);
    assert!(
        queue_section.contains("do #fix1"),
        "queue:\n{queue_section}"
    );
    assert!(
        !counter.exists(),
        "mock queue agent must not be invoked for a non-go persisted queue"
    );
}

#[test]
fn run_auto_queue_stop_fence_halts_continuation_before_next_prompt() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    let active_doc = "---\nagent_doc_format: template\nagent_doc_write: crdt\nagent: mock\nmodel: gpt-5\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue auto go -->\n- do #fix1\n--- stop\n- do #fix2\n<!-- /agent:queue -->\n\n## Pending\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n";
    fs::write(&doc, active_doc).unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc);
    record_selected_queue_head(tmp.path(), &doc, active_doc, "do #fix1");

    let (script, counter) = write_counting_queue_agent(tmp.path());
    let config_root = write_config(tmp.path(), &script);

    agent_doc()
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .args(["run", "--force-disk", doc.to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "[queue] queue continuation stopped after 1 completed item(s): stop_fence before next prompt Some(\"do #fix2\")",
        ));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("### Re: queue item 1 — gpt-5"),
        "first queue response should be written"
    );
    assert!(
        !content.contains("### Re: queue item 2 — gpt-5"),
        "stop fence should prevent the second queue response"
    );
    assert!(content.contains("- ~~do #fix1~~"));
    assert!(content.contains("--- stop"));
    assert!(content.contains("- do #fix2"));
    assert!(
        content.contains("queue: start"),
        "queue remains active (canonical `queue: start`) but halted by the stop fence"
    );
    assert_eq!(fs::read_to_string(counter).unwrap(), "1");
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

    let state = read_cycle_state(&doc);
    assert_eq!(state.phase.as_str(), "preflight_started");
    assert!(state.last_event.contains("direct_invocation_timeout"));
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
    assert_eq!(read_cycle_phase(&doc), "committed");
}

#[test]
fn run_heartbeats_are_visible_and_persisted_while_child_is_waiting() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    let session_id = "test-heartbeat-visible";
    fs::write(&doc, template_doc_with_session(session_id)).unwrap();
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
        .env("AGENT_DOC_TMUX_INPUT_DIAG", "1")
        .env("AGENT_DOC_RUN_AGENT_TIMEOUT_SECS", "10")
        .env("AGENT_DOC_RUN_HEARTBEAT_SECS", "1")
        .args(["run", doc.to_str().unwrap()])
        .spawn()
        .unwrap();

    let mut saw_persisted_heartbeat = false;
    // Under full nextest load, startup/preflight can consume most of a short
    // polling window before the child-agent wait phase begins.
    for _ in 0..150 {
        std::thread::sleep(Duration::from_millis(100));
        if run_heartbeat_persisted(tmp.path(), &doc, session_id) {
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
    assert_eq!(read_cycle_phase(&doc), "committed");
}

#[test]
fn run_heartbeats_redirect_stderr_under_managed_tui_but_persist_progress() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    let session_id = "test-heartbeat-redirect";
    fs::write(&doc, template_doc_with_session(session_id)).unwrap();
    init_git_repo(tmp.path(), &doc);

    let script = write_delayed_mock_agent(
        tmp.path(),
        "<!-- patch:exchange -->\n### Re: delayed — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        3,
    );
    let config_root = write_config(tmp.path(), &script);
    let bin = std::env::var("CARGO_BIN_EXE_agent-doc").unwrap();

    let child = ProcessCommand::new(bin)
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .env("AGENT_DOC_RUN_AGENT_TIMEOUT_SECS", "10")
        .env("AGENT_DOC_RUN_HEARTBEAT_SECS", "1")
        .env("AGENT_DOC_FORCE_RUN_STDERR_REDIRECT", "1")
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE")
        .env_remove("CLAUDE_CODE_SESSION")
        .env_remove("OPENCODE")
        .env_remove("OPENCODE_CLIENT")
        .env("CODEX_SESSION", "codex-session")
        .env("TMUX_PANE", "%77")
        .args(["run", doc.to_str().unwrap()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let mut saw_persisted_heartbeat = false;
    // Under full nextest load, startup/preflight can consume most of a short
    // polling window before the child-agent wait phase begins.
    for _ in 0..150 {
        std::thread::sleep(Duration::from_millis(100));
        if run_heartbeat_persisted(tmp.path(), &doc, session_id) {
            saw_persisted_heartbeat = true;
            break;
        }
    }

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "run should succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        saw_persisted_heartbeat,
        "expected hidden run heartbeat to update cycle progress while the child was still running"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("[run] heartbeat phase=child_agent_wait"),
        "managed TUI stderr must not contain routine heartbeat output:\n{stderr}"
    );
    assert!(
        !stderr.contains("[diff]"),
        "managed TUI stderr must not contain routine diff output:\n{stderr}"
    );
    let redirected = fs::read_to_string(tmp.path().join(".agent-doc/logs/run-stderr.log")).unwrap();
    assert!(
        redirected.contains("[run] stderr redirected"),
        "redirect log should explain the managed-TUI stderr target:\n{redirected}"
    );
    assert!(
        redirected.contains("[run] heartbeat phase=child_agent_wait"),
        "redirect log should retain heartbeat diagnostics:\n{redirected}"
    );
    assert!(
        fs::read_to_string(&doc)
            .unwrap()
            .contains("### Re: delayed — gpt-5")
    );
    assert_eq!(read_cycle_phase(&doc), "committed");
}

#[test]
fn bare_file_invocation_outside_supported_harness_fails_before_run_cycle() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, template_doc()).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE")
        .env_remove("CLAUDE_CODE_SESSION")
        .env_remove("OPENCODE")
        .env_remove("OPENCODE_CLIENT")
        .env_remove("CODEX")
        .env_remove("CODEX_CLI")
        .env_remove("CODEX_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .arg(doc.to_str().unwrap())
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("bare `agent-doc <FILE>` must be run")
                .and(predicate::str::contains(
                    "supported harness (Codex, Claude Code, or OpenCode)",
                ))
                .and(predicate::str::contains("agent-doc run <FILE>")),
        );

    assert!(
        !tmp.path().join(".agent-doc/state/cycles").exists(),
        "plain-shell bare invocation must fail before opening a run cycle"
    );
}

#[test]
fn codex_bare_run_inside_owning_pane_with_unresolved_prompt_fails_before_pre_commit() {
    // #codex-owned-pane-prompt-miss: when the owner pane re-invokes
    // `agent-doc <FILE>` while an unresolved exchange prompt is still pending,
    // the early guard fails closed BEFORE pre-commit / `start_run_cycle`. It
    // names the prompt and the in-pane recovery path instead of abandoning an
    // empty cycle after the prompt was baselined — the prompt stays executable.
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
        // Deterministically simulate a Codex harness. `detect_harness()` checks
        // Claude/OpenCode markers before Codex, so an inherited `CLAUDECODE`
        // (e.g. running the suite from inside a Claude Code session) would
        // otherwise short-circuit the codex owner-pane guard.
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE")
        .env_remove("CLAUDE_CODE_SESSION")
        .env_remove("OPENCODE")
        .env_remove("OPENCODE_CLIENT")
        .env("CODEX_SESSION", "codex-session")
        .env("TMUX_PANE", "%77")
        .arg(doc.to_str().unwrap())
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("owned-pane self-invocation with unresolved exchange prompt")
                .and(predicate::str::contains("Please reply"))
                .and(predicate::str::contains("agent-doc write --commit")),
        );

    // No cycle was opened — the early guard bailed before `start_run_cycle`, so
    // there is no preflight/abandoned cycle to recover and no snapshot advance.
    let state_dir = tmp.path().join(".agent-doc/state/cycles");
    let cycle_files = fs::read_dir(&state_dir)
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(
        cycle_files, 0,
        "early owner-pane prompt-miss guard must not open a run cycle"
    );

    // The prompt remains in the document (still executable, not consumed).
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("Please reply"));
}

#[test]
fn codex_owned_pane_active_auto_queue_hands_off_without_drift() {
    // #codex-owned-pane-auto-queue-stuck: when the owner pane re-invokes
    // `agent-doc <FILE>` while a ready active auto-queue head remains (and no
    // unresolved exchange prompt), the early handoff guard fails closed BEFORE
    // pre-commit / `start_run_cycle`. It names the live head and the in-owner-turn
    // recovery path, opens no cycle, and leaves the queue/boundary state
    // un-drifted — rather than letting pre-commit baseline drift and the late
    // recursive-deadlock guard abandon an empty cycle with the head still stuck.
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    let committed = "---\nagent_doc_session: session-recursive\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nAnswered.\n<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue auto go -->\n- do something\n<!-- /agent:queue -->\n";
    fs::write(&doc, committed).unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc);
    record_selected_queue_head(tmp.path(), &doc, committed, "do something");
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
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE")
        .env_remove("CLAUDE_CODE_SESSION")
        .env_remove("OPENCODE")
        .env_remove("OPENCODE_CLIENT")
        .env("CODEX_SESSION", "codex-session")
        .env("TMUX_PANE", "%77")
        .arg(doc.to_str().unwrap())
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("owned-pane self-invocation with active auto-queue head")
                .and(predicate::str::contains("do something"))
                .and(predicate::str::contains("agent-doc finalize"))
                .and(predicate::str::contains("Do NOT re-run")),
        );

    // No cycle was opened — the early handoff guard bailed before
    // `start_run_cycle`, so there is no preflight/abandoned cycle to recover.
    let state_dir = tmp.path().join(".agent-doc/state/cycles");
    let cycle_files = fs::read_dir(&state_dir)
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(
        cycle_files, 0,
        "early owner-pane queue handoff guard must not open a run cycle"
    );

    // No drift: the document is byte-identical to the committed state — no
    // pre-commit, queue, or boundary mutation — and the head stays live.
    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        content, committed,
        "owner-pane queue handoff must not mutate the document"
    );
    assert!(content.contains("- do something"));
    assert!(content.contains("<!-- agent:queue auto go -->"));
}

#[test]
fn codex_owned_pane_independent_queue_edit_defers_until_closeout() {
    // #cwsp: a user/operator queue edit can arrive while the Codex owner pane is
    // busy answering an earlier queue head. That edit must stay as document
    // state for the current closeout to merge; re-running `agent-doc <FILE>` in
    // the owner pane must not reinterpret the sibling edit as an immediate
    // owner-pane queue handoff or increment the wedge counter.
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    let committed = "---\nagent_doc_session: session-recursive\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nAnswered.\n<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue auto go -->\n- do [#active]\n<!-- /agent:queue -->\n";
    fs::write(&doc, committed).unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc);
    write_codex_owner_session(tmp.path(), &doc);
    save_active_queue_turn_scope(&doc, "active", 1);
    cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();

    let edited = committed.replace(
        "- do [#active]\n<!-- /agent:queue -->",
        "- do [#new]\n- do [#active]\n<!-- /agent:queue -->",
    );
    fs::write(&doc, &edited).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE")
        .env_remove("CLAUDE_CODE_SESSION")
        .env_remove("OPENCODE")
        .env_remove("OPENCODE_CLIENT")
        .env("CODEX_SESSION", "codex-session")
        .env("TMUX_PANE", "%77")
        .arg(doc.to_str().unwrap())
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "owner-pane queue edit deferred until current closeout",
        ));

    assert_eq!(
        fs::read_to_string(&doc).unwrap(),
        edited,
        "independent queue edit must remain visible for the active closeout"
    );
    let state = cycle_state::load(&doc).unwrap().unwrap();
    assert!(
        state.is_open(),
        "the existing owner cycle should stay open for the active closeout"
    );
}

fn write_codex_owner_session(root: &Path, doc: &Path) {
    fs::write(
        root.join(".agent-doc/sessions.json"),
        format!(
            "{{\n  \"session-recursive\": {{\n    \"pane\": \"%77\",\n    \"pid\": 123,\n    \"cwd\": \"{}\",\n    \"started\": \"2026-05-10T00:00:00Z\",\n    \"session_id\": \"session-recursive\",\n    \"file\": \"{}\",\n    \"window\": \"@7\",\n    \"supervisor_instance_id\": \"test-supervisor\"\n  }}\n}}\n",
            root.display(),
            doc.display()
        ),
    )
    .unwrap();
}

fn save_active_queue_turn_scope(doc: &Path, id: &str, exchange_tail_floor: usize) {
    let scope = TurnScope::for_driver_with_exchange_tail(
        Some(Address::node("queue", 0, &format!("queue:0:{id}:0"))),
        Some(exchange_tail_floor),
    );
    agent_doc_turn_scope_io::save(doc, &scope).unwrap();
}

#[test]
fn preflight_emits_owned_pane_self_invocation_for_unresolved_prompt() {
    // #codex-owned-pane-prompt-miss-followups (item: structured result): preflight
    // surfaces a typed owned_pane_self_invocation contract when a Codex owner-pane
    // run still has an unresolved exchange prompt, so Codex guidance can drive an
    // in-pane response instead of only reading the run-time bail diagnostic.
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(
        &doc,
        "---\nagent_doc_session: session-recursive\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n❯ Please reply\n<!-- /agent:exchange -->\n",
    )
    .unwrap();
    init_git_repo(tmp.path(), &doc);
    write_codex_owner_session(tmp.path(), &doc);

    let out = agent_doc()
        .current_dir(tmp.path())
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE")
        .env_remove("CLAUDE_CODE_SESSION")
        .env_remove("OPENCODE")
        .env_remove("OPENCODE_CLIENT")
        .env("CODEX_SESSION", "codex-session")
        .env("TMUX_PANE", "%77")
        .args(["preflight", doc.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "preflight should succeed");
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    let osi = &json["owned_pane_self_invocation"];
    assert!(
        !osi.is_null(),
        "owned_pane_self_invocation must be present: {json}"
    );
    assert_eq!(osi["kind"].as_str().unwrap(), "unresolved_prompt");
    assert_eq!(osi["current_pane"].as_str().unwrap(), "%77");
    assert!(
        osi["work_excerpt"]
            .as_str()
            .unwrap()
            .contains("Please reply")
    );
    assert!(
        osi["persistence_command"]
            .as_str()
            .unwrap()
            .contains("agent-doc finalize")
    );
}

#[test]
fn preflight_owned_pane_self_invocation_absent_for_non_owner_pane() {
    // Compatibility: a non-owner pane (TMUX_PANE != registered owner) is a normal
    // dispatch, not a self-invocation — the field stays null.
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(
        &doc,
        "---\nagent_doc_session: session-recursive\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n❯ Please reply\n<!-- /agent:exchange -->\n",
    )
    .unwrap();
    init_git_repo(tmp.path(), &doc);
    write_codex_owner_session(tmp.path(), &doc);

    let out = agent_doc()
        .current_dir(tmp.path())
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE")
        .env_remove("CLAUDE_CODE_SESSION")
        .env_remove("OPENCODE")
        .env_remove("OPENCODE_CLIENT")
        .env("CODEX_SESSION", "codex-session")
        .env("TMUX_PANE", "%99")
        .args(["preflight", doc.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        json["owned_pane_self_invocation"].is_null(),
        "non-owner pane must not emit owned_pane_self_invocation: {json}"
    );
}

#[test]
fn preflight_emits_owned_pane_self_invocation_for_active_queue_head() {
    // #codex-owned-pane-prompt-miss-followups (guidance): when the Codex owner
    // pane re-invokes the document with no unresolved exchange prompt but an
    // active `agent:queue auto go` head, preflight surfaces the structured contract
    // with kind=active_queue_head so the in-pane guidance can drive the next
    // queue continuation instead of launching a recursive child.
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(
        &doc,
        "---\nagent_doc_session: session-recursive\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nAnswered.\n<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue auto go -->\n- do something\n<!-- /agent:queue -->\n",
    )
    .unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc);
    write_codex_owner_session(tmp.path(), &doc);

    let out = agent_doc()
        .current_dir(tmp.path())
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE")
        .env_remove("CLAUDE_CODE_SESSION")
        .env_remove("OPENCODE")
        .env_remove("OPENCODE_CLIENT")
        .env("CODEX_SESSION", "codex-session")
        .env("TMUX_PANE", "%77")
        .args(["preflight", doc.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "preflight should succeed");
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    let osi = &json["owned_pane_self_invocation"];
    assert!(
        !osi.is_null(),
        "owned_pane_self_invocation must be present: {json}"
    );
    assert_eq!(osi["kind"].as_str().unwrap(), "active_queue_head");
    assert!(
        osi["work_excerpt"]
            .as_str()
            .unwrap()
            .contains("do something")
    );
    assert!(
        osi["persistence_command"]
            .as_str()
            .unwrap()
            .contains("agent-doc finalize")
    );
}

#[test]
fn preflight_suppresses_owned_pane_self_invocation_for_independent_queue_edit() {
    // #cwsp: the turn-scoped user-intent surface and owner-pane self-invocation
    // contract must not treat an independent sibling queue edit as work for the
    // busy owner pane.
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    let committed = "---\nagent_doc_session: session-recursive\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nAnswered.\n<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue auto go -->\n- do [#active]\n<!-- /agent:queue -->\n";
    fs::write(&doc, committed).unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc);
    write_codex_owner_session(tmp.path(), &doc);
    save_active_queue_turn_scope(&doc, "active", 1);
    cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();

    let edited = committed.replace(
        "- do [#active]\n<!-- /agent:queue -->",
        "- do [#new]\n- do [#active]\n<!-- /agent:queue -->",
    );
    fs::write(&doc, edited).unwrap();

    let out = agent_doc()
        .current_dir(tmp.path())
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE")
        .env_remove("CLAUDE_CODE_SESSION")
        .env_remove("OPENCODE")
        .env_remove("OPENCODE_CLIENT")
        .env("CODEX_SESSION", "codex-session")
        .env("TMUX_PANE", "%77")
        .args(["preflight", doc.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "preflight failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        json["owned_pane_self_invocation"].is_null(),
        "independent queue edit must not emit owner-pane self-invocation: {json}"
    );
    assert!(
        json.get("prompt_bearing_changes").is_none(),
        "preflight JSON must not expose removed prompt_bearing_changes field: {json}"
    );
    let user_intent_empty = json["user_intent_prompt_changes"]
        .as_array()
        .is_none_or(|changes| changes.is_empty());
    assert!(
        user_intent_empty,
        "independent queue edit must not count as user intent: {json}"
    );
    let persisted_scope = agent_doc_turn_scope_io::load(&doc).expect("turn scope should remain");
    assert_eq!(
        persisted_scope
            .driver
            .as_ref()
            .and_then(|driver| driver.node_key.as_deref()),
        Some("queue:0:active:0"),
        "recursive preflight must preserve the active owner's turn scope"
    );
}

#[test]
fn orchestrate_handles_already_open_preflight_cycle_for_first_step() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    let initial = template_doc_with_model().replace(
        "❯ Please reply\n",
        "### Re: prior — gpt-5\n\nDone.\n<!-- agent:boundary:keep -->\n",
    );
    fs::write(&doc, &initial).unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc);

    let edited = fs::read_to_string(&doc).unwrap().replace(
        "<!-- agent:boundary:keep -->\n",
        "Synchronous orchestra:\n<!-- agent:boundary:keep -->\n",
    );
    fs::write(&doc, edited).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();

    assert_eq!(read_cycle_phase(&doc), "preflight_started");

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
    assert_eq!(read_cycle_phase(&doc), "committed");
}

#[test]
fn orchestrate_streams_step_patchback_before_finalize() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, template_doc_with_model()).unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc);

    let (script, release_stream) = write_mock_streaming_agent(tmp.path());
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
            let child_state = child.try_wait().unwrap();
            fs::write(&release_stream, b"continue").unwrap();
            assert!(
                child_state.is_none(),
                "partial streamed patchback should land before orchestrate exits"
            );
            break;
        }
    }

    if !saw_partial {
        let _ = fs::write(&release_stream, b"continue");
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
    assert_eq!(read_cycle_phase(&doc), "committed");
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
    assert_eq!(read_cycle_phase(&doc), "committed");
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
    assert_eq!(read_cycle_phase(&doc), "write_applied");

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
    assert_eq!(read_cycle_phase(&doc), "committed");

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
    assert_terminal_closeout_proof(tmp.path(), &doc);
}
