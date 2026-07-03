//! Phase B red-first acceptance test (goal 3: fix CRDT scrambling).
//!
//! Reproduces a REAL, production-path CRDT scramble: when the agent and the
//! operator concurrently add disjoint content to the same `exchange` component,
//! the component-isolated merge (`merge_contents_crdt` → `merge_frontmatter_aware`
//! → `merge_by_component`) falls back to the whole-document yrs merge on the
//! same-component content divergence (`crdt.rs`, audit mechanism #5), which
//! splices ACROSS the component boundary — producing a structurally-corrupt
//! document with a DUPLICATE `<!-- /agent:exchange -->` close marker and the
//! operator's content orphaned OUTSIDE the component.
//!
//! Observed today (2026-07-03): `merge_contents_crdt` yields two
//! `<!-- /agent:exchange -->` markers. The production write path's
//! `normalize_template_structure_or_fail` then REJECTS this mixed-content
//! duplicate (fail-closed safety net) rather than repairing it — so the cycle
//! fails instead of committing corruption, but the merge is still wrong.
//!
//! The fix (Phase B, `plan-crdt-scramble-and-disk-propagation.md`): keep the
//! same-component content divergence inside `merge_by_component`'s per-cell path
//! (state-vector / per-cell reconcile) instead of collapsing to the whole-doc
//! merge, so both contributions land INSIDE the single `exchange` component.
//!
//! This test is `#[ignore]`d because it asserts the POST-FIX contract and is
//! expected to fail until Phase B lands — un-ignore it as the fix's acceptance
//! gate. (Ignored so it does not red `make check` before the fix.)

use agent_doc_merge::crdt::CrdtDoc;

fn tmpl(exchange: &str) -> String {
    format!(
        "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n{exchange}\n<!-- /agent:exchange -->\n"
    )
}

#[test]
fn concurrent_same_component_edits_stay_inside_one_component() {
    let base = tmpl("Q1.");
    let base_state = CrdtDoc::from_text(&base).encode_state();

    let ours = tmpl(
        "Q1.\n\n### Re: Q1\n\nAAAA AAAA AAAA AAAA AAAA AAAA AAAA AAAA AAAA AAAA (agent response).",
    );
    let theirs = tmpl(
        "Q1.\n\nBBBB BBBB BBBB BBBB BBBB BBBB BBBB BBBB BBBB BBBB (operator added a long note).",
    );

    let (merged, _state) =
        agent_doc_merge::merge_contents_crdt(Some(&base_state), &ours, &theirs).unwrap();

    // Post-fix contract: exactly one exchange component, both contributions
    // inside it, no orphaned content, no duplicate close marker.
    assert_eq!(
        merged.matches("<!-- /agent:exchange -->").count(),
        1,
        "merge must not duplicate the component close marker (cross-splice scramble):\n{merged}"
    );
    assert_eq!(
        merged.matches("<!-- agent:exchange -->").count(),
        1,
        "merge must not duplicate the component open marker:\n{merged}"
    );
    assert!(merged.contains("agent response"), "agent content lost:\n{merged}");
    assert!(
        merged.contains("operator added a long note"),
        "operator content lost:\n{merged}"
    );
    // The operator's content must be INSIDE the component (before the close),
    // not orphaned after it.
    let close = merged.find("<!-- /agent:exchange -->").unwrap();
    let op = merged.find("operator added a long note").unwrap();
    assert!(
        op < close,
        "operator content orphaned outside the component:\n{merged}"
    );
}

#[test]
fn divergent_component_sets_merge_by_union_not_whole_doc_splice() {
    // Structural divergence (component set differs): the agent edits `exchange`
    // while the operator concurrently ADDS a whole `queue` component. This must
    // align by name-union (merge exchange, keep the added queue) with valid
    // framing — NOT drop to the whole-doc merge that cross-splices.
    let base = "<!-- agent:exchange -->\nQ.\n<!-- /agent:exchange -->\n";
    let base_state = CrdtDoc::from_text(base).encode_state();

    let ours = "<!-- agent:exchange -->\nQ.\n\n### Re: Q\n\nA (agent).\n<!-- /agent:exchange -->\n";
    let theirs = "<!-- agent:exchange -->\nQ.\n<!-- /agent:exchange -->\n\
<!-- agent:queue -->\n- do [#op1] operator added this\n<!-- /agent:queue -->\n";

    let (merged, _state) =
        agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();

    // Valid framing: exactly one open/close per component, both components present.
    assert_eq!(merged.matches("<!-- agent:exchange -->").count(), 1, "{merged}");
    assert_eq!(merged.matches("<!-- /agent:exchange -->").count(), 1, "{merged}");
    assert_eq!(merged.matches("<!-- agent:queue -->").count(), 1, "{merged}");
    assert_eq!(merged.matches("<!-- /agent:queue -->").count(), 1, "{merged}");
    // Both sides' contributions survive.
    assert!(merged.contains("A (agent)."), "agent edit lost:\n{merged}");
    assert!(
        merged.contains("operator added this"),
        "operator-added component lost:\n{merged}"
    );
}
