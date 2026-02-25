//! CLI integration tests for agent-doc.

use assert_cmd::cargo::cargo_bin_cmd;
use assert_cmd::Command;
use predicates::prelude::*;

fn agent_doc_cmd() -> Command {
    cargo_bin_cmd!("agent-doc")
}

#[test]
fn test_binary_exists() {
    let _cmd = agent_doc_cmd();
}

#[test]
fn test_cli_help() {
    let mut cmd = agent_doc_cmd();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Interactive document sessions"));
}

#[test]
fn test_cli_no_args_shows_error() {
    let mut cmd = agent_doc_cmd();
    cmd.assert().failure();
}

#[test]
fn test_cli_unknown_subcommand() {
    let mut cmd = agent_doc_cmd();
    cmd.arg("nonexistent-command");
    cmd.assert().failure();
}

#[test]
fn test_cli_audit_docs_subcommand() {
    let mut cmd = agent_doc_cmd();
    cmd.arg("audit-docs");
    let output = cmd.output().unwrap();
    // Should run (may exit 0 or 1 depending on doc state, but not crash)
    assert!(output.status.code().is_some());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Auditing docs..."));
}

#[test]
fn test_cli_audit_docs_in_tempdir_no_project_marker() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.arg("audit-docs");
    // Should succeed with a warning, falling back to CWD
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("no project root marker found"));
}

#[test]
fn test_cli_audit_docs_clean_project() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    // Minimal project with no issues
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"test\"\n").unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.current_dir(root);
    cmd.arg("audit-docs");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("No issues found"));
}

#[test]
fn test_cli_audit_docs_finds_claude_md() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"test\"\n").unwrap();
    std::fs::write(root.join("CLAUDE.md"), "# Doc\n\nUse serde.\n").unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.current_dir(root);
    cmd.arg("audit-docs");
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("CLAUDE.md"));
}

#[test]
fn test_cli_audit_docs_reports_missing_tree_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"test\"\n").unwrap();
    std::fs::write(
        root.join("CLAUDE.md"),
        "# Doc\n\n## Project Structure\n\n```\nsrc/\n  missing.rs\n```\n",
    )
    .unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.current_dir(root);
    cmd.arg("audit-docs");
    cmd.assert()
        .failure()
        .stdout(predicate::str::contains("Referenced path does not exist"));
}

#[test]
fn test_cli_run_requires_file() {
    let mut cmd = agent_doc_cmd();
    cmd.arg("run");
    cmd.assert().failure();
}

#[test]
fn test_cli_init_requires_file() {
    let mut cmd = agent_doc_cmd();
    cmd.arg("init");
    cmd.assert().failure();
}

#[test]
fn test_cli_start_requires_file() {
    let mut cmd = agent_doc_cmd();
    cmd.arg("start");
    cmd.assert().failure();
}

#[test]
fn test_cli_route_requires_file() {
    let mut cmd = agent_doc_cmd();
    cmd.arg("route");
    cmd.assert().failure();
}

#[test]
fn test_cli_start_file_not_found() {
    let mut cmd = agent_doc_cmd();
    cmd.args(["start", "/nonexistent/file.md"]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("file not found"));
}

#[test]
fn test_cli_route_file_not_found() {
    let mut cmd = agent_doc_cmd();
    cmd.args(["route", "/nonexistent/file.md"]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("file not found"));
}

#[test]
fn test_cli_start_not_in_tmux() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("test.md");
    std::fs::write(&doc, "---\nsession: test-123\n---\n# Test\n").unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.arg("start");
    cmd.arg(&doc);
    // Remove TMUX env vars to simulate not being in tmux
    cmd.env_remove("TMUX");
    cmd.env_remove("TMUX_PANE");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("not running inside tmux"));
}

#[test]
fn test_cli_route_generates_session_for_bare_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("test.md");
    std::fs::write(&doc, "# No frontmatter\n").unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.arg("route");
    cmd.arg(&doc);
    cmd.current_dir(tmp.path());
    // Prevent auto-start from creating real tmux windows
    cmd.env("AGENT_DOC_NO_AUTOSTART", "1");
    // Route should generate a session UUID (not error), then fail on tmux (not available in CI)
    // The key behavior: it should NOT fail with "no session UUID"
    let output = cmd.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("no session UUID"),
        "route should auto-generate session UUID, got: {}",
        stderr
    );
    // Verify the file was updated with frontmatter
    let content = std::fs::read_to_string(&doc).unwrap();
    assert!(content.contains("session:"), "frontmatter should have been generated");
}

#[test]
fn test_cli_route_generates_session_for_null_session() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("test.md");
    std::fs::write(&doc, "---\nsession: null\nagent: claude\n---\n# Test\n").unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.arg("route");
    cmd.arg(&doc);
    cmd.current_dir(tmp.path());
    // Prevent auto-start from creating real tmux windows
    cmd.env("AGENT_DOC_NO_AUTOSTART", "1");
    let output = cmd.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("no session UUID"),
        "route should auto-generate UUID for null session, got: {}",
        stderr
    );
    // Verify the file now has a real session UUID (not null)
    let content = std::fs::read_to_string(&doc).unwrap();
    assert!(content.contains("session:"), "frontmatter should exist");
    assert!(!content.contains("session: null"), "session should no longer be null");
    // Agent field should be preserved
    assert!(content.contains("agent:"), "other frontmatter fields should be preserved");
}

#[test]
fn test_cli_start_generates_session_for_bare_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("test.md");
    std::fs::write(&doc, "# No frontmatter\n").unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.arg("start");
    cmd.arg(&doc);
    cmd.env_remove("TMUX");
    cmd.env_remove("TMUX_PANE");
    // start should generate the UUID first, THEN fail on tmux check
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("not running inside tmux"));
    // Verify the file was updated with frontmatter before the tmux error
    let content = std::fs::read_to_string(&doc).unwrap();
    assert!(content.contains("session:"), "start should auto-generate session UUID");
}

#[test]
fn test_cli_start_generates_session_for_null_session() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("test.md");
    std::fs::write(&doc, "---\nsession: null\n---\n# Test\n").unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.arg("start");
    cmd.arg(&doc);
    cmd.env_remove("TMUX");
    cmd.env_remove("TMUX_PANE");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("not running inside tmux"));
    let content = std::fs::read_to_string(&doc).unwrap();
    assert!(content.contains("session:"), "frontmatter should exist");
    assert!(!content.contains("session: null"), "session should no longer be null");
}

#[test]
fn test_cli_help_shows_start_and_route() {
    let mut cmd = agent_doc_cmd();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("start"))
        .stdout(predicate::str::contains("route"));
}
