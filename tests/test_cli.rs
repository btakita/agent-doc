//! CLI integration tests for agent-doc.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;

fn agent_doc_cmd() -> Command {
    cargo_bin_cmd!("agent-doc")
}

fn init_git_repo(root: &Path, tracked: &Path) {
    fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/state/cycles")).unwrap();
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

fn seed_snapshot(root: &Path, doc: &Path, content: &str) {
    let canonical = doc.canonicalize().unwrap();
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let hash = hex::encode(hasher.finalize());
    let snapshot = root.join(".agent-doc/snapshots").join(format!("{hash}.md"));
    fs::write(snapshot, content).unwrap();
}

fn extract_preflight_baseline(output: &str) -> String {
    output
        .lines()
        .find_map(|line| {
            line.strip_prefix("[preflight] baseline saved: ")
                .map(str::trim)
                .map(str::to_string)
        })
        .or_else(|| {
            output
                .split("\"baseline_file\": \"")
                .nth(1)
                .and_then(|tail| tail.split('"').next())
                .map(str::to_string)
        })
        .unwrap_or_else(|| panic!("missing preflight baseline in output:\n{output}"))
}

fn template_doc(title: &str, exchange: &str, backlog: &str, icebox: &str) -> String {
    format!(
        "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n# {title}\n\n## Status\n\n<!-- agent:status patch=replace -->\n<!-- /agent:status -->\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n\n## Backlog\n\n<!-- agent:backlog -->\n{backlog}<!-- /agent:backlog -->\n\n## Icebox\n\n<!-- agent:icebox -->\n{icebox}<!-- /agent:icebox -->\n"
    )
}

#[test]
fn test_binary_exists() {
    let _cmd = agent_doc_cmd();
}

#[test]
fn live_tmux_tests_are_not_in_default_development_suite() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources = [
        "src/autoclaim.rs",
        "src/focus.rs",
        "src/resync.rs",
        "src/route.rs",
        "src/session_actor_cmd.rs",
        "src/sessions.rs",
        "src/start.rs",
        "src/sync.rs",
    ];
    let mut unignored = Vec::new();

    for source in sources {
        let path = manifest_dir.join(source);
        let content = fs::read_to_string(&path).unwrap();
        for block in content.split("#[test]").skip(1) {
            if !block.contains("IsolatedTmux::new") {
                continue;
            }
            let header_and_body = block.split("#[test]").next().unwrap_or(block);
            if !header_and_body.contains("#[ignore") {
                let name = header_and_body
                    .split("fn ")
                    .nth(1)
                    .and_then(|rest| rest.split('(').next())
                    .unwrap_or("<unknown>");
                unignored.push(format!("{source}::{name}"));
            }
        }
    }

    assert!(
        unignored.is_empty(),
        "live tmux tests must be #[ignore] and run through `make tmux-ci`: {unignored:?}"
    );
}

#[test]
fn test_cli_help() {
    let mut cmd = agent_doc_cmd();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Interactive document sessions"))
        .stdout(predicate::str::contains("repair"))
        .stdout(predicate::str::contains("fix"));
}

