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
            rewritten: None,
        }]
    );
    // `#selfrepaircleanup`: the kept operator rows are the backlink's direct
    // children, their own structure unchanged.
    assert_eq!(
        repaired,
        doc(concat!(
            "- ### follow up on an action item: agent: stop the ack-text copy.\n",
            "  - **Response [#r-2da6da4a7fd8]**\n",
            "    - **Summary:** Fix pushed.\n",
            "    - **State:** Follow-up work remains.\n",
            "    - **Guidance:** More work remains. Keep this tree and follow the action items in this response.\n",
            "      - **Closed by:** [#r-8eb6516470b1]\n",
            "        - ~~Present the password prompt and complete.~~\n",
            "          - **Response [#r-37bf1620fdbd]**\n",
            "            - **Summary:** The keyring was already unlocked.\n",
            "            - **State:** Complete.\n",
            "        - 🙋 operator: confirm the copy is gone. [operator-verify] [#q-8834c5a41276]\n",
        ))
    );
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

/// `#donetrack` / `#treefinish` end to end on the audited `--ack-text` tree
/// (anonymized): every agent item is done and the one operator row left asks
/// to delete the misplaced hand-back copy. Self-repair retires that copy (its
/// original is in the exchange archive), so the operator row is resolved too
/// and the root response says the tree is finished, with the done tally,
/// instead of "Follow-up work remains".
#[test]
fn audited_tree_reads_as_one_operator_action_left() {
    let session = include_str!("../tests/fixtures/response_remaining_session.md");
    // The operator's leaf was finished and struck.
    let leaf = "        - Complete the rest of the work.";
    let at = session.find(leaf).unwrap();
    let end = at + session[at..].find('\n').unwrap();
    let body = &session[at + "        - ".len()..end];
    let document = format!("{}        - ~~{body}~~{}", &session[..at], &session[end..]);

    // Before the repair the misplaced copy is still there: the operator row
    // asking to delete it is open work.
    let (before, _, _) = crate::response_tree_sync::sync_response_trees(&document).unwrap();
    let root = before.find("- **Response [#r-2da6da4a7fd8]**").unwrap();
    let guidance = before[root..]
        .lines()
        .find_map(|line| line.strip_prefix("    - **Guidance:** "))
        .unwrap();
    assert!(
        guidance.contains(
            " done, 1 yours. Only you can finish this: in the editor, delete the misplaced copy"
        ),
        "{guidance}"
    );

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
        .find_map(|line| line.strip_prefix("    - **State:** "))
        .unwrap();
    let guidance = synced[root..]
        .lines()
        .find_map(|line| line.strip_prefix("    - **Guidance:** "))
        .unwrap();
    assert_eq!(state, crate::queue_consume::RESPONSE_STATE_COMPLETE);
    assert!(guidance.starts_with("✅ "), "{guidance}");
    assert!(
        guidance.ends_with(" done. 🗑️ Safe to delete this tree."),
        "{guidance}"
    );
    assert!(crate::response_tree_sync::sync_response_trees(&synced).is_none());
    assert!(self_repair_document(&synced, &retired, &proven).is_none());
}

/// `#treefinish`: an operator cleanup ask is resolved only once the artifact
/// it names is gone.
#[test]
fn a_cleanup_ask_is_done_only_when_its_artifact_is_gone() {
    let ask = "operator: in the editor, delete the misplaced copy: the block from `- **Response [#r-8eb6516470b1]**` through its Guidance line.";
    let misplaced = doc(concat!(
        "- ~~fix the thing~~\n",
        "  - **Response [#r-2da6da4a7fd8]**\n",
        "    - **State:** Complete.\n",
        "    - **Guidance:** Safe to delete.\n",
        "      - **Response [#r-8eb6516470b1]**\n",
        "        - **Summary:** copied hand-back.\n",
    ));
    assert!(!self_repair_artifacts_absent(&misplaced, ask));
    let (repaired, _) =
        self_repair_document(&misplaced, &retired, &|id| id == "[#r-8eb6516470b1]").unwrap();
    assert!(self_repair_artifacts_absent(&repaired, ask));

    let doubled = doc("- ~~amend core-v1~~\n  - kept note\n- ~~amend core-v1~~\n  - other note\n");
    let extra = "operator: delete the extra struck copies of `amend core-v1`.";
    assert!(
        !self_repair_artifacts_absent(&doubled, extra),
        "copies self-repair keeps still need a person"
    );
}

