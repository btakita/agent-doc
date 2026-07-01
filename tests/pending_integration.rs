//! Integration tests for the pending system (stable hash ids + granular ops).
//!
//! Covers:
//! - `agent-doc backlog <file> add/backfill/done/edit/reorder/clear/reap`
//! - `agent-doc icebox <file> add/backfill/done/edit/reorder/clear/reap`
//! - `agent-doc write --backlog-add/--icebox-edit/--done/...`
//! - `replace:pending` block rejection (and `--allow-replace-pending` escape hatch)

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
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

fn setup_doc_with_icebox(backlog: &str, icebox: &str) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = format!(
        "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n{}\n<!-- /agent:backlog -->\n\n<!-- agent:icebox -->\n{}\n<!-- /agent:icebox -->\n",
        backlog, icebox
    );
    fs::write(&doc, content).unwrap();
    (tmp, doc)
}

fn component_body<'a>(content: &'a str, name: &str) -> &'a str {
    let open = format!("<!-- agent:{} -->", name);
    let close = format!("<!-- /agent:{} -->", name);
    let Some(open_start) = content.find(&open) else {
        return "";
    };
    let mut body_start = open_start + open.len();
    if content[body_start..].starts_with('\n') {
        body_start += 1;
    }
    let Some(close_rel) = content[body_start..].find(&close) else {
        return "";
    };
    content[body_start..body_start + close_rel].trim_end_matches('\n')
}

#[test]
fn pending_backfill_assigns_hashes_to_legacy_items() {
    let (_tmp, doc) = setup_doc("- legacy one\n- legacy two");
    agent_doc()
        .args(["backlog", doc.to_str().unwrap(), "--force-disk", "backfill"])
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
        .args([
            "backlog",
            doc.to_str().unwrap(),
            "--force-disk",
            "add",
            "first task",
        ])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("first task"));
    assert!(content.contains("- [ ] [#"));
}

#[test]
fn pending_alias_still_works_with_deprecation_warning() {
    let (_tmp, doc) = setup_doc("");
    let assert_result = agent_doc()
        .args([
            "pending",
            doc.to_str().unwrap(),
            "--force-disk",
            "add",
            "first task",
        ])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
    assert!(content.contains("first task"));
    assert!(
        stderr.contains("deprecated"),
        "expected deprecation warning in stderr, got: {}",
        stderr
    );
}

#[test]
fn tracked_work_prune_legacy_alias_is_rejected() {
    let (_tmp, doc) = setup_doc_with_icebox(
        "- [x] [#done1] completed backlog task\n",
        "- [x] [#done2] completed icebox task\n",
    );

    for component in ["backlog", "icebox"] {
        let assert_result = agent_doc()
            .args([component, doc.to_str().unwrap(), "--force-disk", "prune"])
            .assert()
            .failure();
        let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
        assert!(
            stderr.contains("unexpected argument") || stderr.contains("unrecognized subcommand"),
            "expected prune to be rejected for {component}, got: {stderr}"
        );
        assert!(
            stderr.contains("prune"),
            "expected stderr to name prune for {component}, got: {stderr}"
        );
    }
}