#[test]
fn test_cli_controller_status_reports_inactive_without_launching() {
    let tmp = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.args([
        "controller",
        "status",
        "--project-root",
        tmp.path().to_str().unwrap(),
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let status: serde_json::Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(status["active"], false);
    assert_eq!(
        status["socket_path"],
        tmp.path()
            .join(".agent-doc/controller.sock")
            .to_string_lossy()
            .as_ref()
    );
    assert!(!tmp.path().join(".agent-doc/controller-state.json").exists());
}

#[test]
fn test_cli_controller_status_ensure_lazy_launches_and_persists_bootstrap() {
    let tmp = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.args([
        "controller",
        "status",
        "--ensure",
        "--project-root",
        tmp.path().to_str().unwrap(),
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let status: serde_json::Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(status["active"], true);
    assert_eq!(status["launch_mode"], "lazy");
    assert!(status["bootstrap_epoch"].as_u64().unwrap() > 0);
    assert!(tmp.path().join(".agent-doc/controller-state.json").exists());

    let mut shutdown = agent_doc_cmd();
    shutdown.args([
        "controller",
        "shutdown",
        "--project-root",
        tmp.path().to_str().unwrap(),
    ]);
    shutdown.assert().success();
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
fn test_cli_ops_summary_groups_ops_log_events() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    fs::write(
        root.join(".agent-doc/logs/ops.log"),
        format!(
            "\
[100] ipc_write_consumed file={} patches=1
[101] commit_success file={}
[102] route_dispatch_only_sent file=tasks/b.md pane=%2 harness=opencode proof=accepted proof_scope=accepted_only
[103] route_dispatch_only_submit_unproven file=tasks/b.md pane=%2 harness=opencode delivery=direct_pane_submit submit_mode=tmux_literal_enter_delayed proof=accepted proof_scope=accepted_only timeout_secs=10
[104] sync_latency phase=prune_stash_panes elapsed_ms=309 budget_ms=250 status=over_budget mode=full
",
            root.join("tasks/a.md").display(),
            root.join("tasks/a.md").display()
        ),
    )
    .unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.args([
        "ops",
        "summary",
        "--project-root",
        root.to_str().unwrap(),
        "--limit",
        "0",
    ]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("write ipc consumed"))
        .stdout(predicate::str::contains("tasks/a.md"))
        .stdout(predicate::str::contains("accepted-only route proof"))
        .stdout(predicate::str::contains("dispatch-only not proven"))
        .stdout(predicate::str::contains("sync over budget"));
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
fn test_manifest_uses_local_agent_kit_path_for_direct_install() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manifest = fs::read_to_string(manifest_path).unwrap();
    let parsed: toml::Value = toml::from_str(&manifest).unwrap();
    let dependency = parsed["dependencies"]["agent-kit"].as_table().unwrap();

    assert_eq!(
        dependency.get("path").and_then(toml::Value::as_str),
        Some("../agent-kit")
    );
    assert_eq!(
        dependency.get("version").and_then(toml::Value::as_str),
        Some("0.4.0")
    );
}

#[test]
fn test_cli_run_requires_file() {
    let mut cmd = agent_doc_cmd();
    cmd.arg("run");
    cmd.assert().failure();
}

#[test]
fn test_cli_patch_replaces_exchange_even_when_component_is_append_mode() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let doc = root.join("session.md");
    fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
    fs::write(&doc, template_doc("Session", "old exchange body\n", "", "")).unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.current_dir(root);
    cmd.args([
        "patch",
        doc.to_str().unwrap(),
        "exchange",
        "new exchange body\n",
    ]);
    cmd.assert().success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("new exchange body"));
    assert!(!content.contains("old exchange body"));
}

#[test]
fn test_commit_explains_head_current_follow_up_without_repair_body_noise() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let doc = root.join("session.md");
    let committed = "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
        ## Exchange\n\n\
        <!-- agent:exchange patch=append -->\n\
        ### Re: prior response — gpt-5\n\
        Already committed.\n\
        <!-- agent:boundary:head-boundary -->\n\
        <!-- /agent:exchange -->\n";
    fs::write(&doc, committed).unwrap();
    init_git_repo(root, &doc);
    seed_snapshot(root, &doc, committed);

    let working = "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
        ## Exchange\n\n\
        <!-- agent:exchange patch=append -->\n\
        ### Re: prior response — gpt-5\n\
        Already committed.\n\
        <!-- agent:boundary:head-boundary -->\n\
        ❯ write response back\n\
        ❯ commit\n\
        <!-- /agent:exchange -->\n";
    fs::write(&doc, working).unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.current_dir(root);
    cmd.args(["commit", doc.to_str().unwrap()]);
    cmd.assert()
        .success()
        .stderr(predicate::str::contains(
            "leaving later local user follow-up edits uncommitted",
        ))
        .stderr(predicate::str::contains(
            "This is not a full closeout for the follow-up prompt",
        ))
        .stderr(predicate::str::contains("agent-doc write --commit"));
}

#[test]
fn test_preflight_finalize_preflight_follow_up_lifecycle_has_no_stale_cycle() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let doc = root.join("session.md");
    let committed = "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
        ## Exchange\n\n\
        <!-- agent:exchange patch=append -->\n\
        ### Re: prior response — gpt-5\n\
        Already committed.\n\
        <!-- agent:boundary:head-boundary -->\n\
        <!-- /agent:exchange -->\n";
    fs::write(&doc, committed).unwrap();
    init_git_repo(root, &doc);
    seed_snapshot(root, &doc, committed);

    let working = committed.replace(
        "<!-- agent:boundary:head-boundary -->",
        "❯ follow-up after dispatch\n<!-- agent:boundary:head-boundary -->",
    );
    fs::write(&doc, working).unwrap();

    let mut preflight = agent_doc_cmd();
    preflight.current_dir(root);
    preflight.args(["preflight", doc.to_str().unwrap()]);
    let output = preflight.output().unwrap();
    assert!(
        output.status.success(),
        "preflight failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let baseline = extract_preflight_baseline(&combined);
    assert!(
        !combined.contains("prior_patchback_without_response_body"),
        "preflight should not reopen missed-response repair for a follow-up prompt:\n{combined}"
    );

    let mut finalize = agent_doc_cmd();
    finalize.current_dir(root);
    finalize.args([
        "finalize",
        doc.to_str().unwrap(),
        "--baseline-file",
        &baseline,
        "--origin",
        "skill",
    ]);
    finalize
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: follow-up — gpt-5\n\nHandled.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let mut second_preflight = agent_doc_cmd();
    second_preflight.current_dir(root);
    second_preflight.args(["preflight", doc.to_str().unwrap()]);
    let second = second_preflight.output().unwrap();
    assert!(
        second.status.success(),
        "second preflight failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    let second_combined = format!(
        "{}{}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        second_combined.contains("\"no_changes\": true"),
        "preflight after finalize should be clean:\n{second_combined}"
    );
    assert!(
        !second_combined.contains("snapshot/head guard mismatch")
            && !second_combined.contains("prior_patchback_without_response_body"),
        "clean follow-up lifecycle should not report stale cycle guards:\n{second_combined}"
    );
}

