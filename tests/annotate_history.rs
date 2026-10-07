use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit_document(root: &Path, doc: &Path, content: &str, message: &str) -> String {
    std::fs::write(doc, content).unwrap();
    git(root, &["add", "session.md"]);
    git(root, &["commit", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

fn annotate(doc: &Path, history: bool) -> (PathBuf, Value) {
    let mut command = cargo_bin_cmd!("agent-doc");
    command.arg("annotate").arg(doc);
    if history {
        command.arg("--history");
    }
    let output = command.assert().success().get_output().stdout.clone();
    let path = PathBuf::from(String::from_utf8(output).unwrap().trim());
    let sidecar = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    (path, sidecar)
}

#[test]
fn annotate_history_adds_blame_and_invalidates_on_path_history_change() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let doc = root.join("session.md");
    git(root, &["init"]);
    git(root, &["config", "user.name", "History Tester"]);
    git(root, &["config", "user.email", "history@example.test"]);

    let original = "first line\nsecond line\n";
    let first_revision = commit_document(root, &doc, original, "initial document");

    let (snapshot_path, snapshot) = annotate(&doc, false);
    assert!(snapshot.get("history_revision").is_none());
    assert!(
        snapshot["lines"]
            .as_array()
            .unwrap()
            .iter()
            .all(|line| line.get("history").is_none())
    );

    let (history_path, history) = annotate(&doc, true);
    assert_eq!(history_path, snapshot_path);
    assert_eq!(history["history_revision"], first_revision);
    for line in history["lines"].as_array().unwrap() {
        assert_eq!(line["history"]["commit"], first_revision);
        assert_eq!(line["history"]["author"], "History Tester");
        assert_eq!(line["history"]["author_email"], "history@example.test");
    }

    commit_document(root, &doc, "changed line\n", "change document");
    let latest_revision = commit_document(root, &doc, original, "restore document");
    assert_ne!(latest_revision, first_revision);

    // Snapshot and file hashes are unchanged; only the document's Git history
    // changed. A history-mode cache hit must therefore use history_revision.
    let (_, refreshed) = annotate(&doc, true);
    assert_eq!(refreshed["history_revision"], latest_revision);
    for line in refreshed["lines"].as_array().unwrap() {
        assert_eq!(line["history"]["commit"], latest_revision);
    }
}

#[test]
fn annotate_history_rejects_a_document_without_committed_git_history() {
    let temp = tempfile::tempdir().unwrap();
    let doc = temp.path().join("untracked.md");
    git(temp.path(), &["init"]);
    git(temp.path(), &["config", "user.name", "History Tester"]);
    git(
        temp.path(),
        &["config", "user.email", "history@example.test"],
    );
    std::fs::write(temp.path().join("README.md"), "repository\n").unwrap();
    git(temp.path(), &["add", "README.md"]);
    git(temp.path(), &["commit", "-m", "initialize repository"]);
    std::fs::write(&doc, "untracked\n").unwrap();

    cargo_bin_cmd!("agent-doc")
        .arg("annotate")
        .arg(&doc)
        .arg("--history")
        .assert()
        .failure()
        .stderr(predicate::str::contains("to have committed Git history"));
}