#[test]
fn pending_add_accepts_custom_id_prefix() {
    let (_tmp, doc) = setup_doc("");
    agent_doc()
        .args([
            "backlog",
            doc.to_str().unwrap(),
            "--force-disk",
            "add",
            "id=spec1 first task",
        ])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [ ] [#spec1] first task"));
}

#[test]
fn pending_add_accepts_bracketed_custom_id_prefix() {
    let (_tmp, doc) = setup_doc("");
    agent_doc()
        .args([
            "backlog",
            doc.to_str().unwrap(),
            "--force-disk",
            "add",
            "[#spec1] first task",
        ])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [ ] [#spec1] first task"));
}

#[test]
fn pending_add_accepts_long_bracketed_custom_id_prefix() {
    let (_tmp, doc) = setup_doc("");
    agent_doc()
        .args([
            "backlog",
            doc.to_str().unwrap(),
            "--force-disk",
            "add",
            "[#sdig2matrix] first task",
        ])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [ ] [#sdig2matrix] first task"));
}

#[test]
fn pending_add_accepts_hyphenated_custom_id_prefix() {
    let (_tmp, doc) = setup_doc("");
    agent_doc()
        .args([
            "backlog",
            doc.to_str().unwrap(),
            "--force-disk",
            "add",
            "id=tmuxcrash-abcd first task",
        ])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [ ] [#tmuxcrash-abcd] first task"));
}

#[test]
fn pending_backfill_assigns_parent_prefixed_nested_subtask_ids() {
    let (_tmp, doc) = setup_doc("- parent task\n  - child dependency\n  - child subtask");
    agent_doc()
        .args(["backlog", doc.to_str().unwrap(), "--force-disk", "backfill"])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    let pending = content
        .split("<!-- agent:pending -->\n")
        .nth(1)
        .and_then(|rest| rest.split("\n<!-- /agent:pending -->").next())
        .unwrap();
    let lines: Vec<&str> = pending.lines().collect();
    let parent_id = lines[0]
        .split("[#")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("parent id");
    assert!(lines[1].starts_with("  - [ ] [#"), "got: {}", lines[1]);
    assert!(lines[2].starts_with("  - [ ] [#"), "got: {}", lines[2]);
    assert!(
        lines[1].contains(&format!("[#{}-", parent_id)),
        "expected nested id prefixed by parent id, got: {}",
        lines[1]
    );
    assert!(
        lines[2].contains(&format!("[#{}-", parent_id)),
        "expected nested id prefixed by parent id, got: {}",
        lines[2]
    );
}

#[test]
fn pending_done_marks_checked_by_id() {
    let (_tmp, doc) = setup_doc("- [ ] [#abcd] task one");
    agent_doc()
        .args([
            "backlog",
            doc.to_str().unwrap(),
            "--force-disk",
            "done",
            "abcd",
        ])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [x] [#abcd] task one"));
}

#[test]
fn pending_edit_preserves_hash() {
    let (_tmp, doc) = setup_doc("- [ ] [#abcd] original text");
    agent_doc()
        .args([
            "backlog",
            doc.to_str().unwrap(),
            "--force-disk",
            "edit",
            "abcd",
            "updated text",
        ])
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
            "backlog",
            doc.to_str().unwrap(),
            "--force-disk",
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
        .args(["backlog", doc.to_str().unwrap(), "--force-disk", "reap"])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    let backlog = component_body(&content, "pending");
    assert!(backlog.contains("aaaa"));
    assert!(!backlog.contains("bbbb"));
    assert!(backlog.contains("cccc"));
    assert!(content.contains("## Completed / Reaped"));
    assert!(content.contains("<!-- agent:done -->"));
    assert!(content.contains("[#bbbb] drop"));
}

#[test]
fn pending_reap_removes_malformed_flush_left_spill_with_done_parent() {
    let (_tmp, doc) = setup_doc(concat!(
        "- [x] [#bbbb] drop\n",
        "Commands:\n",
        "  cargo test -p agent-doc pending::\n",
        "Diff:\n",
        "@@ -1 +1 @@\n",
        "- [ ] [#cccc] keep2\n"
    ));
    agent_doc()
        .args(["backlog", doc.to_str().unwrap(), "--force-disk", "reap"])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    let backlog = component_body(&content, "pending");
    assert!(!backlog.contains("[#bbbb]"));
    assert!(!backlog.contains("Commands:"));
    assert!(!backlog.contains("@@ -1 +1 @@"));
    assert!(backlog.contains("- [ ] [#cccc] keep2"));
    assert!(content.contains("[#bbbb] drop"));
    assert!(content.contains("Commands:"));
    assert!(content.contains("@@ -1 +1 @@"));
}

#[test]
fn pending_reap_backfills_legacy_done_ids_before_removing_items() {
    let (_tmp, doc) = setup_doc("- [ ] keep\n- [x] legacy drop\n");
    agent_doc()
        .args(["backlog", doc.to_str().unwrap(), "--force-disk", "reap"])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("- [ ] [#"),
        "open legacy item should be backfilled: {content}"
    );
    assert!(content.contains("keep"));
    let backlog = component_body(&content, "pending");
    assert!(!backlog.contains("legacy drop"));
    assert!(content.contains("<!-- agent:done -->"));
    assert!(content.contains("legacy drop"));
}

#[test]
fn pending_clear_empties_list() {
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] one\n- [ ] [#bbbb] two");
    agent_doc()
        .args(["backlog", doc.to_str().unwrap(), "--force-disk", "clear"])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(!content.contains("[#"));
}

#[test]
fn write_pending_add_creates_item_with_hash() {
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
fn write_pending_add_dedupes_symptom_key_into_existing_backlog_item() {
    let key = "[symptom-key invariant=stale_queue_pause document=doc-abc component=queue content_hash=sha256:feedface]";
    let (_tmp, doc) = setup_doc(&format!("- [ ] [#sym1] stale pause {key}\n"));
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add",
            &format!("stale pause observed again {key}"),
        ])
        .write_stdin("<!-- patch:exchange -->\nresponse text\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    let body = component_body(&content, "pending");
    assert_eq!(body.matches("[#sym1]").count(), 1, "{body}");
    assert!(
        body.contains("  evidence: stale pause observed again"),
        "{body}"
    );
}

#[test]
fn write_pending_add_after_and_back_position_items_explicitly() {
    // #ah0s: --pending-add-after lands directly below the anchor; --pending-add-back
    // lands at the tail. Both leave the existing head undisturbed.
    let (_tmp, doc) = setup_doc("- [ ] [#anc1] anchor task\n- [ ] [#tail9] tail task\n");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add-after",
            "anc1",
            "inserted after anchor",
            "--pending-add-back",
            "appended at tail",
        ])
        .write_stdin("<!-- patch:exchange -->\nresponse text\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    let body = component_body(&content, "pending");
    let lines: Vec<&str> = body.lines().filter(|l| l.contains("[#")).collect();
    // anchor stays first, the after-insert is directly below it, tail append is last.
    assert!(lines[0].contains("anchor task"), "{body}");
    assert!(lines[1].contains("inserted after anchor"), "{body}");
    assert!(
        lines.last().unwrap().contains("appended at tail"),
        "back insert must land at the tail: {body}"
    );
}

#[test]
fn write_icebox_edit_preserves_hash_and_backlog_untouched() {
    let (_tmp, doc) = setup_doc_with_icebox(
        "- [ ] [#back1] active backlog task\n",
        "- [ ] [#park1] parked task\n",
    );
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--icebox-edit",
            "park1=renamed parked task",
        ])
        .write_stdin("<!-- patch:exchange -->\nresponse text\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        component_body(&content, "backlog").contains("[#back1] active backlog task"),
        "{content}"
    );
    let icebox = component_body(&content, "icebox");
    assert!(
        icebox.contains("- [ ] [#park1] renamed parked task"),
        "{icebox}"
    );
    assert!(!icebox.contains("parked task\n"), "{icebox}");
}

#[test]
fn write_icebox_reorder_and_clear_target_icebox_only() {
    let (_tmp, doc) = setup_doc_with_icebox(
        "- [ ] [#back1] active backlog task\n",
        "- [ ] [#park1] parked one\n- [ ] [#park2] parked two\n",
    );
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--icebox-reorder",
            "park2,park1",
        ])
        .write_stdin("<!-- patch:exchange -->\nresponse text\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    let icebox = component_body(&content, "icebox");
    assert!(
        icebox.find("[#park2]").unwrap() < icebox.find("[#park1]").unwrap(),
        "{icebox}"
    );
    assert!(
        component_body(&content, "backlog").contains("[#back1] active backlog task"),
        "{content}"
    );

    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--icebox-clear",
        ])
        .write_stdin("<!-- patch:exchange -->\nsecond response\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(component_body(&content, "icebox"), "");
    assert!(
        component_body(&content, "backlog").contains("[#back1] active backlog task"),
        "{content}"
    );
}