#[test]
fn test_cli_bare_file_path_aliases_to_run() {
    let tmp = tempfile::TempDir::new().unwrap();
    let missing = tmp.path().join("missing.md");

    let mut cmd = agent_doc_cmd();
    cmd.arg(&missing);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("file not found"));
}

#[test]
fn test_compact_commit_explains_commit_scope() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let doc = root.join("session.md");
    let content = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
        ## Exchange\n\n\
        <!-- agent:exchange patch=append -->\n\
        ### Re: first topic — gpt-5\n\
        first body\n\n\
        ### Re: second topic — gpt-5\n\
        second body\n\
        <!-- /agent:exchange -->\n";
    fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    fs::write(&doc, content).unwrap();
    seed_snapshot(root, &doc, content);
    init_git_repo(root, &doc);

    let mut cmd = agent_doc_cmd();
    cmd.current_dir(root);
    cmd.args([
        "compact",
        doc.to_str().unwrap(),
        "--component",
        "exchange",
        "--tag",
        "skip",
        "--commit",
    ]);
    cmd.assert().success().stderr(predicate::str::contains(
        "[compact] note: --commit persists only the compacted document state now in HEAD",
    ));
}

#[test]
fn test_archive_index_and_search_commands() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let doc = root.join("tasks/session.md");
    fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
    fs::create_dir_all(doc.parent().unwrap()).unwrap();
    fs::write(
        &doc,
        "---\nagent_doc_session: current-session\nagent_doc_format: template\n---\n\nbody\n",
    )
    .unwrap();
    fs::write(
        root.join(".agent-doc/archives/hash-20260506-000000.md"),
        concat!(
            "---\n",
            "archived_from: compact\n",
            "archived_at: 20260506-000000\n",
            "component: exchange\n",
            "document: tasks/session.md\n",
            "session: current-session\n",
            "---\n\n",
            "## User\n\nDo #sqlarcidx.\n\n",
            "## Assistant\n\nPlan: tasks/agent-doc/plan-sqlite-compacted-turn-archive.md\n"
        ),
    )
    .unwrap();

    let mut index_cmd = agent_doc_cmd();
    index_cmd.current_dir(root);
    index_cmd.args(["archive-index", doc.to_str().unwrap(), "--rebuild"]);
    index_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("1 archive(s) indexed"));

    let mut search_cmd = agent_doc_cmd();
    search_cmd.current_dir(root);
    search_cmd.args([
        "archive-search",
        doc.to_str().unwrap(),
        "--id",
        "sqlarcidx",
        "--json",
    ]);
    search_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("\"archive_path\""))
        .stdout(predicate::str::contains("#sqlarcidx"));
}

