use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
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

fn template_doc() -> String {
    "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n❯ Please reply\n<!-- /agent:exchange -->\n\n## Pending\n\n<!-- agent:pending patch=replace -->\n<!-- /agent:pending -->\n".to_string()
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

fn seed_snapshot(root: &Path, doc: &Path) {
    let canonical = doc.canonicalize().unwrap();
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let hash = hex::encode(hasher.finalize());
    let snapshot = root.join(".agent-doc/snapshots").join(format!("{hash}.md"));
    fs::write(snapshot, fs::read_to_string(doc).unwrap()).unwrap();
}

fn template_doc_with_model() -> String {
    "---\nagent_doc_format: template\nagent_doc_write: crdt\nmodel: gpt-5\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n❯ Please reply\n<!-- /agent:exchange -->\n\n## Pending\n\n<!-- agent:pending patch=replace -->\n<!-- /agent:pending -->\n".to_string()
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
fn orchestrate_reuses_open_preflight_cycle_for_first_step() {
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