/// `#selfrepaircleanup`: only `subagent:shortitle#1` is still held.
fn only_shortitle_live(owner: &str, generation: u64) -> bool {
    owner == "subagent:shortitle" && generation == 1
}

/// The live session after the first self-repair (anonymized): an earlier
/// label strip read the truncated origin quote's stray backtick as opening a
/// code span, so it removed `[#subagent:effectvoid#1]` from one title's
/// quoted label but left the word (`running amend core-v1`); another title
/// still quotes the whole label; and two struck copies of the leaf differ
/// from it only by the word. The exchange records each title's hand-back
/// bullet and the labelled leaf, so each is restored and the copies
/// collapse; the id-less agent row, the running row, and the response tree's
/// evidence prose keep their text. A second sync is a no-op.
#[test]
fn claim_label_residue_is_restored_from_exchange_provenance() {
    let session = include_str!("../tests/fixtures/claim_label_residue_session.md");
    let (repaired, repairs) =
        self_repair_document(session, &only_shortitle_live, &unproven).unwrap();
    let expected = session
        .replacen(
            "has struck `running amend core-v1` copies",
            "has struck `amend core-v1` copies",
            1,
        )
        .replacen(
            "delete the extra struck copies of `running [#subagent:effectvoid#1] amend core-v1` (there are 3) and any doubled `~~Should we integrate these?~~`; these are real duplicate rows, so the keyed merge still correctly declines on those cuts and uses the fallback merge until cleaned up. [operator-verify]",
            "delete the extra struck copies of `amend core-v1` (there are 3) and any doubled `~~Should we integrate these?~~`; these are real duplicate rows, so the keyed merge still correctly declines on those cuts and uses the fallback merge until cleaned up. [operator-verify]",
            1,
        )
        .replacen(
            "  - ~~amend core-v1~~\n  - ~~running amend core-v1~~\n  - ~~running amend core-v1~~\n",
            "  - ~~amend core-v1~~\n",
            1,
        );
    assert_ne!(expected, session);
    assert_eq!(repaired, expected, "{repairs:#?}");
    let residue = repairs
        .iter()
        .filter(|repair| repair.kind == SelfRepairKind::ClaimLabelResidue)
        .collect::<Vec<_>>();
    assert_eq!(residue.len(), 4, "{repairs:#?}");
    assert!(residue.iter().all(|repair| repair.rewritten.is_some()));
    assert_eq!(
        repairs
            .iter()
            .filter(|repair| repair.kind == SelfRepairKind::DuplicateStruckRow)
            .count(),
        2,
        "{repairs:#?}"
    );
    assert!(
        repairs
            .iter()
            .all(|repair| repair.kind != SelfRepairKind::StaleClaimLabel),
        "labels quoted in code are left to the residue pass: {repairs:#?}"
    );
    // Evidence prose in the response tree and the exchange is untouched.
    for kept in [
        "(`🟡 running [#subagent:effectvoid#1] amend core-v1`, original leaf",
        "the `running [#…]` rows remain",
        "under the first struck `running [#subagent:effectvoid#1] amend core-v1` row, plus the original).\n  - **Response [#r-2da6da4a7fd8]**",
        "- ### 🚧 [#subagent:shortitle#1] `- 🙋 follow up",
        "> - operator: the live `plan.md` has struck `running [#subagent:effectvoid#1] amend core-v1` copies",
    ] {
        assert!(repaired.contains(kept), "{kept}\n{repaired}");
    }
    assert!(
        self_repair_document(&repaired, &only_shortitle_live, &unproven).is_none(),
        "a second sync is a no-op"
    );
}