#[test]
fn icebox_subcommand_backfill_edit_reorder_reap_targets_icebox() {
    let (_tmp, doc) = setup_doc_with_icebox(
        "- [ ] [#back1] active backlog task\n",
        "- parked legacy\n- [x] [#done1] completed parked task\n",
    );
    agent_doc()
        .args(["icebox", doc.to_str().unwrap(), "--force-disk", "backfill"])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    let generated_id = component_body(&content, "icebox")
        .lines()
        .find_map(|line| {
            line.split("[#")
                .nth(1)
                .and_then(|rest| rest.split(']').next())
        })
        .expect("backfill should assign an id")
        .to_string();

    agent_doc()
        .args([
            "icebox",
            doc.to_str().unwrap(),
            "--force-disk",
            "edit",
            &generated_id,
            "renamed legacy parked task",
        ])
        .assert()
        .success();
    agent_doc()
        .args([
            "icebox",
            doc.to_str().unwrap(),
            "--force-disk",
            "reorder",
            &format!("done1,{generated_id}"),
        ])
        .assert()
        .success();
    agent_doc()
        .args(["icebox", doc.to_str().unwrap(), "--force-disk", "reap"])
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        component_body(&content, "backlog").contains("[#back1] active backlog task"),
        "{content}"
    );
    let icebox = component_body(&content, "icebox");
    assert!(icebox.contains("renamed legacy parked task"), "{icebox}");
    assert!(!icebox.contains("[#done1]"), "{icebox}");
    assert!(
        component_body(&content, "done").contains("[#done1] completed parked task"),
        "{content}"
    );
}