#[test]
fn test_response_toc_and_fetch_commands() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let doc = root.join("tasks/session.md");
    fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
    fs::create_dir_all(doc.parent().unwrap()).unwrap();
    fs::write(
        &doc,
        concat!(
            "---\n",
            "agent_doc_session: current-session\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: current topic — gpt-5\n\n",
            "Body for #restoc.\n",
            "<!-- /agent:exchange -->\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join(".agent-doc/archives/hash-20260506-000000.md"),
        concat!(
            "---\n",
            "archived_from: compact\n",
            "archived_at: 20260506-000000\n",
            "component: exchange\n",
            "document: tasks/session.md\n",
            "session: current-session\n",
            "---\n\n",
            "## User\n\nDo #restoc.\n\n",
            "## Assistant\n\n### Re: archived topic — gpt-5\n\nArchived body for #restoc.\n"
        ),
    )
    .unwrap();

    let mut toc_cmd = agent_doc_cmd();
    toc_cmd.current_dir(root);
    toc_cmd.args([
        "response-toc",
        doc.to_str().unwrap(),
        "--id",
        "restoc",
        "--json",
    ]);
    toc_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("\"locator\""))
        .stdout(predicate::str::contains("live:1"))
        .stdout(predicate::str::contains(
            "archive:.agent-doc/archives/hash-20260506-000000.md#2",
        ));

    let mut fetch_cmd = agent_doc_cmd();
    fetch_cmd.current_dir(root);
    fetch_cmd.args([
        "response-fetch",
        doc.to_str().unwrap(),
        "--locator",
        "archive:.agent-doc/archives/hash-20260506-000000.md#2",
        "--json",
    ]);
    fetch_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("Archived body for #restoc."));
}

#[test]
fn test_cli_repair_aliases_legacy_recover() {
    let tmp = tempfile::TempDir::new().unwrap();
    let missing = tmp.path().join("missing.md");

    let mut repair = agent_doc_cmd();
    repair.arg("repair").arg(&missing);
    repair
        .assert()
        .failure()
        .stderr(predicate::str::contains("file not found"));

    let mut recover = agent_doc_cmd();
    recover.arg("recover").arg(&missing);
    recover
        .assert()
        .failure()
        .stderr(predicate::str::contains("file not found"));
}

#[test]
fn test_cli_fix_missing_file_errors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let missing = tmp.path().join("missing.md");

    let mut cmd = agent_doc_cmd();
    cmd.arg("fix").arg(&missing);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("file not found"));
}

