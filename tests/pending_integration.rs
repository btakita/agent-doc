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

/// `#integrationtesttargets`: this target drives the real `agent-doc` CLI, and
/// a controller launch by that CLI is production behavior, so every spawned
/// command opts in with this reason.
const CONTROLLER_LAUNCH_REASON: &str =
    "pending/queue CLI commands reach the project controller, which they launch as production does";

fn agent_doc() -> Command {
    let mut command = cargo_bin_cmd!("agent-doc");
    command.env(
        agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
        CONTROLLER_LAUNCH_REASON,
    );
    command
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
    let open = format!("<!-- agent:{}", name);
    let close = format!("<!-- /agent:{} -->", name);
    let Some(open_start) = content.find(&open) else {
        return "";
    };
    let Some(marker_end) = content[open_start..].find("-->") else {
        return "";
    };
    let mut body_start = open_start + marker_end + 3;
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

/// `#queuemirrororder` (2): the only recovery for a mis-ordered mirror.
///
/// `agent-doc queue sync` skips ids already present (`already_in_queue`), so it is
/// append-only; before this cascade the only way to move a queued id was `--done`
/// or `--backlog-gate`, both of which assert something false about its state.
#[test]
fn backlog_reorder_cascades_to_queue_mirror() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(
        &doc,
        concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#bbbb]\n",
            "- /goal keep me exactly here\n",
            "- do [#aaaa]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue -->\n",
            "- [ ] [#bbbb] second\n",
            "- [ ] [#aaaa] first\n",
            "<!-- /agent:backlog -->\n",
        ),
    )
    .unwrap();

    agent_doc()
        .args([
            "backlog",
            doc.to_str().unwrap(),
            "--force-disk",
            "reorder",
            "aaaa,bbbb",
        ])
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    let queue_start = content.find("<!-- agent:queue").unwrap();
    let queue_end = content.find("<!-- /agent:queue -->").unwrap();
    let queue = &content[queue_start..queue_end];
    let aaaa = queue.find("- do [#aaaa]").expect("aaaa still queued");
    let bbbb = queue.find("- do [#bbbb]").expect("bbbb still queued");
    assert!(
        aaaa < bbbb,
        "the queue mirror must follow the reordered backlog:\n{queue}"
    );
    assert_eq!(
        queue.lines().nth(2),
        Some("- /goal keep me exactly here"),
        "the operator free-text line keeps its exact slot (#qauthorder):\n{queue}"
    );
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
    assert!(content.contains("## Done"));
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
    let orphan_spill = concat!(
        "6local-splice-batch 3 splices 34346->34137 chars, then 00:40:03 exact inverse ",
        "back to 25509a1e (undo/redo or plugin-internal flip; no VFS reload/projection logged)."
    );
    let (_tmp, doc) = setup_doc(&format!(
        concat!(
            "- [ ] [#aaaa] one\n",
            "- [ ] [#bbbb] two\n",
            "{}\n",
            "```text\n",
            "captured command output\n",
            "```\n",
            "\n",
            "### Operator notes\n",
        ),
        orphan_spill
    ));
    agent_doc()
        .args(["backlog", doc.to_str().unwrap(), "--force-disk", "clear"])
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(!content.contains("[#"));
    assert!(!content.contains(orphan_spill));
    assert!(!content.contains("captured command output"));
    assert!(content.contains("### Operator notes"));
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

/// The own id `--todo-add` gave the todo item whose action is `action`.
fn todo_item_id(content: &str, action: &str) -> String {
    let body = component_body(content, "todo");
    let line = body
        .lines()
        .find(|line| {
            line.ends_with(action)
                || line
                    .strip_suffix(')')
                    .and_then(|line| line.rsplit_once(" (source: agent:"))
                    .is_some_and(|(before, _)| before.ends_with(action))
        })
        .unwrap_or_else(|| panic!("no todo item {action:?}:\n{content}"));
    let rest = line
        .split_once("] [#")
        .unwrap_or_else(|| panic!("todo item has no own id: {line:?}"))
        .1;
    rest.split_once(']').unwrap().0.to_string()
}

/// `#operatortodo`: `--todo-add` creates `agent:todo` (directly below the
/// backlog, above `agent:review`) when the document has none, appends
/// `- [ ] [#id] <action>`, keeps every existing item, and treats an identical
/// action as a no-op.
#[test]
fn write_todo_add_creates_missing_todo_component_and_preserves_items() {
    let (_tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    assert!(!fs::read_to_string(&doc).unwrap().contains("agent:todo"));
    let first = agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--review-add",
            "id=rvw1 needs live verification",
            "--todo-add",
            "Verify [#rvw1] in a live pane",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&first.get_output().stderr);
    assert!(
        !stderr.contains("[todo] advisory"),
        "a referenced gate needs no advisory: {stderr}"
    );
    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        content.matches("<!-- agent:todo priority -->").count(),
        1,
        "{content}"
    );
    let id = todo_item_id(&content, "Verify [#rvw1] in a live pane");
    // `#todoaddidhijack`: the action cites exactly one open review gate, so
    // the todo LINKS to it by the canonical locator under its own minted id;
    // it never takes the gate's id.
    assert_ne!(id, "rvw1", "a todo never takes an open tracked item's id");
    assert!(
        stderr.contains(&format!("[#{id}] (source: agent:review #rvw1)")),
        "the receipt names the linked source: {stderr}"
    );
    assert_eq!(
        component_body(&content, "todo"),
        format!(
            "- [ ] [#{id}] Verify [#rvw1] in a live pane (source: agent:review [#rvw1])"
        )
    );
    let pending_close = content.find("<!-- /agent:pending -->").unwrap();
    let todo_open = content
        .find("## Todo\n\n<!-- agent:todo priority -->")
        .unwrap();
    let review_open = content.find("<!-- agent:review -->").unwrap();
    assert!(
        pending_close < todo_open && todo_open < review_open,
        "Todo must sit below the backlog and above Review:\n{content}"
    );

    // The operator checks the item off and adds their own note.
    let edited = content.replace(
        &format!(
            "- [ ] [#{id}] Verify [#rvw1] in a live pane (source: agent:review [#rvw1])\n"
        ),
        &format!(
            "- [x] [#{id}] Verify [#rvw1] in a live pane (source: agent:review [#rvw1])\noperator note\n"
        ),
    );
    fs::write(&doc, &edited).unwrap();
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--todo-add",
            "Verify [#rvw1] in a live pane",
            "--todo-add",
            "Publish draft docs/launch-note.md",
        ])
        .write_stdin("<!-- patch:exchange -->\nok again\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        content.matches("<!-- agent:todo priority -->").count(),
        1,
        "{content}"
    );
    let publish = todo_item_id(&content, "Publish draft docs/launch-note.md");
    assert_eq!(
        component_body(&content, "todo"),
        format!(
            "- [x] [#{id}] Verify [#rvw1] in a live pane (source: agent:review [#rvw1])\noperator note\n- [ ] [#{publish}] Publish draft docs/launch-note.md"
        )
    );
}

/// GH #275: `--todo-add` rows (`- [ ] [#id] <action>`, no inline code span)
/// must not read as open backlog items outside the live backlog. Before the
/// fix, the committed document made every later `session-check` exit 1 with
/// `INTERRUPTED: open backlog item(s) exist only outside live agent:backlog`.
#[test]
fn write_todo_add_rows_keep_session_check_clean() {
    let (tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    let root = tmp.path();
    let actions = [
        "Approve or decline the draft reply",
        "Open the preview in MS Edge",
        "Reply to the reviewer",
    ];
    let mut args = vec![
        "write",
        doc.to_str().unwrap(),
        "--force-disk",
        "--backlog-only",
    ];
    for action in actions {
        args.extend(["--todo-add", action]);
    }
    agent_doc().args(&args).assert().success();
    let content = fs::read_to_string(&doc).unwrap();
    for action in actions {
        let id = todo_item_id(&content, action);
        assert!(
            content.contains(&format!("- [ ] [#{id}] {action}\n")),
            "{content}"
        );
    }

    let git = |git_args: &[&str]| {
        let status = std::process::Command::new("git")
            .current_dir(root)
            .args(git_args)
            .status()
            .unwrap();
        assert!(status.success(), "git {git_args:?}");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test User"]);
    git(&["add", "session.md"]);
    git(&["commit", "-q", "-m", "todo rows", "--no-verify"]);
    fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    fs::create_dir_all(root.join(".agent-doc/state/cycles")).unwrap();
    agent_doc_snapshot_io::checkpoint_document_baseline(&doc, &content, |_, _| {}).unwrap();

    let check = agent_doc()
        .current_dir(root)
        .args(["session-check", doc.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&check.stdout);
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        !stdout.contains("outside live agent:backlog")
            && !stderr.contains("outside live agent:backlog"),
        "todo rows must not be shadow backlog items:\n{stdout}\n{stderr}"
    );
    assert!(
        check.status.success() && stdout.contains("[session-check] ok"),
        "session-check must stay clean after --todo-add:\n{stdout}\n{stderr}"
    );
}

/// `#sdktododone`, anonymized from `tasks/sdk.md` on 2026-10-15: the
/// operator drove todos as annotated queue heads (`[#tdrename]: rename to
/// sdk-dev`). The work finished, but the closeout had no way to say "this
/// todo's own id is resolved": `--done tdrename` failed with "id not found in
/// backlog/icebox" because a minted todo id is not tracked work, and the
/// auto-check never saw the bare `#tdrename` own id. The agent hand-patched the
/// box instead. Now `--done <todo own id>` checks that todo off in place (bare
/// or bracketed spelling kept), strikes its queue head in the same
/// transaction, and leaves a todo that only cites the id open.
#[test]
fn write_done_naming_a_todo_own_id_checks_it_off_and_strikes_its_queue_head() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let todo_rows = concat!(
        "- [ ] #tdrename Rename the `dev` branches to `sdk-dev`\n",
        "- [ ] #tdbump Decide the contracts bump (split from #tdrename; #gate1; #cligoshell)\n",
        "- [ ] [#tdwebui] Decide the SDK web UI shape (#gate1)\n",
    );
    fs::write(
        &doc,
        format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue -->\n",
                "- [#tdrename]: rename to sdk-dev\n",
                "- [#tdue]: go ahead\n",
                "<!-- /agent:queue -->\n\n",
                "<!-- agent:backlog -->\n- [/] [#gate1] gated work\n<!-- /agent:backlog -->\n\n",
                "## Todo\n\n<!-- agent:todo -->\n{}<!-- /agent:todo -->\n",
            ),
            todo_rows
        ),
    )
    .unwrap();
    let write = |args: &[&str]| {
        let mut full = vec![
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--backlog-only",
        ];
        full.extend_from_slice(args);
        agent_doc().args(&full).output().unwrap()
    };

    let done = write(&["--done", "tdrename"]);
    let stderr = String::from_utf8_lossy(&done.stderr);
    assert!(
        done.status.success(),
        "--done <todo own id> must not fail: {stderr}"
    );
    assert!(
        stderr.contains("[todo] done: #tdrename checked off in agent:todo"),
        "{stderr}"
    );
    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        component_body(&content, "todo"),
        todo_rows
            .replace("- [ ] #tdrename Rename", "- [x] #tdrename Rename")
            .trim_end_matches('\n'),
        "only the owning row's checkbox changes; the cite-only row stays open"
    );
    let queue = component_body(&content, "queue");
    assert!(
        queue.contains("- ~~[#tdrename]: rename to sdk-dev~~"),
        "queue head struck in the same transaction: {queue}"
    );
    assert!(
        queue.contains("- [#tdue]: go ahead"),
        "other heads stay: {queue}"
    );
    assert!(
        component_body(&content, "backlog").contains("- [/] [#gate1] gated work"),
        "tracked work is untouched: {content}"
    );

    // Replaying the same closeout is a no-op, and a bracketed own id works too.
    let replay = write(&["--done", "tdrename", "--done", "tdwebui"]);
    let stderr = String::from_utf8_lossy(&replay.stderr);
    assert!(replay.status.success(), "{stderr}");
    let content = fs::read_to_string(&doc).unwrap();
    let todo = component_body(&content, "todo");
    assert!(todo.contains("- [x] #tdrename Rename"), "{todo}");
    assert!(
        todo.contains("- [x] [#tdwebui] Decide the SDK web UI shape (#gate1)"),
        "{todo}"
    );
    assert!(todo.contains("- [ ] #tdbump Decide"), "{todo}");
    assert_eq!(
        todo.matches("tdrename Rename").count(),
        1,
        "no second [x] copy: {todo}"
    );

    // An id that names neither tracked work nor a todo's own id still fails;
    // a cited-only id is not an own id.
    for missing in ["nosuchid", "cligoshell"] {
        let failed = write(&["--done", missing]);
        assert!(!failed.status.success(), "--done {missing} must still fail");
        assert!(
            String::from_utf8_lossy(&failed.stderr).contains("id not found"),
            "{}",
            String::from_utf8_lossy(&failed.stderr)
        );
    }
}

