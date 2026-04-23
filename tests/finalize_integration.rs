use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
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

#[test]
fn finalize_requires_git_repo_before_mutating_document() {
    let (_tmp, doc) = setup_template_doc();
    let before = fs::read_to_string(&doc).unwrap();

    agent_doc()
        .args(["finalize", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: test — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "finalize requires a git repository",
        ));

    let after = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        before, after,
        "finalize should fail before mutating the file"
    );
}

#[test]
fn finalize_writes_and_commits_template_response() {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--pending-add",
            "follow up task",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: test — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: test — gpt-5"));
    assert!(content.contains("follow up task"));

    let head_blob = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&head_blob.stdout).contains("### Re: test — gpt-5"),
        "HEAD blob should contain the finalized response"
    );

    let log = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["log", "--oneline", "-1"])
        .output()
        .unwrap();
    let log_stdout = String::from_utf8_lossy(&log.stdout);
    assert!(
        log_stdout.contains("agent-doc(session):"),
        "expected finalize to create an agent-doc commit, got: {}",
        log_stdout
    );

    let cycles_dir = tmp.path().join(".agent-doc/state/cycles");
    let mut entries = fs::read_dir(&cycles_dir).unwrap();
    let cycle_path = entries.next().unwrap().unwrap().path();
    let cycle_json = fs::read_to_string(cycle_path).unwrap();
    assert!(
        cycle_json.contains("\"phase\": \"committed\""),
        "cycle should be committed, got: {}",
        cycle_json
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
}
