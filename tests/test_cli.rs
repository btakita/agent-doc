//! CLI integration tests for agent-doc.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
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
fn test_cli_init_no_file_runs_project_init() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.arg("init");
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("Project initialized"));
    // .agent-doc/ directory should be created
    assert!(tmp.path().join(".agent-doc").is_dir());
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
    assert!(
        content.contains("session:"),
        "frontmatter should have been generated"
    );
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
    assert!(
        !content.contains("session: null"),
        "session should no longer be null"
    );
    // Agent field should be preserved
    assert!(
        content.contains("agent:"),
        "other frontmatter fields should be preserved"
    );
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
    assert!(
        content.contains("session:"),
        "start should auto-generate session UUID"
    );
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
    assert!(
        !content.contains("session: null"),
        "session should no longer be null"
    );
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

// ── install tests ────────────────────────────────────────────────────────────

#[test]
fn test_cli_install_help() {
    let mut cmd = agent_doc_cmd();
    cmd.args(["install", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("skip-prereqs"))
        .stdout(predicate::str::contains("skip-plugins"));
}

#[test]
fn test_cli_install_skip_all() {
    let mut cmd = agent_doc_cmd();
    cmd.args(["install", "--skip-prereqs", "--skip-plugins"]);
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("Skipping plugin installation"));
}

#[test]
fn test_cli_install_checks_prereqs() {
    let mut cmd = agent_doc_cmd();
    cmd.args(["install", "--skip-plugins"]);
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("tmux"))
        .stderr(predicate::str::contains("claude"));
}

// ── init tests (project-level, no file arg) ──────────────────────────────────

#[test]
fn test_cli_init_creates_agent_doc_dir() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.arg("init");
    cmd.assert().success();
    assert!(tmp.path().join(".agent-doc/snapshots").is_dir());
    assert!(tmp.path().join(".agent-doc/patches").is_dir());
}

#[test]
fn test_cli_init_idempotent() {
    let tmp = tempfile::TempDir::new().unwrap();

    // First run
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.arg("init");
    cmd.assert().success();

    // Second run in the same dir must also succeed
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.arg("init");
    cmd.assert().success();
}

#[test]
fn test_cli_init_prints_quickstart() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.arg("init");
    // The quick-start hint mentions "agent-doc init" or "quick"
    cmd.assert().success().stderr(
        predicate::str::contains("agent-doc init")
            .or(predicate::str::contains("quick"))
            .or(predicate::str::contains("Quick")),
    );
}

// ── init tests (document-level, with file arg) ───────────────────────────────

#[test]
fn test_cli_init_file_creates_document() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("test.md");

    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.args(["init", "test.md"]);
    cmd.assert().success();

    assert!(doc.exists());
    let content = std::fs::read_to_string(&doc).unwrap();
    // Must have YAML frontmatter with a session ID
    assert!(
        content.contains("agent_doc_session:"),
        "expected frontmatter with session id"
    );
}

#[test]
fn test_cli_init_file_with_mode() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("test.md");

    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.args(["init", "test.md", "--mode", "template"]);
    cmd.assert().success();

    assert!(doc.exists());
    let content = std::fs::read_to_string(&doc).unwrap();
    // Template-mode documents have component markers
    assert!(
        content.contains("agent:exchange"),
        "expected exchange component marker"
    );
    assert!(
        content.contains("agent_doc_format: template"),
        "expected template format in frontmatter"
    );
}

#[test]
fn test_cli_init_file_lazy_project_init() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Confirm .agent-doc/ does not exist yet
    assert!(!tmp.path().join(".agent-doc").exists());

    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.args(["init", "test.md"]);
    cmd.assert().success();

    // Both the project directory and the document should have been created
    assert!(
        tmp.path().join(".agent-doc").is_dir(),
        ".agent-doc/ should be lazily created"
    );
    assert!(
        tmp.path().join("test.md").exists(),
        "test.md should be created"
    );
}

// ── skill tests ───────────────────────────────────────────────────────────────

#[test]
fn test_cli_skill_install_help() {
    let mut cmd = agent_doc_cmd();
    cmd.args(["skill", "install", "--help"]);
    cmd.assert().success();
}

#[test]
fn test_cli_skill_check_help() {
    let mut cmd = agent_doc_cmd();
    cmd.args(["skill", "check", "--help"]);
    cmd.assert().success();
}