/// Without an exchange record proving the labelled original, text that
/// reads like residue is the operator's and stays: a row that says
/// `running`, a title whose action item goes on past the recorded bullet,
/// a struck residue copy with no clean sibling, a struck `running` row with
/// no labelled leaf on record, an attempt the ledger still holds, an id-less
/// follow-up, and a row a worker holds.
#[test]
fn similar_operator_text_without_provenance_is_kept() {
    let exchange = concat!(
        "> - operator: the doc has struck `running [#subagent:effectvoid#1] amend core-v1` copies; decide.\n",
        "> - operator: the doc quotes `running [#subagent:held#2] build it`; decide.\n",
        "> The leaf `running [#subagent:effectvoid#1] amend core-v1` was labelled.\n",
    );
    let queue = concat!(
        "- stop the running amend core-v1 job [#q-111111111111]\n",
        "- 🙋 follow up on an action item from queue item “x”: operator: the doc has struck `running amend core-v1` copies; decide. And also keep the running ones. [operator-verify] [#q-222222222222]\n",
        "- ### group\n",
        "  - ~~running amend core-v1~~\n",
        "- ### other\n",
        "  - ~~running tests~~\n",
        "  - ~~tests~~\n",
        "- 🙋 follow up on an action item from queue item “y”: operator: the doc quotes `running build it`; decide. [operator-verify] [#q-333333333333]\n",
        "- follow up on an action item from queue item “z”: operator: the doc has struck `running amend core-v1` copies; decide.\n",
        "- 🚧 [#subagent:worker#4] follow up on an action item from queue item “w”: operator: the doc has struck `running amend core-v1` copies; decide. [#q-444444444444]\n",
    );
    let document = format!(
        "---\nagent_doc_format: template\n---\n\n## Run <!-- agent:run id=\"main\" subagents -->\n{queue}<!-- /agent:run -->\n\n## Exchange <!-- agent:exchange -->\n{exchange}<!-- /agent:exchange -->\n"
    );
    let is_live = |owner: &str, generation: u64| {
        (owner == "subagent:held" && generation == 2)
            || (owner == "subagent:worker" && generation == 4)
    };
    let result = self_repair_document(&document, &is_live, &unproven);
    assert!(
        result.is_none(),
        "{:#?}",
        result.map(|(_, repairs)| repairs)
    );
    // With no exchange at all, even the exact residue shape is kept.
    let (unchanged, repairs) = restore_claim_label_residue(&doc(queue), &retired);
    assert_eq!(unchanged, doc(queue), "{repairs:?}");
}

/// A follow-up title's origin quote can end inside a code span
/// (`“… prompt ` - …”`). Its stray backtick no longer flips the action
/// item's spans, so a label the action item quotes in code is evidence the
/// label pass leaves whole, instead of stripping the label and leaving its
/// lifecycle word behind.
#[test]
fn a_stray_origin_backtick_never_exposes_a_quoted_label() {
    let queue = "- 🙋 follow up on an action item from queue item “agent-doc duplicated my prompt ` - …”: operator: the doc has struck `running [#subagent:effectvoid#1] amend core-v1` copies. [operator-verify] [#q-8eeadf7937c8]\n";
    let document = doc(queue);
    let (repaired, repairs) = strip_stale_claim_labels(&document, &retired);
    assert_eq!(repaired, document, "{repairs:?}");
    // A label in the origin quote outside code still goes, word included.
    let queue = "- 🙋 follow up on an action item from queue item “running [#subagent:effectvoid#1] amend ` - …”: operator: decide. [operator-verify] [#q-8eeadf7937c8]\n";
    let (repaired, repairs) = strip_stale_claim_labels(&doc(queue), &retired);
    assert_eq!(
        repaired,
        doc(
            "- 🙋 follow up on an action item from queue item “amend ` - …”: operator: decide. [operator-verify] [#q-8eeadf7937c8]\n"
        )
    );
    assert_eq!(
        repairs[0]
            .rewritten
            .as_ref()
            .map(|(_, after)| after.as_str()),
        Some(
            "🙋 follow up on an action item from queue item “amend ` - …”: operator: decide. [operator-verify] [#q-8eeadf7937c8]"
        )
    );
}