/// `#todoidbackfill`, anonymized from `tasks/docs.md` on 2026-10-15: the
/// operator re-authenticated the Drive connector, but the hand-written,
/// id-less todo row stayed open because nothing could address it by id.
/// Preflight now gives every id-less row an own id by the `--todo-add` rules
/// (reuse the one open backlog/review id it cites, else mint), never re-ids a
/// row, keeps the text, and a closeout can then complete the row by that id.
#[test]
fn preflight_assigns_own_ids_to_id_less_todo_rows_and_closeout_completes_them() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let reauth = "Re-authenticate the Google Drive connector (/mcp, sign in) or paste the Google Doc text into tasks/docs.md so #httpsdocsgoogle can proceed; or say skip";
    fs::write(
        &doc,
        format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n",
                "<!-- agent:backlog -->\n- [ ] [#httpsdocsgoogle] Does the Google Doc change PR #51?\n<!-- /agent:backlog -->\n\n",
                "## Todo\n\n<!-- agent:todo -->\n",
                "- [ ] {}\n",
                "- [ ] #tdbare Keep my bare id\n",
                "- [ ] Optional: delete the clean inspection worktrees\n",
                "<!-- /agent:todo -->\n",
            ),
            reauth
        ),
    )
    .unwrap();
    let preflight = || {
        let output = agent_doc()
            .args(["preflight", doc.to_str().unwrap()])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let _: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|err| panic!("preflight JSON ({err}):\n{stdout}\n{stderr}"));
        stderr
    };
    let stderr = preflight();
    assert!(stderr.contains("(#todoidbackfill)"), "{stderr}");
    let content = fs::read_to_string(&doc).unwrap();
    let todo = component_body(&content, "todo").to_string();
    let lines: Vec<&str> = todo.lines().collect();
    assert_eq!(lines.len(), 3, "{todo}");
    // `#todoaddidhijack`: the reauth row cites the open backlog item, so it is
    // LINKED by the canonical locator under its own minted id — it never takes
    // `httpsdocsgoogle`.
    let reauth_id = lines[0]
        .strip_prefix("- [ ] [#")
        .and_then(|rest| {
            rest.split_once(&format!(
                "] {reauth} (source: agent:backlog [#httpsdocsgoogle])"
            ))
        })
        .map(|(id, tail)| {
            assert!(tail.is_empty(), "{}", lines[0]);
            id.to_string()
        })
        .unwrap_or_else(|| panic!("minted, source-linked reauth row expected: {}", lines[0]));
    assert_ne!(reauth_id, "httpsdocsgoogle");
    assert_eq!(
        lines[1], "- [ ] #tdbare Keep my bare id",
        "an owned row is never re-id'd"
    );
    let minted = lines[2]
        .strip_prefix("- [ ] [#")
        .and_then(|rest| rest.split_once("] Optional: delete the clean inspection worktrees"))
        .map(|(id, tail)| {
            assert!(tail.is_empty(), "{}", lines[2]);
            id.to_string()
        })
        .unwrap_or_else(|| panic!("minted own id expected: {}", lines[2]));

    // Idempotent: the next preflight assigns nothing.
    let again = preflight();
    assert!(!again.contains("(#todoidbackfill)"), "{again}");
    assert_eq!(
        component_body(&fs::read_to_string(&doc).unwrap(), "todo"),
        todo
    );

    // The closeout completes a row by its backfilled id, and resolving the
    // backlog item the reauth row links to checks that row off too.
    let write = |args: &[&str]| {
        let mut full = vec![
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--backlog-only",
        ];
        full.extend_from_slice(args);
        agent_doc().args(&full).output().unwrap()
    };
    let done = write(&["--done", &minted, "--done", "httpsdocsgoogle"]);
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let todo = component_body(&fs::read_to_string(&doc).unwrap(), "todo").to_string();
    assert!(
        todo.contains(&format!("- [x] [#{reauth_id}] {reauth}")),
        "{todo}"
    );
    assert!(
        todo.contains(&format!(
            "- [x] [#{minted}] Optional: delete the clean inspection worktrees"
        )),
        "{todo}"
    );
    assert!(todo.contains("- [ ] #tdbare Keep my bare id"), "{todo}");
}

/// `#todocontextmigration`: preflight audits legacy singly linked todo rows in
/// its result before migrating both explicit and implicit links through the
/// same authority-safe maintenance path as id backfill. Ambiguous multi-link
/// rows stay untouched, and the next preflight is clean.
#[test]
fn preflight_audits_and_migrates_legacy_todo_source_qualification() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(
        &doc,
        concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n- [ ] [#alpha] Alpha decision packet\n<!-- /agent:backlog -->\n\n",
            "## Todo\n\n<!-- agent:todo -->\n",
            "- [ ] [#alpha] Decide the alpha policy\n",
            "- [ ] [#custom] Verify [#beta] live\n",
            "- [ ] [#many] Compare [#alpha] with [#beta]\n",
            "<!-- /agent:todo -->\n\n",
            "## Review\n\n<!-- agent:review -->\n- [/] [#beta] Beta verification gate\n<!-- /agent:review -->\n",
        ),
    )
    .unwrap();

    let run = || {
        let output = agent_doc()
            .args(["preflight", doc.to_str().unwrap()])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let parsed: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|err| panic!("preflight JSON ({err}):\n{stdout}\n{stderr}"));
        (parsed, stderr)
    };

    let (first, stderr) = run();
    let legacy: Vec<&serde_json::Value> = first["todo_signals"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|signal| signal["kind"] == "legacy_source_unqualified")
        .collect();
    assert_eq!(legacy.len(), 2, "{first}");
    assert!(
        legacy.iter().any(|signal| signal["todo"] == "alpha")
            && legacy.iter().any(|signal| signal["todo"] == "custom"),
        "{first}"
    );
    assert!(stderr.contains("#todocontextmigration"), "{stderr}");

    let migrated = fs::read_to_string(&doc).unwrap();
    let todo = component_body(&migrated, "todo");
    assert!(
        todo.contains(
            "- [ ] [#alpha] Decide the alpha policy (source: agent:backlog [#alpha])"
        ),
        "{todo}"
    );
    assert!(
        todo.contains(
            "- [ ] [#custom] Verify [#beta] live (source: agent:review [#beta])"
        ),
        "{todo}"
    );
    assert!(
        todo.contains("- [ ] [#many] Compare [#alpha] with [#beta]")
            && !todo.contains("[#many] Compare [#alpha] with [#beta] (source:"),
        "{todo}"
    );
    let ops = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops.contains("todo_source_qualification")
            && ops.contains("todo=alpha id=alpha component=backlog status=migrated")
            && ops.contains("todo=custom id=beta component=review status=migrated"),
        "{ops}"
    );

    let (second, second_stderr) = run();
    assert!(
        second["todo_signals"].as_array().is_none_or(|signals| signals
            .iter()
            .all(|signal| signal["kind"] != "legacy_source_unqualified")),
        "{second}"
    );
    assert!(!second_stderr.contains("#todocontextmigration"), "{second_stderr}");
    assert_eq!(fs::read_to_string(&doc).unwrap(), migrated);
}

/// `#tododone`: the granular todo flags close the operator action loop —
/// add, check off in place, edit, reopen, remove — and never touch an
/// operator-authored item they did not select.
#[test]
fn write_todo_done_edit_reopen_remove_round_trip_preserves_operator_items() {
    let (_tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    let write = |args: &[&str]| {
        let mut full = vec!["write", doc.to_str().unwrap(), "--force-disk", "--backlog-only"];
        full.extend_from_slice(args);
        agent_doc().args(&full).assert().success()
    };
    write(&["--todo-add", "Rotate the deploy key"]);
    let content = fs::read_to_string(&doc).unwrap();
    let id = todo_item_id(&content, "Rotate the deploy key");
    // The operator adds their own items around the agent's.
    let operator = content.replace(
        &format!("- [ ] [#{id}] Rotate the deploy key\n"),
        &format!(
            "Operator note: after lunch\n- [ ] [#{id}] Rotate the deploy key\n- [ ] Call the vendor\n- [x] Already done by me\n"
        ),
    );
    fs::write(&doc, &operator).unwrap();
    let operator_tail = "- [ ] Call the vendor\n- [x] Already done by me";

    let done = write(&["--todo-done", &id]);
    let stderr = String::from_utf8_lossy(&done.get_output().stderr);
    assert!(stderr.contains(&format!("[todo] todo-done: #{id}")), "{stderr}");
    let body = component_body(&fs::read_to_string(&doc).unwrap(), "todo").to_string();
    assert_eq!(
        body,
        format!("Operator note: after lunch\n- [x] [#{id}] Rotate the deploy key\n{operator_tail}")
    );
    // A replayed done is a no-op.
    write(&["--todo-done", &format!("#{id}")]);
    assert_eq!(component_body(&fs::read_to_string(&doc).unwrap(), "todo"), body);

    write(&["--todo-edit", &format!("{id}=Rotate the deploy key (rotated Tue)")]);
    write(&["--todo-reopen", &id]);
    assert_eq!(
        component_body(&fs::read_to_string(&doc).unwrap(), "todo"),
        format!(
            "Operator note: after lunch\n- [ ] [#{id}] Rotate the deploy key (rotated Tue)\n{operator_tail}"
        )
    );
    // An id-less operator item is selected by its exact text.
    write(&["--todo-done", "Call the vendor", "--todo-remove", &id]);
    assert_eq!(
        component_body(&fs::read_to_string(&doc).unwrap(), "todo"),
        "Operator note: after lunch\n- [x] Call the vendor\n- [x] Already done by me"
    );
    // An unknown selector fails the write instead of silently passing.
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--backlog-only",
            "--todo-done",
            "no such item",
        ])
        .assert()
        .failure();
}

