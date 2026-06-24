//! CLI integration tests for agent-doc.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
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

fn cycle_state_path(root: &Path, doc: &Path) -> PathBuf {
    let canonical = doc.canonicalize().unwrap();
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let hash = hex::encode(hasher.finalize());
    root.join(".agent-doc/state/cycles")
        .join(format!("{hash}.json"))
}

fn read_cycle_phase(root: &Path, doc: &Path) -> Option<String> {
    let content = fs::read_to_string(cycle_state_path(root, doc)).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    json["phase"].as_str().map(str::to_string)
}

fn git_commit_count(root: &Path) -> usize {
    let output = ProcessCommand::new("git")
        .current_dir(root)
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git rev-list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
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
fn test_cli_admin_json_receipts_and_inspection_cover_controller_paths() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".agent-doc")).unwrap();
    let doc = root.join("tasks/admin-cli.md");
    fs::create_dir_all(doc.parent().unwrap()).unwrap();
    fs::write(
        &doc,
        "---\nagent_doc_session: session-cli-admin\nagent: codex\n---\nBody\n",
    )
    .unwrap();
    agent_doc_orchestration::session_actor::record_session_start_direct(
        &doc,
        "session-cli-admin",
        "%51",
        "@1",
        1,
    )
    .unwrap();
    agent_doc_orchestration::session_actor::transition_state_direct(
        &doc,
        "session-cli-admin",
        "%51",
        Some(1),
        agent_doc_orchestration::session_actor::ActorState::Ready,
        "supervisor",
        "prompt_ready",
    )
    .unwrap();

    let root_arg = root.to_str().unwrap();
    let doc_arg = doc.to_str().unwrap();

    let pause = agent_doc_cmd()
        .args([
            "admin",
            "queue",
            "pause",
            doc_arg,
            "--project-root",
            root_arg,
            "--observed-generation",
            "1",
            "--reason",
            "cli pause",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let pause: serde_json::Value = serde_json::from_slice(&pause).unwrap();
    assert_eq!(pause["operation_kind"], "queue_paused");
    assert_eq!(pause["status"], "accepted");
    assert!(
        pause["document_id"]
            .as_str()
            .unwrap()
            .ends_with("admin-cli.md")
    );

    let stale_handoff = agent_doc_cmd()
        .args([
            "admin",
            "handoff",
            doc_arg,
            "--to-pane",
            "%52",
            "--project-root",
            root_arg,
            "--observed-generation",
            "0",
            "--reason",
            "stale cli handoff",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stale_handoff: serde_json::Value = serde_json::from_slice(&stale_handoff).unwrap();
    assert_eq!(stale_handoff["status"], "rejected");
    assert_eq!(stale_handoff["failed_stage"], "stale_generation");
    assert_eq!(stale_handoff["current_generation"], 1);

    let handoff = agent_doc_cmd()
        .args([
            "admin",
            "handoff",
            doc_arg,
            "--to-pane",
            "%52",
            "--project-root",
            root_arg,
            "--observed-generation",
            "1",
            "--reason",
            "cli handoff",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let handoff: serde_json::Value = serde_json::from_slice(&handoff).unwrap();
    assert_eq!(handoff["operation_kind"], "admin_handoff");
    assert_eq!(handoff["status"], "accepted");

    let inspect = agent_doc_cmd()
        .args([
            "admin",
            "inspect",
            doc_arg,
            "--project-root",
            root_arg,
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let inspect: serde_json::Value = serde_json::from_slice(&inspect).unwrap();
    assert_eq!(inspect["record"]["generation"], 2);
    assert_eq!(inspect["record"]["pane_id"], "%52");
    assert_eq!(inspect["queue_control"]["state"], "paused");
    assert!(
        inspect["admin_operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|operation| operation["operation_kind"] == "queue_paused"
                && operation["status"] == "accepted")
    );
    assert!(
        inspect["admin_operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|operation| operation["operation_kind"] == "admin_handoff"
                && operation["status"] == "accepted")
    );

    let stale_reap = agent_doc_cmd()
        .args([
            "admin",
            "reap",
            doc_arg,
            "--project-root",
            root_arg,
            "--observed-generation",
            "1",
            "--reason",
            "stale cli reap",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stale_reap: serde_json::Value = serde_json::from_slice(&stale_reap).unwrap();
    assert_eq!(stale_reap["status"], "rejected");
    assert_eq!(stale_reap["failed_stage"], "stale_generation");
    assert_eq!(stale_reap["current_generation"], 2);

    let reap = agent_doc_cmd()
        .args([
            "admin",
            "reap",
            doc_arg,
            "--project-root",
            root_arg,
            "--observed-generation",
            "2",
            "--reason",
            "cli reap",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let reap: serde_json::Value = serde_json::from_slice(&reap).unwrap();
    assert_eq!(reap["operation_kind"], "admin_reap");
    assert_eq!(reap["status"], "accepted");
}

#[test]
fn mcp_serve_handles_initialize_list_and_read() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    write!(
        file,
        "<!-- agent:exchange -->\nMCP body\n<!-- /agent:exchange -->\n"
    )
    .unwrap();

    let initialize = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "agent-doc-test", "version": "1" }
        }
    });
    let tools_list = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    });
    let read = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "agent_doc_read",
            "arguments": {
                "file": file.path().display().to_string(),
                "component": "exchange"
            }
        }
    });
    let input = format!("{initialize}\n{tools_list}\n{read}\n");

    let assert = agent_doc_cmd()
        .args(["mcp", "serve"])
        .write_stdin(input)
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let responses: Vec<serde_json::Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();

    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-06-18");
    assert!(
        responses[1]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "agent_doc_finalize")
    );
    assert!(
        responses[1]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "agent_doc_preflight")
    );
    assert!(
        responses[1]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "agent_doc_plan")
    );
    assert_eq!(
        responses[2]["result"]["structuredContent"]["content"],
        "MCP body\n"
    );
}

#[test]
fn mcp_serve_handles_preflight_probe_and_plan() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    let content = template_doc(
        "Session",
        "❯ Please inspect\n<!-- agent:boundary:1234abcd -->\n",
        "",
        "",
    );
    fs::write(&doc, &content).unwrap();
    init_git_repo(tmp.path(), &doc);
    seed_snapshot(tmp.path(), &doc, &content);

    let preflight = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "agent_doc_preflight",
            "arguments": {
                "file": doc.display().to_string(),
                "probe": true
            }
        }
    });
    let plan = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "agent_doc_plan",
            "arguments": {
                "file": doc.display().to_string()
            }
        }
    });

    let assert = agent_doc_cmd()
        .current_dir(tmp.path())
        .args(["mcp", "serve"])
        .write_stdin(format!("{preflight}\n{plan}\n"))
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let responses: Vec<serde_json::Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();

    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["result"]["isError"], false);
    assert_eq!(
        responses[0]["result"]["structuredContent"]["report"]["no_changes"],
        true
    );
    assert_eq!(responses[1]["result"]["isError"], false);
    assert_eq!(
        responses[1]["result"]["structuredContent"]["plan"]["execution_scope"],
        "normal"
    );
}

#[test]
fn mcp_finalize_uses_strict_write_commit_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(
        &doc,
        template_doc(
            "Session",
            "❯ Please reply\n<!-- agent:boundary:1234abcd -->\n",
            "",
            "",
        ),
    )
    .unwrap();
    init_git_repo(tmp.path(), &doc);

    let finalize = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "agent_doc_finalize",
            "arguments": {
                "file": doc.display().to_string(),
                "response": "<!-- patch:exchange -->\n### Re: MCP finalize - gpt-5\nbody\n<!-- /patch:exchange -->\n"
            }
        }
    });

    let assert = agent_doc_cmd()
        .current_dir(tmp.path())
        .args(["mcp", "serve"])
        .write_stdin(format!("{finalize}\n"))
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let response: serde_json::Value = serde_json::from_str(stdout.lines().next().unwrap()).unwrap();
    assert_eq!(response["result"]["isError"], false);

    let head_blob = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&head_blob.stdout).contains("### Re: MCP finalize - gpt-5"),
        "HEAD blob should contain the MCP finalize response"
    );
}

#[test]
fn mcp_finalize_close_after_capture_recovers_on_next_preflight_once() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let doc = root.join("session.md");
    let content = template_doc(
        "Session",
        "❯ Please reply\n<!-- agent:boundary:1234abcd -->\n",
        "",
        "",
    );
    fs::write(&doc, &content).unwrap();
    init_git_repo(root, &doc);
    seed_snapshot(root, &doc, &content);
    agent_doc_orchestration::cycle_state::start_preflight(&doc, Some(&content), Some(&content))
        .unwrap();

    let response =
        "<!-- patch:exchange -->\n### Re: MCP finalize - gpt-5\nbody\n<!-- /patch:exchange -->\n";
    agent_doc_orchestration::repair::save_pending(&doc, response).unwrap();
    assert_eq!(
        read_cycle_phase(root, &doc).as_deref(),
        Some("response_captured")
    );

    let before = git_commit_count(root);
    let mut preflight = agent_doc_cmd();
    preflight.current_dir(root);
    preflight.args(["preflight", doc.to_str().unwrap()]);
    preflight.assert().success();
    let after = git_commit_count(root);
    assert_eq!(
        after,
        before + 1,
        "recovery preflight should create exactly one closeout commit"
    );

    let head_blob = ProcessCommand::new("git")
        .current_dir(root)
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    let head_doc = String::from_utf8_lossy(&head_blob.stdout);
    assert_eq!(
        head_doc.matches("### Re: MCP finalize - gpt-5").count(),
        1,
        "recovery should commit the captured response exactly once:\n{head_doc}"
    );
    assert!(
        !agent_doc_orchestration::snapshot::pending_path_for(&doc)
            .unwrap()
            .exists(),
        "pending response should be cleared after recovery"
    );

    let mut check = agent_doc_cmd();
    check.current_dir(root);
    check.args(["session-check", doc.to_str().unwrap()]);
    let check_output = check.assert().success().get_output().stdout.clone();
    let check_stdout = String::from_utf8(check_output).unwrap();
    assert!(
        check_stdout.contains("[session-check] ok"),
        "session-check should be clean after close-after-capture recovery:\n{check_stdout}"
    );
}

