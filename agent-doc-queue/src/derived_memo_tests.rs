use super::*;
use crate::incremental_corpus::{FIXTURES, Rng, fixture, random_edit, session_shaped};

fn whole(document: &str) -> Option<Vec<Item>> {
    agent_doc_markdown_ast::mutations::item_nodes_all(document, "queue")
        .ok()
        .map(|nodes| nodes.into_iter().map(|node| node.item).collect())
}

fn scoped(document: &str) -> Option<Vec<Item>> {
    with_component_scope(|| queue_items(document)).map(|items| items.as_ref().clone())
}

/// `#incrementalreconcile`: the component-scoped queue items are exactly the
/// whole-document overlay's, for `text` and random edit sequences of it.
fn check_items(name: &str, text: &str, seeds: u64, steps: usize) {
    assert_eq!(scoped(text), whole(text), "{name}");
    for seed in 0..seeds {
        let mut rng = Rng::new(seed ^ name.len() as u64);
        let mut document = text.to_string();
        for step in 0..steps {
            document = random_edit(&document, &mut rng);
            assert_eq!(
                scoped(&document),
                whole(&document),
                "{name} seed={seed} step={step}\n{document}"
            );
        }
    }
}

fn check_fixture_items(name: &str) {
    check_items(name, fixture(name), 8, 8);
}

mod fixture_items {
    use super::check_fixture_items;
    crate::incremental_corpus::per_fixture_tests!(check_fixture_items);
}

#[test]
fn component_scoped_items_equal_the_whole_document_overlay_on_a_session_shaped_document() {
    check_items("session_shaped", &session_shaped(1, 1200), 3, 6);
}

/// Only the queue component an edit touched is parsed again; an exchange
/// edit parses no queue component.
#[test]
fn only_changed_queue_components_are_parsed_again() {
    let document = session_shaped(1, 1500);
    assert!(scoped(&document).is_some());
    let before = derived_stats();
    let exchange_edit = document.replace(
        "<!-- /agent:exchange -->",
        "### Re: one more cell\n<!-- /agent:exchange -->",
    );
    assert_eq!(scoped(&exchange_edit), whole(&exchange_edit));
    let after = derived_stats();
    assert_eq!(after.slice_parses, before.slice_parses, "{after:?}");
    assert_eq!(after.slice_fallbacks, before.slice_fallbacks);

    // One row added to the first queue: that queue alone is parsed.
    let at = exchange_edit.find("<!-- /agent:run -->").unwrap();
    let mut row_edit = exchange_edit.clone();
    row_edit.insert_str(at, "- one more operator row\n");
    assert_eq!(scoped(&row_edit), whole(&row_edit));
    assert_eq!(derived_stats().slice_parses, after.slice_parses + 1);
}

/// A document whose shape does not guarantee the slice equals the document
/// overlay takes the whole-document overlay.
#[test]
fn unsafe_shapes_take_the_whole_document_overlay() {
    let base = "## Run <!-- agent:run id=\"main\" -->\n- do [#a] work\n<!-- /agent:run -->\n";
    for (label, document) in [
        ("opener after text", format!("intro text\n{base}")),
        ("raw html block", format!("<pre>\n\n{base}")),
        (
            "runnable opener in code",
            format!("{base}\n```\n<!-- agent:run id=\"x\" -->\n```\n"),
        ),
        (
            "indented closer",
            "<!-- agent:run -->\n- a\n  <!-- /agent:run -->\n".to_string(),
        ),
    ] {
        let before = derived_stats().slice_fallbacks;
        assert_eq!(scoped(&document), whole(&document), "{label}");
        assert_eq!(derived_stats().slice_fallbacks, before + 1, "{label}");
    }
    // A fence left open inside the queue never closes the slice either.
    let open_fence = "<!-- agent:run -->\n- a\n```\n<!-- /agent:run -->\n\n<!-- agent:run id=\"b\" -->\n- b\n<!-- /agent:run -->\n";
    assert_eq!(scoped(open_fence), whole(open_fence));
}

/// The memoized queue tree is the tree.
#[test]
fn memoized_queue_tree_matches_a_fresh_build() {
    for (name, text) in FIXTURES {
        let options = crate::queue_tree::QueueTreeOptions::default();
        let first = crate::queue_tree::queue_tree(text, &options).ok();
        let again = crate::queue_tree::queue_tree(text, &options).ok();
        let scoped = with_component_scope(|| crate::queue_tree::queue_tree(text, &options).ok());
        assert_eq!(first, again, "{name}");
        assert_eq!(first, scoped, "{name}");
    }
}