/// `#nestedtodoadd`: run one granular `write --backlog-only` mutation against a
/// fixture project whose root search stops at the temp dir.
fn nested_write(tmp: &TempDir, doc: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut full = vec![
        "write",
        doc.to_str().unwrap(),
        "--force-disk",
        "--backlog-only",
    ];
    full.extend_from_slice(args);
    agent_doc()
        .current_dir(tmp.path())
        .env(
            "AGENT_DOC_ROOT_CEILING_DIRECTORIES",
            tmp.path().parent().unwrap(),
        )
        .args(&full)
        .output()
        .unwrap()
}

fn assert_nested_ok(output: &std::process::Output, what: &str) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "{what} failed: stdout={} stderr={stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
    stderr
}

const NESTED_TODO: &str = "id=pmsyncq Answer the PM-sync design questions\n  - Which provider ships first?\n  - Who owns a conflicting edit?\n  - Is the sync pull-only?";
const NESTED_TODO_BODY: &str = "- [ ] [#pmsyncq] Answer the PM-sync design questions\n  - Which provider ships first?\n  - Who owns a conflicting edit?\n  - Is the sync pull-only?";

/// `#nestedtodoadd`: `--todo-add` text whose later lines are `  - <text>`
/// rows renders one todo item with those nested sub-items; a replay is a
/// no-op; `--todo-done`, a single-line `--todo-edit`, and preflight keep the
/// sub-items with their item, never as new items.
#[test]
fn write_todo_add_with_nested_sub_items_renders_replays_and_completes_with_them() {
    let (tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    let stderr = assert_nested_ok(
        &nested_write(&tmp, &doc, &["--todo-add", NESTED_TODO]),
        "nested todo-add",
    );
    assert!(
        stderr.contains("appended operator action [#pmsyncq]"),
        "{stderr}"
    );
    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(component_body(&content, "todo"), NESTED_TODO_BODY);

    // A replayed add (a captured closeout resumed) never duplicates the item or
    // its sub-items.
    let stderr = assert_nested_ok(
        &nested_write(&tmp, &doc, &["--todo-add", NESTED_TODO]),
        "replayed nested todo-add",
    );
    assert!(stderr.contains("already present in agent:todo"), "{stderr}");
    assert!(!stderr.contains("were kept"), "{stderr}");
    assert_eq!(fs::read_to_string(&doc).unwrap(), content);

    // A minted-id add with sub-items, then its replay.
    let minted =
        "Answer the automation-API design questions\n  - REST or RPC?\n  - Which auth scheme?";
    assert_nested_ok(
        &nested_write(&tmp, &doc, &["--todo-add", minted]),
        "minted add",
    );
    let content = fs::read_to_string(&doc).unwrap();
    let api_id = todo_item_id(&content, "Answer the automation-API design questions");
    let api_body = format!(
        "- [ ] [#{api_id}] Answer the automation-API design questions\n  - REST or RPC?\n  - Which auth scheme?"
    );
    assert_eq!(
        component_body(&content, "todo"),
        format!("{NESTED_TODO_BODY}\n{api_body}")
    );
    assert_nested_ok(
        &nested_write(&tmp, &doc, &["--todo-add", minted]),
        "minted replay",
    );
    assert_eq!(fs::read_to_string(&doc).unwrap(), content);

    // Preflight sees two todo rows: it mints no id for a sub-item line and
    // leaves the component byte-for-byte.
    let preflight = agent_doc()
        .current_dir(tmp.path())
        .env(
            "AGENT_DOC_ROOT_CEILING_DIRECTORIES",
            tmp.path().parent().unwrap(),
        )
        .args(["preflight", doc.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&preflight.stdout);
    let pf_stderr = String::from_utf8_lossy(&preflight.stderr);
    let _: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("preflight JSON ({err}):\n{stdout}\n{pf_stderr}"));
    assert!(!pf_stderr.contains("(#todoidbackfill)"), "{pf_stderr}");
    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        component_body(&content, "todo"),
        format!("{NESTED_TODO_BODY}\n{api_body}")
    );

    // Done keeps the sub-items under the checked item.
    assert_nested_ok(
        &nested_write(&tmp, &doc, &["--todo-done", "pmsyncq"]),
        "todo-done",
    );
    let done_body = NESTED_TODO_BODY.replacen("- [ ] [#pmsyncq]", "- [x] [#pmsyncq]", 1);
    assert_eq!(
        component_body(&fs::read_to_string(&doc).unwrap(), "todo"),
        format!("{done_body}\n{api_body}")
    );
    // A replayed add of the checked item never resurrects it.
    assert_nested_ok(
        &nested_write(&tmp, &doc, &["--todo-add", NESTED_TODO]),
        "add after done",
    );
    assert_eq!(
        component_body(&fs::read_to_string(&doc).unwrap(), "todo"),
        format!("{done_body}\n{api_body}")
    );
    // A single-line edit keeps the sub-items; a multi-line edit replaces them.
    assert_nested_ok(
        &nested_write(
            &tmp,
            &doc,
            &[
                "--todo-edit",
                &format!("{api_id}=Answer the automation-API questions"),
            ],
        ),
        "single-line todo-edit",
    );
    assert_nested_ok(
        &nested_write(
            &tmp,
            &doc,
            &[
                "--todo-edit",
                "pmsyncq=Answer the PM-sync design questions\n  - GitHub first.",
            ],
        ),
        "multi-line todo-edit",
    );
    assert_eq!(
        component_body(&fs::read_to_string(&doc).unwrap(), "todo"),
        format!(
            "- [x] [#pmsyncq] Answer the PM-sync design questions\n  - GitHub first.\n- [ ] [#{api_id}] Answer the automation-API questions\n  - REST or RPC?\n  - Which auth scheme?"
        )
    );
    // A replay of the original add now differs from the item's nested lines:
    // still a no-op, and the receipt says the operator's lines were kept.
    let stderr = assert_nested_ok(
        &nested_write(&tmp, &doc, &["--todo-add", NESTED_TODO]),
        "replay over edited sub-items",
    );
    assert!(stderr.contains("were kept"), "{stderr}");
    // Remove deletes the item with its sub-items.
    assert_nested_ok(
        &nested_write(&tmp, &doc, &["--todo-remove", &api_id]),
        "todo-remove",
    );
    assert_eq!(
        component_body(&fs::read_to_string(&doc).unwrap(), "todo"),
        "- [x] [#pmsyncq] Answer the PM-sync design questions\n  - GitHub first."
    );
}

/// `#nestedtodoadd`: malformed sub-item text fails the write and applies
/// nothing, so a bad line is never written outside the item.
#[test]
fn write_todo_add_rejects_malformed_nested_sub_items_and_applies_nothing() {
    let (tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    let before = fs::read_to_string(&doc).unwrap();
    for (text, why) in [
        ("Item\n\n  - child", "is blank"),
        ("Item\n- flush child", "is not a nested sub-item"),
        ("Item\n  prose under it", "is not a nested sub-item"),
        ("Item\n  - one\n    - deeper", "changes indentation"),
        ("Item\n  - [ ] a checkbox child", "is a checkbox row"),
        (
            "Item\n  - <!-- /agent:todo -->",
            "HTML comment or component marker",
        ),
    ] {
        let output = nested_write(
            &tmp,
            &doc,
            &[
                "--backlog-add",
                "id=sidework unrelated add",
                "--todo-add",
                text,
            ],
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{text:?} must fail: {stderr}");
        assert!(stderr.contains(why), "{text:?}: {stderr}");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            before,
            "{text:?}: a rejected write applies nothing"
        );
    }
}

/// `#nestedtodoadd`: the backlog and icebox add flags take the same
/// `  - <text>` sub-item lines; canonicalization files each one as a tracked
/// subtask of its parent (`  - [ ] [#<parent>-<hash>] <text>`). A replayed add
/// (auto id or explicit id) is a no-op: before the fix the auto-id replay filed
/// the item a second time and the explicit-id replay failed the id guard,
/// because the replay compared the plain `- child` lines against the subtasks
/// canonicalization had made of them.
#[test]
fn write_backlog_and_icebox_add_with_nested_sub_items_replay_as_a_noop() {
    let (tmp, doc) = setup_doc_with_icebox("- [ ] [#open] backlog task", "");
    let explicit = "id=pmsync Sync PM state\n  - GitHub issues\n  - Jira tickets";
    let minted = "Automate the release notes\n  - Collect merged PRs";
    let icebox = "id=autoapi Automation API\n  - REST surface";
    let mut first = String::new();
    for pass in 0..2 {
        assert_nested_ok(
            &nested_write(
                &tmp,
                &doc,
                &[
                    "--backlog-add",
                    explicit,
                    "--backlog-add",
                    minted,
                    "--icebox-add",
                    icebox,
                ],
            ),
            &format!("nested backlog/icebox add, pass {pass}"),
        );
        let content = fs::read_to_string(&doc).unwrap();
        if pass == 0 {
            first = content;
        } else {
            assert_eq!(content, first, "a replayed add changes nothing");
        }
    }
    // Shape: parent row, then one parent-prefixed tracked subtask per sub-item.
    let shape = |body: &str| -> Vec<String> {
        // The empty-icebox fixture leaves one blank line ahead of the item.
        body.lines()
            .filter(|line| !line.is_empty())
            .map(|line| match line.split_once("] [#") {
                Some((head, rest)) => {
                    let (id, text) = rest.split_once("] ").unwrap();
                    let id = match id.rsplit_once('-') {
                        Some((parent, _hash)) if head.starts_with("  ") => format!("{parent}-*"),
                        _ => id.to_string(),
                    };
                    format!("{head}] [#{id}] {text}")
                }
                None => line.to_string(),
            })
            .collect()
    };
    let backlog = shape(component_body(&first, "backlog"));
    let minted_id = backlog[3]
        .strip_prefix("- [ ] [#")
        .and_then(|rest| rest.split_once(']'))
        .map(|(id, _)| id.to_string())
        .unwrap_or_else(|| panic!("minted parent row expected: {backlog:#?}"));
    assert_eq!(
        backlog,
        vec![
            "- [ ] [#pmsync] Sync PM state".to_string(),
            "  - [ ] [#pmsync-*] GitHub issues".to_string(),
            "  - [ ] [#pmsync-*] Jira tickets".to_string(),
            format!("- [ ] [#{minted_id}] Automate the release notes"),
            format!("  - [ ] [#{minted_id}-*] Collect merged PRs"),
            "- [ ] [#open] backlog task".to_string(),
        ]
    );
    assert_eq!(
        shape(component_body(&first, "icebox")),
        vec![
            "- [ ] [#autoapi] Automation API".to_string(),
            "  - [ ] [#autoapi-*] REST surface".to_string(),
        ]
    );
}

/// `#tododone`: resolving tracked work that an open todo only CITES (its own
/// id is a custom surrogate, because the todo spans two review items —
/// `#todoidlayer`) prints a non-blocking advisory naming the `--todo-done`
/// command and never writes; checking the todo off in a later closeout
/// silences it.
#[test]
fn write_review_resolve_with_open_todo_emits_todo_done_advisory() {
    let (_tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--review-add",
            "id=rvw3 needs live verification",
            "--review-add",
            "id=rvw4 needs a second live verification",
            "--todo-add",
            "id=vrfy3 Verify [#rvw3] and [#rvw4] in a live pane",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let id = todo_item_id(
        &fs::read_to_string(&doc).unwrap(),
        "Verify [#rvw3] and [#rvw4] in a live pane",
    );
    assert_eq!(
        id, "vrfy3",
        "an explicit surrogate id is kept for a todo spanning two tracked items"
    );
    let resolved = agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--backlog-only",
            "--review-resolve",
            "rvw3",
        ])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&resolved.get_output().stderr);
    assert!(
        stderr.contains("[todo] advisory (#tododone): this closeout resolved #rvw3"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains(&format!("--todo-done {id}")), "stderr: {stderr}");
    assert!(!stderr.contains("[todo] auto-done"), "cite-only todos are never auto-completed: {stderr}");
    // The advisory never writes: the item is still open.
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        component_body(&content, "todo").starts_with(&format!("- [ ] [#{id}]")),
        "{content}"
    );
    let closed = agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--backlog-only",
            "--todo-done",
            &id,
        ])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&closed.get_output().stderr);
    assert!(!stderr.contains("(#tododone): this closeout"), "stderr: {stderr}");
    assert!(
        component_body(&fs::read_to_string(&doc).unwrap(), "todo")
            .starts_with(&format!("- [x] [#{id}]"))
    );
}

