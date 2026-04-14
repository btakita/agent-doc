//! Integration tests for the pending system (stable hash ids + granular ops).
//!
//! Covers:
//! - `agent-doc pending <file> add/backfill/done/edit/reorder/clear/reap`
//! - `agent-doc write --pending-add/done/...`
//! - `patch:pending` block rejection (and `--allow-patch-pending` escape hatch)

use assert_cmd::cargo::cargo_bin_cmd;
use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn agent_doc() -> Command {
    cargo_bin_cmd!("agent-doc")
}

fn setup_doc(body: &str) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    // Project marker + snapshots dir so canonicalize finds a root.
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = format!(
        "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n<!-- agent:pending -->\n{}\n<!-- /agent:pending -->\n",
        body
    );
    fs::write(&doc, content).unwrap();
    (tmp, doc)
}

#[test]
fn pending_backfill_assigns_hashes_to_legacy_items() {
    let (_tmp, doc) = setup_doc("- legacy one\n- legacy two");
    agent_doc()
        .args(["pending", doc.to_str().unwrap(), "backfill"])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(content.matches("[#").count(), 2);
    assert!(content.contains("- [ ] [#"));
    assert!(content.contains("legacy one"));
    assert!(content.contains("legacy two"));
}

#[test]
fn pending_add_creates_item_with_hash() {
    let (_tmp, doc) = setup_doc("");
    agent_doc()
        .args(["pending", doc.to_str().unwrap(), "add", "first task"])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("first task"));
    assert!(content.contains("- [ ] [#"));
}

#[test]
fn pending_done_marks_checked_by_id() {
    let (_tmp, doc) = setup_doc("- [ ] [#abcd] task one");
    agent_doc()
        .args(["pending", doc.to_str().unwrap(), "done", "abcd"])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [x] [#abcd] task one"));
}

#[test]
fn pending_edit_preserves_hash() {
    let (_tmp, doc) = setup_doc("- [ ] [#abcd] original text");
    agent_doc()
        .args(["pending", doc.to_str().unwrap(), "edit", "abcd", "updated text"])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("[#abcd]"));
    assert!(content.contains("updated text"));
    assert!(!content.contains("original text"));
}

#[test]
fn pending_reorder_by_id() {
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] first\n- [ ] [#bbbb] second\n- [ ] [#cccc] third");
    agent_doc()
        .args([
            "pending",
            doc.to_str().unwrap(),
            "reorder",
            "cccc,aaaa",
        ])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    let cccc = content.find("cccc").unwrap();
    let aaaa = content.find("aaaa").unwrap();
    let bbbb = content.find("bbbb").unwrap();
    assert!(cccc < aaaa && aaaa < bbbb);
}

#[test]
fn pending_reap_removes_checked_items() {
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] keep\n- [x] [#bbbb] drop\n- [ ] [#cccc] keep2");
    agent_doc()
        .args(["pending", doc.to_str().unwrap(), "reap"])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("aaaa"));
    assert!(!content.contains("bbbb"));
    assert!(content.contains("cccc"));
}

#[test]
fn pending_clear_empties_list() {
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] one\n- [ ] [#bbbb] two");
    agent_doc()
        .args(["pending", doc.to_str().unwrap(), "clear"])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(!content.contains("[#"));
}

#[test]
fn write_pending_add_appends_with_hash() {
    let (_tmp, doc) = setup_doc("");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add",
            "new task",
        ])
        .write_stdin("<!-- patch:exchange -->\nresponse text\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("new task"));
    assert!(content.contains("- [ ] [#"));
}

#[test]
fn write_rejects_patch_pending_block() {
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] existing");
    let payload =
        "<!-- patch:pending -->\n- [ ] [#zzzz] new\n<!-- /patch:pending -->\n";
    let assert_result = agent_doc()
        .args(["write", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin(payload)
        .assert()
        .failure();
    let output = assert_result.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("patch:pending block forbidden"),
        "stderr was: {}",
        stderr
    );
}

#[test]
fn write_allows_patch_pending_with_escape_hatch() {
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] existing");
    let payload =
        "<!-- patch:pending -->\n- [ ] [#zzzz] replaced\n<!-- /patch:pending -->\n";
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--allow-patch-pending",
        ])
        .write_stdin(payload)
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("zzzz"));
}

#[test]
fn write_pending_done_marks_checked() {
    let (_tmp, doc) = setup_doc("- [ ] [#abcd] task");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-done",
            "abcd",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [x] [#abcd]"));
}

// ---- Phase 2: gate / ungate ----

#[test]
fn write_pending_gate_open_to_gated() {
    let (_tmp, doc) = setup_doc("- [ ] [#abcd] task");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-gate",
            "abcd",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [/] [#abcd]"), "got: {}", content);
}

#[test]
fn write_pending_gate_idempotent_on_gated() {
    let (_tmp, doc) = setup_doc("- [/] [#abcd] task");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-gate",
            "abcd",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [/] [#abcd]"));
}

#[test]
fn write_pending_gate_done_errors() {
    let (_tmp, doc) = setup_doc("- [x] [#abcd] task");
    let assert_result = agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-gate",
            "abcd",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
    assert!(stderr.contains("cannot gate Done item"), "stderr: {}", stderr);
}

#[test]
fn write_pending_ungate_gated_to_open() {
    let (_tmp, doc) = setup_doc("- [/] [#abcd] task");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-ungate",
            "abcd",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [ ] [#abcd]"));
}

#[test]
fn write_pending_ungate_open_errors() {
    let (_tmp, doc) = setup_doc("- [ ] [#abcd] task");
    let assert_result = agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-ungate",
            "abcd",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
    assert!(stderr.contains("cannot ungate Open"), "stderr: {}", stderr);
}

#[test]
fn write_pending_gate_then_done_in_one_call() {
    // gate runs before done in the apply order, so this should land on `[x]`
    // (Gated → Done is a valid transition).
    let (_tmp, doc) = setup_doc("- [ ] [#abcd] task");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-gate",
            "abcd",
            "--pending-done",
            "abcd",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [x] [#abcd]"), "got: {}", content);
}

#[test]
fn preflight_emits_pending_reordered_flag() {
    // Create a doc with a fully-migrated pending component.
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] one\n- [ ] [#bbbb] two");

    // First preflight: initialize snapshot.
    let _ = agent_doc()
        .args(["preflight", doc.to_str().unwrap()])
        .output()
        .unwrap();

    // Swap the order in the document.
    let content = fs::read_to_string(&doc).unwrap();
    let reordered = content.replace(
        "- [ ] [#aaaa] one\n- [ ] [#bbbb] two",
        "- [ ] [#bbbb] two\n- [ ] [#aaaa] one",
    );
    fs::write(&doc, &reordered).unwrap();

    // Second preflight: should detect reorder.
    let output = agent_doc()
        .args(["preflight", doc.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("preflight output should be JSON");
    assert_eq!(
        parsed.get("pending_reordered").and_then(|v| v.as_bool()),
        Some(true),
        "expected pending_reordered: true, full output: {}",
        stdout
    );
}
