//! CLI integration tests for agent-doc.

use agent_doc_hash::content_hash;
use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
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
    let hash = content_hash(canonical.to_string_lossy().as_ref());
    let snapshot = root.join(".agent-doc/snapshots").join(format!("{hash}.md"));
    fs::write(snapshot, content).unwrap();
}

fn cycle_state_path(root: &Path, doc: &Path) -> PathBuf {
    let canonical = doc.canonicalize().unwrap();
    let hash = content_hash(canonical.to_string_lossy().as_ref());
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

fn assert_terminal_closeout_proof(root: &Path, doc: &Path) {
    let canonical_doc = doc.canonicalize().unwrap();
    let ledger_path = agent_doc_orchestration::flow::proof_ledger::proof_ledger_path(
        &root.canonicalize().unwrap(),
        &canonical_doc,
    );
    let records =
        agent_doc_orchestration::flow::proof_ledger::read_operation_proofs(&ledger_path).unwrap();
    assert!(
        records.iter().any(|record| {
            record.operation_kind
                == agent_doc_orchestration::flow::proof_ledger::ProofOperationKind::TerminalProof
                && record.proof_kind
                    == agent_doc_orchestration::flow::proof_ledger::ProofEvidenceKind::TerminalStateObserved
                && record.outcome
                    == agent_doc_orchestration::flow::proof_ledger::ProofOutcome::Recorded
                && record.proof.contains("phase=committed")
                && record.proof.contains("agreement=file_snapshot_head")
        }),
        "expected committed terminal closeout proof in {}",
        ledger_path.display()
    );
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
        agent_doc_sqlite::state_store::ActorState::Ready,
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
fn test_cli_admin_reap_all_stale_reports_summary_and_guards_tmux_unavailable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".agent-doc")).unwrap();
    let doc = root.join("tasks/admin-bulk-reap.md");
    fs::create_dir_all(doc.parent().unwrap()).unwrap();
    fs::write(
        &doc,
        "---\nagent_doc_session: session-cli-bulk-reap\nagent: codex\n---\nBody\n",
    )
    .unwrap();
    let record = agent_doc_orchestration::session_actor::record_session_start_direct(
        &doc,
        "session-cli-bulk-reap",
        "%999999",
        "@999999",
        1,
    )
    .unwrap();
    agent_doc_orchestration::session_actor::transition_state_direct(
        &doc,
        "session-cli-bulk-reap",
        "%999999",
        Some(1),
        agent_doc_sqlite::state_store::ActorState::Ready,
        "supervisor",
        "prompt_ready",
    )
    .unwrap();

    let output = agent_doc_cmd()
        .args([
            "admin",
            "reap",
            "--all-stale",
            "--project-root",
            root.to_str().unwrap(),
            "--reason",
            "cli bulk reap",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let summary: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(summary["reason"], "manual_reap_all_stale cli bulk reap");
    let reaped = summary["reaped"].as_u64().unwrap();
    assert!(
        reaped <= 1,
        "single stale fixture can reap at most one actor: {summary}"
    );

    let current =
        agent_doc_orchestration::project_controller::load_actor_record(root, &record.document_id)
            .unwrap()
            .unwrap();
    if reaped == 1 {
        assert_eq!(
            current.state,
            agent_doc_sqlite::state_store::ActorState::Closed
        );
        assert_eq!(current.pane_id, "");
        assert_eq!(current.window_id, "");
    } else {
        assert_eq!(
            current.state,
            agent_doc_sqlite::state_store::ActorState::Ready
        );
        assert_eq!(current.pane_id, "%999999");
    }
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
            .any(|tool| tool["name"] == "agent_doc_admit")
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
fn mcp_serve_handles_admit_and_plan() {
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

    let admit = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "agent_doc_admit",
            "arguments": {
                "file": doc.display().to_string()
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
        .write_stdin(format!("{admit}\n{plan}\n"))
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
        responses[0]["result"]["structuredContent"]["admission"]["source"],
        "admit"
    );
    assert_eq!(
        responses[0]["result"]["structuredContent"]["admission"]["maintenance_required"],
        false
    );
    assert_eq!(responses[1]["result"]["isError"], false);
    assert_eq!(
        responses[1]["result"]["structuredContent"]["plan"]["execution_scope"],
        "normal"
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
    assert_terminal_closeout_proof(root, &doc);
}

#[test]
fn test_codex_shared_closeout_spec_invariants() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let codex_support = fs::read_to_string(root.join("specs/codex-support.md")).unwrap();
    let agent_backend = fs::read_to_string(root.join("specs/05-agent-backend.md")).unwrap();
    let closeout = fs::read_to_string(root.join("specs/07-closeout-commands.md")).unwrap();
    let commit_runbook = fs::read_to_string(root.join("runbooks/commit.md")).unwrap();
    let harness_runbook = fs::read_to_string(root.join("runbooks/harness-invocation.md")).unwrap();

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
    assert!(
        commit_runbook.contains("editor_convergence_required")
            && commit_runbook.contains("operator_text_authority_v1")
            && commit_runbook
                .contains("Do not continue queue drain or final-answer delivery past this guard")
            && commit_runbook.contains("Do not run `--force-disk` from a harness"),
        "commit runbook must keep editor convergence failures from becoming harness-level success"
    );
    assert!(
        harness_runbook.contains("editor_convergence_required")
            && harness_runbook.contains("operator_text_authority_v1")
            && harness_runbook.contains("Do not report success, stop, continue an auto-queue")
            && harness_runbook.contains("Do not run `--force-disk`"),
        "harness invocation runbook must forbid bypassing operator-text authority guards"
    );
}

#[test]
fn realtime_workflow_spec_pins_lazily_backed_authority() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let realtime = fs::read_to_string(root.join("specs/14-realtime-workflow.md")).unwrap();
    let spec = fs::read_to_string(root.join("SPEC.md")).unwrap();

    assert!(
        realtime.contains("## Lazily-RS State Backbone")
            && realtime.contains("`agent-doc-document-realtime` must use `lazily-rs`")
            && realtime.contains("Other realtime loops")
            && realtime.contains("tmux, supervisor, editor-plugin, and controller")
            && realtime.contains("lazily::ThreadSafeStateMachine")
            && realtime.contains("lazily::ThreadSafeContext")
            && realtime.contains("lazily-spec")
            && realtime.contains("snapshot/delta graph"),
        "realtime workflow spec must require lazily-rs for realtime state"
    );
    assert!(
        realtime.contains("DiskDriftObserved")
            && realtime.contains("Pluginless editor or external process saves the file")
            && realtime.contains("out-of-band disk write"),
        "realtime workflow spec must model pluginless/out-of-band disk writes"
    );
    assert!(
        realtime.contains("## Disk Visibility And Durability")
            && realtime.contains("fresh\nread of the document path")
            && realtime.contains("not on proof that the\nstorage device has durably flushed")
            && realtime.contains("File watcher events are hints, not proof")
            && realtime.contains("Durability barriers (`fsync`, git object writes,\nbackup snapshots, and commit recovery sidecars) belong to backup"),
        "realtime workflow spec must distinguish hot-path read visibility from durable disk persistence"
    );
    assert!(
        realtime.contains("storing realtime authority only in a turn-local cycle sidecar")
            && realtime.contains("instead of a lazily-backed projection"),
        "realtime workflow spec must forbid turn-local realtime authority"
    );
    assert!(
        realtime.contains("operator_text_authority_v1")
            && realtime.contains("capability-unknown frontend")
            && realtime.contains("safe delivery proof")
            && realtime.contains("even when the reported buffer currently equals\ndisk:")
            && realtime.contains("normalization repair")
            && realtime.contains("file-IPC fallback")
            && realtime.contains("expected editor text")
            && realtime.contains("Editor API success alone is not proof"),
        "realtime workflow spec must require frontend capability proof before trusting editor mutation delivery"
    );
    assert!(
        realtime.contains("target the live plugin-owner `editor_id`")
            && realtime.contains("Untargeted file-IPC\n   fallback is not delivery proof")
            && spec.contains("Editor delivery must target the live plugin-owner `editor_id`")
            && spec.contains("untargeted file-IPC fallback is not delivery proof"),
        "realtime workflow spec must require targeted editor delivery for editor-owned documents"
    );
    assert!(
        realtime.contains("## Editor Frontend Hot Path")
            && realtime.contains("The editor text-change callback is a capture boundary, not a convergence worker")
            && realtime.contains("must not perform full-buffer reads")
            && realtime.contains("CRDT merge, code-point offset conversion, socket I/O, native sidecar writes, patch application, or document saves")
            && realtime.contains("queued onto cancellable background work")
            && realtime.contains("disposed when the document closes or the plugin unloads"),
        "realtime workflow spec must keep editor UI/extension-host text-change callbacks fast"
    );
    assert!(
        spec.contains("document realtime state\n  machine")
            && spec.contains("`agent-doc-document-realtime`")
            && spec.contains("lazily-rs-backed state"),
        "top-level spec must surface the lazily-backed realtime authority invariant"
    );
}

#[test]
fn realtime_workflow_spec_keeps_merge_and_commit_lifecycles_distinct() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let realtime = fs::read_to_string(root.join("specs/14-realtime-workflow.md")).unwrap();

    assert!(
        realtime.contains("CRDT merge does not commit")
            && realtime.contains("document turn lifecycle owns commits")
            && realtime.contains("merge/realtime paths must not run git commit"),
        "realtime workflow spec must keep pure merge/realtime application separate from turn closeout commits"
    );
}

#[test]
fn realtime_workflow_spec_pins_stale_tool_host_fail_closed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let realtime = fs::read_to_string(root.join("specs/14-realtime-workflow.md")).unwrap();

    assert!(
        realtime.contains("stale MCP server")
            && realtime.contains("long-lived tool host")
            && realtime.contains("must refuse mutating tools")
            && realtime.contains("agent_doc_admit")
            && realtime.contains("agent_doc_preflight")
            && realtime.contains("agent_doc_finalize"),
        "realtime workflow spec must make stale MCP/tool-host mutation fail closed"
    );
}

#[test]
fn realtime_workflow_spec_models_parse_state_and_editor_feedback() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let realtime = fs::read_to_string(root.join("specs/14-realtime-workflow.md")).unwrap();
    let lifecycle = fs::read_to_string(root.join("specs/15-turn-lifecycle.md")).unwrap();
    let spec = fs::read_to_string(root.join("SPEC.md")).unwrap();

    assert!(
        realtime.contains("## Realtime Parse State")
            && realtime.contains("ParseValid")
            && realtime.contains("ParseRecoverable")
            && realtime.contains("ParseBlocked"),
        "realtime workflow spec must define a separate parse-state machine"
    );
    assert!(
        realtime.contains("editor plugins must surface")
            && realtime.contains("inline diagnostics")
            && realtime.contains("quick-fix proposals"),
        "parse issues must be visible as realtime editor feedback"
    );
    assert!(
        realtime.contains("preflight repair must not be the normal parse recovery path")
            && lifecycle.contains("ParseBlocked")
            && lifecycle.contains("InterruptedBlocked"),
        "preflight/turn lifecycle must consume parse state instead of repairing as the hot path"
    );
    assert!(
        spec.contains("parse state projection")
            && spec.contains("editor diagnostics")
            && spec.contains("preflight repair"),
        "top-level spec must surface realtime parse-state authority"
    );
}

#[test]
fn turn_lifecycle_spec_cross_links_realtime_state_machine() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let realtime = fs::read_to_string(root.join("specs/14-realtime-workflow.md")).unwrap();
    let lifecycle = fs::read_to_string(root.join("specs/15-turn-lifecycle.md")).unwrap();
    let spec = fs::read_to_string(root.join("SPEC.md")).unwrap();

    assert!(
        spec.contains("[Turn Lifecycle Authority](specs/15-turn-lifecycle.md)")
            && realtime.contains("[Turn Lifecycle Authority](15-turn-lifecycle.md)")
            && lifecycle.contains("[Real-Time Workflow Authority](14-realtime-workflow.md)"),
        "realtime and turn lifecycle specs must cross-link their distinct state machines"
    );
    assert!(
        lifecycle.contains("## Turn States")
            && lifecycle.contains("## Turn State Transitions")
            && lifecycle.contains("flowchart LR")
            && lifecycle.contains("CommitPending")
            && lifecycle.contains("NoCommitComplete")
            && lifecycle.contains("InterruptedBlocked")
            && lifecycle.contains("document turn lifecycle owns commits"),
        "turn lifecycle spec must define states, transitions, diagrams, and commit ownership"
    );
    assert!(
        lifecycle.contains("agent-doc-merge does not commit")
            && lifecycle.contains("realtime handoff proof")
            && lifecycle.contains("commit is optional when the selected turn policy is no-commit"),
        "turn lifecycle spec must consume realtime proofs without making merge/realtime commit"
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
        "agent-doc-document-realtime/src/write_policy.rs",
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
        // +1 (#live-editor-commit-guard): `commit_with_outcome` now routes a
        // live editor buffer ahead of disk through `log_closeout_guard_event`
        // with `CloseoutGuardReason::ReplicaDeliveryPending`, blocking stale
        // already-current/pre-stage commits until editor delivery is proven.
        // +1 (#preserved-queue-replay-neutralized): the committed git path now
        // runs the same stale-snapshot reset drift guard before classifying a
        // preserved queue addition neutralized by replay normalization. This is
        // the existing reset-drift guard call, not a new guard boundary.
        // 21 -> 15 (#transient-marker-document-policy): strip_guard_markers and
        // strip_head_markers policy/tests moved from git into the focused
        // `agent-doc-document::transient_markers` module. The removed tokens are
        // document-normalization names, not removed flow guard boundaries.
        ("agent-doc-orchestration/src/git.rs", "guard_") => 15,
        // #safe-mutation-extract: safe out-of-band mutation classification moved
        // into the focused realtime write policy crate. The four `guard_` tokens
        // are the existing transient marker stripper name/call plus two policy
        // unit-test names, not new orchestration guard boundaries.
        // 4 -> 2 (#transient-marker-document-policy): realtime write policy now
        // imports transient marker normalization from agent-doc-document instead
        // of carrying duplicate guard-marker stripping helpers.
        ("agent-doc-document-realtime/src/write_policy.rs", "guard_") => 2,
        // 1 -> 0 (#transient-marker-document-policy): git/normalize no longer
        // owns transient guard marker normalization.
        ("agent-doc-orchestration/src/git/normalize.rs", "guard_") => 0,
        // `#pcwcrt`: the legacy post-commit working-tree revert tower
        // (`postcommit_worktree_lost_committed_content` / `send_postcommit_editor_refresh`
        // and their transport-tagged reconcile logs: `reason=committed_content_lost`,
        // `reason=no_listener`/`reason=no_ack`, `reason=clear_carry_forward_drift`,
        // `reason=stale_editor_exchange`, `reason=stale_editor_queue_resurrection`)
        // was removed in favor of observe-only post-commit drift handling — the binary
        // no longer silently reverts an operator working-tree edit back to HEAD. That
        // drops the historical `reason=` annotations from 20 to the 7 that survive on
        // the non-reverting paths: 3 generic `reason=` log formats, `reason=already_current`,
        // and the 3 `#editorbufwin` P2 `reason=preserved_queue_addition_replay_neutralized`
        // markers/assertions (replay-neutralized queue additions are committed only after
        // closeout recovery evidence, not during ordinary independent queue edits).
        // 7 -> 8 (#preserved-queue-replay-neutralized): a second regression
        // assertion covers the same replay-neutralized queue-addition recovery
        // after stale exchange-collapse cleanup. The production reason remains
        // the existing typed recovery diagnostic.
        // 8 -> 10 (#commit-live-buffer-staged-snapshot): the commit barrier now
        // logs the audited allowance when the staged snapshot already matches a
        // synced operator-authoritative live buffer. This is a proof diagnostic
        // for the existing pre-stage guard path, not a new ad hoc flow branch.
        ("agent-doc-orchestration/src/git.rs", "reason=") => 10,
        ("src/orchestrate.rs", "guard_") => 0,
        ("src/orchestrate/dag.rs", "guard_") => 2,
        // +1 (`reason=probe_inspection_only`): `preflight --probe` logs why it
        // skipped opening a `preflight_started` cycle (#preflight-probe-side-effect-free).
        // +1 (`reason=struck_items_below_close_marker`): queue-escape repair logs
        // when it removed struck queue items displaced below the closing marker
        // (#queue-completed-items-escape-below-component).
        // +1 (`reason=editor_buffer_lost_committed_exchange`): #ipctruncrecover —
        // the layout-guard editor-buffer-as-truth recovery logs when it REFUSED to
        // trust a flushed editor buffer that itself dropped the committed `exchange`
        // response, falling through to the safe bail instead of auto-committing a
        // response-less document.
        ("agent-doc-orchestration/src/preflight.rs", "reason=") => 3,
        // 1 -> 0: queue selected-head handling moved to typed queue projection
        // fields, retiring the last ad hoc `reason=` token from preflight/run.
        ("agent-doc-orchestration/src/preflight/run.rs", "reason=") => 0,
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
        // 5 -> 6 (`reason={}`): typed deferred queue-head projection records the
        // owning defer reason for stop/time-gated heads.
        ("agent-doc-orchestration/src/preflight/maintenance.rs", "reason=") => 6,
        // +1 (#pm-live-buffer-guard): pending maintenance now reuses the shared
        // visible-write idle/current guard before it can send queue/backlog/status
        // convergence through editor IPC. This prevents a maintenance reap from
        // touching an unsaved operator-visible buffer ahead of disk.
        ("agent-doc-orchestration/src/preflight/maintenance.rs", "guard_") => 1,
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
        // 21 -> 7: authoritative actor runtime dispatch guard policy moved to
        // `agent-doc-controller::dispatch`, and the route-local guard tests were
        // deleted instead of preserving a facade. The surviving route tokens are
        // adapter/logging references around the focused guard decisions.
        ("agent-doc-orchestration/src/route.rs", "guard_") => 7,
        // 3 -> 0: route submit-observation proof rendering moved to
        // `agent-doc-controller::dispatch`; route now adapts proof facts into
        // the focused controller formatter instead of owning `proof=` log text.
        ("agent-doc-orchestration/src/route.rs", "proof=") => 0,
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
        // +1 (`reason=force_disk`): route's explicit no-listener/headless
        // escape hatch logs when `--force-disk` bypasses editor convergence for
        // route-owned session/queue writes.
        ("agent-doc-orchestration/src/route.rs", "reason=") => 10,
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
        // prompts that mention `/agent-doc`/`/clear`. The skip itself now lives
        // in `agent_doc_queue::queue_command` and carries no `guard_` substring.
        // +1 (#partial-staging-guard-cross-doc-noise): the
        // `partial_staging_closeout_guard_ignores_cross_document_markdown_noise`
        // regression test-fn name (substring `guard_`). The fix itself drops `md`
        // from `is_partial_staging_relevant_path` and adds no `guard_` token.
        // +2 (#eqrecovery): the
        // `committed_without_response_body_guard_skips_noop_queue_recovery`
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
        // +2 (#closeoutstall): typed editor-convergence closeout interruption
        // reports the blocked state as `reason=<no_ack|...>` in the canonical
        // session-check diagnostic, plus the focused regression assertion for
        // `reason=no_ack`. This routes the former ad hoc stall through the cycle
        // state's `blocked_closeout` surface instead of a generic repair branch.
        ("agent-doc-orchestration/src/session_check.rs", "reason=") => 3,
        ("agent-doc-orchestration/src/session_check/closeout_guards.rs", "guard_") => 4,
        // +3 (#samplequeuepreserve): the audited
        // `queue_head_removal_guard_proof` diagnostic plus two regression test
        // names proving removed id-backed/free-text queue heads log their proof
        // source instead of disappearing silently.
        // +3 (#qheadresidue): the audited
        // `free_text_queue_completed_residue_guard_fired` diagnostic plus two
        // regression test names proving answered free-text heads cannot remain
        // active queue residue.
        ("agent-doc-orchestration/src/session_check/queue_head_provenance_guards.rs", "guard_") => {
            // 12 baseline + 2 (#qimpstrike) for the new `residue_guard_exempts_recurring_imperative_deploy_head`
            // regression test name and its `free_text_queue_completed_residue_guard_fired`
            // negative-assertion substring. Both are test-only `guard_` substrings, not
            // new production flow guards: the residue guard reuses the existing
            // `free_text_queue_completed_residue_guard_fired` event, only adding an
            // early `is_recurring_imperative_head` exemption before it can fire.
            14
        }
        // 8 -> 12: guard-mode precedence/defaulting moved to
        // `agent-doc-frontmatter`, so this file now names the focused
        // `project_config::resolve_*_guard_mode` functions at five adapter
        // call sites. `resolve_review_done_guard_mode_with_context` was deleted
        // because there was no cached-context call site to preserve, for a net
        // +4 audited tokens.
        ("agent-doc-orchestration/src/session_check/pending_guards.rs", "guard_") => 12,
        ("agent-doc-orchestration/src/session_check/queue_head_guards.rs", "guard_") => 2,
        ("agent-doc-orchestration/src/session_check/partial_staging.rs", "guard_") => 2,
        ("agent-doc-orchestration/src/session_check/response_guards.rs", "guard_") => 8,
        ("agent-doc-orchestration/src/session_check/detect.rs", "guard_") => 1,
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
        // +2 (#8j86) for the audited `agent_doc_document::transient_markers::strip_guard_markers(&probe)` call
        // in `response_materialization_probe_from_response` plus its doc-comment
        // cross-reference: the materialization probe strips the same ephemeral
        // guard markers `git::commit` strips, so a captured response body carrying
        // `<!-- no-pending-done-guard -->` still matches the committed HEAD/archive
        // blob and `stuck_captured_cycle` stops false-alarming. Reuses the existing
        // `agent_doc_document::transient_markers::strip_guard_markers` helper — no new flow guard token.
        // +1 (#sampleipcdrift) for the audited visible-write idle/current guard on
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
        // snapshot reset-drift guard before granular backlog/review/status
        // mutations, so a failed finalize cannot alter backlog state without the
        // exchange response. Reuses the existing reset-drift boundary.
        // +1 (#missing-head-response-recovery): strict empty-response closeout
        // uses the existing visible-write idle/current guard before merging a
        // committed HEAD response back into a stale visible document. This is an
        // audited reuse of the document-write guard, not a new authority source.
        ("agent-doc-orchestration/src/write.rs", "guard_") => 47,
        ("agent-doc-orchestration/src/write/pending_checks.rs", "guard_") => 4,
        // 3 -> 2 (#template-materialization-policy): raw-response probe
        // construction now lives in `agent-doc-template::response_materialization`,
        // so the audited transient guard-marker stripping token moved out of
        // the write adapter with that focused policy.
        ("agent-doc-orchestration/src/write/materialize.rs", "guard_") => 2,
        ("agent-doc-orchestration/src/write/exchange_reconcile.rs", "guard_") => 5,
        // -2 `guard_`, -1 `reason=` (#nodiskipc): active IPC timeout/no-proof
        // paths no longer enter the direct document-write fallback, so the removed
        // visible-write guard/reason tokens are retired rather than rerouted.
        // +1 `reason=`: non-git repair template replay now uses an explicit
        // `apply_template_writeback ... reason=force_disk` marker when the
        // existing repair replay policy elects the audited force-disk transport.
        ("agent-doc-orchestration/src/write/run_entry.rs", "guard_") => 10,
        ("agent-doc-orchestration/src/write/run_entry.rs", "reason=") => 2,
        // queue-prompt consumption, IPC transport/repair, and live-prompt-drift
        // convergence extracted into write/queue_consume.rs, write/ipc.rs, and
        // write/converge.rs (#splitmods3 large-module split). The moved
        // `guard_`/`reason=` tokens are tracked against the new submodules,
        // not added anew.
        ("agent-doc-orchestration/src/write/queue_consume.rs", "guard_") => 0,
        // +3 (#freshqueueauth): direct queue-head removals now log explicit
        // proof fields for prune/orphan/acknowledgement paths, and the new
        // acknowledgement regression asserts that proof marker. The operations
        // stay routed through the existing queue-consume/converge write boundary.
        ("agent-doc-orchestration/src/write/queue_consume.rs", "proof=") => 3,
        // 1 -> 4 (#editorbufwin Fix A): the queue-consume head-equality check now
        // reconciles a benign live-buffer head divergence instead of hard-bailing,
        // mirroring the existing remaining-queue `reason=crdt_merge_authoritative`
        // reconcile. The new `reason=live_buffer_addition_authoritative` token
        // appears in the production ops_log line, its explanatory comment, and the
        // regression test assertion — gated on recorded dropped-queue evidence
        // (no evidence still bails, preserving the corruption guard).
        // 4 -> 5 (#typed-stop-fence): post-consume next-head projection records
        // typed stop-fence deferral as `QueueHeadDeferred` with the owning reason.
        ("agent-doc-orchestration/src/write/queue_consume.rs", "reason=") => 5,
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
        // 17 -> 21 (#live-drift-visible-repair): +4 test-only guard substrings
        // from `guard_live_prompt_drift_requires_visible_repair`,
        // `guard_live_prompt_drift_accepts_ack_visible_union`, and their direct
        // calls to the existing live-prompt-drift IPC adoption guard. These prove
        // content_ours adoption after a live editor ACK requires visible response
        // proof unless the ACK already contains the response union.
        ("agent-doc-orchestration/src/write/ipc.rs", "guard_") => 21,
        // 17 -> 18 (#smconv): +1 production `reason=node_keyed_semantic_merge` on
        // the new `live_prompt_drift_semantic_merged` ops_log — the node-keyed
        // merge success path, mirroring the sibling `#fintol2`
        // `reason=independent_concurrent_edit` forward-merge log (an ops_log
        // human-reason, not a new flow-enum outcome).
        // 18 -> 19 (#detached-disk-current-file): the degraded-socket editorless
        // regression now asserts `reason=listener_degraded_editor_detached`,
        // proving the path tries file IPC first and only then uses guarded
        // DetachedDisk when no live editor sidecar owns the document.
        // 19 -> 20 (#ack-content-live-buffer-sync): already-applied ack-content
        // adoption logs `reason=no_editor_id` when the binary has no editor id
        // to mark the live-buffer epoch as synced. The snapshot still follows
        // the existing ack-content proof path; this is diagnostic context for
        // skipping the live-buffer barrier update.
        // 20 -> 21 (#ack-content-disk-write-through): proven ACK-content disk
        // write-through logs `reason=already_current` when disk already matches
        // the editor-proven content. This is diagnostic context for skipping the
        // write-through effect, not a new flow branch.
        ("agent-doc-orchestration/src/write/ipc.rs", "reason=") => 21,
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
        // +2 (#provauth3): the post-`/clear` binary-origin compaction rebase test
        // and its no-provenance fail-closed safety-rail test call the same existing
        // `guard_no_stale_snapshot_reset_drift` boundary. Still test-only coverage;
        // the production guard boundary is unchanged.
        // -2 (#realtime-authority): removed stale doc-comment references to
        // snapshot-adoption guard fallbacks. The production guard boundary count
        // is unchanged; the comment now describes current-document merge instead.
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
        // 21 -> 20 (#mergestatemachine2): the `ack_mismatch` converge branch stopped
        // emitting its own `bail!("... (reason=ack_mismatch)")` and now routes through
        // `refuse_or_editorless_disk_fallback(file, source, "ack_mismatch")`, so the
        // reason flows through the helper's existing `reason={reason}` token. Net -1:
        // a CLI-only/no-editor session now disk-falls-back on ack_mismatch instead of
        // hard-refusing (the #6b5h wedge). Audited — no new flow boundary.
        // 20 -> 23 (#operator-text-authority): a diverged live-buffer sidecar from
        // a capability-unknown editor now fails closed before IPC send with
        // `reason=editor_capability_missing`; the other two new occurrences are
        // regression assertions proving the guard fired and a capable sidecar did
        // not. Routed through the existing converge fail-closed boundary.
        // 23 -> 24 (#operator-text-authority-clean): a matching live-buffer sidecar
        // from a capability-unknown editor also fails closed before IPC send. This
        // covers the delivery-vs-next-keystroke race while using the same
        // `reason=editor_capability_missing` converge boundary.
        // 9 -> 11 (#operator-text-authority-refresh): two regression test names
        // contain `capability_guard_`; the production guard boundary is unchanged.
        // +1 (#detached-disk-current-file): the detached-disk convergence path
        // calls `guard_visible_write_idle_and_current` immediately before the
        // atomic write, so no editorless fallback writes over a newer file epoch.
        // +1 (#active-capture-response-preserve): the stale snapshot reset drift
        // guard now has a regression path proving it skips visible rebase when
        // the active captured response is present in the snapshot but missing
        // from visible current content.
        ("agent-doc-orchestration/src/write/converge.rs", "guard_") => 13,
        // 24 -> 26 (#operator-text-authority-refresh): a missing-authority sidecar
        // now asks the editor to republish a read-only live-buffer proof before
        // failing closed. The two `reason=publish_live_buffer_failed` diagnostics
        // explain the blocked convergence closeout when that refresh cannot prove
        // authority.
        // 26 -> 27 (#detached-disk-current-file): the audited `DetachedDisk`
        // path logs `transport=disk_detached reason=<...>` after proving no live
        // editor owner/sidecar and rechecking the current visible file.
        ("agent-doc-orchestration/src/write/converge.rs", "reason=") => 27,
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
        // cycle instead of carrying it forward through a snapshot-only retry.
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
        // / `reason=no_listener` assertions in the degraded-socket convergence
        // tests). The
        // socket failure path also now feeds `record_ipc_socket_ack_timeout` /
        // clears via `clear_ipc_socket_ack_timeouts` — no new `reason=` token.
        // 12 -> 11 (#live-drift-visible-repair): the focused file-IPC regression
        // now proves the earlier partial-materialization retry path instead of
        // asserting an extra visible-repair `reason=` literal. The production
        // recovery still logs `recovery=visible_repair_required` in write/ipc.rs.
        ("agent-doc-orchestration/src/write.rs", "reason=") => 11,
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
    // `#recycle-no-boundaries`: escalation to a cold-start fires only for an actual
    // session document (one with an `agent_doc_session` id). This fixture has no
    // frontmatter, so recycle must degrade to a clean no-op exit (not a hard
    // `restart` error) with `escalated_cold_start:false`.
    assert!(
        stdout.contains("\"escalated_cold_start\":false"),
        "{stdout}"
    );
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
        "- advance [#samplefeed-prop]\n",
        "- advance [#gvj5]\n",
        "<!-- /agent:queue -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog priority queue -->\n",
        "- [ ] [#2qrx] [P1] Offline click-upload backfill\n",
        "- [ ] [#rating-emails] [P2] Enable review opt-in\n",
        "- [ ] [#samplefeed-prop] [P3] Existing advance head\n",
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
            "skipped already represented backlog id(s): #samplefeed-prop, #gvj5 (reason: already_in_queue)"
        ),
        "sync should explain ids represented by existing non-do heads:\n{sync_stdout}"
    );
    assert!(
        sync_stdout.contains("materialized backlog id(s): #2qrx, #rating-emails, #cf-txn-email, #884m, #tk2p, #pdp-video-footage"),
        "sync should report newly materialized ids:\n{sync_stdout}"
    );

    let synced = fs::read_to_string(&doc).unwrap();
    assert!(synced.contains("- advance [#samplefeed-prop]"));
    assert!(synced.contains("- advance [#gvj5]"));
    assert!(synced.contains("- do [#2qrx]"));
    assert!(synced.contains("- do [#rating-emails]"));
    assert!(synced.contains("- do [#cf-txn-email]"));
    assert!(synced.contains("- do [#884m]"));
    assert!(synced.contains("- do [#tk2p]"));
    assert!(synced.contains("- do [#pdp-video-footage]"));
    assert_eq!(
        synced.matches("do [#samplefeed-prop]").count(),
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
        "agent-doc-config",
        "agent-doc-debounce",
        "agent-doc-diff",
        "agent-doc-document-realtime",
        "agent-doc-markdown-ast",
        "agent-doc-ffi",
        "agent-doc-frontmatter",
        "agent-doc-fs",
        "agent-doc-merge",
        "agent-doc-model-tier",
        "agent-doc-orchestration",
        "agent-doc-project-config-io",
        "agent-doc-prompt-contract",
        "agent-doc-queue",
        "agent-doc-response-toc-io",
        "agent-doc-template",
        "agent-doc-turn",
        "agent-doc-workflow",
        "agent-doc-work-graph",
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

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let members = parsed["workspace"]["members"].as_array().unwrap();
    assert!(
        !members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-core")),
        "agent-doc-core must not be retained as an empty facade workspace crate"
    );
    assert!(
        !manifest_dir.join("agent-doc-core/Cargo.toml").exists(),
        "agent-doc-core manifest should be deleted once focused crates own the API"
    );

    for manifest in
        std::iter::once(manifest_dir.join("Cargo.toml")).chain(members.iter().map(|member| {
            manifest_dir
                .join(member.as_str().unwrap())
                .join("Cargo.toml")
        }))
    {
        let manifest_content = fs::read_to_string(&manifest).unwrap();
        let parsed_manifest: toml::Value = toml::from_str(&manifest_content).unwrap();
        let dependencies = parsed_manifest
            .get("dependencies")
            .and_then(toml::Value::as_table);
        assert!(
            dependencies.is_none_or(|dependencies| !dependencies.contains_key("agent-doc-core")),
            "{} must not depend on deleted agent-doc-core",
            manifest.display()
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
fn test_agent_doc_model_tier_owns_context_usage_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let package_version = workspace["package"]["version"].as_str();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-model-tier")),
        "agent-doc-model-tier must stay a first-class workspace crate"
    );

    let model_tier_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-model-tier/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&model_tier_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    assert!(
        dependencies.contains_key("serde_json"),
        "context transcript token parsing belongs in agent-doc-model-tier and needs serde_json"
    );
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-model-tier context usage policy must stay free of core, orchestration, git, editor IPC, sqlite, or tmux crates"
        );
    }

    let orchestration_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/Cargo.toml")).unwrap();
    let orchestration: toml::Value = toml::from_str(&orchestration_manifest).unwrap();
    let orchestration_dependencies = orchestration["dependencies"].as_table().unwrap();
    let dependency = orchestration_dependencies["agent-doc-model-tier"]
        .as_table()
        .unwrap();
    assert_eq!(
        dependency.get("path").and_then(toml::Value::as_str),
        Some("../agent-doc-model-tier")
    );
    assert_eq!(
        dependency.get("version").and_then(toml::Value::as_str),
        package_version
    );

    let model_tier_source =
        fs::read_to_string(manifest_dir.join("agent-doc-model-tier/src/lib.rs")).unwrap();
    for required_snippet in [
        "pub fn canonical_harness_name(",
        "pub const HARNESS_MISMATCH_WARNING_CODE",
        "pub struct HarnessMismatchWarning",
        "pub fn harness_mismatch_warning(",
        "pub fn short_model_name(",
        "pub fn resolve_agent_model(",
    ] {
        assert!(
            model_tier_source.contains(required_snippet),
            "agent-doc-model-tier should own preflight model attribution and harness policy directly: {required_snippet}"
        );
    }

    let context_usage =
        fs::read_to_string(manifest_dir.join("agent-doc-model-tier/src/context_usage.rs")).unwrap();
    for required_snippet in [
        "pub enum Harness",
        "pub struct UsedTokens",
        "pub const CLAUDE_CONTEXT_WINDOW",
        "pub fn claude_project_hash(",
        "pub fn claude_transcript_path(",
        "pub fn claude_projects_subdir(",
        "pub fn parse_claude_jsonl_used_tokens(",
        "pub fn context_window_for_model(",
        "pub fn context_pct(",
        "pub fn parse_codex_jsonl_context_pct(",
        "pub struct ClearDecision",
        "pub fn clear_decision(",
    ] {
        assert!(
            context_usage.contains(required_snippet),
            "agent-doc-model-tier should own context usage policy directly: {required_snippet}"
        );
    }

    let orchestration_context =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/context_pct.rs"))
            .unwrap();
    let preflight_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/preflight.rs")).unwrap();
    let preflight_run_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/preflight/run.rs"))
            .unwrap();
    for forbidden_snippet in [
        "fn canonical_harness_name(",
        "fn harness_mismatch_warning(",
        "fn short_model_name(",
        "fn resolve_agent_model(",
    ] {
        assert!(
            !preflight_source.contains(forbidden_snippet),
            "preflight.rs must not re-own model-tier attribution or harness policy: {forbidden_snippet}"
        );
    }
    assert!(
        preflight_run_source.contains("agent_doc_model_tier::harness_mismatch_warning(")
            && preflight_run_source.contains("agent_doc_model_tier::canonical_harness_name(")
            && preflight_run_source.contains("agent_doc_model_tier::resolve_agent_model("),
        "preflight/run.rs should adapt preflight facts into focused model-tier policy directly"
    );
    for forbidden_snippet in [
        "pub enum Harness",
        "pub struct UsedTokens",
        "pub const CLAUDE_CONTEXT_WINDOW",
        "pub fn claude_project_hash(",
        "pub fn claude_transcript_path(",
        "pub fn claude_projects_subdir(",
        "pub fn parse_claude_jsonl_used_tokens(",
        "pub fn context_window_for_model(",
        "pub fn context_pct(",
        "pub fn parse_codex_jsonl_context_pct(",
        "pub struct ClearDecision",
        "pub fn clear_decision(",
    ] {
        assert!(
            !orchestration_context.contains(forbidden_snippet),
            "context_pct.rs must stay a transcript IO adapter and not re-own context usage policy: {forbidden_snippet}"
        );
    }
    assert!(
        orchestration_context.contains("use agent_doc_model_tier::context_usage::{"),
        "context_pct.rs should call focused model-tier context usage helpers directly"
    );

    let codex_hook =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/codex_hook.rs")).unwrap();
    assert!(
        codex_hook.contains("use agent_doc_model_tier::context_usage::{")
            && codex_hook.contains("clear_decision")
            && codex_hook.contains("Harness"),
        "codex hook should use focused context usage policy directly"
    );
}

#[test]
fn test_agent_doc_prompt_cache_owns_prompt_cache_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let package_version = workspace["package"]["version"].as_str();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-prompt-cache")),
        "agent-doc-prompt-cache must stay a first-class workspace crate"
    );

    let prompt_cache_source =
        fs::read_to_string(manifest_dir.join("agent-doc-prompt-cache/src/lib.rs")).unwrap();
    for required_snippet in [
        "pub const PROMPT_CACHE_BOUNDARY",
        "pub const PROMPT_CACHE_CONTROL",
        "pub struct PromptCacheBlocks",
        "pub struct PromptCacheReplayKey",
        "pub struct PromptCacheSessionCostSample",
        "pub struct PromptCacheEffectivenessSample",
        "pub struct PromptCacheMissCause",
        "pub struct PromptCacheTrendThresholds",
        "pub enum PromptCacheTrendStatus",
        "pub struct PromptCacheTrendCheck",
        "pub fn render_prompt_cache_blocks",
        "pub fn rank_cache_miss_causes",
        "pub fn check_prompt_cache_effectiveness_trend",
        "pub fn render_cache_miss_ranking",
    ] {
        assert!(
            prompt_cache_source.contains(required_snippet),
            "prompt-cache policy crate must own {required_snippet}"
        );
    }

    let prompt_cache_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-prompt-cache/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&prompt_cache_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    assert!(dependencies.contains_key("serde"));
    assert!(dependencies.contains_key("sha2"));
    for forbidden in [
        "agent-doc-controller",
        "agent-doc-core",
        "agent-doc-orchestration",
        "agent-doc-project-config-io",
        "agent-doc-tmux-commands",
        "agent-doc-tmux-io",
        "anyhow",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-prompt-cache must stay free of core, orchestration, git, editor IPC, sqlite, or tmux crates"
        );
    }

    let orchestration_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/Cargo.toml")).unwrap();
    let orchestration: toml::Value = toml::from_str(&orchestration_manifest).unwrap();
    let orchestration_dependencies = orchestration["dependencies"].as_table().unwrap();
    let dependency = orchestration_dependencies["agent-doc-prompt-cache"]
        .as_table()
        .unwrap();
    assert_eq!(
        dependency.get("path").and_then(toml::Value::as_str),
        Some("../agent-doc-prompt-cache")
    );
    assert_eq!(
        dependency.get("version").and_then(toml::Value::as_str),
        package_version
    );

    let orchestration_prompt_cache =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/prompt_cache.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub use agent_doc_prompt_cache",
        "pub const PROMPT_CACHE_BOUNDARY",
        "pub const PROMPT_CACHE_CONTROL",
        "pub struct PromptCacheBlocks",
        "pub struct PromptCacheReplayKey",
        "pub struct PromptCacheSessionCostSample",
        "pub struct PromptCacheEffectivenessSample",
        "pub struct PromptCacheMissCause",
        "pub struct PromptCacheTrendThresholds",
        "pub enum PromptCacheTrendStatus",
        "pub struct PromptCacheTrendCheck",
        "pub fn render_prompt_cache_blocks",
        "pub fn rank_cache_miss_causes",
        "pub fn check_prompt_cache_effectiveness_trend",
        "pub fn render_cache_miss_ranking",
        "fn cached_input_loss",
        "fn creation_token_spike",
        "fn content_sha256",
    ] {
        assert!(
            !orchestration_prompt_cache.contains(forbidden_snippet),
            "orchestration must not define or re-export prompt-cache policy: {forbidden_snippet}"
        );
    }
    assert!(
        orchestration_prompt_cache.contains("append_prompt_cache_effectiveness_sample"),
        "orchestration may keep prompt-cache history file IO adapters"
    );

    let run_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/run.rs")).unwrap();
    assert!(
        run_source.contains("use agent_doc_prompt_cache::{"),
        "run.rs should import focused prompt-cache APIs directly"
    );
    assert!(
        !run_source.contains("crate::prompt_cache::PromptCache")
            && !run_source.contains("crate::prompt_cache::PROMPT_CACHE")
            && !run_source.contains("crate::prompt_cache::render_cache_miss_ranking"),
        "run.rs must not route pure prompt-cache policy through orchestration"
    );
}

#[test]
fn test_agent_doc_queue_owns_queue_continuation_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-queue")),
        "agent-doc-queue must stay a first-class workspace crate"
    );
    assert!(
        manifest_dir
            .join("agent-doc-queue/src/queue_continuation.rs")
            .exists(),
        "queue continuation drainability policy should live in the queue crate"
    );
    assert!(
        manifest_dir
            .join("agent-doc-queue/src/queue_journal.rs")
            .exists(),
        "queue journal replay policy should live in the queue crate"
    );
    let queue_policy =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_continuation.rs")).unwrap();
    for required_snippet in [
        "pub struct QueueContinuation",
        "pub struct BacklogDrainSkip",
        "pub fn required_continuation",
        "pub fn collect_backlog_execution_contexts",
        "pub fn partition_drainable_backlog_ids",
        "pub fn drainable_head_prompt_for_scope",
    ] {
        assert!(
            queue_policy.contains(required_snippet),
            "agent-doc-queue must own queue continuation content policy: {required_snippet}"
        );
    }

    let orchestration_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/queue_continuation.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub struct QueueContinuation",
        "fn detect_in_content",
        "frontmatter_io::parse_for_file_with_context",
        "agent_doc_element::element::parse(content)",
        "document_queue::detect_head_prompt_modified",
        "pub fn deferred_backlog_ids",
        "pub(crate) fn deferred_backlog_ids",
        "pub fn supervisor_deferred_backlog_ids",
        "fn head_is_drainable",
        "pub fn is_drainable_queue_head",
        "pub(crate) fn is_drainable_queue_head",
        "pub fn is_noise_queue_head",
        "pub(crate) fn is_noise_queue_head",
        "pub fn live_drainable_continuation_head",
        "pub fn drainable_head_count",
        "pub fn review_phase_routed",
        "const QUEUE_DIRECTIVE_VERBS",
    ] {
        assert!(
            !orchestration_source.contains(forbidden_snippet),
            "orchestration must not re-own pure queue continuation policy: {forbidden_snippet}"
        );
    }
    assert!(
        orchestration_source.contains("queue_policy::required_continuation"),
        "orchestration queue_continuation should read file/snapshot and call agent-doc-queue directly"
    );
    let preflight_maintenance = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/preflight/maintenance.rs"),
    )
    .unwrap();
    for forbidden_snippet in [
        "pub(crate) struct UndrainableSkip",
        "struct UndrainableSkip",
        "pub(crate) fn partition_drainable_backlog_ids",
        "pub(crate) fn collect_backlog_execution_contexts",
    ] {
        assert!(
            !preflight_maintenance.contains(forbidden_snippet),
            "preflight maintenance must not re-own queue backlog drain policy: {forbidden_snippet}"
        );
    }
    assert!(
        preflight_maintenance
            .contains("agent_doc_queue::queue_continuation::collect_backlog_execution_contexts")
            && preflight_maintenance
                .contains("agent_doc_queue::queue_continuation::partition_drainable_backlog_ids"),
        "preflight maintenance should call queue backlog drain policy from agent-doc-queue directly"
    );
    let queue_journal_source =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_journal.rs")).unwrap();
    for required_snippet in [
        "pub struct QueueJournalEntry",
        "pub fn queue_prompts(",
        "pub fn present_queue_texts(",
        "pub fn plan_append_entries(",
        "pub fn missing_entries(",
        "pub fn replay_missing_entries(",
        "pub fn unique_queue_prompts_from_contents(",
        "pub fn merge_missing_into_content(",
    ] {
        assert!(
            queue_journal_source.contains(required_snippet),
            "agent-doc-queue must own queue journal policy: {required_snippet}"
        );
    }
    let orchestration_queue_journal =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/queue_journal.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub struct QueueJournalEntry",
        "fn queue_prompts(",
        "pub fn queue_prompts(",
        "fn present_queue_texts(",
        "pub fn present_queue_texts(",
        "fn plan_append_entries(",
        "pub fn plan_append_entries(",
        "fn missing_entries(",
        "pub fn missing_entries(",
        "fn replay_missing_entries(",
        "pub fn replay_missing_entries(",
        "fn unique_queue_prompts_from_contents(",
        "pub fn unique_queue_prompts_from_contents(",
        "fn durable_queue_texts(",
        "pub fn merge_missing_into_content(",
        "pub use agent_doc_queue::queue_journal",
    ] {
        assert!(
            !orchestration_queue_journal.contains(forbidden_snippet),
            "orchestration must not re-own or facade queue journal policy: {forbidden_snippet}"
        );
    }
    assert!(
        orchestration_queue_journal
            .contains("agent_doc_queue::queue_journal as queue_journal_policy")
            && orchestration_queue_journal.contains("queue_journal_policy::queue_prompts(")
            && orchestration_queue_journal.contains("queue_journal_policy::plan_append_entries(")
            && orchestration_queue_journal
                .contains("queue_journal_policy::unique_queue_prompts_from_contents(")
            && orchestration_queue_journal
                .contains("queue_journal_policy::replay_missing_entries("),
        "orchestration queue journal adapter should call focused queue policy directly"
    );
    let start_run_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/start/run.rs")).unwrap();
    assert!(
        start_run_source.contains("agent_doc_queue::queue_journal::merge_missing_into_content")
            && !start_run_source.contains("crate::queue_journal::merge_missing_into_content"),
        "startup replay should merge through the focused queue journal policy directly"
    );

    let queue_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&queue_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    assert!(dependencies.contains_key("agent-doc-element"));
    assert!(dependencies.contains_key("agent-doc-element-backlog"));
    assert!(dependencies.contains_key("agent-doc-element-queue"));
    assert!(dependencies.contains_key("agent-doc-frontmatter"));
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-queue must stay free of core, orchestration, git, editor IPC, sqlite, or tmux crates"
        );
    }
}

#[test]
fn test_agent_doc_queue_owns_queue_convergence_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let queue_lib = fs::read_to_string(manifest_dir.join("agent-doc-queue/src/lib.rs")).unwrap();
    assert!(
        queue_lib.contains("pub mod queue_convergence;"),
        "agent-doc-queue should expose queue convergence policy"
    );

    let queue_convergence =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_convergence.rs")).unwrap();
    for required_snippet in [
        "pub fn realign_baseline_to_converged_queue(",
        "pub fn queue_body_diff_is_non_selected_future_state(",
        "fn first_queue_prompt_identity(",
        "fn content_without_queue_body(",
        "fn strip_exchange_boundary_lines(",
        "fn realign_component_when_only_in_progress_marker_changed(",
    ] {
        assert!(
            queue_convergence.contains(required_snippet),
            "agent-doc-queue must own queue convergence policy: {required_snippet}"
        );
    }

    let preflight_run =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/preflight/run.rs"))
            .unwrap();
    for forbidden_snippet in [
        "fn realign_baseline_to_converged_queue(",
        "fn queue_body_diff_is_non_selected_future_state(",
        "fn first_queue_prompt_identity(",
        "fn queue_prompt_identity(",
        "fn content_without_queue_body(",
        "fn strip_exchange_boundary_lines(",
        "fn realign_component_when_only_in_progress_marker_changed(",
        "fn pending_body_without_in_progress_markers(",
        "pub use agent_doc_queue::queue_convergence",
    ] {
        assert!(
            !preflight_run.contains(forbidden_snippet),
            "preflight/run.rs must not re-own or facade queue convergence policy: {forbidden_snippet}"
        );
    }
    assert!(
        preflight_run.contains("use agent_doc_queue::queue_convergence::{")
            && preflight_run.contains("realign_baseline_to_converged_queue")
            && preflight_run.contains("queue_body_diff_is_non_selected_future_state"),
        "preflight/run.rs should call focused queue convergence policy directly"
    );
}

#[test]
fn test_agent_doc_queue_owns_do_directive_target_parsing() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let queue_directive =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_directive.rs")).unwrap();
    for required in [
        "pub fn explicit_do_directive_target_id",
        "pub fn explicit_do_directive_target_ids",
        "pub fn do_directive_target_ids",
        "pub fn do_directive_target_ids_in_line",
        "fn leads_with_bare_id_token",
    ] {
        assert!(
            queue_directive.contains(required),
            "agent-doc-queue must own id-backed queue directive parsing: {required}"
        );
    }

    let backlog_source =
        fs::read_to_string(manifest_dir.join("agent-doc-element-backlog/src/backlog.rs")).unwrap();
    assert!(
        backlog_source.contains("pub fn extract_pending_hash_ids"),
        "ordered tracked-work #id scanning should live with backlog/tracked-work parsing"
    );

    let done_signals = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/done_signals.rs"),
    )
    .unwrap();
    for forbidden in [
        "pub fn do_directive_target_ids",
        "pub(crate) fn do_directive_target_ids_in_line",
        "pub(crate) fn extract_pending_hash_ids",
        "pub(crate) fn leads_with_bare_id_token",
    ] {
        assert!(
            !done_signals.contains(forbidden),
            "session_check must not re-own queue directive parsing: {forbidden}"
        );
    }
    let closeout_signal =
        fs::read_to_string(manifest_dir.join("agent-doc-turn/src/closeout_signal.rs")).unwrap();
    assert!(
        closeout_signal.contains("agent_doc_element_backlog::backlog::extract_pending_hash_ids"),
        "done signal parsing should reuse the focused tracked-work #id scanner from agent-doc-turn"
    );

    let tsift_graph = fs::read_to_string(manifest_dir.join("src/tsift_graph.rs")).unwrap();
    for forbidden in [
        "pub(crate) fn extract_do_target",
        "pub(crate) fn extract_do_targets",
        "fn extract_hash_ids",
    ] {
        assert!(
            !tsift_graph.contains(forbidden),
            "root tsift_graph must not re-own queue directive target parsing: {forbidden}"
        );
    }
    assert!(
        tsift_graph.contains("agent_doc_queue::queue_directive::explicit_do_directive_target_id"),
        "root tsift graph should call focused explicit-do target parsing directly"
    );
    for relative in ["src/plan.rs", "src/jobs.rs"] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            source.contains("agent_doc_queue::queue_directive::explicit_do_directive_target_ids"),
            "{relative} should call focused explicit-do target parsing directly"
        );
        assert!(
            !source.contains("crate::tsift_graph::extract_do_targets"),
            "{relative} must not route explicit-do target parsing through tsift_graph"
        );
    }

    for relative in [
        "agent-doc-orchestration/src/preflight/run.rs",
        "agent-doc-orchestration/src/project_controller.rs",
        "agent-doc-orchestration/src/session_check/queue_head_guards.rs",
        "agent-doc-orchestration/src/session_check/response_guards.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            source.contains("agent_doc_queue::queue_directive::do_directive_target_ids"),
            "{relative} should call focused queue directive parsing directly"
        );
        assert!(
            !source.contains("crate::session_check::do_directive_target_ids"),
            "{relative} must not route queue directive parsing through session_check"
        );
    }
    let queue_closeout_guard =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_closeout_guard.rs"))
            .unwrap();
    assert!(
        queue_closeout_guard.contains("queue_directive::do_directive_target_ids"),
        "queue closeout guard policy should call focused queue directive parsing directly"
    );
    let queue_head_provenance_guards = fs::read_to_string(
        manifest_dir
            .join("agent-doc-orchestration/src/session_check/queue_head_provenance_guards.rs"),
    )
    .unwrap();
    assert!(
        !queue_head_provenance_guards
            .contains("agent_doc_queue::queue_directive::do_directive_target_ids"),
        "queue_head_provenance_guards should consume queue_closeout_guard policy instead of parsing queue directives directly"
    );
}

#[test]
fn test_agent_doc_queue_owns_queue_response_head_matching_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let queue_response =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_response.rs")).unwrap();
    let queue_directive =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_directive.rs")).unwrap();

    for required in [
        "pub fn queue_prompt_done_id",
        "pub fn normalize_done_id",
        "pub fn response_heading_topic",
        "pub fn response_topic_matches_queue_head",
        "pub fn normalize_queue_prompt_text",
        "pub fn queue_prompt_text_matches",
    ] {
        assert!(
            queue_response.contains(required),
            "agent-doc-queue must own queue response/head matching policy: {required}"
        );
    }
    for required in [
        "pub fn topic_resolves_to_exact_id",
        "pub fn topic_resolves_to_only_id_directives",
    ] {
        assert!(
            queue_directive.contains(required),
            "agent-doc-queue must own queue topic id-resolution policy: {required}"
        );
    }

    for relative in [
        "agent-doc-orchestration/src/write/queue_consume.rs",
        "agent-doc-orchestration/src/preflight.rs",
        "agent-doc-orchestration/src/preflight/maintenance.rs",
        "agent-doc-orchestration/src/write.rs",
        "agent-doc-orchestration/src/project_controller/rpc.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        for forbidden in [
            "fn queue_prompt_done_id(",
            "fn normalize_done_id(",
            "fn response_heading_topic(",
            "fn response_topic_matches_queue_head(",
            "fn topic_resolves_to_exact_id(",
            "fn topic_resolves_to_only_id_directives(",
        ] {
            assert!(
                !source.contains(forbidden),
                "{relative} must not re-own queue response/head matching policy: {forbidden}"
            );
        }
    }
}

#[test]
fn test_agent_doc_queue_owns_queue_consumption_entry_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let queue_lib = fs::read_to_string(manifest_dir.join("agent-doc-queue/src/lib.rs")).unwrap();
    assert!(
        queue_lib.contains("pub mod queue_consume;"),
        "agent-doc-queue should expose queue consumption entry policy"
    );

    let queue_consume_policy =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_consume.rs")).unwrap();
    for required in [
        "pub fn first_n_queue_prompt_texts",
        "pub fn queue_consume_count_for_done_ids",
        "pub fn queue_prompt_texts_match_for_consumption",
        "pub fn mark_first_matching_prompts_completed_by_texts",
        "pub fn mark_entries_completed_by_done_ids",
        "pub fn normalized_done_id_bag",
    ] {
        assert!(
            queue_consume_policy.contains(required),
            "agent-doc-queue must own queue consumption entry policy: {required}"
        );
    }

    let orchestration_queue_consume =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/queue_consume.rs"))
            .unwrap();
    for forbidden in [
        "pub(crate) fn first_n_queue_prompt_texts",
        "pub(crate) fn queue_consume_count_for_done_ids",
        "fn queue_prompt_texts_match_for_consumption",
        "fn mark_first_matching_prompts_completed_by_texts",
        "pub(crate) fn mark_entries_completed_by_done_ids",
        "pub(crate) fn normalized_done_id_bag",
    ] {
        assert!(
            !orchestration_queue_consume.contains(forbidden),
            "write/queue_consume.rs must not re-own queue consumption entry policy: {forbidden}"
        );
    }
    assert!(
        orchestration_queue_consume.contains("queue_consume::{"),
        "write/queue_consume.rs should call queue consumption entry policy through agent-doc-queue directly"
    );
}

#[test]
fn test_agent_doc_queue_owns_queue_prompt_drift_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let queue_lib = fs::read_to_string(manifest_dir.join("agent-doc-queue/src/lib.rs")).unwrap();
    assert!(
        queue_lib.contains("pub mod queue_prompt_drift;"),
        "agent-doc-queue should expose queue prompt drift policy"
    );

    let queue_prompt_drift =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_prompt_drift.rs")).unwrap();
    for required in [
        "pub fn queue_component_text",
        "pub fn queue_prompt_texts",
        "pub fn queue_prompt_texts_including_consumed",
        "pub fn queue_prompt_deletions_between",
        "pub fn dropped_queue_prompt_lines_after_content_ours",
        "pub fn preserve_content_ours_over_live_queue_deletions",
    ] {
        assert!(
            queue_prompt_drift.contains(required),
            "agent-doc-queue must own queue prompt drift policy: {required}"
        );
    }

    let orchestration_write =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write.rs")).unwrap();
    for forbidden in [
        "fn queue_component_text(",
        "fn queue_prompt_texts(",
        "fn queue_prompt_texts_including_consumed(",
        "fn queue_prompt_counts(",
        "fn queue_prompt_count(",
        "fn queue_prompt_deletions_between(",
        "fn dropped_queue_prompt_lines_after_content_ours(",
        "fn preserve_content_ours_over_live_queue_deletions(",
    ] {
        assert!(
            !orchestration_write.contains(forbidden),
            "write.rs must not re-own queue prompt drift policy: {forbidden}"
        );
    }
    assert!(
        orchestration_write.contains("agent_doc_queue::queue_prompt_drift::{"),
        "write.rs should call queue prompt drift policy through agent-doc-queue directly"
    );
}

#[test]
fn test_agent_doc_queue_owns_free_text_response_proof_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let queue_response =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_response.rs")).unwrap();
    for required in [
        "pub fn normalize_for_answer_match",
        "pub fn head_carries_in_progress_marker",
        "pub fn free_text_head_match_prose",
        "pub fn free_text_head_answered_by_response",
    ] {
        assert!(
            queue_response.contains(required),
            "agent-doc-queue must own free-text queue response proof policy: {required}"
        );
    }

    for relative in [
        "agent-doc-orchestration/src/write/queue_consume.rs",
        "agent-doc-orchestration/src/session_check/queue_head_provenance_guards.rs",
        "agent-doc-orchestration/src/preflight/maintenance.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        for forbidden in [
            "fn normalize_for_answer_match",
            "fn response_blockquote_text",
            "fn response_explicit_queue_prompt_echoes_head",
            "fn head_carries_in_progress_marker",
            "fn free_text_head_match_prose",
            "fn free_text_head_answered_by_response",
        ] {
            assert!(
                !source.contains(forbidden),
                "{relative} must not re-own free-text queue response proof policy: {forbidden}"
            );
        }
    }

    let queue_consume =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/queue_consume.rs"))
            .unwrap();
    assert!(
        queue_consume.contains("free_text_head_answered_by_response")
            && queue_consume.contains("free_text_head_match_prose")
            && queue_consume.contains("head_carries_in_progress_marker")
            && queue_consume.contains("normalize_for_answer_match"),
        "queue_consume should import focused free-text queue response proof policy directly"
    );
}

#[test]
fn test_agent_doc_queue_owns_queue_prompt_echo_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let queue_response =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_response.rs")).unwrap();
    for required in [
        "pub fn first_nonempty_line",
        "pub fn format_consumed_prompt_echo",
        "pub fn summarize_consumed_prompt",
        "pub fn line_is_response_heading",
        "pub fn normalize_prompt_line",
        "pub fn locate_response_heading_offset",
        "pub fn embed_consumed_prompt_in_response",
    ] {
        assert!(
            queue_response.contains(required),
            "agent-doc-queue must own queue prompt echo/embedding policy: {required}"
        );
    }

    let queue_consume =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/queue_consume.rs"))
            .unwrap();
    for forbidden in [
        "pub(crate) fn first_nonempty_line",
        "pub(crate) fn format_consumed_prompt_echo",
        "pub(crate) fn summarize_consumed_prompt",
        "pub(crate) fn line_is_response_heading",
        "pub(crate) fn normalize_prompt_line",
        "fn normalize_prompt_echo_presence_line",
        "pub(crate) fn locate_response_heading_offset",
        "pub(crate) fn embed_consumed_prompt_in_response",
    ] {
        assert!(
            !queue_consume.contains(forbidden),
            "write/queue_consume.rs must not re-own queue prompt echo/embedding policy: {forbidden}"
        );
    }
    assert!(
        queue_consume.contains("embed_consumed_prompt_in_response")
            && queue_consume.contains("first_nonempty_line"),
        "queue_consume should call focused queue prompt echo policy directly"
    );

    let codex_hook =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/codex_hook.rs")).unwrap();
    assert!(
        codex_hook.contains("agent_doc_queue::queue_response::format_consumed_prompt_echo")
            && !codex_hook.contains("crate::write::format_consumed_prompt_echo"),
        "codex_hook must call queue prompt echo policy through agent-doc-queue directly"
    );
}

#[test]
fn test_agent_doc_queue_owns_queue_command_classification() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let queue_command =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_command.rs")).unwrap();
    for required in [
        "pub fn is_queue_directive_prompt",
        "pub fn mentions_slash_command_reference",
    ] {
        assert!(
            queue_command.contains(required),
            "agent-doc-queue must own queue command/prompt classification: {required}"
        );
    }

    let response_guards = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/response_guards.rs"),
    )
    .unwrap();
    for forbidden in [
        "pub(crate) fn is_queue_directive_prompt",
        "pub(crate) fn mentions_slash_command",
    ] {
        assert!(
            !response_guards.contains(forbidden),
            "response_guards must not re-own queue command/prompt classification: {forbidden}"
        );
    }
    for required in [
        "agent_doc_queue::queue_command::is_queue_directive_prompt",
        "agent_doc_queue::queue_command::mentions_slash_command_reference",
    ] {
        assert!(
            response_guards.contains(required),
            "response_guards should call focused queue command classification directly: {required}"
        );
    }
}

#[test]
fn test_agent_doc_turn_owns_closeout_signal_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        manifest_dir
            .join("agent-doc-turn/src/closeout_signal.rs")
            .exists(),
        "closeout response and done-signal policy should live in the focused turn crate"
    );
    assert!(
        manifest_dir
            .join("agent-doc-turn/src/closeout_recovery.rs")
            .exists(),
        "pure closeout recovery policy should live in the focused turn crate"
    );
    assert!(
        manifest_dir
            .join("agent-doc-turn/src/closeout_guard.rs")
            .exists(),
        "closeout guard vocabulary should live in the focused turn crate"
    );
    assert!(
        manifest_dir
            .join("agent-doc-turn/src/exchange_tail.rs")
            .exists(),
        "exchange-tail prompt/response policy should live in the focused turn crate"
    );

    let turn_source =
        fs::read_to_string(manifest_dir.join("agent-doc-turn/src/closeout_signal.rs")).unwrap();
    let exchange_tail_source =
        fs::read_to_string(manifest_dir.join("agent-doc-turn/src/exchange_tail.rs")).unwrap();
    let guard_source =
        fs::read_to_string(manifest_dir.join("agent-doc-turn/src/closeout_guard.rs")).unwrap();
    let recovery_source =
        fs::read_to_string(manifest_dir.join("agent-doc-turn/src/closeout_recovery.rs")).unwrap();
    for required in [
        "pub enum CloseoutGuardReason",
        "pub enum CloseoutGuardOutcome",
        "pub struct CommittedWithoutResponseBodyEvidence",
        "pub enum CommittedWithoutResponseBodyDecision",
        "pub fn closeout_cycle_phase_from_str",
        "pub const fn closeout_terminal_guard_outcome",
        "pub fn exchange_has_assistant_response_body",
        "pub fn committed_without_response_body_decision",
    ] {
        assert!(
            guard_source.contains(required),
            "agent-doc-turn must own closeout guard policy: {required}"
        );
    }
    let response_guards = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/response_guards.rs"),
    )
    .unwrap();
    for forbidden in [
        "if !matches!(state.phase, agent_doc_turn::CyclePhase::Committed)",
        "if state.capture_id.is_some() || state.response_sha256.is_some()",
        "if !state.had_pending_mutations",
        "if crate::cycle_state::is_noop_commit_event(&state.last_event)",
    ] {
        assert!(
            !response_guards.contains(forbidden),
            "response_guards must not re-own committed-without-response-body policy: {forbidden}"
        );
    }
    assert!(
        response_guards
            .contains("agent_doc_turn::closeout_guard::committed_without_response_body_decision")
            && response_guards
                .contains("agent_doc_turn::closeout_guard::exchange_has_assistant_response_body"),
        "response_guards should call focused turn committed-response guard policy directly"
    );
    for required in [
        "pub enum CloseoutRecoveryState",
        "pub enum CloseoutRecoveryDrift",
        "pub struct CloseoutRecoveryCycleInput",
        "pub struct CloseoutRecoveryStateInput",
        "pub fn classify_closeout_recovery_state_from_input",
        "pub fn closeout_content_component_signature",
        "pub struct OpenCycleRecoveryCommandInput",
        "pub struct CloseoutRecoveryCommandInput",
        "pub fn closeout_recovery_command",
        "pub fn open_cycle_recovery_command",
        "pub struct CloseoutRecoveryDecisionInput",
        "pub enum CloseoutRecoveryDecision",
        "pub fn closeout_recovery_decision_from_state",
        "pub enum CloseoutRecoveryMutationReason",
        "pub enum MetadataDriftAuthority",
        "pub const fn capture_refresh_event",
        "pub const fn capture_refresh_message",
        "pub fn metadata_drift_authority",
    ] {
        assert!(
            recovery_source.contains(required),
            "agent-doc-turn must own closeout recovery policy: {required}"
        );
    }
    let closeout_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/flow/closeout.rs"))
            .unwrap();
    let flow_types_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/flow/types.rs")).unwrap();
    for forbidden in [
        "pub enum CloseoutGuardReason",
        "impl CloseoutGuardReason",
        "pub fn closeout_state_from_cycle_phase",
        "pub fn terminal_guard_outcome",
    ] {
        assert!(
            !closeout_source.contains(forbidden),
            "flow::closeout must not re-own or facade focused closeout guard policy: {forbidden}"
        );
    }
    assert!(
        !flow_types_source.contains("pub enum CloseoutState"),
        "flow types must not keep a duplicate closeout state vocabulary"
    );
    assert!(
        closeout_source.contains("use agent_doc_turn::closeout_guard::CloseoutGuardReason;"),
        "flow::closeout should adapt focused closeout guard reasons into flow events"
    );
    for relative in [
        "agent-doc-orchestration/src/flow/mod.rs",
        "agent-doc-orchestration/src/git.rs",
        "agent-doc-orchestration/src/repair.rs",
        "agent-doc-orchestration/src/write.rs",
        "agent-doc-orchestration/src/write/ipc/transport.rs",
        "agent-doc-orchestration/src/write/pending_checks.rs",
        "agent-doc-orchestration/src/write/run_entry.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            !source.contains("crate::flow::closeout::CloseoutGuardReason")
                && !source.contains("closeout::CloseoutGuardReason"),
            "{relative} should use focused closeout guard reasons directly"
        );
    }
    for forbidden in [
        "pub enum CloseoutRecoveryState",
        "pub struct CloseoutRecoveryDecisionInput",
        "pub enum CloseoutRecoveryDecision",
        "pub fn closeout_recovery_decision_from_state",
        "pub enum CloseoutRecoveryMutationReason",
        "pub enum MetadataDriftAuthority",
        "impl CloseoutRecoveryMutationReason",
        "pub fn metadata_drift_authority",
        "pub fn closeout_recovery_command(",
        "fn open_cycle_recovery_command(",
        "pipe the final response (with `<!-- patch:exchange -->` blocks)",
        "rewrite the response with real `<!-- patch:exchange -->` blocks",
        "preserve the user-authored content and finish through `agent-doc finalize",
        "pub fn classify_closeout_recovery_state(",
        "fn content_component_signature(",
    ] {
        assert!(
            !closeout_source.contains(forbidden),
            "orchestration must not re-own or facade closeout recovery policy: {forbidden}"
        );
    }
    assert!(
        closeout_source.contains("use agent_doc_turn::closeout_recovery::{")
            && closeout_source.contains("CloseoutRecoveryDecision")
            && closeout_source.contains("CloseoutRecoveryDecisionInput")
            && closeout_source.contains("CloseoutRecoveryDrift")
            && closeout_source.contains("CloseoutRecoveryStateInput")
            && closeout_source.contains("CloseoutRecoveryMutationReason")
            && closeout_source.contains("CloseoutRecoveryState")
            && closeout_source.contains("CloseoutRecoveryCommandInput")
            && closeout_source.contains("OpenCycleRecoveryCommandInput")
            && closeout_source.contains("MetadataDriftAuthority")
            && closeout_source.contains("classify_closeout_recovery_state_from_input")
            && closeout_source.contains("closeout_content_component_signature")
            && closeout_source
                .contains("closeout_recovery_command as render_closeout_recovery_command")
            && closeout_source.contains("closeout_recovery_decision_from_state")
            && closeout_source.contains("metadata_drift_authority"),
        "orchestration closeout recovery should call focused turn policy directly"
    );
    for (relative, required) in [
        (
            "agent-doc-orchestration/src/capture.rs",
            "use agent_doc_turn::closeout_recovery::CloseoutRecoveryMutationReason;",
        ),
        (
            "agent-doc-orchestration/src/repair.rs",
            "use agent_doc_turn::closeout_recovery::CloseoutRecoveryMutationReason;",
        ),
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            source.contains(required),
            "{relative} should use focused closeout recovery reason directly"
        );
        assert!(
            !source.contains("crate::flow::closeout::CloseoutRecoveryMutationReason"),
            "{relative} must not route focused closeout recovery reason through orchestration"
        );
    }
    for required in [
        "pub enum ResponseSource",
        "pub struct ReapedResponseLossInput",
        "pub fn directive_response_source",
        "pub fn content_has_re_heading_for_id",
        "pub fn reaped_directive_ids_without_response",
        "pub fn text_has_blocked_future_action_signal",
        "pub fn text_has_no_followup_justification",
        "pub fn blocked_signal_tied_to_id",
        "pub fn body_enumerates_multiple_gated_phases",
        "pub fn count_phase_markers",
        "pub fn body_already_split_into_child_ids",
        "pub const QUEUE_AUDIT_SUBSTEP_COMPLETE_PHRASES",
        "pub fn queue_audit_collapses_partial_completion",
        "pub fn queue_audit_has_none_complete_claim",
        "pub const QUEUE_AUDIT_GUARD_SUPPRESS_MARKER",
        "pub struct QueueAuditPartialCompletionEvidence",
        "pub enum QueueAuditPartialCompletionDecision",
        "pub fn queue_audit_partial_completion_decision",
        "pub const PARTIAL_CLOSEOUT_REMAINING_PHRASES",
        "pub fn text_has_shipped_signal",
        "pub fn text_has_partial_remaining_signal",
        "pub fn response_text_for_guards",
        "pub const PARTIAL_CLOSEOUT_GUARD_SUPPRESS_MARKER",
        "pub struct PartialCloseoutStateEvidence",
        "pub enum PartialCloseoutStateDecision",
        "pub fn partial_closeout_state_decision",
        "pub const PENDING_DONE_GUARD_SUPPRESS_MARKER",
        "pub struct TrackedWorkCompletionEvidence",
        "pub enum TrackedWorkCompletionDecision",
        "pub fn pending_done_suppressed",
        "pub fn tracked_work_completion_decision",
        "pub fn tracked_work_completion_missing_done_ids",
        "pub struct ExpectDoneOrGateEvidence",
        "pub enum ExpectDoneOrGateDecision",
        "pub fn expect_done_or_gate_decision",
        "pub const BLOCKED_CLOSEOUT_FOLLOWUP_GUARD_SUPPRESS_MARKER",
        "pub struct BlockedCloseoutFollowupEvidence",
        "pub enum BlockedCloseoutFollowupDecision",
        "pub fn blocked_closeout_followup_decision",
        "pub const GATED_PHASE_SPLIT_GUARD_SUPPRESS_MARKER",
        "pub struct GatedPhaseSplitItemEvidence",
        "pub struct GatedPhaseSplitEvidence",
        "pub enum GatedPhaseSplitDecision",
        "pub fn gated_phase_split_decision",
        "pub fn normalized_prompt_for_match",
        "pub fn exchange_contains_prompt_line",
        "pub fn is_exchange_response_heading",
        "pub fn is_direct_response_patchback_heading",
        "pub fn has_new_response_heading_marker",
        "pub fn is_binary_authored_recovery_diagnostic_heading",
        "pub fn is_queue_continuation_response_heading",
        "pub fn assistant_response_text",
        "pub fn response_clearly_completes_pending_id",
        "pub fn response_heading_resolves_to_pending_id",
        "pub fn explicit_done_signal_ids",
        "pub fn plain_done_signal",
    ] {
        assert!(
            turn_source.contains(required),
            "agent-doc-turn must own closeout signal policy: {required}"
        );
    }
    assert!(
        turn_source.contains("agent_doc_element_backlog::backlog::extract_pending_hash_ids"),
        "closeout signal policy should reuse tracked-work #id scanning"
    );
    for required in [
        "pub fn unresolved_exchange_prompt_in_content",
        "pub fn exchange_tail_has_response_heading",
        "pub fn prompt_only_exchange_tail",
    ] {
        assert!(
            exchange_tail_source.contains(required),
            "agent-doc-turn must own exchange-tail prompt/response policy: {required}"
        );
    }

    let detect_source = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/detect.rs"),
    )
    .unwrap();
    for forbidden in [
        "pub(crate) enum ResponseSource",
        "pub(crate) struct ReapedResponseLossInput",
        "pub(crate) fn directive_response_source",
        "pub(crate) fn content_has_re_heading_for_id",
        "pub(crate) fn reaped_directive_ids_without_response",
    ] {
        assert!(
            !detect_source.contains(forbidden),
            "session_check::detect must not re-own closeout response-loss policy: {forbidden}"
        );
    }

    let queue_head_guards = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/queue_head_guards.rs"),
    )
    .unwrap();
    for required in [
        "agent_doc_turn::closeout_signal::directive_response_source",
        "agent_doc_turn::closeout_signal::reaped_directive_ids_without_response",
        "agent_doc_turn::closeout_signal::ReapedResponseLossInput",
    ] {
        assert!(
            queue_head_guards.contains(required),
            "queue_head_guards should call focused closeout signal policy directly: {required}"
        );
    }

    let closeout_guards = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/closeout_guards.rs"),
    )
    .unwrap();
    for forbidden in [
        "pub fn is_exchange_response_heading",
        "pub fn is_queue_continuation_response_heading",
        "pub(crate) fn has_new_response_heading_marker",
        "pub(crate) fn is_binary_authored_recovery_diagnostic_heading",
        "pub(crate) fn body_enumerates_multiple_gated_phases",
        "pub(crate) fn count_phase_markers",
        "pub(crate) fn body_already_split_into_child_ids",
        "pub(crate) const QUEUE_AUDIT_SUBSTEP_COMPLETE_PHRASES",
        "pub(crate) fn queue_audit_collapses_partial_completion",
        "pub(crate) fn queue_audit_has_none_complete_claim",
        "agent_doc_turn::closeout_signal::queue_audit_collapses_partial_completion",
        ".contains(\"<!-- no-queue-audit-guard -->\")",
        "agent_doc_turn::closeout_signal::response_text_for_guards",
        "agent_doc_turn::closeout_signal::text_has_blocked_future_action_signal",
        "agent_doc_turn::closeout_signal::text_has_no_followup_justification",
        "agent_doc_turn::closeout_signal::blocked_signal_tied_to_id",
        ".contains(\"<!-- no-blocked-followup-guard -->\")",
        ".contains(\"<!-- no-pending-done-guard -->\")",
        "agent_doc_turn::closeout_signal::body_enumerates_multiple_gated_phases",
        "agent_doc_turn::closeout_signal::body_already_split_into_child_ids",
        ".contains(\"<!-- no-gated-phase-split-guard -->\")",
        "agent_doc_element_backlog::backlog::normalize_pending_id",
        "let kept_open: std::collections::HashSet<String>",
        "let directed: std::collections::HashSet<String>",
    ] {
        assert!(
            !closeout_guards.contains(forbidden),
            "closeout_guards must not re-own gated-phase closeout policy: {forbidden}"
        );
    }
    for required in [
        "agent_doc_turn::closeout_signal::blocked_closeout_followup_decision",
        "agent_doc_turn::closeout_signal::BlockedCloseoutFollowupEvidence",
        "agent_doc_turn::closeout_signal::BlockedCloseoutFollowupDecision",
        "agent_doc_turn::closeout_signal::gated_phase_split_decision",
        "agent_doc_turn::closeout_signal::GatedPhaseSplitEvidence",
        "agent_doc_turn::closeout_signal::GatedPhaseSplitItemEvidence",
        "agent_doc_turn::closeout_signal::GatedPhaseSplitDecision",
        "agent_doc_turn::closeout_signal::queue_audit_partial_completion_decision",
        "agent_doc_turn::closeout_signal::QueueAuditPartialCompletionEvidence",
        "agent_doc_turn::closeout_signal::QueueAuditPartialCompletionDecision",
        "agent_doc_turn::closeout_signal::is_exchange_response_heading",
        "agent_doc_turn::closeout_signal::is_direct_response_patchback_heading",
        "agent_doc_turn::closeout_signal::has_new_response_heading_marker",
        "agent_doc_turn::closeout_signal::is_binary_authored_recovery_diagnostic_heading",
    ] {
        assert!(
            closeout_guards.contains(required),
            "closeout_guards should call focused closeout signal policy directly: {required}"
        );
    }
    for required in [
        "super::closeout_signal::is_exchange_response_heading",
        "super::closeout_signal::is_queue_continuation_response_heading",
        "super::closeout_signal::normalized_prompt_for_match",
    ] {
        assert!(
            exchange_tail_source.contains(required),
            "exchange_tail should reuse focused closeout signal policy directly: {required}"
        );
    }
    for forbidden in [
        "pub(crate) fn unresolved_exchange_prompt_in_content",
        "pub(crate) fn prompt_only_exchange_tail",
    ] {
        assert!(
            !closeout_guards.contains(forbidden),
            "closeout_guards must not re-own exchange-tail prompt/response policy: {forbidden}"
        );
    }
    assert!(
        closeout_guards
            .contains("agent_doc_turn::exchange_tail::unresolved_exchange_prompt_in_content")
            && closeout_guards
                .contains("agent_doc_turn::exchange_tail::exchange_tail_has_response_heading"),
        "closeout_guards should keep only file adapters over focused exchange-tail policy"
    );
    assert!(
        detect_source.contains("agent_doc_turn::exchange_tail::prompt_only_exchange_tail"),
        "session_check::detect should call focused prompt-only exchange-tail policy directly"
    );

    let response_guards = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/response_guards.rs"),
    )
    .unwrap();
    for forbidden in [
        "pub(crate) fn normalized_prompt_for_match",
        "pub(crate) fn exchange_contains_prompt_line",
        "pub(crate) fn assistant_response_text",
    ] {
        assert!(
            !response_guards.contains(forbidden),
            "response_guards must not re-own closeout prompt/response text policy: {forbidden}"
        );
    }
    for required in [
        "agent_doc_turn::closeout_signal::exchange_contains_prompt_line",
        "agent_doc_turn::closeout_signal::assistant_response_text",
        "agent_doc_turn::closeout_signal::normalized_prompt_for_match",
    ] {
        assert!(
            response_guards.contains(required),
            "response_guards should call focused closeout prompt/response text policy directly: {required}"
        );
    }

    let provenance_guards = fs::read_to_string(
        manifest_dir
            .join("agent-doc-orchestration/src/session_check/queue_head_provenance_guards.rs"),
    )
    .unwrap();
    for forbidden in [
        "pub(crate) const BLOCKED_FUTURE_ACTION_PHRASES",
        "pub(crate) const NO_FOLLOWUP_JUSTIFICATION_PHRASES",
        "pub(crate) fn text_has_blocked_future_action_signal",
        "pub(crate) fn text_has_no_followup_justification",
        "pub(crate) fn blocked_signal_tied_to_id",
        "pub(crate) fn free_text_queue_marker_has_bare_heading_residue",
        "pub(crate) fn response_head_plausibly_answers",
        "agent_doc_turn::closeout_signal::free_text_queue_marker_has_bare_heading_residue",
        "agent_doc_turn::closeout_signal::response_head_plausibly_answers",
        "pub(crate) const PARTIAL_CLOSEOUT_REMAINING_PHRASES",
        "pub(crate) fn text_has_shipped_signal",
        "pub(crate) fn text_has_partial_remaining_signal",
        ".contains(\"<!-- no-pending-done-guard -->\")",
        "let resolved: std::collections::HashSet<String>",
    ] {
        assert!(
            !provenance_guards.contains(forbidden),
            "queue_head_provenance_guards must not re-own closeout signal policy: {forbidden}"
        );
    }
    for required in [
        "agent_doc_turn::closeout_signal::expect_done_or_gate_decision",
        "agent_doc_turn::closeout_signal::ExpectDoneOrGateEvidence",
        "agent_doc_turn::closeout_signal::ExpectDoneOrGateDecision",
    ] {
        assert!(
            provenance_guards.contains(required),
            "queue_head_provenance_guards should call focused expect-done-or-gate policy directly: {required}"
        );
    }
    assert!(
        provenance_guards.contains(
            "agent_doc_queue::queue_closeout_guard::free_text_queue_head_provenance_decision"
        ),
        "queue_head_provenance_guards should call focused queue provenance policy directly"
    );

    let partial_staging = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/partial_staging.rs"),
    )
    .unwrap();
    for forbidden in [
        "pub(crate) const PARTIAL_CLOSEOUT_REMAINING_PHRASES",
        "pub(crate) fn text_has_shipped_signal",
        "pub(crate) fn text_has_partial_remaining_signal",
        "agent_doc_turn::closeout_signal::text_has_shipped_signal",
        "agent_doc_turn::closeout_signal::text_has_partial_remaining_signal",
        ".contains(\"<!-- no-partial-closeout-guard -->\")",
        "let resolved: std::collections::HashSet<String>",
        "let open_backlog: std::collections::HashSet<String>",
        "agent_doc_element_backlog::backlog::normalize_pending_id",
    ] {
        assert!(
            !partial_staging.contains(forbidden),
            "partial_staging must not re-own partial closeout signal policy: {forbidden}"
        );
    }
    for required in [
        "agent_doc_turn::closeout_signal::partial_closeout_state_decision",
        "agent_doc_turn::closeout_signal::PartialCloseoutStateEvidence",
        "agent_doc_turn::closeout_signal::PartialCloseoutStateDecision",
    ] {
        assert!(
            partial_staging.contains(required),
            "partial_staging should call focused closeout signal policy directly: {required}"
        );
    }

    let done_signals = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/done_signals.rs"),
    )
    .unwrap();
    for forbidden in [
        "pub(crate) fn response_clearly_completes_pending_id",
        "pub(crate) fn response_heading_resolves_to_pending_id",
        "pub(crate) fn explicit_done_signal_ids",
        "pub(crate) fn plain_done_signal",
        "pub(crate) fn normalize_done_signal_text",
        "fn leading_hash_id",
        "fn extract_bracket_ids",
        "fn contains_completion_marker",
        "agent_doc_element_backlog::backlog::extract_pending_hash_ids",
        "agent_doc_element_backlog::backlog::parse_items(component.content(&content))",
    ] {
        assert!(
            !done_signals.contains(forbidden),
            "session_check must not re-own closeout signal policy: {forbidden}"
        );
    }
    for required in [
        "agent_doc_turn::closeout_signal::explicit_done_signal_ids",
        "agent_doc_turn::closeout_signal::plain_done_signal",
        "agent_doc_element_backlog::backlog::open_tracked_work_ids_in_content",
        "agent_doc_element_backlog::backlog::open_backlog_ids_in_content",
    ] {
        assert!(
            done_signals.contains(required),
            "done_signals should call focused closeout signal policy directly: {required}"
        );
    }

    let preflight_run =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/preflight/run.rs"))
            .unwrap();
    assert!(
        preflight_run.contains("agent_doc_element_backlog::backlog::open_backlog_ids_in_content"),
        "preflight should call focused open-backlog id extraction directly"
    );
    for forbidden in [
        "agent_doc_element_backlog::backlog::parse_items(component.content(content))",
        ".filter(|component| {\n                            agent_doc_element::element::is_backlog_component(&component.name)",
    ] {
        assert!(
            !preflight_run.contains(forbidden),
            "preflight must not re-own open backlog id extraction policy: {forbidden}"
        );
    }

    let pending_guards = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/pending_guards.rs"),
    )
    .unwrap();
    assert!(
        !pending_guards.contains("pub fn response_text_for_guards"),
        "pending_guards must not re-own closeout response text normalization"
    );
    for forbidden in [
        "pub fn detect_missing_pending_done_ids",
        ".contains(\"<!-- no-pending-done-guard -->\")",
        "agent_doc_turn::closeout_signal::response_clearly_completes_pending_id",
    ] {
        assert!(
            !pending_guards.contains(forbidden),
            "pending_guards must not re-own tracked-work completion policy: {forbidden}"
        );
    }
    for required in [
        "agent_doc_turn::closeout_signal::tracked_work_completion_decision",
        "agent_doc_turn::closeout_signal::TrackedWorkCompletionEvidence",
        "agent_doc_turn::closeout_signal::TrackedWorkCompletionDecision",
    ] {
        assert!(
            pending_guards.contains(required),
            "pending_guards should call focused tracked-work completion policy directly: {required}"
        );
    }

    for relative in [
        "agent-doc-orchestration/src/session_check/pending_guards.rs",
        "agent-doc-orchestration/src/write/pending_checks.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            source.contains("agent_doc_turn::closeout_signal::response_text_for_guards"),
            "{relative} should call focused closeout response text normalization directly"
        );
        assert!(
            !source.contains("crate::session_check::response_text_for_guards"),
            "{relative} must not route closeout response text normalization through session_check"
        );
    }

    let backlog_guards = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/backlog_guards.rs"),
    )
    .unwrap();
    assert!(
        backlog_guards
            .contains("agent_doc_turn::closeout_signal::response_clearly_completes_pending_id"),
        "backlog_guards should call focused closeout signal policy directly"
    );
    assert!(
        !backlog_guards.contains("session_check::response_clearly_completes_pending_id"),
        "backlog_guards must not route closeout signal policy through session_check"
    );

    let pending_checks = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/write/pending_checks.rs"),
    )
    .unwrap();
    for forbidden in [
        "crate::session_check::detect_missing_pending_done_ids",
        ".contains(\"<!-- no-pending-done-guard -->\")",
    ] {
        assert!(
            !pending_checks.contains(forbidden),
            "write pending checks must not route tracked-work completion policy through session_check: {forbidden}"
        );
    }
    for required in [
        "agent_doc_turn::closeout_signal::pending_done_suppressed",
        "agent_doc_turn::closeout_signal::tracked_work_completion_missing_done_ids",
        "agent_doc_turn::closeout_signal::tracked_work_completion_decision",
        "agent_doc_turn::closeout_signal::TrackedWorkCompletionEvidence",
        "agent_doc_turn::closeout_signal::TrackedWorkCompletionDecision",
    ] {
        assert!(
            pending_checks.contains(required),
            "write pending checks should call focused tracked-work completion policy directly: {required}"
        );
    }

    let turn_response_text =
        fs::read_to_string(manifest_dir.join("agent-doc-turn/src/response_text.rs")).unwrap();
    assert!(
        turn_response_text.contains("pub fn strip_assistant_heading"),
        "agent-doc-turn must own append response heading normalization"
    );
    for required in [
        "pub fn response_prompt_target_from_re_heading",
        "pub fn summarize_response_for_hook",
        "fn truncate_response_summary",
    ] {
        assert!(
            turn_response_text.contains(required),
            "agent-doc-turn must own hook response projection policy: {required}"
        );
    }
    for required in [
        "pub fn response_satisfies_imperative_contract",
        "const IMPERATIVE_STATUS_ONLY_SIGNALS",
        "const IMPERATIVE_META_REFUSAL_SIGNALS",
        "const IMPERATIVE_BLOCKER_SIGNALS",
        "const IMPERATIVE_EVIDENCE_LABELS",
    ] {
        assert!(
            turn_response_text.contains(required),
            "agent-doc-turn must own imperative response contract policy: {required}"
        );
    }
    let turn_lib = fs::read_to_string(manifest_dir.join("agent-doc-turn/src/lib.rs")).unwrap();
    assert!(
        turn_lib.contains("pub mod response_text;"),
        "agent-doc-turn should expose response text policy through its owning module"
    );
    assert!(
        !turn_lib.contains("pub use response_text"),
        "agent-doc-turn should not add a response_text root facade"
    );
    let write_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write.rs")).unwrap();
    assert!(
        !write_source.contains("pub fn strip_assistant_heading"),
        "orchestration write must not re-own append response heading normalization"
    );
    let hooks_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/hooks.rs")).unwrap();
    assert!(
        hooks_source.contains("agent_doc_turn::response_text::{")
            && hooks_source.contains("response_prompt_target_from_re_heading")
            && hooks_source.contains("summarize_response_for_hook"),
        "orchestration hooks should call focused response text projection directly"
    );
    for forbidden in [
        "fn extract_agent_doc_prompt_target",
        "fn summarize_agent_doc_response",
        "fn truncate_agent_doc_summary",
    ] {
        assert!(
            !hooks_source.contains(forbidden),
            "orchestration hooks must not re-own hook response projection policy: {forbidden}"
        );
    }
    let write_normalize =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/normalize.rs"))
            .unwrap();
    for forbidden in [
        "const IMPERATIVE_STATUS_ONLY_SIGNALS",
        "const IMPERATIVE_META_REFUSAL_SIGNALS",
        "const IMPERATIVE_BLOCKER_SIGNALS",
        "const IMPERATIVE_EVIDENCE_LABELS",
        "fn response_satisfies_imperative_contract",
        "fn contains_execution_evidence",
        "fn has_commandish_backticks",
        "fn has_code_path",
        "fn contains_commit_hash",
    ] {
        assert!(
            !write_normalize.contains(forbidden),
            "write::normalize must not re-own imperative response contract policy: {forbidden}"
        );
    }
    assert!(
        write_normalize
            .contains("agent_doc_turn::response_text::response_satisfies_imperative_contract"),
        "write::normalize should call focused imperative response contract policy directly"
    );
    for relative in [
        "agent-doc-orchestration/src/write/run_entry.rs",
        "agent-doc-orchestration/src/run.rs",
        "src/orchestrate/dispatch.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            source.contains("agent_doc_turn::response_text::strip_assistant_heading"),
            "{relative} should call focused append response heading normalization directly"
        );
        assert!(
            !source.contains("write::strip_assistant_heading"),
            "{relative} must not route append response heading normalization through write"
        );
    }
}

#[test]
fn test_agent_doc_turn_owns_no_change_cycle_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        manifest_dir
            .join("agent-doc-turn/src/no_change.rs")
            .exists(),
        "no-change cycle diagnostics should live in the focused turn crate"
    );

    let turn_source =
        fs::read_to_string(manifest_dir.join("agent-doc-turn/src/no_change.rs")).unwrap();
    for required in [
        "pub struct NoChangeCycleStateInput",
        "pub enum NoChangeVerdict",
        "pub fn classify_no_change_cycle_state",
        "pub const fn has_bookkeeping_without_response",
    ] {
        assert!(
            turn_source.contains(required),
            "agent-doc-turn must own no-change cycle policy: {required}"
        );
    }

    let turn_lib = fs::read_to_string(manifest_dir.join("agent-doc-turn/src/lib.rs")).unwrap();
    assert!(
        turn_lib.contains("pub mod no_change;"),
        "agent-doc-turn should expose no-change policy through its owning module"
    );
    assert!(
        !turn_lib.contains("pub use no_change"),
        "agent-doc-turn should not add a no_change root facade"
    );

    let run_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/run.rs")).unwrap();
    for forbidden in [
        "enum NoChangeVerdict",
        "fn classify_no_change_cycle_state(",
        ".starts_with(\"recursive_direct_invocation_blocked\")",
        "bookkeeping-only closeout",
        "let bookkeeping =",
    ] {
        assert!(
            !run_source.contains(forbidden),
            "orchestration run must not re-own or facade no-change cycle policy: {forbidden}"
        );
    }
    for required in [
        "use agent_doc_turn::no_change::{",
        "NoChangeCycleStateInput",
        "classify_no_change_cycle_state",
    ] {
        assert!(
            run_source.contains(required),
            "orchestration run should call focused no-change policy directly: {required}"
        );
    }
}

#[test]
fn test_agent_doc_turn_owns_turn_status_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let turn_status =
        fs::read_to_string(manifest_dir.join("agent-doc-turn/src/turn_status.rs")).unwrap();
    for required in [
        "pub const TURN_ACTIVE_PANE_TITLE",
        "pub const STALE_SUPERVISOR_PANE_MARKER",
        "pub const STALE_SUPERVISOR_MARKER",
        "pub const TURN_ACTIVE_MARKER",
        "pub const TURN_ACTIVE_TTL_SECS",
        "pub struct TurnActiveMarker",
        "pub fn turn_active_marker_is_fresh",
        "pub fn turn_active_marker_matches_pane",
        "pub fn pane_title_for_state",
        "pub fn pane_title_for_status",
    ] {
        assert!(
            turn_status.contains(required),
            "agent-doc-turn must own turn-status policy vocabulary: {required}"
        );
    }

    let turn_lib = fs::read_to_string(manifest_dir.join("agent-doc-turn/src/lib.rs")).unwrap();
    assert!(
        turn_lib.contains("pub mod turn_status;"),
        "agent-doc-turn should expose turn-status policy through its owning module"
    );
    assert!(
        !turn_lib.contains("pub use turn_status"),
        "agent-doc-turn should not add a turn_status root facade"
    );

    let orchestration =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/turn_status.rs"))
            .unwrap();
    for forbidden in [
        "pub const TURN_ACTIVE_PANE_TITLE",
        "pub const STALE_SUPERVISOR_PANE_MARKER",
        "pub const STALE_SUPERVISOR_MARKER",
        "pub const TURN_ACTIVE_MARKER",
        "pub const TURN_ACTIVE_TTL_SECS",
        "pub struct TurnActiveMarker",
        "pub fn pane_title_for_state",
        "pub fn pane_title_for_status",
        "pub use agent_doc_turn::turn_status",
    ] {
        assert!(
            !orchestration.contains(forbidden),
            "orchestration turn_status must stay an effect adapter, not a facade: {forbidden}"
        );
    }
    for required in [
        "agent_doc_turn::turn_status::{",
        "TurnActiveMarker",
        "pane_title_for_status",
        "turn_active_marker_is_fresh",
        "turn_active_marker_matches_pane",
    ] {
        assert!(
            orchestration.contains(required),
            "orchestration turn_status should call focused turn-status policy directly: {required}"
        );
    }
}

#[test]
fn test_agent_doc_turn_owns_owner_pane_recursion_diagnostics() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        manifest_dir
            .join("agent-doc-turn/src/owner_pane_recursion.rs")
            .exists(),
        "owner-pane self-invocation diagnostic policy should live in the focused turn crate"
    );

    let turn_source =
        fs::read_to_string(manifest_dir.join("agent-doc-turn/src/owner_pane_recursion.rs"))
            .unwrap();
    for required in [
        "pub struct OwnerPaneQueueHead",
        "pub const OWNER_PANE_WEDGE_THRESHOLD",
        "pub struct OwnerPaneWedgeRecord",
        "pub fn record_owner_pane_wedge_fire",
        "pub fn owner_pane_wedge_threshold_reached",
        "pub fn recursive_direct_invocation_message",
        "pub fn recursive_start_invocation_message",
        "pub fn prompt_miss_message",
        "pub fn queue_handoff_message",
        "pub fn queue_wedge_halt_message",
    ] {
        assert!(
            turn_source.contains(required),
            "agent-doc-turn must own owner-pane recursion diagnostic policy: {required}"
        );
    }

    let turn_lib = fs::read_to_string(manifest_dir.join("agent-doc-turn/src/lib.rs")).unwrap();
    assert!(
        turn_lib.contains("pub mod owner_pane_recursion;"),
        "agent-doc-turn should expose owner-pane recursion diagnostics through the owning module"
    );
    assert!(
        !turn_lib.contains("pub use owner_pane_recursion"),
        "agent-doc-turn should not add an owner_pane_recursion root facade"
    );

    let run_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/run.rs")).unwrap();
    let wedge_counter_source = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/owner_pane_wedge_counter.rs"),
    )
    .unwrap();
    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/recguard_wedge.rs")
            .exists(),
        "old recguard_wedge module path should not remain as a compatibility alias"
    );
    for forbidden in [
        "fn format_recursive_start_diagnostic",
        "fn owned_pane_prompt_miss_diagnostic",
        "fn owned_pane_queue_handoff_diagnostic",
        "fn owned_pane_queue_wedge_halt_diagnostic",
        "recursive self-owned-pane start would deadlock:",
        "owned-pane self-invocation with unresolved exchange prompt:",
        "owned-pane self-invocation with active auto-queue head:",
        "owned-pane self-invocation WEDGE:",
        "managed owner-pane supervisor will submit",
    ] {
        assert!(
            !run_source.contains(forbidden),
            "orchestration run must not re-own or facade owner-pane recursion diagnostics: {forbidden}"
        );
    }
    for forbidden in [
        "pub const WEDGE_THRESHOLD",
        "struct WedgeRecord",
        "pub fn is_wedged(",
        "pub fn threshold_reached(",
    ] {
        assert!(
            !wedge_counter_source.contains(forbidden),
            "orchestration wedge-counter adapter must not re-own extracted owner-pane wedge policy: {forbidden}"
        );
    }
    for required in [
        "use agent_doc_turn::owner_pane_recursion::{",
        "OwnerPaneQueueHead",
        "owner_pane_wedge_threshold_reached",
        "prompt_miss_message",
        "queue_handoff_message",
        "queue_wedge_halt_message",
        "recursive_direct_invocation_message",
        "recursive_start_invocation_message",
    ] {
        assert!(
            run_source.contains(required),
            "orchestration run should call focused owner-pane recursion diagnostics directly: {required}"
        );
    }
    for required in ["OwnerPaneWedgeRecord", "record_owner_pane_wedge_fire"] {
        assert!(
            wedge_counter_source.contains(required),
            "orchestration wedge-counter adapter should persist focused owner-pane wedge records directly: {required}"
        );
    }
}

#[test]
fn test_agent_doc_turn_owns_pending_capture_heuristics() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let heuristics =
        fs::read_to_string(manifest_dir.join("agent-doc-turn/src/heuristics.rs")).unwrap();
    for required in [
        "pub fn detect_uncaptured_recommendations(",
        "pub fn response_explicitly_has_no_followups(",
        "pub fn future_work_signal(",
    ] {
        assert!(
            heuristics.contains(required),
            "agent-doc-turn must own pending-capture closeout heuristic: {required}"
        );
    }

    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/prompt_contract.rs")
            .exists(),
        "orchestration must not keep a prompt_contract module that can re-own turn closeout no-follow-up policy"
    );

    let write_normalize =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/normalize.rs"))
            .unwrap();
    for forbidden in [
        "pub fn check_future_work_signals",
        "const FUTURE_WORK_SIGNALS",
        "pub(crate) const FUTURE_WORK_SIGNALS",
    ] {
        assert!(
            !write_normalize.contains(forbidden),
            "write::normalize must not re-own turn future-work phrase policy: {forbidden}"
        );
    }

    let write_run_entry =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/run_entry.rs"))
            .unwrap();
    assert!(
        write_run_entry.contains("agent_doc_turn::heuristics::future_work_signal"),
        "write run entry should call focused future-work response policy directly"
    );

    for relative in [
        "agent-doc-orchestration/src/session_check/pending_guards.rs",
        "agent-doc-orchestration/src/write/pending_checks.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            source.contains("agent_doc_turn::heuristics::response_explicitly_has_no_followups"),
            "{relative} should call focused no-follow-up heuristic directly"
        );
        assert!(
            !source.contains("crate::prompt_contract::response_explicitly_has_no_followups"),
            "{relative} must not route no-follow-up policy through prompt_contract"
        );
    }
}

#[test]
fn test_agent_doc_turn_owns_drain_stall_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-turn")),
        "agent-doc-turn must stay a first-class workspace crate"
    );
    assert!(
        manifest_dir
            .join("agent-doc-turn/src/drain_stall.rs")
            .exists(),
        "queue-stall turn policy should live in the focused turn crate"
    );

    let orchestration_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/drain_stall.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub use agent_doc_turn::drain_stall",
        "pub fn classify_stall",
        "pub struct StallFacts",
        "pub enum StallVerdict",
        "pub const QUEUE_STALL_DETECTED",
    ] {
        assert!(
            !orchestration_source.contains(forbidden_snippet),
            "orchestration must not re-export or re-own pure drain-stall policy: {forbidden_snippet}"
        );
    }

    let preflight_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/preflight/run.rs"))
            .unwrap();
    assert!(
        preflight_source.contains("use agent_doc_turn::drain_stall::{"),
        "preflight must consume focused drain-stall policy directly"
    );

    let turn_manifest = fs::read_to_string(manifest_dir.join("agent-doc-turn/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&turn_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-turn must stay pure and free of orchestration, git, editor IPC, sqlite, or tmux crates"
        );
    }
}

#[test]
fn test_agent_doc_turn_cycle_phase_has_no_cycle_state_facade() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-turn")),
        "agent-doc-turn must stay a first-class workspace crate"
    );

    let cycle_state_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/cycle_state.rs"))
            .unwrap();
    assert!(
        !cycle_state_source.contains("pub use agent_doc_turn::CyclePhase"),
        "cycle_state must not re-export CyclePhase from the focused turn crate"
    );
    assert!(
        cycle_state_source
            .contains("use agent_doc_turn::{CycleEvent, CyclePhase, CyclePhaseMachine};"),
        "cycle_state should import the focused turn lifecycle model privately"
    );
    let turn_source = fs::read_to_string(manifest_dir.join("agent-doc-turn/src/lib.rs")).unwrap();
    assert!(
        turn_source.contains("pub const fn as_str(self) -> &'static str"),
        "agent-doc-turn must own canonical cycle phase labels"
    );

    fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push(path);
            }
        }
    }

    let mut source_files = Vec::new();
    collect_rs_files(
        &manifest_dir.join("agent-doc-orchestration/src"),
        &mut source_files,
    );
    collect_rs_files(&manifest_dir.join("src"), &mut source_files);
    for path in source_files {
        let source = fs::read_to_string(&path).unwrap();
        let relative = path.strip_prefix(manifest_dir).unwrap().display();
        for forbidden_snippet in [
            "cycle_state::CyclePhase",
            "agent_doc_orchestration::cycle_state::CyclePhase",
            "use crate::cycle_state::CyclePhase",
            "fn cycle_phase_name(",
            "fn phase_name(phase: agent_doc_turn::CyclePhase)",
        ] {
            assert!(
                !source.contains(forbidden_snippet),
                "{relative} must call agent_doc_turn::CyclePhase directly, not re-own turn lifecycle phase policy: {forbidden_snippet}"
            );
        }
    }

    let orchestration_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/Cargo.toml")).unwrap();
    let orchestration: toml::Value = toml::from_str(&orchestration_manifest).unwrap();
    let orchestration_dependencies = orchestration["dependencies"].as_table().unwrap();
    assert!(
        orchestration_dependencies.contains_key("agent-doc-turn"),
        "orchestration must depend on the focused turn crate directly"
    );
    let root_dependencies = workspace["dependencies"].as_table().unwrap();
    assert!(
        root_dependencies.contains_key("agent-doc-turn"),
        "the CLI shell must depend on the focused turn crate directly"
    );
}

#[test]
fn test_agent_doc_log_time_has_no_ops_log_facade() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-log-time")),
        "agent-doc-log-time must stay a first-class workspace crate"
    );

    let ops_log_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/ops_log.rs")).unwrap();
    for forbidden_snippet in [
        "pub use agent_doc_log_time",
        "pub fn format_log_timestamp",
        "pub fn parse_log_timestamp",
    ] {
        assert!(
            !ops_log_source.contains(forbidden_snippet),
            "ops_log must not re-export or re-own log timestamp helpers: {forbidden_snippet}"
        );
    }
    assert!(
        ops_log_source.contains("agent_doc_log_time::format_log_timestamp"),
        "ops_log should call the focused log-time crate directly"
    );

    for relative in [
        "agent-doc-orchestration/src/ops_log.rs",
        "agent-doc-orchestration/src/session_accretion.rs",
        "agent-doc-orchestration/src/start.rs",
        "agent-doc-orchestration/src/startup_miss.rs",
        "agent-doc-orchestration/src/sync.rs",
        "agent-doc-orchestration/src/write.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            !source.contains("crate::ops_log::format_log_timestamp")
                && !source.contains("crate::ops_log::parse_log_timestamp"),
            "{relative} must call agent_doc_log_time helpers directly"
        );
    }

    let log_time_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-log-time/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&log_time_manifest).unwrap();
    let dependencies = parsed.get("dependencies").and_then(toml::Value::as_table);
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            dependencies.is_none_or(|dependencies| !dependencies.contains_key(forbidden)),
            "agent-doc-log-time must stay pure and free of core, orchestration, git, editor IPC, sqlite, or tmux crates"
        );
    }
}

#[test]
fn test_agent_doc_session_accretion_owns_pure_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-session-accretion")),
        "agent-doc-session-accretion must stay a first-class workspace crate"
    );

    let focused_source =
        fs::read_to_string(manifest_dir.join("agent-doc-session-accretion/src/lib.rs")).unwrap();
    for required_snippet in [
        "pub enum SessionAccretionLevel",
        "pub struct SessionAccretionReport",
        "pub struct SessionAccretionInput",
        "pub fn evaluate_session_accretion(",
        "pub fn level_label(",
        "pub fn restart_or_drain_guidance(",
        "pub fn compaction_guidance(",
        "pub fn exchange_metrics(",
        "pub fn is_restart_churn_event(",
        "pub fn recent_restart_count_from_session_log(",
        "pub fn resolve_queue_context_reset_opt_in(",
        "pub fn resolve_clear_threshold(",
        "pub fn context_reset_reason_for_recent_compaction(",
        "pub fn context_reset_reason_for_report(",
    ] {
        assert!(
            focused_source.contains(required_snippet),
            "agent-doc-session-accretion must own pure session-accretion policy: {required_snippet}"
        );
    }

    let orchestration_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/session_accretion.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub enum SessionAccretionLevel",
        "pub struct SessionAccretionReport",
        "pub struct SessionAccretionInput",
        "pub fn evaluate_session_accretion(",
        "fn evaluate_session_accretion(",
        "pub fn exchange_metrics(",
        "fn exchange_metrics(",
        "pub fn level_label(",
        "fn level_label(",
        "pub fn is_restart_churn_event(",
        "fn is_restart_churn_event(",
        "pub fn recent_restart_count_from_session_log(",
        "fn recent_restart_count_from_session_log(",
        "strip_prefix('[')",
        "split_once(']')",
        "split_once(\"] \")",
        "pub fn resolve_queue_context_reset_opt_in(",
        "fn resolve_queue_context_reset_opt_in(",
        "pub fn resolve_clear_threshold(",
        "fn resolve_clear_threshold(",
        "pub fn context_reset_reason_for_recent_compaction(",
        "fn context_reset_reason_for_recent_compaction(",
        "pub fn context_reset_reason_for_report(",
        "fn context_reset_reason_for_report(",
        "unwrap_or(DEFAULT_CLEAR_THRESHOLD)",
        "pub use agent_doc_session_accretion",
        "type SessionAccretion",
    ] {
        assert!(
            !orchestration_source.contains(forbidden_snippet),
            "orchestration must not re-own or facade pure session-accretion policy: {forbidden_snippet}"
        );
    }
    assert!(
        orchestration_source.contains("evaluate_session_accretion(session_accretion_input(")
            && orchestration_source.contains("SessionAccretionInput {"),
        "orchestration session_accretion should gather IO facts then call focused policy directly"
    );
    assert!(
        orchestration_source.contains("recent_restart_count_from_session_log(&content, now)"),
        "orchestration session_accretion should delegate session-log restart counting to the focused crate"
    );
    assert!(
        orchestration_source.contains("resolve_queue_context_reset_opt_in(")
            && orchestration_source.contains("resolve_clear_threshold(")
            && orchestration_source.contains("context_reset_reason_for_recent_compaction(")
            && orchestration_source.contains("context_reset_reason_for_report("),
        "orchestration session_accretion should call focused context-reset policy directly"
    );

    for relative in [
        "src/orchestrate.rs",
        "agent-doc-orchestration/src/preflight.rs",
        "agent-doc-orchestration/src/prompt_context.rs",
        "agent-doc-orchestration/src/run.rs",
        "agent-doc-orchestration/src/stream.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            source.contains("agent_doc_session_accretion::"),
            "{relative} should import session-accretion types from the focused crate"
        );
        assert!(
            !source.contains("agent_doc_orchestration::session_accretion::SessionAccretion")
                && !source.contains("crate::session_accretion::SessionAccretion"),
            "{relative} must not route session-accretion types through orchestration"
        );
    }

    let root_dependencies = workspace["dependencies"].as_table().unwrap();
    assert!(
        root_dependencies.contains_key("agent-doc-session-accretion"),
        "the CLI shell must depend on the focused session-accretion crate directly"
    );
    let orchestration_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/Cargo.toml")).unwrap();
    let orchestration: toml::Value = toml::from_str(&orchestration_manifest).unwrap();
    let orchestration_dependencies = orchestration["dependencies"].as_table().unwrap();
    assert!(
        orchestration_dependencies.contains_key("agent-doc-session-accretion"),
        "orchestration must depend on the focused session-accretion crate directly"
    );

    let focused_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-session-accretion/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&focused_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    assert!(dependencies.contains_key("agent-doc-element"));
    assert!(dependencies.contains_key("agent-doc-log-time"));
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-session-accretion must stay free of orchestration, git, editor IPC, sqlite, or tmux-router effects"
        );
    }
}

#[test]
fn test_agent_doc_prompt_context_owns_pure_rendering_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-prompt-context")),
        "agent-doc-prompt-context must stay a first-class workspace crate"
    );

    let focused_source =
        fs::read_to_string(manifest_dir.join("agent-doc-prompt-context/src/lib.rs")).unwrap();
    for required_snippet in [
        "pub struct BoundedResponseContext",
        "pub struct DocumentSectionContext",
        "pub fn document_section_needs_response_toc(",
        "pub fn render_document_section(",
        "pub fn format_active_format_requirements(",
        "pub fn render_full_document_section(",
        "pub fn render_remote_host_scope(",
        "pub fn render_bounded_response_context(",
        "fn collect_active_format_requirements(",
        "fn render_prompt_targets(",
        "fn extract_session_summary(",
        "fn render_backlog_head(",
        "fn render_recent_exchange_turns(",
        "fn collect_recent_exchange_turn_sections(",
        "fn render_available_components(",
        "fn should_render_bounded_document_section(",
    ] {
        assert!(
            focused_source.contains(required_snippet),
            "agent-doc-prompt-context must own pure prompt-context rendering policy: {required_snippet}"
        );
    }

    let orchestration_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/prompt_context.rs"))
            .unwrap();
    for forbidden_snippet in [
        "fn render_prompt_targets(",
        "fn extract_session_summary(",
        "fn render_backlog_head(",
        "fn render_recent_exchange_turns(",
        "fn collect_recent_exchange_turn_sections(",
        "fn render_available_components(",
        "render_bounded_response_context(BoundedResponseContext",
        "element::parse(doc)",
        "fn render_remote_host_scope(",
        "Globally approved SSH commands",
        "pub use agent_doc_prompt_context",
    ] {
        assert!(
            !orchestration_source.contains(forbidden_snippet),
            "orchestration must not re-own or facade pure prompt-context rendering policy: {forbidden_snippet}"
        );
    }
    assert!(
        orchestration_source.contains("agent_doc_prompt_context::{")
            && orchestration_source.contains("render_document_section(DocumentSectionContext")
            && orchestration_source.contains("document_section_needs_response_toc(")
            && orchestration_source.contains("render_remote_host_scope(&declared_targets)")
            && orchestration_source.contains("frontmatter_io::parse_for_file_with_context")
            && orchestration_source.contains("agent_doc_response_toc_io::render_prompt_toc"),
        "orchestration prompt_context should gather project context then call focused rendering policy directly"
    );

    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/prompt_contract.rs")
            .exists(),
        "orchestration must not keep prompt_contract as a facade for prompt-context rendering policy"
    );

    for (path, required_snippet, forbidden_snippets) in [
        (
            "src/orchestrate.rs",
            "agent_doc_prompt_context::format_active_format_requirements",
            [
                "agent_doc_orchestration::prompt_contract::format_active_format_requirements",
                "prompt_contract::format_active_format_requirements",
                "crate::prompt_contract::format_active_format_requirements",
            ],
        ),
        (
            "agent-doc-orchestration/src/run.rs",
            "agent_doc_prompt_context::format_active_format_requirements",
            [
                "agent_doc_orchestration::prompt_contract::format_active_format_requirements",
                "prompt_contract::format_active_format_requirements",
                "crate::prompt_contract::format_active_format_requirements",
            ],
        ),
        (
            "agent-doc-orchestration/src/stream.rs",
            "agent_doc_prompt_context::format_active_format_requirements",
            [
                "agent_doc_orchestration::prompt_contract::format_active_format_requirements",
                "prompt_contract::format_active_format_requirements",
                "crate::prompt_contract::format_active_format_requirements",
            ],
        ),
    ] {
        let source = fs::read_to_string(manifest_dir.join(path)).unwrap();
        assert!(
            source.contains(required_snippet),
            "{path} should call focused format requirement rendering policy directly"
        );
        for forbidden_snippet in forbidden_snippets {
            assert!(
                !source.contains(forbidden_snippet),
                "{path} must not call orchestration prompt_contract for focused format requirement rendering policy"
            );
        }
    }

    let orchestration_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/Cargo.toml")).unwrap();
    let orchestration: toml::Value = toml::from_str(&orchestration_manifest).unwrap();
    let orchestration_dependencies = orchestration["dependencies"].as_table().unwrap();
    assert!(
        orchestration_dependencies.contains_key("agent-doc-prompt-context"),
        "orchestration must depend on the focused prompt-context crate directly"
    );

    let focused_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-prompt-context/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&focused_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    for required in [
        "agent-doc-element",
        "agent-doc-element-backlog",
        "agent-doc-session-accretion",
    ] {
        assert!(
            dependencies.contains_key(required),
            "agent-doc-prompt-context should depend on {required} for pure rendering inputs"
        );
    }
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "agent-doc-frontmatter",
        "agent-doc-fs",
        "agent-doc-workflow",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-prompt-context must stay free of orchestration and project/effect dependencies"
        );
    }
}

#[test]
fn test_agent_doc_response_toc_owns_live_toc_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-response-toc")),
        "agent-doc-response-toc must stay a first-class workspace crate"
    );
    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-response-toc-io")),
        "agent-doc-response-toc-io must stay a first-class workspace crate for file/archive adapters"
    );

    let focused_source =
        fs::read_to_string(manifest_dir.join("agent-doc-response-toc/src/lib.rs")).unwrap();
    for required_snippet in [
        "pub struct LiveTocEntry",
        "pub struct LiveSection",
        "pub struct PromptFilters",
        "pub fn live_toc_entries(",
        "pub fn live_sections(",
        "pub fn collect_live_sections(",
        "pub fn live_section_window(",
        "pub fn extract_backlog_ids(",
        "pub fn preview_text(",
        "pub fn normalize_text(",
        "pub fn normalize_backlog_id(",
    ] {
        assert!(
            focused_source.contains(required_snippet),
            "agent-doc-response-toc must own live response TOC policy: {required_snippet}"
        );
    }

    let io_source =
        fs::read_to_string(manifest_dir.join("agent-doc-response-toc-io/src/lib.rs")).unwrap();
    for required_snippet in [
        "pub fn run_toc(",
        "pub fn run_fetch(",
        "pub fn render_prompt_toc(",
        "fn build_toc_entries(",
        "fn archive_entries(",
        "fn fetch_sections(",
        "agent_doc_response_toc::live_toc_entries",
        "agent_doc_sqlite::archive_index",
    ] {
        assert!(
            io_source.contains(required_snippet),
            "agent-doc-response-toc-io must own response TOC file/archive adapter policy: {required_snippet}"
        );
    }

    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/response_toc.rs")
            .exists(),
        "orchestration must not keep response_toc as an adapter module or facade"
    );

    let orchestration_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/lib.rs")).unwrap();
    assert!(
        !orchestration_lib.contains("pub mod response_toc"),
        "orchestration must not expose response_toc as a module facade"
    );

    let prompt_context =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/prompt_context.rs"))
            .unwrap();
    assert!(
        prompt_context.contains("agent_doc_response_toc_io::render_prompt_toc")
            && !prompt_context.contains("crate::response_toc::render_prompt_toc"),
        "orchestration prompt_context should call response TOC IO directly"
    );

    let main_source = fs::read_to_string(manifest_dir.join("src/main.rs")).unwrap();
    assert!(
        main_source.contains("agent_doc_response_toc_io::run_toc")
            && main_source.contains("agent_doc_response_toc_io::run_fetch")
            && !main_source.contains("agent_doc_orchestration::response_toc"),
        "CLI response TOC commands should call response TOC IO directly"
    );

    let orchestration_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/Cargo.toml")).unwrap();
    let orchestration: toml::Value = toml::from_str(&orchestration_manifest).unwrap();
    let orchestration_dependencies = orchestration["dependencies"].as_table().unwrap();
    assert!(
        orchestration_dependencies.contains_key("agent-doc-response-toc-io"),
        "orchestration must depend on the focused response-TOC IO crate directly"
    );
    assert!(
        !orchestration_dependencies.contains_key("agent-doc-response-toc"),
        "orchestration should depend on response TOC through the IO adapter, not the pure live-TOC crate"
    );

    let focused_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-response-toc/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&focused_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    assert!(dependencies.contains_key("agent-doc-element"));
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "agent-doc-sqlite",
        "agent-doc-fs",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-response-toc must stay free of orchestration and archive/effect dependencies"
        );
    }

    let io_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-response-toc-io/Cargo.toml")).unwrap();
    let io_parsed: toml::Value = toml::from_str(&io_manifest).unwrap();
    let io_dependencies = io_parsed["dependencies"].as_table().unwrap();
    for required in ["agent-doc-response-toc", "agent-doc-sqlite", "serde_json"] {
        assert!(
            io_dependencies.contains_key(required),
            "agent-doc-response-toc-io should depend on {required} for response TOC adapters"
        );
    }
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "tmux-router",
    ] {
        assert!(
            !io_dependencies.contains_key(forbidden),
            "agent-doc-response-toc-io must stay free of core, orchestration, git, editor IPC, notify, and tmux-router dependencies"
        );
    }
}

#[test]
fn test_agent_doc_prompt_contract_owns_prompt_contract_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-prompt-contract")),
        "agent-doc-prompt-contract must stay a first-class workspace crate"
    );

    let focused_source =
        fs::read_to_string(manifest_dir.join("agent-doc-prompt-contract/src/lib.rs")).unwrap();
    let focused_harness_prompt_source =
        fs::read_to_string(manifest_dir.join("agent-doc-prompt-contract/src/harness_prompt.rs"))
            .unwrap();
    for required_snippet in [
        "pub fn prompt_requests_plan_work(",
        "pub fn prompt_requests_backlog_work(",
        "pub fn requested_prompt_presets(",
        "pub struct PromptPresetRequestResolution",
        "pub fn resolve_prompt_preset_requests(",
        "pub fn explicit_backlog_targets(",
        "pub fn required_explicit_backlog_item_count(",
        "pub fn required_plan_reference_count(",
        "pub fn ordered_issue_units_for_agent_doc_bug(",
        "pub fn collect_added_diff_lines(",
        "fn effective_prompt_texts(",
        "fn referenced_presets_in_text(",
        "fn issue_units_in_text(",
        "agent_doc_fs::referenced_markdown_path_checked",
        "pub mod harness_prompt;",
    ] {
        assert!(
            focused_source.contains(required_snippet),
            "agent-doc-prompt-contract must own prompt contract policy directly: {required_snippet}"
        );
    }
    for required_snippet in [
        "pub enum HarnessInvocationKind",
        "pub struct ParsedHarnessInvocation",
        "pub fn prompt_body_from_text(",
        "pub fn synthetic_diff_from_body(",
        "pub fn agent_doc_invocation_file_from_text(",
        "pub fn parse_agent_doc_invocation(",
    ] {
        assert!(
            focused_harness_prompt_source.contains(required_snippet),
            "agent-doc-prompt-contract must own harness prompt parsing policy directly: {required_snippet}"
        );
    }

    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/prompt_contract.rs")
            .exists(),
        "orchestration must not keep a prompt_contract module or facade"
    );

    let orchestration_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/lib.rs")).unwrap();
    assert!(
        !orchestration_lib.contains("pub mod prompt_contract"),
        "orchestration must not expose prompt_contract as a module facade"
    );

    for (path, required_snippets, forbidden_snippets) in [
        (
            "src/plan.rs",
            vec![
                "agent_doc_prompt_contract::collect_added_diff_lines",
                "agent_doc_prompt_contract::prompt_requests_backlog_work",
                "agent_doc_prompt_contract::ordered_issue_units_for_agent_doc_bug",
                "agent_doc_prompt_contract::explicit_backlog_targets",
            ],
            vec![
                "agent_doc_orchestration::prompt_contract",
                "crate::prompt_contract",
            ],
        ),
        (
            "agent-doc-orchestration/src/preflight.rs",
            vec!["agent_doc_prompt_contract::requested_prompt_presets"],
            vec!["crate::prompt_contract"],
        ),
        (
            "agent-doc-orchestration/src/preflight/run.rs",
            vec![
                "agent_doc_prompt_contract::collect_added_diff_lines",
                "agent_doc_prompt_contract::resolve_prompt_preset_requests",
                "agent_doc_prompt_contract::prompt_requests_backlog_work",
                "agent_doc_prompt_contract::explicit_backlog_targets",
                "agent_doc_prompt_contract::required_explicit_backlog_item_count",
                "agent_doc_prompt_contract::required_plan_reference_count",
            ],
            vec![
                "crate::prompt_contract",
                "diff::detect_prompt_preset_requests",
                "frontmatter::resolve_prompt_preset_key(&frontmatter_prompt_presets",
                "let missing_prompt_presets =",
                "prompt_presets_requested = prompt_presets_requested",
            ],
        ),
        (
            "agent-doc-orchestration/src/harness_prompt.rs",
            vec![
                "agent_doc_prompt_contract::harness_prompt::prompt_body_from_text",
                "agent_doc_prompt_contract::harness_prompt::synthetic_diff_from_body",
            ],
            vec![
                "enum InvocationKind",
                "struct ParsedInvocation",
                "fn prompt_body_from_text(",
                "fn synthetic_diff_from_body(",
                "fn parse_agent_doc_invocation(",
                "fn same_file(",
            ],
        ),
        (
            "agent-doc-orchestration/src/codex_hook.rs",
            vec!["agent_doc_prompt_contract::harness_prompt::agent_doc_invocation_file_from_text"],
            vec![
                "fn parse_agent_doc_invocation_line(",
                "fn agent_doc_invocation_file_from_text(",
                "pub fn agent_doc_invocation_file_from_text(",
            ],
        ),
    ] {
        let source = fs::read_to_string(manifest_dir.join(path)).unwrap();
        for required_snippet in required_snippets {
            assert!(
                source.contains(required_snippet),
                "{path} should call prompt contract policy through agent-doc-prompt-contract directly: {required_snippet}"
            );
        }
        for forbidden_snippet in forbidden_snippets {
            assert!(
                !source.contains(forbidden_snippet),
                "{path} must not route prompt contract policy through orchestration"
            );
        }
    }

    let orchestration_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/Cargo.toml")).unwrap();
    let orchestration: toml::Value = toml::from_str(&orchestration_manifest).unwrap();
    let orchestration_dependencies = orchestration["dependencies"].as_table().unwrap();
    assert!(
        orchestration_dependencies.contains_key("agent-doc-prompt-contract"),
        "orchestration must depend on the focused prompt-contract crate directly"
    );

    let root_manifest = toml::from_str::<toml::Value>(&workspace_manifest).unwrap();
    let root_dependencies = root_manifest["dependencies"].as_table().unwrap();
    assert!(
        root_dependencies.contains_key("agent-doc-prompt-contract"),
        "the CLI crate must depend on the focused prompt-contract crate directly"
    );

    let focused_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-prompt-contract/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&focused_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    for required in ["agent-doc-diff", "agent-doc-frontmatter", "agent-doc-fs"] {
        assert!(
            dependencies.contains_key(required),
            "agent-doc-prompt-contract should depend on {required} for prompt contract inputs"
        );
    }
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-prompt-contract must stay free of core, orchestration, git, editor IPC, sqlite, or tmux-router effects"
        );
    }
}

#[test]
fn test_agent_doc_fs_owns_markdown_reference_resolution() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fs_source = fs::read_to_string(manifest_dir.join("agent-doc-fs/src/lib.rs")).unwrap();
    for required_snippet in [
        "pub fn referenced_markdown_path(",
        "pub fn referenced_markdown_path_checked(",
        "fn strip_redundant_project_prefix(",
        "fn project_roots_for(",
        "ambiguous markdown reference",
        "project-prefixed markdown reference",
    ] {
        assert!(
            fs_source.contains(required_snippet),
            "agent-doc-fs must own markdown reference resolution policy: {required_snippet}"
        );
    }

    let security_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/security.rs")).unwrap();
    for forbidden_snippet in [
        "pub fn referenced_markdown_path(",
        "pub fn referenced_markdown_path_checked(",
        "fn strip_redundant_project_prefix(",
        "fn project_roots_for(",
        "pub use agent_doc_fs",
    ] {
        assert!(
            !security_source.contains(forbidden_snippet),
            "orchestration security must not re-own or facade markdown reference resolution: {forbidden_snippet}"
        );
    }

    for (path, required_snippet, forbidden_snippet) in [
        (
            "src/plan.rs",
            "agent_doc_fs::referenced_markdown_path",
            "security::referenced_markdown_path",
        ),
        (
            "agent-doc-project-config-io/src/lib.rs",
            "agent_doc_fs::referenced_markdown_path_checked",
            "crate::security::referenced_markdown_path_checked",
        ),
        (
            "agent-doc-orchestration/src/write/pending_checks.rs",
            "agent_doc_fs::referenced_markdown_path",
            "crate::security::referenced_markdown_path",
        ),
    ] {
        let source = fs::read_to_string(manifest_dir.join(path)).unwrap();
        assert!(
            source.contains(required_snippet),
            "{path} should call markdown reference resolution through agent-doc-fs directly"
        );
        assert!(
            !source.contains(forbidden_snippet),
            "{path} must not route markdown reference resolution through orchestration security"
        );
    }
}

#[test]
fn test_agent_doc_lease_is_freshness_boundary() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-lease")),
        "agent-doc-lease must stay a first-class workspace crate"
    );

    let package_version = workspace["package"]["version"].as_str();
    for relative_manifest in [
        "agent-doc-orchestration/Cargo.toml",
        "agent-doc-plugin-owner/Cargo.toml",
        "agent-doc-queue/Cargo.toml",
        "agent-doc-supervisor/Cargo.toml",
    ] {
        let manifest = fs::read_to_string(manifest_dir.join(relative_manifest)).unwrap();
        let parsed: toml::Value = toml::from_str(&manifest).unwrap();
        let dependencies = parsed["dependencies"].as_table().unwrap();
        let dependency = dependencies["agent-doc-lease"].as_table().unwrap();
        assert_eq!(
            dependency.get("version").and_then(toml::Value::as_str),
            package_version,
            "{relative_manifest} should depend on the versioned lease crate"
        );
    }

    let lease_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-lease/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&lease_manifest).unwrap();
    let dependencies = parsed.get("dependencies").and_then(toml::Value::as_table);
    assert!(
        dependencies.is_none_or(|dependencies| dependencies.is_empty()),
        "agent-doc-lease must stay pure and dependency-free"
    );

    for (relative, forbidden) in [
        (
            "agent-doc-queue/src/drain_owner.rs",
            "pub fn drain_owner_lease_is_fresh(",
        ),
        (
            "agent-doc-plugin-owner/src/lib.rs",
            "pub fn plugin_owner_lease_is_fresh(",
        ),
        (
            "agent-doc-queue/src/queue_edit_owner.rs",
            "pub fn queue_edit_owner_lease_is_fresh(",
        ),
        (
            "agent-doc-supervisor/src/recycle_yield.rs",
            "pub fn recycle_yield_is_fresh(",
        ),
        (
            "agent-doc-supervisor/src/recycle_inflight.rs",
            "pub fn recycle_inflight_is_fresh(",
        ),
        (
            "agent-doc-supervisor/src/route_submit_inflight.rs",
            "pub fn route_submit_lease_is_fresh(",
        ),
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            !source.contains(forbidden) && !source.contains("saturating_sub("),
            "{relative} must not re-own TTL freshness policy: {forbidden}"
        );
        assert!(
            source.contains("agent_doc_lease::timestamp_is_fresh"),
            "{relative} should call the focused lease crate directly"
        );
    }
}

#[test]
fn test_agent_doc_plugin_owner_owns_editor_lease_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();
    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-plugin-owner")),
        "agent-doc-plugin-owner must stay a first-class workspace crate"
    );

    let package_version = workspace["package"]["version"].as_str();
    for relative_manifest in ["Cargo.toml", "agent-doc-orchestration/Cargo.toml"] {
        let manifest = fs::read_to_string(manifest_dir.join(relative_manifest)).unwrap();
        let parsed: toml::Value = toml::from_str(&manifest).unwrap();
        let dependencies = parsed["dependencies"].as_table().unwrap();
        let dependency = dependencies["agent-doc-plugin-owner"].as_table().unwrap();
        assert_eq!(
            dependency.get("version").and_then(toml::Value::as_str),
            package_version,
            "{relative_manifest} should depend on the versioned plugin-owner crate"
        );
    }

    let focused =
        fs::read_to_string(manifest_dir.join("agent-doc-plugin-owner/src/lib.rs")).unwrap();
    for required in [
        "pub struct PluginOwnerLease",
        "pub fn plugin_owner_ttl(",
        "pub fn read_plugin_owner_lease(",
        "pub fn plugin_owner_pid_is_live(",
        "pub fn ownership_liveness_from_lease(",
        "pub fn ownership_liveness_for_file(",
        "pub fn disk_write_permitted_for_file(",
        "pub fn live_editor_endpoint_attached(",
        "pub fn live_plugin_owner_consumer_id(",
        "pub fn live_plugin_owner_consumer_id_for_lease(",
        "pub fn editor_endpoint_attached_for_lease(",
        "pub fn try_acquire_plugin_owner(",
        "pub fn release_plugin_owner(",
        "agent_doc_lease::timestamp_is_fresh",
        "agent_doc_merge::ownership",
    ] {
        assert!(
            focused.contains(required),
            "agent-doc-plugin-owner must own editor plugin-owner lease policy: {required}"
        );
    }

    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/plugin_owner.rs")
            .exists(),
        "orchestration must not keep a plugin_owner module or facade"
    );
    let orchestration_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/lib.rs")).unwrap();
    assert!(
        !orchestration_lib.contains("pub mod plugin_owner;")
            && !orchestration_lib.contains("pub use agent_doc_plugin_owner"),
        "orchestration must not expose a plugin-owner facade"
    );
}

#[test]
fn test_agent_doc_supervisor_owns_recycle_marker_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let supervisor_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-supervisor/src/lib.rs")).unwrap();
    assert!(supervisor_lib.contains("pub mod recycle_yield;"));
    assert!(supervisor_lib.contains("pub mod recycle_inflight;"));

    let supervisor_yield =
        fs::read_to_string(manifest_dir.join("agent-doc-supervisor/src/recycle_yield.rs")).unwrap();
    for required in [
        "pub const RECYCLE_YIELD_DIR",
        "pub const DEFAULT_RECYCLE_YIELD_TTL_SECS",
        "pub const RECYCLE_YIELD_TTL_SECS_ENV",
        "pub const RECYCLE_YIELD_STALE_BINARY",
        "pub const RECYCLE_YIELD_STATE_FLUSH",
        "pub struct RecycleYieldRequest",
        "pub fn recycle_yield_request(",
        "pub fn recycle_yield_ttl(",
        "pub fn recycle_yield_request_is_fresh(",
        "agent_doc_lease::timestamp_is_fresh",
    ] {
        assert!(
            supervisor_yield.contains(required),
            "agent-doc-supervisor must own recycle-yield marker policy: {required}"
        );
    }

    let supervisor_inflight =
        fs::read_to_string(manifest_dir.join("agent-doc-supervisor/src/recycle_inflight.rs"))
            .unwrap();
    for required in [
        "pub const RECYCLE_INFLIGHT_DIR",
        "pub const RECYCLE_INFLIGHT_FILE",
        "pub const DEFAULT_RECYCLE_INFLIGHT_TTL_SECS",
        "pub const RECYCLE_INFLIGHT_TTL_SECS_ENV",
        "pub const RECYCLE_INFLIGHT_AUTO_INSTALL",
        "pub const RECYCLE_INFLIGHT_RESTART",
        "pub const RECYCLE_SETTLE_WAIT",
        "pub const RECYCLE_SETTLE_POLL",
        "pub struct RecycleInflightMarker",
        "pub fn recycle_inflight_marker(",
        "pub fn recycle_inflight_ttl(",
        "pub fn recycle_inflight_marker_is_fresh(",
        "agent_doc_lease::timestamp_is_fresh",
    ] {
        assert!(
            supervisor_inflight.contains(required),
            "agent-doc-supervisor must own recycle-inflight marker policy: {required}"
        );
    }

    for (relative, forbidden) in [
        (
            "agent-doc-orchestration/src/recycle_yield.rs",
            vec![
                "pub const RECYCLE_YIELD_",
                "const DEFAULT_RECYCLE_YIELD",
                "pub struct RecycleYieldRequest",
                "pub fn recycle_yield_ttl(",
                "agent_doc_lease::timestamp_is_fresh",
                "pub use agent_doc_supervisor::recycle_yield",
            ],
        ),
        (
            "agent-doc-orchestration/src/recycle_inflight.rs",
            vec![
                "pub const RECYCLE_INFLIGHT_",
                "const DEFAULT_RECYCLE_INFLIGHT",
                "pub const RECYCLE_SETTLE_",
                "const RECYCLE_SETTLE_",
                "pub struct RecycleInflightMarker",
                "pub fn recycle_inflight_ttl(",
                "agent_doc_lease::timestamp_is_fresh",
                "pub use agent_doc_supervisor::recycle_inflight",
            ],
        ),
        (
            "agent-doc-orchestration/src/route/dispatch_only.rs",
            vec!["const RECYCLE_SETTLE_WAIT", "const RECYCLE_SETTLE_POLL"],
        ),
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        for forbidden in forbidden {
            assert!(
                !source.contains(forbidden),
                "{relative} must not re-own or facade supervisor recycle marker policy: {forbidden}"
            );
        }
    }

    let recycle_yield =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/recycle_yield.rs"))
            .unwrap();
    assert!(
        recycle_yield.contains("use agent_doc_supervisor::recycle_yield::{")
            && recycle_yield.contains("recycle_yield_request_is_fresh"),
        "recycle_yield adapter should import focused supervisor marker policy directly"
    );
    let recycle_inflight =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/recycle_inflight.rs"))
            .unwrap();
    assert!(
        recycle_inflight.contains("use agent_doc_supervisor::recycle_inflight::{")
            && recycle_inflight.contains("recycle_inflight_marker_is_fresh"),
        "recycle_inflight adapter should import focused supervisor marker policy directly"
    );
    for (relative, required) in [
        (
            "agent-doc-orchestration/src/start/idle_watch.rs",
            "agent_doc_supervisor::recycle_yield::RECYCLE_YIELD_STALE_BINARY",
        ),
        (
            "agent-doc-orchestration/src/start/idle_watch.rs",
            "agent_doc_supervisor::recycle_inflight::RECYCLE_INFLIGHT_AUTO_INSTALL",
        ),
        (
            "agent-doc-orchestration/src/route/dispatch_only.rs",
            "agent_doc_supervisor::recycle_inflight::RECYCLE_SETTLE_WAIT",
        ),
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            source.contains(required),
            "{relative} should call focused supervisor recycle marker policy directly: {required}"
        );
    }
}

#[test]
fn test_agent_doc_supervisor_owns_route_submit_inflight_marker_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let supervisor_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-supervisor/src/lib.rs")).unwrap();
    assert!(supervisor_lib.contains("pub mod route_submit_inflight;"));

    let supervisor_route_submit =
        fs::read_to_string(manifest_dir.join("agent-doc-supervisor/src/route_submit_inflight.rs"))
            .unwrap();
    for required in [
        "pub const ROUTE_IN_FLIGHT_DIR",
        "pub const ROUTE_IN_FLIGHT_TTL_SECS",
        "pub const ROUTE_READY_PROBE_TTL_SECS",
        "pub const ROUTE_BLOCKED_DIR",
        "pub const ROUTE_BLOCKED_TTL_SECS",
        "pub const ROUTE_DISPATCH_SUBMIT_REASON",
        "pub const ROUTE_DISPATCH_ONLY_READY_PROBE_REASON",
        "pub struct RouteSubmitInFlight",
        "pub struct RouteSubmitBlocked",
        "pub fn route_submit_inflight_marker(",
        "pub fn route_submit_blocked_marker(",
        "pub fn route_submit_ttl_secs_for_reason(",
        "pub fn route_submit_inflight_marker_is_fresh(",
        "pub fn route_submit_blocked_marker_is_fresh(",
        "agent_doc_lease::timestamp_is_fresh",
    ] {
        assert!(
            supervisor_route_submit.contains(required),
            "agent-doc-supervisor must own route-submit marker policy: {required}"
        );
    }

    let route_in_flight =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/route_in_flight.rs"))
            .unwrap();
    for forbidden in [
        "const ROUTE_IN_FLIGHT_",
        "const ROUTE_READY_PROBE_",
        "const ROUTE_BLOCKED_",
        "pub struct RouteSubmitInFlight {",
        "pub struct RouteSubmitBlocked {",
        "fn route_submit_ttl_secs_for_reason(",
        "pub fn route_submit_ttl_secs_for_reason(",
        "agent_doc_lease::timestamp_is_fresh",
        "pub use agent_doc_supervisor::route_submit_inflight",
    ] {
        assert!(
            !route_in_flight.contains(forbidden),
            "route_in_flight must stay a marker IO adapter, not re-own or facade route-submit marker policy: {forbidden}"
        );
    }
    assert!(
        route_in_flight.contains("use agent_doc_supervisor::route_submit_inflight::{")
            && route_in_flight.contains("route_submit_inflight_marker_is_fresh")
            && route_in_flight.contains("route_submit_blocked_marker_is_fresh"),
        "route_in_flight adapter should import focused supervisor marker policy directly"
    );
}

#[test]
fn test_agent_doc_queue_owns_drain_owner_lease_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    let queue_lib = fs::read_to_string(manifest_dir.join("agent-doc-queue/src/lib.rs")).unwrap();
    assert!(
        queue_lib.contains("pub mod drain_owner;"),
        "agent-doc-queue should expose drain-owner lease policy"
    );

    let drain_owner_source =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/drain_owner.rs")).unwrap();
    for required in [
        "pub const DRAIN_OWNER_CLAUDE_LOOP",
        "pub struct DrainOwnerLease",
        "pub fn drain_owner_ttl(",
        "pub fn refresh_drain_owner_lease(",
        "pub fn read_drain_owner_lease(",
        "pub fn fresh_drain_owner_lease(",
        "pub fn fresh_loop_drain_owner_lease(",
        "pub fn clear_drain_owner_lease(",
    ] {
        assert!(
            drain_owner_source.contains(required),
            "agent-doc-queue must own drain-owner lease policy: {required}"
        );
    }

    let orchestration_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/lib.rs")).unwrap();
    assert!(
        !orchestration_lib.contains("pub mod drain_owner;"),
        "agent-doc-orchestration must not re-export drain-owner lease policy"
    );
    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/drain_owner.rs")
            .exists(),
        "agent-doc-orchestration must not keep a drain_owner compatibility module"
    );

    for relative_path in [
        "src/main.rs",
        "src/sim_world.rs",
        "agent-doc-orchestration/src/start.rs",
        "agent-doc-orchestration/src/start/idle_watch.rs",
        "agent-doc-orchestration/src/preflight/run.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative_path)).unwrap();
        assert!(
            source.contains("agent_doc_queue::drain_owner::"),
            "{relative_path} should call drain-owner lease policy through agent-doc-queue directly"
        );
        assert!(
            !source.contains("agent_doc_orchestration::drain_owner")
                && !source.contains("crate::drain_owner"),
            "{relative_path} must not call drain-owner lease policy through orchestration"
        );
    }

    let queue_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&queue_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    for forbidden_dependency in [
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden_dependency),
            "agent-doc-queue drain-owner lease policy must stay free of orchestration and route effects: {forbidden_dependency}"
        );
    }
}

#[test]
fn test_agent_doc_queue_owns_context_clear_in_flight_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    let queue_source =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue.rs")).unwrap();
    for required in [
        "pub const CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS",
        "pub struct IdleQueueContextClearInFlightSettleFacts",
        "pub struct IdleQueueContextClearInFlightSettle",
        "pub fn context_clear_in_flight_marker_active(",
        "pub fn idle_queue_context_clear_in_flight_settle_ticks(",
    ] {
        assert!(
            queue_source.contains(required),
            "agent-doc-queue must own context-clear in-flight policy: {required}"
        );
    }

    let context_clear_adapter = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/context_clear_in_flight.rs"),
    )
    .unwrap();
    for forbidden in [
        "const CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS",
        "saturating_sub(marker.written_at)",
    ] {
        assert!(
            !context_clear_adapter.contains(forbidden),
            "context_clear_in_flight.rs must stay marker storage, not re-own stale-marker policy: {forbidden}"
        );
    }
    assert!(
        context_clear_adapter.contains("agent_doc_queue::queue::{")
            && context_clear_adapter.contains("context_clear_in_flight_marker_active"),
        "context_clear_in_flight.rs should call queue-owned marker policy directly"
    );

    let idle_watch =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/start/idle_watch.rs"))
            .unwrap();
    for forbidden in [
        "clear_settle_idle_ticks = clear_settle_idle_ticks.saturating_add",
        "let clear_settled_now =",
    ] {
        assert!(
            !idle_watch.contains(forbidden),
            "idle_watch.rs must not re-own context-clear settle tick policy: {forbidden}"
        );
    }
    assert!(
        idle_watch.contains("idle_queue_context_clear_in_flight_settle_ticks")
            && idle_watch.contains("IdleQueueContextClearInFlightSettleFacts"),
        "idle_watch.rs should call queue-owned context-clear settle policy directly"
    );
}

#[test]
fn test_agent_doc_work_graph_is_source_agnostic_boundary() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-work-graph")),
        "agent-doc-work-graph must stay a first-class workspace crate"
    );
    assert!(
        manifest_dir
            .join("agent-doc-work-graph/src/lib.rs")
            .exists(),
        "work-graph analysis should live in the focused crate"
    );
    let work_graph_source =
        fs::read_to_string(manifest_dir.join("agent-doc-work-graph/src/lib.rs")).unwrap();
    for required in [
        "pub mod schedule;",
        "pub enum AutoDagScheduleDecision",
        "pub enum BatchProgressDecision",
        "pub fn classify_batch_progress",
        "pub const fn as_str",
    ] {
        assert!(
            work_graph_source.contains(required),
            "agent-doc-work-graph must own Auto-DAG scheduling policy: {required}"
        );
    }
    let schedule_source =
        fs::read_to_string(manifest_dir.join("agent-doc-work-graph/src/schedule.rs")).unwrap();
    for required in [
        "pub struct AutoDagSchedule",
        "pub enum AutoDagNodeState",
        "pub fn build_schedule",
        "pub fn schedule_seed",
        "pub fn update_schedule_node_state",
        "pub fn classify_session_review_log",
        "pub fn guard_blocker",
        "pub struct AutoDagGraphEvidence",
        "pub struct AutoDagGraphTargetEvidence",
        "pub fn validate_schedule_graph_evidence",
        "pub fn target_evidence_for_schedule",
    ] {
        assert!(
            schedule_source.contains(required),
            "agent-doc-work-graph must own the Auto-DAG schedule kernel: {required}"
        );
    }
    let root_auto_dag_source = fs::read_to_string(manifest_dir.join("src/auto_dag.rs")).unwrap();
    for forbidden in [
        "pub(crate) struct AutoDagSchedule",
        "pub(crate) enum AutoDagNodeState",
        "pub(crate) struct SessionReviewGuardReport",
        "pub(crate) fn classify_session_review_log",
        "fn parse_tasks(",
        "fn mark_ready_nodes(",
        "fn validate_graph_evidence(",
        "fn target_evidence_from_graph(",
        "pub(crate) fn guard_blocker",
    ] {
        assert!(
            !root_auto_dag_source.contains(forbidden),
            "root auto_dag adapter must not re-own focused schedule behavior: {forbidden}"
        );
    }
    assert!(
        root_auto_dag_source.contains("agent_doc_work_graph::schedule::{")
            && root_auto_dag_source.contains("build_schedule as build_auto_dag_schedule")
            && root_auto_dag_source.contains("update_schedule_node_state")
            && root_auto_dag_source.contains("validate_schedule_graph_evidence")
            && root_auto_dag_source.contains("target_evidence_for_schedule"),
        "root auto_dag adapter should call the focused schedule crate directly"
    );
    let orchestration_batch = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/flow/orchestration_batch.rs"),
    )
    .unwrap();
    assert!(
        !orchestration_batch.contains("pub enum AutoDagScheduleDecision"),
        "orchestration batch flow must not re-own Auto-DAG scheduling policy"
    );
    assert!(
        !orchestration_batch.contains("pub enum BatchProgressDecision")
            && !orchestration_batch.contains("pub fn classify_batch_progress"),
        "orchestration batch flow must not re-own batch progress policy"
    );
    assert!(
        orchestration_batch.contains("agent_doc_work_graph::AutoDagScheduleDecision"),
        "orchestration batch flow should call the focused Auto-DAG scheduling policy directly"
    );
    assert!(
        orchestration_batch.contains("agent_doc_work_graph::classify_batch_progress")
            && orchestration_batch.contains("use agent_doc_work_graph::BatchProgressDecision;"),
        "orchestration batch flow should call focused batch progress policy directly"
    );
    assert!(
        !manifest_dir
            .join("agent-doc-document/src/auto_dag.rs")
            .exists(),
        "Auto-DAG graph policy should not live under the markdown document projection crate"
    );

    let root_dependencies = workspace["dependencies"].as_table().unwrap();
    assert!(
        root_dependencies.contains_key("agent-doc-work-graph"),
        "the CLI should call the focused work-graph crate directly"
    );

    let work_graph_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-work-graph/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&work_graph_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();

    assert!(dependencies.contains_key("agent-doc-element"));
    assert!(dependencies.contains_key("agent-doc-element-backlog"));
    for forbidden in [
        "agent-doc-core",
        "agent-doc-document",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-work-graph must stay source-agnostic and free of document, orchestration, git, editor IPC, sqlite, or tmux crates"
        );
    }
}

#[test]
fn test_agent_doc_workflow_owns_cross_cutting_workflow_kernel() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-workflow")),
        "agent-doc-workflow must stay a first-class workspace crate"
    );
    assert!(
        manifest_dir.join("agent-doc-workflow/src/lib.rs").exists(),
        "cross-cutting workflow policy should live in the focused workflow crate"
    );

    let workflow_source =
        fs::read_to_string(manifest_dir.join("agent-doc-workflow/src/lib.rs")).unwrap();
    for required in [
        "pub mod invariants;",
        "pub mod doctor;",
        "pub mod session_cycle;",
        "pub enum WorkflowEvidenceKind",
        "pub enum WorkflowProof",
        "pub enum WorkflowMutation",
        "pub enum WorkflowDecision",
        "pub struct WorkflowTransition",
        "pub fn decide_stale_supervisor",
        "pub fn decide_queue_drainability",
        "pub fn decide_captured_response",
        "pub fn decide_live_buffer",
    ] {
        assert!(
            workflow_source.contains(required),
            "agent-doc-workflow must own workflow kernel policy: {required}"
        );
    }
    let session_cycle_policy =
        fs::read_to_string(manifest_dir.join("agent-doc-workflow/src/session_cycle.rs")).unwrap();
    for required in [
        "pub enum SessionExecutionScope",
        "pub enum FinalizePendingMutationKind",
        "pub struct FinalizePendingMutation",
        "pub fn prompt_targets_from_changes",
        "pub fn prompt_targets_from_diff",
        "pub fn classify_execution_scope",
        "pub fn finalize_command",
    ] {
        assert!(
            session_cycle_policy.contains(required),
            "agent-doc-workflow must own session-cycle workflow policy: {required}"
        );
    }
    let workflow_invariants =
        fs::read_to_string(manifest_dir.join("agent-doc-workflow/src/invariants.rs")).unwrap();
    for required in [
        "pub struct WorkflowInvariantCatalog",
        "pub enum WorkflowInvariantId",
        "pub enum FactSourceKind",
        "pub enum RemediationAction",
        "pub fn workflow_invariant_catalog",
        "pub fn workflow_invariant_catalog_json",
    ] {
        assert!(
            workflow_invariants.contains(required),
            "agent-doc-workflow must own workflow invariant catalog policy: {required}"
        );
    }

    let workflow_doctor =
        fs::read_to_string(manifest_dir.join("agent-doc-workflow/src/doctor.rs")).unwrap();
    for required in [
        "WORKFLOW_DOCTOR_SCHEMA_VERSION",
        "pub enum WorkflowDoctorOutcome",
        "pub struct WorkflowDoctorReport",
        "pub struct WorkflowDoctorFacts",
        "pub struct PreflightDoctorFacts",
        "pub struct SessionCheckDoctorFacts",
        "pub struct CycleStateDoctorFacts",
        "pub struct OpsLogDoctorFacts",
        "pub struct ActorDoctorFacts",
        "pub struct GitDoctorFacts",
        "pub struct EditorDoctorFacts",
        "pub struct WorkflowInvariantResult",
        "pub fn evaluate_catalog",
        "pub fn classify_ops_marker",
    ] {
        assert!(
            workflow_doctor.contains(required),
            "agent-doc-workflow must own workflow doctor policy API: {required}"
        );
    }
    let orchestration_doctor =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/doctor.rs")).unwrap();
    for forbidden in [
        "WORKFLOW_DOCTOR_SCHEMA_VERSION",
        "pub enum WorkflowDoctorOutcome",
        "pub struct WorkflowDoctorReport",
        "pub struct WorkflowDoctorFacts",
        "pub struct PreflightDoctorFacts",
        "pub struct SessionCheckDoctorFacts",
        "pub struct CycleStateDoctorFacts",
        "pub struct OpsLogDoctorFacts",
        "pub struct ActorDoctorFacts",
        "pub struct GitDoctorFacts",
        "pub struct EditorDoctorFacts",
        "pub struct WorkflowInvariantResult",
        "pub fn evaluate_catalog",
        "fn evaluate_queue_continuation",
        "fn evaluate_stale_supervisor",
        "fn evaluate_closeout_commit",
        "fn evaluate_editor_convergence",
        "fn evaluate_generation_redirect",
        "fn evaluate_parent_gitlink",
        "fn closeout_repair_commands",
        "fn safe_commands",
        "fn operator_steps",
        "fn remediation_label",
        "fn has_marker",
        "fn classify_ops_marker(",
        "pub fn classify_ops_marker(",
        "pub use agent_doc_workflow::doctor",
        "pub(crate) use agent_doc_workflow::doctor",
    ] {
        assert!(
            !orchestration_doctor.contains(forbidden),
            "orchestration doctor must gather facts, not re-own or facade workflow doctor policy: {forbidden}"
        );
    }
    assert!(
        orchestration_doctor.contains("use agent_doc_workflow::doctor::{")
            && orchestration_doctor.contains("classify_ops_marker, evaluate_catalog"),
        "orchestration doctor should call the focused doctor policy directly"
    );
    let orchestration_autofix =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/autofix.rs")).unwrap();
    assert!(
        orchestration_autofix.contains("use agent_doc_workflow::doctor::{")
            && orchestration_autofix.contains("WorkflowDoctorOutcome")
            && orchestration_autofix.contains("WorkflowDoctorReport")
            && orchestration_autofix.contains("WorkflowInvariantResult")
            && orchestration_autofix.contains("WorkflowDoctorFacts, evaluate_catalog"),
        "orchestration autofix should consume focused workflow doctor policy directly"
    );

    let flow_mod =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/flow/mod.rs")).unwrap();
    assert!(
        !flow_mod.contains("pub mod workflow_state"),
        "orchestration must not expose a workflow_state facade"
    );
    assert!(
        !flow_mod.contains("pub mod workflow_invariants"),
        "orchestration must not expose a workflow_invariants facade"
    );
    for forbidden in [
        "pub use agent_doc_workflow::session_cycle",
        "SessionExecutionScope",
        "FinalizePendingMutation",
    ] {
        assert!(
            !flow_mod.contains(forbidden),
            "orchestration flow module must not re-export session-cycle workflow policy: {forbidden}"
        );
    }
    let orchestration_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/lib.rs")).unwrap();
    for forbidden in [
        "pub use agent_doc_workflow::session_cycle",
        "SessionExecutionScope",
        "FinalizePendingMutation",
    ] {
        assert!(
            !orchestration_lib.contains(forbidden),
            "orchestration lib must not re-export session-cycle workflow policy: {forbidden}"
        );
    }
    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/flow/workflow_state.rs")
            .exists(),
        "orchestration must not keep a workflow_state module after extraction"
    );
    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/flow/workflow_invariants.rs")
            .exists(),
        "orchestration must not keep a workflow_invariants module after extraction"
    );
    let orchestration_session_cycle =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/flow/session_cycle.rs"))
            .unwrap();
    for forbidden in [
        "pub enum SessionExecutionScope",
        "pub enum FinalizePendingMutationKind",
        "pub struct FinalizePendingMutation",
        "pub fn prompt_targets_from_changes",
        "fn extract_prompt_targets(",
        "pub fn classify_execution_scope",
        "pub fn finalize_command",
        "pub use agent_doc_workflow::session_cycle",
        "agent_doc_workflow::session_cycle::SessionExecutionScope",
        "agent_doc_workflow::session_cycle::FinalizePendingMutation",
    ] {
        assert!(
            !orchestration_session_cycle.contains(forbidden),
            "orchestration must not define or re-export session-cycle workflow policy: {forbidden}"
        );
    }
    let preflight_run =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/preflight/run.rs"))
            .unwrap();
    assert!(
        preflight_run.contains("agent_doc_workflow::session_cycle::prompt_targets_from_changes"),
        "preflight should call focused session-cycle workflow policy directly"
    );
    let plan_source = fs::read_to_string(manifest_dir.join("src/plan.rs")).unwrap();
    assert!(
        plan_source.contains("use agent_doc_workflow::session_cycle::{")
            && plan_source.contains("classify_execution_scope")
            && plan_source.contains("finalize_command")
            && plan_source.contains("prompt_targets_from_changes"),
        "plan.rs should call focused session-cycle workflow policy directly"
    );
    assert!(
        !plan_source.contains("agent_doc_orchestration::flow::session_cycle::"),
        "plan.rs must not route session-cycle workflow policy through orchestration"
    );
    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/prompt_contract.rs")
            .exists(),
        "orchestration must not keep a prompt_contract module for session-cycle policy facades"
    );
    let prompt_context =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/prompt_context.rs"))
            .unwrap();
    assert!(
        prompt_context.contains("use agent_doc_workflow::session_cycle::prompt_targets_from_diff;")
            && prompt_context.contains("prompt_targets_from_diff(diff_text)")
            && !prompt_context.contains("fn extract_prompt_targets("),
        "prompt_context should call focused prompt-target extraction directly"
    );
    for relative_path in [
        "agent-doc-orchestration/src/doctor.rs",
        "agent-doc-orchestration/src/autofix.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative_path)).unwrap();
        assert!(
            source.contains("agent_doc_workflow::invariants::")
                && source.contains("workflow_invariant_catalog"),
            "{relative_path} should call the focused workflow invariant catalog API directly"
        );
    }

    let workflow_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-workflow/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&workflow_manifest).unwrap();
    let dependencies = parsed
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .cloned()
        .unwrap_or_default();
    for expected in [
        "agent-doc-diff",
        "agent-doc-frontmatter",
        "indexmap",
        "serde",
        "serde_json",
    ] {
        assert!(
            dependencies.contains_key(expected),
            "agent-doc-workflow may use focused pure-policy dependency: {expected}"
        );
    }
    assert!(
        dependencies.keys().all(|dependency| matches!(
            dependency.as_str(),
            "agent-doc-diff" | "agent-doc-frontmatter" | "indexmap" | "serde" | "serde_json"
        )),
        "agent-doc-workflow should remain pure and free of orchestration, git, editor IPC, sqlite, or tmux dependencies"
    );
}

#[test]
fn test_agent_doc_diff_owns_partial_staging_pure_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let diff_source = fs::read_to_string(manifest_dir.join("agent-doc-diff/src/lib.rs")).unwrap();
    for required in [
        "pub struct PartialStagingCompanionFinding",
        "pub fn partial_staging_companion_finding(",
        "pub fn is_partial_staging_relevant_path",
        "pub fn partial_staging_paths_look_related",
        "pub fn extract_changed_string_literals",
        "pub fn first_bare_prompt_prefix_target_before_marker",
    ] {
        assert!(
            diff_source.contains(required),
            "agent-doc-diff must own partial-staging pure diff/path policy: {required}"
        );
    }

    let partial_staging = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/partial_staging.rs"),
    )
    .unwrap();
    for forbidden in [
        "pub(crate) fn is_partial_staging_relevant_path",
        "pub(crate) fn partial_staging_paths_look_related",
        "pub(crate) fn path_looks_test_like",
        "pub(crate) fn extract_changed_string_literals",
        "pub(crate) fn extract_string_literals_from_line",
        "pub(crate) fn interesting_changed_literal",
        "agent_doc_diff::is_partial_staging_relevant_path",
        "agent_doc_diff::partial_staging_paths_look_related",
        "agent_doc_diff::extract_changed_string_literals",
        ".intersection(&dirty_literals)",
    ] {
        assert!(
            !partial_staging.contains(forbidden),
            "session_check partial-staging must stay an adapter, not re-own pure diff/path policy"
        );
    }
    assert!(
        partial_staging.contains("agent_doc_diff::partial_staging_companion_finding("),
        "session_check partial-staging should call the focused diff classifier directly"
    );
}

#[test]
fn test_agent_doc_diff_owns_truncation_pure_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let diff_source = fs::read_to_string(manifest_dir.join("agent-doc-diff/src/lib.rs")).unwrap();
    for required in [
        "pub fn extract_last_added_line(",
        "pub fn looks_truncated(",
        "pub fn truncate_for_log(",
    ] {
        assert!(
            diff_source.contains(required),
            "agent-doc-diff must own pure truncation policy: {required}"
        );
    }

    let diff_io_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/diff_io.rs")).unwrap();
    for forbidden in [
        "fn extract_last_added_line(",
        "fn looks_truncated(",
        "fn truncate_for_log(",
        "use similar::{ChangeTag, TextDiff};",
    ] {
        assert!(
            !diff_io_source.contains(forbidden),
            "diff_io must stay a file/editor wait adapter, not re-own pure truncation policy: {forbidden}"
        );
    }
    for required in [
        "agent_doc_diff::{",
        "extract_last_added_line",
        "looks_truncated",
        "truncate_for_log",
    ] {
        assert!(
            diff_io_source.contains(required),
            "diff_io should call focused diff truncation helpers directly: {required}"
        );
    }
}

#[test]
fn test_agent_doc_diff_owns_unstarted_prompt_bearing_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let diff_source = fs::read_to_string(manifest_dir.join("agent-doc-diff/src/lib.rs")).unwrap();
    for required in [
        "pub fn prompt_bearing_body_for_unstarted_prompt_guard",
        "pub fn strip_queue_components_for_unstarted_prompt_guard",
        "pub fn prompt_target_is_immediately_before_existing_response",
        "pub fn first_unstarted_prompt_bearing_change_from_diff",
    ] {
        assert!(
            diff_source.contains(required),
            "agent-doc-diff must own unstarted prompt-bearing diff policy: {required}"
        );
    }

    let closeout_guards = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/closeout_guards.rs"),
    )
    .unwrap();
    for forbidden in [
        "pub(crate) fn strip_queue_components_for_unstarted_prompt_guard",
        "pub(crate) fn prompt_target_is_immediately_before_existing_response",
        "fn strip_queue_components_for_unstarted_prompt_guard",
        "fn prompt_target_is_immediately_before_existing_response",
    ] {
        assert!(
            !closeout_guards.contains(forbidden),
            "session_check closeout guards must not re-own unstarted prompt-bearing policy"
        );
    }
    for required in [
        "agent_doc_diff::prompt_bearing_body_for_unstarted_prompt_guard",
        "agent_doc_diff::first_unstarted_prompt_bearing_change_from_diff",
    ] {
        assert!(
            closeout_guards.contains(required),
            "session_check closeout guards should call the focused diff helper directly: {required}"
        );
    }
}

#[test]
fn test_agent_doc_diff_owns_post_exchange_comment_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let diff_source = fs::read_to_string(manifest_dir.join("agent-doc-diff/src/lib.rs")).unwrap();
    for required in [
        "pub fn post_exchange_ordinary_html_comments",
        "pub fn post_exchange_comment_directive_signals",
    ] {
        assert!(
            diff_source.contains(required),
            "agent-doc-diff must own post-exchange comment directive policy: {required}"
        );
    }

    let preflight_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/preflight.rs")).unwrap();
    for forbidden in [
        "fn post_exchange_ordinary_html_comments",
        "fn comment_is_user_note",
        "fn post_exchange_comment_directive_signals",
        "fn first_word",
        "fn looks_like_slash_command",
    ] {
        assert!(
            !preflight_source.contains(forbidden),
            "preflight must not re-own post-exchange comment directive policy: {forbidden}"
        );
    }
    for required in [
        "agent_doc_diff::post_exchange_ordinary_html_comments",
        "agent_doc_diff::post_exchange_comment_directive_signals",
    ] {
        assert!(
            preflight_source.contains(required),
            "preflight should call the focused diff comment policy directly: {required}"
        );
    }
}

#[test]
fn test_project_config_io_tmux_helpers_have_no_config_facade() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();
    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-project-config-io")),
        "agent-doc-project-config-io must stay a first-class workspace crate"
    );

    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/config.rs")
            .exists(),
        "orchestration must not keep global config as a module facade"
    );

    let project_config_source =
        fs::read_to_string(manifest_dir.join("agent-doc-project-config-io/src/lib.rs")).unwrap();
    for required_snippet in [
        "pub fn load_project()",
        "pub fn load_project_for_doc(",
        "pub fn project_root_for_doc(",
        "pub fn agent_doc_bug_target_document_for_doc(",
        "pub fn load_project_from(",
        "pub fn project_tmux_session()",
        "pub fn save_project(",
        "pub fn save_project_to(",
        "pub fn update_project_tmux_session(",
        "pub fn clear_project_tmux_session()",
        "agent_doc_fs::find_project_root",
        "agent_doc_fs::referenced_markdown_path_checked",
    ] {
        assert!(
            project_config_source.contains(required_snippet),
            "agent-doc-project-config-io must own file-backed project config helper: {required_snippet}"
        );
    }

    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/project_config_io.rs")
            .exists(),
        "orchestration must not keep project_config_io as an adapter module or facade"
    );
    let orchestration_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/lib.rs")).unwrap();
    assert!(
        !orchestration_lib.contains("pub mod project_config_io"),
        "orchestration must not expose project_config_io as a module facade"
    );

    for relative in [
        "src/session_cmd.rs",
        "src/session_actor_cmd.rs",
        "src/orchestrate.rs",
        "src/describe_image.rs",
        "src/plan.rs",
        "src/patch.rs",
        "agent-doc-orchestration/src/frontmatter_io.rs",
        "agent-doc-orchestration/src/graph.rs",
        "agent-doc-orchestration/src/template_io.rs",
        "agent-doc-orchestration/src/claim.rs",
        "agent-doc-orchestration/src/lint_gate.rs",
        "agent-doc-orchestration/src/route.rs",
        "agent-doc-orchestration/src/route/session_resolution.rs",
        "agent-doc-orchestration/src/resync.rs",
        "agent-doc-orchestration/src/sync.rs",
        "agent-doc-orchestration/src/session_accretion.rs",
        "agent-doc-orchestration/src/session_check/pending_guards.rs",
        "agent-doc-orchestration/src/project_controller/rpc.rs",
        "agent-doc-orchestration/src/start.rs",
        "agent-doc-orchestration/src/start/run.rs",
        "agent-doc-orchestration/src/start/detection.rs",
        "agent-doc-orchestration/src/start/supervisor_io.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            source.contains("agent_doc_project_config_io"),
            "{relative} should call project config IO through agent-doc-project-config-io directly"
        );
        for forbidden_snippet in [
            "agent_doc_orchestration::project_config_io",
            "crate::project_config_io",
            "pub use agent_doc_project_config_io",
            "pub use crate::project_config_io",
            "use crate::project_config_io",
            "use crate::{project_config_io",
            "config::project_tmux_session",
            "config::clear_project_tmux_session",
            "config::update_project_tmux_session",
            "crate::config::project_tmux_session",
        ] {
            assert!(
                !source.contains(forbidden_snippet),
                "{relative} must not route project-config IO through orchestration or config: {forbidden_snippet}"
            );
        }
    }

    let root_manifest: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let root_dependencies = root_manifest["dependencies"].as_table().unwrap();
    assert!(
        root_dependencies.contains_key("agent-doc-project-config-io"),
        "the CLI crate must depend on the focused project-config IO crate directly"
    );
    let orchestration_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/Cargo.toml")).unwrap();
    let orchestration: toml::Value = toml::from_str(&orchestration_manifest).unwrap();
    let orchestration_dependencies = orchestration["dependencies"].as_table().unwrap();
    assert!(
        orchestration_dependencies.contains_key("agent-doc-project-config-io"),
        "orchestration must depend on the focused project-config IO crate directly"
    );

    let focused_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-project-config-io/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&focused_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    for required in ["agent-doc-frontmatter", "agent-doc-fs", "toml"] {
        assert!(
            dependencies.contains_key(required),
            "agent-doc-project-config-io should depend on {required} for project config IO"
        );
    }
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-project-config-io must stay free of core, orchestration, git, editor IPC, sqlite, and tmux-router dependencies"
        );
    }

    let closeout_guards = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/closeout_guards.rs"),
    )
    .unwrap();
    assert!(
        !closeout_guards.contains("pub(crate) fn first_bare_prompt_prefix_target_before_marker"),
        "session_check closeout guards must not re-own marker-scoped prompt-prefix diff slicing"
    );
    assert!(
        closeout_guards.contains("agent_doc_diff::first_bare_prompt_prefix_target_before_marker"),
        "session_check closeout guards should call the focused diff helper directly"
    );
}

#[test]
fn test_global_config_has_no_orchestration_facade() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();
    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-config")),
        "agent-doc-config must stay a first-class workspace crate"
    );

    let config_source =
        fs::read_to_string(manifest_dir.join("agent-doc-config/src/lib.rs")).unwrap();
    for required_snippet in [
        "pub enum ExecutionMode",
        "pub struct Config",
        "pub struct TerminalConfig",
        "pub struct AgentConfig",
        "pub fn load()",
        "fn config_path()",
        "CodexNetworkAccess",
        "ModelConfig",
    ] {
        assert!(
            config_source.contains(required_snippet),
            "agent-doc-config must own global config vocabulary and loading: {required_snippet}"
        );
    }

    let orchestration_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/lib.rs")).unwrap();
    assert!(
        !orchestration_lib.contains("pub mod config"),
        "orchestration must not expose global config as a module facade"
    );

    for relative in [
        "src/main.rs",
        "src/init.rs",
        "src/orchestrate.rs",
        "src/session_actor_cmd.rs",
        "src/terminal.rs",
        "agent-doc-orchestration/src/agent/mod.rs",
        "agent-doc-orchestration/src/agent/codex.rs",
        "agent-doc-orchestration/src/graph.rs",
        "agent-doc-orchestration/src/harness.rs",
        "agent-doc-orchestration/src/run.rs",
        "agent-doc-orchestration/src/start.rs",
        "agent-doc-orchestration/src/start/run.rs",
        "agent-doc-orchestration/src/start/idle_watch.rs",
        "agent-doc-orchestration/src/stream.rs",
        "agent-doc-orchestration/src/watch.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            source.contains("agent_doc_config"),
            "{relative} should call global config through agent-doc-config directly"
        );
        for forbidden_snippet in [
            "agent_doc_orchestration::config",
            "crate::config::",
            "use crate::config",
            "use crate::{config",
            "pub use agent_doc_config",
        ] {
            assert!(
                !source.contains(forbidden_snippet),
                "{relative} must not route global config through orchestration: {forbidden_snippet}"
            );
        }
    }

    let root_manifest: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let root_dependencies = root_manifest["dependencies"].as_table().unwrap();
    assert!(
        root_dependencies.contains_key("agent-doc-config"),
        "the CLI crate must depend on the focused global config crate directly"
    );
    let orchestration_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/Cargo.toml")).unwrap();
    let orchestration: toml::Value = toml::from_str(&orchestration_manifest).unwrap();
    let orchestration_dependencies = orchestration["dependencies"].as_table().unwrap();
    assert!(
        orchestration_dependencies.contains_key("agent-doc-config"),
        "orchestration must depend on the focused global config crate directly"
    );

    let focused_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-config/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&focused_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    for required in [
        "agent-doc-frontmatter",
        "agent-doc-model-tier",
        "serde",
        "toml",
    ] {
        assert!(
            dependencies.contains_key(required),
            "agent-doc-config should depend on {required} for global config parsing"
        );
    }
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-config must stay free of core, orchestration, git, editor IPC, sqlite, and tmux-router dependencies"
        );
    }
}

#[test]
fn test_agent_doc_frontmatter_owns_cross_document_security_review_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let frontmatter_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-frontmatter/src/lib.rs")).unwrap();
    let security_review =
        fs::read_to_string(manifest_dir.join("agent-doc-frontmatter/src/security_review.rs"))
            .unwrap();

    assert!(
        frontmatter_lib.contains("pub mod security_review;"),
        "agent-doc-frontmatter should expose focused security-review policy"
    );
    for required in [
        "pub enum SecurityReviewSubject",
        "pub struct CrossDocumentSecurityReviewDecision",
        "pub fn cross_document_security_review_decision(",
        "CollaborationMode::Shared",
        "has_security_review()",
    ] {
        assert!(
            security_review.contains(required),
            "agent-doc-frontmatter must own cross-document security-review policy: {required}"
        );
    }

    let orchestration_security =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/security.rs")).unwrap();
    for forbidden in [
        "CollaborationMode::Shared",
        "has_security_review()",
        "pub fn cross_document_security_review_decision(",
        "pub enum SecurityReviewSubject",
        "pub struct CrossDocumentSecurityReviewDecision",
        "pub use agent_doc_frontmatter::security_review",
        "pub(crate) use agent_doc_frontmatter::security_review",
    ] {
        assert!(
            !orchestration_security.contains(forbidden),
            "orchestration security must adapt paths/errors, not re-own or facade frontmatter security-review policy: {forbidden}"
        );
    }
    assert!(
        orchestration_security.contains("use agent_doc_frontmatter::security_review::{")
            && orchestration_security.contains("cross_document_security_review_decision"),
        "orchestration security should call focused frontmatter security-review policy directly"
    );

    let frontmatter_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-frontmatter/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&frontmatter_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-frontmatter security-review policy must stay free of core, orchestration, git, editor IPC, sqlite, or tmux crates"
        );
    }
}

#[test]
fn test_snapshot_has_no_find_project_root_facade() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let snapshot_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/snapshot.rs")).unwrap();
    assert!(
        !snapshot_source.contains("pub use agent_doc_fs::find_project_root"),
        "snapshot.rs must not re-export the agent-doc-fs project-root helper"
    );
    assert!(
        snapshot_source.contains("use agent_doc_fs::find_project_root;"),
        "snapshot.rs should import the agent-doc-fs project-root helper privately"
    );

    let orchestration_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/lib.rs")).unwrap();
    assert!(
        !orchestration_lib.contains("pub mod fs_util"),
        "orchestration must not keep an fs_util facade over agent-doc-fs"
    );

    fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push(path);
            }
        }
    }

    let mut source_files = Vec::new();
    collect_rs_files(
        &manifest_dir.join("agent-doc-orchestration/src"),
        &mut source_files,
    );
    collect_rs_files(&manifest_dir.join("src"), &mut source_files);
    for path in source_files {
        let source = fs::read_to_string(&path).unwrap();
        let relative = path.strip_prefix(manifest_dir).unwrap().display();
        for forbidden_snippet in [
            "snapshot::find_project_root",
            "crate::snapshot::find_project_root",
            "agent_doc_orchestration::snapshot::find_project_root",
            "crate::fs_util::find_project_root",
            "agent_doc_orchestration::fs_util::find_project_root",
        ] {
            assert!(
                !source.contains(forbidden_snippet),
                "{relative} must call agent_doc_fs::find_project_root directly: {forbidden_snippet}"
            );
        }
    }
}

#[test]
fn test_session_actor_has_no_sqlite_state_facade() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let session_actor_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/session_actor.rs"))
            .unwrap();
    assert!(
        !session_actor_source.contains("pub use agent_doc_sqlite::state_store"),
        "session_actor.rs must not re-export SQLite actor storage types"
    );
    assert!(
        session_actor_source.contains(
            "use agent_doc_sqlite::state_store::{ActorLastTransition, ActorRecord, ActorState};"
        ),
        "session_actor.rs should import SQLite actor storage types privately"
    );

    fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push(path);
            }
        }
    }

    let mut source_files = Vec::new();
    collect_rs_files(
        &manifest_dir.join("agent-doc-orchestration/src"),
        &mut source_files,
    );
    collect_rs_files(&manifest_dir.join("src"), &mut source_files);
    for path in source_files {
        let source = fs::read_to_string(&path).unwrap();
        let relative = path.strip_prefix(manifest_dir).unwrap().display();
        for forbidden_snippet in [
            "session_actor::ActorState",
            "session_actor::ActorRecord",
            "session_actor::ActorLastTransition",
        ] {
            assert!(
                !source.contains(forbidden_snippet),
                "{relative} must import actor storage types from agent_doc_sqlite::state_store directly: {forbidden_snippet}"
            );
        }
    }
}

#[test]
fn test_project_controller_has_no_sqlite_status_facade() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let project_controller_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/project_controller.rs"))
            .unwrap();
    assert!(
        !project_controller_source.contains("pub use state_store::{"),
        "project_controller.rs must not re-export SQLite status/storage types"
    );
    assert!(
        project_controller_source.contains("use state_store::{"),
        "project_controller.rs should import SQLite status/storage types privately"
    );

    fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push(path);
            }
        }
    }

    let mut source_files = Vec::new();
    collect_rs_files(
        &manifest_dir.join("agent-doc-orchestration/src"),
        &mut source_files,
    );
    collect_rs_files(&manifest_dir.join("src"), &mut source_files);
    for path in source_files {
        let source = fs::read_to_string(&path).unwrap();
        let relative = path.strip_prefix(manifest_dir).unwrap().display();
        for forbidden_snippet in [
            "project_controller::ActorTransitionStatus",
            "project_controller::AdminOperationStatus",
            "project_controller::DispatchAttemptStatus",
            "project_controller::ProjectionDiagnosticStatus",
            "project_controller::QueueBackpressureStatus",
            "project_controller::QueueControlStatus",
            "project_controller::QueueHeadStatus",
            "project_controller::SessionOperatorStatus",
            "project_controller::SupervisorLeaseStatus",
            "project_controller::state_db_path",
        ] {
            assert!(
                !source.contains(forbidden_snippet),
                "{relative} must import controller storage/status types from agent_doc_sqlite::state_store directly: {forbidden_snippet}"
            );
        }
    }
}

#[test]
fn test_sessions_has_no_tmux_router_type_facade() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sessions_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/sessions.rs")).unwrap();
    assert!(
        !sessions_source.contains("pub use tmux_router"),
        "sessions.rs must not re-export tmux-router types"
    );
    assert!(
        sessions_source.contains("use tmux_router::{"),
        "sessions.rs should import tmux-router types privately for its adapter helpers"
    );

    fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push(path);
            }
        }
    }

    let mut source_files = Vec::new();
    collect_rs_files(
        &manifest_dir.join("agent-doc-orchestration/src"),
        &mut source_files,
    );
    collect_rs_files(&manifest_dir.join("src"), &mut source_files);
    for path in source_files {
        let source = fs::read_to_string(&path).unwrap();
        let relative = path.strip_prefix(manifest_dir).unwrap().display();
        for forbidden_snippet in [
            "sessions::Tmux",
            "sessions::IsolatedTmux",
            "sessions::PaneMoveOp",
            "sessions::RegistryLock",
            "sessions::SessionRegistry",
            "sessions::SessionEntry",
            "agent_doc_orchestration::sessions::Tmux",
            "agent_doc_orchestration::sessions::IsolatedTmux",
            "agent_doc_orchestration::sessions::PaneMoveOp",
            "agent_doc_orchestration::sessions::RegistryLock",
            "agent_doc_orchestration::sessions::SessionRegistry",
            "agent_doc_orchestration::sessions::SessionEntry",
            "crate::sessions::Tmux",
            "crate::sessions::IsolatedTmux",
            "crate::sessions::PaneMoveOp",
            "crate::sessions::RegistryLock",
            "crate::sessions::SessionRegistry",
            "crate::sessions::SessionEntry",
        ] {
            assert!(
                !source.contains(forbidden_snippet),
                "{relative} must import tmux-router types from tmux_router directly: {forbidden_snippet}"
            );
        }
    }
}

#[test]
fn test_agent_doc_merge_is_pure_workspace_boundary() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-merge")),
        "agent-doc-merge must stay a first-class workspace crate"
    );
    assert!(
        !members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-document-merge")),
        "merge policy should stay in agent-doc-merge, not under the pure document crate family"
    );
    let realtime_spec =
        fs::read_to_string(manifest_dir.join("specs/14-realtime-workflow.md")).unwrap();
    assert!(
        realtime_spec.contains("`agent-doc-merge` for pure merge semantics")
            && !realtime_spec.contains("agent-doc-document-merge"),
        "realtime spec must keep agent-doc-merge as the pure merge-policy boundary"
    );

    let merge_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-merge/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&merge_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();

    assert!(
        !dependencies.contains_key("agent-doc-core"),
        "agent-doc-merge owns pure merge policy directly and must not depend on the transitional core crate"
    );
    assert!(dependencies.contains_key("agent-doc-element"));
    assert!(dependencies.contains_key("agent-doc-element-queue"));
    assert!(dependencies.contains_key("agent-doc-frontmatter"));
    assert!(dependencies.contains_key("agent-doc-markdown-ast"));
    let orchestration_merge =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/merge.rs")).unwrap();
    for forbidden_snippet in ["pub fn merge_contents_crdt(", "fn merge_frontmatter_aware"] {
        assert!(
            !orchestration_merge.contains(forbidden_snippet),
            "orchestration must not re-own pure frontmatter-aware CRDT merge policy: {forbidden_snippet}"
        );
    }
    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/merge_control_state_machine.rs")
            .exists(),
        "orchestration must not keep a merge-control facade over agent-doc-merge::ownership"
    );
    let orchestration_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/lib.rs")).unwrap();
    assert!(
        !orchestration_lib.contains("merge_control_state_machine"),
        "orchestration must not re-export merge-control ownership policy"
    );
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-merge must not depend on realtime, turn, git, editor, or sqlite crates"
        );
    }
}

#[test]
fn test_agent_doc_debounce_is_sidecar_boundary() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-debounce")),
        "agent-doc-debounce must stay a first-class workspace crate"
    );

    let package_version = workspace["package"]["version"].as_str();
    let orchestration_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/Cargo.toml")).unwrap();
    let orchestration: toml::Value = toml::from_str(&orchestration_manifest).unwrap();
    let orchestration_dependencies = orchestration["dependencies"].as_table().unwrap();
    let dependency = orchestration_dependencies["agent-doc-debounce"]
        .as_table()
        .unwrap();
    assert_eq!(
        dependency.get("path").and_then(toml::Value::as_str),
        Some("../agent-doc-debounce")
    );
    assert_eq!(
        dependency.get("version").and_then(toml::Value::as_str),
        package_version
    );

    let debounce_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-debounce/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&debounce_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();

    assert!(dependencies.contains_key("agent-doc-log-time"));
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-debounce must not depend on core, orchestration, git, editor IPC, sqlite, or tmux crates"
        );
    }
}

#[test]
fn test_agent_doc_controller_owns_route_trigger_matching_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let controller_dispatch =
        fs::read_to_string(manifest_dir.join("agent-doc-controller/src/dispatch.rs")).unwrap();
    for required in [
        "pub fn recent_lines_contain_trigger",
        "pub fn line_contains_trigger",
        "pub fn compact_trigger_text",
        "pub fn strip_leading_prompt_prefix",
        "pub fn shares_trigger_prefix",
        "pub fn recent_lines_contain_wrapped_trigger",
        "pub fn route_trigger_visible_in_current_draft",
        "fn line_contains_equivalent_agent_doc_path_trigger",
        "fn single_agent_doc_path_arg",
        "struct AgentDocPathArg",
        "fn agent_doc_path_arg",
        "fn agent_doc_path_args_equivalent",
        "fn wrapped_trigger_starts_at_line",
    ] {
        assert!(
            controller_dispatch.contains(required),
            "agent-doc-controller must own route trigger matching policy: {required}"
        );
    }

    for relative in [
        "agent-doc-orchestration/src/route/cycle_ack.rs",
        "agent-doc-orchestration/src/route/dispatch.rs",
        "agent-doc-orchestration/src/route.rs",
        "agent-doc-orchestration/src/start/detection.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        for forbidden in [
            "fn recent_lines_contain_trigger",
            "fn line_contains_trigger",
            "fn compact_trigger_text",
            "fn strip_leading_prompt_prefix",
            "fn shares_trigger_prefix",
            "fn recent_lines_contain_wrapped_trigger",
            "pub(crate) fn direct_pane_existing_draft_visible",
            "fn direct_pane_existing_draft_visible",
            "fn line_contains_equivalent_agent_doc_path_trigger",
            "fn single_agent_doc_path_arg",
            "struct AgentDocPathArg",
            "fn agent_doc_path_arg",
            "fn agent_doc_path_args_equivalent",
            "fn wrapped_trigger_starts_at_line",
        ] {
            assert!(
                !source.contains(forbidden),
                "{relative} must not re-own route trigger matching policy: {forbidden}"
            );
        }
    }
    let route_dispatch =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/route/dispatch.rs"))
            .unwrap();
    let start_detection =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/start/detection.rs"))
            .unwrap();
    assert!(
        route_dispatch.contains("route_trigger_visible_in_current_draft(&content, &trigger")
            && start_detection.contains("route_trigger_visible_in_current_draft(content, payload"),
        "route/start should call focused controller current-draft visibility policy directly"
    );
}

#[test]
fn test_agent_doc_controller_dispatch_has_no_rpc_facade() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let members = workspace["workspace"]["members"].as_array().unwrap();

    assert!(
        members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-controller")),
        "agent-doc-controller must stay a first-class workspace crate"
    );
    let root_dependencies = workspace["dependencies"].as_table().unwrap();
    assert!(
        root_dependencies.contains_key("agent-doc-controller"),
        "the CLI shell should call focused controller dispatch helpers directly"
    );

    let rpc_source = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/project_controller/rpc.rs"),
    )
    .unwrap();
    let project_controller_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/project_controller.rs"))
            .unwrap();
    let command_line_source =
        fs::read_to_string(manifest_dir.join("agent-doc-controller/src/command_line.rs")).unwrap();
    let controller_dispatch =
        fs::read_to_string(manifest_dir.join("agent-doc-controller/src/dispatch.rs")).unwrap();
    let controller_status =
        fs::read_to_string(manifest_dir.join("agent-doc-controller/src/status.rs")).unwrap();
    for required_snippet in [
        "pub fn cmdline_is_agent_doc_owner_session(",
        "pub fn cmdline_references_md_document(",
        "pub fn owner_document_from_cmdline(",
    ] {
        assert!(
            command_line_source.contains(required_snippet),
            "agent-doc-controller should own process command-line ownership recognition: {required_snippet}"
        );
    }
    for required_snippet in [
        "pub fn dispatch_diagnostic_field",
        "pub fn append_dispatch_proof_payload(",
    ] {
        assert!(
            controller_dispatch.contains(required_snippet),
            "agent-doc-controller should own controller dispatch diagnostic payload policy: {required_snippet}"
        );
    }
    for required_snippet in [
        "pub struct ControllerProcessFreshness",
        "pub struct ControllerFreshnessStatus",
        "pub struct ControlPlaneStatus",
        "pub struct ControlPlaneActorStatus",
        "pub enum ControllerHandoffState",
        "pub fn default_control_plane_status(",
        "pub fn status_categories",
        "pub fn control_plane_status(",
        "pub fn controller_status_from_bootstrap(",
        "pub fn inactive_controller_status(",
        "pub fn controller_freshness_status(",
        "pub fn controller_process_freshness_from_inodes(",
        "pub fn parse_handoff_state(",
    ] {
        assert!(
            controller_status.contains(required_snippet),
            "agent-doc-controller should own controller status/freshness projection: {required_snippet}"
        );
    }
    for forbidden_snippet in [
        "pub use agent_doc_controller::dispatch",
        "pub use agent_doc_controller::status",
        "pub fn dispatch_error_stale_generation_redirect_target",
        "pub fn dispatch_error_supervisor_restart_redirect",
        "pub(crate) fn dispatch_command_kind_is_operator_reopen",
        "pub(crate) fn pause_reason_is_stale_supervisor_churn_stop",
        "pub(crate) fn stale_supervisor_pid_from_pause_reason",
        "pub(crate) fn spent_preset_id_from_pause_reason",
        "pub(crate) struct StaleQueuePauseRecovery",
        "pub(crate) fn dispatch_error_stale_queue_pause_recovery",
        "pub(crate) struct CloseoutBlockDispatchFacts",
        "pub(crate) enum CloseoutBlockDispatchDecision",
        "pub(crate) fn classify_closeout_block_dispatch",
        "pub(crate) fn recycle_debounce_decision",
        "pub(crate) fn force_overrides_in_flight_gate",
        "fn dispatch_diagnostic_field",
        "fn append_dispatch_proof_payload(",
    ] {
        assert!(
            !rpc_source.contains(forbidden_snippet),
            "project_controller::rpc must not re-export or wrap pure controller dispatch helpers: {forbidden_snippet}"
        );
    }
    for forbidden_snippet in [
        "pub struct ControllerProcessFreshness",
        "pub struct ControllerFreshnessStatus",
        "pub struct ControlPlaneStatus",
        "pub struct ControlPlaneActorStatus",
        "pub enum ControllerHandoffState",
        "fn default_control_plane_status(",
        "fn control_plane_status(",
        "fn controller_status_from_bootstrap(",
        "fn inactive_controller_status(",
        "fn controller_freshness_status(",
        "fn controller_process_freshness_from_inodes(",
        "fn parse_handoff_state(",
        "pub use agent_doc_controller::status",
    ] {
        assert!(
            !project_controller_source.contains(forbidden_snippet),
            "project_controller must not re-own or facade controller status/freshness projection: {forbidden_snippet}"
        );
    }
    assert!(
        rpc_source.contains("use agent_doc_controller::dispatch::{"),
        "project_controller::rpc should import focused controller dispatch helpers privately"
    );
    assert!(
        rpc_source.contains("dispatch_diagnostic_field(diagnostic_payload,")
            && rpc_source.contains("append_dispatch_proof_payload(&diagnostic_payload,"),
        "project_controller::rpc should call focused controller dispatch diagnostic helpers directly"
    );
    assert!(
        rpc_source.contains("use agent_doc_controller::status")
            && project_controller_source.contains("use agent_doc_controller::status::{"),
        "orchestration should call focused controller status/freshness helpers directly"
    );
    for forbidden_snippet in [
        "fn agent_doc_controller_serve_arg_index(args:",
        "fn controller_serve_project_root_from_args(args:",
    ] {
        assert!(
            !project_controller_source.contains(forbidden_snippet),
            "project_controller must not wrap pure controller command-line helpers: {forbidden_snippet}"
        );
    }
    for source in [&rpc_source, &project_controller_source] {
        assert!(
            source.contains(
                "agent_doc_controller::command_line::controller_serve_project_root_from_args"
            ),
            "orchestration should call focused controller command-line parsing directly"
        );
    }
    let sync_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/sync.rs")).unwrap();
    for forbidden_snippet in [
        "fn token_is_agent_doc_binary(",
        "fn token_is_harness_binary(",
        "fn token_is_non_owner_agent_doc_subcommand(",
        "fn cmdline_is_agent_doc_owner_session(",
        "fn cmdline_references_md_document(",
        "fn owner_document_from_cmdline(",
    ] {
        assert!(
            !sync_source.contains(forbidden_snippet),
            "sync.rs must not re-own pure process command-line ownership policy: {forbidden_snippet}"
        );
    }
    for required_snippet in [
        "agent_doc_controller::command_line::cmdline_is_agent_doc_owner_session",
        "agent_doc_controller::command_line::cmdline_references_md_document",
        "agent_doc_controller::command_line::owner_document_from_cmdline",
    ] {
        assert!(
            sync_source.contains(required_snippet),
            "sync.rs should call focused controller command-line policy directly: {required_snippet}"
        );
    }

    let authoritative_actor = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/route/authoritative_actor.rs"),
    )
    .unwrap();
    let route_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/route.rs")).unwrap();
    let route_dispatch_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/route/dispatch.rs"))
            .unwrap();
    let route_dispatch_only_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/route/dispatch_only.rs"))
            .unwrap();
    let route_busy_pane_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/route/busy_pane.rs"))
            .unwrap();
    let route_startup_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/route/startup.rs"))
            .unwrap();
    let route_pane_resolution_source = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/route/pane_resolution.rs"),
    )
    .unwrap();
    let route_cycle_ack_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/route/cycle_ack.rs"))
            .unwrap();
    let flow_routed_reopen_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/flow/routed_reopen.rs"))
            .unwrap();
    let flow_types_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/flow/types.rs")).unwrap();
    assert!(
        authoritative_actor.contains("agent_doc_controller::dispatch::dispatch_error_is_coalesced"),
        "route authorization should call the focused controller dispatch classifier directly"
    );
    assert!(
        authoritative_actor.contains(
            "agent_doc_controller::dispatch::stale_queue_pause_recovery_from_dispatch_error"
        ),
        "route authorization should call the focused stale-queue recovery classifier directly"
    );
    let controller_dispatch =
        fs::read_to_string(manifest_dir.join("agent-doc-controller/src/dispatch.rs")).unwrap();
    for required_snippet in [
        "pub enum DispatchActorState",
        "pub enum RouteDecision",
        "pub enum ActorDispatchState",
        "pub enum ReopenMode",
        "pub struct RoutedReopenFacts",
        "pub struct RoutedReopenOutcome",
        "pub fn decide_authoritative_reopen(",
        "pub enum AuthoritativeActorDispatchAction",
        "pub struct AuthoritativeActorDispatchActionFacts",
        "pub fn classify_authoritative_actor_dispatch_action(",
        "pub fn dispatch_only_focus_only_should_fail_closed(",
        "pub struct PromptReadyBarrierFacts",
        "pub enum PromptReadyBarrierDecision",
        "pub fn classify_prompt_ready_barrier(",
        "pub struct AuthoritativeActorReadyFacts",
        "pub struct AuthoritativePromptReadyBarrierFacts",
        "pub fn classify_authoritative_prompt_ready_barrier(",
        "pub struct StartingActorLogFacts",
        "pub fn starting_actor_not_ready_log_line(",
        "pub fn starting_actor_ready_log_line(",
        "pub fn starting_actor_terminal_log_line(",
        "pub fn starting_actor_timeout_coalesced_log_line(",
        "pub const fn actor_start_wait_terminal_state(",
        "pub const fn actor_dispatch_blocker_reason(",
        "pub const fn actor_can_queue_optimistically(",
        "pub const fn busy_projection_repaired_by_ready_prompt(",
        "pub const fn actor_waiting_input_recoverable(",
        "pub fn actor_recovery_hint(",
        "pub enum BusyPaneAutoFixOutcome",
        "pub struct BusyPaneAutoFixFacts",
        "pub fn busy_existing_pane_auto_fix_outcome(",
        "pub struct DegradedAuthoritativeActorFacts",
        "pub fn can_use_degraded_authoritative_actor(",
        "pub struct DegradedAuthoritativeActorDirectSubmit",
        "pub fn degraded_authoritative_actor_direct_submit_log_message(",
        "pub enum RoutedReopenGuardReason",
        "pub fn is_interactive_shell_substate_reason(",
        "pub fn dispatch_only_blocked_guard_reason(",
        "pub enum ActorLifecycleState",
        "pub fn effective_authoritative_actor_state(",
        "pub enum DispatchRuntimeHealth",
        "pub struct AuthoritativeRuntimeFacts",
        "pub fn authoritative_actor_dispatch_guard_reason(",
        "pub enum RoutedDispatchStartProof",
        "pub enum DispatchStartProofDecision",
        "pub struct DispatchStartProofFacts",
        "pub fn classify_dispatch_start_proof(",
        "pub fn dispatch_only_dispatch_start_proof_required(",
        "pub struct RetryBudget",
        "pub fn authoritative_actor_ready_retry_budget(",
        "pub fn dispatch_only_starting_pane_ready_timeout_for_binary(",
        "pub fn dispatch_only_starting_pane_recovery_timeout_for_binary(",
        "pub fn dispatch_only_starting_pane_ready_retry_budget(",
        "pub fn dispatch_only_starting_pane_recovery_retry_budget(",
        "pub const STARTING_ACTOR_TIMEOUT_REASON",
        "pub struct StartingTimeoutActorFacts",
        "pub fn actor_blocked_by_starting_timeout(",
        "pub fn starting_timeout_blocked_actor_can_recover(",
        "pub struct StartupMissRouteFacts",
        "pub fn startup_miss_requires_fresh_start(",
        "pub fn startup_miss_superseded_by_later_open_start(",
        "pub fn startup_miss_should_restart_live_owner(",
        "pub fn startup_miss_should_fail_closed(",
        "pub enum FreshStartAckOutcome",
        "pub const fn fresh_start_ack_outcome(",
        "pub enum DirectPaneSubmitStatus",
        "pub fn direct_pane_submit_acceptance_timeout(",
        "pub fn direct_pane_submit_acceptance_budget(",
        "pub fn direct_pane_submit_outcome(",
        "pub struct DirectPaneDispatchStartProofFacts",
        "pub fn direct_pane_should_await_dispatch_start_proof(",
        "pub const DIRECT_PANE_EMPTY_ACCEPTANCE_STABLE_FOR",
        "pub struct DirectPaneAcceptancePollState",
        "pub fn direct_pane_acceptance_poll_status(",
        "pub const DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT",
        "pub struct DirectPaneEnterResubmitFacts",
        "pub fn direct_pane_needs_enter_resubmit(",
        "pub struct DirectPaneEnterResubmitAttemptFacts",
        "pub fn direct_pane_can_continue_enter_resubmit(",
        "pub struct DirectPaneExistingDraftSubmitFacts",
        "pub fn direct_pane_can_enter_existing_draft(",
        "pub fn dispatch_only_busy_should_wait_for_ready(",
        "pub fn dispatch_only_should_probe_active_turn_cue(",
        "pub enum DispatchDrainRetryDecision",
        "pub fn dispatch_drain_retry_decision(",
        "pub struct CloseoutBlockDispatchFacts",
        "pub enum CloseoutBlockDispatchDecision",
        "pub fn classify_closeout_block_dispatch(",
        "pub struct RoutedCycleAckFacts",
        "pub fn should_require_routed_cycle_ack(",
        "pub struct MissingCycleAckFacts",
        "pub fn should_optimistically_accept_missing_cycle_ack(",
        "pub enum RouteSubmitObservation",
        "pub struct RouteSubmitObservationFacts",
        "pub fn route_submit_observation_message(",
        "pub fn route_submit_issue_message(",
        "pub struct RoutedTriggerPayloadFacts",
        "pub fn routed_trigger_payload_rejection(",
        "pub struct DirectPaneResubmitProofFacts",
        "pub fn direct_pane_resubmit_proof_line(",
        "pub enum RouteLatencyStatus",
        "pub struct RouteLatencyFacts",
        "pub fn route_latency_status(",
        "pub fn route_latency_message(",
        "pub struct RouteStartupMissDiagnosticFacts",
        "pub fn route_startup_miss_diagnostic_message(",
        "pub struct RouteBusyDiagnosticFacts",
        "pub fn route_busy_diagnostic_message(",
        "pub struct RouteBusyQueuedDiagnosticFacts",
        "pub fn route_busy_queued_diagnostic_message(",
        "pub struct DuplicatePanePolicyErrorFacts",
        "pub fn duplicate_pane_policy_error_message(",
        "pub struct RouteDispatchBugReportItemFacts",
        "pub fn route_dispatch_bug_report_item(",
        "pub enum DispatchOnlyReopenDelivery",
        "pub struct DispatchOnlyProofOutcomeFacts",
        "pub const fn dispatch_only_should_print_unproven_progress(",
        "pub fn dispatch_only_sent_log_message(",
        "pub fn dispatch_only_sent_console_message(",
        "pub fn accepted_only_dispatch_start_log_message(",
        "pub fn accepted_only_dispatch_start_refusal_message(",
        "pub fn routed_dispatch_start_timeout(",
        "pub fn routed_dispatch_start_timeout_for_binary(",
        "pub fn fresh_route_start_ack_timeout(",
        "pub fn routed_cycle_ack_timeout(",
        "pub fn existing_pane_ready_timeout(",
        "pub struct DispatchOnlyBusyRefusalFacts",
        "pub fn dispatch_only_busy_refusal_message(",
    ] {
        assert!(
            controller_dispatch.contains(required_snippet),
            "agent-doc-controller should own route dispatch policy directly: {required_snippet}"
        );
    }
    for forbidden_snippet in [
        "pub enum ActorRuntimeHealth",
        "fn effective_authoritative_actor_state(",
        "pub struct AuthoritativeRuntimeFacts",
        "pub fn authoritative_actor_dispatch_guard_reason(",
        "fn busy_dispatch_only_should_wait_for_ready(",
        "fn dispatch_only_should_probe_active_turn_cue(",
        "fn dispatch_only_starting_pane_ready_timeout_for_binary(",
        "fn dispatch_only_starting_pane_recovery_timeout(",
        "fn actor_blocked_by_starting_timeout(",
        "fn starting_timeout_blocked_actor_can_recover(",
        "fn startup_miss_requires_fresh_start(",
        "fn startup_miss_superseded_by_later_open_start(",
        "fn startup_miss_should_restart_live_owner(",
        "fn startup_miss_should_fail_closed(",
        "fn direct_pane_submit_acceptance_timeout(",
        "fn direct_pane_submit_acceptance_budget(",
        "fn direct_pane_submit_outcome(",
        "fn direct_pane_should_await_dispatch_start_proof(",
        "const DIRECT_PANE_EMPTY_ACCEPTANCE_STABLE_FOR",
        "struct DirectPaneAcceptancePollState",
        "fn direct_pane_acceptance_poll_status(",
        "enum DrainRetryDecision",
        "fn classify_drain_retry(",
        "enum RouteCloseoutBlockDecision",
        "fn classify_closeout_block_dispatch(",
        "enum RouteSubmitObservation",
        "fn route_submit_observation_message(",
        "fn route_submit_issue_message(",
        "fn route_latency_status(",
        "fn route_latency_message(",
        "fn startup_miss_diagnostic_message(",
        "fn busy_route_diagnostic_message(",
        "fn busy_route_queued_diagnostic_message(",
        "fn format_duplicate_pane_policy_error(",
        "fn route_dispatch_bug_report_item(",
        "fn routed_dispatch_start_timeout(",
        "fn dispatch_only_busy_refusal_message(",
    ] {
        assert!(
            !route_source.contains(forbidden_snippet),
            "route.rs must not re-own pure controller dispatch policy: {forbidden_snippet}"
        );
    }
    for forbidden_snippet in [
        "pub enum RouteDecision",
        "pub enum ActorDispatchState",
        "pub enum ReopenMode",
        "pub struct RoutedReopenFacts",
        "pub struct RoutedReopenOutcome",
        "pub fn decide_authoritative_reopen(",
        "pub enum AuthoritativeActorDispatchAction",
        "pub struct AuthoritativeActorDispatchActionFacts",
        "pub fn classify_authoritative_actor_dispatch_action(",
        "pub fn dispatch_only_focus_only_should_fail_closed(",
        "pub struct PromptReadyBarrierFacts",
        "pub enum PromptReadyBarrierDecision",
        "pub fn classify_prompt_ready_barrier(",
        "pub struct AuthoritativeActorReadyFacts",
        "pub struct AuthoritativePromptReadyBarrierFacts",
        "pub fn classify_authoritative_prompt_ready_barrier(",
        "pub struct StartingActorLogFacts",
        "pub fn starting_actor_not_ready_log_line(",
        "pub fn starting_actor_ready_log_line(",
        "pub fn starting_actor_terminal_log_line(",
        "pub fn starting_actor_timeout_coalesced_log_line(",
        "pub const fn actor_start_wait_terminal_state(",
        "pub const fn actor_dispatch_blocker_reason(",
        "pub const fn actor_can_queue_optimistically(",
        "pub const fn busy_projection_repaired_by_ready_prompt(",
        "pub const fn actor_waiting_input_recoverable(",
        "pub fn actor_recovery_hint(",
        "pub enum BusyPaneAutoFixOutcome",
        "pub struct BusyPaneAutoFixFacts",
        "pub fn busy_existing_pane_auto_fix_outcome(",
        "pub struct DegradedAuthoritativeActorFacts",
        "pub fn can_use_degraded_authoritative_actor(",
        "pub struct DegradedAuthoritativeActorDirectSubmit",
        "pub fn degraded_authoritative_actor_direct_submit_log_message(",
        "pub enum RoutedReopenGuardReason",
        "pub fn is_interactive_shell_substate_reason(",
        "pub fn dispatch_only_blocked_guard_reason(",
        "pub enum ActorRuntimeHealth",
        "pub enum ActorLifecycleState",
        "pub fn effective_authoritative_actor_state(",
        "pub struct AuthoritativeRuntimeFacts",
        "pub fn authoritative_actor_dispatch_guard_reason(",
        "pub enum RoutedDispatchStartProof",
        "pub enum DispatchStartProofDecision",
        "pub struct DispatchStartProofFacts",
        "pub fn classify_dispatch_start_proof(",
        "pub fn dispatch_only_dispatch_start_proof_required(",
        "pub struct RetryBudget",
        "pub fn authoritative_actor_ready_retry_budget(",
        "pub fn dispatch_only_starting_pane_ready_timeout_for_binary(",
        "pub fn dispatch_only_starting_pane_recovery_timeout_for_binary(",
        "pub fn dispatch_only_starting_pane_ready_retry_budget(",
        "pub fn dispatch_only_starting_pane_recovery_retry_budget(",
        "pub const STARTING_ACTOR_TIMEOUT_REASON",
        "pub struct StartingTimeoutActorFacts",
        "pub fn actor_blocked_by_starting_timeout(",
        "pub fn starting_timeout_blocked_actor_can_recover(",
        "pub struct StartupMissRouteFacts",
        "pub fn startup_miss_requires_fresh_start(",
        "pub fn startup_miss_superseded_by_later_open_start(",
        "pub fn startup_miss_should_restart_live_owner(",
        "pub fn startup_miss_should_fail_closed(",
        "pub enum DirectPaneSubmitStatus",
        "pub fn direct_pane_submit_acceptance_timeout(",
        "pub fn direct_pane_submit_acceptance_budget(",
        "pub fn direct_pane_submit_outcome(",
        "pub const DIRECT_PANE_EMPTY_ACCEPTANCE_STABLE_FOR",
        "pub struct DirectPaneAcceptancePollState",
        "pub fn direct_pane_acceptance_poll_status(",
        "pub struct CloseoutBlockDispatchFacts",
        "pub enum CloseoutBlockDispatchDecision",
        "pub fn classify_closeout_block_dispatch(",
        "pub struct RoutedCycleAckFacts",
        "pub fn should_require_routed_cycle_ack(",
        "pub struct MissingCycleAckFacts",
        "pub fn should_optimistically_accept_missing_cycle_ack(",
        "pub enum RouteSubmitObservation",
        "pub struct RouteSubmitObservationFacts",
        "pub fn route_submit_observation_message(",
        "pub fn route_submit_issue_message(",
        "pub enum DispatchOnlyReopenDelivery",
        "pub struct DispatchOnlyProofOutcomeFacts",
        "pub fn dispatch_only_sent_log_message(",
        "pub fn dispatch_only_sent_console_message(",
        "pub fn accepted_only_dispatch_start_log_message(",
        "pub fn accepted_only_dispatch_start_refusal_message(",
        "pub fn should_print_dispatch_only_unproven_progress(",
        "pub fn routed_dispatch_start_timeout(",
        "pub fn routed_dispatch_start_timeout_for_binary(",
        "pub fn fresh_route_start_ack_timeout(",
        "pub fn routed_cycle_ack_timeout(",
        "pub fn existing_pane_ready_timeout(",
        "pub enum RouteLatencyStatus",
        "pub struct RouteLatencyFacts",
        "pub fn route_latency_status(",
        "pub fn route_latency_message(",
    ] {
        assert!(
            !flow_routed_reopen_source.contains(forbidden_snippet),
            "flow::routed_reopen must not re-own pure controller dispatch policy: {forbidden_snippet}"
        );
    }
    assert!(
        !flow_types_source.contains("pub enum RouteDecision"),
        "flow::types must not keep route decision policy after it moves to agent-doc-controller"
    );
    assert!(
        route_source.contains("use agent_doc_controller::dispatch::{")
            && route_source.contains("ActorDispatchState")
            && route_source.contains("ReopenMode")
            && route_source.contains("RoutedReopenFacts")
            && route_source.contains("decide_authoritative_reopen")
            && route_source.contains("AuthoritativeActorDispatchAction")
            && route_source.contains("AuthoritativeActorDispatchActionFacts")
            && route_source.contains("classify_authoritative_actor_dispatch_action")
            && route_source.contains("PromptReadyBarrierDecision")
            && route_source.contains("AuthoritativeActorReadyFacts")
            && route_source.contains("AuthoritativePromptReadyBarrierFacts")
            && route_source.contains("classify_authoritative_prompt_ready_barrier")
            && route_source.contains("RoutedReopenGuardReason")
            && route_source.contains("dispatch_only_blocked_guard_reason")
            && route_source.contains("ActorLifecycleState")
            && route_source.contains("effective_authoritative_actor_state")
            && route_source.contains("DispatchRuntimeHealth")
            && route_source.contains("controller_authoritative_actor_dispatch_guard_reason(")
            && route_source.contains("RoutedDispatchStartProof")
            && route_source.contains("classify_dispatch_start_proof")
            && route_source.contains("DirectPaneSubmitStatus as CommandDispatchStatus")
            && route_source.contains("direct_pane_submit_outcome")
            && route_source.contains("DirectPaneDispatchStartProofFacts")
            && route_source.contains("direct_pane_should_await_dispatch_start_proof")
            && route_source.contains("DirectPaneAcceptancePollState")
            && route_source.contains("direct_pane_acceptance_poll_status")
            && route_source.contains("DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT")
            && route_source.contains("DirectPaneEnterResubmitAttemptFacts")
            && route_source.contains("DirectPaneExistingDraftSubmitFacts")
            && route_source.contains("direct_pane_can_continue_enter_resubmit")
            && route_source.contains("direct_pane_can_enter_existing_draft")
            && route_source.contains("RetryBudget")
            && route_source.contains("authoritative_actor_ready_retry_budget")
            && route_source.contains("CloseoutBlockDispatchDecision")
            && route_source.contains("CloseoutBlockDispatchFacts")
            && route_source.contains("classify_closeout_block_dispatch")
            && route_source.contains("dispatch_only_starting_pane_ready_timeout_for_binary")
            && route_source.contains("dispatch_only_starting_pane_recovery_retry_budget")
            && route_source.contains("dispatch_only_starting_pane_recovery_timeout_for_binary")
            && route_source.contains("STARTING_ACTOR_TIMEOUT_REASON")
            && route_source.contains("StartingTimeoutActorFacts")
            && route_source.contains("actor_blocked_by_starting_timeout")
            && route_source.contains("starting_timeout_blocked_actor_can_recover")
            && route_source.contains("StartupMissRouteFacts")
            && route_source.contains("startup_miss_requires_fresh_start")
            && route_source.contains("startup_miss_superseded_by_later_open_start")
            && route_source.contains("startup_miss_should_restart_live_owner")
            && route_source.contains("startup_miss_should_fail_closed")
            && route_source.contains("RoutedCycleAckFacts")
            && route_source.contains("should_require_routed_cycle_ack")
            && route_source.contains("MissingCycleAckFacts")
            && route_source.contains("should_optimistically_accept_missing_cycle_ack")
            && route_source.contains("RouteSubmitObservation")
            && route_source.contains("ControllerRouteSubmitObservationFacts")
            && route_source.contains("route_submit_observation_message(")
            && route_source.contains("route_submit_issue_message(")
            && route_source.contains("RoutedTriggerPayloadFacts")
            && route_source.contains("routed_trigger_payload_rejection")
            && route_source.contains("DirectPaneResubmitProofFacts")
            && route_source.contains("direct_pane_resubmit_proof_line")
            && route_source.contains("RouteLatencyFacts")
            && route_source.contains("RouteLatencyStatus")
            && route_source.contains("route_latency_message(")
            && route_source.contains("route_latency_status(")
            && route_source.contains("RouteStartupMissDiagnosticFacts")
            && route_source.contains("route_startup_miss_diagnostic_message(")
            && route_source.contains("RouteBusyDiagnosticFacts")
            && route_source.contains("route_busy_diagnostic_message(")
            && route_source.contains("RouteBusyQueuedDiagnosticFacts")
            && route_source.contains("route_busy_queued_diagnostic_message(")
            && route_source.contains("RouteDispatchBugReportItemFacts")
            && route_source.contains("route_dispatch_bug_report_item(")
            && route_source.contains("DispatchOnlyReopenDelivery")
            && route_source.contains("DispatchOnlyProofOutcomeFacts")
            && route_source.contains("dispatch_only_sent_log_message")
            && route_source.contains("dispatch_only_sent_console_message")
            && route_source.contains("accepted_only_dispatch_start_log_message")
            && route_source.contains("accepted_only_dispatch_start_refusal_message")
            && route_source.contains("dispatch_only_should_print_unproven_progress")
            && route_source.contains("routed_dispatch_start_timeout_for_binary")
            && route_source.contains("fresh_route_start_ack_timeout")
            && route_source.contains("routed_cycle_ack_timeout")
            && route_source.contains("existing_pane_ready_timeout")
            && route_source.contains("DispatchOnlyBusyRefusalFacts")
            && route_source.contains("controller_dispatch_only_busy_refusal_message(")
            && route_source.contains("DispatchActorState")
            && route_source.contains("dispatch_only_busy_should_wait_for_ready(")
            && route_source.contains("dispatch_only_should_probe_active_turn_cue(")
            && route_source.contains("DispatchDrainRetryDecision")
            && route_source.contains("dispatch_drain_retry_decision("),
        "route.rs should call focused controller dispatch policy directly"
    );
    for forbidden_snippet in [
        "const DIRECT_PANE_EMPTY_ACCEPTANCE_STABLE_FOR",
        "struct DirectPaneAcceptancePollState",
        "fn direct_pane_acceptance_poll_status(",
        "const DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT",
        "fn direct_pane_needs_enter_resubmit(",
        "fn direct_pane_can_continue_enter_resubmit(",
        "fn direct_pane_can_enter_existing_draft(",
        "fn direct_pane_should_await_dispatch_start_proof(",
        "fn resubmit_result_label(",
        "fn route_submit_resubmit_proof_line(",
        "fn routed_trigger_payload(",
        "fn validate_routed_trigger_payload(",
    ] {
        assert!(
            !route_dispatch_source.contains(forbidden_snippet),
            "route/dispatch.rs must not re-own direct-pane controller policy: {forbidden_snippet}"
        );
    }
    assert!(
        route_dispatch_source.contains("DirectPaneAcceptancePollState::default()")
            && route_dispatch_source.contains("direct_pane_acceptance_poll_status(")
            && route_dispatch_source.contains(".saw_trigger_visible()")
            && route_dispatch_source.contains("DirectPaneEnterResubmitAttemptFacts")
            && route_dispatch_source.contains("direct_pane_can_continue_enter_resubmit(")
            && route_dispatch_source.contains("DirectPaneExistingDraftSubmitFacts")
            && route_dispatch_source.contains("direct_pane_can_enter_existing_draft(")
            && route_dispatch_source.contains("DirectPaneDispatchStartProofFacts")
            && route_dispatch_source.contains("direct_pane_should_await_dispatch_start_proof(")
            && route_dispatch_source.contains("DirectPaneResubmitProofFacts")
            && route_dispatch_source.contains("direct_pane_resubmit_proof_line(")
            && route_dispatch_source.contains("routed_dispatch_start_timeout_for_binary(")
            && route_dispatch_source.contains("Some(harness.binary.as_str())")
            && route_dispatch_source.contains("cfg!(test)")
            && route_dispatch_source.contains("RoutedTriggerPayloadFacts")
            && route_dispatch_source.contains("routed_trigger_payload_rejection("),
        "route/dispatch.rs should adapt tmux captures into focused controller direct-pane policy"
    );
    for forbidden_snippet in [
        "fn route_dispatch_only_sent_log_message(",
        "fn route_dispatch_only_sent_console_message(",
        "pub fn dispatch_only_sent_log_message(",
        "pub fn accepted_only_dispatch_start_refusal_message(",
    ] {
        assert!(
            !route_dispatch_only_source.contains(forbidden_snippet),
            "route/dispatch_only.rs must not re-own dispatch-only proof outcome policy: {forbidden_snippet}"
        );
    }
    assert!(
        route_dispatch_only_source.contains("DispatchOnlyProofOutcomeFacts")
            && route_dispatch_only_source.contains("dispatch_only_sent_log_message(")
            && route_dispatch_only_source.contains("dispatch_only_sent_console_message(")
            && route_dispatch_only_source.contains("accepted_only_dispatch_start_log_message(")
            && route_dispatch_only_source.contains("accepted_only_dispatch_start_refusal_message(")
            && route_dispatch_only_source.contains("routed_dispatch_start_timeout_for_binary(")
            && route_dispatch_only_source.contains("Some(harness.binary.as_str())")
            && route_dispatch_only_source.contains("cfg!(test)")
            && route_dispatch_only_source.contains("dispatch_only_should_print_unproven_progress("),
        "route/dispatch_only.rs should adapt route facts into focused controller dispatch-only proof policy"
    );
    for forbidden_snippet in [
        "pub(crate) fn fresh_route_start_ack_timeout(",
        "pub(crate) fn routed_cycle_ack_timeout(",
    ] {
        assert!(
            !route_cycle_ack_source.contains(forbidden_snippet),
            "route/cycle_ack.rs must not wrap focused route timeout policy: {forbidden_snippet}"
        );
    }
    assert!(
        route_cycle_ack_source.contains("fresh_route_start_ack_timeout(cfg!(test))")
            && route_cycle_ack_source
                .contains("routed_cycle_ack_timeout(live_child_for_file, cfg!(test))"),
        "route/cycle_ack.rs should pass route/test facts into focused controller timeout policy"
    );
    assert!(
        !route_busy_pane_source.contains("fn existing_pane_ready_timeout(")
            && route_busy_pane_source.contains("existing_pane_ready_timeout(cfg!(test))")
            && route_busy_pane_source.contains("fresh_route_start_ack_timeout(cfg!(test))"),
        "route/busy_pane.rs should pass route/test facts into focused controller timeout policy without wrappers"
    );
    assert!(
        route_pane_resolution_source.contains("startup_miss_route_facts(")
            && route_pane_resolution_source.contains("startup_miss_requires_fresh_start(")
            && route_pane_resolution_source
                .contains("startup_miss_superseded_by_later_open_start(")
            && route_pane_resolution_source.contains("startup_miss_should_restart_live_owner(")
            && route_pane_resolution_source.contains("startup_miss_should_fail_closed("),
        "route pane resolution should adapt startup-miss sidecars into focused controller policy"
    );
    assert!(
        route_startup_source.contains("DuplicatePanePolicyErrorFacts")
            && route_startup_source.contains("duplicate_pane_policy_error_message("),
        "route startup should adapt tmux/session facts into focused duplicate-pane diagnostic policy"
    );
    for forbidden_snippet in [
        "pub(crate) enum FreshStartAckOutcome",
        "pub(crate) fn fresh_start_ack_outcome(",
    ] {
        assert!(
            !route_startup_source.contains(forbidden_snippet),
            "route/startup.rs must not re-own fresh-start ack policy: {forbidden_snippet}"
        );
    }
    assert!(
        route_startup_source.contains(
            "use agent_doc_controller::dispatch::{FreshStartAckOutcome, fresh_start_ack_outcome};"
        ) && route_startup_source.contains(
            "fresh_start_ack_outcome(false, ready_prompt_candidate(&content, harness).is_some())"
        ),
        "route/startup.rs should adapt pane prompt detection into focused fresh-start ack policy"
    );
    for forbidden_snippet in [
        "pub(crate) fn should_require_routed_cycle_ack(",
        "pub(crate) fn should_optimistically_accept_missing_cycle_ack(",
    ] {
        assert!(
            !route_cycle_ack_source.contains(forbidden_snippet),
            "route/cycle_ack.rs must not re-own pure controller dispatch policy: {forbidden_snippet}"
        );
    }
    assert!(
        route_cycle_ack_source.contains("RoutedCycleAckFacts")
            && route_cycle_ack_source.contains("should_require_routed_cycle_ack(")
            && route_cycle_ack_source.contains("MissingCycleAckFacts")
            && route_cycle_ack_source.contains("should_optimistically_accept_missing_cycle_ack("),
        "route/cycle_ack.rs should adapt cycle and harness facts into focused controller policy"
    );
    let sim_world = fs::read_to_string(manifest_dir.join("src/sim_world/engine.rs")).unwrap();
    assert!(
        sim_world.contains("agent_doc_controller::dispatch::dispatch_should_coalesce_in_flight"),
        "SimWorld should share the focused controller dispatch classifier directly"
    );
    let controller_claim =
        fs::read_to_string(manifest_dir.join("agent-doc-controller/src/claim.rs")).unwrap();
    for required_snippet in [
        "pub enum CrossSessionDecision",
        "pub const CROSS_SESSION_REJECT_MARKER",
        "pub fn cross_session_reject_marker(",
        "pub fn cross_session_decision(",
        "pub fn cross_session_decision_with_lease(",
    ] {
        assert!(
            controller_claim.contains(required_snippet),
            "agent-doc-controller should own claim admission policy directly: {required_snippet}"
        );
    }
    let claim_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/claim.rs")).unwrap();
    for forbidden_snippet in [
        "pub enum CrossSessionDecision",
        "pub const CROSS_SESSION_REJECT_MARKER",
        "pub fn cross_session_reject_marker(",
        "pub fn cross_session_decision(",
        "pub fn cross_session_decision_with_lease(",
    ] {
        assert!(
            !claim_source.contains(forbidden_snippet),
            "claim.rs must stay a tmux/file adapter and not re-own controller claim policy: {forbidden_snippet}"
        );
    }
    assert!(
        claim_source.contains("use agent_doc_controller::claim::{"),
        "claim.rs should call focused controller claim policy directly"
    );
    let controller_operator_clear =
        fs::read_to_string(manifest_dir.join("agent-doc-controller/src/operator_clear.rs"))
            .unwrap();
    for required_snippet in [
        "pub enum OperatorClearInputState",
        "pub enum OperatorClearGuardOutcome",
        "pub const fn clear_guard_outcome",
    ] {
        assert!(
            controller_operator_clear.contains(required_snippet),
            "agent-doc-controller should own operator-clear guard policy directly: {required_snippet}"
        );
    }
    let flow_operator_clear =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/flow/operator_clear.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub enum OperatorClearInputState",
        "pub enum OperatorClearGuardOutcome",
        "pub fn clear_guard_outcome",
        "pub const fn clear_guard_outcome",
    ] {
        assert!(
            !flow_operator_clear.contains(forbidden_snippet),
            "flow::operator_clear must stay a flow adapter and not re-own operator-clear policy: {forbidden_snippet}"
        );
    }
    assert!(
        flow_operator_clear.contains(
            "use agent_doc_controller::operator_clear::{OperatorClearGuardOutcome, OperatorClearInputState};"
        ) && flow_operator_clear.contains(
            "agent_doc_controller::operator_clear::clear_guard_outcome"
        ),
        "flow::operator_clear should call focused operator-clear policy directly"
    );
    let session_actor_cmd_source =
        fs::read_to_string(manifest_dir.join("src/session_actor_cmd.rs")).unwrap();
    assert!(
        session_actor_cmd_source
            .contains("use agent_doc_controller::operator_clear::OperatorClearInputState;")
            && session_actor_cmd_source
                .contains("use agent_doc_controller::operator_clear::OperatorClearGuardOutcome;")
            && !session_actor_cmd_source
                .contains("agent_doc_orchestration::flow::operator_clear::OperatorClearInputState"),
        "session actor commands should import focused operator-clear policy directly"
    );

    let controller_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-controller/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&controller_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-controller must stay free of core, orchestration, git, editor IPC, sqlite, or tmux crates"
        );
    }
}

#[test]
fn test_agent_doc_controller_owns_editor_route_error_naming_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let controller_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-controller/src/lib.rs")).unwrap();
    let controller_policy =
        fs::read_to_string(manifest_dir.join("agent-doc-controller/src/editor_route_error.rs"))
            .unwrap();
    let orchestration_adapter =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/editor_route_errors.rs"))
            .unwrap();
    let orchestration_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/lib.rs")).unwrap();

    assert!(
        controller_lib.contains("pub mod editor_route_error;"),
        "agent-doc-controller should expose editor route-error naming policy as a focused module"
    );
    for required in [
        "pub const EDITOR_ROUTE_ERROR_DIAGNOSTICS_DIR",
        "pub fn editor_route_error_diagnostic_name(",
        "pub fn editor_route_error_file_name(",
    ] {
        assert!(
            controller_policy.contains(required),
            "agent-doc-controller must own editor route-error naming policy: {required}"
        );
    }
    for forbidden in [
        "fn sanitized_route_error_name(",
        "pub fn editor_route_error_diagnostic_name(",
        "pub fn editor_route_error_file_name(",
        "pub const EDITOR_ROUTE_ERROR_DIAGNOSTICS_DIR",
        "pub use agent_doc_controller::editor_route_error",
    ] {
        assert!(
            !orchestration_adapter.contains(forbidden),
            "orchestration editor route-error adapter must not re-own or facade naming policy: {forbidden}"
        );
        assert!(
            !orchestration_lib.contains(forbidden),
            "orchestration lib must not facade editor route-error naming policy: {forbidden}"
        );
    }
    assert!(
        orchestration_adapter.contains("use agent_doc_controller::editor_route_error::{")
            && orchestration_adapter.contains("EDITOR_ROUTE_ERROR_DIAGNOSTICS_DIR")
            && orchestration_adapter.contains("editor_route_error_file_name(&relative)"),
        "orchestration should import focused editor route-error naming policy directly"
    );
}

#[test]
fn test_agent_doc_controller_owns_fleet_dashboard_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let controller_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-controller/src/lib.rs")).unwrap();
    let controller_fleet =
        fs::read_to_string(manifest_dir.join("agent-doc-controller/src/fleet.rs")).unwrap();

    assert!(
        controller_lib.contains("pub mod fleet;"),
        "agent-doc-controller should expose fleet dashboard policy as a focused module"
    );
    for required in [
        "pub struct AdminActor",
        "pub struct AdminFinding",
        "pub struct DashboardRow",
        "pub struct DashboardActorDiagnostics",
        "pub struct DashboardModel",
        "pub fn build_dashboard_model(",
        "pub fn build_dashboard_model_with_diagnostics(",
        "pub fn render_dashboard(",
    ] {
        assert!(
            controller_fleet.contains(required),
            "agent-doc-controller must own fleet dashboard policy: {required}"
        );
    }

    let orchestration_admin =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/admin.rs")).unwrap();
    let orchestration_dashboard =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/dashboard.rs")).unwrap();
    for forbidden in [
        "pub struct AdminActor",
        "pub struct AdminFinding",
        "pub struct DashboardRow",
        "pub struct DashboardActorDiagnostics",
        "pub struct DashboardModel",
        "pub fn build_dashboard_model(",
        "pub fn build_dashboard_model_with_diagnostics(",
        "pub fn render_dashboard(",
        "fn truncate(",
        "fn queue_summary(",
        "fn row_flags(",
        "pub use agent_doc_controller::fleet",
        "pub(crate) use agent_doc_controller::fleet",
    ] {
        assert!(
            !orchestration_admin.contains(forbidden)
                && !orchestration_dashboard.contains(forbidden),
            "orchestration must not re-own or facade fleet dashboard policy: {forbidden}"
        );
    }
    assert!(
        orchestration_admin
            .contains("use agent_doc_controller::fleet::{AdminActor, AdminFinding};"),
        "admin should construct focused fleet actor/finding rows directly"
    );
    assert!(
        orchestration_dashboard.contains("use agent_doc_controller::fleet::{")
            && orchestration_dashboard.contains("build_dashboard_model_with_diagnostics")
            && orchestration_dashboard.contains("render_dashboard"),
        "dashboard should call focused fleet model/rendering policy directly"
    );

    let controller_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-controller/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&controller_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-controller fleet policy must stay free of core, orchestration, git, editor IPC, sqlite, or tmux crates"
        );
    }
}

#[test]
fn test_agent_doc_turn_executor_owns_capability_proof_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let workspace: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let package_version = workspace["package"]["version"].as_str();

    let executor_policy =
        fs::read_to_string(manifest_dir.join("agent-doc-turn-executor/src/capability_proof.rs"))
            .unwrap();
    let executor_auto_trigger =
        fs::read_to_string(manifest_dir.join("agent-doc-turn-executor/src/auto_trigger.rs"))
            .unwrap();
    let executor_codex_launch =
        fs::read_to_string(manifest_dir.join("agent-doc-turn-executor/src/codex_launch.rs"))
            .unwrap();
    let executor_agent_stream =
        fs::read_to_string(manifest_dir.join("agent-doc-turn-executor/src/agent_stream.rs"))
            .unwrap();
    let executor_capture =
        fs::read_to_string(manifest_dir.join("agent-doc-turn-executor/src/capture.rs")).unwrap();
    for required_snippet in [
        "pub struct ManagedProofPolicy",
        "pub struct ManagedProofPolicyInputs",
        "pub enum ProofRetryDecision",
        "pub fn resolve_managed_proof_policy(",
        "pub fn proof_retry_decision(",
        "pub fn managed_capability_proof_status_message(",
    ] {
        assert!(
            executor_policy.contains(required_snippet),
            "agent-doc-turn-executor should own capability-proof policy directly: {required_snippet}"
        );
    }
    for required_snippet in [
        "pub struct AutoTriggerMonitor",
        "pub enum AutoTriggerStopOutcome",
        "pub enum AutoTriggerCooldownAction",
        "pub enum AutoTriggerNoPromptAction",
        "pub fn auto_trigger_clear_cooldown_action(",
        "pub fn auto_trigger_no_prompt_action(",
    ] {
        assert!(
            executor_auto_trigger.contains(required_snippet),
            "agent-doc-turn-executor should own auto-trigger readiness policy directly: {required_snippet}"
        );
    }
    for required_snippet in [
        "pub const CODEX_SANDBOX_NETWORK_DISABLED_ENV",
        "pub enum CodexResumeRestartArgsError",
        "pub struct CodexNetworkPolicyStatus",
        "pub fn resolve_codex_network_access(",
        "pub fn apply_codex_network_access_env_map(",
        "pub fn apply_codex_network_access_env_overrides(",
        "pub fn codex_network_status_from_env_map(",
        "pub fn codex_network_status_from_overrides(",
        "pub fn codex_resume_restart_args(",
        "pub fn looks_like_codex_transport_403_429(",
        "pub fn codex_transport_403_429_diagnostic(",
        "pub struct CodexStderrNoiseReport",
        "pub fn codex_stderr_noise_report(",
        "pub fn filter_codex_stderr_noise(",
        "pub fn classify_child_network_probe_failure(",
        "pub fn classify_child_required_ssh_probe_failure(",
        "pub fn classify_child_writable_root_probe_failure(",
        "pub fn looks_like_local_browser_cdp_permission_denied(",
        "pub fn resume_capability_drift_notice(",
        "pub fn looks_like_ssh_dns_failure(",
        "pub fn looks_like_ssh_network_failure(",
        "pub fn looks_like_ssh_auth_failure(",
        "pub fn looks_like_ssh_alias_config_failure(",
        "pub fn transcript_has_required_ssh_failure(",
        "pub fn transcript_proves_required_ssh_success(",
        "pub fn format_required_ssh_failure(",
    ] {
        assert!(
            executor_codex_launch.contains(required_snippet),
            "agent-doc-turn-executor should own Codex launch/transport policy directly: {required_snippet}"
        );
    }
    for required_snippet in [
        "pub struct StreamChunk",
        "pub fn parse_stream_line(",
        "pub fn parse_codex_line(",
    ] {
        assert!(
            executor_agent_stream.contains(required_snippet),
            "agent-doc-turn-executor should own agent stream parsing directly: {required_snippet}"
        );
    }
    for required_snippet in [
        "pub fn capture_delta(",
        "pub fn limit_capture_lines(",
        "capture_delta_returns_from_first_modified_line",
    ] {
        assert!(
            executor_capture.contains(required_snippet),
            "agent-doc-turn-executor should own executor capture-delta policy directly: {required_snippet}"
        );
    }

    let orchestration_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/Cargo.toml")).unwrap();
    let orchestration: toml::Value = toml::from_str(&orchestration_manifest).unwrap();
    let orchestration_dependencies = orchestration["dependencies"].as_table().unwrap();
    let dependency = orchestration_dependencies["agent-doc-turn-executor"]
        .as_table()
        .unwrap();
    assert_eq!(
        dependency.get("path").and_then(toml::Value::as_str),
        Some("../agent-doc-turn-executor")
    );
    assert_eq!(
        dependency.get("version").and_then(toml::Value::as_str),
        package_version
    );

    let agent_mod =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/agent/mod.rs")).unwrap();
    for forbidden_snippet in [
        "pub struct ManagedProofPolicy",
        "pub enum ProofRetryDecision",
        "pub fn resolve_managed_proof_policy(",
        "pub fn proof_retry_decision(",
        "DEFAULT_MANAGED_PROOF",
        "MAX_MANAGED_PROOF",
        "pub const CODEX_SANDBOX_NETWORK_DISABLED_ENV",
        "pub struct CodexNetworkPolicyStatus",
        "pub fn resolve_codex_network_access(",
        "pub fn apply_codex_network_access_env_map(",
        "pub fn apply_codex_network_access_env_overrides(",
        "pub fn codex_network_status_from_env_map(",
        "pub fn codex_network_status_from_overrides(",
    ] {
        assert!(
            !agent_mod.contains(forbidden_snippet),
            "agent::mod must not re-own capability-proof or Codex network launch policy: {forbidden_snippet}"
        );
    }

    let start =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/start.rs")).unwrap();
    for forbidden_snippet in [
        "struct AutoTriggerMonitor",
        "enum AutoTriggerCooldownAction",
        "enum AutoTriggerNoPromptAction",
        "fn auto_trigger_clear_cooldown_action(",
        "fn auto_trigger_no_prompt_action(",
        "fn managed_capability_proof_status_message(",
    ] {
        assert!(
            !start.contains(forbidden_snippet),
            "start.rs must not re-own pure auto-trigger readiness policy: {forbidden_snippet}"
        );
    }
    assert!(
        start.contains("agent_doc_turn_executor::capability_proof::resolve_managed_proof_policy")
            && start.contains("agent_doc_turn_executor::capability_proof::proof_retry_decision")
            && start.contains(
                "use agent_doc_turn_executor::capability_proof::managed_capability_proof_status_message;"
            )
            && start.contains("agent_doc_turn_executor::auto_trigger::{"),
        "start should call focused turn-executor policy directly"
    );
    let harness =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/harness.rs")).unwrap();
    for forbidden_snippet in [
        "fn parse_sandbox_mode_config(",
        "fn record_codex_resume_sandbox_mode(",
        "fn push_codex_resume_sandbox_config(",
        "fn codex_resume_restart_args(",
    ] {
        assert!(
            !harness.contains(forbidden_snippet),
            "harness.rs must not re-own pure Codex resume launch policy: {forbidden_snippet}"
        );
    }
    assert!(
        harness.contains("use agent_doc_turn_executor::codex_launch::codex_resume_restart_args;"),
        "harness.rs should call focused Codex resume launch policy directly"
    );
    let codex = fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/agent/codex.rs"))
        .unwrap();
    for forbidden_snippet in [
        "fn looks_like_codex_transport_403_429(",
        "fn append_resume_args(",
        "fn codex_resume_restart_args(",
        "fn format_transport_403_429_diagnostic(",
        "fn codex_transport_403_429_diagnostic(",
        "struct CodexStderrNoiseReport",
        "fn is_codex_marketplace_manifest_noise(",
        "fn codex_stderr_noise_report(",
        "fn filter_codex_stderr_noise(",
        "fn classify_child_network_probe_failure(",
        "fn classify_child_required_ssh_probe_failure(",
        "fn classify_child_writable_root_probe_failure(",
        "fn looks_like_local_browser_cdp_permission_denied(",
        "fn resume_capability_drift_notice(",
        "fn looks_like_ssh_dns_failure(",
        "fn looks_like_ssh_network_failure(",
        "fn looks_like_ssh_auth_failure(",
        "fn looks_like_ssh_alias_config_failure(",
        "fn transcript_has_required_ssh_failure(",
        "fn transcript_proves_required_ssh_success(",
        "fn format_required_ssh_failure(",
    ] {
        assert!(
            !codex.contains(forbidden_snippet),
            "agent::codex must not re-own Codex launch/transport policy: {forbidden_snippet}"
        );
    }
    assert!(
        codex.contains("use agent_doc_turn_executor::codex_launch::{")
            && codex.contains("looks_like_codex_transport_403_429")
            && codex.contains("codex_transport_403_429_diagnostic")
            && codex.contains("filter_codex_stderr_noise")
            && codex.contains("classify_child_network_probe_failure")
            && codex.contains("classify_child_required_ssh_probe_failure")
            && codex.contains("classify_child_writable_root_probe_failure")
            && codex.contains("codex_resume_restart_args")
            && codex.contains("looks_like_local_browser_cdp_permission_denied")
            && codex.contains("resolve_codex_network_access")
            && codex.contains("resume_capability_drift_notice")
            && codex.contains("transcript_has_required_ssh_failure")
            && codex.contains("transcript_proves_required_ssh_success")
            && codex.contains("format_required_ssh_failure"),
        "agent::codex should call focused Codex launch/transport policy directly"
    );
    assert!(
        !codex.contains("super::resolve_codex_network_access("),
        "agent::codex must not call obsolete orchestration Codex network policy"
    );
    assert!(
        codex.contains(
            "agent_doc_turn_executor::capability_proof::DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT"
        ),
        "codex probe tests should use the focused capability-proof defaults directly"
    );
    let streaming =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/agent/streaming.rs"))
            .unwrap();
    let claude =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/agent/claude.rs"))
            .unwrap();
    let stream =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/stream.rs")).unwrap();
    for (name, source) in [
        ("agent/streaming.rs", streaming.as_str()),
        ("agent/codex.rs", codex.as_str()),
    ] {
        for forbidden_snippet in [
            "pub struct StreamChunk",
            "pub fn parse_stream_line(",
            "pub fn parse_codex_line(",
            "pub use",
        ] {
            assert!(
                !source.contains(forbidden_snippet),
                "{name} must not define or reexport focused stream parser API: {forbidden_snippet}"
            );
        }
    }
    assert!(
        streaming.contains("use agent_doc_turn_executor::agent_stream::StreamChunk;")
            && claude.contains("agent_doc_turn_executor::agent_stream::{")
            && claude.contains("parse_stream_line")
            && codex.contains("agent_doc_turn_executor::agent_stream::{")
            && codex.contains("parse_codex_line")
            && stream.contains("use agent_doc_turn_executor::agent_stream::StreamChunk;"),
        "orchestration should call focused agent stream parsing/chunk APIs directly"
    );
    let watch =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/watch.rs")).unwrap();
    for forbidden_snippet in ["fn extract_new_lines(", "fn limit_lines("] {
        assert!(
            !watch.contains(forbidden_snippet),
            "watch.rs must not re-own pure executor capture policy: {forbidden_snippet}"
        );
    }
    assert!(
        watch.contains(
            "use agent_doc_turn_executor::capture::{capture_delta, limit_capture_lines};"
        ) && watch.contains("capture_delta(&ss.last_capture, &captured)")
            && watch.contains("limit_capture_lines(&new_content, ss.max_lines)"),
        "watch.rs should call focused executor capture policy directly"
    );

    let executor_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-turn-executor/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&executor_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-turn-executor must stay pure and free of core, orchestration, git, editor IPC, sqlite, or tmux crates"
        );
    }
}

#[test]
fn test_agent_doc_supervisor_policy_has_no_start_decisions_facade() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        manifest_dir
            .join("agent-doc-supervisor/src/lifecycle.rs")
            .exists(),
        "agent-doc-supervisor must own supervisor lifecycle policy"
    );
    assert!(
        manifest_dir
            .join("agent-doc-supervisor/src/agent_change.rs")
            .exists(),
        "agent-doc-supervisor must own harness-change policy"
    );
    assert!(
        manifest_dir
            .join("agent-doc-supervisor/src/run_loop.rs")
            .exists(),
        "agent-doc-supervisor must own pure run-loop exit dispatch"
    );
    assert!(
        manifest_dir
            .join("agent-doc-supervisor/src/selfkill.rs")
            .exists(),
        "agent-doc-supervisor must own pure supervisor self-kill policy"
    );
    assert!(
        manifest_dir
            .join("agent-doc-supervisor/src/crash_policy.rs")
            .exists(),
        "agent-doc-supervisor must own child crash/restart policy"
    );
    assert!(
        manifest_dir
            .join("agent-doc-supervisor/src/idle_reconcile.rs")
            .exists(),
        "agent-doc-supervisor must own supervisor idle/ready reconcile policy"
    );
    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/start/decisions.rs")
            .exists(),
        "orchestration must not keep a start::decisions facade over focused policy crates"
    );
    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/supervisor/state.rs")
            .exists(),
        "orchestration must not keep a supervisor::state facade over crash policy"
    );

    let supervisor_mod =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/supervisor/mod.rs"))
            .unwrap();
    assert!(
        !supervisor_mod.contains("pub mod state"),
        "orchestration supervisor module must not re-export crash policy state"
    );
    let rpc_source = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/project_controller/rpc.rs"),
    )
    .unwrap();
    let supervisor_config =
        fs::read_to_string(manifest_dir.join("agent-doc-supervisor/src/config.rs")).unwrap();
    let supervisor_crash_policy =
        fs::read_to_string(manifest_dir.join("agent-doc-supervisor/src/crash_policy.rs")).unwrap();
    let supervisor_run_loop =
        fs::read_to_string(manifest_dir.join("agent-doc-supervisor/src/run_loop.rs")).unwrap();
    for required_snippet in [
        "pub struct AgentLaunchArgsSources",
        "pub fn resolve_agent_launch_args(",
        "pub const STALE_INSTALL_GRACE_SECS",
        "pub fn classify_stale_install_artifacts",
        "pub fn is_agent_doc_dogfood_session",
    ] {
        assert!(
            supervisor_config.contains(required_snippet),
            "agent-doc-supervisor config should own stale-install policy directly: {required_snippet}"
        );
    }
    for forbidden_snippet in [
        "pub(crate) fn resolve_supervisor_auto_recycle",
        "pub(crate) fn resolve_agent_change_restart",
        "pub(crate) fn source_newer_than_installed_binary",
        "pub(crate) fn resolve_supervisor_auto_install",
        "pub(crate) fn auto_install_should_retry",
        "pub(crate) fn host_supervisor_is_stale",
        "fn is_agent_doc_dogfood_session",
    ] {
        assert!(
            !rpc_source.contains(forbidden_snippet),
            "project_controller::rpc must not wrap pure supervisor config helpers: {forbidden_snippet}"
        );
    }
    for required_snippet in [
        "agent_doc_supervisor::config::auto_install_should_retry",
        "agent_doc_supervisor::config::host_supervisor_is_stale",
        "agent_doc_supervisor::config::is_agent_doc_dogfood_session",
    ] {
        assert!(
            rpc_source.contains(required_snippet),
            "project_controller::rpc should call focused supervisor config helpers directly: {required_snippet}"
        );
    }
    let preflight =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/preflight.rs")).unwrap();
    for forbidden_snippet in [
        "const STALE_INSTALL_GRACE_SECS",
        "fn classify_stale_install_artifacts",
    ] {
        assert!(
            !preflight.contains(forbidden_snippet),
            "preflight must not re-own pure stale-install policy: {forbidden_snippet}"
        );
    }
    assert!(
        preflight.contains("agent_doc_supervisor::config::classify_stale_install_artifacts")
            && preflight.contains("agent_doc_supervisor::config::STALE_INSTALL_GRACE_SECS"),
        "preflight should call focused stale-install policy directly"
    );
    for required_snippet in [
        "pub enum SupervisorPromptDecision",
        "pub enum SupervisorCleanExitResolution",
        "pub enum SupervisorRestartContinueExitStrategy",
        "pub const FAILED_RESUME_WINDOW",
        "pub struct FailedResumeTracker",
        "pub fn classify_supervisor_prompt_input",
        "pub fn supervisor_policy_exit_code",
        "pub fn supervisor_clean_exit_resolution",
        "pub fn restart_continue_exit_strategy",
        "pub fn supervisor_resume_handoff_failed",
        "pub fn supervisor_clean_exit_before_prompt_seen",
    ] {
        assert!(
            supervisor_crash_policy.contains(required_snippet),
            "agent-doc-supervisor crash_policy should own supervisor restart policy directly: {required_snippet}"
        );
    }
    let start_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/start.rs")).unwrap();
    let start_run_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/start/run.rs")).unwrap();
    for forbidden_snippet in ["fn resolve_agent_args("] {
        assert!(
            !start_source.contains(forbidden_snippet),
            "start.rs must not re-own pure supervisor launch-args precedence: {forbidden_snippet}"
        );
    }
    assert!(
        start_source.contains("fn agent_launch_args_sources(")
            && start_run_source
                .contains("agent_doc_supervisor::config::resolve_agent_launch_args("),
        "start paths should project launch-args facts and call focused supervisor config policy directly"
    );
    for required_snippet in ["pub struct ChildLaunchPlan", "pub fn child_launch_plan("] {
        assert!(
            supervisor_run_loop.contains(required_snippet),
            "agent-doc-supervisor run_loop should own supervisor child-launch planning directly: {required_snippet}"
        );
    }
    for forbidden_snippet in ["struct ChildLaunchPlan", "fn child_launch_plan("] {
        assert!(
            !start_run_source.contains(forbidden_snippet),
            "start/run.rs must not re-own pure supervisor child-launch planning: {forbidden_snippet}"
        );
    }
    assert!(
        start_run_source.contains("run_loop::{") && start_run_source.contains("child_launch_plan"),
        "start/run.rs should call focused supervisor child-launch planning directly"
    );
    for forbidden_snippet in [
        "enum PromptDecision",
        "fn classify_prompt_decision",
        "fn policy_exit_code_for_supervisor",
        "enum CleanExitResolution",
        "enum RestartContinueExitStrategy",
        "struct FailedResumeTracker",
        "const FAILED_RESUME_THRESHOLD",
        "fn clean_exit_resolution",
        "fn restart_continue_exit_strategy",
        "fn resume_handoff_failed",
        "fn clean_exit_before_prompt_seen",
    ] {
        assert!(
            !start_source.contains(forbidden_snippet),
            "start.rs must not re-own pure supervisor restart policy: {forbidden_snippet}"
        );
    }
    assert!(
        start_source.contains("classify_supervisor_prompt_input")
            && start_source.contains("SupervisorPromptDecision")
            && start_source.contains("supervisor_policy_exit_code")
            && start_source.contains("FailedResumeTracker")
            && start_run_source.contains("supervisor_policy_exit_code(")
            && start_run_source.contains("supervisor_clean_exit_resolution(")
            && start_run_source.contains("restart_continue_exit_strategy(")
            && start_run_source.contains("supervisor_resume_handoff_failed(")
            && start_run_source.contains("supervisor_clean_exit_before_prompt_seen("),
        "start paths should call focused supervisor restart policy directly"
    );
    let supervisor_route_owned =
        fs::read_to_string(manifest_dir.join("agent-doc-supervisor/src/route_owned.rs")).unwrap();
    for required_snippet in [
        "pub enum RouteOwnedReapPolicy",
        "pub enum RouteOwnedLivenessReason",
        "pub struct RouteOwnedReapDecision",
        "pub fn route_owned_reap_decision(",
        "pub fn route_owned_file_dirty_after_commit(",
        "pub fn route_owned_liveness_reason_for_content(",
        "pub fn route_owned_backlog_has_live_items(",
        "pub fn route_owned_queue_has_prompts(",
        "pub fn route_owned_exchange_tail_has_unresolved_prompt(",
    ] {
        assert!(
            supervisor_route_owned.contains(required_snippet),
            "agent-doc-supervisor route_owned should own route-owned reap policy directly: {required_snippet}"
        );
    }
    for forbidden_snippet in [
        "pub enum RouteOwnedReapPolicy",
        "pub enum RouteOwnedLivenessReason",
        "struct RouteOwnedReapDecision",
        "fn route_owned_reap_decision(",
        "fn route_owned_file_dirty_after_commit(",
        "fn route_owned_liveness_reason_for_content(",
        "fn route_owned_reap_decision_for_file(",
        "fn route_owned_backlog_has_live_items(",
        "fn route_owned_queue_has_prompts(",
        "fn route_owned_exchange_tail_has_unresolved_prompt(",
        "fn route_owned_line_is_response_heading(",
    ] {
        assert!(
            !start_source.contains(forbidden_snippet),
            "start.rs must not re-own pure route-owned reap policy: {forbidden_snippet}"
        );
    }
    for required_snippet in [
        "route_owned_liveness_reason_for_content",
        "route_owned_reap_decision(",
        "RouteOwnedLivenessReason::AdapterFailure",
    ] {
        assert!(
            start_source.contains(required_snippet),
            "start.rs should only adapt file reads and call focused route-owned policy directly: {required_snippet}"
        );
    }
    let cli_main = fs::read_to_string(manifest_dir.join("src/main.rs")).unwrap();
    assert!(
        cli_main.contains("agent_doc_supervisor::route_owned::RouteOwnedReapPolicy")
            && !cli_main.contains("agent_doc_orchestration::start::RouteOwnedReapPolicy"),
        "the CLI shell should use the focused route-owned reap policy type directly"
    );

    for relative in [
        "agent-doc-orchestration/src/start.rs",
        "agent-doc-orchestration/src/supervisor/in_process.rs",
        "agent-doc-orchestration/src/start/idle_watch.rs",
        "agent-doc-orchestration/src/start/run.rs",
        "agent-doc-orchestration/src/harness.rs",
        "agent-doc-orchestration/src/project_controller/rpc.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            !source.contains("start::decisions")
                && !source.contains("pub mod decisions")
                && !source.contains("super::decisions")
                && !source.contains("supervisor::state")
                && !source.contains("super::state"),
            "{relative} must call focused supervisor/queue/process crates directly"
        );
    }

    let supervisor_selfkill =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/supervisor_selfkill.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub fn supervisor_self_kill_action(",
        "pub fn supervisor_force_kill_decision(",
        "pub fn start_route_owned_doc_from_args(",
    ] {
        assert!(
            !supervisor_selfkill.contains(forbidden_snippet),
            "supervisor_selfkill must stay an effect adapter, not re-own pure self-kill policy: {forbidden_snippet}"
        );
    }
    let supervisor_selfkill_policy =
        fs::read_to_string(manifest_dir.join("agent-doc-supervisor/src/selfkill.rs")).unwrap();
    let supervisor_lifecycle_policy =
        fs::read_to_string(manifest_dir.join("agent-doc-supervisor/src/lifecycle.rs")).unwrap();
    for required_snippet in [
        "pub fn supervisor_self_kill_action(",
        "pub fn supervisor_force_kill_decision(",
        "pub fn start_route_owned_doc_from_args(",
    ] {
        assert!(
            supervisor_selfkill_policy.contains(required_snippet),
            "agent-doc-supervisor should expose pure self-kill policy directly: {required_snippet}"
        );
    }
    assert!(
        supervisor_lifecycle_policy.contains("pub fn write_wedged_from_ipc_failures("),
        "agent-doc-supervisor lifecycle policy should own the write_wedged evidence classifier"
    );
    for required_snippet in [
        "pub fn start_session_retryable_during_recycle(",
        "pub fn recycle_interrupted_resubmit_should_wait(",
    ] {
        assert!(
            supervisor_lifecycle_policy.contains(required_snippet),
            "agent-doc-supervisor lifecycle policy should own recycle-in-flight routing decisions: {required_snippet}"
        );
    }
    let write_converge =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/converge.rs"))
            .unwrap();
    assert!(
        !write_converge.contains("pub fn write_wedged_from_ipc_failures("),
        "write::converge must not re-own pure supervisor write-wedge classification"
    );
    let write_ipc =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/ipc.rs")).unwrap();
    assert!(
        write_ipc.contains("agent_doc_supervisor::lifecycle::write_wedged_from_ipc_failures"),
        "write::ipc should call focused supervisor write-wedge classification directly"
    );
    let recycle_inflight =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/recycle_inflight.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub fn start_session_retryable_during_recycle(",
        "fn start_session_retryable_during_recycle(",
        "pub fn recycle_interrupted_resubmit_should_wait(",
        "fn recycle_interrupted_resubmit_should_wait(",
        "pub use agent_doc_supervisor::lifecycle",
    ] {
        assert!(
            !recycle_inflight.contains(forbidden_snippet),
            "recycle_inflight must stay a marker IO adapter, not re-own or facade supervisor recycle policy: {forbidden_snippet}"
        );
    }
    let start_run =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/start/run.rs")).unwrap();
    assert!(
        start_run.contains("agent_doc_supervisor::")
            && start_run.contains("start_session_retryable_during_recycle"),
        "start/run should call focused supervisor start-session recycle retry policy directly"
    );
    let route_dispatch =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/route/dispatch.rs"))
            .unwrap();
    assert!(
        route_dispatch.contains("agent_doc_supervisor::")
            && route_dispatch.contains("recycle_interrupted_resubmit_should_wait"),
        "route dispatch should call focused supervisor recycle-resubmit policy directly"
    );
    let idle_watch =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/start/idle_watch.rs"))
            .unwrap();
    assert!(
        idle_watch.contains("agent_doc_supervisor::selfkill::supervisor_self_kill_action"),
        "idle_watch should call focused supervisor self-kill policy directly"
    );
    assert!(
        supervisor_selfkill
            .contains("agent_doc_supervisor::selfkill::start_route_owned_doc_from_args"),
        "supervisor_selfkill should call focused route-owned cmdline parsing directly"
    );
    assert!(
        supervisor_selfkill
            .contains("agent_doc_supervisor::selfkill::supervisor_force_kill_decision"),
        "supervisor_selfkill should call focused force-kill escalation policy directly"
    );

    let start_detection =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/start/detection.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub(crate) fn ready_busy_conflict_reconcile_decision(",
        "pub(crate) fn stale_busy_idle_reconcile_decision(",
        "pub(crate) fn reconcile_stale_busy_idle_queue_state(",
    ] {
        assert!(
            !start_detection.contains(forbidden_snippet),
            "start::detection must not re-own pure supervisor reconcile policy: {forbidden_snippet}"
        );
    }
    let idle_watch =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/start/idle_watch.rs"))
            .unwrap();
    assert!(
        idle_watch.contains("agent_doc_supervisor::{")
            && idle_watch.contains("idle_reconcile::{")
            && start_source.contains(
                "agent_doc_supervisor::idle_reconcile::ready_busy_conflict_reconcile_decision"
            ),
        "start paths should call focused supervisor idle_reconcile policy directly"
    );

    for relative in [
        "agent-doc-orchestration/src/start.rs",
        "agent-doc-orchestration/src/supervisor/in_process.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            source.contains("agent_doc_supervisor::crash_policy::{"),
            "{relative} should call focused crash policy directly"
        );
    }

    let supervisor_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-supervisor/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&supervisor_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    for required in [
        "agent-doc-diff",
        "agent-doc-element",
        "agent-doc-element-backlog",
        "agent-doc-hash",
        "agent-doc-queue",
    ] {
        assert!(
            dependencies.contains_key(required),
            "agent-doc-supervisor route-owned liveness should depend only on focused pure domain crates: {required}"
        );
    }
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "agent-doc-supervisor-process",
        "interprocess",
        "notify",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-supervisor must stay pure and not depend on orchestration/process effects"
        );
    }
}

#[test]
fn test_agent_doc_queue_has_no_manual_addition_compatibility_shim() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let queue_source =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/document_queue.rs")).unwrap();

    for required in [
        "pub fn operator_authored_prompt_identities(",
        "pub fn normalized_queue_line_for_match(",
        "pub fn queue_contains_prompt_line(",
        "pub fn queue_ids_including_struck(",
        "pub fn queue_delete_counts(",
        "pub fn queue_counts_are_subset(",
        "pub fn queue_counts_have_deletion(",
    ] {
        assert!(
            queue_source.contains(required),
            "agent-doc-queue should expose the focused queue identity API: {required}"
        );
    }
    for forbidden_snippet in [
        "pub fn annotate_manual_queue_additions(",
        "compatibility shim for older call sites",
    ] {
        assert!(
            !queue_source.contains(forbidden_snippet),
            "agent-doc-queue must not retain the removed manual-addition compatibility shim: {forbidden_snippet}"
        );
    }

    let response_guards = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/response_guards.rs"),
    )
    .unwrap();
    for forbidden in [
        "pub(crate) fn normalized_queue_line_for_match(",
        "pub(crate) fn queue_contains_prompt_line(",
        "pub(crate) fn queue_ids_including_struck(",
    ] {
        assert!(
            !response_guards.contains(forbidden),
            "response_guards must not re-own queue prompt identity policy: {forbidden}"
        );
    }
    for required in [
        "agent_doc_queue::document_queue::queue_contains_prompt_line",
        "agent_doc_queue::document_queue::queue_ids_including_struck",
    ] {
        assert!(
            response_guards.contains(required),
            "response_guards should call focused queue identity policy directly: {required}"
        );
    }

    let maintenance_source = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/preflight/maintenance.rs"),
    )
    .unwrap();
    for forbidden in [
        "type QueueDeleteCounts =",
        "fn queue_entry_delete_key(",
        "fn queue_delete_counts(",
        "fn queue_counts_are_subset(",
        "fn queue_counts_have_deletion(",
    ] {
        assert!(
            !maintenance_source.contains(forbidden),
            "preflight maintenance must not re-own queue deletion identity policy: {forbidden}"
        );
    }
    for required in [
        "agent_doc_queue::document_queue::queue_delete_counts",
        "agent_doc_queue::document_queue::queue_counts_are_subset",
        "agent_doc_queue::document_queue::queue_counts_have_deletion",
    ] {
        assert!(
            maintenance_source.contains(required),
            "preflight maintenance should call focused queue deletion policy directly: {required}"
        );
    }
}

#[test]
fn test_agent_doc_supervisor_process_owns_resize_effects() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        manifest_dir
            .join("agent-doc-supervisor-process/src/resize.rs")
            .exists(),
        "agent-doc-supervisor-process must own terminal resize process effects"
    );
    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/supervisor/resize.rs")
            .exists(),
        "orchestration must not keep a supervisor::resize facade"
    );

    let supervisor_mod =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/supervisor/mod.rs"))
            .unwrap();
    assert!(
        !supervisor_mod.contains("pub mod resize"),
        "orchestration supervisor module must not re-export resize effects"
    );

    let start_run =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/start/run.rs")).unwrap();
    assert!(
        start_run.contains("agent_doc_supervisor_process::{") && start_run.contains("resize,"),
        "supervisor run loop should import resize from agent-doc-supervisor-process directly"
    );
}

#[test]
fn test_agent_doc_queue_owns_backlog_queue_sync_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let queue_sync =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/backlog_sync.rs")).unwrap();
    for required in [
        "pub struct BacklogQueueSyncRequest",
        "pub fn collect_backlog_queue_sync",
        "pub struct OneShotBacklogQueueSyncRequest",
        "pub fn collect_one_shot_backlog_queue_sync",
        "pub struct BacklogQueueSyncReport",
        "pub fn backlog_queue_sync_report",
        "pub fn reconcile_queue_tombstones",
        "pub fn format_queue_ids",
        "pub fn collect_backlog_priority_ranks",
        "pub fn collect_after_deps",
    ] {
        assert!(
            queue_sync.contains(required),
            "agent-doc-queue should own backlog-to-queue sync policy: {required}"
        );
    }

    let preflight_maintenance = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/preflight/maintenance.rs"),
    )
    .unwrap();
    for forbidden_snippet in [
        "pub(crate) struct BacklogQueueSyncRequest",
        "struct BacklogQueueSyncRequest",
        "pub(crate) fn collect_backlog_queue_sync",
        "pub(crate) fn collect_backlog_priority_ranks",
        "pub(crate) fn collect_after_deps",
    ] {
        assert!(
            !preflight_maintenance.contains(forbidden_snippet),
            "preflight maintenance must not re-own backlog queue sync policy: {forbidden_snippet}"
        );
    }
    assert!(
        preflight_maintenance.contains("agent_doc_queue::backlog_sync::collect_backlog_queue_sync")
            && preflight_maintenance
                .contains("agent_doc_queue::backlog_sync::collect_backlog_priority_ranks")
            && preflight_maintenance.contains("agent_doc_queue::backlog_sync::collect_after_deps"),
        "preflight maintenance should call backlog queue sync policy from agent-doc-queue directly"
    );

    let queue_tombstone = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/preflight/queue_tombstone.rs"),
    )
    .unwrap();
    for forbidden_snippet in [
        "pub(crate) fn reconcile(",
        "fn reconcile(",
        "snapshot_active_ids.difference(current_all_ids)",
    ] {
        assert!(
            !queue_tombstone.contains(forbidden_snippet),
            "preflight queue_tombstone must not re-own tombstone reconciliation policy: {forbidden_snippet}"
        );
    }
    assert!(
        queue_tombstone.contains("use agent_doc_queue::backlog_sync::reconcile_queue_tombstones;")
            && queue_tombstone.contains("fn reconcile_for_file(")
            && queue_tombstone.contains("reconcile_queue_tombstones("),
        "preflight queue_tombstone should load/save sidecars and call focused queue tombstone policy directly"
    );

    let component_attrs =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/component_attrs.rs")).unwrap();
    for required in [
        "pub struct ComponentAttrWarning",
        "pub fn component_attr_warning",
        "pub fn message_body",
    ] {
        assert!(
            component_attrs.contains(required),
            "agent-doc-queue should own component attribute warning policy: {required}"
        );
    }
    let preflight_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/preflight.rs")).unwrap();
    for forbidden_snippet in [
        "QUEUE_ONLY_COMPONENT_ATTRS",
        "KNOWN_COMPONENT_ATTRS",
        "fn misplaced_component_attr_warning",
        "BacklogQueueSyncMode::parse(value)",
    ] {
        assert!(
            !preflight_source.contains(forbidden_snippet),
            "preflight.rs must not re-own or facade component attribute warning policy: {forbidden_snippet}"
        );
    }
    assert!(
        preflight_source
            .contains("agent_doc_queue::component_attrs::component_attr_warning(content)")
            && preflight_source.contains("fn component_attr_warning_for_file("),
        "preflight should only adapt focused component-attr warning facts into PreflightWarning"
    );

    let queue_cmd =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/queue_cmd.rs")).unwrap();
    for forbidden_snippet in [
        "fn queue_prompt_reference_id",
        "fn queue_entry_reference_id",
        "fn format_queue_ids",
        "fn collect_one_shot_backlog_queue_sync",
        "fn backlog_queue_sync_report",
    ] {
        assert!(
            !queue_cmd.contains(forbidden_snippet),
            "queue_cmd.rs must not re-own one-shot backlog queue sync policy: {forbidden_snippet}"
        );
    }
    assert!(
        queue_cmd.contains("agent_doc_queue::backlog_sync")
            && queue_cmd.contains("collect_one_shot_backlog_queue_sync")
            && queue_cmd.contains("backlog_queue_sync_report"),
        "queue_cmd.rs should call one-shot queue sync policy through agent-doc-queue directly"
    );
}

#[test]
fn test_agent_doc_queue_owns_continuation_guidance_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let queue_policy =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_continuation.rs")).unwrap();
    assert!(
        queue_policy.contains("pub const CONTINUATION_NO_STALL_GUIDANCE")
            && queue_policy.contains("pub const RECYCLE_YIELD_GUIDANCE")
            && queue_policy.contains("pub fn continuation_guidance")
            && queue_policy.contains("pub struct EffectiveContinuationOutput")
            && queue_policy.contains("pub fn effective_continuation_output"),
        "agent-doc-queue must own queue continuation guidance policy"
    );

    let orchestration_adapter =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/queue_continuation.rs"))
            .unwrap();
    for forbidden in [
        "pub const CONTINUATION_NO_STALL_GUIDANCE",
        "pub const RECYCLE_YIELD_GUIDANCE",
        "pub fn continuation_guidance",
        "pub fn effective_continuation_output",
    ] {
        assert!(
            !orchestration_adapter.contains(forbidden),
            "orchestration must not keep queue continuation guidance facades"
        );
    }

    let preflight_run =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/preflight/run.rs"))
            .unwrap();
    let session_check =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/session_check.rs"))
            .unwrap();
    assert!(
        preflight_run
            .contains("agent_doc_queue::queue_continuation::effective_continuation_output"),
        "preflight should delegate effective continuation output policy to agent-doc-queue"
    );
    for forbidden in [
        "queue_state.queue_continuation_required && !recycle_yield_pending",
        "let queue_continuation_guidance = if recycle_yield_pending",
        "RECYCLE_YIELD_GUIDANCE.to_string()",
    ] {
        assert!(
            !preflight_run.contains(forbidden),
            "preflight/run.rs must not re-own effective continuation output policy: {forbidden}"
        );
    }
    assert!(
        session_check.contains("agent_doc_queue::queue_continuation::continuation_guidance")
            || session_check
                .contains("agent_doc_queue::queue_continuation::CONTINUATION_NO_STALL_GUIDANCE"),
        "session_check should use agent-doc-queue guidance directly"
    );
}

#[test]
fn test_agent_doc_element_review_owns_review_projection_and_ungate_planning() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let review_model =
        fs::read_to_string(manifest_dir.join("agent-doc-element-review/src/lib.rs")).unwrap();
    for required in [
        "pub struct ReviewItemView",
        "pub struct ReviewListFilter",
        "pub fn find_review_component_in_content",
        "pub fn ensure_review_component_in_document",
        "pub struct ReviewItemRemovalPlan",
        "pub fn remove_review_items_from_document",
        "pub fn resolve_review_items_in_document",
        "pub fn review_item_views_from_content",
        "pub struct UngateTasksReport",
        "pub struct UngateTasksPlan",
        "pub fn plan_ungate_tasks_for_review",
    ] {
        assert!(
            review_model.contains(required),
            "agent-doc-element-review must own review projection and planning API"
        );
    }

    let backlog_cmd =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/backlog_cmd.rs"))
            .unwrap();
    let orchestration_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/lib.rs")).unwrap();
    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/pending_cmd.rs")
            .exists(),
        "orchestration must not keep a pending_cmd compatibility module"
    );
    assert!(
        orchestration_lib.contains("pub mod backlog_cmd;")
            && !orchestration_lib.contains("pub mod pending_cmd")
            && !orchestration_lib.contains("pub use backlog_cmd as pending_cmd"),
        "orchestration should expose backlog_cmd directly without a pending_cmd alias"
    );
    for forbidden in [
        "pub struct ReviewItemView",
        "pub struct ReviewListFilter",
        "pub struct UngateTasksReport",
        "fn find_review_component_in_content",
        "fn insert_empty_review_after_backlog",
        "fn ensure_review_component(",
        "fn ungate_task_text",
        "fn extract_review_tags",
        "fn extract_review_next",
        "backlog::op_take_all_by_id(existing, id)",
        "item.state = backlog::PendingState::Done",
    ] {
        assert!(
            !backlog_cmd.contains(forbidden),
            "backlog_cmd must stay a file-IO adapter, not a review projection/planning facade"
        );
    }
    assert!(
        backlog_cmd.contains("find_review_component_in_content")
            && backlog_cmd.contains("ensure_review_component_in_document"),
        "backlog_cmd should call review component helpers from agent-doc-element-review"
    );
    assert!(
        backlog_cmd.contains("remove_review_items_from_document")
            && backlog_cmd.contains("resolve_review_items_in_document"),
        "backlog_cmd should delegate review removal/resolve planning to agent-doc-element-review directly"
    );
    assert!(
        backlog_cmd.contains("agent_doc_element_review::review_item_views_from_content"),
        "backlog_cmd should delegate review projection to agent-doc-element-review directly"
    );
    assert!(
        backlog_cmd.contains("agent_doc_element_review::plan_ungate_tasks_for_review"),
        "backlog_cmd should delegate review ungate planning to agent-doc-element-review directly"
    );

    let cli = fs::read_to_string(manifest_dir.join("src/main.rs")).unwrap();
    assert!(
        cli.contains("agent_doc_element_review::ReviewListFilter"),
        "CLI should construct review filters from the focused review crate"
    );
}

#[test]
fn test_agent_doc_element_backlog_owns_tracked_line_remove_and_prune_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let backlog_model =
        fs::read_to_string(manifest_dir.join("agent-doc-element-backlog/src/backlog.rs")).unwrap();
    for required in [
        "pub enum TrackedWorkList",
        "pub fn find_tracked_work_component_in_content",
        "pub fn find_open_tracked_work_component_in_content",
        "pub fn open_tracked_work_component_name_in_content",
        "pub fn content_has_resolved_tracked_work_id",
        "pub struct TrackedComponentItemDrop",
        "pub fn tracked_component_item_counts",
        "pub fn dropped_tracked_component_items",
        "pub fn trim_tracked_parent_prefix",
        "pub fn line_is_legacy_done_item",
        "pub fn op_remove_matching_tracked_line",
        "pub fn op_prune_legacy_done_lines",
        "pub struct PendingAddBatchOutcome",
        "pub fn op_prepend_many_with_outcomes",
    ] {
        assert!(
            backlog_model.contains(required),
            "agent-doc-element-backlog must own tracked line remove/prune policy"
        );
    }

    let backlog_cmd =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/backlog_cmd.rs"))
            .unwrap();
    for forbidden in [
        "enum TrackedWorkList",
        "enum TrackedList",
        "is_icebox_component",
        "is_tracked_work_component",
        "is_backlog_done_component",
        "fn trim_tracked_parent_prefix",
        "fn line_is_legacy_done_item",
        "let lines: Vec<&str> = existing.lines().collect();",
        ".filter(|line| !line_is_legacy_done_item(line))",
        "for item in items.iter().rev()",
        "ids.reverse()",
    ] {
        assert!(
            !backlog_cmd.contains(forbidden),
            "backlog_cmd must stay a file-IO adapter, not re-own tracked-work batch/remove/prune policy"
        );
    }
    assert!(
        backlog_cmd.contains("backlog::find_tracked_work_component_in_content")
            && backlog_cmd.contains("backlog::find_open_tracked_work_component_in_content")
            && backlog_cmd.contains("backlog::open_tracked_work_component_name_in_content")
            && backlog_cmd.contains("backlog::content_has_resolved_tracked_work_id")
            && backlog_cmd.contains("backlog::TrackedWorkList"),
        "backlog_cmd should call tracked-work component lookup helpers from agent-doc-element-backlog"
    );
    assert!(
        backlog_cmd.contains("backlog::op_remove_matching_tracked_line")
            && backlog_cmd.contains("backlog::op_prune_legacy_done_lines"),
        "backlog_cmd should call tracked line remove/prune policy from agent-doc-element-backlog"
    );
    assert!(
        backlog_cmd.contains("backlog::op_prepend_many_with_outcomes"),
        "backlog_cmd should call tracked-work batch add policy from agent-doc-element-backlog"
    );

    let compact =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/compact.rs")).unwrap();
    for forbidden in [
        "fn non_exchange_list_item_counts(",
        "fn tracked_component_item_counts(",
        "fn dropped_tracked_component_items(",
        "agent_doc_element_backlog::backlog::parse_items",
    ] {
        assert!(
            !compact.contains(forbidden),
            "compact must stay an adapter, not re-own tracked component preservation policy: {forbidden}"
        );
    }
    assert!(
        compact.contains("agent_doc_element_backlog::backlog::dropped_tracked_component_items"),
        "compact should call tracked component preservation policy from agent-doc-element-backlog"
    );
}

#[test]
fn test_agent_doc_element_backlog_owns_malformed_tracked_item_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let backlog_model =
        fs::read_to_string(manifest_dir.join("agent-doc-element-backlog/src/backlog.rs")).unwrap();
    for required in [
        "pub struct MalformedPendingItemLine",
        "pub struct MalformedTrackedItemRef",
        "pub fn detect_malformed_item_lines",
        "pub fn malformed_tracked_item_refs_in_components",
        "pub fn malformed_tracked_item_refs",
        "pub fn malformed_tracked_item_interruption_message",
    ] {
        assert!(
            backlog_model.contains(required),
            "agent-doc-element-backlog must own malformed tracked checklist policy: {required}"
        );
    }

    let backlog_guards = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/backlog_guards.rs"),
    )
    .unwrap();
    for forbidden in [
        "pub fn malformed_tracked_item_refs",
        "pub(crate) fn malformed_tracked_item_refs_in",
        "pub fn malformed_tracked_item_message",
        "detect_malformed_item_lines(",
    ] {
        assert!(
            !backlog_guards.contains(forbidden),
            "session_check backlog guards must not re-own malformed tracked checklist policy: {forbidden}"
        );
    }
    assert!(
        backlog_guards.contains("malformed_tracked_item_refs_in_components")
            && backlog_guards.contains("malformed_tracked_item_interruption_message"),
        "session_check backlog guards should call malformed tracked checklist policy from agent-doc-element-backlog"
    );

    let pending_checks = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/write/pending_checks.rs"),
    )
    .unwrap();
    assert!(
        !pending_checks.contains("crate::session_check::malformed_tracked_item"),
        "write pending checks must not route malformed tracked checklist policy through session_check"
    );
    assert!(
        pending_checks.contains("agent_doc_element_backlog::backlog::malformed_tracked_item_refs")
            && pending_checks.contains(
                "agent_doc_element_backlog::backlog::malformed_tracked_item_interruption_message"
            ),
        "write pending checks should call malformed tracked checklist policy directly"
    );
}

#[test]
fn test_agent_doc_element_backlog_owns_ops_proof_completion_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ops_proof =
        fs::read_to_string(manifest_dir.join("agent-doc-element-backlog/src/ops_proof.rs"))
            .unwrap();
    for required in [
        "pub struct OpsProofCompletion",
        "pub fn surface_pending_ids",
        "pub fn ops_proof_completion_candidates",
        "pub fn classify_ops_proof_completion",
        "pub fn marker_is_leading_status",
        "pub fn is_live_verify_gate",
        "pub fn contains_successful_ci_proof",
        "pub fn contains_commit_hash",
    ] {
        assert!(
            ops_proof.contains(required),
            "agent-doc-element-backlog must own ops-proof tracked-work completion policy: {required}"
        );
    }

    let preflight_maintenance = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/preflight/maintenance.rs"),
    )
    .unwrap();
    for forbidden in [
        "pub(crate) struct OpsProofCompletion",
        "struct OpsProofCompletion",
        "pub(crate) fn surface_pending_ids",
        "pub(crate) fn ops_proof_completion_candidates",
        "pub(crate) fn classify_ops_proof_completion",
        "pub(crate) fn marker_is_leading_status",
        "pub(crate) fn is_live_verify_gate",
        "pub(crate) fn contains_successful_ci_proof",
        "pub(crate) fn contains_commit_hash",
    ] {
        assert!(
            !preflight_maintenance.contains(forbidden),
            "preflight maintenance must not re-own ops-proof tracked-work completion policy: {forbidden}"
        );
    }
    assert!(
        preflight_maintenance.contains("agent_doc_element_backlog::ops_proof::surface_pending_ids")
            && preflight_maintenance
                .contains("agent_doc_element_backlog::ops_proof::ops_proof_completion_candidates"),
        "preflight maintenance should call ops-proof policy from agent-doc-element-backlog directly"
    );
}

#[test]
fn test_focus_no_stash_promote_compatibility_shim_is_removed() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in [
        "src/main.rs",
        "agent-doc-orchestration/src/focus.rs",
        "specs/07-session-tmux-commands.md",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            !source.contains("no-stash-promote")
                && !source.contains("no_stash_promote")
                && !source.contains("run_no_promote"),
            "{relative} must not keep the removed focus no-promotion compatibility shim"
        );
    }
}

#[test]
fn test_agent_doc_tmux_owns_focus_pane_decision() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmux_manifest = fs::read_to_string(manifest_dir.join("agent-doc-tmux/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&tmux_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();

    let tmux_source = fs::read_to_string(manifest_dir.join("agent-doc-tmux/src/lib.rs")).unwrap();
    assert!(
        tmux_source.contains("pub enum FocusPaneDecision")
            && tmux_source.contains("pub fn decide_focus_pane("),
        "agent-doc-tmux must own pure focus pane selection"
    );

    let focus_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/focus.rs")).unwrap();
    for forbidden_snippet in ["pub enum FocusPaneDecision", "pub fn decide_focus_pane("] {
        assert!(
            !focus_source.contains(forbidden_snippet),
            "focus.rs must call agent_doc_tmux directly instead of re-owning focus pane policy: {forbidden_snippet}"
        );
    }
    assert!(
        focus_source.contains("use agent_doc_tmux::{FocusPaneDecision, decide_focus_pane};"),
        "focus.rs should import the focused tmux pane decision API directly"
    );

    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-tmux focus policy must stay free of orchestration, git, editor IPC, sqlite, or tmux-router effects"
        );
    }
}

#[test]
fn test_agent_doc_tmux_owns_destructive_repair_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmux_manifest = fs::read_to_string(manifest_dir.join("agent-doc-tmux/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&tmux_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();

    let tmux_source = fs::read_to_string(manifest_dir.join("agent-doc-tmux/src/lib.rs")).unwrap();
    for required in [
        "pub fn repair_layout_skips_rescue_phase(",
        "pub const DESTRUCTIVE_REPAIR_MIN_INTERVAL_MS",
        "pub fn destructive_repair_throttled(",
    ] {
        assert!(
            tmux_source.contains(required),
            "agent-doc-tmux must own destructive repair pure policy: {required}"
        );
    }

    let sync_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/sync.rs")).unwrap();
    for forbidden in [
        "fn repair_layout_skips_rescue_phase(",
        "const DESTRUCTIVE_REPAIR_MIN_INTERVAL_MS",
        "fn destructive_repair_throttled(",
        "pub fn repair_layout_skips_rescue_phase(",
        "pub const DESTRUCTIVE_REPAIR_MIN_INTERVAL_MS",
        "pub fn destructive_repair_throttled(",
    ] {
        assert!(
            !sync_source.contains(forbidden),
            "sync.rs must call agent_doc_tmux directly instead of re-owning destructive repair policy: {forbidden}"
        );
    }
    assert!(
        sync_source.contains("agent_doc_tmux::repair_layout_skips_rescue_phase(")
            && sync_source.contains("agent_doc_tmux::destructive_repair_throttled(")
            && sync_source.contains("agent_doc_tmux::DESTRUCTIVE_REPAIR_MIN_INTERVAL_MS"),
        "sync.rs should import destructive repair policy from agent-doc-tmux directly"
    );

    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-tmux destructive repair policy must stay free of orchestration, git, editor IPC, sqlite, or tmux-router effects"
        );
    }
}

#[test]
fn test_agent_doc_tmux_owns_stash_prune_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmux_manifest = fs::read_to_string(manifest_dir.join("agent-doc-tmux/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&tmux_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();

    let tmux_source = fs::read_to_string(manifest_dir.join("agent-doc-tmux/src/lib.rs")).unwrap();
    for required in [
        "pub enum PruneCleanupMode",
        "pub struct StashTtlCandidate",
        "pub fn stash_ttl_prune_candidate(",
        "pub fn stash_ttl_prune_targets(",
    ] {
        assert!(
            tmux_source.contains(required),
            "agent-doc-tmux must own stash prune policy: {required}"
        );
    }

    let resync_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/resync.rs")).unwrap();
    for forbidden in [
        "pub enum PruneCleanupMode",
        "pub struct StashTtlCandidate",
        "pub fn stash_ttl_prune_candidate(",
        "pub fn stash_ttl_prune_targets(",
        "pub use agent_doc_tmux",
        "pub(crate) use agent_doc_tmux",
    ] {
        assert!(
            !resync_source.contains(forbidden),
            "resync.rs must keep prune effects only, not re-own or facade stash prune policy: {forbidden}"
        );
    }
    assert!(
        resync_source.contains(
            "use agent_doc_tmux::{PruneCleanupMode, StashTtlCandidate, stash_ttl_prune_targets};"
        ),
        "resync.rs should import focused stash prune policy directly"
    );

    let sync_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/sync.rs")).unwrap();
    assert!(
        !sync_source.contains("resync::PruneCleanupMode")
            && !sync_source.contains("crate::resync::PruneCleanupMode"),
        "sync.rs must not route prune cleanup mode through resync.rs"
    );
    assert!(
        sync_source.contains("agent_doc_tmux::PruneCleanupMode"),
        "sync.rs should use the focused prune cleanup mode directly"
    );

    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "agent-doc-tmux-commands",
        "agent-doc-tmux-io",
        "anyhow",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-tmux stash prune policy must stay free of orchestration, git, editor IPC, sqlite, tmux command, or tmux-router effects"
        );
    }
}

#[test]
fn test_agent_doc_tmux_owns_pane_position_selection() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmux_manifest = fs::read_to_string(manifest_dir.join("agent-doc-tmux/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&tmux_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();

    let tmux_source = fs::read_to_string(manifest_dir.join("agent-doc-tmux/src/lib.rs")).unwrap();
    for required in [
        "pub const TMUX_PANE_GEOMETRY_FORMAT",
        "pub enum PanePosition",
        "pub struct TmuxPaneGeometry",
        "pub fn parse_tmux_pane_geometry(",
        "pub fn select_pane_by_position(",
    ] {
        assert!(
            tmux_source.contains(required),
            "agent-doc-tmux must own pure pane position selection: {required}"
        );
    }

    let sessions_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/sessions.rs")).unwrap();
    for forbidden in [
        "fn select_pane_by_position(",
        "Vec<(String, u32, u32, u32, u32)>",
        "\"#{pane_id} #{pane_left} #{pane_top} #{pane_width} #{pane_height}\"",
    ] {
        assert!(
            !sessions_source.contains(forbidden),
            "sessions.rs must query tmux, not re-own pane geometry selection: {forbidden}"
        );
    }
    assert!(
        sessions_source.contains("use agent_doc_tmux::{")
            && sessions_source.contains("TMUX_PANE_GEOMETRY_FORMAT")
            && sessions_source.contains("select_pane_by_position"),
        "sessions.rs should call the focused tmux pane position API directly"
    );

    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-tmux pane position policy must stay free of orchestration, git, editor IPC, sqlite, or tmux-router effects"
        );
    }
}

#[test]
fn test_agent_doc_tmux_owns_associated_pane_resolution_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmux_manifest = fs::read_to_string(manifest_dir.join("agent-doc-tmux/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&tmux_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();

    let tmux_source = fs::read_to_string(manifest_dir.join("agent-doc-tmux/src/lib.rs")).unwrap();
    for required in [
        "pub enum AssociatedPaneSource",
        "pub struct AssociatedPaneCandidate",
        "pub enum AssociatedPaneResolution",
        "pub fn parse_pane_inventory_line(",
        "pub fn resolve_associated_panes(",
        "pub fn is_stash(&self) -> bool",
        "pub fn source_summary(&self) -> String",
    ] {
        assert!(
            tmux_source.contains(required),
            "agent-doc-tmux must own associated pane resolution policy: {required}"
        );
    }

    let sync_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/sync.rs")).unwrap();
    for forbidden in [
        "pub enum AssociatedPaneSource",
        "pub struct AssociatedPaneCandidate",
        "pub enum AssociatedPaneResolution",
        "fn parse_pane_inventory_line(",
        "pub fn resolve_associated_panes(",
        "pub use agent_doc_tmux",
        "pub(crate) use agent_doc_tmux",
    ] {
        assert!(
            !sync_source.contains(forbidden),
            "sync.rs must collect tmux evidence, not re-own or facade associated pane resolution policy: {forbidden}"
        );
    }
    assert!(
        sync_source.contains("use agent_doc_tmux::{")
            && sync_source.contains("AssociatedPaneCandidate")
            && sync_source.contains("AssociatedPaneResolution")
            && sync_source.contains("AssociatedPaneSource")
            && sync_source.contains("parse_pane_inventory_line")
            && sync_source.contains("resolve_associated_panes"),
        "sync.rs should call the focused associated pane API directly"
    );

    for relative_path in [
        "agent-doc-orchestration/src/route.rs",
        "agent-doc-orchestration/src/resync.rs",
        "agent-doc-orchestration/src/route/pane_resolution.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative_path)).unwrap();
        assert!(
            !source.contains("crate::sync::AssociatedPane")
                && !source.contains("crate::sync::resolve_associated_panes"),
            "{relative_path} must import associated pane policy from agent-doc-tmux, not through sync.rs"
        );
        assert!(
            source.contains("agent_doc_tmux::AssociatedPane")
                || source.contains("agent_doc_tmux::resolve_associated_panes"),
            "{relative_path} should call the focused associated pane API directly"
        );
    }

    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "agent-doc-tmux-commands",
        "agent-doc-tmux-io",
        "anyhow",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-tmux associated pane policy must stay free of orchestration, git, editor IPC, sqlite, or tmux-router effects"
        );
    }
}

#[test]
fn test_agent_doc_tmux_owns_bare_shell_command_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmux_manifest = fs::read_to_string(manifest_dir.join("agent-doc-tmux/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&tmux_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();

    let tmux_source = fs::read_to_string(manifest_dir.join("agent-doc-tmux/src/lib.rs")).unwrap();
    assert!(
        tmux_source.contains("pub fn pane_current_command_is_bare_shell("),
        "agent-doc-tmux must own pure pane current-command shell classification"
    );

    let dispatch_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/route/dispatch.rs"))
            .unwrap();
    for forbidden in [
        "pub(crate) fn pane_current_command_is_bare_shell(",
        "\"zsh\" | \"bash\" | \"sh\" | \"fish\" | \"dash\" | \"ksh\" | \"tcsh\" | \"csh\"",
    ] {
        assert!(
            !dispatch_source.contains(forbidden),
            "route dispatch must observe tmux state, not re-own bare-shell command policy: {forbidden}"
        );
    }
    assert!(
        dispatch_source.contains("use agent_doc_tmux::pane_current_command_is_bare_shell;"),
        "route dispatch should call the focused tmux bare-shell API directly"
    );

    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-tmux bare-shell policy must stay free of orchestration, git, editor IPC, sqlite, or tmux-router effects"
        );
    }
}

#[test]
fn test_agent_doc_turn_executor_tmux_owns_prompt_parser_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmux_executor_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-turn-executor-tmux/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&tmux_executor_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();

    let prompt_source =
        fs::read_to_string(manifest_dir.join("agent-doc-turn-executor-tmux/src/prompt.rs"))
            .unwrap();
    for required in [
        "pub struct PromptInfo",
        "pub struct PromptOption",
        "pub fn parse_prompt(",
        "pub fn strip_ansi(",
        "pub fn navigation_axis_for_prompt(",
        "pub fn navigation_keys_for_prompt(",
        "pub fn opencode_option_requires_confirmation(",
        "pub fn is_codex_idle_placeholder_prompt(",
        "pub fn codex_idle_placeholder_prompt(",
        "pub fn codex_idle_placeholder_candidate(",
        "pub fn codex_prompt_candidate_is_dim_placeholder(",
    ] {
        assert!(
            prompt_source.contains(required),
            "agent-doc-turn-executor-tmux must own pure prompt parser policy: {required}"
        );
    }

    let orchestration_prompt_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/prompt.rs")).unwrap();
    for forbidden in [
        "pub struct PromptInfo",
        "pub struct PromptOption",
        "pub fn parse_prompt(",
        "pub fn strip_ansi(",
        "fn parse_claude_prompt(",
        "fn parse_opencode_prompt(",
        "fn parse_option_line(",
        "fn opencode_selected_option_from_ansi(",
    ] {
        assert!(
            !orchestration_prompt_source.contains(forbidden),
            "orchestration prompt command must stay an effect adapter, not re-own parser policy: {forbidden}"
        );
    }
    assert!(
        orchestration_prompt_source.contains("use agent_doc_turn_executor_tmux::prompt::{"),
        "orchestration prompt adapter should call the focused prompt parser directly"
    );

    let harness_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/harness.rs")).unwrap();
    for forbidden in [
        "fn codex_idle_placeholder_prompt(",
        "fn codex_idle_placeholder_candidate(",
        "fn codex_prompt_candidate_is_dim_placeholder(",
        "fn codex_prompt_line_body_starts_dim(",
        "fn apply_sgr_sequence(",
        "fn is_safe_codex_placeholder_token(",
    ] {
        assert!(
            !harness_source.contains(forbidden),
            "harness.rs must not re-own Codex prompt placeholder parsing policy: {forbidden}"
        );
    }
    assert!(
        harness_source.contains("codex_idle_placeholder_candidate")
            && harness_source.contains("codex_prompt_candidate_is_dim_placeholder")
            && harness_source.contains("is_codex_idle_placeholder_prompt"),
        "harness.rs should call focused Codex prompt placeholder policy directly"
    );
    let start_detection_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/start/detection.rs"))
            .unwrap();
    let session_actor_source =
        fs::read_to_string(manifest_dir.join("src/session_actor_cmd.rs")).unwrap();
    let sim_world_engine_source =
        fs::read_to_string(manifest_dir.join("src/sim_world/engine.rs")).unwrap();
    for source in [
        &harness_source,
        &start_detection_source,
        &session_actor_source,
        &sim_world_engine_source,
    ] {
        assert!(
            !source.contains("agent_doc_orchestration::prompt::strip_ansi")
                && !source.contains("crate::prompt::strip_ansi")
                && !source.contains("agent_doc_orchestration::prompt::parse_prompt")
                && !source.contains("crate::prompt::parse_prompt"),
            "callers should import prompt parser policy from agent-doc-turn-executor-tmux directly"
        );
    }

    for forbidden in [
        "agent-doc-orchestration",
        "anyhow",
        "tmux-router",
        "agent-doc-frontmatter",
        "interprocess",
        "notify",
        "rusqlite",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-turn-executor-tmux prompt parser must stay free of orchestration/session/tmux effects"
        );
    }
}

#[test]
fn test_agent_doc_turn_executor_tmux_owns_context_clear_submit_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmux_executor_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-turn-executor-tmux/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&tmux_executor_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();

    let lib_source =
        fs::read_to_string(manifest_dir.join("agent-doc-turn-executor-tmux/src/lib.rs")).unwrap();
    assert!(
        lib_source.contains("pub mod context_clear;"),
        "agent-doc-turn-executor-tmux must export context-clear submit policy directly"
    );

    let context_clear_source =
        fs::read_to_string(manifest_dir.join("agent-doc-turn-executor-tmux/src/context_clear.rs"))
            .unwrap();
    for required in [
        "pub enum ContextClearSubmitStatus",
        "pub struct ContextClearSubmitObservation",
        "pub fn context_clear_command_visible_in_active_input(",
        "pub fn context_clear_submit_needs_enter_resubmit(",
        "pub fn context_clear_submit_observation_line(",
        "pub fn context_clear_submit_resubmit_proof_line(",
        "pub fn context_clear_submit_blocked_line(",
        "pub fn context_clear_submit_blocked_message(",
    ] {
        assert!(
            context_clear_source.contains(required),
            "agent-doc-turn-executor-tmux must own context-clear submit policy: {required}"
        );
    }

    let session_actor_source =
        fs::read_to_string(manifest_dir.join("src/session_actor_cmd.rs")).unwrap();
    for forbidden in [
        "enum ClearDirectSubmitStatus",
        "struct ClearDirectSubmitObservation",
        "fn clear_direct_submit_needs_enter_resubmit(",
        "fn clear_direct_submit_observation_line(",
        "fn clear_direct_submit_resubmit_proof_line(",
        "fn clear_direct_submit_blocked_line(",
        "fn clear_direct_submit_blocked_message(",
        "fn clear_command_visible_in_active_input(",
        "fn line_shows_clear_command_input(",
        "fn clear_command_candidate_visible(",
        "fn line_starts_with_clear_prompt_prefix(",
        "fn strip_clear_prompt_prefix(",
    ] {
        assert!(
            !session_actor_source.contains(forbidden),
            "session_actor_cmd.rs must keep context-clear submit effects only, not re-own pure policy: {forbidden}"
        );
    }
    assert!(
        session_actor_source.contains("use agent_doc_turn_executor_tmux::context_clear::{"),
        "session_actor_cmd.rs should call focused context-clear policy directly"
    );

    let start_detection_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/start/detection.rs"))
            .unwrap();
    for forbidden in [
        "pub(crate) fn context_clear_command_visible_in_active_input(",
        "fn line_shows_context_clear_command_input(",
        "fn context_clear_command_candidate_visible(",
        "fn line_starts_with_context_clear_prompt_prefix(",
        "fn strip_context_clear_prompt_prefix(",
    ] {
        assert!(
            !start_detection_source.contains(forbidden),
            "start/detection.rs must not duplicate context-clear visibility policy: {forbidden}"
        );
    }
    assert!(
        start_detection_source
            .contains("use agent_doc_turn_executor_tmux::context_clear::context_clear_command_visible_in_active_input;"),
        "start/detection.rs should call focused context-clear policy directly"
    );

    for forbidden in [
        "agent-doc-orchestration",
        "agent-doc-tmux-commands",
        "anyhow",
        "tmux-router",
        "agent-doc-frontmatter",
        "interprocess",
        "notify",
        "rusqlite",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-turn-executor-tmux context-clear policy must stay free of orchestration/session/tmux command effects"
        );
    }
}

#[test]
fn test_agent_doc_tmux_owns_layout_memory_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmux_manifest = fs::read_to_string(manifest_dir.join("agent-doc-tmux/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&tmux_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();

    let tmux_source = fs::read_to_string(manifest_dir.join("agent-doc-tmux/src/lib.rs")).unwrap();
    for required in [
        "pub struct TmuxLayoutColumn",
        "pub fn apply_column_memory(",
        "pub fn build_layout_state(",
        "pub enum TmuxFocusOnlyExpansionMode",
        "pub fn expand_focus_only_columns_for_editor_switch(",
        "pub fn apply_focus_only_expansion_policy(",
    ] {
        assert!(
            tmux_source.contains(required),
            "agent-doc-tmux must own tmux layout memory and focus-only expansion policy: {required}"
        );
    }

    let sync_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/sync.rs")).unwrap();
    assert!(
        sync_source.contains("agent_doc_tmux::apply_column_memory(")
            && sync_source.contains("agent_doc_tmux::build_layout_state(")
            && sync_source.contains("agent_doc_tmux::apply_focus_only_expansion_policy("),
        "sync.rs should call the focused tmux layout policy API directly"
    );

    let sync_layout_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/sync/layout.rs"))
            .unwrap();
    assert!(
        sync_layout_source.contains("classify_sync_layout_columns(")
            && sync_layout_source.contains("TmuxLayoutColumn::new("),
        "orchestration should keep only the document/frontmatter layout-column classifier"
    );
    for forbidden in [
        "fn apply_column_memory(",
        "fn build_layout_state(",
        "fn expand_focus_only_columns_for_editor_switch(",
        "fn apply_focus_only_expansion_policy(",
    ] {
        assert!(
            !sync_layout_source.contains(forbidden),
            "sync/layout.rs must not keep old layout policy wrappers after extraction: {forbidden}"
        );
    }

    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-tmux layout policy must stay free of orchestration, git, editor IPC, sqlite, or tmux-router effects"
        );
    }
}

#[test]
fn test_agent_doc_tmux_commands_owns_submit_profile_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmux_commands_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-tmux-commands/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&tmux_commands_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();

    let tmux_commands_source =
        fs::read_to_string(manifest_dir.join("agent-doc-tmux-commands/src/lib.rs")).unwrap();
    for required in [
        "pub struct TmuxSubmitProfile",
        "pub const fn tmux_submit_profile_for_harness(",
        "pub const fn tmux_submit_mode_for_harness(",
        "pub const fn tmux_submit_transform_for_harness(",
        "pub const fn tmux_submit_key_for_harness(",
        "pub fn submitted_text_without_trailing_line_endings(",
        "pub fn text_submit_command(",
        "pub fn text_only_command(",
    ] {
        assert!(
            tmux_commands_source.contains(required),
            "agent-doc-tmux-commands must own pure tmux submit policy: {required}"
        );
    }

    let sessions_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/sessions.rs")).unwrap();
    for forbidden in [
        "pub struct TmuxSubmitProfile",
        "pub const fn tmux_submit_profile_for_harness(",
        "pub const fn tmux_submit_mode_for_harness(",
        "pub const fn tmux_submit_transform_for_harness(",
        "pub const fn tmux_submit_key_for_harness(",
        "fn text_submit_command_args(",
        "fn text_only_command_args(",
    ] {
        assert!(
            !sessions_source.contains(forbidden),
            "sessions.rs must execute tmux commands, not re-own submit profile policy: {forbidden}"
        );
    }
    assert!(
        sessions_source.contains("use agent_doc_tmux_commands::{")
            && sessions_source.contains("text_submit_command(")
            && sessions_source.contains("text_only_command(")
            && sessions_source.contains("tmux_submit_profile_for_harness("),
        "sessions.rs should call the focused tmux command API directly"
    );

    let route_dispatch_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/route/dispatch.rs"))
            .unwrap();
    let start_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/start.rs")).unwrap();
    let idle_watch_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/start/idle_watch.rs"))
            .unwrap();
    let session_actor_source =
        fs::read_to_string(manifest_dir.join("src/session_actor_cmd.rs")).unwrap();
    let queue_dispatch_source =
        fs::read_to_string(manifest_dir.join("src/queue_dispatch.rs")).unwrap();
    let sim_world_source = fs::read_to_string(manifest_dir.join("src/sim_world.rs")).unwrap();
    for source in [
        &route_dispatch_source,
        &start_source,
        &idle_watch_source,
        &session_actor_source,
        &queue_dispatch_source,
        &sim_world_source,
    ] {
        assert!(
            !source.contains("crate::sessions::tmux_submit_")
                && !source.contains("agent_doc_orchestration::sessions::tmux_submit_"),
            "orchestration callers should import tmux submit policy from agent-doc-tmux-commands directly"
        );
    }
    assert!(
        !route_dispatch_source.contains("fn routed_trigger_submit_diagnostic(")
            && !route_dispatch_source.contains("fn routed_trigger_payload(")
            && !route_dispatch_source.contains("fn validate_routed_trigger_payload(")
            && route_dispatch_source.contains("tmux_submit_transform_for_harness(")
            && route_dispatch_source.contains("tmux_submit_key_for_harness("),
        "route dispatch should call focused tmux submit diagnostics directly, without a local wrapper"
    );
    let supervisor_ipc_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/supervisor/ipc.rs"))
            .unwrap();
    let route_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/route.rs")).unwrap();
    for source in [
        &supervisor_ipc_source,
        &route_dispatch_source,
        &route_source,
        &start_source,
        &queue_dispatch_source,
        &session_actor_source,
    ] {
        assert!(
            !source.contains("normalize_submit_text(")
                && !source.contains("routed_trigger_submit_payload("),
            "submit-text normalization should be consumed from agent-doc-tmux-commands directly, without orchestration wrappers"
        );
    }
    assert!(
        supervisor_ipc_source.contains("submitted_text_without_trailing_line_endings(")
            && route_dispatch_source.contains("submitted_text_without_trailing_line_endings(")
            && start_source.contains("submitted_text_without_trailing_line_endings(")
            && queue_dispatch_source.contains("submitted_text_without_trailing_line_endings(")
            && session_actor_source.contains("submitted_text_without_trailing_line_endings("),
        "submit-text callers should use the focused tmux command normalization API directly"
    );

    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-tmux-commands submit policy must stay free of orchestration, git, editor IPC, sqlite, or tmux-router effects"
        );
    }
}

#[test]
fn test_agent_doc_hash_owns_sha256_content_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let root_manifest: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let workspace_members = root_manifest["workspace"]["members"].as_array().unwrap();
    assert!(
        workspace_members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-hash")),
        "agent-doc-hash must be a workspace member"
    );
    let root_dependencies = root_manifest["dependencies"].as_table().unwrap();
    assert!(
        root_dependencies.contains_key("agent-doc-hash"),
        "root crate callers should depend on agent-doc-hash directly"
    );

    let hash_source = fs::read_to_string(manifest_dir.join("agent-doc-hash/src/lib.rs")).unwrap();
    assert!(
        hash_source.contains("pub fn content_hash(")
            && hash_source.contains("pub fn bytes_hash(")
            && hash_source.contains("pub fn path_hash(")
            && hash_source.contains("pub fn path_string_hash(")
            && hash_source.contains("pub fn document_id_for_path(")
            && hash_source.contains("Sha256::new()"),
        "agent-doc-hash must own shared SHA-256 hex and path-derived document-id policy"
    );

    let debounce_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-debounce/Cargo.toml")).unwrap();
    let debounce_manifest: toml::Value = toml::from_str(&debounce_manifest).unwrap();
    let debounce_dependencies = debounce_manifest["dependencies"].as_table().unwrap();
    assert!(
        debounce_dependencies.contains_key("agent-doc-hash")
            && !debounce_dependencies.contains_key("sha2")
            && !debounce_dependencies.contains_key("hex"),
        "agent-doc-debounce should call the focused hash crate instead of owning SHA-256 deps"
    );

    let orchestration_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/Cargo.toml")).unwrap();
    let orchestration_manifest: toml::Value = toml::from_str(&orchestration_manifest).unwrap();
    let orchestration_dependencies = orchestration_manifest["dependencies"].as_table().unwrap();
    assert!(
        orchestration_dependencies.contains_key("agent-doc-hash")
            && !orchestration_dependencies.contains_key("sha2")
            && !orchestration_dependencies.contains_key("hex"),
        "agent-doc-orchestration should call the focused hash crate instead of owning SHA-256 deps"
    );

    for relative in [
        "agent-doc-orchestration/src/ops_log.rs",
        "agent-doc-orchestration/src/op_capture.rs",
        "agent-doc-orchestration/src/backlog_cmd.rs",
        "agent-doc-orchestration/src/graph.rs",
        "agent-doc-orchestration/src/snapshot.rs",
        "agent-doc-debounce/src/lib.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        for forbidden in [
            "pub fn content_hash(",
            "fn content_hash(",
            "pub fn doc_id_for(",
            "fn doc_id_for(",
            "fn hash_path_str(",
            "use sha2::",
            "hex::encode(",
            "pub use agent_doc_hash",
        ] {
            assert!(
                !source.contains(forbidden),
                "{relative} must not keep a content-hash shim or local SHA-256 policy: {forbidden}"
            );
        }
    }

    fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_rs_files(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }

    for relative in [
        "src",
        "agent-doc-orchestration/src",
        "agent-doc-debounce/src",
    ] {
        let mut files = Vec::new();
        collect_rs_files(&manifest_dir.join(relative), &mut files);
        for file in files {
            let source = fs::read_to_string(&file).unwrap();
            for forbidden in [
                "ops_log::content_hash",
                "op_capture::content_hash",
                "agent_doc_debounce::content_hash",
                "pending_cmd::doc_id_for",
                "crate::pending_cmd::doc_id_for",
                "agent_doc_orchestration::pending_cmd",
                "backlog_cmd::doc_id_for",
                "crate::backlog_cmd::doc_id_for",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "callers must import agent_doc_hash directly instead of old path {forbidden} in {}",
                    file.display()
                );
            }
        }
    }
}

#[test]
fn test_agent_doc_ipc_protocol_owns_ack_classification() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let root_manifest: toml::Value = toml::from_str(&workspace_manifest).unwrap();
    let workspace_members = root_manifest["workspace"]["members"].as_array().unwrap();
    assert!(
        workspace_members
            .iter()
            .any(|member| member.as_str() == Some("agent-doc-ipc-protocol")),
        "agent-doc-ipc-protocol must be a workspace member"
    );
    let root_dependencies = root_manifest["dependencies"].as_table().unwrap();
    assert!(
        root_dependencies.contains_key("agent-doc-ipc-protocol"),
        "root crate callers should depend on agent-doc-ipc-protocol directly"
    );

    let protocol_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-ipc-protocol/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&protocol_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    let protocol_source =
        fs::read_to_string(manifest_dir.join("agent-doc-ipc-protocol/src/lib.rs")).unwrap();
    assert!(
        protocol_source.contains("pub enum AckClassification")
            && protocol_source.contains("pub fn classify_ack("),
        "agent-doc-ipc-protocol must own plugin IPC ack classification"
    );
    for required in [
        "pub fn early_ack_tagged_message(",
        "pub fn message_requests_early_ack(",
        "pub fn early_ack_line(",
        "pub fn early_ack_ops_marker(",
    ] {
        assert!(
            protocol_source.contains(required),
            "agent-doc-ipc-protocol must own early-ack protocol policy: {required}"
        );
    }
    for required in [
        "pub struct CallbackRequest",
        "pub struct CallbackPatch",
        "pub struct CallbackResponse",
        "pub struct PendingCallback",
        "pub fn callback_request(",
        "pub fn callback_response(",
        "pub fn callback_request_is_expired(",
        "pub fn callback_urgency_for_elapsed(",
        "pub fn pending_callback_from_request(",
        "pub fn callback_response_matches_request(",
    ] {
        assert!(
            protocol_source.contains(required),
            "agent-doc-ipc-protocol must own callback protocol policy: {required}"
        );
    }

    let orchestration_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/Cargo.toml")).unwrap();
    let orchestration_manifest: toml::Value = toml::from_str(&orchestration_manifest).unwrap();
    let orchestration_dependencies = orchestration_manifest["dependencies"].as_table().unwrap();
    assert!(
        orchestration_dependencies.contains_key("agent-doc-ipc-protocol"),
        "agent-doc-orchestration should call the focused IPC protocol crate"
    );

    let ipc_socket_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/ipc_socket.rs")).unwrap();
    assert!(
        ipc_socket_source.contains("use agent_doc_ipc_protocol::{")
            && ipc_socket_source.contains("AckClassification")
            && ipc_socket_source.contains("classify_ack")
            && ipc_socket_source.contains("early_ack_tagged_message")
            && ipc_socket_source.contains("message_requests_early_ack"),
        "ipc_socket.rs should import the focused IPC protocol API directly"
    );
    for forbidden in [
        "pub enum AckClassification",
        "pub fn classify_ack(",
        "fn early_ack_tagged_message(",
        "pub fn message_requests_early_ack(",
        "pub fn early_ack_line(",
        "pub fn early_ack_ops_marker(",
    ] {
        assert!(
            !ipc_socket_source.contains(forbidden),
            "ipc_socket.rs must not keep old IPC protocol policy after extraction: {forbidden}"
        );
    }

    let callback_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/callback.rs")).unwrap();
    assert!(
        callback_source.contains("use agent_doc_ipc_protocol::{")
            && callback_source.contains("callback_request_is_expired")
            && callback_source.contains("pending_callback_from_request")
            && callback_source.contains("callback_response_matches_request"),
        "callback.rs should import focused callback protocol policy directly"
    );
    for forbidden in [
        "pub struct CallbackRequest",
        "pub struct Patch",
        "pub struct CallbackResponse",
        "pub struct PendingCallback",
        "fn callback_request_is_expired(",
        "fn callback_urgency_for_elapsed(",
        "pub use agent_doc_ipc_protocol",
    ] {
        assert!(
            !callback_source.contains(forbidden),
            "callback.rs must stay a filesystem adapter, not a callback protocol facade: {forbidden}"
        );
    }

    let preflight_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/preflight.rs")).unwrap();
    assert!(
        preflight_source.contains("Vec<agent_doc_ipc_protocol::PendingCallback>")
            && !preflight_source.contains("Vec<crate::callback::PendingCallback>"),
        "preflight should type pending callbacks through the focused IPC protocol crate"
    );

    let sim_world_source = fs::read_to_string(manifest_dir.join("src/sim_world.rs")).unwrap();
    assert!(
        sim_world_source.contains("agent_doc_ipc_protocol::classify_ack(")
            && !sim_world_source.contains("ipc_socket::classify_ack")
            && !sim_world_source.contains("ipc_socket::AckClassification"),
        "root callers should use the focused IPC protocol crate instead of the old orchestration path"
    );

    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-ipc-protocol must stay free of orchestration, git, editor IPC, sqlite, or tmux-router effects"
        );
    }
}

#[test]
fn test_agent_doc_tmux_commands_owns_input_diag_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmux_commands_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-tmux-commands/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&tmux_commands_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();

    let tmux_commands_source =
        fs::read_to_string(manifest_dir.join("agent-doc-tmux-commands/src/lib.rs")).unwrap();
    for required in [
        "pub mod input_diag",
        "pub const PREFIX",
        "pub const EDITOR_ROUTE_ATTEMPT_ID_ENV",
        "pub struct KeyEventMeta",
        "pub fn sanitize_field(",
        "pub fn bytes_hash(",
        "pub fn key_name(",
        "pub fn verbose_enabled(",
        "pub fn format_key_event(",
        "pub fn format_payload_event(",
        "pub fn format_byte_event(",
        "pub fn format_transform_event(",
        "pub fn format_prompt_detection(",
    ] {
        assert!(
            tmux_commands_source.contains(required),
            "agent-doc-tmux-commands must own pure input diagnostic policy: {required}"
        );
    }

    let orchestration_input_diag =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/input_diag.rs")).unwrap();
    for forbidden in [
        "const PREFIX",
        "EDITOR_ROUTE_ATTEMPT_ID_ENV",
        "pub struct KeyEventMeta",
        "fn sanitize_field(",
        "fn bytes_hash(",
        "fn key_name(",
        "pub fn verbose_enabled(",
        "pub fn format_key_event(",
        "pub fn format_payload_event(",
        "pub fn format_byte_event(",
        "pub fn format_transform_event(",
        "pub fn format_prompt_detection(",
        "pub use agent_doc_tmux_commands::input_diag",
        "use sha2::",
    ] {
        assert!(
            !orchestration_input_diag.contains(forbidden),
            "orchestration input_diag must stay an effectful adapter, not re-own pure policy: {forbidden}"
        );
    }
    assert!(
        orchestration_input_diag.contains("input_diag::format_key_event(")
            && orchestration_input_diag.contains("input_diag::format_payload_event(")
            && orchestration_input_diag.contains("input_diag::format_byte_event(")
            && orchestration_input_diag.contains("input_diag::format_transform_event(")
            && orchestration_input_diag.contains("input_diag::format_prompt_detection("),
        "orchestration input_diag should call focused formatters directly"
    );

    for relative in [
        "src/queue_dispatch.rs",
        "agent-doc-orchestration/src/run.rs",
        "agent-doc-orchestration/src/route.rs",
        "agent-doc-orchestration/src/sessions.rs",
        "agent-doc-orchestration/src/start/run.rs",
        "agent-doc-orchestration/src/start.rs",
        "agent-doc-orchestration/src/start/idle_watch.rs",
        "agent-doc-orchestration/src/start/supervisor_io.rs",
        "agent-doc-orchestration/src/supervisor/pty.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            !source.contains("crate::input_diag::verbose_enabled(")
                && !source.contains("agent_doc_orchestration::input_diag::verbose_enabled(")
                && !source.contains("crate::input_diag::KeyEventMeta")
                && !source.contains("agent_doc_orchestration::input_diag::KeyEventMeta"),
            "{relative} should call focused input diagnostic gates/data directly"
        );
    }
    let route_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/route.rs")).unwrap();
    assert!(
        !route_source.contains("const EDITOR_ROUTE_ATTEMPT_ID_ENV"),
        "route diagnostics should use the focused editor route attempt-id env constant"
    );

    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-tmux-commands input diagnostic policy must stay free of orchestration, git, editor IPC, sqlite, or tmux-router effects"
        );
    }
}

#[test]
fn test_agent_doc_document_owns_status_projection_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let document_status =
        fs::read_to_string(manifest_dir.join("agent-doc-document/src/status_projection.rs"))
            .unwrap();
    for required in [
        "pub const STALE_SUPERVISOR_STATUS_MARKER",
        "pub fn reconcile_top_backlog_status_content",
        "pub fn apply_stale_supervisor_marker",
        "pub fn reconcile_stale_supervisor_status_content",
    ] {
        assert!(
            document_status.contains(required),
            "agent-doc-document must own pure status projection policy: {required}"
        );
    }

    let status_cmd =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/status_cmd.rs")).unwrap();
    for forbidden in [
        "STALE_SUPERVISOR_STATUS_MARKER",
        "fn first_live_backlog_id",
        "fn extract_status_top_backlog_id",
        "fn replace_top_backlog_sentence",
        "fn reconcile_top_backlog_status_content",
        "fn apply_stale_supervisor_marker",
        "fn reconcile_stale_supervisor_status_content",
    ] {
        assert!(
            !status_cmd.contains(forbidden),
            "status_cmd must stay a writeback adapter, not a status projection facade: {forbidden}"
        );
    }

    for relative in [
        "agent-doc-orchestration/src/compact.rs",
        "agent-doc-orchestration/src/repair.rs",
        "agent-doc-orchestration/src/backlog_cmd.rs",
        "agent-doc-orchestration/src/preflight/maintenance.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            source.contains("agent_doc_document::status_projection::"),
            "{relative} should call focused document status projection directly"
        );
        assert!(
            !source.contains("status_cmd::reconcile")
                && !source.contains("status_cmd::STALE_SUPERVISOR_STATUS_MARKER"),
            "{relative} must not route status projection through status_cmd"
        );
    }
}

#[test]
fn test_agent_doc_document_owns_transient_marker_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let transient_markers =
        fs::read_to_string(manifest_dir.join("agent-doc-document/src/transient_markers.rs"))
            .unwrap();
    for required in [
        "pub fn strip_boundary_markers(",
        "pub fn strip_head_markers(",
        "pub fn strip_guard_markers(",
        "pub fn normalize_transient_agent_doc_markers(",
        "pub fn strip_re_heading_attribution(",
        "pub fn normalize_post_commit_re_heading_drift(",
    ] {
        assert!(
            transient_markers.contains(required),
            "agent-doc-document must own transient marker document policy: {required}"
        );
    }
    let document_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-document/src/lib.rs")).unwrap();
    assert!(
        document_lib.contains("pub mod transient_markers;"),
        "agent-doc-document should expose transient marker policy through its owning module"
    );

    for relative in [
        "agent-doc-orchestration/src/git/normalize.rs",
        "agent-doc-orchestration/src/preflight.rs",
        "agent-doc-orchestration/src/session_check.rs",
        "agent-doc-orchestration/src/session_check/detect.rs",
        "agent-doc-orchestration/src/session_check/closeout_guards.rs",
        "agent-doc-orchestration/src/write.rs",
        "agent-doc-orchestration/src/write/converge.rs",
        "agent-doc-orchestration/src/write/materialize.rs",
        "agent-doc-orchestration/src/write/run_entry.rs",
        "agent-doc-orchestration/src/write/ipc/transport.rs",
        "agent-doc-orchestration/src/flow/closeout.rs",
        "agent-doc-orchestration/src/graph.rs",
        "agent-doc-orchestration/src/realtime_model.rs",
        "agent-doc-orchestration/src/repair.rs",
        "agent-doc-document-realtime/src/write_policy.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            !source.contains("crate::git::normalize_transient_agent_doc_markers")
                && !source.contains("crate::git::strip_guard_markers")
                && !source.contains("git::normalize_transient_agent_doc_markers")
                && !source.contains("git::strip_guard_markers"),
            "{relative} must call focused transient marker policy directly, not through git"
        );
    }

    let git_normalize =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/git/normalize.rs"))
            .unwrap();
    let git_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/git.rs")).unwrap();
    let realtime_write_policy =
        fs::read_to_string(manifest_dir.join("agent-doc-document-realtime/src/write_policy.rs"))
            .unwrap();
    for forbidden in [
        "pub fn normalize_transient_agent_doc_markers(",
        "fn normalize_transient_agent_doc_markers(",
        "pub fn normalize_post_commit_re_heading_drift(",
        "fn normalize_post_commit_re_heading_drift(",
        "fn strip_re_heading_attribution(",
        "fn strip_boundary_markers(",
    ] {
        assert!(
            !git_normalize.contains(forbidden) && !realtime_write_policy.contains(forbidden),
            "transient marker policy must not be re-owned outside agent-doc-document: {forbidden}"
        );
    }
    for forbidden in [
        "fn strip_head_markers(",
        "fn strip_guard_markers(",
        "fn code_block_byte_ranges(",
    ] {
        assert!(
            !git_source.contains(forbidden),
            "git must not re-own transient marker stripping policy: {forbidden}"
        );
    }
}

#[test]
fn test_agent_doc_element_exchange_owns_exchange_prompt_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let exchange_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-element-exchange/src/lib.rs")).unwrap();
    for required in [
        "pub fn exchange_content_len",
        "pub fn exchange_content",
        "pub fn exchange_component",
        "pub fn normalized_prompt_text",
        "pub fn is_markdown_heading_line",
        "pub fn normalized_prompt_counts",
        "pub fn split_line_segment",
        "pub fn is_code_fence_delimiter",
        "pub fn response_aware_user_prompt_counts",
        "pub fn user_prompt_count_growth",
        "pub fn exchange_has_live_user_edit",
        "pub fn exchange_prompt_prefix_count",
        "pub fn exchange_prompt_text_duplicated",
        "pub fn normalization_target_counts",
        "pub fn exchange_user_region",
        "pub fn is_exchange_response_heading_for_prefix_repair",
        "pub fn is_prefixed_exchange_response_heading_for_prefix_repair",
        "pub fn normalization_target_matches_line",
        "pub fn starts_prompt_run_after_response",
        "pub fn starts_targeted_prompt_repair_after_response",
        "pub fn starts_targeted_or_prefixed_prompt_repair_after_response",
        "pub fn exchange_prompt_prefix_eligible_lines",
        "pub struct PromptLineInfo",
        "pub fn exchange_prompt_reconciliation_infos",
        "pub fn prompt_reconciliation_counts",
        "pub fn last_exchange_boundary_tail_start",
        "pub fn probable_live_prompt_prefix_variant",
        "pub fn dedupe_live_prompt_prefix_variants_in_exchange_tail",
        "pub fn dedupe_adjacent_prompt_prefix_duplicates_in_exchange",
        "pub fn dedupe_prompt_lines_against_before_exchange",
        "pub fn extract_post_commit_normalization_targets",
        "pub fn normalize_exchange_prefixes_for_targets",
    ] {
        assert!(
            exchange_lib.contains(required),
            "agent-doc-element-exchange must own exchange prompt/content policy: {required}"
        );
    }

    let orchestration_cargo =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/Cargo.toml")).unwrap();
    assert!(
        orchestration_cargo.contains("agent-doc-element-exchange"),
        "orchestration must depend on the focused exchange element crate directly"
    );

    let write_exchange_reconcile = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/write/exchange_reconcile.rs"),
    )
    .unwrap();
    assert!(
        write_exchange_reconcile.contains("use agent_doc_element_exchange::{"),
        "write exchange reconciliation should import exchange policy from the focused crate"
    );

    let write_normalize =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/normalize.rs"))
            .unwrap();
    assert!(
        write_normalize.contains("use agent_doc_element_exchange::{"),
        "write normalization should import exchange prefix policy from the focused crate"
    );

    let write_ipc =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/ipc.rs")).unwrap();
    assert!(
        write_ipc.contains("use agent_doc_element_exchange::{"),
        "write IPC should import exchange prefix policy from the focused crate"
    );

    let write_ipc_transport =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/ipc/transport.rs"))
            .unwrap();
    assert!(
        write_ipc_transport.contains("use agent_doc_element_exchange::{"),
        "write IPC transport should import exchange prefix policy from the focused crate"
    );

    let orchestration_policy_sources = [
        ("write/exchange_reconcile.rs", write_exchange_reconcile),
        ("write/normalize.rs", write_normalize),
        ("write/ipc.rs", write_ipc),
        ("write/ipc/transport.rs", write_ipc_transport),
    ];
    for forbidden in [
        "pub(crate) fn extract_exchange_content_len",
        "pub(crate) fn exchange_content(",
        "pub(crate) fn normalized_prompt_text",
        "pub(crate) fn is_markdown_heading_line",
        "pub(crate) fn normalized_prompt_counts",
        "pub(crate) fn split_line_segment",
        "pub(crate) fn is_exchange_code_fence_delimiter",
        "pub(crate) fn response_aware_user_prompt_counts",
        "pub(crate) fn user_prompt_count_growth",
        "pub(crate) fn exchange_has_live_user_edit",
        "pub(crate) fn exchange_prompt_prefix_count",
        "pub(crate) fn exchange_prompt_text_duplicated",
        "pub(crate) fn exchange_user_region",
        "pub(crate) fn is_exchange_response_heading_for_prefix_repair",
        "pub(crate) fn is_prefixed_exchange_response_heading_for_prefix_repair",
        "pub(crate) fn normalization_target_matches_line",
        "pub(crate) fn starts_prompt_run_after_response",
        "pub(crate) fn starts_targeted_prompt_repair_after_response",
        "pub(crate) fn starts_targeted_or_prefixed_prompt_repair_after_response",
        "pub(crate) fn exchange_prompt_prefix_eligible_lines",
        "pub(crate) struct PromptLineInfo",
        "pub(crate) fn exchange_prompt_reconciliation_infos",
        "pub(crate) fn prompt_reconciliation_counts",
        "pub(crate) fn last_exchange_boundary_tail_start",
        "pub(crate) fn probable_live_prompt_prefix_variant",
        "pub(crate) fn dedupe_live_prompt_prefix_variants_in_exchange_tail",
        "pub(crate) fn dedupe_adjacent_prompt_prefix_duplicates_in_exchange",
        "pub(crate) fn dedupe_prompt_lines_against_before_exchange",
        "pub fn extract_post_commit_normalization_targets",
        "pub fn normalize_exchange_prefixes_for_targets",
        "pub(crate) fn normalization_target_counts",
    ] {
        for (path, source) in &orchestration_policy_sources {
            assert!(
                !source.contains(forbidden),
                "{path} must stay an adapter, not re-own or facade exchange element policy: {forbidden}"
            );
        }
    }
}

#[test]
fn test_agent_doc_document_owns_active_identity_projection_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let active_identity =
        fs::read_to_string(manifest_dir.join("agent-doc-document/src/active_identity.rs")).unwrap();
    for required in [
        "pub fn document_active_identities",
        "pub fn detect_identity_collisions",
        "pub fn identity_collision_for_new_id",
    ] {
        assert!(
            active_identity.contains(required),
            "agent-doc-document must own active identity projection policy: {required}"
        );
    }

    let document_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-document/src/lib.rs")).unwrap();
    assert!(
        document_lib.contains("pub mod active_identity;"),
        "agent-doc-document should expose active identity projection through its owning module"
    );

    let preflight =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/preflight.rs")).unwrap();
    for forbidden in [
        "pub fn document_active_identities",
        "pub fn detect_identity_collisions",
        "pub fn identity_collision_for_new_id",
        "fn document_active_identities",
        "fn detect_identity_collisions",
        "fn identity_collision_for_new_id",
    ] {
        assert!(
            !preflight.contains(forbidden),
            "preflight must stay a warning adapter, not an active identity policy facade: {forbidden}"
        );
    }
    assert!(
        preflight.contains("agent_doc_document::active_identity::detect_identity_collisions"),
        "preflight should call focused active identity collision detection directly"
    );

    let backlog_cmd =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/backlog_cmd.rs"))
            .unwrap();
    assert!(
        backlog_cmd.contains("agent_doc_document::active_identity::identity_collision_for_new_id"),
        "backlog_cmd should call focused active identity collision enforcement directly"
    );
    assert!(
        !backlog_cmd.contains("crate::preflight::identity_collision_for_new_id"),
        "backlog_cmd must not route active identity policy through preflight"
    );
}

#[test]
fn test_agent_doc_document_owns_markdown_outline_projection_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let outline_projection =
        fs::read_to_string(manifest_dir.join("agent-doc-document/src/outline_projection.rs"))
            .unwrap();
    for required in [
        "pub struct MarkdownOutlineSection",
        "pub fn project_markdown_outline",
        "pub fn render_markdown_outline_text",
        "fn collect_headings",
    ] {
        assert!(
            outline_projection.contains(required),
            "agent-doc-document must own pure markdown outline projection: {required}"
        );
    }
    let document_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-document/src/lib.rs")).unwrap();
    assert!(
        document_lib.contains("pub mod outline_projection;"),
        "agent-doc-document should expose outline projection through its owning module"
    );

    assert!(
        !manifest_dir.join("src/outline.rs").exists(),
        "root crate must not keep the old outline parser module or facade"
    );
    let main_source = fs::read_to_string(manifest_dir.join("src/main.rs")).unwrap();
    assert!(
        !main_source.contains("mod outline;") && !main_source.contains("outline::run"),
        "root main must route outline through the adapter, not the old parser module"
    );

    let adapter = fs::read_to_string(manifest_dir.join("src/outline_cmd.rs")).unwrap();
    for forbidden in [
        "pulldown_cmark",
        "fn parse_sections",
        "fn collect_headings",
        "struct Section",
    ] {
        assert!(
            !adapter.contains(forbidden),
            "outline_cmd must stay an IO/stdout adapter, not re-own outline projection: {forbidden}"
        );
    }
    assert!(
        adapter.contains("agent_doc_document::outline_projection::project_markdown_outline")
            && adapter
                .contains("agent_doc_document::outline_projection::render_markdown_outline_text"),
        "outline_cmd should call focused document outline projection directly"
    );

    let root_manifest = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let root_toml: toml::Value = toml::from_str(&root_manifest).unwrap();
    let root_deps = root_toml["dependencies"].as_table().unwrap();
    assert!(
        root_deps.contains_key("agent-doc-document"),
        "root CLI adapter should depend on focused document projection crate"
    );
    assert!(
        !root_deps.contains_key("pulldown-cmark"),
        "root crate must not depend on pulldown-cmark after outline projection extraction"
    );

    let document_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-document/Cargo.toml")).unwrap();
    let document_toml: toml::Value = toml::from_str(&document_manifest).unwrap();
    let document_deps = document_toml["dependencies"].as_table().unwrap();
    assert!(
        document_deps.contains_key("pulldown-cmark"),
        "agent-doc-document should own markdown parser dependency for outline projection"
    );
}

#[test]
fn test_agent_doc_document_realtime_owns_ack_mismatch_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let realtime_write_policy =
        fs::read_to_string(manifest_dir.join("agent-doc-document-realtime/src/write_policy.rs"))
            .unwrap();
    for required in [
        "pub enum AckMismatchRecovery",
        "pub fn classify_ack_mismatch_recovery",
    ] {
        assert!(
            realtime_write_policy.contains(required),
            "agent-doc-document-realtime must own ACK-mismatch recovery policy: {required}"
        );
    }

    let converge_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/converge.rs"))
            .unwrap();
    for forbidden in [
        "enum AckMismatchRecovery",
        "fn classify_ack_mismatch_recovery",
        "fn missing_agent_response_block",
        "fn stale_queue_prompt_exchange_artifact",
        "fn blank_components_named",
    ] {
        assert!(
            !converge_source.contains(forbidden),
            "write::converge must not re-own ACK-mismatch recovery policy: {forbidden}"
        );
    }
    assert!(
        converge_source.contains("agent_doc_document_realtime::write_policy::{")
            && converge_source.contains("classify_ack_mismatch_recovery("),
        "write::converge should call the focused realtime ACK-mismatch policy directly"
    );
}

#[test]
fn test_agent_doc_document_realtime_owns_safe_mutation_classification() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let realtime_write_policy =
        fs::read_to_string(manifest_dir.join("agent-doc-document-realtime/src/write_policy.rs"))
            .unwrap();
    for required_snippet in [
        "pub fn classify_safe_out_of_band_agent_doc_mutation",
        "pub fn classify_committed_historical_agent_doc_mutation",
        "pub fn detect_reintroduced_reaped_pending_ids",
        "pub fn is_empty_template_scaffold_snapshot",
        "pub fn is_safe_user_follow_up_exchange_growth",
    ] {
        assert!(
            realtime_write_policy.contains(required_snippet),
            "agent-doc-document-realtime must own safe mutation classification: {required_snippet}"
        );
    }

    assert!(
        !manifest_dir
            .join("agent-doc-orchestration/src/git/safe_mutation.rs")
            .exists(),
        "agent-doc-orchestration must not keep a safe_mutation policy module"
    );

    let git_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/git.rs")).unwrap();
    for forbidden_snippet in [
        "mod safe_mutation",
        "pub use safe_mutation",
        "fn classify_safe_out_of_band_agent_doc_mutation",
        "fn classify_committed_historical_agent_doc_mutation(",
        "classify_committed_historical_agent_doc_mutation_policy",
        "fn detect_reintroduced_reaped_pending_ids",
        "fn is_empty_template_scaffold_snapshot",
        "fn is_safe_user_follow_up_exchange_growth",
    ] {
        assert!(
            !git_source.contains(forbidden_snippet),
            "git.rs must not define or reexport safe mutation classification: {forbidden_snippet}"
        );
    }
    assert!(
        git_source.contains("agent_doc_document_realtime::write_policy::{")
            && git_source.contains("classify_safe_out_of_band_agent_doc_mutation")
            && git_source.contains("classify_committed_historical_agent_doc_mutation"),
        "git.rs should call the focused realtime safe mutation policy directly"
    );
}

#[test]
fn test_agent_doc_document_realtime_owns_authority_boundaries() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let realtime_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-document-realtime/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&realtime_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();

    assert!(
        manifest_dir
            .join("agent-doc-document-realtime/src/write_authority.rs")
            .exists()
    );
    assert!(
        manifest_dir
            .join("agent-doc-document-realtime/src/write_policy.rs")
            .exists()
    );
    assert!(
        manifest_dir
            .join("agent-doc-document-realtime/src/watch_authority.rs")
            .exists()
    );
    assert!(
        manifest_dir
            .join("agent-doc-document-realtime/src/read_authority.rs")
            .exists()
    );
    assert!(
        manifest_dir
            .join("agent-doc-document-realtime/src/session_ops.rs")
            .exists()
    );
    let realtime_session_ops =
        fs::read_to_string(manifest_dir.join("agent-doc-document-realtime/src/session_ops.rs"))
            .unwrap();
    assert!(
        realtime_session_ops.contains("pub enum SessionOpKind"),
        "agent-doc-document-realtime must own document session operation vocabulary"
    );
    assert!(
        manifest_dir
            .join("agent-doc-document-realtime/src/crdt_authority.rs")
            .exists()
    );
    let orchestration_realtime =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/realtime_model.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub enum DocAuthority",
        "pub struct BufferState",
        "pub struct Reconciliation",
        "pub fn reconcile_current_doc",
        "pub fn current_doc",
        "pub fn buffer_supersedes",
    ] {
        assert!(
            !orchestration_realtime.contains(forbidden_snippet),
            "orchestration must not re-own pure realtime read-authority policy: {forbidden_snippet}"
        );
    }
    let orchestration_session_actor =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/session_actor.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub enum SessionOpKind",
        "enum SessionOpKind",
        "pub use agent_doc_document_realtime::session_ops::SessionOpKind",
    ] {
        assert!(
            !orchestration_session_actor.contains(forbidden_snippet),
            "orchestration must not re-own or facade session operation vocabulary: {forbidden_snippet}"
        );
    }
    for relative in [
        "agent-doc-orchestration/src/session_actor.rs",
        "agent-doc-orchestration/src/write_queue.rs",
        "agent-doc-orchestration/src/document_watcher.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            source.contains("agent_doc_document_realtime::session_ops::SessionOpKind"),
            "{relative} should import SessionOpKind from the focused realtime crate"
        );
    }
    let realtime_write_policy =
        fs::read_to_string(manifest_dir.join("agent-doc-document-realtime/src/write_policy.rs"))
            .unwrap();
    for required_snippet in [
        "pub struct VisibleWriteTypingFacts",
        "pub enum VisibleWriteDecision",
        "pub fn decide_visible_write_after_typing",
        "pub struct FullContentSourceProof",
        "pub fn decide_full_content_visible_replacement",
        "pub enum FullContentScopeRejection",
        "pub fn full_content_scope_rejection_reason",
        "pub enum ReconnectBufferDecision",
        "pub fn decide_reconnect_buffer",
        "pub enum EditorlessDiskFallbackDecision",
        "pub fn decide_editorless_disk_fallback",
    ] {
        assert!(
            realtime_write_policy.contains(required_snippet),
            "agent-doc-document-realtime must own write/reconnect policy: {required_snippet}"
        );
    }
    let realtime_crdt_authority =
        fs::read_to_string(manifest_dir.join("agent-doc-document-realtime/src/crdt_authority.rs"))
            .unwrap();
    for required_snippet in [
        "pub enum CrdtAuthority",
        "pub fn authority_for(",
        "pub fn authority_from_liveness(",
        "pub fn sync_under_authority(",
        "pub fn commit_barrier_under_authority(",
    ] {
        assert!(
            realtime_crdt_authority.contains(required_snippet),
            "agent-doc-document-realtime must own CRDT authority policy: {required_snippet}"
        );
    }
    let orchestration_document_mutation = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/flow/document_mutation.rs"),
    )
    .unwrap();
    for forbidden_snippet in [
        "pub struct VisibleWriteTypingFacts",
        "pub enum VisibleWriteDecision",
        "pub fn decide_visible_write_after_typing",
        "pub struct FullContentSourceProof",
        "pub fn full_content_source_proof",
        "pub fn decide_full_content_visible_replacement",
        "pub enum FullContentScopeRejection",
        "pub fn full_content_scope_rejection_reason",
        "pub enum ReconnectBufferDecision",
        "pub fn decide_reconnect_buffer",
        "pub enum EditorlessDiskFallbackDecision",
        "pub fn decide_editorless_disk_fallback",
        "pub use agent_doc_document_realtime",
    ] {
        assert!(
            !orchestration_document_mutation.contains(forbidden_snippet),
            "orchestration must not re-own or facade realtime write policy: {forbidden_snippet}"
        );
    }
    let orchestration_crdt_authority =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/crdt_authority.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub enum CrdtAuthority",
        "pub fn authority_for(",
        "pub fn authority_from_liveness(",
        "pub fn sync_under_authority(",
        "pub fn commit_barrier_under_authority(",
        "pub use agent_doc_document_realtime::crdt_authority",
    ] {
        assert!(
            !orchestration_crdt_authority.contains(forbidden_snippet),
            "orchestration must not re-own or facade CRDT authority policy: {forbidden_snippet}"
        );
    }
    let write_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write.rs")).unwrap();
    assert!(
        write_source.contains(
            "agent_doc_document_realtime::write_policy::decide_visible_write_after_typing"
        ),
        "orchestration write path should call the focused realtime policy directly"
    );
    let write_ipc_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/ipc.rs")).unwrap();
    assert!(
        write_ipc_source
            .contains("agent_doc_document_realtime::write_policy::FullContentSourceProof"),
        "normalization repair payloads should use the focused source-proof type directly"
    );
    let write_ipc_transport_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/ipc/transport.rs"))
            .unwrap();
    assert!(
        write_ipc_transport_source.contains(
            "agent_doc_document_realtime::write_policy::full_content_scope_rejection_reason"
        ),
        "full-content scope checks should call the focused realtime policy directly"
    );
    for forbidden_snippet in [
        "pub(crate) fn frontmatter_mode_is_explicit_template",
        "pub(crate) fn content_declares_template_frontmatter",
        "pub(crate) fn content_has_agent_components",
        "pub(crate) fn full_content_ipc_scope_rejection_reason",
    ] {
        assert!(
            !write_ipc_transport_source.contains(forbidden_snippet),
            "write/ipc/transport.rs must not re-own or facade full-content scope policy: {forbidden_snippet}"
        );
    }
    for relative in [
        "agent-doc-orchestration/src/crdt_relay.rs",
        "agent-doc-orchestration/src/crdt_relay_host.rs",
        "src/sim_world.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative)).unwrap();
        assert!(
            source.contains("agent_doc_document_realtime::crdt_authority::CrdtAuthority"),
            "{relative} must import CrdtAuthority from the focused realtime crate"
        );
        assert!(
            !source.contains("agent_doc_orchestration::crdt_authority::CrdtAuthority")
                && !source.contains("use crate::crdt_authority::CrdtAuthority"),
            "{relative} must not import CrdtAuthority through orchestration"
        );
    }
    for forbidden in [
        "agent-doc-core",
        "agent-doc-orchestration",
        "git2",
        "interprocess",
        "notify",
        "rusqlite",
        "tmux-router",
    ] {
        assert!(
            !dependencies.contains_key(forbidden),
            "agent-doc-document-realtime authority policy must not depend on core, orchestration, git, editor IPC, sqlite, or tmux crates"
        );
    }
}

#[test]
fn test_agent_doc_template_owns_patchback_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let template_patchback =
        fs::read_to_string(manifest_dir.join("agent-doc-template/src/patchback.rs")).unwrap();
    for required_snippet in [
        "pub enum PatchbackShape",
        "pub struct PatchbackShapeFacts",
        "pub fn classify_patchback_shape",
        "pub fn raw_component_block_count",
        "pub fn patchback_marker_count_outside_code",
        "pub struct TemplatePatchbackPlan",
        "pub fn parse_template_patchback_plan",
        "pub enum OrchestratePatchbackRejectReason",
        "pub fn classify_orchestrate_patchback",
        "pub fn classify_orchestrate_plain_response",
        "pub fn enforce_orchestrate_patchback_contract",
        "pub enum ChildPatchbackNormalizationDecision",
        "pub struct ChildPatchbackNormalization",
        "pub fn normalize_child_template_response",
    ] {
        assert!(
            template_patchback.contains(required_snippet),
            "agent-doc-template must own template patchback policy: {required_snippet}"
        );
    }

    let template_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-template/src/lib.rs")).unwrap();
    assert!(
        template_lib.contains("pub mod patchback;"),
        "agent-doc-template should expose patchback policy through its owning module"
    );
    assert!(
        !template_lib.contains("pub use patchback"),
        "agent-doc-template should not add a patchback root facade"
    );

    let flow_types =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/flow/types.rs")).unwrap();
    let flow_document_mutation = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/flow/document_mutation.rs"),
    )
    .unwrap();
    let write_materialize =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/materialize.rs"))
            .unwrap();
    let orchestration_batch = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/flow/orchestration_batch.rs"),
    )
    .unwrap();
    let orchestration_sources = [
        (
            "agent-doc-orchestration/src/flow/types.rs",
            flow_types.as_str(),
        ),
        (
            "agent-doc-orchestration/src/flow/document_mutation.rs",
            flow_document_mutation.as_str(),
        ),
        (
            "agent-doc-orchestration/src/write/materialize.rs",
            write_materialize.as_str(),
        ),
        (
            "agent-doc-orchestration/src/flow/orchestration_batch.rs",
            orchestration_batch.as_str(),
        ),
    ];
    for (source, content) in orchestration_sources {
        for forbidden_snippet in [
            "pub enum PatchbackShape",
            "pub struct PatchbackShapeFacts",
            "pub fn classify_patchback_shape",
            "pub fn raw_component_block_count",
            "pub fn patchback_marker_count_outside_code",
            "pub struct TemplatePatchbackPlan",
            "pub enum OrchestratePatchbackRejectReason",
            "pub fn classify_orchestrate_patchback",
            "pub fn classify_orchestrate_plain_response",
            "pub fn enforce_orchestrate_patchback_contract",
            "pub enum ChildPatchbackNormalizationDecision",
            "pub struct ChildPatchbackNormalization",
            "pub fn normalize_child_template_response",
        ] {
            assert!(
                !content.contains(forbidden_snippet),
                "orchestration must not re-own or facade template patchback policy in {source}: {forbidden_snippet}"
            );
        }
    }

    assert!(
        orchestration_batch.contains("patchback::ChildPatchbackNormalization"),
        "orchestration batch events should accept focused template patchback normalization results directly"
    );
    let write_run_entry =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/run_entry.rs"))
            .unwrap();
    assert!(
        write_run_entry
            .contains("agent_doc_template::patchback::enforce_orchestrate_patchback_contract"),
        "write run entry should enforce orchestrate patchback contracts through the focused template API"
    );
}

#[test]
fn test_agent_doc_template_owns_response_materialization_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let template_response_materialization =
        fs::read_to_string(manifest_dir.join("agent-doc-template/src/response_materialization.rs"))
            .unwrap();
    for required_snippet in [
        "pub struct TemplateResponseWriteProof",
        "pub fn template_response_write_proof",
        "pub fn ensure_template_response_write_proof",
        "pub fn same_ignoring_trailing_newlines",
        "pub fn serialize_template_response",
        "pub fn response_materialization_probe",
        "pub fn response_materialization_probe_from_response",
        "pub fn strip_partial_response_materialization_from_exchange",
        "pub fn materialized_template_response",
        "pub fn push_materialization_segment",
        "pub fn reject_marker_response_with_zero_patches",
        "pub fn sanitize_template_patchback_response",
    ] {
        assert!(
            template_response_materialization.contains(required_snippet),
            "agent-doc-template must own template response materialization policy: {required_snippet}"
        );
    }

    let template_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-template/src/lib.rs")).unwrap();
    assert!(
        template_lib.contains("pub mod response_materialization;"),
        "agent-doc-template should expose response materialization through its owning module"
    );
    assert!(
        !template_lib.contains("pub use response_materialization"),
        "agent-doc-template should not add a response materialization root facade"
    );

    let write_materialize =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/materialize.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub struct TemplateResponseWriteProof",
        "pub(crate) struct TemplateResponseWriteProof",
        "pub fn template_response_write_proof",
        "pub(crate) fn template_response_write_proof",
        "pub fn ensure_template_response_write_proof",
        "pub(crate) fn ensure_template_response_write_proof",
        "pub fn same_ignoring_trailing_newlines",
        "pub(crate) fn same_ignoring_trailing_newlines",
        "pub fn serialize_template_response",
        "pub(crate) fn serialize_template_response",
        "pub fn response_materialization_probe(",
        "pub(crate) fn response_materialization_probe(",
        "pub fn response_materialization_probe_from_response",
        "pub(crate) fn response_materialization_probe_from_response",
        "pub fn strip_partial_response_materialization_from_exchange",
        "pub(crate) fn strip_partial_response_materialization_from_exchange",
        "pub fn materialized_template_response",
        "pub(crate) fn materialized_template_response",
        "pub fn push_materialization_segment",
        "pub(crate) fn push_materialization_segment",
        "pub fn reject_marker_response_with_zero_patches",
        "pub(crate) fn reject_marker_response_with_zero_patches",
        "pub fn sanitize_template_patchback_response",
        "pub(crate) fn sanitize_template_patchback_response",
        "sanitize_template_patchback_response_for_write",
    ] {
        assert!(
            !write_materialize.contains(forbidden_snippet),
            "orchestration must not re-own or facade template response materialization policy: {forbidden_snippet}"
        );
    }
    assert!(
        write_materialize.contains("use agent_doc_template::response_materialization::{"),
        "write materialization adapters should import the focused response materialization API directly"
    );

    let focused_callers = [
        "agent-doc-orchestration/src/run.rs",
        "agent-doc-orchestration/src/write/run_entry.rs",
        "agent-doc-orchestration/src/write/ipc/transport.rs",
        "agent-doc-orchestration/src/write/ipc.rs",
        "agent-doc-orchestration/src/write/materialize.rs",
        "agent-doc-orchestration/src/write/normalize.rs",
    ];
    for relative_path in focused_callers {
        let source = fs::read_to_string(manifest_dir.join(relative_path)).unwrap();
        assert!(
            source.contains("agent_doc_template::response_materialization::"),
            "{relative_path} should call the focused response materialization API directly"
        );
    }
}

#[test]
fn test_agent_doc_template_owns_strict_response_heading_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let template_response_materialization =
        fs::read_to_string(manifest_dir.join("agent-doc-template/src/response_materialization.rs"))
            .unwrap();
    for required_snippet in [
        "pub fn ensure_strict_template_response_heading",
        "pub fn ensure_strict_template_response_heading_for_current_doc",
        "pub fn template_response_has_heading",
        "pub fn live_exchange_tail_proves_streamed_response_heading",
        "pub fn offset_after_last_prompt_line",
        "pub fn response_text_has_heading",
    ] {
        assert!(
            template_response_materialization.contains(required_snippet),
            "agent-doc-template must own strict template response-heading policy: {required_snippet}"
        );
    }

    let write_materialize =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/materialize.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub use agent_doc_template::response_materialization",
        "pub fn ensure_strict_template_response_heading",
        "pub(crate) fn ensure_strict_template_response_heading",
        "fn template_response_has_heading",
        "fn live_exchange_tail_proves_streamed_response_heading",
        "fn offset_after_last_prompt_line",
        "fn response_text_has_heading",
    ] {
        assert!(
            !write_materialize.contains(forbidden_snippet),
            "write/materialize.rs must not facade or re-own strict heading policy: {forbidden_snippet}"
        );
    }

    for relative_path in [
        "agent-doc-orchestration/src/write/run_entry.rs",
        "agent-doc-orchestration/src/write/normalize.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative_path)).unwrap();
        assert!(
            source.contains(
                "agent_doc_template::response_materialization::ensure_strict_template_response_heading"
            ),
            "{relative_path} should call strict heading policy through agent-doc-template directly"
        );
    }
}

#[test]
fn test_agent_doc_queue_owns_queue_head_classification_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let queue_response =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_response.rs")).unwrap();
    for required_snippet in [
        "pub fn head_id_names_tracked_directive_item",
        "pub fn head_id_is_registered_preset",
        "pub fn active_queue_head_is_registered_preset",
        "pub fn queue_head_is_bare_do_directive",
        "pub fn queue_prompt_text_is_queue_activation_trigger",
        "pub fn queue_prompt_text_is_free_text",
        "pub fn queue_head_is_free_text_prompt",
    ] {
        assert!(
            queue_response.contains(required_snippet),
            "agent-doc-queue must own queue-head classification policy: {required_snippet}"
        );
    }

    let queue_consume =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/queue_consume.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub(crate) use agent_doc_queue::queue_response",
        "pub fn queue_head_is_free_text_prompt",
        "pub(crate) fn queue_head_is_free_text_prompt",
        "pub fn queue_prompt_text_is_free_text",
        "pub(crate) fn queue_prompt_text_is_free_text",
        "pub fn head_id_is_registered_preset",
        "pub(crate) fn head_id_is_registered_preset",
        "pub fn queue_head_is_bare_do_directive",
        "pub(crate) fn queue_head_is_bare_do_directive",
    ] {
        assert!(
            !queue_consume.contains(forbidden_snippet),
            "write/queue_consume.rs must not re-own or facade queue-head classification: {forbidden_snippet}"
        );
    }

    for relative_path in [
        "agent-doc-orchestration/src/queue_cmd.rs",
        "agent-doc-orchestration/src/project_controller/rpc.rs",
        "agent-doc-orchestration/src/session_check/queue_head_provenance_guards.rs",
        "agent-doc-orchestration/src/repair.rs",
        "agent-doc-orchestration/src/preflight/maintenance.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative_path)).unwrap();
        assert!(
            !source.contains("crate::write::queue_head_is_free_text_prompt")
                && !source.contains("crate::write::queue_prompt_text_is_free_text")
                && !source.contains("crate::write::head_id_is_registered_preset")
                && !source.contains("fn active_queue_head_is_registered_preset("),
            "{relative_path} should call queue classification through agent-doc-queue directly"
        );
    }
    let rpc_source = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/project_controller/rpc.rs"),
    )
    .unwrap();
    assert!(
        rpc_source
            .contains("agent_doc_queue::queue_response::active_queue_head_is_registered_preset("),
        "project_controller::rpc should call the queue-owned registered-preset classifier directly"
    );
}

#[test]
fn test_agent_doc_queue_owns_active_queue_head_projection_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let queue_heads =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_heads.rs")).unwrap();
    for required_snippet in [
        "pub fn active_queue_heads(",
        "pub fn active_free_text_queue_heads(",
        "pub fn active_queue_head_text(",
        "pub enum ActiveQueueHeadKind",
        "pub fn classify_active_queue_head(",
        "pub fn queue_skip_diagnostic_for_content(",
        "pub fn queue_head_has_explicit_completion_signal(",
        "pub fn explicit_queue_completion_ids(",
        "pub fn queue_head_matches_done_ids(",
        "pub fn free_text_queue_head_identity(",
        "pub fn committed_queue_contains_free_text_head(",
        "pub fn free_text_queue_head_is_completed_residue(",
        "pub fn is_do_directive(",
        "fn leads_with_bare_id_directive(",
    ] {
        assert!(
            queue_heads.contains(required_snippet),
            "agent-doc-queue must own active queue-head projection policy: {required_snippet}"
        );
    }

    for forbidden_snippet in [
        "pub fn active_queue_directive_heads(",
        "pub use active_queue_heads as active_queue_directive_heads",
    ] {
        assert!(
            !queue_heads.contains(forbidden_snippet),
            "agent-doc-queue must not keep the deprecated queue-head projection name: {forbidden_snippet}"
        );
    }

    let cycle_state =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/cycle_state.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub fn active_queue_directive_heads(",
        "pub fn active_free_text_queue_heads(",
        "fn is_do_directive(",
        "fn leads_with_bare_id_directive(",
        "pub use agent_doc_queue::queue_heads",
        "active_queue_directive_heads",
    ] {
        assert!(
            !cycle_state.contains(forbidden_snippet),
            "cycle_state must not re-own, facade, or preserve deprecated active queue-head policy: {forbidden_snippet}"
        );
    }
    assert!(
        cycle_state.contains("agent_doc_queue::queue_heads::active_queue_heads"),
        "cycle_state should call active queue-head projection through agent-doc-queue directly"
    );
    assert!(
        cycle_state.contains("agent_doc_queue::queue_heads::active_free_text_queue_heads"),
        "cycle_state should call free-text queue-head projection through agent-doc-queue directly"
    );

    let queue_consume =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/queue_consume.rs"))
            .unwrap();
    for forbidden_snippet in [
        "pub(crate) fn active_queue_head_text(",
        "pub(crate) fn queue_skip_diagnostic_for_content(",
        "pub(crate) fn queue_head_has_explicit_completion_signal(",
        "pub(crate) fn explicit_queue_completion_ids(",
        "pub(crate) fn queue_head_matches_done_ids(",
    ] {
        assert!(
            !queue_consume.contains(forbidden_snippet),
            "queue_consume must not re-own or facade active queue-head policy: {forbidden_snippet}"
        );
    }
    assert!(
        queue_consume.contains("agent_doc_queue::queue_heads::queue_skip_diagnostic_for_content"),
        "queue_consume should keep only the file-reading adapter for skip diagnostics"
    );

    let queue_cmd =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/queue_cmd.rs")).unwrap();
    for forbidden_snippet in [
        "enum HeadKind",
        "fn classify_active_head(",
        "agent_doc_queue::document_queue as queue",
        "agent_doc_queue::queue_response::queue_head_is_free_text_prompt",
    ] {
        assert!(
            !queue_cmd.contains(forbidden_snippet),
            "queue_cmd must not re-own explicit queue-head classification policy: {forbidden_snippet}"
        );
    }
    assert!(
        queue_cmd.contains("agent_doc_queue::queue_heads::{")
            && queue_cmd.contains("ActiveQueueHeadKind")
            && queue_cmd.contains("classify_active_queue_head"),
        "queue_cmd should call explicit queue-head classification through agent-doc-queue directly"
    );

    let queue_response =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_response.rs")).unwrap();
    assert!(
        !queue_response.contains("fn active_queue_head_text("),
        "queue_response must not duplicate active queue-head document projection"
    );
    assert!(
        queue_response.contains("crate::queue_heads::active_queue_head_text"),
        "queue_response should share active queue-head projection through queue_heads"
    );

    let queue_closeout_guard =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/queue_closeout_guard.rs"))
            .unwrap();
    for required_snippet in [
        "pub fn committed_queue_head_ids(",
        "pub fn committed_current_queue_head_ids(",
        "pub fn no_response_live_queue_head_ids(",
        "pub fn queue_head_removal_decision(",
        "pub struct FreeTextQueueHeadProvenanceDecision",
        "pub fn free_text_queue_head_provenance_decision(",
        "pub enum QueueHeadRemovalProofSource",
    ] {
        assert!(
            queue_closeout_guard.contains(required_snippet),
            "agent-doc-queue must own queue closeout guard policy: {required_snippet}"
        );
    }

    let queue_head_guards = fs::read_to_string(
        manifest_dir.join("agent-doc-orchestration/src/session_check/queue_head_guards.rs"),
    )
    .unwrap();
    let provenance_guards = fs::read_to_string(
        manifest_dir
            .join("agent-doc-orchestration/src/session_check/queue_head_provenance_guards.rs"),
    )
    .unwrap();
    for forbidden_snippet in [
        "pub(crate) fn committed_queue_head_ids(",
        "pub(crate) fn committed_current_queue_head_ids(",
        "fn committed_queue_head_ids(",
        "fn committed_current_queue_head_ids(",
        "fn normalized_free_text_queue_head_identity(",
        "fn committed_queue_contains_active_free_text_head(",
        "fn no_response_live_queue_head_ids(",
        "fn queue_head_removal_decision(",
        "fn free_text_queue_head_provenance_decision(",
        "queue_heads::free_text_queue_head_is_completed_residue(",
        "queue_heads::committed_queue_contains_free_text_head(",
        "free_text_head_answered_by_response(",
        "queue_continuation::is_recurring_imperative_head(",
    ] {
        assert!(
            !queue_head_guards.contains(forbidden_snippet)
                && !provenance_guards.contains(forbidden_snippet),
            "session_check must not re-own queue closeout/provenance policy: {forbidden_snippet}"
        );
    }
    assert!(
        queue_head_guards
            .contains("agent_doc_queue::queue_closeout_guard::no_response_live_queue_head_ids",)
            && provenance_guards
                .contains("agent_doc_queue::queue_closeout_guard::queue_head_removal_decision",)
            && provenance_guards.contains(
                "agent_doc_queue::queue_closeout_guard::free_text_queue_head_provenance_decision"
            ),
        "session_check should call queue closeout/provenance policy through agent-doc-queue directly"
    );

    let queue_manifest =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/Cargo.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&queue_manifest).unwrap();
    let dependencies = parsed["dependencies"].as_table().unwrap();
    for forbidden_dependency in [
        "agent-doc-orchestration",
        "tmux-router",
        "interprocess",
        "notify",
        "rusqlite",
        "git2",
    ] {
        assert!(
            !dependencies.contains_key(forbidden_dependency),
            "agent-doc-queue must keep active queue-head projection free of orchestration/effect dependencies: {forbidden_dependency}"
        );
    }
}

#[test]
fn test_agent_doc_queue_owns_route_dispatch_queue_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let route_dispatch =
        fs::read_to_string(manifest_dir.join("agent-doc-queue/src/route_dispatch.rs")).unwrap();
    for required_snippet in [
        "pub fn route_prompt_text_for_change(",
        "pub fn active_auto_route_queue_prompt_texts(",
        "pub fn operator_prioritize_route_prompt(",
        "pub fn prepare_route_dispatch_queue_update(",
        "pub fn activate_existing_route_queue_content(",
        "pub fn inactive_route_queue_head(",
        "pub fn committed_snapshot_backs_queue_head(",
        "pub enum RouteInactiveQueueHead",
        "pub struct RouteDispatchQueueUpdate",
    ] {
        assert!(
            route_dispatch.contains(required_snippet),
            "agent-doc-queue must own route-dispatch queue policy: {required_snippet}"
        );
    }

    let route_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/route.rs")).unwrap();
    let route_cycle_ack =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/route/cycle_ack.rs"))
            .unwrap();
    let preflight_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/preflight.rs")).unwrap();
    for forbidden_snippet in [
        "fn queue_prompt_text_for_route_change(",
        "fn operator_prioritize_route_prompt(",
        "fn route_queue_head_backed_by_committed_snapshot(",
        "fn strip_queue_component_auto_attr(",
        "fn insert_queue_component(",
        "document_queue::resolve_activation(",
        "document_queue::has_stop_fence_at_head(",
        "document_queue::time_gate_at_head(",
        "document_queue::marker_control(",
    ] {
        assert!(
            !route_source.contains(forbidden_snippet)
                && !route_cycle_ack.contains(forbidden_snippet),
            "route must stay an effect adapter and not re-own route-dispatch queue policy: {forbidden_snippet}"
        );
    }
    for forbidden_snippet in [
        "fn route_queue_prompt_texts(",
        "fn normalize_route_queue_prompt_text(",
    ] {
        assert!(
            !preflight_source.contains(forbidden_snippet),
            "preflight must stay an effect adapter and not re-own route queue prompt policy: {forbidden_snippet}"
        );
    }
    assert!(
        route_source
            .contains("agent_doc_queue::route_dispatch::prepare_route_dispatch_queue_update")
            && route_source.contains("agent_doc_queue::route_dispatch::inactive_route_queue_head")
            && route_source
                .contains("agent_doc_queue::route_dispatch::activate_existing_route_queue_content")
            && route_cycle_ack
                .contains("agent_doc_queue::route_dispatch::route_prompt_text_for_change")
            && preflight_source
                .contains("agent_doc_queue::route_dispatch::active_auto_route_queue_prompt_texts"),
        "route/preflight should call route-dispatch queue policy through agent-doc-queue directly"
    );
}

#[test]
fn test_agent_doc_document_realtime_owns_exchange_recovery_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let realtime_write_policy =
        fs::read_to_string(manifest_dir.join("agent-doc-document-realtime/src/write_policy.rs"))
            .unwrap();
    for required_snippet in [
        "pub fn exchange_change_is_safe_historical_reduction",
        "pub fn exchange_response_block_ranges",
        "pub fn live_prompt_drift_auto_recovery_safe",
        "pub fn live_prompt_drift_recovery_target",
        "pub fn snapshot_contains_dropped_prompt",
        "pub struct StaleSnapshotResetDrift",
        "pub fn stale_snapshot_reset_drift",
    ] {
        assert!(
            realtime_write_policy.contains(required_snippet),
            "agent-doc-document-realtime must own exchange/live-drift recovery policy: {required_snippet}"
        );
    }

    let converge =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/converge.rs"))
            .unwrap();
    let write_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write.rs")).unwrap();
    for forbidden_snippet in [
        "pub fn live_prompt_drift_auto_recovery_safe",
        "fn live_prompt_drift_recovery_target",
        "pub(crate) fn snapshot_contains_dropped_prompt",
        "fn snapshot_contains_dropped_prompt(",
        "fn exchange_change_is_safe_historical_reduction",
        "fn exchange_response_block_ranges",
        "pub(crate) fn stale_snapshot_reset_drift",
        "fn stale_snapshot_reset_drift(",
        "STALE_SNAPSHOT_RESET_DRIFT_MIN_BYTES",
        "STALE_SNAPSHOT_RESET_DRIFT_MAX_RATIO",
    ] {
        assert!(
            !converge.contains(forbidden_snippet),
            "write/converge.rs must not re-own realtime exchange/live-drift policy: {forbidden_snippet}"
        );
    }
    for forbidden_snippet in [
        "STALE_SNAPSHOT_RESET_DRIFT_MIN_BYTES",
        "STALE_SNAPSHOT_RESET_DRIFT_MAX_RATIO",
    ] {
        assert!(
            !write_source.contains(forbidden_snippet),
            "write.rs must not keep stale snapshot reset-drift threshold policy: {forbidden_snippet}"
        );
    }
    assert!(
        converge.contains("agent_doc_document_realtime::write_policy::{")
            && converge.contains("live_prompt_drift_recovery_target(")
            && converge.contains("stale_snapshot_reset_drift("),
        "write/converge.rs should adapt effect gates into focused realtime recovery policy"
    );
}

#[test]
fn test_agent_doc_controller_owns_route_text_predicates() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let controller_dispatch =
        fs::read_to_string(manifest_dir.join("agent-doc-controller/src/dispatch.rs")).unwrap();
    for required_snippet in [
        "pub fn is_codex_shell_search_blocker",
        "pub fn normalize_context_session",
        "pub fn is_stash_window_name",
    ] {
        assert!(
            controller_dispatch.contains(required_snippet),
            "agent-doc-controller must own route textual predicate policy: {required_snippet}"
        );
    }

    for relative_path in [
        "agent-doc-orchestration/src/route/busy_pane.rs",
        "agent-doc-orchestration/src/route/session_resolution.rs",
        "agent-doc-orchestration/src/sync.rs",
        "agent-doc-orchestration/src/resync.rs",
    ] {
        let source = fs::read_to_string(manifest_dir.join(relative_path)).unwrap();
        for forbidden_snippet in [
            "pub(crate) use agent_doc_controller::dispatch",
            "pub fn is_codex_shell_search_blocker",
            "pub(crate) fn is_codex_shell_search_blocker",
            "fn normalize_context_session",
            "fn is_stash_window_name(",
        ] {
            assert!(
                !source.contains(forbidden_snippet),
                "{relative_path} must not re-own or facade route textual predicate policy: {forbidden_snippet}"
            );
        }
    }
}

#[test]
fn test_agent_doc_template_owns_patch_sanitization_policy() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let template_sanitize =
        fs::read_to_string(manifest_dir.join("agent-doc-template/src/sanitize.rs")).unwrap();
    for required_snippet in [
        "pub fn sanitize_component_tags",
        "pub fn sanitize_patches",
        "pub fn sanitize_unmatched",
        "fn utf8_char_len",
        "fn find_comment_close",
    ] {
        assert!(
            template_sanitize.contains(required_snippet),
            "agent-doc-template must own template patch sanitization policy: {required_snippet}"
        );
    }

    let template_lib =
        fs::read_to_string(manifest_dir.join("agent-doc-template/src/lib.rs")).unwrap();
    assert!(
        template_lib.contains("pub mod sanitize;"),
        "agent-doc-template should expose sanitization through its owning module"
    );
    assert!(
        !template_lib.contains("pub use sanitize"),
        "agent-doc-template should not add a sanitization root facade"
    );

    let write_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write.rs")).unwrap();
    for forbidden_snippet in [
        "pub fn sanitize_component_tags",
        "pub fn sanitize_patches",
        "pub fn sanitize_unmatched",
        "fn utf8_char_len",
        "fn find_comment_close",
    ] {
        assert!(
            !write_source.contains(forbidden_snippet),
            "orchestration must not re-own or facade template sanitization policy: {forbidden_snippet}"
        );
    }

    let write_run_entry =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/run_entry.rs"))
            .unwrap();
    let write_materialize =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/write/materialize.rs"))
            .unwrap();
    let run_source =
        fs::read_to_string(manifest_dir.join("agent-doc-orchestration/src/run.rs")).unwrap();
    for (source, content) in [
        (
            "agent-doc-orchestration/src/write/run_entry.rs",
            write_run_entry.as_str(),
        ),
        (
            "agent-doc-orchestration/src/write/materialize.rs",
            write_materialize.as_str(),
        ),
        ("agent-doc-orchestration/src/run.rs", run_source.as_str()),
    ] {
        assert!(
            content.contains("agent_doc_template::sanitize")
                || content.contains("template::sanitize"),
            "template write adapters should call focused sanitization directly in {source}"
        );
    }
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
        "--force-disk",
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
    cmd.arg("--force-disk");
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
    cmd.arg("--force-disk");
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
    cmd.arg("--force-disk");
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

fn assert_operator_authority_instructions(content: &str, surface: &str) {
    assert!(
        content.contains("Preserve user edits; let `agent-doc write --stream` merge"),
        "{surface} should tell agents to preserve user edits through the merge path"
    );
    assert!(
        content.contains("Operator-visible document text is authoritative"),
        "{surface} should state the operator-visible document is authoritative"
    );
    assert!(
        content.contains("never recover, patch, or hook-closeout by replacing it with `content_ours`, a snapshot, or ACK-content"),
        "{surface} should forbid content_ours/snapshot/ACK-content replacement that drops operator text"
    );
    assert!(
        content.contains("Snapshots are backup/audit state, not hot-path authority"),
        "{surface} should keep snapshots out of hot-path document authority"
    );
    assert!(
        content.contains("fail closed or retry through the editor instead"),
        "{surface} should require fail-closed/editor retry instead of dropping operator text"
    );
    assert!(
        !content.contains("content_ours (baseline + response, no user edits) is saved as the snapshot on IPC success"),
        "{surface} must not describe content_ours as the IPC-success snapshot authority"
    );
}

#[test]
fn root_agents_instructions_preserve_operator_authority() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let content = fs::read_to_string(root.join("AGENTS.md")).unwrap();

    assert_operator_authority_instructions(&content, "root AGENTS.md");
}

#[test]
fn docs_do_not_reintroduce_content_ours_snapshot_authority() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let docs = [
        "AGENTS.md",
        "README.md",
        "SPEC.md",
        "specs/06-config.md",
        "specs/07-closeout-commands.md",
        "specs/14-realtime-workflow.md",
        "specs/pending-system.md",
        ".claude/skills/agent-doc/SKILL.md",
    ];
    let banned = [
        "content_ours (baseline + response, no user edits) is saved as the snapshot on IPC success",
        "snapshot == baseline + response",
        "saves the `content_ours` snapshot (baseline + response)",
    ];

    for doc in docs {
        let content = fs::read_to_string(root.join(doc)).unwrap();
        for phrase in banned {
            assert!(
                !content.contains(phrase),
                "{doc} must not reintroduce content_ours snapshot authority phrase: {phrase}"
            );
        }
    }
}

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
    assert_operator_authority_instructions(&content, "Claude SKILL.md");

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
        "SKILL.md should require creating plan files before adding plan-backed backlog items"
    );
    assert!(
        content.contains("include that exact plan file path"),
        "SKILL.md should require plan-backed backlog items to include the plan path"
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
    assert!(
        tmp.path()
            .join(".claude/skills/agent-doc/okf/index.md")
            .exists(),
        "skill install should write the OKF concept index"
    );
    let okf_index =
        std::fs::read_to_string(tmp.path().join(".claude/skills/agent-doc/okf/index.md")).unwrap();
    assert!(
        okf_index.contains("Session Cycle"),
        "installed OKF index should include session-cycle navigation"
    );
    let pending_ops = std::fs::read_to_string(
        tmp.path()
            .join(".claude/skills/agent-doc/runbooks/pending-ops.md"),
    )
    .unwrap();
    assert!(
        pending_ops.contains("plan-spec2-rollout.md"),
        "installed pending-ops runbook should document plan-backed backlog items"
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
    let skill_path = tmp.path().join(".codex/skills/agent-doc/SKILL.md");
    assert!(hooks_path.exists(), "missing {}", hooks_path.display());
    assert!(config_path.exists(), "missing {}", config_path.display());
    assert!(skill_path.exists(), "missing {}", skill_path.display());
    assert!(
        !tmp.path().join(".codex/AGENTS.md").exists(),
        "Codex workflow must not be installed into always-on .codex/AGENTS.md"
    );

    let skill = std::fs::read_to_string(&skill_path).unwrap();
    assert_operator_authority_instructions(&skill, "Codex SKILL.md");
    assert!(skill.contains("Interactive markdown session for Codex"));
    assert!(skill.contains("agent-doc skill install --harness codex --reload restart"));

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
fn test_opencode_skill_install_preserves_operator_authority_instructions() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut cmd = agent_doc_cmd();
    cmd.current_dir(tmp.path());
    cmd.env("OPENCODE", "1");
    cmd.env_remove("CLAUDE_CODE");
    cmd.env_remove("CLAUDE_CODE_ENTRYPOINT");
    cmd.env_remove("CODEX_CLI");
    cmd.args(["skill", "install"]);
    cmd.assert().success();

    let skill_path = tmp.path().join(".opencode/skills/agent-doc/SKILL.md");
    let content = std::fs::read_to_string(&skill_path).unwrap();
    assert_operator_authority_instructions(&content, "OpenCode SKILL.md");
    assert!(content.contains("Interactive markdown session for OpenCode"));
    assert!(content.contains("agent-doc skill install --harness opencode"));
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
    cmd.args([
        "compact",
        "session.md",
        "--message",
        "-",
        "--tag",
        "skip",
        "--force-disk",
    ]);
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