/// `#trackedworktodobacklinks`: metadata survives done/archive. A Todo linked
/// from several tracked items stays open until all resolve; one source may
/// clear several Todos.
#[test]
fn tracked_work_backlinks_complete_all_todos_only_after_every_source_is_done() {
    let (_tmp, doc) = setup_doc(concat!(
        "- [ ] [#build] Build release (todos: [#approval])\n",
        "- [ ] [#deploy] Deploy release (todos: [#approval] [#announce])",
    ));
    let write = |args: &[&str]| {
        let mut full = vec!["write", doc.to_str().unwrap(), "--force-disk", "--backlog-only"];
        full.extend_from_slice(args);
        agent_doc().args(&full).assert().success()
    };

    write(&[
        "--todo-add", "id=approval Approve the release",
        "--todo-add", "id=announce Announce the release",
    ]);
    let first = write(&["--done", "build"]);
    let first_stderr = String::from_utf8_lossy(&first.get_output().stderr);
    assert!(!first_stderr.contains("[todo] auto-done"), "{first_stderr}");
    let content = fs::read_to_string(&doc).unwrap();
    assert!(component_body(&content, "todo").contains("- [ ] [#approval]"));
    assert!(component_body(&content, "todo").contains("- [ ] [#announce]"));

    let second = write(&["--done", "deploy"]);
    let second_stderr = String::from_utf8_lossy(&second.get_output().stderr);
    assert!(second_stderr.contains("[todo] auto-done: #approval"), "{second_stderr}");
    assert!(second_stderr.contains("[todo] auto-done: #announce"), "{second_stderr}");
    let content = fs::read_to_string(&doc).unwrap();
    assert!(component_body(&content, "todo").contains("- [x] [#approval]"));
    assert!(component_body(&content, "todo").contains("- [x] [#announce]"));
}

/// `#trackedworktodobacklinks`: free-text queue heads cannot evade the
/// backlink invariant. A head still held by the typing gate is explicitly
/// reported until normal preflight can give it a stable backlog surrogate.
#[test]
fn free_text_queue_head_is_promoted_then_flagged_without_a_todo_backlink() {
    let (_tmp, doc) = setup_doc("");
    let content = fs::read_to_string(&doc).unwrap().replace(
        "<!-- /agent:exchange -->",
        "<!-- /agent:exchange -->\n\n<!-- agent:queue go -->\n- Ship the release\n<!-- /agent:queue -->",
    );
    fs::write(&doc, content).unwrap();

    let output = agent_doc()
        .args(["preflight", doc.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "preflight failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!(
            "preflight JSON ({err}): stdout={stdout} stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let signals = parsed["todo_signals"].as_array().expect("todo_signals array");
    assert!(signals.iter().any(|signal| {
        signal["kind"] == "tracked_work_missing_todo_backlink"
            && signal["ids"].as_array().is_some_and(Vec::is_empty)
            && signal["action"].as_str().is_some_and(|action| action.contains("promote"))
    }), "{stdout}");
}

/// `#todoids` end to end: a todo linked to a review gate (by its canonical
/// source locator, under its own minted id, `#todoaddidhijack`) is the
/// operator prompt for that gate.
/// 1. `--todo-add` citing the open gate links to it; a second add for the
///    same gate folds into it; an explicit id naming resolved work is refused.
/// 2. The operator checks the box in the editor (here: on disk). Preflight
///    recognises it with no agent write and signals that the gate is ready to
///    resolve; a later stale `patch:todo` carrying the item open cannot
///    uncheck it.
/// 3. A todo linked to a gate the closeout resolves is checked off in that
///    same closeout, with an `ops.log` reason.
#[test]
fn todo_ids_link_review_gates_and_round_trip_through_preflight_and_closeout() {
    let (_tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    let write = |args: &[&str], stdin: &str| {
        let mut full = vec!["write", doc.to_str().unwrap(), "--force-disk"];
        full.extend_from_slice(args);
        agent_doc().args(&full).write_stdin(stdin.to_string()).assert().success()
    };
    let exchange = "<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n";
    write(
        &[
            "--review-add",
            "id=gatea launcher policy decision pending",
            "--review-add",
            "id=gateb live verification pending",
            "--todo-add",
            "Decide the launcher policy (#gatea)",
            "--todo-add",
            "Verify [#gateb] in the 263 IDE",
        ],
        exchange,
    );
    let content = fs::read_to_string(&doc).unwrap();
    let todo_a = todo_item_id(&content, "Decide the launcher policy (#gatea)");
    let todo_b = todo_item_id(&content, "Verify [#gateb] in the 263 IDE");
    for own in [&todo_a, &todo_b] {
        assert!(!["gatea", "gateb"].contains(&own.as_str()), "{content}");
    }
    assert_eq!(
        component_body(&content, "todo"),
        format!(
            "- [ ] [#{todo_a}] Decide the launcher policy (#gatea) (source: agent:review [#gatea])\n- [ ] [#{todo_b}] Verify [#gateb] in the 263 IDE (source: agent:review [#gateb])"
        )
    );
    // A second todo for an already-prompted gate folds into the open item.
    let again = write(&["--backlog-only", "--todo-add", "Verify [#gateb] again"], "");
    let stderr = String::from_utf8_lossy(&again.get_output().stderr);
    assert!(stderr.contains("already present"), "{stderr}");
    assert_eq!(component_body(&fs::read_to_string(&doc).unwrap(), "todo").lines().count(), 2);

    // 2. The operator checks gatea's box; no agent write happens.
    let checked = fs::read_to_string(&doc).unwrap().replace(
        &format!("- [ ] [#{todo_a}] Decide"),
        &format!("- [x] [#{todo_a}] Decide"),
    );
    fs::write(&doc, &checked).unwrap();
    // Preflight reads the same document in a fresh project (the write cycles
    // above were never committed, which preflight would otherwise recover first).
    // The OPEN gateb todo stays: a todo own id is tracked, so the
    // shadow-backlog guard admits it, and a todo linked to OPEN work signals
    // nothing until the operator acts.
    let (_preflight_tmp, preflight_doc) = setup_doc("");
    fs::write(&preflight_doc, &checked).unwrap();
    let preflight = agent_doc()
        .args(["preflight", preflight_doc.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&preflight.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!(
            "preflight JSON ({err}): stdout={stdout} stderr={}",
            String::from_utf8_lossy(&preflight.stderr)
        )
    });
    let signals = parsed
        .get("todo_signals")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("todo_signals missing: {stdout}"));
    let signal = signals
        .iter()
        .find(|signal| signal["kind"] == "operator_done_review_open")
        .unwrap_or_else(|| panic!("operator_done_review_open missing: {stdout}"));
    assert_eq!(signal["todo"], todo_a.as_str());
    assert!(
        signal["action"].as_str().unwrap().contains("--review-resolve gatea"),
        "{stdout}"
    );
    // A stale patch:todo (built before the check) carries gatea open; the
    // operator's check survives.
    write(
        &[],
        &format!(
            "<!-- patch:exchange -->\nstale\n<!-- /patch:exchange -->\n<!-- patch:todo -->\n- [ ] [#{todo_a}] Decide the launcher policy (#gatea) (source: agent:review [#gatea])\n- [ ] [#{todo_b}] Verify [#gateb] in the 263 IDE (source: agent:review [#gateb])\n<!-- /patch:todo -->\n"
        ),
    );
    assert!(
        component_body(&fs::read_to_string(&doc).unwrap(), "todo")
            .starts_with(&format!("- [x] [#{todo_a}] Decide")),
        "an agent write must not revert the operator's check"
    );

    // 3. Resolving gateb checks its linked todo off in the same closeout.
    let resolved = write(&["--backlog-only", "--review-resolve", "gateb"], "");
    let stderr = String::from_utf8_lossy(&resolved.get_output().stderr);
    assert!(stderr.contains(&format!("[todo] auto-done: #{todo_b}")), "{stderr}");
    assert!(!stderr.contains("(#tododone): this closeout"), "{stderr}");
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        component_body(&content, "todo")
            .contains(&format!("- [x] [#{todo_b}] Verify [#gateb] in the 263 IDE")),
        "{content}"
    );
    let ops_log = fs::read_to_string(
        doc.parent().unwrap().join(".agent-doc/logs/ops.log"),
    )
    .unwrap_or_default();
    assert!(
        ops_log.contains("todo_auto_done")
            && ops_log.contains(&format!("id={todo_b} reason=linked_work_resolved")),
        "{ops_log}"
    );
    // An explicit todo id naming resolved work is moot and refused.
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--backlog-only",
            "--todo-add",
            "id=gateb Verify again",
        ])
        .assert()
        .failure();
}