/// A short-title follow-up row (`[#<origin>] follow-up: <item>`) is restored
/// the same way as a legacy one.
#[test]
fn short_title_followup_residue_is_restored() {
    let queue = "- ### 🙋 [#q-8834c5a41276] follow-up: operator: the doc has struck `running amend core-v1` copies; decide. [operator-verify] [#q-8eeadf7937c8]\n";
    let document = format!(
        "---\nagent_doc_format: template\n---\n\n## Run <!-- agent:run id=\"main\" subagents -->\n{queue}<!-- /agent:run -->\n\n## Exchange <!-- agent:exchange -->\n> - operator: the doc has struck `running [#subagent:effectvoid#1] amend core-v1` copies; decide.\n<!-- /agent:exchange -->\n"
    );
    let (repaired, repairs) = self_repair_document(&document, &retired, &unproven).unwrap();
    assert_eq!(
        repaired,
        document.replace(
            "struck `running amend core-v1` copies; decide. [operator-verify]",
            "struck `amend core-v1` copies; decide. [operator-verify]"
        ),
        "{repairs:?}"
    );
    assert!(self_repair_document(&repaired, &retired, &unproven).is_none());
}

/// `#selfrepaircleanup` migration, on the live tree (anonymized): an earlier
/// retirement kept `~~Present the password prompt and complete.~~` under its
/// `**Closed by:**` backlink at its old depth (12 spaces under a 6-space
/// backlink). Sync moves it, with its response, up to be the backlink's
/// direct child; every line's text after its indentation is unchanged, and a
/// second sync is a no-op.
#[test]
fn rows_kept_too_deep_under_a_closed_by_backlink_are_renested() {
    let session = include_str!("../tests/fixtures/closed_by_nest_session.md");
    let (repaired, repairs) = self_repair_document(session, &retired, &unproven).unwrap();
    assert_eq!(
        repairs,
        vec![SelfRepair {
            kind: SelfRepairKind::MisnestedClosedByRows,
            detail: "[#r-8eb6516470b1]".to_string(),
            rewritten: None,
        }]
    );
    let backlink = "      - **Closed by:** [#r-8eb6516470b1]\n";
    let at = repaired.find(backlink).unwrap() + backlink.len();
    let after: Vec<&str> = repaired[at..].lines().take(8).collect();
    assert_eq!(
        after[0],
        "        - ~~Present the password prompt and complete.~~"
    );
    assert_eq!(after[1], "          - **Response [#r-37bf1620fdbd]**");
    assert!(
        after[2].starts_with("            - **Timestamp:** "),
        "{}",
        after[2]
    );
    assert!(
        after[6].starts_with("            - **Guidance:** "),
        "{}",
        after[6]
    );
    assert!(
        after[7].starts_with("        - 🙋 follow up on an action item"),
        "{}",
        after[7]
    );
    // Only indentation moved: the same lines, the same text after it.
    let stripped = |text: &str| {
        text.lines()
            .map(|line| line.trim_start_matches(' ').to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(stripped(&repaired), stripped(session));
    // Nothing outside the backlink's subtree moved.
    let before_backlink = session.find(backlink).unwrap();
    assert_eq!(repaired[..before_backlink], session[..before_backlink]);
    assert!(self_repair_document(&repaired, &retired, &unproven).is_none());
}

/// `#selfrepaircleanup`: rows already directly under a backlink, and deeper rows
/// that sit one step under their own parent, are left alone; a row two
/// generated levels down keeps its children's relative depth when it moves.
#[test]
fn closed_by_renest_keeps_relative_structure() {
    let queue = concat!(
        "- ### root\n",
        "  - **Response [#r-2da6da4a7fd8]**\n",
        "    - **Guidance:** ✅ Done.\n",
        "      - **Closed by:** [#r-8eb6516470b1]\n",
        "            - ~~answer~~\n",
        "              - nested reply\n",
        "                continuation text\n",
        "                  - deeper reply\n",
        "        - sibling at the right depth\n",
        "          - its child\n",
        "  - **Closed by:** [#r-aaaaaaaaaaaa]\n",
        "    - already direct\n",
        "      - one step down\n",
    );
    let (repaired, repairs) = renest_closed_by_rows(&doc(queue));
    assert_eq!(repairs.len(), 1, "{repairs:?}");
    assert_eq!(
        repaired,
        doc(concat!(
            "- ### root\n",
            "  - **Response [#r-2da6da4a7fd8]**\n",
            "    - **Guidance:** ✅ Done.\n",
            "      - **Closed by:** [#r-8eb6516470b1]\n",
            "        - ~~answer~~\n",
            "          - nested reply\n",
            "            continuation text\n",
            "            - deeper reply\n",
            "        - sibling at the right depth\n",
            "          - its child\n",
            "  - **Closed by:** [#r-aaaaaaaaaaaa]\n",
            "    - already direct\n",
            "      - one step down\n",
        ))
    );
    assert!(renest_closed_by_rows(&repaired).1.is_empty());
    assert_eq!(renest_closed_by_rows(&repaired).0, repaired);
}

/// `#struckidtrail` (agent-doc-bugs.md, 2026-11-23): a retained rebase
/// appended the struck row's own id after its closing `~~`, so the finished
/// row read as live and the pool re-offered it. Self-repair folds the copy,
/// the row reads as struck again, a second run is a no-op, and the sync that
/// follows gives the finished tree its 🗑️ glyph inside the strike.
#[test]
fn a_struck_rows_trailing_id_copy_folds_back_into_the_strike() {
    const WORK: &str = "follow up on an action item from queue item \u{201c}` - **Guidance:** \u{1f64b} 3/5 done, 2 yours. Only you can finish this: 2 items for you\u{2026}\u{201d}: agent: check CI run 1001 once it finishes. [#q-0a1b2c3d4e5f]";
    let queue_doc = |row: &str| {
        format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue subagents -->\n",
                "{}\n",
                "  - **Response [#r-0123456789ab]**\n",
                "    - **Timestamp:** 2026-11-23T02:22:20.000Z\n",
                "    - **Summary:** All named commits passed CI.\n",
                "    - **State:** Complete.\n",
                "    - **Guidance:** \u{1f5d1}\u{fe0f} Safe to delete this tree. Its action items moved to follow-up rows.\n",
                "- [#q-0a1b2c3d4e5f] follow-up: agent: re-check runs 1002 and 1003 once they finish. [#q-5f4e3d2c1b0a]\n",
                "<!-- /agent:queue -->\n",
            ),
            row
        )
    };
    let row_closed = |document: &str| {
        let tree = crate::queue_tree::queue_tree(
            document,
            &crate::queue_tree::QueueTreeOptions::default(),
        )
        .unwrap();
        tree.nodes
            .iter()
            .find(|node| node.depth == 0 && node.full_text.contains("check CI run 1001"))
            .map(|node| node.closed)
            .expect("the origin row is a queue node")
    };
    let damaged = queue_doc(&format!("- ### ~~{WORK}~~ [#q-0a1b2c3d4e5f]"));
    assert!(
        !row_closed(&damaged),
        "the trailing copy makes the row read as live"
    );

    let (repaired, repairs) =
        self_repair_document(&damaged, &retired, &unproven).expect("the copy is folded");
    assert_eq!(repaired, queue_doc(&format!("- ### ~~{WORK}~~")));
    assert_eq!(repairs.len(), 1, "{repairs:?}");
    assert_eq!(repairs[0].kind, SelfRepairKind::StruckRowTrailingId);
    assert!(
        repairs[0]
            .log_fields()
            .starts_with("kind=struck_row_trailing_id detail=")
    );
    assert_eq!(
        repairs[0].rewritten,
        Some((
            format!("### ~~{WORK}~~ [#q-0a1b2c3d4e5f]"),
            format!("### ~~{WORK}~~")
        ))
    );
    assert!(row_closed(&repaired), "the repaired row is struck again");
    assert!(
        self_repair_document(&repaired, &retired, &unproven).is_none(),
        "a second run is a no-op"
    );

    let synced = crate::response_tree_sync::sync_response_trees_striking(&repaired, &|_| false)
        .map(|sync| sync.document)
        .unwrap_or_else(|| repaired.clone());
    assert!(
        synced.contains(&format!("- ### ~~\u{1f5d1}\u{fe0f} {WORK}~~\n")),
        "the finished tree's row carries the delete glyph inside its strike:\n{synced}"
    );
    let resynced = self_repair_document(&synced, &retired, &unproven)
        .map(|(document, _)| document)
        .unwrap_or_else(|| synced.clone());
    let resynced = crate::response_tree_sync::sync_response_trees_striking(&resynced, &|_| false)
        .map(|sync| sync.document)
        .unwrap_or(resynced);
    assert_eq!(resynced, synced, "a second repair + sync is a no-op");
}

