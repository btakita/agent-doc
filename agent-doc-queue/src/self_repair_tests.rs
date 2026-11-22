use super::*;

fn doc(queue: &str) -> String {
    format!(
        "---\nagent_doc_format: template\n---\n\n## Run <!-- agent:run id=\"main\" subagents -->\n{queue}<!-- /agent:run -->\n\n## Exchange <!-- agent:exchange -->\n<!-- /agent:exchange -->\n"
    )
}

fn retired(_: &str, _: u64) -> bool {
    false
}

fn unproven(_: &str) -> bool {
    false
}

/// The incident shape (anonymized): three struck copies of one row, only one
/// holding the worker's response tree, and a doubled struck operator prompt
/// above the prompt that reported it.
#[test]
fn extra_struck_copies_collapse_to_the_copy_holding_the_tree() {
    let queue = concat!(
        "- ### group\n",
        "  - ~~amend core-v1~~\n",
        "  - ~~amend core-v1~~\n",
        "    - **Response [#r-92b7ccc53170]**\n",
        "      - **Summary:** amended.\n",
        "  - ~~amend core-v1~~\n",
        "  - ~~Should we integrate these?~~\n",
        "  - ~~Should we integrate these?~~\n",
        "- ### agent-doc duplicated my prompt above this prompt.\n",
    );
    let (repaired, repairs) = dedupe_struck_rows(&doc(queue));
    assert_eq!(
        repaired,
        doc(concat!(
            "- ### group\n",
            "  - ~~amend core-v1~~\n",
            "    - **Response [#r-92b7ccc53170]**\n",
            "      - **Summary:** amended.\n",
            "  - ~~Should we integrate these?~~\n",
            "- ### agent-doc duplicated my prompt above this prompt.\n",
        ))
    );
    assert_eq!(repairs.len(), 3, "{repairs:?}");
    assert!(
        repairs
            .iter()
            .all(|repair| repair.kind == SelfRepairKind::DuplicateStruckRow)
    );
    let (again, none) = dedupe_struck_rows(&repaired);
    assert_eq!(again, repaired, "a second pass is a no-op");
    assert!(none.is_empty());
}

/// Similar operator text is never a duplicate: a different word, a live row,
/// a copy under another parent, and copies with different children all stay.
#[test]
fn similar_or_unique_operator_rows_are_kept() {
    let queue = concat!(
        "- ~~Should we integrate these?~~\n",
        "- ~~Should we integrate this?~~\n",
        "- ~~Should we integrate these?~~ ⟦published⟧\n",
        "- Should we integrate these?\n",
        "- Should we integrate these?\n",
        "- ### parent\n",
        "  - ~~Should we integrate these?~~\n",
        "- ~~keep after unlocking~~\n",
        "  - yes, keep it\n",
        "- ~~keep after unlocking~~\n",
        "  - no, drop it\n",
    );
    let document = doc(queue);
    let (repaired, repairs) = dedupe_struck_rows(&document);
    assert_eq!(repaired, document, "{repairs:?}");
    assert!(repairs.is_empty());
}

/// A copy whose children match a kept copy's byte for byte (relative to its
/// own indent) holds nothing unique and is removed.
#[test]
fn a_copy_with_identical_children_is_removed() {
    let queue = concat!(
        "- ~~Do the next actions.~~\n",
        "  - ~~What is the status of the parent?~~\n",
        "- ~~Do the next actions.~~\n",
        "  - ~~What is the status of the parent?~~\n",
    );
    let (repaired, repairs) = dedupe_struck_rows(&doc(queue));
    assert_eq!(
        repaired,
        doc(concat!(
            "- ~~Do the next actions.~~\n",
            "  - ~~What is the status of the parent?~~\n",
        ))
    );
    assert_eq!(repairs.len(), 1);
}