/// `#todoaddonsamecycledone`: the exact 2026-10-27 closeout sequence. One
/// write carried `--done claimfollowstruck` and a `--todo-add` whose action
/// cites `#claimfollowstruck` only as context. Before the fix the todo ops
/// (which run before `--done`) layered the row on the still-open item, the
/// same closeout's auto-completion checked it off, and a replay of the
/// captured plan (evaluated after the item resolved, so with no locator) no
/// longer recognised that row and minted an open surrogate:
/// `[x] [#12t8] ... (source: agent:backlog [#claimfollowstruck])` +
/// `[ ] [#krwr] ...`. Now exactly one open, standalone follow-up exists, and a
/// replay folds into it.
#[test]
fn todo_add_citing_an_item_the_same_closeout_resolves_mints_one_open_follow_up() {
    let (_tmp, doc) = setup_doc_with_icebox(
        "- [ ] [#claimfollowstruck] fix claim follow\n- [ ] [#other] other work",
        "",
    );
    let action = "Decide whether deleting a claimed queue row should strike (current #queue-mutation-steering design; raised by the #claimfollowstruck fix).";
    let closeout = |extra: &[&str]| {
        let mut args = vec![
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "claimfollowstruck",
            "--todo-add",
            action,
        ];
        args.extend_from_slice(extra);
        agent_doc()
            .args(&args)
            .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
            .assert()
            .success()
    };
    let first = closeout(&[]);
    let stderr = String::from_utf8_lossy(&first.get_output().stderr).to_string();
    assert!(!stderr.contains("[todo] auto-done"), "{stderr}");
    assert!(
        !stderr.contains("(#tododone)"),
        "the new follow-up is not finished work: {stderr}"
    );
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        component_body(&content, "backlog").contains("[x] [#claimfollowstruck]")
            || !component_body(&content, "backlog").contains("claimfollowstruck"),
        "{content}"
    );
    let todo = todo_item_id(&content, action);
    assert_eq!(
        component_body(&content, "todo"),
        format!("- [ ] [#{todo}] {action}"),
        "one open standalone follow-up, no locator to finished work"
    );

    // A replay of the same closeout (a captured-plan resume) is a no-op.
    let replay = agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--backlog-only",
            "--todo-add",
            action,
        ])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&replay.get_output().stderr);
    assert!(stderr.contains("already present"), "{stderr}");
    assert_eq!(
        component_body(&fs::read_to_string(&doc).unwrap(), "todo"),
        format!("- [ ] [#{todo}] {action}")
    );

    // A later closeout with tracked-work flags leaves the follow-up open: it
    // only cites the resolved item, so it is never auto-completed.
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--backlog-only",
            "--done",
            "other",
        ])
        .assert()
        .success();
    assert_eq!(
        component_body(&fs::read_to_string(&doc).unwrap(), "todo"),
        format!("- [ ] [#{todo}] {action}")
    );
}

/// `#todoaddonsamecycledone`: two `--done` + `--todo-add` pairs in ONE
/// closeout each mint exactly one open follow-up; a todo added in an EARLIER
/// cycle and layered on one of those items keeps its semantics (checked off
/// by the closeout that resolves it), and an add citing an item the closeout
/// does not resolve still layers on it.
#[test]
fn two_done_and_todo_add_pairs_in_one_closeout_keep_both_follow_ups_open() {
    let (_tmp, doc) = setup_doc_with_icebox(
        "- [ ] [#claimfollowstruck] fix claim follow\n- [ ] [#prunetestisolation] prune isolation\n- [ ] [#keepopen] still open",
        "",
    );
    // Earlier cycle: an operator prompt layered on #prunetestisolation.
    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--backlog-only",
            "--todo-add",
            "Approve the isolation plan for [#prunetestisolation]",
        ])
        .assert()
        .success();
    let earlier = todo_item_id(
        &fs::read_to_string(&doc).unwrap(),
        "Approve the isolation plan for [#prunetestisolation]",
    );
    let follow_a = "Decide whether deleting a claimed queue row strikes (raised by the #claimfollowstruck fix).";
    let follow_b =
        "Decide whether ~/.agent-doc/ is pruned (found by #prunetestisolation; see logs).";
    let layered = "Grant CI access for [#keepopen]";
    let out = agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "claimfollowstruck",
            "--todo-add",
            follow_a,
            "--done",
            "prunetestisolation",
            "--todo-add",
            follow_b,
            "--todo-add",
            layered,
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let content = fs::read_to_string(&doc).unwrap();
    let a = todo_item_id(&content, follow_a);
    let b = todo_item_id(&content, follow_b);
    let c = todo_item_id(&content, layered);
    assert_eq!(
        component_body(&content, "todo"),
        format!(
            "- [x] [#{earlier}] Approve the isolation plan for [#prunetestisolation] (source: agent:backlog [#prunetestisolation])\n- [ ] [#{a}] {follow_a}\n- [ ] [#{b}] {follow_b}\n- [ ] [#{c}] {layered} (source: agent:backlog [#keepopen])"
        ),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("[todo] auto-done: #{earlier}")),
        "{stderr}"
    );
    for id in [&a, &b, &c] {
        assert!(
            !stderr.contains(&format!("[todo] auto-done: #{id}")),
            "{stderr}"
        );
    }
}

/// `#todoaddidhijack` end to end: a todo is a visibility layer over the one
/// review/backlog/icebox item it surfaces an operator decision for, but its
/// own id is its OWN identity — an explicit `id=` always wins, an id an open
/// tracked item owns is refused, and a cited `#<id>` citation links by the canonical
/// `(source: agent:<component> [#<id>])` locator.
/// 1. `--todo-add` keeps an explicit custom id beside a cited review gate,
///    mints for a cited parked icebox item, a two-item action, and an
///    unassociated action.
/// 2. The operator checks the icebox-linked todo; preflight signals
///    `operator_done_icebox_open` and never moves or closes the item itself.
/// 3. `--done <icebox id>` resolves the icebox item, and the same closeout
///    auto-completes the linked todo.
/// 4. An explicit `id=` that an open tracked item owns is refused without
///    writing.
#[test]
fn todo_ids_never_take_a_tracked_id_and_link_by_source_locator() {
    let (_tmp, doc) = setup_doc_with_icebox("- [ ] [#open] backlog task", "- [ ] [#coldwork] parked idea");
    let write = |args: &[&str], stdin: &str| {
        let mut full = vec!["write", doc.to_str().unwrap(), "--force-disk"];
        full.extend_from_slice(args);
        agent_doc().args(&full).write_stdin(stdin.to_string()).assert().success()
    };
    let added = write(
        &[
            "--review-add",
            "id=gatex launcher policy decision pending",
            "--todo-add",
            "id=tdgatex Decide the launcher policy (#gatex)",
            "--todo-add",
            "Thaw or drop the parked idea (#coldwork)?",
            "--todo-add",
            "Decide #gatex before thawing #coldwork",
            "--todo-add",
            "Rotate the deploy key",
        ],
        "<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n",
    );
    let stderr = String::from_utf8_lossy(&added.get_output().stderr);
    assert!(stderr.contains("[#tdgatex] (source: agent:review #gatex)"), "{stderr}");
    assert!(!stderr.contains("not used"), "{stderr}");
    let content = fs::read_to_string(&doc).unwrap();
    let todo = component_body(&content, "todo");
    let lines: Vec<&str> = todo.lines().collect();
    assert_eq!(lines.len(), 4, "{todo}");
    assert_eq!(
        lines[0],
        "- [ ] [#tdgatex] Decide the launcher policy (#gatex) (source: agent:review [#gatex])"
    );
    let cold = todo_item_id(&content, "Thaw or drop the parked idea (#coldwork)?");
    assert!(stderr.contains(&format!("[#{cold}] (source: agent:icebox #coldwork)")), "{stderr}");
    assert_eq!(
        lines[1],
        format!("- [ ] [#{cold}] Thaw or drop the parked idea (#coldwork)? (source: agent:icebox [#coldwork])")
    );
    let two = todo_item_id(&content, "Decide #gatex before thawing #coldwork");
    let none = todo_item_id(&content, "Rotate the deploy key");
    for own in [&cold, &two, &none] {
        assert!(
            !["gatex", "coldwork", "open"].contains(&own.as_str()),
            "todo id {own} must not take a tracked id: {todo}"
        );
    }
    // The icebox item is still parked: linking never moves it.
    assert!(component_body(&content, "icebox").contains("- [ ] [#coldwork] parked idea"));

    // 2. The operator checks the icebox-linked todo; preflight (fresh project,
    //    the write cycles above were never committed) signals it.
    let checked = content.replace(
        &format!("- [ ] [#{cold}] Thaw"),
        &format!("- [x] [#{cold}] Thaw"),
    );
    let (_preflight_tmp, preflight_doc) = setup_doc("");
    fs::write(&preflight_doc, &checked).unwrap();
    let preflight = agent_doc()
        .args(["preflight", preflight_doc.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&preflight.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!(
            "preflight JSON ({err}): stdout={stdout} stderr={}",
            String::from_utf8_lossy(&preflight.stderr)
        )
    });
    let signals = parsed
        .get("todo_signals")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("todo_signals missing: {stdout}"));
    let signal = signals
        .iter()
        .find(|signal| signal["kind"] == "operator_done_icebox_open")
        .unwrap_or_else(|| panic!("operator_done_icebox_open missing: {stdout}"));
    assert_eq!(signal["todo"], cold.as_str());
    assert!(
        signal["action"].as_str().unwrap().contains("--done coldwork"),
        "{stdout}"
    );
    assert!(
        component_body(&fs::read_to_string(&preflight_doc).unwrap(), "icebox")
            .contains("- [ ] [#coldwork] parked idea"),
        "preflight never moves or closes parked work from a checkbox"
    );

    // 3. `--done coldwork` resolves the icebox item, and the linked todo is
    //    auto-completed in the same closeout.
    let done = write(&["--backlog-only", "--done", "coldwork"], "");
    let stderr = String::from_utf8_lossy(&done.get_output().stderr);
    assert!(stderr.contains(&format!("[todo] auto-done: #{cold}")), "{stderr}");
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !component_body(&content, "icebox").contains("- [ ] [#coldwork]"),
        "the icebox item is resolved: {content}"
    );
    assert!(
        component_body(&content, "todo").contains(&format!(
            "- [x] [#{cold}] Thaw or drop the parked idea (#coldwork)?"
        )),
        "{content}"
    );
    assert!(
        component_body(&content, "todo").contains("- [ ] [#tdgatex] Decide the launcher policy"),
        "the review-linked todo stays open: {content}"
    );

    // 4. An explicit id the open review gate owns is refused, nothing written.
    let before = fs::read_to_string(&doc).unwrap();
    let refused = agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--backlog-only",
            "--todo-add",
            "id=gatex Decide the launcher policy again",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr);
    assert!(
        stderr.contains("already owned by the open agent:review item [#gatex]")
            && stderr.contains("#todoaddidhijack"),
        "{stderr}"
    );
    assert_eq!(fs::read_to_string(&doc).unwrap(), before);
}