#[test]
fn write_pending_add_to_updates_target_not_current_doc() {
    let (tmp, doc) = setup_doc("");
    let target = tmp.path().join("target.md");
    fs::write(
        &target,
        "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n- [ ] [#old1] existing target task\n<!-- /agent:backlog -->\n",
    )
    .unwrap();

    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add-to",
            target.to_str().unwrap(),
            "id=xdoc cross document task",
        ])
        .write_stdin("<!-- patch:exchange -->\nresponse text\n<!-- /patch:exchange -->\n")
        .assert()
        .success();

    let current = fs::read_to_string(&doc).unwrap();
    let target_content = fs::read_to_string(&target).unwrap();
    assert!(!current.contains("cross document task"));
    assert!(target_content.contains("- [ ] [#xdoc] cross document task"));
    let added = target_content.find("[#xdoc] cross document task").unwrap();
    let existing = target_content.find("[#old1] existing target task").unwrap();
    assert!(added < existing, "new target item should be prepended");
}

#[test]
fn write_pending_add_to_missing_target_fails_closed() {
    let (tmp, doc) = setup_doc("");
    let missing = tmp.path().join("missing.md");

    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add-to",
            missing.to_str().unwrap(),
            "id=xdoc cross document task",
        ])
        .write_stdin("<!-- patch:exchange -->\nresponse text\n<!-- /patch:exchange -->\n")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "--pending-add-to target file not found",
        ));
}

#[test]
fn write_pending_add_to_target_without_backlog_fails_closed() {
    let (tmp, doc) = setup_doc("");
    let target = tmp.path().join("target.md");
    fs::write(
        &target,
        "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n",
    )
    .unwrap();

    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add-to",
            target.to_str().unwrap(),
            "id=xdoc cross document task",
        ])
        .write_stdin("<!-- patch:exchange -->\nresponse text\n<!-- /patch:exchange -->\n")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "has no agent:backlog/agent:pending component",
        ));
}