/// A retired attempt's label leaves struck rows and follow-up titles that
/// quote it; a live attempt, a live row's leading projection, and a label
/// quoted inside inline code are untouched.
#[test]
fn retired_claim_labels_leave_titles_and_struck_rows() {
    let queue = concat!(
        "- ~~running [#subagent:effectvoid#1] amend core-v1~~\n",
        "- 🙋 follow up on an action item from queue item “running [#subagent:effectvoid#1] amend core-v1”: operator: decide whether to amend core-v1. [operator-verify] [#q-8eeadf7937c8]\n",
        "- 🚧 [#subagent:ci53#1] check the CI run for `53fa578ad6`\n",
        "- follow up on an action item from queue item “[#subagent:live#2] keep going”: agent: finish it. [#q-1234567890ab]\n",
        "- follow up on an action item from queue item “[#subagent:gone#3] id-less”: agent: finish it.\n",
        "- 🙋 operator: the doc has struck `running [#subagent:effectvoid#1] amend core-v1` copies; decide whether to keep them. [operator-verify]\n",
    );
    let is_live = |owner: &str, generation: u64| owner == "subagent:live" && generation == 2;
    let (repaired, repairs) = strip_stale_claim_labels(&doc(queue), &is_live);
    assert_eq!(
        repaired,
        doc(concat!(
            "- ~~amend core-v1~~\n",
            "- 🙋 follow up on an action item from queue item “amend core-v1”: operator: decide whether to amend core-v1. [operator-verify] [#q-8eeadf7937c8]\n",
            "- 🚧 [#subagent:ci53#1] check the CI run for `53fa578ad6`\n",
            "- follow up on an action item from queue item “[#subagent:live#2] keep going”: agent: finish it. [#q-1234567890ab]\n",
            "- follow up on an action item from queue item “[#subagent:gone#3] id-less”: agent: finish it.\n",
            "- 🙋 operator: the doc has struck `running [#subagent:effectvoid#1] amend core-v1` copies; decide whether to keep them. [operator-verify]\n",
        ))
    );
    assert_eq!(repairs.len(), 2, "{repairs:?}");
    assert!(
        repairs
            .iter()
            .all(|repair| repair.kind == SelfRepairKind::StaleClaimLabel)
    );
    let (again, none) = strip_stale_claim_labels(&repaired, &is_live);
    assert_eq!(again, repaired);
    assert!(none.is_empty());
}

/// Generated response structure keeps its prose, labels included.
#[test]
fn response_structure_rows_keep_labels() {
    let queue = concat!(
        "- ~~amend core-v1~~\n",
        "  - **Response [#r-92b7ccc53170]**\n",
        "    - **Summary:** all three rows carried [#subagent:effectvoid#1] text.\n",
        "    - **Action items:** <!-- agent-doc:response-detail -->\n",
        "      - agent: find [#subagent:effectvoid#1] origin. <!-- agent-doc:response-detail -->\n",
    );
    let document = doc(queue);
    let (repaired, repairs) = strip_stale_claim_labels(&document, &retired);
    assert_eq!(repaired, document, "{repairs:?}");
}

/// All passes together: the labelled struck copies become identical to the
/// original row and collapse onto the copy holding the response tree, and a
/// response tree copied under a second same-text row collapses too.
#[test]
fn self_repair_document_heals_the_incident_and_is_idempotent() {
    let queue = concat!(
        "- ### group\n",
        "  - ~~running [#subagent:effectvoid#1] amend core-v1~~\n",
        "  - ~~amend core-v1~~\n",
        "    - **Response [#r-92b7ccc53170]**\n",
        "      - **Timestamp:** 2026-11-21T08:05:59.000Z\n",
        "      - **Summary:** amended.\n",
        "      - **State:** Complete.\n",
        "  - ~~running [#subagent:effectvoid#1] amend core-v1~~\n",
    );
    let (repaired, repairs) = self_repair_document(&doc(queue), &retired, &unproven).unwrap();
    assert_eq!(
        repaired,
        doc(concat!(
            "- ### group\n",
            "  - ~~amend core-v1~~\n",
            "    - **Response [#r-92b7ccc53170]**\n",
            "      - **Timestamp:** 2026-11-21T08:05:59.000Z\n",
            "      - **Summary:** amended.\n",
            "      - **State:** Complete.\n",
        ))
    );
    assert!(
        repairs
            .iter()
            .any(|repair| repair.kind == SelfRepairKind::StaleClaimLabel)
    );
    assert!(
        repairs
            .iter()
            .any(|repair| repair.kind == SelfRepairKind::DuplicateStruckRow)
    );
    assert!(self_repair_document(&repaired, &retired, &unproven).is_none());
}