/// `#todoaddidhijack`, the exact live 2026-10-26 closeout shape: `--done`,
/// `--icebox-add id=...`, and `--todo-add id=<custom> ...` whose action text
/// mentions the open, claimed backlog item `#jbshrinkrestarts`. The todo must
/// get its explicit id `jb339r1decision`, never `jbshrinkrestarts`; the backlog
/// item keeps sole ownership of its id.
#[test]
fn todo_add_explicit_id_wins_over_a_cited_open_backlog_id_in_a_live_closeout() {
    let (_tmp, doc) = setup_doc_with_icebox(
        "- [/] [#jbshrinkrestarts] Shrink JetBrains restarts and report the benchmark\n- [ ] [#jb339shelldesign] GH #339 restart-free shell design",
        "- [ ] [#parked] parked idea",
    );
    let output = agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "jb339shelldesign",
            "--icebox-add",
            "id=jbfrontendswap Swap the JetBrains frontend loader after R1",
            "--todo-add",
            "id=jb339r1decision Decide whether to queue GH #339 shell stage R1 (restart-free shell) once #jbshrinkrestarts reports its benchmark and moves. Same decision: restate the \"Exact-263 decision\" reopen rule",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    let content = fs::read_to_string(&doc).unwrap();
    let todo = component_body(&content, "todo");
    assert!(
        todo.starts_with("- [ ] [#jb339r1decision] Decide whether to queue GH #339 shell stage R1"),
        "the explicit id wins: {todo}\n{stderr}"
    );
    assert!(!todo.contains("[ ] [#jbshrinkrestarts]"), "{todo}");
    assert!(
        todo.trim_end().ends_with("(source: agent:backlog [#jbshrinkrestarts])"),
        "the cited item stays a reference, linked by locator: {todo}"
    );
    assert!(stderr.contains("[#jb339r1decision] (source: agent:backlog #jbshrinkrestarts)"), "{stderr}");
    assert!(
        component_body(&content, "icebox").contains("[#jbfrontendswap] Swap the JetBrains frontend loader"),
        "{content}"
    );
    assert!(
        component_body(&content, "backlog").contains("- [/] [#jbshrinkrestarts] Shrink JetBrains restarts"),
        "the backlog item keeps its id: {content}"
    );
}

/// `#todoaddidhijack`: `--review-add` runs the same explicit-id guard as
/// `--backlog-add` / `--icebox-add`, so a review gate never takes the id an
/// open backlog item owns; replaying an identical review add stays a no-op.
#[test]
fn review_add_refuses_an_explicit_id_an_open_backlog_item_owns() {
    let (_tmp, doc) = setup_doc_with_icebox("- [ ] [#open] backlog task", "- [ ] [#parked] parked idea");
    let before = fs::read_to_string(&doc).unwrap();
    for item in ["id=open needs a live check", "id=parked thaw decision"] {
        let refused = agent_doc()
            .args(["write", doc.to_str().unwrap(), "--force-disk", "--backlog-only", "--review-add", item])
            .assert()
            .failure();
        let stderr = String::from_utf8_lossy(&refused.get_output().stderr);
        assert!(stderr.contains("refusing to add item with explicit id"), "{item}: {stderr}");
        assert_eq!(fs::read_to_string(&doc).unwrap(), before, "{item}");
    }
    for _ in 0..2 {
        agent_doc()
            .args([
                "write",
                doc.to_str().unwrap(),
                "--force-disk",
                "--backlog-only",
                "--review-add",
                "id=gatey needs a live check",
            ])
            .assert()
            .success();
    }
    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(component_body(&content, "review").matches("[#gatey]").count(), 1, "{content}");
}

