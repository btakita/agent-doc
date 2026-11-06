//! `#queuelineageparent`: a `re [#id]` reference row (for example the
//! `- ### re [#decisionidstamp] response` heading an operator types above
//! replies) is the lineage parent of the rows nested under it.
//!
//! Before this, the reference row parsed as inert `Freeform` with no list
//! depth, so ancestry walks skipped it. The replies were claimed under the
//! previous sibling (`fix "Open: …": <reply>`), that sibling became a context
//! group parent of rows it never owned, and the brief named the wrong context.

use super::*;

/// Redacted queue shape of agent-doc-bugs.md (2026-11-05).
const REPLY_UNDER_REFERENCE: &str = concat!(
    "- ### fix \"Open: a future caller passing a fragment to parse would misread its last line.\"\n",
    "- ### re [#decisionidstamp] response\n",
    "  - 📥 \"Waiting-on-you heads are not stamped directly.\"\n",
    "  - Waiting-on-you heads are not stamped directly.\n",
    "    - 📥 I like option 1 + 2 as lookups. Does preflight --probe present a search?\n",
);

const FIX: usize = 0;
const REFERENCE: usize = 1;
const QUOTED_REPLY: usize = 2;
const REPLY_PARENT: usize = 3;
const NESTED_REPLY: usize = 4;

const REFERENCE_LABEL: &str = "re [#decisionidstamp] response";

fn entries() -> Vec<QueueEntry> {
    let entries = parse(REPLY_UNDER_REFERENCE).unwrap();
    assert_eq!(entries.len(), 5, "{entries:?}");
    assert!(
        matches!(entries[REFERENCE], QueueEntry::Freeform(_)),
        "a reference row is never executable: {:?}",
        entries[REFERENCE]
    );
    entries
}

#[test]
fn reference_heading_is_the_lineage_parent_of_its_rows() {
    let entries = entries();
    assert_eq!(
        lineage_claim_text(&entries, QUOTED_REPLY).as_deref(),
        Some("re [#decisionidstamp] response: \"Waiting-on-you heads are not stamped directly.\"")
    );
    assert_eq!(
        lineage_claim_text(&entries, NESTED_REPLY).as_deref(),
        Some(
            "re [#decisionidstamp] response: Waiting-on-you heads are not stamped directly.: \
             I like option 1 + 2 as lookups. Does preflight --probe present a search?"
        )
    );
    for index in [QUOTED_REPLY, REPLY_PARENT, NESTED_REPLY] {
        let claim = lineage_claim_text(&entries, index).unwrap();
        assert!(
            !claim.contains("fix \"Open:"),
            "the preceding sibling is not an ancestor of row {index}: {claim}"
        );
        assert!(entry_is_ancestor_of(&entries, REFERENCE, index));
        assert!(!entry_is_ancestor_of(&entries, FIX, index));
    }
}

#[test]
fn reference_heading_closes_the_preceding_sibling_subtree() {
    let entries = entries();
    // The `fix` row has no children of its own: it stays a runnable leaf
    // instead of becoming a context group over the reply rows.
    assert!(!entry_is_group_parent(&entries, FIX));
    assert!(entry_is_group_parent(&entries, REPLY_PARENT));
    assert!(!entry_is_group_parent(&entries, QUOTED_REPLY));
    assert!(!entry_is_group_parent(&entries, NESTED_REPLY));
    assert_eq!(
        submitted_prompt_at(&entries, FIX).as_deref(),
        Some(
            "fix \"Open: a future caller passing a fragment to parse would misread its last line.\""
        )
    );
}

#[test]
fn submitted_reply_names_the_reference_heading_as_context() {
    let entries = entries();
    let submitted = submitted_prompt_at(&entries, NESTED_REPLY).unwrap();
    assert!(
        submitted.starts_with(&format!(
            "{REFERENCE_LABEL}: Waiting-on-you heads are not stamped directly.: I like option"
        )),
        "{submitted}"
    );
    assert!(
        submitted.contains(&format!("> - {REFERENCE_LABEL}\n")),
        "the prompt tree shows the reference heading: {submitted}"
    );

    let packet = prompt_context_packet_at(&entries, NESTED_REPLY).unwrap();
    assert_eq!(
        packet.origin.item_text,
        "Waiting-on-you heads are not stamped directly."
    );
    assert!(
        !packet
            .decision_evidence
            .iter()
            .any(|evidence| evidence.contains("fix \"Open:")),
        "{packet:?}"
    );
}

#[test]
fn nested_reference_annotation_leaves_its_parent_runnable() {
    let entries = parse("- do [#alpha]\n  - re [#beta] see also\n- do [#gamma]\n").unwrap();
    assert!(matches!(entries[1], QueueEntry::Freeform(_)));
    assert!(!entry_is_group_parent(&entries, 0));
    assert_eq!(
        submitted_prompt_at(&entries, 0).as_deref(),
        Some("do [#alpha]")
    );
    assert_eq!(
        lineage_claim_text(&entries, 2).as_deref(),
        Some("do [#gamma]")
    );
}

#[test]
fn nested_reference_with_children_is_their_parent() {
    let entries = parse(concat!(
        "- epic\n",
        "  - re [#beta] earlier answer\n",
        "    - follow the answer\n",
    ))
    .unwrap();
    assert!(entry_is_group_parent(&entries, 0));
    assert_eq!(
        lineage_claim_text(&entries, 2).as_deref(),
        Some("epic: re [#beta] earlier answer: follow the answer")
    );
}

#[test]
fn reference_annotation_does_not_break_an_ordered_sequence() {
    let entries = parse(concat!(
        "- group\n",
        "  1. do [#first]\n",
        "  2. re [#first] note\n",
        "  3. do [#second]\n",
    ))
    .unwrap();
    assert!(matches!(entries[2], QueueEntry::Freeform(_)));
    let deps = ordered_list_after_deps(&entries);
    assert_eq!(
        deps.get("second").map(Vec::as_slice),
        Some(&["first".to_string()][..]),
        "{deps:?}"
    );
}