#[test]
fn duplicate_response_tree_collapses_through_the_canonical_heal() {
    let queue = concat!(
        "- ~~amend core-v1~~\n",
        "  - **Response [#r-92b7ccc53170]**\n",
        "    - **Summary:** amended.\n",
        "    - **State:** Complete.\n",
        "  - **Response [#r-92b7ccc53170]**\n",
        "    - **Summary:** amended.\n",
        "    - **State:** Complete.\n",
    );
    let (repaired, repairs) = self_repair_document(&doc(queue), &retired, &unproven).unwrap();
    assert_eq!(
        repaired.matches("**Response [#r-92b7ccc53170]**").count(),
        1,
        "{repaired}"
    );
    assert_eq!(repairs[0].kind, SelfRepairKind::DuplicateResponseTree);
    assert_eq!(repairs[0].detail, "[#r-92b7ccc53170]");
}

/// The live cleanup asks from the incident are agent-doc's own artifacts;
/// operator work about anything else is not.
#[test]
fn cleanup_items_for_agent_doc_artifacts_are_recognized() {
    for item in [
        "operator: in the live `tasks/agent-doc/sample-bugs.md` queue, delete the extra struck copies of `running [#subagent:effectvoid#1] amend core-v1` (there are 3) and any doubled `~~Should we integrate these?~~`.",
        "operator: the live `sample-bugs.md` has struck `running [#subagent:effectvoid#1] amend core-v1` copies and follow-up titles quoting the label; decide whether to clean those up by hand.",
        "operator: in the editor, delete the misplaced copy under ` - ~~Complete the work. ⟦published⟧~~`: the block from ` - **Response [#r-8eb6516470b1]**` through its Guidance line.",
        "operator: apply edits 1–3 above in the editor if you want the old titles cleaned, then strike the \"delete the extra struck copies\" rows.",
    ] {
        assert!(is_self_repair_cleanup_item(item), "{item}");
    }
    for item in [
        "operator: decide whether to revoke the PyPI API token named `publish 2026-11-13`.",
        "operator: decide whether to refresh the VS Code package (`make install` skipped it).",
        "operator: unlock the `login` keyring once.",
        "operator: delete the duplicate GitHub release draft for v1.2.",
        "agent: delete the extra struck copies of `amend core-v1`.",
        "operator: remove the stale worktree under `/tmp/sample`.",
    ] {
        assert!(!is_self_repair_cleanup_item(item), "{item}");
    }
}

/// The audited `--ack-text` tree (anonymized): a whole hand-back tree was
/// copied under another response's `**Guidance:**` line. Its original is in
/// the exchange archive, so the copy's generated rows go; the operator's
/// answer row (with its own response) and the live operator row nested in it
/// stay under an inert backlink, byte for byte.
#[test]
fn a_response_tree_under_a_guidance_line_is_retired_keeping_operator_rows() {
    let queue = concat!(
        "- ### follow up on an action item: agent: stop the ack-text copy.\n",
        "  - **Response [#r-2da6da4a7fd8]**\n",
        "    - **Summary:** Fix pushed.\n",
        "    - **State:** Follow-up work remains.\n",
        "    - **Guidance:** More work remains. Keep this tree and follow the action items in this response.\n",
        "      - **Response [#r-8eb6516470b1]**\n",
        "        - **Timestamp:** 2026-11-21T20:20:16.000Z\n",
        "        - **Summary:** <!-- agent-doc:response-detail -->\n",
        "          Both gh commands now return in under a second.\n",
        "        - **What changed:** <!-- agent-doc:response-detail -->\n",
        "          - New file `gh` wrapper. <!-- agent-doc:response-detail -->\n",
        "        - **Action items:** <!-- agent-doc:response-detail -->\n",
        "          - operator: unlock the `login` keyring once. [operator-verify] <!-- agent-doc:response-detail -->\n",
        "            - ~~Present the password prompt and complete.~~\n",
        "              - **Response [#r-37bf1620fdbd]**\n",
        "                - **Summary:** The keyring was already unlocked.\n",
        "                - **State:** Complete.\n",
        "        - **State:** Follow-up work remains.\n",
        "        - **Guidance:** More work remains. Keep this tree and follow the action items in this response.\n",
        "        - 🙋 operator: confirm the copy is gone. [operator-verify] [#q-8834c5a41276]\n",
    );
    let proven = |id: &str| id == "[#r-8eb6516470b1]";
    let (repaired, repairs) = self_repair_document(&doc(queue), &retired, &proven).unwrap();
    assert_eq!(
        repairs,
        vec![SelfRepair {
            kind: SelfRepairKind::MisplacedResponseTree,
            detail: "[#r-8eb6516470b1]".to_string(),
        }]
    );
    assert!(!repaired.contains("Both gh commands"), "{repaired}");
    assert!(!repaired.contains("New file `gh` wrapper"), "{repaired}");
    assert!(
        repaired.contains("**Closed by:** [#r-8eb6516470b1]"),
        "{repaired}"
    );
    for kept in [
        "            - ~~Present the password prompt and complete.~~\n",
        "              - **Response [#r-37bf1620fdbd]**\n",
        "        - 🙋 operator: confirm the copy is gone. [operator-verify] [#q-8834c5a41276]\n",
        "  - **Response [#r-2da6da4a7fd8]**\n",
    ] {
        assert!(repaired.contains(kept), "{kept}\n{repaired}");
    }
    assert!(self_repair_document(&repaired, &retired, &proven).is_none());
    // Without proof that the original lives elsewhere, nothing is touched.
    assert!(self_repair_document(&doc(queue), &retired, &unproven).is_none());
}