/// `#tododupdone`, anonymized from the live 2026-10-15 session: a `patch:todo`
/// that completes items by appending checked copies must check the existing
/// items, not leave the open originals beside the copies. A follow-up that
/// cites the same tracked id stays a separate item.
#[test]
fn write_patch_todo_completion_copy_checks_existing_item_without_duplicating() {
    let (_tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    let original = concat!(
        "- [ ] Decide the api merge order of #645 vs #715 (#apimigrate)\n",
        "- [ ] Approve/merge contracts #160 and pick the version bump (#bundleadopt)\n",
        "- [ ] Decide provisional sessions, option 3. Talk to the leads. (#provauth)\n",
    );
    agent_doc()
        .args(["write", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin(format!(
            "<!-- patch:exchange -->\nseed\n<!-- /patch:exchange -->\n<!-- patch:todo -->\n{original}<!-- /patch:todo -->\n"
        ))
        .assert()
        .success();
    let patch = format!(
        "{original}{}",
        concat!(
            "- [x] Decide the api merge order of #645 vs #715 (#apimigrate) — moot: either order works\n",
            "- [x] Approve/merge contracts #160 and pick the version bump (#bundleadopt) — merged; continues as #162 below\n",
            "- [ ] Decide provisional sessions, option 3. Talk to the leads. (#provauth)\n",
            "- [ ] Approve/merge contracts #162 once CI can run (#bundleadopt)\n",
        )
    );
    agent_doc()
        .args(["write", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin(format!(
            "<!-- patch:exchange -->\nclosing\n<!-- /patch:exchange -->\n<!-- patch:todo -->\n{patch}<!-- /patch:todo -->\n"
        ))
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        component_body(&content, "todo"),
        concat!(
            "- [x] Decide the api merge order of #645 vs #715 (#apimigrate) — moot: either order works\n",
            "- [x] Approve/merge contracts #160 and pick the version bump (#bundleadopt) — merged; continues as #162 below\n",
            "- [ ] Decide provisional sessions, option 3. Talk to the leads. (#provauth)\n",
            "- [ ] Approve/merge contracts #162 once CI can run (#bundleadopt)",
        ),
        "{content}"
    );
}

/// `#operatortodo`: a closeout that opens a human gate without any todo naming
/// it gets a deterministic, non-blocking advisory naming the id and the
/// `--todo-add` command; the write itself still succeeds.
#[test]
fn write_review_add_without_operator_todo_emits_advisory() {
    let (_tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    let assert_result = agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--review-add",
            "id=rvw2 waiting on operator",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
    assert!(
        stderr.contains("[todo] advisory (#operatortodo): this closeout gated #rvw2"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("--todo-add"), "stderr: {stderr}");
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("agent:todo"),
        "the advisory never writes a todo by itself:\n{content}"
    );
}

/// `#operatortodo`, GH #314: completing an old todo must not let a response
/// hand a new unpublished-draft decision to the operator without an advisory.
#[test]
fn write_response_request_without_todo_add_emits_operator_todo_advisory() {
    let (_tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    let content = fs::read_to_string(&doc).unwrap().replace(
        "<!-- /agent:exchange -->\n",
        "<!-- /agent:exchange -->\n\n<!-- agent:todo -->\n- [ ] [#old] Review prior draft\n<!-- /agent:todo -->\n",
    );
    fs::write(&doc, content).unwrap();

    let assert_result = agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--todo-done",
            "old",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n**Description: draft below, not posted.** Say `post` to apply it. Full draft: `docs/launch-note.md`.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
    assert!(
        stderr.contains(
            "[todo] advisory (#operatortodo): this closeout response leaves 1 operator action pending but supplied 0 --todo-add"
        ),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("--todo-add"), "stderr: {stderr}");
    assert!(
        component_body(&fs::read_to_string(&doc).unwrap(), "todo")
            .contains("- [x] [#old] Review prior draft")
    );
}

/// GH #318: a bolded `**Still stale:**` list is operator work. One
/// `--todo-add` no longer silences the advisory for the whole closeout; it
/// counts asks against adds and quotes each unrecorded item with the label
/// that marked it, in stderr and in ops.log.
#[test]
fn write_residual_work_list_with_one_todo_add_still_emits_operator_todo_advisory() {
    let (tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    let assert_result = agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--todo-add",
            "Rotate the deploy key",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: alice reply posted, note rewritten\n\nPosted the reply.\n\n**Still stale:**\n- Your note `n-a` in `t-a` still argues for tags over a list.\n- The description section still cites note `n-b` for the tags-vs-list argument.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
    assert!(
        stderr.contains(
            "this closeout response leaves 2 operator actions pending but supplied 1 --todo-add; no todo matches: \"Your note `n-a` in `t-a` still argues for tags over a list.\" (residual work list: \"Still stale\")"
        ),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("\"The description section still cites note `n-b`"),
        "stderr: {stderr}"
    );
    let ops = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    let line = ops
        .lines()
        .find(|line| line.contains("operator_todo_response_advisory"))
        .unwrap_or_else(|| panic!("ops.log: {ops}"));
    assert!(
        line.contains("signal=residual work list asks=2 todo_adds=1 unrecorded=2"),
        "{line}"
    );
    assert!(
        line.contains("matched=[\"Still stale\", \"Still stale\"]"),
        "{line}"
    );
    assert!(
        !line.contains("reply post"),
        "the heading's `reply posted` is not a signal: {line}"
    );
}

/// GH #318: `reply posted` in a response heading is not the `reply post`
/// request signal, and an ask an open `agent:todo` item already records does
/// not re-fire the advisory.
#[test]
fn write_heading_or_already_recorded_ask_emits_no_operator_todo_advisory() {
    let (_tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    let content = fs::read_to_string(&doc).unwrap().replace(
        "<!-- /agent:exchange -->\n",
        "<!-- /agent:exchange -->\n\n<!-- agent:todo -->\n- [ ] [#stalenote] Fix note n-a in t-a that still argues tags over a list\n<!-- /agent:todo -->\n",
    );
    fs::write(&doc, content).unwrap();

    let assert_result = agent_doc()
        .args(["write", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: alice reply posted, note rewritten\n\nPosted the reply.\n\n**Still stale:**\n- Your note `n-a` in `t-a` still argues for tags over a list.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
    assert!(
        !stderr.contains("this closeout response leaves"),
        "stderr: {stderr}"
    );
}

/// `#packettodo`: the 3ad3e23c closeout response — a blocking
/// `[#subagent-operator-request-<unique-id>]` packet beside a review gate, with
/// the `queue_item`, `request`, and `context` values nested beneath their keys.
const BLOCKING_PACKET_RESPONSE: &str = "<!-- patch:exchange -->\n\
`#apipendingorphans` is gated on review of #733 and on your decision below.\n\
\n\
[#subagent-operator-request-pending-rows] subagent operator request\n\
- *queue_item:*\n\
\x20\x20- do [#apipendingorphans]\n\
- *kind:* question\n\
- *blocking:* true\n\
- *request:*\n\
\x20\x20- What should happen to the chat messages already stuck in `pending` on STG (including the rows from the 2026-10-20 incident)? And should a reaper be added?\n\
- *context:*\n\
\x20\x20- PR #733 stops new stuck rows for refused sends and edits.\n\
\x20\x20- The rows from the 2026-10-20 incident are still `pending`.\n\
- *choices:*\n\
\x20\x20- **withdraw**: soft-delete the stuck sends.\n\
\x20\x20- **resubmit**: send each stuck row to FPE again.\n\
\x20\x20- **release**: set the stuck rows to `allowed` without screening.\n\
\x20\x20- **leave**: change nothing.\n\
\n\
**Recommendation:** withdraw, after STG gets migration `0022`.\n\
<!-- /patch:exchange -->\n";

/// The same closeout in the historical layout (each value on its key line) that
/// documents written before nested values still carry.
const LEGACY_FLAT_PACKET_RESPONSE: &str = "<!-- patch:exchange -->\n\
`#apipendingorphans` is gated on review of #733 and on your decision below.\n\
\n\
[#subagent-operator-request-pending-rows] subagent operator request\n\
- *queue_item:* do [#apipendingorphans]\n\
- *kind:* question\n\
- *blocking:* true\n\
- *request:* What should happen to the chat messages already stuck in `pending` on STG (including the rows from the 2026-10-20 incident)? And should a reaper be added?\n\
- *context:* PR #733 stops new stuck rows for refused sends and edits.\n\
- *choices:*\n\
\x20\x20- **withdraw**: soft-delete the stuck sends.\n\
\x20\x20- **resubmit**: send each stuck row to FPE again.\n\
\x20\x20- **release**: set the stuck rows to `allowed` without screening.\n\
\x20\x20- **leave**: change nothing.\n\
\n\
**Recommendation:** withdraw, after STG gets migration `0022`.\n\
<!-- /patch:exchange -->\n";

const PACKET_TODO: &str = "Answer the blocking subagent question for tracked work apipendingorphans for queue item ‘do [#apipendingorphans]’ in agent:exchange: What should happen to the chat messages already stuck in `pending` on STG (including the rows from the 2026-10-20 incident)? (choices: withdraw, resubmit, release, leave)";

fn write_packet_closeout(doc: &std::path::Path, extra: &[&str], response: &str) -> String {
    let mut args = vec!["write", doc.to_str().unwrap(), "--force-disk"];
    args.extend_from_slice(extra);
    let assert_result = agent_doc()
        .args(&args)
        .write_stdin(response.to_string())
        .assert()
        .success();
    String::from_utf8_lossy(&assert_result.get_output().stderr).into_owned()
}

/// `#packettodo` / `#wayposttodo`: a closeout presenting a blocking operator
/// request packet and gating its item, but passing no `--todo-add`, still
/// records the operator decision in `agent:todo` under the packet's own id.
#[test]
fn write_blocking_operator_request_packet_records_layered_todo_without_todo_add() {
    for response in [BLOCKING_PACKET_RESPONSE, LEGACY_FLAT_PACKET_RESPONSE] {
        assert_layered_packet_todo(response);
    }
}

fn assert_layered_packet_todo(response: &str) {
    let (_tmp, doc) = setup_doc("- [ ] [#apipendingorphans] stuck pending chat messages");
    let stderr = write_packet_closeout(
        &doc,
        &["--pending-gate", "apipendingorphans"],
        response,
    );
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        component_body(&content, "review").contains("- [/] [#apipendingorphans]"),
        "{content}"
    );
    assert_eq!(
        component_body(&content, "todo"),
        format!("- [ ] [#subagent-operator-request-pending-rows] {PACKET_TODO}"),
        "{content}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("[todo] blocking operator request packet (#packettodo)"),
        "stderr: {stderr}"
    );
    assert!(
        !stderr.contains("this closeout gated #apipendingorphans in agent:review but agent:todo does not reference it"),
        "the derived todo references the gate, so no unreferenced-gate advisory: {stderr}"
    );

    // Replay of the same packet (no new gate; the response still cites the
    // item) folds into the open layered todo instead of duplicating it.
    write_packet_closeout(&doc, &[], response);
    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        component_body(&content, "todo")
            .matches("Answer the blocking subagent question")
            .count(),
        1,
        "{content}"
    );
    assert_eq!(
        component_body(&content, "todo").lines().count(),
        1,
        "{content}"
    );
}

/// `#packettodo`: an explicit `--todo-add` is the closeout's own record of the
/// operator decision; the binary adds no second, derived item.
#[test]
fn write_blocking_operator_request_packet_with_todo_add_adds_no_duplicate() {
    let (_tmp, doc) = setup_doc("- [ ] [#apipendingorphans] stuck pending chat messages");
    write_packet_closeout(
        &doc,
        &[
            "--pending-gate",
            "apipendingorphans",
            "--todo-add",
            "id=subagent-operator-request-pending-rows Decide what to do with the stuck STG pending rows",
        ],
        BLOCKING_PACKET_RESPONSE,
    );
    let content = fs::read_to_string(&doc).unwrap();
    let todo = component_body(&content, "todo");
    assert_eq!(todo.lines().count(), 1, "{content}");
    assert!(
        todo.contains("Decide what to do with the stuck STG pending rows"),
        "{content}"
    );
    assert!(!todo.contains("Answer the blocking subagent"), "{content}");
}

/// `#queueoperatoraction`: one request identity is projected across exchange,
/// todo, and the exact blocked queue row, then clears when the todo closes.
#[test]
fn write_blocking_operator_request_projects_and_clears_queue_indicator() {
    let (_tmp, doc) = setup_doc("- [ ] [#apipendingorphans] stuck pending chat messages");
    let initial = fs::read_to_string(&doc).unwrap();
    fs::write(
        &doc,
        format!(
            "{initial}\n<!-- agent:queue -->\n- do [#apipendingorphans]\n<!-- /agent:queue -->\n"
        ),
    )
    .unwrap();

    write_packet_closeout(&doc, &[], BLOCKING_PACKET_RESPONSE);
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        component_body(&content, "queue")
            .contains("🙋 do [#apipendingorphans] [#subagent-operator-request-pending-rows]"),
        "{content}"
    );
    assert!(
        component_body(&content, "todo").contains("[#subagent-operator-request-pending-rows]"),
        "{content}"
    );

    let answered = content.replace(
        "- 🙋 do [#apipendingorphans] [#subagent-operator-request-pending-rows]\n",
        "- 🙋 do [#apipendingorphans] [#subagent-operator-request-pending-rows]: withdraw\n",
    );
    fs::write(&doc, answered).unwrap();
    let stderr = write_packet_closeout(
        &doc,
        &[],
        "<!-- patch:exchange -->\nOperator answered.\n<!-- /patch:exchange -->\n",
    );
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !component_body(&content, "queue").contains("🙋"),
        "{content}"
    );
    assert!(
        component_body(&content, "todo")
            .contains("- [x] [#subagent-operator-request-pending-rows]"),
        "{content}"
    );
    assert!(
        component_body(&content, "queue")
            .contains("[#subagent-operator-request-pending-rows]: withdraw"),
        "the local answer remains beside its request id: {content}"
    );
    assert!(stderr.contains("#queueoperatoranswer"), "stderr: {stderr}");
}

/// `#packettodo`: a non-blocking packet, a packet quoted in a code fence, or a
/// blockquoted packet is not a pending operator decision.
#[test]
fn write_nonblocking_or_quoted_operator_request_packet_records_no_todo() {
    let nonblocking = BLOCKING_PACKET_RESPONSE.replace("*blocking:* true", "*blocking:* false");
    let fenced = BLOCKING_PACKET_RESPONSE
        .replace(
            "[#subagent-operator-request-pending-rows] subagent operator request\n",
            "```text\n[#subagent-operator-request-pending-rows] subagent operator request\n",
        )
        .replace(
            "  - **leave**: change nothing.\n",
            "  - **leave**: change nothing.\n```\n",
        );
    assert!(fenced.contains("```text\n") && fenced.contains("nothing.\n```\n"));
    let quoted = BLOCKING_PACKET_RESPONSE
        .lines()
        .map(|line| {
            if line.starts_with("<!--") {
                line.to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    for response in [nonblocking, fenced, quoted] {
        let (_tmp, doc) = setup_doc("- [ ] [#apipendingorphans] stuck pending chat messages");
        let stderr = write_packet_closeout(&doc, &[], &response);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(
            !content.contains("agent:todo"),
            "no todo for a non-blocking or quoted packet:\n{content}\nstderr: {stderr}"
        );
        assert!(!stderr.contains("#packettodo"), "stderr: {stderr}");
    }
}

/// Request markers are tracked identities, so one closeout may not present two
/// different operator questions under the same id.
#[test]
fn write_rejects_duplicate_blocking_operator_request_ids() {
    let (_tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    let duplicate = BLOCKING_PACKET_RESPONSE.replace(
        "<!-- /patch:exchange -->\n",
        &format!("\n{BLOCKING_PACKET_RESPONSE}"),
    );
    agent_doc()
        .args(["write", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin(duplicate)
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "reuse [#subagent-operator-request-pending-rows]",
        ))
        .stderr(predicates::str::contains(
            "give every packet a unique [#subagent-operator-request-<unique-id>] marker",
        ));
}

/// `#packettodo`: with no single tracked item, the derived todo still owns the
/// packet id.
#[test]
fn write_blocking_operator_request_packet_without_tracked_item_mints_surrogate_todo() {
    let (_tmp, doc) = setup_doc("- [ ] [#open] unrelated backlog task");
    let response = BLOCKING_PACKET_RESPONSE.replace(
        "`#apipendingorphans` is gated on review of #733 and on your decision below.",
        "The worker needs your decision below.",
    );
    write_packet_closeout(&doc, &[], &response);
    let content = fs::read_to_string(&doc).unwrap();
    let todo = component_body(&content, "todo");
    assert_eq!(todo.lines().count(), 1, "{content}");
    assert!(
        todo.starts_with("- [ ] [#subagent-operator-request-pending-rows]"),
        "{content}"
    );
    assert!(
        todo.contains("Answer the blocking subagent question")
            && todo.contains("in agent:exchange: What should happen to the chat messages"),
        "{content}"
    );
    assert!(!todo.contains("[#open]"), "{content}");
}

/// `#operatortodo`: a `patch:todo` against a document without `agent:todo`
/// creates the component instead of spilling the checklist into the exchange.
#[test]
fn write_patch_todo_creates_missing_todo_component() {
    let (_tmp, doc) = setup_doc("- [ ] [#open] backlog task");
    agent_doc()
        .args(["write", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin(
            "<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n<!-- patch:todo -->\n- [ ] Publish draft docs/launch-note.md\n<!-- /patch:todo -->\n",
        )
        .assert()
        .success();
    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        component_body(&content, "todo"),
        "- [ ] Publish draft docs/launch-note.md"
    );
    assert!(
        !component_body(&content, "exchange").contains("Publish draft"),
        "todo checklist must not spill into exchange:\n{content}"
    );
    let pending_close = content.find("<!-- /agent:pending -->").unwrap();
    let todo_open = content.find("<!-- agent:todo priority -->").unwrap();
    assert!(pending_close < todo_open, "{content}");
}

/// `#closeouttrackedtxn`: one finalize/write envelope may close some review
/// gates, move another review item to icebox, reactivate and edit a third item,
/// and add fresh backlog work. Review exits must release their ids before adds,
/// while ungating must precede backlog edits.
#[test]
fn write_mixed_review_closeout_mutations_land_as_one_valid_envelope() {
    let (_tmp, doc) = setup_doc("");
    let content = fs::read_to_string(&doc).unwrap().replace(
        "<!-- /agent:pending -->\n",
        "<!-- /agent:pending -->\n\n\
         <!-- agent:review -->\n\
         - [/] [#resolved] completed review work\n\
         - [/] [#parked] review work to park\n\
         - [/] [#reactivated] review work to reactivate\n\
         <!-- /agent:review -->\n\n\
         <!-- agent:icebox -->\n\
         <!-- /agent:icebox -->\n\n\
         <!-- agent:done -->\n\
         <!-- /agent:done -->\n",
    );
    fs::write(&doc, content).unwrap();

    agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--review-resolve",
            "resolved",
            "--review-remove",
            "parked",
            "--icebox-add",
            "id=parked parked after review",
            "--pending-ungate",
            "reactivated",
            "--pending-edit",
            "reactivated=reactivated and narrowed",
            "--pending-add",
            "id=fresh newly captured follow-up",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    let backlog = component_body(&content, "pending");
    let review = component_body(&content, "review");
    let icebox = component_body(&content, "icebox");
    let done = component_body(&content, "done");
    assert!(backlog.contains("- [ ] [#fresh] newly captured follow-up"));
    assert!(backlog.contains("- [ ] [#reactivated] reactivated and narrowed"));
    assert!(!review.contains("[#resolved]"));
    assert!(!review.contains("[#parked]"));
    assert!(!review.contains("[#reactivated]"));
    assert!(icebox.contains("- [ ] [#parked] parked after review"));
    assert!(done.contains("[#resolved] completed review work"));
}

/// A semantic failure late in a mixed tracked-work envelope must not publish an
/// earlier successful add. This is the partial-closeout shape that stranded a
/// response and one backlog item while every following review transition was
/// silently absent.
#[test]
fn write_mixed_review_closeout_error_rolls_back_all_tracked_work() {
    let (_tmp, doc) = setup_doc("");
    let content = fs::read_to_string(&doc).unwrap().replace(
        "<!-- /agent:pending -->\n",
        "<!-- /agent:pending -->\n\n\
         <!-- agent:review -->\n\
         - [/] [#parked] review work still owns this id\n\
         <!-- /agent:review -->\n\n\
         <!-- agent:icebox -->\n\
         <!-- /agent:icebox -->\n",
    );
    fs::write(&doc, content).unwrap();

    let assert_result = agent_doc()
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add",
            "id=fresh must roll back",
            "--icebox-add",
            "id=parked duplicate id must fail",
        ])
        .write_stdin("<!-- patch:exchange -->\nok\n<!-- /patch:exchange -->\n")
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
    assert!(stderr.contains("parked"), "stderr: {stderr}");
    assert!(
        stderr.to_ascii_lowercase().contains("already"),
        "stderr must retain the concrete duplicate-id cause: {stderr}"
    );
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !component_body(&content, "pending").contains("[#fresh]"),
        "an earlier tracked-work add must not survive a later flag failure:\n{content}"
    );
    assert!(component_body(&content, "review").contains("[#parked]"));
    assert!(!component_body(&content, "icebox").contains("[#parked]"));
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

/// `#preflightcallbackroot`: preflight scans pending callbacks in the
/// DOCUMENT's project, not the process cwd's. Observed in `make check` on
/// 2026-10-16: once a test left `.agent-doc/state.db` in the agent-doc
/// checkout, every later preflight test (cwd = the checkout, document in a temp
/// dir) opened that checkout's state.db, tripped the `#testisolationleaks`
/// runtime-scope guard, and printed no JSON.
#[test]
fn preflight_scans_callbacks_in_the_document_project_not_the_cwd_project() {
    let outer = TempDir::new().unwrap();
    let cwd_project = outer.path().join("cwd-project");
    fs::create_dir_all(cwd_project.join(".agent-doc")).unwrap();
    // A foreign project's state.db, in the cwd's project root.
    fs::write(cwd_project.join(".agent-doc/state.db"), "").unwrap();
    let doc_project = outer.path().join("doc-project");
    fs::create_dir_all(doc_project.join(".agent-doc/snapshots")).unwrap();
    let doc = doc_project.join("session.md");
    fs::write(
        &doc,
        "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n<!-- agent:pending -->\n- [ ] [#aaaa] open\n<!-- /agent:pending -->\n",
    )
    .unwrap();
    let doc_scope = doc_project.canonicalize().unwrap();
    let output = agent_doc()
        .current_dir(&cwd_project)
        .env("AGENT_DOC_TEST_RUNTIME_SCOPE", &doc_scope)
        .env("AGENT_DOC_ROOT_CEILING_DIRECTORIES", outer.path())
        .args(["preflight", doc.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("outside its runtime scope"),
        "preflight must not open the cwd project's state.db:\n{stderr}"
    );
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("preflight JSON ({err}):\n{stdout}\n{stderr}"));
    assert!(parsed.is_object(), "{stdout}");
    assert_eq!(
        fs::metadata(cwd_project.join(".agent-doc/state.db"))
            .unwrap()
            .len(),
        0,
        "the cwd project's state.db stays untouched"
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

/// `#bkqattrcli`: reproduce the reported defect end to end, then prove the new
/// command fixes it. Before `set-attr`, the only route to a `queue` attribute was
/// hand-editing the marker — unavailable precisely when the document is open in a
/// live editor, because a raw disk edit there becomes a disk candidate the agent
/// must never resolve.
#[test]
fn backlog_set_attr_makes_backlog_adds_mirrorable_into_the_queue() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(
        &doc,
        "---\nagent_doc_format: template\n---\n\n\
         <!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
         <!-- agent:queue -->\n<!-- /agent:queue -->\n\n\
         <!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
    )
    .unwrap();

    // The marker starts without `queue`, which is the reported state.
    let before = fs::read_to_string(&doc).unwrap();
    assert!(
        before.contains("<!-- agent:backlog -->"),
        "fixture must start without a queue attribute:\n{before}"
    );

    let output = agent_doc()
        .args([
            "backlog",
            doc.to_str().unwrap(),
            "set-attr",
            "queue",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "set-attr queue failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let after = fs::read_to_string(&doc).unwrap();
    assert!(
        after.contains("<!-- agent:backlog queue -->"),
        "set-attr must add the bare queue flag to the marker:\n{after}"
    );
    // A marker-span edit must not disturb the body or any other component.
    assert!(
        after.contains("<!-- agent:queue -->\n<!-- /agent:queue -->"),
        "an unrelated component must be untouched:\n{after}"
    );

    // And it round-trips: unset-attr returns the marker to its original shape.
    let output = agent_doc()
        .args([
            "backlog",
            doc.to_str().unwrap(),
            "unset-attr",
            "queue",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "unset-attr queue failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&doc).unwrap(),
        before,
        "unset-attr must restore the original marker byte for byte"
    );
}

/// An unrecognized attribute is PARSED but ignored, so accepting it would look
/// like it worked and change nothing — the exact silent-failure class this command
/// exists to remove.
#[test]
fn backlog_set_attr_refuses_an_attribute_that_would_be_ignored() {
    let (_tmp, doc) = setup_doc("- [ ] [#a1] item");

    let output = agent_doc()
        .args(["backlog", doc.to_str().unwrap(), "set-attr", "qeueu"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "a typo'd attribute must be refused, not silently written"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not an editable `agent:backlog` attribute")
            && stderr.contains("queue")
            && stderr.contains("priority"),
        "the refusal must name what is accepted: {stderr}"
    );

    let output = agent_doc()
        .args([
            "backlog",
            doc.to_str().unwrap(),
            "set-attr",
            "queue",
            "sideways",
        ])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "an unrecognized queue sync mode must be refused"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not a recognized backlog sync mode"),
        "the refusal must name the recognized modes: {stderr}"
    );
}

/// `#reviewpriority`: `agent-doc review set-attr <FILE> priority` adds the bare
/// flag to the `agent:review` marker through the CLI, preflight then sorts the
/// gated review items by their `priority=<1..9>` token without flagging the
/// attribute as unrecognized, and `queue` stays refused on review.
#[test]
fn review_set_attr_priority_sorts_review_items_on_preflight() {
    let (_tmp, doc) = setup_doc("- [ ] [#aaaa] open");
    let content = fs::read_to_string(&doc).unwrap().replace(
        "<!-- /agent:pending -->\n",
        "<!-- /agent:pending -->\n\n<!-- agent:review -->\n- [/] [#rlow] priority=4 later\n- [/] [#rhigh] priority=1 first\n<!-- /agent:review -->\n",
    );
    fs::write(&doc, content).unwrap();

    let refused = agent_doc()
        .args(["review", "set-attr", doc.to_str().unwrap(), "queue"])
        .output()
        .unwrap();
    assert!(
        !refused.status.success(),
        "queue must not be editable on agent:review"
    );
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("not an editable `agent:review` attribute") && stderr.contains("priority"),
        "the refusal must name what review accepts: {stderr}"
    );

    let output = agent_doc()
        .args(["review", "set-attr", doc.to_str().unwrap(), "priority"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "review set-attr priority failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let after = fs::read_to_string(&doc).unwrap();
    assert!(
        after.contains("<!-- agent:review priority -->"),
        "set-attr must add the bare priority flag to the review marker:\n{after}"
    );

    let output = agent_doc()
        .args(["lint", doc.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "`agent:review priority` must pass lint: {}",
        String::from_utf8_lossy(&output.stderr)
    );

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
        !warnings
            .iter()
            .any(|warning| warning.get("code").and_then(|v| v.as_str())
                == Some("misplaced_component_attr")),
        "priority on agent:review must be a recognized attribute: {}",
        serde_json::Value::Array(warnings)
    );
    assert_eq!(parsed.get("review_count").and_then(|v| v.as_u64()), Some(2));

    let sorted = fs::read_to_string(&doc).unwrap();
    let review = sorted
        .split("<!-- agent:review priority -->")
        .nth(1)
        .unwrap();
    assert!(
        review.find("[#rhigh]").unwrap() < review.find("[#rlow]").unwrap(),
        "preflight must sort the priority review by item rank:\n{sorted}"
    );

    let output = agent_doc()
        .args(["review", "unset-attr", doc.to_str().unwrap(), "priority"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "review unset-attr priority failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fs::read_to_string(&doc)
            .unwrap()
            .contains("<!-- agent:review -->"),
        "unset-attr must restore the bare review marker"
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
