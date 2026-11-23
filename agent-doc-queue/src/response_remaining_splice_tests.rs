//! `#tornsplice`: a response's Remaining list holds an answered entry whose
//! answer carries its own response; both responses re-render in one pass.

use super::*;

/// The live shape, before the tear: a step-two tree whose response lists two
/// agent entries, each answered by a struck `#justdoit` that carries its own
/// response; the second one's response (a package refresh) still says
/// `Follow-up work remains.` for its one operator action item.
const NESTED: &str = include_str!("../tests/fixtures/response_remaining_nested_answer_splice.md");

/// Every non-blank line between the queue markers is a list row: a torn
/// splice leaves a bare tail fragment (`ing on operator input.`) that is a
/// lazy continuation of the row above it.
fn assert_rows_only(document: &str) {
    let start = document.find("<!-- agent:run").expect("queue open");
    let end = document.find("<!-- /agent:run -->").expect("queue close");
    for line in document[start..end].lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        assert!(
            line.trim_start().starts_with("- "),
            "torn line {line:?} in\n{document}"
        );
    }
}

#[test]
fn nested_answer_response_and_outer_remaining_render_in_one_pass_without_tearing() {
    let (synced, _) = reconcile_response_remaining(NESTED).expect("both responses re-render");
    assert_rows_only(&synced);
    // Each Guidance appears once per response: no duplicated tail.
    assert_eq!(
        synced
            .matches("- **Guidance:** 🙋 1/2 done, 1 yours.")
            .count(),
        1,
        "{synced}"
    );
    assert_eq!(
        reconcile_response_remaining(&synced),
        None,
        "re-sync is a no-op:\n{synced}"
    );
}

#[test]
fn the_full_tree_sync_never_tears_a_nested_answer_response() {
    let synced = crate::response_tree_sync::sync_response_trees(NESTED)
        .map_or_else(|| NESTED.to_string(), |(document, _, _)| document);
    assert_rows_only(&synced);
    assert_eq!(
        crate::response_tree_sync::sync_response_trees(&synced),
        None
    );
}