/// `#struckidtrail`: only an exact repeat of the strike's own id is folded.
#[test]
fn struck_row_trailing_text_that_is_not_the_same_id_is_kept() {
    let queue = concat!(
        "- ~~check it now [#q-0a1b2c3d4e5f]~~ [#q-ffffffffffff]\n",
        "- ~~check it now [#q-0a1b2c3d4e5f]~~ see [#q-0a1b2c3d4e5f]\n",
        "- live row [#q-0a1b2c3d4e5f] [#q-0a1b2c3d4e5f]\n",
        "- ~~check it later~~ [#q-0a1b2c3d4e5f]\n",
    );
    let document = format!("<!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n");
    let (repaired, repairs) = fold_struck_row_trailing_ids(&document);
    assert!(repairs.is_empty(), "{repairs:?}");
    assert_eq!(repaired, document);
    let doubled = document.replace(
        "- ~~check it later~~ [#q-0a1b2c3d4e5f]",
        "  - ~~check it later [#q-0a1b2c3d4e5f]~~ [#q-0a1b2c3d4e5f]",
    );
    let (repaired, repairs) = fold_struck_row_trailing_ids(&doubled);
    assert_eq!(repairs.len(), 1, "a nested row folds too: {repairs:?}");
    assert!(
        repaired.contains("  - ~~check it later [#q-0a1b2c3d4e5f]~~\n"),
        "{repaired}"
    );
}