/// A copy with nothing but generated rows beneath it goes entirely.
#[test]
fn a_misplaced_generated_only_copy_is_removed_outright() {
    let queue = concat!(
        "- ~~fix the thing~~\n",
        "  - **Response [#r-2da6da4a7fd8]**\n",
        "    - **State:** Complete.\n",
        "    - **Guidance:** Safe to delete.\n",
        "      - **Response [#r-8eb6516470b1]**\n",
        "        - **Summary:** copied hand-back.\n",
        "        - **State:** Complete.\n",
    );
    let proven = |id: &str| id == "[#r-8eb6516470b1]";
    let (repaired, _) = self_repair_document(&doc(queue), &retired, &proven).unwrap();
    assert_eq!(
        repaired,
        doc(concat!(
            "- ~~fix the thing~~\n",
            "  - **Response [#r-2da6da4a7fd8]**\n",
            "    - **State:** Complete.\n",
            "    - **Guidance:** Safe to delete.\n",
        ))
    );
}

/// `#donetrack` end to end on the audited `--ack-text` tree (anonymized):
/// every agent item is done and one operator row is left. Self-repair
/// retires the misplaced hand-back copy (its original is in the exchange
/// archive), and the root response then says it waits on the operator, with
/// a done tally and the one thing to do, instead of "Follow-up work remains".
#[test]
fn audited_tree_reads_as_one_operator_action_left() {
    let session = include_str!("../tests/fixtures/response_remaining_session.md");
    // The operator's leaf was finished and struck.
    let leaf = "        - Complete the rest of the work.";
    let at = session.find(leaf).unwrap();
    let end = at + session[at..].find('\n').unwrap();
    let body = &session[at + "        - ".len()..end];
    let document = format!("{}        - ~~{body}~~{}", &session[..at], &session[end..]);

    let proven = |id: &str| id == "[#r-8eb6516470b1]";
    let (repaired, repairs) = self_repair_document(&document, &retired, &proven).unwrap();
    assert!(
        repairs
            .iter()
            .any(|repair| repair.kind == SelfRepairKind::MisplacedResponseTree)
    );
    assert!(
        !repaired.contains("Both gh commands now return"),
        "{repaired}"
    );
    assert!(repaired.contains("- ~~Present the password prompt and complete.~~"));

    let (synced, _, _) = crate::response_tree_sync::sync_response_trees(&repaired).unwrap();
    let root = synced.find("- **Response [#r-2da6da4a7fd8]**").unwrap();
    let state = synced[root..]
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("- **State:** "))
        .unwrap();
    let guidance = synced[root..]
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("- **Guidance:** "))
        .unwrap();
    assert_eq!(
        state,
        crate::queue_consume::RESPONSE_STATE_WAITING_ON_OPERATOR
    );
    assert!(guidance.starts_with("🙋 "), "{guidance}");
    assert!(
        guidance.contains(
            " done, 1 yours. Only you can finish this: in the editor, delete the misplaced copy"
        ),
        "{guidance}"
    );
    assert!(crate::response_tree_sync::sync_response_trees(&synced).is_none());
    assert!(self_repair_document(&synced, &retired, &proven).is_none());
    eprintln!("{guidance}");
}