#[test]
fn write_pending_add_multiple_flags_keep_cli_order_at_top() {
    let (_tmp, doc) = setup_doc("- [ ] [#old1] existing task");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add",
            "id=first first task",
            "--pending-add",
            "id=second second task",
            "--pending-add",
            "id=third third task",
        ])
        .write_stdin("<!-- patch:exchange -->\nresponse text\n<!-- /patch:exchange -->\n")
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    let pending = content
        .split("<!-- agent:pending -->\n")
        .nth(1)
        .and_then(|rest| rest.split("\n<!-- /agent:pending -->").next())
        .unwrap();
    let first = pending.find("[#first] first task").unwrap();
    let second = pending.find("[#second] second task").unwrap();
    let third = pending.find("[#third] third task").unwrap();
    let existing = pending.find("[#old1] existing task").unwrap();

    assert!(
        first < second && second < third && third < existing,
        "expected pending-add flags to keep CLI order above existing backlog, got:\n{}",
        pending
    );
}

#[test]
fn write_pending_add_accepts_custom_id_prefix() {
    let (_tmp, doc) = setup_doc("");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add",
            "id=fix42 new task",
        ])
        .write_stdin("<!-- patch:exchange -->\nresponse text\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [ ] [#fix42] new task"));
}

#[test]
fn write_pending_add_accepts_bracketed_custom_id_prefix() {
    let (_tmp, doc) = setup_doc("");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add",
            "[#fix42] new task",
        ])
        .write_stdin("<!-- patch:exchange -->\nresponse text\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [ ] [#fix42] new task"));
}

#[test]
fn write_pending_add_accepts_long_bracketed_custom_id_prefix() {
    let (_tmp, doc) = setup_doc("");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add",
            "[#sdig2matrix] new task",
        ])
        .write_stdin("<!-- patch:exchange -->\nresponse text\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [ ] [#sdig2matrix] new task"));
}