#[test]
fn test_codex_shared_closeout_spec_invariants() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let codex_support = fs::read_to_string(root.join("specs/codex-support.md")).unwrap();
    let agent_backend = fs::read_to_string(root.join("specs/05-agent-backend.md")).unwrap();
    let closeout = fs::read_to_string(root.join("specs/07-closeout-commands.md")).unwrap();

    assert!(
        codex_support.contains("Codex differs from Claude Code")
            && codex_support.contains("layers only"),
        "Codex spec must keep harness differences scoped to launch/routing/backend"
    );
    assert!(
        codex_support.contains("Do not standardize on the Claude Code slash command"),
        "Codex spec must not prescribe the Claude-only slash/Skill path"
    );
    assert!(
        codex_support.contains("full-content editor IPC disabled"),
        "Codex spec must preserve the shared full-content IPC ban"
    );
    assert!(
        agent_backend
            .contains("Agent backends are response producers, not session-document writers"),
        "backend spec must keep document mutation outside harness backends"
    );
    assert!(
        closeout.contains("## Harness-neutral closeout"),
        "closeout spec must name the shared harness-neutral closeout boundary"
    );
    assert!(
        closeout.contains("Codex direct")
            && closeout.contains("Codex Stop hook")
            && closeout.contains("recovery/backstop inputs"),
        "closeout spec must route Codex hook recovery through shared finalize/write machinery"
    );
}

#[test]
fn live_tmux_tests_are_not_in_default_development_suite() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources = [
        "src/autoclaim.rs",
        "agent-doc-orchestration/src/focus.rs",
        "agent-doc-orchestration/src/resync.rs",
        "agent-doc-orchestration/src/route.rs",
        "src/session_actor_cmd.rs",
        "agent-doc-orchestration/src/sessions.rs",
        "agent-doc-orchestration/src/start.rs",
        "agent-doc-orchestration/src/sync.rs",
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
fn process_global_test_mutations_share_session_check_lock() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let test_support = fs::read_to_string(manifest_dir.join("src/test_support.rs")).unwrap();
    assert!(
        test_support.contains("crate::test_support::TEST_ENV_LOCK")
            && test_support.contains("struct ProcessGlobalLockGuard")
            && test_support.contains("PROCESS_GLOBAL_LOCK_DEPTH")
            && test_support.contains("struct ScopedCurrentDir")
            && test_support.contains("std::env::set_current_dir(path)")
            && test_support.contains("std::env::set_current_dir(&self.prev_cwd)"),
        "test_support must route env and cwd test guards through a reentrant shared process-global lock"
    );

    // session_check was decomposed into functional submodules (#splitmods4); its
    // integration-style tests + shared inspection helpers were bundled back inline
    // into `session_check.rs`'s own `#[cfg(test)] mod tests` (the helpers shadow
    // core fn names like `inspect`, so they stay in the core test mod).
    let session_check_tests =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/session_check.rs"))
            .unwrap();
    assert!(
        session_check_tests.contains("fn inspect(file: &std::path::Path)")
            && session_check_tests.contains("fn inspect_with_warnings(file: &std::path::Path)")
            && session_check_tests
                .contains("let _process_global_lock = crate::test_support::env_lock()"),
        "session_check test inspection helpers must use the crate-wide process-global lock"
    );

    let pty =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/supervisor/pty.rs"))
            .unwrap();
    assert!(
        pty.contains("struct EnvGuard")
            && pty.contains("AGENT_DOC_PTY_PARENT_LEAK")
            && pty.contains("std::env::remove_var(self.key)"),
        "parent env leak test must restore process env through a guard"
    );
}