/// `#selfrepaircleanup`: the PR-tracking queue a retained rebase corrupted on
/// 2026-11-23. The claim projection marked every copy of the operator's
/// `#rebase` row, so one attempt's row read several times, back to back and
/// under a copied PR heading, beside doubled operator rows. Exact copies
/// collapse to one; the copy holding a response stays; every row whose text
/// differs, and the operator's own repeated headings, stay.
#[test]
fn dupsdk_duplicate_claim_and_sibling_rows_collapse_keeping_the_copy_with_children() {
    const CLAIM_380: &str = "  - 🚧 [#subagent:rebase380#1] #rebase\n";
    const CLAIM_377: &str = "  - 🚧 [#subagent:rebase377#1] #rebase\n";
    const RESPONSE: &str = concat!(
        "    - **Response [#r-0a1b2c3d4e5f]**\n",
        "      - **Summary:** Rebased onto sdk-dev.\n",
    );
    let damaged = doc(&format!(
        concat!(
            "- ### https://github.com/acme/widget-sdk/pull/377\n",
            "  - %% docs(sdk): add quickstarts to every README %%\n",
            "{claim377}",
            "{claim377}",
            "  - 📥 squash and merge\n",
            "  - 📥 squash and me\n",
            "- ### https://github.com/acme/widget-sdk/pull/378\n",
            "  - %% feat(sdk): port the credential lifecycle %%\n",
            "- ### 🔒 https://github.com/acme/widget-sdk/pull/379\n",
            "  - 📥 squash and merge\n",
            "  - 📥 squash and merge\n",
            "- ### https://github.com/acme/widget-sdk/pull/380\n",
            "  - %% test(conformance): prove the credential lifecycle %%\n",
            "{claim380}",
            "{claim380}",
            "  - squash and merge\n",
            "- ### https://github.com/acme/widget-sdk/pull/378\n",
            "  - %% feat(sdk): port the credential lifecycle %%\n",
            "- ### https://github.com/acme/widget-sdk/pull/380\n",
            "  - %% test(conformance): prove the credential lifecycle %%\n",
            "{claim380}",
            "{response}",
            "  - 📥 #rebase\n",
            "- ### https://github.com/acme/widget-sdk/pull/383\n",
            "  - %% chore(license): ship the license %%\n",
            "  - 📥 #adversarial-pr\n",
            "  - 📥 #rebase\n",
            "  - 📥 #adversarial-pr\n",
        ),
        claim377 = CLAIM_377,
        claim380 = CLAIM_380,
        response = RESPONSE,
    ));
    let (repaired, repairs) =
        self_repair_document(&damaged, &retired, &unproven).expect("the copies collapse");
    assert_eq!(
        repaired,
        doc(&format!(
            concat!(
                "- ### https://github.com/acme/widget-sdk/pull/377\n",
                "  - %% docs(sdk): add quickstarts to every README %%\n",
                "{claim377}",
                "  - 📥 squash and merge\n",
                "  - 📥 squash and me\n",
                "- ### https://github.com/acme/widget-sdk/pull/378\n",
                "  - %% feat(sdk): port the credential lifecycle %%\n",
                "- ### 🔒 https://github.com/acme/widget-sdk/pull/379\n",
                "  - 📥 squash and merge\n",
                "- ### https://github.com/acme/widget-sdk/pull/380\n",
                "  - %% test(conformance): prove the credential lifecycle %%\n",
                "  - squash and merge\n",
                "- ### https://github.com/acme/widget-sdk/pull/378\n",
                "  - %% feat(sdk): port the credential lifecycle %%\n",
                "- ### https://github.com/acme/widget-sdk/pull/380\n",
                "  - %% test(conformance): prove the credential lifecycle %%\n",
                "{claim380}",
                "{response}",
                "  - 📥 #rebase\n",
                "- ### https://github.com/acme/widget-sdk/pull/383\n",
                "  - %% chore(license): ship the license %%\n",
                "  - 📥 #adversarial-pr\n",
                "  - 📥 #rebase\n",
            ),
            claim377 = CLAIM_377,
            claim380 = CLAIM_380,
            response = RESPONSE,
        )),
    );
    let kinds = |kind: SelfRepairKind| repairs.iter().filter(|r| r.kind == kind).count();
    assert_eq!(kinds(SelfRepairKind::DuplicateClaimRow), 3, "{repairs:?}");
    assert_eq!(kinds(SelfRepairKind::DuplicateSiblingRow), 2, "{repairs:?}");
    assert_eq!(repairs.len(), 5, "{repairs:?}");
    assert!(repairs.iter().any(|repair| repair.log_fields()
        == "kind=duplicate_claim_row detail=\"🚧 [#subagent:rebase380#1] #rebase\""));
    assert!(
        self_repair_document(&repaired, &retired, &unproven).is_none(),
        "a second run is a no-op"
    );
    assert!(self_repair_artifacts_absent(
        &repaired,
        "operator: delete the duplicate `[#subagent:rebase380#1]` row"
    ));
    assert!(!self_repair_artifacts_absent(
        &damaged,
        "operator: delete the duplicate `[#subagent:rebase380#1]` row"
    ));
}

