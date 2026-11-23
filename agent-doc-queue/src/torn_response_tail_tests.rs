//! Tests for [`super`] (`#tornsplice`).

use super::*;

/// The live shape after the torn splice, then the operator's answer: under
/// the hoisted `🙋 operator: reload …` Remaining entry, the column-0 tail
/// `ing on operator input.` and a copy of the nested response's Guidance,
/// with the operator's struck answer (and its Complete response) nested
/// under that copy.
const TORN: &str = include_str!("../tests/fixtures/response_remaining_torn_tail.md");

/// The splice's first output, before any answer: the same tail, and the
/// copied Guidance at the nested response's own detail indent.
const NESTED: &str = include_str!("../tests/fixtures/response_remaining_nested_answer_splice.md");

const ENTRY: &str =
    "      - 🙋 operator: reload any open VSCodium windows <!-- agent-doc:response-detail -->\n";
const TAIL: &str = "ing on operator input.\n";
/// The tail as its own line (`Waiting on operator input.` contains [`TAIL`]).
const TAIL_LINE: &str = "\ning on operator input.\n";
const COPIED_GUIDANCE: &str = "- **Guidance:** 🙋 1/2 done, 1 yours. Only you can finish this: reload any open VSCodium windows. Strike or answer its row when done.\n";

fn kinds(repairs: &[SelfRepair]) -> Vec<SelfRepairKind> {
    repairs.iter().map(|repair| repair.kind).collect()
}

#[test]
fn drops_the_torn_tail_and_moves_the_answer_up_under_its_entry() {
    let (repaired, repairs) = drop_torn_response_tails(TORN);
    assert_eq!(
        kinds(&repairs),
        vec![
            SelfRepairKind::TornResponseTail,
            SelfRepairKind::DuplicateEntryGuidance
        ]
    );
    assert_eq!(
        repairs[0].log_fields(),
        "kind=torn_response_tail detail=\"ing on operator input.\""
    );
    assert!(!repaired.contains(TAIL_LINE), "{repaired}");
    assert_eq!(
        repaired
            .matches(&format!("\n        {COPIED_GUIDANCE}"))
            .count(),
        0,
        "{repaired}"
    );
    // The nested response's own Guidance stays.
    assert_eq!(
        repaired
            .matches(&format!("\n            {COPIED_GUIDANCE}"))
            .count(),
        1
    );
    // The operator's answer is now the entry's direct answer, text unchanged.
    let answer = format!(
        "{ENTRY}        - ~~VSCodium not running. [#q-c4b5011d0725]~~\n          - **Response [#r-91a2943e8ec0]**\n"
    );
    assert!(repaired.contains(&answer), "{repaired}");
    // Only the two lines and the answer subtree's leading spaces changed.
    let expected = TORN
        .replace(
            &format!("{ENTRY}{TAIL}        {COPIED_GUIDANCE}"),
            ENTRY,
        )
        .replace(
            "          - ~~VSCodium not running.",
            "        - ~~VSCodium not running.",
        )
        .replace(
            "            - **Response [#r-91a2943e8ec0]**\n              - **Timestamp:** 2026-11-23T15:19:54.000Z\n",
            "          - **Response [#r-91a2943e8ec0]**\n            - **Timestamp:** 2026-11-23T15:19:54.000Z\n",
        )
        .replace(
            "              - **Summary:** No VSCodium",
            "            - **Summary:** No VSCodium",
        )
        .replace(
            "              - **State:** Complete.\n              - **Guidance:** ✅ Done.\n    - **State:** Waiting",
            "            - **State:** Complete.\n            - **Guidance:** ✅ Done.\n    - **State:** Waiting",
        );
    assert_eq!(repaired, expected);

    let (again, none) = drop_torn_response_tails(&repaired);
    assert!(none.is_empty(), "{none:?}");
    assert_eq!(again, repaired, "a second run is a no-op");
}