#[test]
fn test_cli_resync_fix_alias_accepts_optional_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let missing = tmp.path().join("missing.md");

    let mut cmd = agent_doc_cmd();
    cmd.args(["resync", "--fix", missing.to_str().unwrap()]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("file not found"));
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
        "SKILL.md should still mention the direct session-check command"
    );
    assert!(
        content.contains(
            "The response persistence command is the final document-mutation boundary for the cycle"
        ),
        "SKILL.md should treat response persistence as the close-out boundary"
    );
    assert!(
        content.contains("Imperative edits are executable directives"),
        "SKILL.md should treat document-local `do` edits as executable work directives"
    );
    assert!(
        content.contains(
            "Harness-native `agent-doc` entrypoints start the binary-owned response cycle"
        ),
        "SKILL.md should treat harness-native agent-doc invocations as binary-owned workflow starts"
    );
    assert!(
        content.contains("Do not manually patch the final assistant response into the document"),
        "SKILL.md should forbid manual final-response patchback on harness-native agent-doc turns"
    );
    assert!(
        content.contains("Do not stop at the newest question"),
        "SKILL.md should require reconciling the changed exchange tail oldest-first"
    );
    assert!(
        content.contains("runbooks/command-synonyms.md"),
        "SKILL.md should point agents to the orchestrate synonym runbook"
    );
    assert!(
        content.contains("runbooks/compound-task-steering.md"),
        "SKILL.md should point agents to the compound-task steering runbook"
    );
    assert!(
        content.contains("orchestration_request"),
        "SKILL.md should treat binary-owned orchestration requests as first-class preflight output"
    );
    assert!(
        content.contains(
            "agent-doc orchestrate <FILE> --mode <orchestration_request.mode> --from-exchange"
        ),
        "SKILL.md should require dispatching preflight orchestration requests through agent-doc orchestrate"
    );
    assert!(
        content.contains("agent-doc plan <FILE>"),
        "SKILL.md should require the binary-owned planning phase after preflight"
    );
    assert!(
        content.contains("runbooks/planning-dispatch.md"),
        "SKILL.md should point agents to the planning/dispatch runbook"
    );
    assert!(
        content.contains("external CI-start blocker"),
        "SKILL.md should classify empty-step no-log GitHub jobs as external CI-start blockers"
    );
    assert!(
        content.contains("billing/spending-limit exhaustion"),
        "SKILL.md should name billing/spending-limit failures as CI-start blockers"
    );
    assert!(
        content.contains("create the plan file first"),
        "SKILL.md should require creating plan files before adding plan-backed pending items"
    );
    assert!(
        content.contains("include that exact plan file path"),
        "SKILL.md should require plan-backed pending items to include the plan path"
    );
    assert!(
        tmp.path()
            .join(".claude/skills/agent-doc/runbooks/compound-task-steering.md")
            .exists(),
        "skill install should write the compound-task steering runbook"
    );
    assert!(
        tmp.path()
            .join(".claude/skills/agent-doc/runbooks/planning-dispatch.md")
            .exists(),
        "skill install should write the planning-dispatch runbook"
    );
    let pending_ops = std::fs::read_to_string(
        tmp.path()
            .join(".claude/skills/agent-doc/runbooks/pending-ops.md"),
    )
    .unwrap();
    assert!(
        pending_ops.contains("plan-spec2-rollout.md"),
        "installed pending-ops runbook should document plan-backed pending items"
    );
}

#[test]
fn test_codex_skill_install_writes_hook_artifacts() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.env("CODEX_CLI", "1");
    cmd.env_remove("CLAUDE_CODE");
    cmd.env_remove("CLAUDE_CODE_ENTRYPOINT");
    cmd.env_remove("OPENCODE");
    cmd.args(["skill", "install"]);
    cmd.assert().success();

    let hooks_path = tmp.path().join(".codex/hooks.json");
    let config_path = tmp.path().join(".codex/config.toml");
    assert!(hooks_path.exists(), "missing {}", hooks_path.display());
    assert!(config_path.exists(), "missing {}", config_path.display());

    let hooks: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&hooks_path).unwrap()).unwrap();
    assert!(
        hooks["hooks"]["UserPromptSubmit"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry["hooks"].as_array().unwrap().iter().any(|hook| {
                    hook["command"].as_str() == Some("agent-doc hook codex-user-prompt-submit")
                })
            })
    );
    assert!(
        hooks["hooks"]["Stop"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry["hooks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|hook| hook["command"].as_str() == Some("agent-doc hook codex-stop"))
            })
    );

    let config: toml::Value =
        toml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    assert_eq!(config["features"]["hooks"].as_bool(), Some(true));
    assert!(config["features"].get("codex_hooks").is_none());
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