#[test]
fn test_cli_skill_install_creates_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.env("CLAUDE_CODE", "1"); // Force ClaudeCode environment for deterministic path
    cmd.args(["skill", "install"]);
    cmd.assert().success();

    let skill_path = tmp.path().join(".claude/skills/agent-doc/SKILL.md");
    assert!(skill_path.exists(), "SKILL.md should be created");
    let content = std::fs::read_to_string(&skill_path).unwrap();
    assert!(
        content.contains("agent-doc-version:"),
        "SKILL.md should have agent-doc-version in frontmatter"
    );
}

#[test]
fn test_cli_skill_check_after_install() {
    let tmp = tempfile::TempDir::new().unwrap();

    // Install first
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.env("CLAUDE_CODE", "1");
    cmd.args(["skill", "install"]);
    cmd.assert().success();

    // Check should succeed (version matches)
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.env("CLAUDE_CODE", "1");
    cmd.args(["skill", "check"]);
    cmd.assert().success();
}

#[test]
fn test_cli_skill_install_idempotent() {
    let tmp = tempfile::TempDir::new().unwrap();

    // First install
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.env("CLAUDE_CODE", "1");
    cmd.args(["skill", "install"]);
    cmd.assert().success();

    // Second install must also succeed
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.env("CLAUDE_CODE", "1");
    cmd.args(["skill", "install"]);
    cmd.assert().success();

    let skill_path = tmp.path().join(".claude/skills/agent-doc/SKILL.md");
    assert!(
        skill_path.exists(),
        "SKILL.md should still exist after second install"
    );
}

#[test]
fn test_cli_skill_install_reload_compact() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.env("CLAUDE_CODE", "1");
    cmd.args(["skill", "install", "--reload", "compact"]);
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "skill install --reload compact should succeed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Either the skill was freshly installed (prints SKILL_RELOAD=compact to stdout)
    // or it was already up to date (prints "already up to date" to stderr).
    assert!(
        stdout.contains("SKILL_RELOAD=compact") || stderr.contains("already up to date"),
        "expected SKILL_RELOAD=compact or 'already up to date', got stdout={stdout:?} stderr={stderr:?}"
    );
}

#[test]
fn test_skill_md_contains_required_steps() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.env("CLAUDE_CODE", "1");
    cmd.args(["skill", "install"]);
    cmd.assert().success();

    let skill_path = tmp.path().join(".claude/skills/agent-doc/SKILL.md");
    let content = std::fs::read_to_string(&skill_path).unwrap();

    let required_steps = ["### 0.", "### 1.", "### 2."];
    for step in &required_steps {
        assert!(
            content.contains(step),
            "SKILL.md missing required workflow step: {step}"
        );
    }
    assert!(
        content.contains("agent-doc finalize <FILE>"),
        "SKILL.md should use finalize for the normal response cycle"
    );
    assert!(
        content.contains("agent-doc session-check <FILE>"),
        "SKILL.md should require session-check after final response persistence"
    );
    assert!(
        content.contains("The response persistence command is the final document-mutation boundary for the cycle"),
        "SKILL.md should treat response persistence as the close-out boundary"
    );
}