/// Self-repair then the tree sync settle the tree: the answered entry is
/// done and nothing torn is left. The answer also resolves the nested copy
/// of its entry (`#answeredentry`), so the whole tree is finished and the
/// root, not the answer row, carries the 🗑️ glyph.
#[test]
fn the_repaired_tree_syncs_its_answered_entry() {
    let (repaired, repairs) =
        crate::self_repair::self_repair_document(TORN, &|_, _| false, &|_| true)
            .expect("the torn tail is repaired");
    assert!(
        repairs
            .iter()
            .any(|repair| repair.kind == SelfRepairKind::TornResponseTail)
    );
    let synced = crate::response_tree_sync::sync_response_trees(&repaired)
        .map_or(repaired, |(document, _, _)| document);
    assert!(!synced.contains(TAIL_LINE), "{synced}");
    assert!(
        synced.contains(
            "      - ✅ operator: reload any open VSCodium windows <!-- agent-doc:response-detail -->\n        - ~~VSCodium not running."
        ),
        "{synced}"
    );
    assert!(synced.contains("- ### ~~🗑️ [#q-e9483473162d]"), "{synced}");
    assert!(!synced.contains("🙋"), "{synced}");
    assert_eq!(
        crate::response_tree_sync::sync_response_trees(&synced),
        None
    );
}

/// The first torn output: the copied Guidance sat at the nested response's
/// detail indent, with nothing under it.
#[test]
fn drops_a_torn_tail_and_its_deeper_guidance_copy() {
    let (clean, _) = crate::response_remaining::reconcile_response_remaining(NESTED)
        .expect("the nested tree re-renders");
    // The outer entry: the nested response's own entry is deeper.
    let torn = clean.replacen(
        &format!("\n{ENTRY}"),
        &format!("\n{ENTRY}{TAIL}            {COPIED_GUIDANCE}"),
        1,
    );
    assert_ne!(torn, clean);
    let (repaired, repairs) = drop_torn_response_tails(&torn);
    assert_eq!(
        kinds(&repairs),
        vec![
            SelfRepairKind::TornResponseTail,
            SelfRepairKind::DuplicateEntryGuidance
        ]
    );
    assert_eq!(repaired, clean);
}

#[test]
fn leaves_operator_text_and_a_unique_guidance_alone() {
    // Operator prose after a detail row that is no generated line's tail.
    let prose = TORN.replacen(
        &format!("{ENTRY}{TAIL}"),
        &format!("{ENTRY}see the release notes first\n"),
        1,
    );
    let (_, repairs) = drop_torn_response_tails(&prose);
    assert!(
        !repairs
            .iter()
            .any(|repair| repair.kind == SelfRepairKind::TornResponseTail),
        "{repairs:?}"
    );
    // A Guidance under an entry no other Guidance line repeats is not a
    // copy, unless it is a response tally (`#answeredentry`): only a
    // response carries one, so a tally under a Remaining entry is a copy
    // even once the response it came from re-rendered its own.
    let unique = TORN.replacen(
        &format!("{TAIL}        {COPIED_GUIDANCE}"),
        "        - **Guidance:** 🗑️ Safe to delete this tree.\n",
        1,
    );
    let (kept, repairs) = drop_torn_response_tails(&unique);
    assert!(repairs.is_empty(), "{repairs:?}");
    assert_eq!(kept, unique);
    let unique_tally = TORN.replacen(
        &format!("{TAIL}        {COPIED_GUIDANCE}"),
        "        - **Guidance:** ✅ 9/9 done.\n",
        1,
    );
    let (_, repairs) = drop_torn_response_tails(&unique_tally);
    assert_eq!(
        kinds(&repairs),
        vec![SelfRepairKind::DuplicateEntryGuidance],
        "{repairs:?}"
    );
    // A tail inside fenced code is code, not a tear.
    let fenced = format!("```\n{ENTRY}{TAIL}```\n");
    let document = TORN.replacen(
        "<!-- /agent:run -->",
        &format!("{fenced}<!-- /agent:run -->"),
        1,
    );
    let (repaired, _) = drop_torn_response_tails(&document);
    assert!(repaired.contains(&fenced));
}
