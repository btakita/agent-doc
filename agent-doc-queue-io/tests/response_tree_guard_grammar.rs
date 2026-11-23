//! Every response tree the queue renders is evidence to the archive orphan
//! guard.
//!
//! The renderer (`agent-doc-queue`: strike hand-back trees, split long prose,
//! sync Remaining/State/Guidance) and the guard's projection parser
//! (`agent-doc-archive-io`) are separate code. Twice on 2026-11-22 a new
//! rendered shape (nested trees, then split prose) was rejected by the parser,
//! so a present tree counted as absent and every closeout refused with
//! "response index would orphan referenced response id(s)". These tests
//! render each generated shape through the real renderer and require that
//! every response id the guard counts as a reference is a present projection.

use std::collections::BTreeSet;

use agent_doc_archive_io::{extract_response_reference_ids, present_inline_response_ids};

const AT: &str = "2026-11-22T23:06:45.000Z";

fn session(leaf: &str, sibling: &str, exchange: &str) -> String {
    format!(
        "---\nqueue_active: true\n---\n\n\
         <!-- agent:exchange -->\n{exchange}\n<!-- /agent:exchange -->\n\n\
         <!-- agent:run id=\"bugs\" subagents -->\n\
         - Parent row\n\
         \x20 - Nested parent\n\
         \x20   - {leaf}\n\
         - {sibling}\n\
         <!-- /agent:run -->\n"
    )
}

const LONG_SUMMARY: &str = "The tool now ships one combined package for every supported build. \
It replaces the two separate packages that each covered one build range. \
It is pushed to `main` as `3b3561a1b1`, and the verification receipts match.";

const LONG_NOTES: &str = "The first note is long enough that the renderer splits it into sentences. \
The second note covers the remaining compatibility gap for older builds. \
The third note says `a. B` stays whole.";

fn handback(summary: &str, action_items: &str) -> String {
    format!(
        "**Summary**\n\n{summary}\n\n\
         **What changed**\n\n- `src/a.rs`: the change.\n- `src/b.rs`: its test.\n\n\
         **Verification**\n\n- `make check`: passed\n\n\
         **Action items**\n\n{action_items}\n\n\
         **Notes**\n\n{LONG_NOTES}\n\n\
         **Next actions**\n\nnone\n"
    )
}

/// Render `handback` onto a struck row, with its completion cell in the
/// document (slim tree) or only in the response body (full tree), then run
/// the tree sync that rewrites Remaining/State/Guidance.
fn rendered_shapes(summary: &str, action_items: &str) -> Vec<(String, String)> {
    let work = "Ship the combined package. [#q-0123456789ab]";
    let struck = format!("~~{work}~~");
    let sibling = "do [#install]";
    let cell = agent_doc_document::subagent_completion::cell_with_response(
        "subagent:sample",
        &format!("Parent row: Nested parent: {work}"),
        "sample-key",
        &handback(summary, action_items),
    )
    .unwrap();
    let mut shapes = Vec::new();
    for (label, exchange, response_body) in
        [("slim", cell.as_str(), ""), ("full", "", cell.as_str())]
    {
        let before = session(work, sibling, exchange);
        let after = session(&struck, sibling, exchange);
        let projected = agent_doc_queue::queue_consume::attach_stopped_row_responses(
            &before,
            &after,
            response_body,
            AT,
        )
        .unwrap();
        let synced = agent_doc_queue::response_tree_sync::sync_response_trees(&projected)
            .map_or_else(|| projected.clone(), |(synced, _, _)| synced);
        shapes.push((format!("{label} strike"), projected));
        shapes.push((format!("{label} synced"), synced));
    }
    shapes
}

fn assert_every_reference_is_present(label: &str, document: &str) {
    let refs = extract_response_reference_ids(document);
    assert!(
        !refs.is_empty(),
        "{label}: the renderer wrote no response tree\n{document}"
    );
    let present = present_inline_response_ids(document);
    let missing = refs.difference(&present).cloned().collect::<BTreeSet<_>>();
    assert!(
        missing.is_empty(),
        "{label}: the guard parser rejects rendered tree(s) {missing:?}\n{document}"
    );
}

#[test]
fn split_prose_trees_are_present_projections() {
    let mut split_seen = false;
    for (label, document) in rendered_shapes(LONG_SUMMARY, "none") {
        split_seen |= document.contains(&format!(
            "- It replaces the two separate packages that each covered one build range. {}",
            agent_doc_markdown_ast::overlay::RESPONSE_DETAIL_MARKER
        ));
        assert_every_reference_is_present(&label, &document);
    }
    assert!(split_seen, "the long summary rendered split at least once");
}

#[test]
fn trees_with_open_action_items_and_remaining_are_present_projections() {
    let action_items = "- agent: run the single install and confirm the version.\n\
                        - operator: restart the IDE after that install.";
    let mut remaining_seen = false;
    for (label, document) in rendered_shapes(LONG_SUMMARY, action_items) {
        remaining_seen |= document.contains("**Remaining:**");
        assert_every_reference_is_present(&label, &document);
    }
    assert!(remaining_seen, "the tree sync rendered a Remaining list");
}

#[test]
fn short_and_multi_paragraph_summaries_are_present_projections() {
    for summary in [
        "Done.",
        "The first paragraph of a long summary.\n\nWhy it matters:\n\n- One reason.\n- Another reason.",
    ] {
        for (label, document) in rendered_shapes(summary, "none") {
            assert_every_reference_is_present(&label, &document);
        }
    }
}