#[test]
fn test_codex_skill_install_writes_hook_artifacts() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.env("CODEX_CLI", "1");
    cmd.args(["skill", "install"]);
    cmd.assert().success();

    let hooks_path = tmp.path().join(".codex/hooks.json");
    let config_path = tmp.path().join(".codex/config.toml");
    assert!(hooks_path.exists(), "missing {}", hooks_path.display());
    assert!(config_path.exists(), "missing {}", config_path.display());

    let hooks: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&hooks_path).unwrap()).unwrap();
    assert!(hooks["hooks"]["UserPromptSubmit"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| {
            entry["hooks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|hook| hook["command"].as_str() == Some("agent-doc hook codex-user-prompt-submit"))
        }));
    assert!(hooks["hooks"]["Stop"].as_array().unwrap().iter().any(|entry| {
        entry["hooks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hook| hook["command"].as_str() == Some("agent-doc hook codex-stop"))
    }));

    let config: toml::Value =
        toml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    assert_eq!(config["features"]["codex_hooks"].as_bool(), Some(true));
}

#[test]
fn test_skill_md_references_valid_commands() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.env("CLAUDE_CODE", "1");
    cmd.args(["skill", "install"]);
    cmd.assert().success();

    let skill_path = tmp.path().join(".claude/skills/agent-doc/SKILL.md");
    let content = std::fs::read_to_string(&skill_path).unwrap();

    // Get valid subcommands by running `agent-doc --help`
    let help_output = agent_doc_cmd().arg("--help").output().unwrap();
    let help_text = String::from_utf8_lossy(&help_output.stdout);
    let valid_subcommands: std::collections::HashSet<String> = help_text
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            // Help lines for subcommands start with the command name followed by spaces and description
            let word = trimmed.split_whitespace().next()?;
            // Only accept lowercase words that look like subcommand names (no punctuation)
            if word.chars().all(|c| c.is_ascii_lowercase() || c == '-') && !word.is_empty() {
                Some(word.to_string())
            } else {
                None
            }
        })
        .collect();

    // Tokens that are not subcommands but are valid in SKILL.md:
    // - `submit` is the skill's invocation name (used in the title/heading)
    // - flags like `--version`, `--help` are valid options, not subcommands
    let allowed_non_subcommands: std::collections::HashSet<&str> =
        ["submit", "--version", "--help"].iter().copied().collect();

    // Extract all `agent-doc <word>` patterns from SKILL.md
    let mut invalid_refs: Vec<String> = Vec::new();
    for line in content.lines() {
        // Find all occurrences of `agent-doc <something>` in the line
        let mut search = line;
        while let Some(pos) = search.find("agent-doc") {
            let after = &search[pos + "agent-doc".len()..];
            // Skip if nothing follows or followed by non-whitespace (e.g., `agent-doc-version:`)
            let after_trimmed = after.trim_start_matches(' ');
            if after_trimmed == after && !after.is_empty() {
                // No space after `agent-doc` — skip (it's part of another word like `agent-doc-version`)
                search = &search[pos + "agent-doc".len()..];
                continue;
            }
            // Extract the next word after `agent-doc `
            let next_word = after_trimmed.split_whitespace().next();
            if let Some(cmd_name) = next_word {
                // Strip any trailing punctuation like `:`, `)`, `` ` ``
                let cmd_clean: String = cmd_name
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                    .collect();
                if !cmd_clean.is_empty()
                    && !valid_subcommands.contains(&cmd_clean)
                    && !allowed_non_subcommands.contains(cmd_clean.as_str())
                {
                    invalid_refs.push(format!("agent-doc {cmd_clean}"));
                }
            }
            search = &search[pos + "agent-doc".len()..];
        }
    }

    assert!(
        invalid_refs.is_empty(),
        "SKILL.md references unknown agent-doc subcommands: {:?}\nValid subcommands: {:?}",
        invalid_refs,
        valid_subcommands
    );
}

#[test]
fn test_submodule_write_patches_dir_structure() {
    use std::fs;
    use tempfile::TempDir;

    // This is a simpler integration test that verifies the expected directory structure
    // for submodule patch routing. The actual git submodule test is in write.rs unit tests
    // where we can create real git structures.

    let parent_dir = TempDir::new().unwrap();
    let parent = parent_dir.path();

    // Set up parent repo's .agent-doc structure
    let parent_agent_doc = parent.join(".agent-doc");
    fs::create_dir_all(parent_agent_doc.join("patches")).unwrap();
    fs::create_dir_all(parent_agent_doc.join("snapshots")).unwrap();
    fs::create_dir_all(parent_agent_doc.join("crdt")).unwrap();

    // Verify patches directory exists and is accessible
    let parent_patches = parent.join(".agent-doc/patches");
    assert!(
        parent_patches.exists(),
        "parent should have .agent-doc/patches directory"
    );
    assert!(
        parent_patches.is_dir(),
        ".agent-doc/patches should be a directory"
    );

    // Simulate a document in a submodule location
    let simulated_submodule_path = parent.join("src/submodule/tasks");
    fs::create_dir_all(&simulated_submodule_path).unwrap();
    let doc = simulated_submodule_path.join("test.md");
    fs::write(&doc, "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->test<!-- /agent:exchange -->\n").unwrap();

    // Verify the document file exists
    assert!(doc.exists(), "test document should exist");
    assert!(doc.is_file(), "test document should be a file");

    // Verify parent's patches directory is still accessible (would receive patches in actual IPC scenario)
    let entries: Vec<_> = fs::read_dir(&parent_patches)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    // Directory should be empty initially
    assert!(
        entries.is_empty(),
        "patches directory should be initially empty"
    );
}