#[test]
fn write_normalizes_replace_pending_block_into_pending_ops() {
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] existing");
    let payload = concat!(
        "<!-- patch:exchange -->\n",
        "### Re: topic — gpt-5\n\n",
        "Done.\n",
        "<!-- /patch:exchange -->\n",
        "<!-- replace:pending -->\n",
        "- [x] [#aaaa] existing\n",
        "- [ ] [#bbbb] add regression coverage\n",
        "<!-- /replace:pending -->\n",
    );
    agent_doc()
        .args(["write", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin(payload)
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: topic — gpt-5"));
    assert!(content.contains("- [x] [#aaaa] existing"));
    assert!(content.contains("- [ ] [#bbbb] add regression coverage"));
    assert!(!content.contains("replace:pending"));
}

#[test]
fn write_normalizes_replace_pending_block_preserves_long_custom_id() {
    let (_tmp, doc) = setup_doc("");
    let payload = concat!(
        "<!-- patch:exchange -->\n",
        "### Re: topic — gpt-5\n\n",
        "Done.\n",
        "<!-- /patch:exchange -->\n",
        "<!-- replace:pending -->\n",
        "- [ ] [#sdig2matrix] Fixture evidence matrix\n",
        "<!-- /replace:pending -->\n",
    );
    agent_doc()
        .args(["write", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin(payload)
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [ ] [#sdig2matrix] Fixture evidence matrix"));
    assert_eq!(
        content.matches("[#").count(),
        1,
        "unexpected duplicate id in: {}",
        content
    );
    assert_eq!(content.matches("[#sdig2matrix]").count(), 1);
}

#[test]
fn write_rejects_replace_pending_block() {
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] existing");
    let payload = "<!-- replace:pending -->\n- [ ] [#zzzz] new\n<!-- /replace:pending -->\n";
    let assert_result = agent_doc()
        .args(["write", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin(payload)
        .assert()
        .failure();
    let output = assert_result.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no patch blocks or content found in response"),
        "stderr was: {}",
        stderr
    );
}

#[test]
fn write_rejects_legacy_patch_pending_block() {
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] existing");
    let payload = "<!-- patch:pending -->\n- [ ] [#zzzz] new\n<!-- /patch:pending -->\n";
    let assert_result = agent_doc()
        .args(["write", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin(payload)
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
    assert!(
        stderr.contains("legacy pending patch block is no longer supported"),
        "stderr was: {}",
        stderr
    );
    assert!(
        !stderr.contains("deprecated"),
        "legacy syntax should be rejected without migration warning, got: {}",
        stderr
    );
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [ ] [#aaaa] existing"));
    assert!(!content.contains("zzzz"));
}

#[test]
fn write_allows_replace_pending_with_escape_hatch() {
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] existing");
    let payload = "<!-- replace:pending -->\n- [ ] [#zzzz] replaced\n<!-- /replace:pending -->\n";
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--allow-replace-pending",
        ])
        .write_stdin(payload)
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("zzzz"));
}

#[test]
fn write_applies_replace_icebox_block_without_exchange_fallback() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_format: template\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:pending -->\n",
        "- [ ] [#aaaa] existing\n",
        "<!-- /agent:pending -->\n\n",
        "<!-- agent:icebox -->\n",
        "<!-- /agent:icebox -->\n",
    );
    fs::write(&doc, content).unwrap();

    let payload = concat!(
        "<!-- patch:exchange -->\n",
        "### Re: #iceboxpatch — gpt-5\n\n",
        "Applied the icebox rewrite through the binary-owned template path.\n",
        "<!-- /patch:exchange -->\n",
        "<!-- replace:icebox -->\n",
        "- [ ] [#park1] Parked follow-up\n",
        "<!-- /replace:icebox -->\n",
    );
    let assert_result = agent_doc()
        .args(["write", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin(payload)
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
    assert!(
        !stderr.contains("0 template patches found"),
        "stderr unexpectedly reported zero patches: {}",
        stderr
    );

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: #iceboxpatch — gpt-5"));
    assert!(
        content.contains(
            "<!-- agent:icebox -->\n- [ ] [#park1] Parked follow-up\n<!-- /agent:icebox -->"
        ),
        "icebox content should be rewritten in place: {}",
        content
    );
    assert!(
        !content.contains(
            "Applied the icebox rewrite through the binary-owned template path.\n- [ ] [#park1] Parked follow-up"
        ),
        "icebox payload should not be synthesized into exchange: {}",
        content
    );
    assert!(!content.contains("replace:icebox"));
}

#[test]
fn write_rejects_removed_allow_patch_pending_flag() {
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] existing");
    let payload = "<!-- patch:pending -->\n- [ ] [#zzzz] replaced\n<!-- /patch:pending -->\n";
    let assert_result = agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--allow-patch-pending",
        ])
        .write_stdin(payload)
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
    assert!(
        stderr.contains("unexpected argument") && stderr.contains("--allow-patch-pending"),
        "expected removed legacy flag rejection, got: {}",
        stderr
    );
}

#[test]
fn write_rejects_replace_pending_with_removed_legacy_env_var() {
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] existing");
    let payload = "<!-- replace:pending -->\nnot a backlog list\n<!-- /replace:pending -->\n";
    let assert_result = agent_doc()
        .args(["write", doc.to_str().unwrap(), "--force-disk"])
        .env("AGENT_DOC_ALLOW_PATCH_PENDING", "1")
        .write_stdin(payload)
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
    assert!(
        stderr.contains("pending/backlog patch"),
        "legacy env var must not authorize replacement, got: {}",
        stderr
    );
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [ ] [#aaaa] existing"));
    assert!(!content.contains("not a backlog list"));
}