/// `#selfrepaircleanup`: copies whose subtrees differ are both operator text, and a
/// claim row with a different attempt or text is a different row.
#[test]
fn dupsdk_rows_that_differ_or_hold_different_children_are_kept() {
    let queue = concat!(
        "- ### https://github.com/acme/widget-sdk/pull/380\n",
        "  - 🚧 [#subagent:rebase380#1] #rebase\n",
        "    - keep the history linear\n",
        "  - 🚧 [#subagent:rebase380#1] #rebase\n",
        "    - squash first\n",
        "  - 🚧 [#subagent:rebase380#2] #rebase\n",
        "  - 🟡 [#subagent:rebase380#1] #rebase\n",
        "  - squash and merge\n",
        "    - after CI\n",
        "  - squash and merge\n",
        "    - before CI\n",
        "  - squash and merge \n",
        "- squash and merge\n",
        "- squash and merge\n",
    );
    let document = doc(queue);
    let (repaired, repairs) = dedupe_live_rows(&document);
    assert!(repairs.is_empty(), "{repairs:?}");
    assert_eq!(repaired, document);
    assert!(self_repair_document(&document, &retired, &unproven).is_none());
}

/// `#selfrepaircleanup`: the retained rebase repeated whole PR blocks of the
/// queue section. A block that repeats an earlier one byte for byte and
/// carries work (a claim, operator rows) collapses to its first copy; a
/// heading whose only child is its title comment is the operator's own list
/// entry (the last known-good queue repeats those) and stays; a block that
/// differs by one row stays, and its claim copy collapses into the first.
#[test]
fn duplicated_queue_blocks_collapse_and_title_only_entries_stay() {
    const PR378: &str = concat!(
        "- ### https://github.com/acme/widget-sdk/pull/378\n",
        "  - %% feat(sdk): port the credential lifecycle %%\n",
    );
    const PR380: &str = concat!(
        "- ### https://github.com/acme/widget-sdk/pull/380\n",
        "  - %% test(conformance): prove the credential lifecycle %%\n",
        "  - 🚧 [#subagent:rebase380#1] #rebase\n",
    );
    const PR381: &str = concat!(
        "- ### 🔒 https://github.com/acme/widget-sdk/pull/381\n",
        "  - %% chore(contracts): advance the pin %%\n",
        "  - 📥 #rebase\n",
        "  - 📥 #adversarial-pr\n",
    );
    let damaged = doc(&format!(
        "{PR378}{PR380}{PR381}{PR378}{PR380}{PR381}{PR378}{PR380}  - squash and merge\n{PR381}"
    ));
    let (repaired, repairs) =
        self_repair_document(&damaged, &retired, &unproven).expect("the copies collapse");
    assert_eq!(
        repaired,
        doc(&format!(
            "{PR378}{PR380}{PR381}{PR378}{PR378}{}  - squash and merge\n",
            PR380.replace("  - 🚧 [#subagent:rebase380#1] #rebase\n", ""),
        )),
    );
    let kinds = |kind: SelfRepairKind| repairs.iter().filter(|r| r.kind == kind).count();
    assert_eq!(kinds(SelfRepairKind::DuplicateQueueBlock), 3, "{repairs:?}");
    assert_eq!(kinds(SelfRepairKind::DuplicateClaimRow), 1, "{repairs:?}");
    assert!(repairs.iter().any(|repair| {
        repair
            .log_fields()
            .starts_with("kind=duplicate_queue_block detail=")
    }));
    assert!(self_repair_document(&repaired, &retired, &unproven).is_none());
}