#[test]
fn test_compact_message_dash_reads_stdin() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/state/cycles")).unwrap();

    init_git_repo(root, &root.join("session.md"));

    let doc = concat!(
        "---\nagent_doc_session: stdin-test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: topic\n\nSome response content.\n",
        "<!-- /agent:exchange -->\n",
    );
    fs::write(root.join("session.md"), doc).unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.current_dir(root);
    cmd.args(["compact", "session.md", "--message", "-", "--tag", "skip"]);
    cmd.write_stdin("Summary from stdin pipe.");
    cmd.assert().success();

    let result = fs::read_to_string(root.join("session.md")).unwrap();
    assert!(
        result.contains("Summary from stdin pipe."),
        "compact should use stdin content as message, got:\n{result}"
    );
    assert!(
        !result.contains("### Re: topic"),
        "original content should be archived"
    );
}

#[test]
fn test_transfer_exchange_auto_creates_target_with_backlog_and_icebox_scaffold() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let source = root.join("source.md");
    let target = root.join("target.md");

    let source_doc = template_doc(
        "Source",
        "### Re: topic — gpt-5\n\nTransferred exchange body.\n",
        "- [ ] [#back1] Keep this backlog item with the transfer\n",
        "- [ ] [#cold1] Parked context that should transfer too\n",
    );
    fs::write(&source, &source_doc).unwrap();
    init_git_repo(root, &source);
    seed_snapshot(root, &source, &source_doc);

    let mut cmd = agent_doc_cmd();
    cmd.current_dir(root);
    cmd.args([
        "transfer",
        "source.md",
        "target.md",
        "exchange",
        "--bypass-claim",
    ]);
    cmd.assert().success();

    let source_after = fs::read_to_string(&source).unwrap();
    assert!(
        !source_after.contains("Transferred exchange body."),
        "source exchange should be cleared after transfer:\n{source_after}"
    );
    assert!(
        !source_after.contains("[#back1]"),
        "source backlog should be cleared after transfer:\n{source_after}"
    );
    assert!(
        !source_after.contains("[#cold1]"),
        "source icebox should be cleared after transfer:\n{source_after}"
    );

    let target_after = fs::read_to_string(&target).unwrap();
    assert!(target_after.contains("## Status"));
    assert!(target_after.contains("## Queue"));
    assert!(target_after.contains("<!-- agent:backlog -->"));
    assert!(target_after.contains("<!-- agent:icebox -->"));
    assert!(target_after.contains("Transferred exchange body."));
    assert!(target_after.contains("[#back1]"));
    assert!(target_after.contains("[#cold1]"));
}

#[test]
fn test_transfer_items_supports_icebox_component() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let source = root.join("source.md");
    let target = root.join("target.md");

    let source_doc = template_doc(
        "Source",
        "",
        "",
        "- [ ] [#cold1] First parked item\n- [ ] [#cold2] Second parked item\n",
    );
    let target_doc = template_doc("Target", "", "", "- [ ] [#keep1] Existing target item\n");
    fs::write(&source, &source_doc).unwrap();
    fs::write(&target, &target_doc).unwrap();
    init_git_repo(root, &source);
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["add", "target.md"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add target", "--no-verify"])
        .status()
        .unwrap();
    seed_snapshot(root, &source, &source_doc);
    seed_snapshot(root, &target, &target_doc);

    let mut cmd = agent_doc_cmd();
    cmd.current_dir(root);
    cmd.args([
        "transfer",
        "source.md",
        "target.md",
        "icebox",
        "--items",
        "#cold1",
        "--bypass-claim",
    ]);
    cmd.assert().success();

    let source_after = fs::read_to_string(&source).unwrap();
    assert!(
        !source_after.contains("[#cold1]"),
        "matched icebox item should leave source:\n{source_after}"
    );
    assert!(source_after.contains("[#cold2]"));

    let target_after = fs::read_to_string(&target).unwrap();
    assert!(target_after.contains("[#keep1]"));
    assert!(target_after.contains("[#cold1]"));
    assert!(
        !target_after.contains("[#cold2]"),
        "unmatched icebox item should stay in source:\n{target_after}"
    );
}