#[test]
fn flowcore_hot_path_guard_and_proof_tokens_are_budgeted() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let hot_paths = [
        "agent-doc-orchestration/src/git.rs",
        "agent-doc-orchestration/src/git/safe_mutation.rs",
        "agent-doc-orchestration/src/git/normalize.rs",
        "agent-doc-orchestration/src/git/dirs.rs",
        "src/orchestrate.rs",
        "src/orchestrate/dag.rs",
        "agent-doc-orchestration/src/preflight.rs",
        "agent-doc-orchestration/src/preflight/run.rs",
        "agent-doc-orchestration/src/preflight/maintenance.rs",
        "agent-doc-orchestration/src/preflight/semantic_diff.rs",
        "agent-doc-orchestration/src/repair.rs",
        "agent-doc-orchestration/src/route.rs",
        "agent-doc-orchestration/src/route/dispatch_only.rs",
        "agent-doc-orchestration/src/route/authoritative_actor.rs",
        "agent-doc-orchestration/src/route/pane_resolution.rs",
        "agent-doc-orchestration/src/route/dispatch.rs",
        "agent-doc-orchestration/src/route/session_resolution.rs",
        "agent-doc-orchestration/src/route/cycle_ack.rs",
        "agent-doc-orchestration/src/route/busy_pane.rs",
        "agent-doc-orchestration/src/route/startup.rs",
        "agent-doc-orchestration/src/session_check.rs",
        "agent-doc-orchestration/src/session_check/partial_staging.rs",
        "agent-doc-orchestration/src/session_check/closeout_guards.rs",
        "agent-doc-orchestration/src/session_check/queue_head_provenance_guards.rs",
        "agent-doc-orchestration/src/session_check/pending_guards.rs",
        "agent-doc-orchestration/src/session_check/queue_head_guards.rs",
        "agent-doc-orchestration/src/session_check/response_guards.rs",
        "agent-doc-orchestration/src/session_check/detect.rs",
        "agent-doc-orchestration/src/write.rs",
        "agent-doc-orchestration/src/write/queue_consume.rs",
        "agent-doc-orchestration/src/write/ipc.rs",
        "agent-doc-orchestration/src/write/ipc/transport.rs",
        "agent-doc-orchestration/src/write/normalize.rs",
        "agent-doc-orchestration/src/write/converge.rs",
        "agent-doc-orchestration/src/write/pending_checks.rs",
        "agent-doc-orchestration/src/write/materialize.rs",
        "agent-doc-orchestration/src/write/exchange_reconcile.rs",
        "agent-doc-orchestration/src/write/run_entry.rs",
    ];
    let tokens = [
        "guard_",
        "proof=",
        "proof_scope=",
        "reason=",
        "flow_reason=",
        "accepted_only",
    ];
    let mut violations = Vec::new();

    for source in hot_paths {
        let content = fs::read_to_string(manifest_dir.join(source)).unwrap();
        for token in tokens {
            let actual = content.matches(token).count();
            let expected = flowcore_hot_path_token_budget(source, token);
            if actual != expected {
                violations.push(format!(
                    "{source} token `{token}`: expected {expected}, got {actual}"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "FlowCore regression gate failed. New hot-path guard/proof/reason tokens must be audited and routed through the owning flow enum/event before this budget changes:\n{}",
        violations.join("\n")
    );
}

fn flowcore_hot_path_token_budget(source: &str, token: &str) -> usize {
    match (source, token) {
        // 19 -> 13: the `#[cfg(test)] mod tests` was extracted into
        // `git/tests.rs` (large-module split, #splitmods2). The 6 removed
        // `guard_` occurrences were test-assertion literals, not production
        // hot-path guards.
        // +1 (#fccaudit): `strip_guard_markers_from_disk` now routes the
        // working-tree marker strip through `converge_or_disk_write(...,
        // "strip_guard_markers")` instead of a bare `std::fs::write`, so no
        // disk write touches the session doc behind a live JB editor listener.
        // The new `guard_` occurrence is only the `"strip_guard_markers"` source
        // label string — not a new flow guard boundary.
        ("agent-doc-orchestration/src/git.rs", "guard_") => 19,
        ("agent-doc-orchestration/src/git/normalize.rs", "guard_") => 1,
        // +1 (`reason=committed_content_lost`): #pcwc post-commit auto-reconcile
        // logs when it restored the working tree to HEAD because committed content
        // was dropped with no new user work (vs a preserved carry-forward superset).
        // +2 (`reason=no_listener`, `reason=no_ack`): #pcwc post-commit editor-buffer
        // refresh logs when it skipped the IPC push back to the IDE because no
        // listener was active or the plugin sent no ack after the HEAD-authoritative
        // working-tree repair (so the IDE stops writing the stale buffer back).
        // +2 (`reason=clear_carry_forward_drift`, `reason={e}`): #jb-editor-save-resolves-drift
        // post-commit editor flush logs whether it asked the live plugin to save its
        // (carry-forward-superset) buffer to clear the dirty flag, or skipped/failed —
        // routed through `flush_editor_buffer_to_clear_drift`.
        // +2 (file-signal `reason=clear_carry_forward_drift`, `reason={e}`):
        // #jbeditorsavedrift-vscode adds a file-based `save-document.signal` fallback
        // in the same `flush_editor_buffer_to_clear_drift` flow for editors that watch
        // `.agent-doc/patches/` (VS Code) instead of the socket (JetBrains).
        // +2 (#pcwcdiskfree): post-commit auto-reconcile now logs which
        // transport it used (`transport=editor_ipc_skipped_disk_write` when a JB
        // listener is active vs `transport=disk` headless), so a hot editor
        // buffer no longer triggers `File Cache Conflict`. Each branch keeps its
        // own `reason=committed_content_lost transport=…` log line, replacing
        // the prior single unconditional `reason=committed_content_lost`.
        // +2 (`#pcwcwarn`): the per-component stale-exchange reconcile logs
        // `reason=stale_editor_exchange transport=…` on both the editor-IPC and
        // disk transports, the INVERSE of `#qpcwcmerge` (HEAD wins inside the
        // agent-owned `exchange`, editor wins outside it).
        // +2 (`#pzjy`): the per-component stale-queue reconcile logs
        // `reason=stale_editor_queue_resurrection transport=…` on both the
        // editor-IPC and disk transports so committed completed queue rows win
        // over stale live-buffer unstrikes before the generic editor flush.
        // +3 (`#editorbufwin` P2): one production
        // `reason=preserved_queue_addition_replay_neutralized` marker plus two
        // regression assertions prove replay-neutralized queue additions are
        // committed only after closeout recovery evidence, not during ordinary
        // independent queue edits.
        ("agent-doc-orchestration/src/git.rs", "reason=") => 20,
        ("src/orchestrate.rs", "guard_") => 0,
        ("src/orchestrate/dag.rs", "guard_") => 2,
        // +1 (`reason=probe_inspection_only`): `preflight --probe` logs why it
        // skipped opening a `preflight_started` cycle (#preflight-probe-side-effect-free).
        // +1 (`reason=struck_items_below_close_marker`): queue-escape repair logs
        // when it removed struck queue items displaced below the closing marker
        // (#queue-completed-items-escape-below-component).
        ("agent-doc-orchestration/src/preflight.rs", "reason=") => 2,
        ("agent-doc-orchestration/src/preflight/run.rs", "reason=") => 1,
        // 1 -> 2 (`reason=clean_session|operator_verify`): the go-mode
        // backlog→queue sync skips agent-undrainable heads and logs
        // `go_queue_skip_undrainable id=#<id> reason=<reason> session=<...>`
        // (#goqueuestall).
        // 2 -> 3 (`reason=plugin_listener_active`): `persist_queue_maintenance_doc`
        // logs `write_authority action=routed reason=plugin_listener_active
        // surface=queue_maintenance` when it routes a queue-maintenance mutation
        // through the editor IPC instead of a raw disk write (#fccqueue).
        // 3 -> 4 (`reason=queue_edit_lease`): `run_queue_maintenance` logs
        // `queue_maintenance_deferred reason=queue_edit_lease holder_pid=<pid>`
        // when it defers ALL queue mutation while a different live process holds a
        // fresh queue-edit lease (a direct `queue prune-noise`/`consume` in flight;
        // #sqedit-race Phase 2).
        // 4 -> 5 (`reason=force_disk`): closeout pending maintenance now logs
        // the explicit operator escape hatch used by `finalize --force-disk` when
        // it bypasses a stale active editor listener instead of failing before
        // the binary-owned closeout boundary (#pzjy closeout recovery).
        ("agent-doc-orchestration/src/preflight/maintenance.rs", "reason=") => 5,
        ("agent-doc-orchestration/src/repair.rs", "guard_") => 10,
        ("agent-doc-orchestration/src/repair.rs", "reason=") => 5,
        ("agent-doc-orchestration/src/route.rs", "accepted_only") => 2,
        ("agent-doc-orchestration/src/route.rs", "flow_reason=") => 2,
        // +5 for the audited `#snrun` blocked-in-interactive-substate guard:
        // the `dispatch_only_blocked_guard_reason` import + its `guard_reason`
        // binding, the `log_prompt_ready_barrier_failed(file, guard_reason)`
        // emission, and the `guard_reason == ...BlockedInInteractiveSubstate`
        // branch that names the interactive terminal substate distinctly from a
        // generic busy actor on the dispatch-only fail-closed path. Routed
        // through the `RoutedReopenGuardReason` enum + `prompt_ready_barrier`
        // FlowEvent.
        ("agent-doc-orchestration/src/route.rs", "guard_") => 21,
        // +2 (#jb-run-agent-doc-submit-diagnostics): the redacted
        // `route_submit_observation` / `route_submit_issue` helpers can include
        // dispatch-start proof labels while keeping prompt-submit failures
        // visible in ops-log review.
        ("agent-doc-orchestration/src/route.rs", "proof=") => 3,
        // +1 for the audited route resilience diagnostic
        // `route_queue_dispatch_unparseable_preserved ... reason={parse_err}`:
        // when the existing agent:queue is polluted/unparseable the route
        // preserves it and appends the new dispatch instead of bailing fatally
        // (the "route queue dispatch: failed to parse existing agent:queue" bug).
        // +1 for the audited `#codex-route-busy-ctrl-g-opens-editor` diagnostic
        // `route_busy_existing_pane_interrupt_skipped_ctrl_g ... reason=not_shell_search`:
        // the busy-pane reroute now logs when it skips the editor-opening `C-g`
        // because the live Codex pane is not in a shell reverse-i-search /
        // history-search state, and goes straight to the Escape + C-c path.
        // +1 for the audited `#qdispatchloss` diagnostic
        // `route_dispatch_uncommitted_head ... reason=head_not_in_committed_snapshot`:
        // the inactive-queue-head read defers (returns None) instead of
        // consuming a head that is not backed by the committed snapshot (an
        // uncommitted editor-buffer-only operator edit), so route never feeds a
        // half-typed/uncommitted line into the agent prompt and loses it.
        ("agent-doc-orchestration/src/route.rs", "reason=") => 9,
        ("agent-doc-orchestration/src/route/dispatch_only.rs", "guard_") => 4,
        // +1/+1 (#jbsimpleroute): the Codex accepted-only delivery regression
        // asserts the typed `proof=accepted proof_scope=accepted_only` log shape
        // now that accepted Enter delivery is success instead of a hard route
        // failure.
        ("agent-doc-orchestration/src/route/dispatch_only.rs", "proof=") => 5,
        ("agent-doc-orchestration/src/route/dispatch_only.rs", "proof_scope=") => 5,
        // +4 (#jbdisprecycle): the recycle-in-flight dispatch guard adds four
        // audited `reason=` tokens — the `dispatch_only_recycle_inflight_error`
        // message (`reason={reason}`) plus three ops_log lines
        // (`route_dispatch_only_recycle_inflight_wait/_unsettled/_settled
        // ... reason={}`). All carry the supervisor recycle reason
        // (`auto_install_reexec` / `restart_reexec`) so a deferred-then-settled
        // or fail-closed dispatch is auditable against the marker that gated it.
        ("agent-doc-orchestration/src/route/dispatch_only.rs", "reason=") => 6,
        // +1 (#3x90): regression coverage for tracked dispatch-start timeouts
        // asserts that `DispatchStartUnproven` fails closed even though plain
        // `CommandAcceptedOnly` remains allowed for harnesses with no tracker.
        ("agent-doc-orchestration/src/route/dispatch_only.rs", "accepted_only") => 10,
        // +1 (`reason=in_flight_coalesce`): #qflood2 `route_dispatch_deduped_pane`
        // logs the benign in-flight dedup before returning deduped-success without a
        // re-send. Routed through the `RouteDispatchAuthorization::CoalescedDeduped`
        // outcome so every dispatch site handles the coalesce at compile time.
        ("agent-doc-orchestration/src/route/authoritative_actor.rs", "reason=") => 2,
        ("agent-doc-orchestration/src/route/pane_resolution.rs", "guard_") => 1,
        ("agent-doc-orchestration/src/route/pane_resolution.rs", "reason=") => 4,
        ("agent-doc-orchestration/src/route/dispatch.rs", "proof=") => 2,
        // +1 (#1vhn `reason=harness_exited_to_bare_shell`): the pre-send
        // dead-harness guard logs `route_dispatch_into_dead_shell_blocked` when
        // the harness has crashed/exited to a bare interactive shell, so route
        // fails closed instead of typing the trigger into the dead shell.
        // +2 (#jbtsiftnosub
        // `reason=harness_not_dispatch_ready_before_auto_start_send`,
        // `reason=harness_exited_to_bare_shell_before_auto_start_send`): the
        // auto-start cold-start re-verify gate (`reverify_auto_start_dispatch_ready`)
        // fails closed and logs `dispatch_into_starting_pane` / `dispatch_into_shell`
        // when a freshly created pane is still cold-starting (or dropped to a bare
        // shell) at send time, instead of typing into a not-yet-submit-ready composer.
        ("agent-doc-orchestration/src/route/dispatch.rs", "reason=") => 3,
        ("agent-doc-orchestration/src/route/busy_pane.rs", "reason=") => 1,
        // +8 for the audited `#do-id-closeout-open-backlog` guard:
        // `expect_done_or_gate_guard_fired` ops_log diagnostic plus seven
        // `expect_done_or_gate_guard_*` test names. +1 for the audited
        // `#queue-user-edit-overwrite` `dropped_queue_prompt_guard_failed`
        // ops_log diagnostic. +4 for the audited
        // `#jb-run-agent-doc-response-queue-contamination` guard:
        // `queue_response_contamination_guard_failed` ops_log diagnostic plus
        // three `queue_contamination_guard_*` test names. All follow the same
        // ops_log pattern as the sibling session-check pending guards.
        // +9 for the audited `#blocked-closeout-followup-capture` guard:
        // the `resolve_pending_done_guard_mode` reuse in
        // `check_blocked_closeout_followup_guard`, the
        // `blocked_closeout_followup_guard_fired` ops_log diagnostic, and seven
        // `blocked_closeout_followup_guard_*` test names. Same ops_log pattern
        // as the sibling `expect_done_or_gate` / partial-closeout guards.
        // +2 for the audited no-op-commit exemption of the
        // `committed_without_response_body` guard (tsift.md deadlock): the
        // `committed_without_response_body_guard_skipped_noop_commit` ops_log
        // diagnostic plus its `..._guard_skips_noop_commit_reap_only_cycle`
        // regression test name. A no-op commit (`commit_already_current`)
        // committed no binary-owned work, so the guard skips it instead of
        // looping the cycle forever.
        // +6 (#gated-followup-split-enforcement): one `gated_phase_split_guard_fired`
        // ops event for the warn-first multi-phase split advisory, plus its five
        // `check_gated_phase_split_guard` regression test-fn names.
        // +6 (#queue-audit-partial-completion): one `queue_audit_partial_completion_guard_fired`
        // ops event for the warn-first queue-audit collapse advisory, plus its
        // five `queue_audit_guard_*` regression test-fn names.
        // +5 (#lr-config-3): three `_with_context` guard resolution variants
        // (pending_capture, pending_done, review_done) mirroring the originals
        // for RunContext-backed project config access, plus 2 additional
        // guard-mode resolution call sites in queue-head removal logic.
        // +4 (#queue-clear-unrun-items): the four `queue_head_removal_guard_*`
        // regression test-fn names for `check_queue_head_removal_guard` (its
        // `resolve_pending_done_guard_mode` reuse + `queue_head_removal_guard_fired`
        // ops diagnostic are already counted in the #lr-config-3 line above).
        // +3 (#lr-content-6): the Phase 6 regression that proves the guard-mode
        // resolvers read from the cached `FrontmatterSlot` — the
        // `phase6_guard_mode_resolves_from_frontmatter_slot_not_file` test-fn name
        // plus its two `resolve_pending_done_guard_mode_with_context` call sites.
        // The guard sweep converting `check_pending_*` / `check_expect_*` /
        // `check_blocked_*` / `check_queue_head_removal_guard` over to the
        // `_with_context` resolvers is a 1:1 token-for-token swap (no net change).
        // +1 (#nochange-after-stall-breadth): the no-response active-queue-head
        // closeout check reuses pending_done_guard mode for strict/warn/off policy.
        // +2 (#codex-queue-drain-no-response-body): two new test fn names
        // `committed_without_response_body_guard_{fires,passes}_…` contain the
        // `guard_` substring (test identifiers, not new flow guards).
        // +1 (#lazily-missing-response-recovery): one regression test name
        // proves recovered committed exchange bodies clear the same guard even
        // after stale cycle capture metadata was lost.
        // +4 (#partial-staging-closeout-guard): one session-check guard call
        // site, one guard function, one ops-log diagnostic, and one regression
        // test name for dirty companion source/test changes with overlapping
        // changed string literals after a partial manual commit.
        // +2 (#lr-queue-patchback-miss): two regression test names for free-text
        // queue-head provenance after binary consume without exchange history.
        // +1 (#compact-reap-no-response-record): the
        // `resolve_pending_done_guard_mode_with_context` reuse in
        // `check_reaped_queue_head_without_response` (substring `guard_` in
        // `..._guard_mode_...`). The guard fails closed when a no-response reap-only
        // closeout reaped a `do`-directive head whose `### Re:` never landed in the
        // exchange or a HEAD compact archive; it reuses the same guard-mode
        // resolution as the sibling no-response-active-head guard. Its own ops_log
        // diagnostic (`reaped_queue_head_without_response_fired`) carries no
        // `guard_` substring.
        // +2 (#queue-contamination-guard-false-positive): two new
        // `queue_contamination_guard_*` regression test-fn names
        // (`..._skips_user_prompt_mentioning_slash_command` and
        // `..._still_flags_prose_without_slash_command`) for the slash-command
        // skip that stops the contamination guard from flagging legit user
        // prompts that mention `/agent-doc`/`/clear`. The skip itself
        // (`mentions_slash_command`) carries no `guard_` substring.
        // +1 (#partial-staging-guard-cross-doc-noise): the
        // `partial_staging_closeout_guard_ignores_cross_document_markdown_noise`
        // regression test-fn name (substring `guard_`). The fix itself drops `md`
        // from `is_partial_staging_relevant_path` and adds no `guard_` token.
        // +2 (#eqrecovery): the
        // `committed_without_response_body_guard_skips_equityfundingsource_noop_queue_recovery`
        // regression test-fn name plus direct guard/log assertions. It proves a
        // drained queue/backlog recovery carrying queue-turn evidence remains
        // terminal when the commit event is `commit_already_current`.
        // 96 -> 31: the `#[cfg(test)] mod tests` was extracted into
        // `session_check/tests.rs` (large-module split). The 65 removed `guard_`
        // occurrences were test-assertion literals, not production hot-path
        // guards; only production `guard_` tokens are budgeted here now.
        ("agent-doc-orchestration/src/session_check.rs", "guard_") => 65,
        // +1 (#qpausemix-verify / #j9ja): when `queue_continuation_required` is
        // emitted on a controller-paused queue, session-check now drops a
        // distinctive `queue_paused_continuation_guidance_emitted pause_reason={..}`
        // SUCCESS marker into ops.log so an operator live test of the pause-aware
        // guidance is provable/disprovable from the log (auto-verify keys on
        // `ops_log:queue_paused_continuation_guidance_emitted`). The recorded
        // `pause_reason=` is the controller pause text, not a new flow boundary.
        ("agent-doc-orchestration/src/session_check.rs", "reason=") => 1,
        ("agent-doc-orchestration/src/session_check/closeout_guards.rs", "guard_") => 4,
        // +3 (#mrhqueuepreserve): the audited
        // `queue_head_removal_guard_proof` diagnostic plus two regression test
        // names proving removed id-backed/free-text queue heads log their proof
        // source instead of disappearing silently.
        // +3 (#qheadresidue): the audited
        // `free_text_queue_completed_residue_guard_fired` diagnostic plus two
        // regression test names proving answered free-text heads cannot remain
        // active queue residue.
        ("agent-doc-orchestration/src/session_check/queue_head_provenance_guards.rs", "guard_") => {
            12
        }
        ("agent-doc-orchestration/src/session_check/pending_guards.rs", "guard_") => 8,
        ("agent-doc-orchestration/src/session_check/queue_head_guards.rs", "guard_") => 2,
        ("agent-doc-orchestration/src/session_check/partial_staging.rs", "guard_") => 2,
        ("agent-doc-orchestration/src/session_check/response_guards.rs", "guard_") => 8,
        ("agent-doc-orchestration/src/session_check/detect.rs", "guard_") => 1,
        // 70 baseline + 1 for the audited `recguard_wedge` clear call on the
        // #recguard-wedge-escape head-consumed reset path (substring `guard_`
        // comes from the module name `recguard_wedge`, not a new flow guard).
        // +1 for the audited `guard_visible_write_idle(..., "queue_done_id_mark")`
        // call: opportunistic done-id queue marking is a document write path and
        // must use the same visible editor drift guard as active-head queue consume.
        // +2 for the audited explicit-baseline replay guard: one guard function and
        // one strict write call site reject stale-baseline responses after commit.
        // +10 for the audited #ipc-drift-visbuf-reconcile foreign-disk-write
        // reconcile path: the `guard_visible_write_reconcile` function plus its
        // production call sites in `run_template`/`run_stream`, the
        // `reconcile_visible_write` loop helper references, and the
        // deterministic reconcile unit tests. A clean CRDT merge that hits a
        // foreign agent-doc disk append re-merges instead of failing closed.
        // +1 (#nm1x) for the audited `guard_visible_write_reconcile` call in the
        // `visible_write_reconcile_treats_editor_matching_disk_as_reconcilable_drift`
        // regression test: a live-buffer divergence whose editor digest equals the
        // current disk content is reconcilable, not a fail-closed user edit — it
        // reuses the existing visible-write guard, no new flow token.
        // +2 (#exch-intermix) for the two doc-comment cross-references in the
        // `live_prompt_drift_auto_recovery_safe` / `try_auto_recover_live_prompt_drift`
        // auto-recovery helpers: they name the existing `guard_ipc_snapshot_adoption_against_live_prompt_drift`
        // and `guard_no_stale_snapshot_reset_drift` guards to explain the wedge
        // they recover. No new guard flow token — the recovery reuses the existing
        // commit-time `guard_no_stale_snapshot_reset_drift` boundary.
        // +2 (#8j86) for the audited `crate::git::strip_guard_markers(&probe)` call
        // in `response_materialization_probe_from_response` plus its doc-comment
        // cross-reference: the materialization probe strips the same ephemeral
        // guard markers `git::commit` strips, so a captured response body carrying
        // `<!-- no-pending-done-guard -->` still matches the committed HEAD/archive
        // blob and `stuck_captured_cycle` stops false-alarming. Reuses the existing
        // `git::strip_guard_markers` helper — no new flow guard token.
        // +1 (#mrhipcdrift) for the audited visible-write idle/current guard on
        // the socket already_applied missing-disk-response repair path. The
        // recovery writes only the visible response materialization, then keeps
        // the committed snapshot on content_ours instead of falling back through
        // stale file IPC.
        // +1 (#queueeditloss) for the regression's direct call to the existing
        // live-prompt-drift IPC adoption guard. No new guard boundary; the fix
        // reconciles live `agent:queue` edits inside the existing guard.
        // 91 -> 89 (#fcc0): the queue-consume and done-id-mark direct
        // `guard_visible_write_idle(...)` calls were replaced by the shared
        // `converge_document_or_disk` gate, which routes the no-listener disk
        // fallback through the single `guard_visible_write_idle_and_current`
        // guard inside `atomic_write_if_current_pub`. Fewer hot-path guard
        // tokens, not more — the guard boundary is centralized, not added.
        // +1 (#ipcproofcloseout): `run_command` now invokes the existing stale
        // snapshot reset-drift guard before granular pending/review/status
        // mutations, so a failed finalize cannot alter backlog state without the
        // exchange response. Reuses the existing reset-drift boundary.
        ("agent-doc-orchestration/src/write.rs", "guard_") => 46,
        ("agent-doc-orchestration/src/write/pending_checks.rs", "guard_") => 4,
        ("agent-doc-orchestration/src/write/materialize.rs", "guard_") => 3,
        ("agent-doc-orchestration/src/write/exchange_reconcile.rs", "guard_") => 5,
        // -2 `guard_`, -1 `reason=` (#nodiskipc): active IPC timeout/no-proof
        // paths no longer enter the direct document-write fallback, so the removed
        // visible-write guard/reason tokens are retired rather than rerouted.
        ("agent-doc-orchestration/src/write/run_entry.rs", "guard_") => 10,
        ("agent-doc-orchestration/src/write/run_entry.rs", "reason=") => 1,
        // queue-prompt consumption, IPC transport/repair, and live-prompt-drift
        // convergence extracted into write/queue_consume.rs, write/ipc.rs, and
        // write/converge.rs (#splitmods3 large-module split). The moved
        // `guard_`/`reason=` tokens are tracked against the new submodules,
        // not added anew.
        ("agent-doc-orchestration/src/write/queue_consume.rs", "guard_") => 1,
        // +4 (#freshqueueauth): direct queue-head removals now log explicit
        // proof fields for prune/orphan/acknowledgement paths, and the new
        // acknowledgement regression asserts that proof marker. The operations
        // stay routed through the existing queue-consume/converge write boundary.
        ("agent-doc-orchestration/src/write/queue_consume.rs", "proof=") => 4,
        // 1 -> 4 (#editorbufwin Fix A): the queue-consume head-equality check now
        // reconciles a benign live-buffer head divergence instead of hard-bailing,
        // mirroring the existing remaining-queue `reason=crdt_merge_authoritative`
        // reconcile. The new `reason=live_buffer_addition_authoritative` token
        // appears in the production ops_log line, its explanatory comment, and the
        // regression test assertion — gated on recorded dropped-queue evidence
        // (no evidence still bails, preserving the corruption guard).
        ("agent-doc-orchestration/src/write/queue_consume.rs", "reason=") => 4,
        // +4 `guard_` (#dupcontent: two `guard_adopts/refuses_*` adoption tests
        // + two `guard_ipc_snapshot_adoption_against_live_prompt_drift` calls in
        // those tests) and +2 `reason=` (the two `content_ours_adoption_refused_structural`
        // ops_log lines gating corrupt-buffer adoption on both content_ours guards).
        // +4 `guard_` (#dupcontent2): two stale-supervisor adoption guard tests
        // plus two calls through the existing guard functions; no new guard
        // boundary is introduced. +1 `reason=` for the audited
        // `content_ours_adoption_refused_stale_supervisor ... reason=supervisor_binary_stale`
        // ops-log diagnostic routed through `log_ipc_proof_failure`.
        // -2 `guard_`, +1 `reason=` (#nodiskipc): sidecar-normalization and IPC
        // dedupe repair no longer fall back to direct disk repair guards when
        // editor redelivery is unproven; they fail closed with a retry reason.
        // -2 `reason=` (#fcc0-degraded-file-ipc): the degraded convergence
        // regression assertions moved from the old disk-fallback
        // `reason=listener_degraded*` shape to `degraded_cause=...`, because the
        // raw disk fallback is gone and this is no longer a flow reason.
        // +1 `reason=` (#wedged-ipc-ack-probe): the degraded-socket self-heal
        // path now logs `ipc_socket_degraded_self_heal_probe_failed ... reason=...`
        // when `ipc.sock` is connectable but the plugin does not ack, keeping
        // the session on file-IPC instead of clearing the latch from connect-only
        // evidence.
        // 12 -> 16 (#smconv): +4 `guard_` are test-assertion calls to
        // `guard_ipc_snapshot_adoption_against_live_prompt_drift(` in the new
        // `#smconv` semantic-merge convergence tests — not production hot-path
        // guards (the production guard count is unchanged).
        // 16 -> 17 (#qdup-freetext): +1 more test-assertion call to the same guard
        // in `smconv_preserves_freetext_fenced_queue_head_on_drift`, which proves a
        // multi-line free-text queue head now converges instead of blocking. Still a
        // test call, not a production guard boundary.
        ("agent-doc-orchestration/src/write/ipc.rs", "guard_") => 17,
        // 17 -> 18 (#smconv): +1 production `reason=node_keyed_semantic_merge` on
        // the new `live_prompt_drift_semantic_merged` ops_log — the node-keyed
        // merge success path, mirroring the sibling `#fintol2`
        // `reason=independent_concurrent_edit` forward-merge log (an ops_log
        // human-reason, not a new flow-enum outcome).
        ("agent-doc-orchestration/src/write/ipc.rs", "reason=") => 18,
        // +1 `guard_` (#fcc0-degraded-file-ipc): `IpcPollOptions::convergence`
        // centralizes the existing committed-cycle file-IPC poll guard for
        // convergence callers; this is a constructor for the existing guard, not
        // a new flow guard boundary.
        ("agent-doc-orchestration/src/write/ipc/transport.rs", "guard_") => 9,
        ("agent-doc-orchestration/src/write/ipc/transport.rs", "reason=") => 10,
        // +2 (#docdriftgrace): the stale-snapshot reset regression tests call the
        // existing `guard_no_stale_snapshot_reset_drift` boundary for the safe
        // visible rebase and fail-closed active-driver cases. +2 (#docdriftfinalize):
        // compact-summary stream-write rebase and fake-summary rejection tests call
        // the same existing boundary. The production guard boundary is unchanged;
        // the new matches are test coverage.
        ("agent-doc-orchestration/src/write/converge.rs", "guard_") => 9,
        // +9 (#fcc0-no-external-write): active editor listeners no longer allow
        // disk fallback when component convergence cannot prove editor apply.
        // The added `reason=` tokens are the blocked production reasons
        // (`no_component_delta`, `no_ack_content`, `ack_mismatch`, `no_ack`,
        // `send_failed`, and auto-recovery `editor_ipc_unconfirmed`) plus focused
        // regression assertions proving the guarded and plain fallback gates do
        // not write behind the plugin. No new flow guard; this tightens the
        // existing converge boundary from `disk_fallback` to `transport=blocked`
        // while a listener is active.
        // -1 (#fcc0-degraded-file-ipc): the degraded-socket convergence branch no
        // longer logs `reason=listener_degraded` on a disk fallback because that
        // fallback is gone. It now records `degraded_cause=...` on the file-IPC /
        // fail-closed path, preserving diagnostics without expanding flow reasons.
        // +1 (#fccroute): the route/dispatch session-write sites
        // (`route_session_id`, `route_dedup_scrub`, `route_queue_activation`) now
        // route through `converge_document_or_disk`; the added no-listener route
        // fallback regression test asserts `reason=no_listener` in the ops-log,
        // which is a test-assertion string, not a new hot-path flow reason.
        // +1 (#supselfheal Ph2): write_wedged_supervisor_recycle_requested ops-log
        // line carries `reason=repeated_ack_timeout_active_listener` — audited.
        // 19 -> 17 (#6b5h): the no_ack / no_ack_content / send_failed converge
        // refusals were centralized into `refuse_or_editorless_disk_fallback`,
        // which decides fail-closed (live editor) vs editor-less disk fallback.
        // The four per-branch `reason=` literals collapsed into the helper's three
        // (`transport=blocked reason={reason}`, the bail `(reason={reason})`, and
        // `transport=disk_fallback reason={reason}`), a net -2 audited reduction.
        // 17 -> 20 (#fcc0-ack-mismatch): ACK-mismatched editor convergence now
        // attempts a hash-guarded refresh only for the narrow stale queue-prompt
        // artifact shape. The three added production reasons log why that refresh
        // did not rewrite the editor buffer (`untrusted_ack_content_contains_user_drift`,
        // `no_ack`, `send_failed`) while the existing `ack_mismatch` writeback
        // reason remains the fail-closed flow boundary.
        // +1 (#docdriftgrace): `stale_snapshot_visible_rebased ... reason={}` logs
        // the audited safe-rebase reason after the reset guard refreshes snapshot
        // and CRDT sidecars for unrelated visible drift.
        ("agent-doc-orchestration/src/write/converge.rs", "reason=") => 21,
        // +1 for the audited `bare_write_escalated_to_commit ... reason=response_body_placed`
        // ops_log diagnostic on the #bare-write-captured-uncommitted escalation path.
        // +1 for the audited `queue_consume_divergence_reconciled ... reason=crdt_merge_authoritative`
        // ops_log diagnostic: when the post-CRDT-merge document queue diverges from
        // the snapshot, consume reconciles (document wins) instead of bailing
        // (#finalize-divergence-orphans-committed-head / IPC-CRDT resilience).
        // +2 for the audited `ipc_listener_degraded_direct_disk ... reason=repeated_ack_timeout`
        // diagnostics: repeated socket IPC ack timeouts de-wedge the current
        // document/session away from socket/file IPC and onto the direct-disk path.
        // +1 for the audited `ipc_socket_degraded_prefer_file_ipc ... reason=repeated_ack_timeout`
        // diagnostic (#ipc-degraded-prefers-file-ipc): a latched-degraded socket
        // routes the write through the file-IPC patch queue (plugin applies via
        // Document API) instead of a raw disk write; disk write is last resort.
        // +1 (#finalize-stale-baseline-reopen-friction): the
        // `explicit_baseline_replay_rejected ... reason={reason}` diagnostic now
        // names WHY the committed-cycle gate fired (`response_already_in_head`,
        // `empty_response`, `no_head_baseline`) so a true replay is distinguished
        // from the genuinely-new-response path that auto-reopens from HEAD instead
        // of bailing. Routed through the same explicit-baseline replay gate.
        // +2 (#queueeditloss) for the audited
        // `queue_content_ours_reconciled ... reason=live_queue_deletion_authoritative`
        // ops-log marker and its regression assertion. It proves content_ours
        // adoption removes live-deleted baseline queue prompts instead of
        // resurrecting them; live additions remain covered by the existing
        // dropped-queue evidence path.
        // +6 (#w42v) for the audited `compact_writeback ... transport=disk_fallback
        // reason=<no_listener|no_component_delta|no_ack_content|ack_mismatch|no_ack|
        // send_failed>` markers in `try_editor_converge` (the #fcc0-generalized
        // former `try_compact_editor_converge`). These are diagnostic
        // disk-fallback reasons on the editor-IPC convergence path (not flow
        // guards), each proving why a write site fell back to the guarded disk
        // write instead of converging through the editor.
        // +2 (#mps Rung 3) for the audited `mps_baseline_resolve source=md_fallback
        // reason=<no_model|model_error>` markers in `read_explicit_baseline`. These
        // are diagnostic baseline-fallback reasons on the model-projected-baseline
        // cutover path (not flow guards), logging why finalize fell back to the
        // legacy `.md` baseline instead of the projected model overlay.
        // +1 (#fintol2) for the audited `live_prompt_drift_forward_merged ...
        // reason=independent_concurrent_edit` ops-log marker in
        // `guard_ipc_snapshot_adoption_against_live_prompt_drift`. It proves the
        // finalize-tolerance forward-merge path: when a concurrent user edit is
        // disjoint from the response target (no prompt/directive, outside
        // `exchange`, conflict-free 3-way union), the gate commits the union this
        // cycle instead of carrying it forward via `content_ours_snapshot_next_cycle`.
        // A diagnostic on the tolerance path, not a new flow guard.
        // +1 (#fcc0) for the `reason=no_listener` assertion literal in the
        // `converge_document_or_disk_falls_back_to_guarded_disk_without_listener`
        // regression test. The 6 production disk-fallback reasons (now emitted by
        // the generalized `try_editor_converge` for every write site, not just
        // compact) are unchanged; this single increment is a test-assertion
        // literal proving the queue-consume disk fallback is source-labelled.
        // +4 (#fcc0e) for the de-wedge circuit-breaker integration of
        // `try_editor_converge`: +1 PRODUCTION `reason=listener_degraded` (the
        // converger now short-circuits to the guarded disk fallback when the
        // `#ipcdrift` latch is degraded, mirroring the reposition/finalize socket
        // paths) and +3 test literals (the comment + the `reason=listener_degraded`
        // / `reason=no_listener` assertions in
        // `try_editor_converge_skips_wedged_socket_when_latched_degraded`). The
        // socket failure path also now feeds `record_ipc_socket_ack_timeout` /
        // clears via `clear_ipc_socket_ack_timeouts` — no new `reason=` token.
        ("agent-doc-orchestration/src/write.rs", "reason=") => 12,
        _ => 0,
    }
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
fn test_admin_recycle_help_accepts_document_or_project_target() {
    let mut cmd = agent_doc_cmd();
    cmd.args(["admin", "recycle", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("[FILE_OR_PROJECT_ROOT]"))
        .stdout(predicate::str::contains(
            "Optional document path or project root to recycle",
        ));
}

#[test]
fn test_admin_recycle_accepts_document_target() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    let tasks = root.join("tasks");
    fs::create_dir_all(&tasks).unwrap();
    let doc = tasks.join("session.md");
    fs::write(&doc, "# Session\n").unwrap();
    let doc_arg = Path::new("tasks/session.md");
    let expected_root = root.canonicalize().unwrap().display().to_string();

    let mut cmd = agent_doc_cmd();
    cmd.current_dir(root);
    cmd.args(["admin", "recycle", doc_arg.to_str().unwrap(), "--json"]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let stdout = String::from_utf8_lossy(&output);
    assert!(!stdout.contains("unexpected argument"), "{stdout}");
    assert!(stdout.contains("\"scope\":\"project\""), "{stdout}");
    assert!(stdout.contains(&expected_root), "{stdout}");
    assert!(stdout.contains("\"recycled\":false"), "{stdout}");
}

#[test]
fn test_queue_sync_materializes_priority_go_backlog_and_session_check_stays_clean_after_commit() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let doc = root.join("session.md");
    fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/state/cycles")).unwrap();

    let content = concat!(
        "---\n",
        "agent_doc_session: test-session\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue: start\n",
        "prompt_presets:\n",
        "  '#spec-test-commit-push': update spec + tests. commit + push\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "## Queue\n\n",
        "<!-- agent:queue preset=\"#spec-test-commit-push\" priority go -->\n",
        "- advance [#mrhfeed-prop]\n",
        "- advance [#gvj5]\n",
        "<!-- /agent:queue -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog priority queue -->\n",
        "- [ ] [#2qrx] [P1] Offline click-upload backfill\n",
        "- [ ] [#rating-emails] [P2] Enable review opt-in\n",
        "- [ ] [#mrhfeed-prop] [P3] Existing advance head\n",
        "- [ ] [#cf-txn-email] [P3] Transactional email migration\n",
        "- [ ] [#884m] [P3] News sitemap cleanup\n",
        "- [ ] [#gvj5] [P3] Existing advance head\n",
        "- [ ] [#tk2p] [P3] Hetzner migration\n",
        "- [ ] [#pdp-video-footage] [P3] Product video footage\n",
        "<!-- /agent:backlog -->\n",
    );
    fs::write(&doc, content).unwrap();
    init_git_repo(root, &doc);
    seed_snapshot(root, &doc, content);

    let mut sync = agent_doc_cmd();
    sync.current_dir(root);
    sync.args(["queue", "sync", "session.md"]);
    let sync_output = sync.assert().success().get_output().stdout.clone();
    let sync_stdout = String::from_utf8(sync_output).unwrap();
    assert!(
        sync_stdout.contains("synced 8 backlog id(s)"),
        "sync should report all active backlog ids:\n{sync_stdout}"
    );
    assert!(
        sync_stdout.contains(
            "skipped already represented backlog id(s): #mrhfeed-prop, #gvj5 (reason: already_in_queue)"
        ),
        "sync should explain ids represented by existing non-do heads:\n{sync_stdout}"
    );
    assert!(
        sync_stdout.contains("materialized backlog id(s): #2qrx, #rating-emails, #cf-txn-email, #884m, #tk2p, #pdp-video-footage"),
        "sync should report newly materialized ids:\n{sync_stdout}"
    );

    let synced = fs::read_to_string(&doc).unwrap();
    assert!(synced.contains("- advance [#mrhfeed-prop]"));
    assert!(synced.contains("- advance [#gvj5]"));
    assert!(synced.contains("- do [#2qrx]"));
    assert!(synced.contains("- do [#rating-emails]"));
    assert!(synced.contains("- do [#cf-txn-email]"));
    assert!(synced.contains("- do [#884m]"));
    assert!(synced.contains("- do [#tk2p]"));
    assert!(synced.contains("- do [#pdp-video-footage]"));
    assert_eq!(
        synced.matches("do [#mrhfeed-prop]").count(),
        0,
        "existing advance head should prevent duplicate do head:\n{synced}"
    );
    assert_eq!(
        synced.matches("do [#gvj5]").count(),
        0,
        "existing advance head should prevent duplicate do head:\n{synced}"
    );

    ProcessCommand::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["commit", "-m", "sync queue", "--no-verify"])
        .status()
        .unwrap();

    let mut check = agent_doc_cmd();
    check.current_dir(root);
    check.args(["session-check", doc.to_str().unwrap()]);
    let check_output = check.assert().success().get_output().stdout.clone();
    let check_stdout = String::from_utf8(check_output).unwrap();
    assert!(
        check_stdout.contains("[session-check] ok"),
        "session-check should stay clean after the synced queue is committed:\n{check_stdout}"
    );
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
fn test_cli_audit_docs_treats_mtime_staleness_as_advisory() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"test\"\n").unwrap();
    std::fs::write(
        root.join("AGENTS.md"),
        "# Agent Instructions\n\nUse `cargo test` before changing code.\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

    let old_time = filetime::FileTime::from_unix_time(1_700_000_000, 0);
    let new_time = filetime::FileTime::from_unix_time(1_700_000_100, 0);
    filetime::set_file_mtime(root.join("AGENTS.md"), old_time).unwrap();
    filetime::set_file_mtime(root.join("src/main.rs"), new_time).unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.current_dir(root);
    cmd.arg("audit-docs");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Mtime advisory"))
        .stdout(predicate::str::contains("No blocking issues found"));
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
[103] route_dispatch_only_submit_unproven file=tasks/b.md pane=%2 harness=opencode delivery=direct_pane_submit submit_mode=tmux_literal_kitty_return proof=accepted proof_scope=accepted_only timeout_secs=10
[104] sync_latency phase=prune_stash_panes elapsed_ms=309 budget_ms=250 status=over_budget mode=full
[105] flow_event file=tasks/b.md flow=document_mutation stage=pre_write_guard outcome=blocked reason=visible_write_typing_defer_active_typing:socket_ipc
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
        .stdout(predicate::str::contains(
            "flow document mutation pre-write guard blocked",
        ))
        .stdout(predicate::str::contains("sync over budget"));
}

#[test]
fn test_cli_ops_diagnose_gathers_cycle_artifacts() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/captures/doc")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/codex-hooks/sessions")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/hooks/post_write")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/state/startup-miss")).unwrap();

    fs::write(
        root.join(".agent-doc/logs/ops.log"),
        "[100] flow_event file=tasks/a.md flow=closeout stage=commit outcome=blocked cycle_id=cycle-a patch_id=patch-a\n",
    )
    .unwrap();
    fs::write(
        root.join(".agent-doc/logs/cycles.jsonl"),
        "{\"op\":\"commit\",\"file\":\"tasks/a.md\",\"cycle_id\":\"cycle-a\"}\n",
    )
    .unwrap();
    fs::write(
        root.join(".agent-doc/logs/session-1.log"),
        "[101] codex_start cycle_id=cycle-a patch_id=patch-a\n",
    )
    .unwrap();
    fs::write(
        root.join(".agent-doc/captures/doc/cycle-a.json"),
        r#"{"cycle_id":"cycle-a","capture_id":"cycle-a","state":"captured","response_body":"secret body"}"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent-doc/codex-hooks/sessions/thread.json"),
        r#"{"thread_id":"thread-1","cycle_id":"cycle-a"}"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent-doc/hooks/post_write/patch-a.json"),
        r#"{"patch_id":"patch-a","cycle_id":"cycle-a","payload":"large plugin payload"}"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent-doc/patches/patch-a.md"),
        "patch_id=patch-a cycle_id=cycle-a\n",
    )
    .unwrap();
    fs::write(
        root.join(".agent-doc/session-actors.json"),
        r#"{"documents":[{"session_id":"session-1","cycle_id":"cycle-a"}]}"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent-doc/state/startup-miss/cycle-a.json"),
        r#"{"cycle_baseline_id":"cycle-a","session_id":"session-1"}"#,
    )
    .unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.args([
        "ops",
        "diagnose",
        "--project-root",
        root.to_str().unwrap(),
        "--cycle-id",
        "cycle-a",
        "--patch-id",
        "patch-a",
        "--session-id",
        "session-1",
        "--limit",
        "0",
        "--json",
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let stdout = String::from_utf8(output).unwrap();

    assert!(stdout.contains("\"ops log\""), "{stdout}");
    assert!(stdout.contains("\"captures\""), "{stdout}");
    assert!(stdout.contains("\"codex hook sessions\""), "{stdout}");
    assert!(stdout.contains("patch-a.md"), "{stdout}");
    assert!(
        stdout.contains("<13 bytes omitted from diagnosis summary>"),
        "large JSON fields should be summarized instead of dumped:\n{stdout}"
    );
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
fn test_manifest_uses_publishable_dependency_contract() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manifest = fs::read_to_string(manifest_path).unwrap();
    let parsed: toml::Value = toml::from_str(&manifest).unwrap();
    let package_version = parsed["package"]["version"].as_str();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    let agent_kit = dependencies["agent-kit"].as_table().unwrap();

    assert_eq!(
        agent_kit.get("path").and_then(toml::Value::as_str),
        None,
        "published manifests must not depend on a sibling-only agent-kit path"
    );
    assert_eq!(
        agent_kit.get("version").and_then(toml::Value::as_str),
        Some("0.4.1")
    );

    for crate_name in [
        "agent-doc-core",
        "agent-doc-markdown-ast",
        "agent-doc-orchestration",
    ] {
        let dependency = dependencies[crate_name].as_table().unwrap();
        assert!(
            dependency
                .get("path")
                .and_then(toml::Value::as_str)
                .is_some(),
            "{crate_name} should keep a local path for workspace builds"
        );
        assert_eq!(
            dependency.get("version").and_then(toml::Value::as_str),
            package_version,
            "{crate_name} must also carry a registry version for cargo publish"
        );
    }

    let tmux_router = dependencies["tmux-router"].as_table().unwrap();
    assert_eq!(
        tmux_router.get("path").and_then(toml::Value::as_str),
        Some("../tmux-router")
    );
    assert_eq!(
        tmux_router.get("version").and_then(toml::Value::as_str),
        Some("0.3.11")
    );
}

#[test]
fn test_codex_plugin_manifest_omits_invalid_claude_skill_path() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = manifest_dir.join(".codex-plugin/plugin.json");
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&manifest).unwrap();

    assert!(
        parsed.get("skills").is_none(),
        "Codex plugin manifests must not point skills at .claude/skills; use ./skills/ only when a Codex skills tree exists"
    );

    for field in ["composerIcon", "logo"] {
        let path = parsed["interface"][field].as_str().unwrap();
        assert!(
            path.starts_with("./assets/") && !path.contains(".."),
            "{field} must be a plugin-root-relative asset path, got {path}"
        );
        assert!(
            manifest_dir.join(path.trim_start_matches("./")).exists(),
            "{field} asset does not exist: {path}"
        );
    }
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
fn test_preflight_active_auto_queue_head_is_not_user_intent() {
    // #agent-doc-bug auto-queue stall: with an active `auto` queue and NO real
    // user/document diff this cycle, preflight synthesizes the queue head as the
    // cycle prompt. That synthetic continuation must NOT appear in
    // `user_intent_prompt_changes`, or the skill's auto-loop precondition never
    // holds and the queue stalls after each item. The head must still be exposed
    // via `queue_prompts` so the cycle has work to do.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let doc = root.join("session.md");
    let content = "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n\
        ## Exchange\n\n\
        <!-- agent:exchange patch=append -->\n\
        ### Re: prior — gpt-5\n\nDone.\n\
        <!-- agent:boundary:head-boundary -->\n\
        <!-- /agent:exchange -->\n\n\
        <!-- agent:queue auto -->\n\
        - do [#alpha]\n\
        - do [#beta]\n\
        <!-- /agent:queue -->\n";
    fs::write(&doc, content).unwrap();
    // Commit is the baseline; no separate seeded snapshot. There is no real diff
    // this cycle, so the active queue head is the only prompt source — the stall
    // repro shape.
    init_git_repo(root, &doc);

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
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("preflight stdout must be JSON");

    assert_eq!(
        parsed["queue_active"],
        serde_json::Value::Bool(true),
        "queue should be active: {parsed}"
    );
    let queue_prompts = parsed["queue_prompts"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !queue_prompts.is_empty(),
        "active queue must still expose its head as work: {parsed}"
    );
    // `user_intent_prompt_changes` is `skip_serializing_if` empty, so an empty
    // value is omitted (null) — exactly what we want here.
    let user_intent_empty = parsed["user_intent_prompt_changes"]
        .as_array()
        .map_or(true, |a| a.is_empty());
    assert!(
        user_intent_empty,
        "synthetic auto-queue head must NOT count as user intent (would stall the auto-loop): {}",
        parsed["user_intent_prompt_changes"]
    );
    // The head is still surfaced through prompt_bearing_changes for compatibility.
    assert!(
        !parsed["prompt_bearing_changes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .is_empty(),
        "queue head should remain in prompt_bearing_changes: {parsed}"
    );
}

/// `#cleardrainsim`: pin the FULL preflight JSON drainability contract — not just
/// the `queue_continuation::drainable_head_count` unit — that the SKILL.md
/// auto-loop (`queue_continuation_required`) and the supervisor idle-watch both
/// depend on. A go-mode active queue whose only materialized heads are
/// `[operator-verify]` + inert-noise must emit `queue_continuation_required:false`
/// + `queue_drainable_head_count:0` (the `#qchurn` no-op-loop guard) WITHOUT the
/// non-stall guidance, so the auto-loop stops instead of churning. `[clean-session]`
/// heads stay drainable in place (`#qcontdrain`).
fn preflight_json(root: &Path, doc: &Path) -> serde_json::Value {
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
    serde_json::from_slice(&output.stdout).expect("preflight stdout must be JSON")
}

#[test]
fn test_preflight_drainability_contract_zero_when_only_deferred_and_noise() {
    // Active go-mode queue whose ONLY heads are an `[operator-verify]` id-head and
    // an inert artifact noise line. Neither is agent-drainable, so the
    // authoritative no-loop signal must be `queue_continuation_required:false` +
    // `queue_drainable_head_count:0`, and the don't-stall guidance must be ABSENT.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let doc = root.join("session.md");
    let content = "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n\
        ## Exchange\n\n\
        <!-- agent:exchange patch=append -->\n\
        ### Re: prior — gpt-5\n\nDone.\n\
        <!-- agent:boundary:head-boundary -->\n\
        <!-- /agent:exchange -->\n\n\
        ## Backlog\n\n\
        <!-- agent:backlog -->\n\
        - [ ] [#ov1] [operator-verify] live drive needs a human editor\n\
        <!-- /agent:backlog -->\n\n\
        ## Queue\n\n\
        <!-- agent:queue go -->\n\
        - do [#ov1]\n\
        - [route] target tmux session: 0\n\
        <!-- /agent:queue -->\n";
    fs::write(&doc, content).unwrap();
    init_git_repo(root, &doc);
    seed_snapshot(root, &doc, content);

    let parsed = preflight_json(root, &doc);
    assert_eq!(
        parsed["queue_active"],
        serde_json::Value::Bool(true),
        "queue should still be active: {parsed}"
    );
    assert_eq!(
        parsed["queue_drainable_head_count"], 0,
        "operator-verify + noise heads are not agent-drainable: {parsed}"
    );
    assert_eq!(
        parsed["queue_continuation_required"],
        serde_json::Value::Bool(false),
        "no drainable head → auto-loop must stop, not churn (#qchurn): {parsed}"
    );
    assert!(
        parsed["queue_continuation_guidance"].is_null(),
        "non-stall guidance must be absent when continuation is not required: {parsed}"
    );
}

#[test]
fn test_preflight_drainability_contract_true_with_real_drainable_head() {
    // Same active go-mode queue but now with a `[clean-session]` id-head (drains in
    // place, #qcontdrain) and a free-text directive head ("fix ...") alongside the
    // deferred `[operator-verify]` head and the inert noise line. The drainable
    // count must be EXACTLY 2 (clean-session + directive), continuation required,
    // and the shared non-stall guidance present.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let doc = root.join("session.md");
    let content = "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n\
        ## Exchange\n\n\
        <!-- agent:exchange patch=append -->\n\
        ### Re: prior — gpt-5\n\nDone.\n\
        <!-- agent:boundary:head-boundary -->\n\
        <!-- /agent:exchange -->\n\n\
        ## Backlog\n\n\
        <!-- agent:backlog -->\n\
        - [ ] [#cs1] [clean-session] tighten the parser\n\
        - [ ] [#ov1] [operator-verify] live drive needs a human editor\n\
        <!-- /agent:backlog -->\n\n\
        ## Queue\n\n\
        <!-- agent:queue go -->\n\
        - do [#cs1]\n\
        - do [#ov1]\n\
        - fix the parser bug in the tokenizer\n\
        - [route] target tmux session: 0\n\
        <!-- /agent:queue -->\n";
    fs::write(&doc, content).unwrap();
    init_git_repo(root, &doc);
    seed_snapshot(root, &doc, content);

    let parsed = preflight_json(root, &doc);
    assert_eq!(
        parsed["queue_drainable_head_count"], 2,
        "clean-session id-head + free-text directive are drainable; operator-verify + noise are not: {parsed}"
    );
    assert_eq!(
        parsed["queue_continuation_required"],
        serde_json::Value::Bool(true),
        "a real drainable head remains → keep draining: {parsed}"
    );
    let guidance = parsed["queue_continuation_guidance"]
        .as_str()
        .expect("non-stall guidance must be present when continuation is required");
    assert!(
        guidance.contains("file-IPC") && guidance.contains("NOT stop reasons"),
        "guidance must carry the degraded-IPC no-stall contract: {guidance}"
    );
}

#[test]
fn test_preflight_exchange_slash_command_is_command_only() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let doc = root.join("session.md");
    let baseline = "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
        ## Exchange\n\n\
        <!-- agent:exchange patch=append -->\n\
        ### Re: prior — gpt-5\n\nDone.\n\
        <!-- /agent:exchange -->\n";
    fs::write(&doc, baseline).unwrap();
    init_git_repo(root, &doc);
    seed_snapshot(root, &doc, baseline);

    let current = baseline.replace(
        "<!-- /agent:exchange -->",
        "/clear\n<!-- /agent:exchange -->",
    );
    fs::write(&doc, current).unwrap();

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
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("preflight stdout must be JSON");

    assert_eq!(parsed["no_changes"], false);
    assert_eq!(parsed["builtin_commands"][0], "/clear");
    assert!(
        parsed
            .get("prompt_bearing_changes")
            .and_then(|value| value.as_array())
            .is_none_or(|changes| changes.is_empty()),
        "slash-only exchange diff must not be answered as a prompt target: {parsed}"
    );
    assert!(
        parsed
            .get("user_intent_prompt_changes")
            .and_then(|value| value.as_array())
            .is_none_or(|changes| changes.is_empty()),
        "slash-only exchange diff must not count as user intent prompt work: {parsed}"
    );
    let phase = read_cycle_phase(root, &doc);
    assert!(
        !matches!(
            phase.as_deref(),
            Some("preflight_started" | "response_captured" | "write_applied")
        ),
        "slash-only exchange command must not open a response cycle, got {phase:?}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("command-only"),
        "preflight should explain the command-only handoff:\n{stderr}"
    );
    assert!(
        !stderr.contains("preflight_diff_start"),
        "command-only handoff must not start a normal response cycle:\n{stderr}"
    );
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
fn test_preflight_warns_on_frontmatter_agent_harness_mismatch() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let doc = root.join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test-session\n",
        "agent_doc_format: template\n",
        "agent: codex\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior response — gpt-5\n",
        "Already committed.\n",
        "<!-- /agent:exchange -->\n",
    );
    fs::write(&doc, content).unwrap();
    init_git_repo(root, &doc);
    seed_snapshot(root, &doc, content);

    let mut preflight = agent_doc_cmd();
    preflight.current_dir(root);
    preflight.env("CLAUDE_CODE", "1");
    preflight.args(["preflight", doc.to_str().unwrap()]);
    let output = preflight.output().unwrap();
    assert!(
        output.status.success(),
        "preflight failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("preflight output should be JSON");
    assert_eq!(json["warnings"][0]["code"], "harness_mismatch");
    assert_eq!(json["warnings"][0]["document_agent"], "codex");
    assert_eq!(json["warnings"][0]["active_harness"], "claude-code");
    assert_eq!(json["no_changes"], true);
}