#[test]
fn write_pending_done_marks_checked() {
    let (_tmp, doc) = setup_doc("- [ ] [#abcd] task");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "abcd",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [x] [#abcd]"));
}

#[test]
fn write_pending_done_accepts_hash_prefixed_id() {
    let (_tmp, doc) = setup_doc("- [ ] [#abcd] task");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "#abcd",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [x] [#abcd]"));
}

#[test]
fn write_done_rejects_removed_done_aliases() {
    for removed_alias in ["--pending-done", "--backlog-done"] {
        let (_tmp, doc) = setup_doc("- [ ] [#abcd] task");
        let assert_result = agent_doc()
            .args([
                "write",
                doc.to_str().unwrap(),
                "--force-disk",
                removed_alias,
                "abcd",
            ])
            .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
            .assert()
            .failure();
        let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
        assert!(
            stderr.contains(removed_alias),
            "expected stderr to name {removed_alias}, got: {stderr}"
        );
        assert!(
            stderr.contains("unexpected argument"),
            "expected {removed_alias} to be rejected by clap, got: {stderr}"
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert!(
            content.contains("- [ ] [#abcd] task"),
            "{removed_alias} must not mutate tracked work:\n{content}"
        );
    }
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
    let backlog = component_body(&content, "pending");
    let review = component_body(&content, "review");
    assert!(!backlog.contains("[#abcd] task"), "got: {}", content);
    assert!(review.contains("- [/] [#abcd] task"), "got: {}", content);
}

#[test]
fn write_pending_gate_idempotent_on_gated() {
    let (_tmp, doc) = setup_doc("");
    let content = fs::read_to_string(&doc).unwrap().replace(
        "<!-- /agent:pending -->\n",
        "<!-- /agent:pending -->\n\n## Review\n\n<!-- agent:review -->\n- [/] [#abcd] task\n<!-- /agent:review -->\n",
    );
    fs::write(&doc, content).unwrap();
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
    assert_eq!(content.matches("[#abcd] task").count(), 1);
    assert!(component_body(&content, "review").contains("- [/] [#abcd] task"));
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
    assert!(
        stderr.contains("cannot gate Done item"),
        "stderr: {}",
        stderr
    );
}

#[test]
fn write_pending_ungate_gated_to_open() {
    let (_tmp, doc) = setup_doc("");
    let content = fs::read_to_string(&doc).unwrap().replace(
        "<!-- /agent:pending -->\n",
        "<!-- /agent:pending -->\n\n## Review\n\n<!-- agent:review -->\n- [/] [#abcd] task\n<!-- /agent:review -->\n",
    );
    fs::write(&doc, content).unwrap();
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
    assert!(component_body(&content, "pending").contains("- [ ] [#abcd] task"));
    assert!(!component_body(&content, "review").contains("[#abcd] task"));
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
            "--done",
            "abcd",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [x] [#abcd]"), "got: {}", content);
}

#[test]
fn write_review_add_and_edit_mutate_review_component() {
    let (_tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--review-add",
            "id=rvw1 needs review",
            "--review-edit",
            "rvw1=review text updated",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    let review = component_body(&content, "review");
    assert!(review.contains("- [/] [#rvw1] review text updated"));
}

#[test]
fn write_review_add_dedupes_symptom_key_into_existing_review_item() {
    let key = "[symptom-key invariant=component_drift document=doc-abc component=exchange content_hash=sha256:cafebabe]";
    let (_tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--review-add",
            &format!("id=rvw1 first drift symptom {key}"),
            "--review-add",
            &format!("second drift symptom {key}"),
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    let review = component_body(&content, "review");
    assert_eq!(review.matches("[#rvw1]").count(), 1, "{review}");
    assert!(
        review.contains("  evidence: second drift symptom"),
        "{review}"
    );
}

#[test]
fn write_review_done_guard_strict_blocks_backlog_done() {
    let (_tmp, doc) = setup_doc("- [ ] [#abcd] task");
    let content = fs::read_to_string(&doc).unwrap().replace(
        "agent_doc_format: template\n",
        "agent_doc_format: template\nreview_done_guard: error\n",
    );
    fs::write(&doc, content).unwrap();
    let assert_result = agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "abcd",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
    assert!(stderr.contains("review_done_guard"), "stderr: {}", stderr);
    assert!(stderr.contains("agent:pending"), "stderr: {}", stderr);
}

#[test]
fn write_review_done_guard_strict_allows_gate_then_done() {
    let (_tmp, doc) = setup_doc("- [ ] [#abcd] task");
    let content = fs::read_to_string(&doc).unwrap().replace(
        "agent_doc_format: template\n",
        "agent_doc_format: template\nreview_done_guard: error\n",
    );
    fs::write(&doc, content).unwrap();
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-gate",
            "abcd",
            "--done",
            "abcd",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(component_body(&content, "review").contains("- [x] [#abcd] task"));
}

#[test]
fn preflight_emits_backlog_reordered_flag() {
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
        parsed.get("backlog_reordered").and_then(|v| v.as_bool()),
        Some(true),
        "expected backlog_reordered: true, full output: {}",
        stdout
    );
    assert!(
        parsed.get("pending_reordered").is_none(),
        "pending_reordered alias should not be emitted, full output: {}",
        stdout
    );
}

#[test]
fn preflight_emits_backlog_gated_count() {
    // Doc with one open + two gated + one done item. Reap drops [x],
    // leaving one open + two gated → expected count = 2.
    let (_tmp, doc) = setup_doc(
        "- [ ] [#aaaa] open\n- [/] [#bbbb] gated one\n- [/] [#cccc] gated two\n- [x] [#dddd] reaped",
    );

    let output = agent_doc()
        .args(["preflight", doc.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("preflight output should be JSON");
    assert_eq!(
        parsed.get("backlog_gated_count").and_then(|v| v.as_u64()),
        Some(2),
        "expected backlog_gated_count: 2, full output: {}",
        stdout
    );
    assert!(
        parsed.get("pending_gated_count").is_none(),
        "pending_gated_count alias should not be emitted, full output: {}",
        stdout
    );
}

#[test]
fn preflight_emits_review_counts() {
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] open");
    let content = fs::read_to_string(&doc).unwrap().replace(
        "<!-- /agent:pending -->\n",
        "<!-- /agent:pending -->\n\n## Review\n\n<!-- agent:review -->\n- [/] [#bbbb] review one\n- [/] [#cccc] review two\n<!-- /agent:review -->\n",
    );
    fs::write(&doc, content).unwrap();

    let output = agent_doc()
        .args(["preflight", doc.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("preflight output should be JSON");
    assert_eq!(parsed.get("review_count").and_then(|v| v.as_u64()), Some(2));
    assert_eq!(
        parsed.get("review_gated_count").and_then(|v| v.as_u64()),
        Some(2)
    );
}

#[test]
fn preflight_warns_for_legacy_gated_backlog_items() {
    let (_tmp, doc) = setup_doc("- [/] [#bbbb] legacy gated");
    let output = agent_doc()
        .args(["preflight", doc.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("preflight output should be JSON");
    let warnings = parsed
        .get("warnings")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        warnings
            .iter()
            .any(|warning| warning.get("code").and_then(|v| v.as_str())
                == Some("legacy_gated_in_backlog")),
        "warnings: {}",
        serde_json::Value::Array(warnings)
    );
}

#[test]
fn preflight_omits_backlog_gated_count_when_zero() {
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] open\n- [ ] [#bbbb] also open");
    let output = agent_doc()
        .args(["preflight", doc.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("preflight output should be JSON");
    // Zero is omitted via skip_serializing_if — field should be absent entirely.
    assert!(
        parsed.get("backlog_gated_count").is_none(),
        "expected backlog_gated_count to be omitted at zero, got: {}",
        stdout
    );
    assert!(
        parsed.get("pending_gated_count").is_none(),
        "pending_gated_count alias should not be emitted, got: {}",
        stdout
    );
}

#[test]
fn write_rejects_replace_pending_via_library_default() {
    // Phase 3 inversion: library-level callers must default to reject.
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] existing");
    let payload = "<!-- replace:pending -->\n- [ ] [#zzzz] new\n<!-- /replace:pending -->\n";
    let assert_result = agent_doc()
        .args(["write", doc.to_str().unwrap(), "--force-disk"])
        .env_remove("AGENT_DOC_ALLOW_REPLACE_PENDING")
        .write_stdin(payload)
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
    assert!(
        stderr.contains("no patch blocks or content found in response"),
        "stderr: {}",
        stderr
    );
}