#[test]
fn test_preflight_warns_but_does_not_target_inactive_queue_edit() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let doc = root.join("session.md");
    let baseline = concat!(
        "---\n",
        "agent_doc_session: test-session\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: false\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior response — gpt-5\n",
        "Already committed.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#gdbpropscan] Inspect graph DB properties.\n",
        "<!-- /agent:backlog -->\n",
    );
    let current = baseline.replace(
        "<!-- agent:queue -->\n<!-- /agent:queue -->",
        "<!-- agent:queue -->\n- do [#gdbpropscan]\n<!-- /agent:queue -->",
    );
    fs::write(&doc, &baseline).unwrap();
    init_git_repo(root, &doc);
    seed_snapshot(root, &doc, baseline);
    fs::write(&doc, current).unwrap();

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

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("preflight output should be JSON");
    assert_eq!(json["warnings"][0]["code"], "inactive_queue_residue");
    assert!(
        json.get("prompt_bearing_changes")
            .and_then(|value| value.as_array())
            .is_none_or(|changes| changes.is_empty()),
        "inactive queue edit should not produce prompt-bearing changes: {json}"
    );
    assert_eq!(json["queue_active"], serde_json::Value::Null);
}

#[test]
fn test_cli_bare_file_path_aliases_to_run() {
    let tmp = tempfile::TempDir::new().unwrap();
    let missing = tmp.path().join("missing.md");

    let mut cmd = agent_doc_cmd();
    cmd.arg(&missing);
    cmd.env_remove("CLAUDECODE");
    cmd.env_remove("CLAUDE_CODE");
    cmd.env_remove("CLAUDE_CODE_SESSION");
    cmd.env_remove("OPENCODE");
    cmd.env_remove("OPENCODE_CLIENT");
    cmd.env_remove("CODEX");
    cmd.env_remove("CODEX_CLI");
    cmd.env_remove("CODEX_THREAD_ID");
    cmd.env("CODEX_SESSION", "codex-session");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("file not found"));

    let mut plain_shell = agent_doc_cmd();
    plain_shell.arg(&missing);
    plain_shell.env_remove("CLAUDECODE");
    plain_shell.env_remove("CLAUDE_CODE");
    plain_shell.env_remove("CLAUDE_CODE_SESSION");
    plain_shell.env_remove("OPENCODE");
    plain_shell.env_remove("OPENCODE_CLIENT");
    plain_shell.env_remove("CODEX");
    plain_shell.env_remove("CODEX_CLI");
    plain_shell.env_remove("CODEX_SESSION");
    plain_shell.env_remove("CODEX_THREAD_ID");
    plain_shell.assert().failure().stderr(
        predicate::str::contains("bare `agent-doc <FILE>` must be run")
            .and(predicate::str::contains(
                "supported harness (Codex, Claude Code, or OpenCode)",
            ))
            .and(predicate::str::contains("agent-doc run <FILE>")),
    );
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
    // Opt the bare file in via the `auto_session_for_all_md` escape hatch so the
    // session-generation behavior is still exercised under the opt-in gate (#4a6p).
    std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
    std::fs::write(
        tmp.path().join(".agent-doc/config.toml"),
        "[documents]\nauto_session_for_all_md = true\n",
    )
    .unwrap();
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
fn test_cli_route_rejects_plain_md_without_opt_in() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("notes.md");
    let original = "# Plain notes\n\nNot a session.\n";
    std::fs::write(&doc, original).unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.arg("route");
    cmd.arg(&doc);
    cmd.current_dir(tmp.path());
    cmd.env("AGENT_DOC_NO_AUTOSTART", "1");
    let output = cmd.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("is not an agent-doc document"),
        "route should fail closed on a plain .md, got: {}",
        stderr
    );
    // The gate must not mutate the file (no session injected).
    assert_eq!(std::fs::read_to_string(&doc).unwrap(), original);
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
    // Opt in via the escape hatch so the session-generation path runs under the
    // opt-in gate (#4a6p).
    std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
    std::fs::write(
        tmp.path().join(".agent-doc/config.toml"),
        "[documents]\nauto_session_for_all_md = true\n",
    )
    .unwrap();
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
fn test_cli_start_rejects_plain_md_without_opt_in() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("notes.md");
    let original = "# Plain notes\n";
    std::fs::write(&doc, original).unwrap();

    let mut cmd = agent_doc_cmd();
    cmd.arg("start");
    cmd.arg(&doc);
    cmd.env_remove("TMUX");
    cmd.env_remove("TMUX_PANE");
    // The opt-in gate fails closed before the tmux check.
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("is not an agent-doc document"));
    // The gate must not mutate the file.
    assert_eq!(std::fs::read_to_string(&doc).unwrap(), original);
}

#[test]
fn test_cli_start_generates_session_for_null_session() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("test.md");
    // `agent:` is an agent-doc marker, so this opts in even with a null session.
    std::fs::write(&doc, "---\nsession: null\nagent: claude\n---\n# Test\n").unwrap();

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
    assert_eq!(
        config["mcp_servers"]["agent-doc"]["command"].as_str(),
        Some("agent-doc")
    );
    assert_eq!(
        config["mcp_servers"]["agent-doc"]["default_tools_approval_mode"].as_str(),
        Some("approve")
    );
    let mcp_args: Vec<&str> = config["mcp_servers"]["agent-doc"]["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap())
        .collect();
    assert_eq!(
        mcp_args,
        vec![
            "mcp",
            "serve",
            "--project-root",
            std::fs::canonicalize(tmp.path()).unwrap().to_str().unwrap()
        ]
    );
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
