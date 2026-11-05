//! `#straytextqueueitem`: stray text glued before a component closing marker
//! (`t<!-- /agent:run -->`, tolerated since `#strayclosemarker`) is lint-only
//! content. It must never become dispatchable queue work, while the same text
//! typed on its own line stays an ordinary, dispatchable queue item.

use crate::document_queue::{self, QueueEntry};
use crate::queue_continuation::{self, DrainScope};
use crate::queue_graph::QueueGraph;
use crate::queue_sync_payload::{QueueSyncDocumentScope, project_document_queue_sync};

/// The fpe.md shape: an `agent:run ... go` whose closer carries a stray `t`.
fn welded(stray: &str) -> String {
    format!(
        concat!(
            "## main <!-- agent:run id=\"main\" subagents priority go -->\n",
            "- https://github.com/haiven-dev/fpe/pull/217\n",
            "  - #adversarial-pr\n",
            "{stray}<!-- /agent:run -->\n",
        ),
        stray = stray
    )
}

/// The same text typed by the operator on its own line above the closer.
fn own_line(text: &str) -> String {
    format!(
        concat!(
            "## main <!-- agent:run id=\"main\" subagents priority go -->\n",
            "- https://github.com/haiven-dev/fpe/pull/217\n",
            "  - #adversarial-pr\n",
            "{text}\n",
            "<!-- /agent:run -->\n",
        ),
        text = text
    )
}

fn run_body(doc: &str) -> &str {
    let components = agent_doc_element::element::parse(doc).expect("document parses");
    let run = components
        .into_iter()
        .find(|component| component.name == "queue")
        .expect("the run component");
    &doc[run.open_end..run.close_start]
}

fn dispatch_items(doc: &str) -> Vec<String> {
    project_document_queue_sync(
        "/tmp/fpe.md",
        QueueSyncDocumentScope::Current,
        doc,
        None,
        &[],
    )
    .expect("queue sync projects")
    .items()
    .map(|item| item.item.clone())
    .collect()
}

fn mentions(rows: &[String], needle: &str) -> bool {
    rows.iter()
        .any(|row| row.trim() == needle || row.contains(needle))
}

#[test]
fn welded_stray_text_parses_as_one_inert_entry() {
    for stray in ["t", "- t", "1. fix it", "  indented stray"] {
        let doc = welded(stray);
        let entries = document_queue::parse(run_body(&doc)).unwrap();
        assert_eq!(
            entries.last(),
            Some(&QueueEntry::StrayCloseText(stray.to_string())),
            "{stray:?}: {entries:?}"
        );
        assert!(
            document_queue::prompts(&entries)
                .iter()
                .all(|prompt| !prompt
                    .text
                    .contains(stray.trim_start_matches(['-', '1', '.', ' ']))),
            "{stray:?} must not be a prompt: {entries:?}"
        );
        let executable = document_queue::executable_prompt_indexes(&entries);
        assert!(
            executable
                .iter()
                .all(|index| !matches!(entries[*index], QueueEntry::StrayCloseText(_))),
            "{stray:?}: {entries:?}"
        );
    }
}

#[test]
fn own_line_short_text_stays_a_dispatchable_queue_item() {
    // Controls: the same bytes on their own line are operator work. These
    // prove the dispatch instruments below can see a one-character item.
    let doc = own_line("t");
    let entries = document_queue::parse(run_body(&doc)).unwrap();
    assert!(
        document_queue::prompts(&entries)
            .iter()
            .any(|prompt| prompt.text.trim() == "t"),
        "{entries:?}"
    );
    assert!(
        mentions(&dispatch_items(&doc), "t"),
        "{:?}",
        dispatch_items(&doc)
    );
    assert!(
        queue_continuation::live_executable_rows(&doc)
            .unwrap_or_default()
            .iter()
            .any(|(raw, _)| raw.trim() == "t"),
        "steering sees the own-line item"
    );

    let doc = own_line("- t");
    assert!(
        mentions(&dispatch_items(&doc), "t"),
        "{:?}",
        dispatch_items(&doc)
    );

    // Preflight selection control: the drained fixture of
    // `welded_stray_text_is_never_selected_by_preflight` with the text moved
    // onto its own line selects it.
    let doc = drained_run("- t\n");
    for scope in [DrainScope::InSessionLoop, DrainScope::Supervisor] {
        assert_eq!(
            queue_continuation::pending_head_prompt_text(&doc, scope).as_deref(),
            Some("t"),
            "{scope:?}"
        );
    }
    assert!(mentions(&dispatch_items(&doc), "t"));
}

/// A run whose only row is completed, followed by `tail` and the closer.
fn drained_run(tail: &str) -> String {
    format!("<!-- agent:run id=\"main\" go -->\n- ~~do [#done]~~\n{tail}<!-- /agent:run -->\n")
}

#[test]
fn welded_stray_text_is_never_dispatched() {
    // The welded document dispatches exactly what the same document without
    // the stray text dispatches; on its own line the text adds an item.
    let clean = dispatch_items(&welded(""));
    assert!(mentions(&clean, "#adversarial-pr"), "{clean:?}");
    for stray in ["t", "- t", "- fix the build"] {
        let items = dispatch_items(&welded(stray));
        assert_eq!(items, clean, "{stray:?} reached the dispatch payload");
        assert_ne!(
            dispatch_items(&own_line(stray)),
            clean,
            "{stray:?} on its own line is operator work"
        );
    }
}

#[test]
fn welded_stray_text_is_never_selected_by_preflight() {
    // Only stray text after a completed row: nothing is runnable.
    for stray in ["t", "- t"] {
        let doc = drained_run(stray);
        assert_nothing_selected(&doc);
    }
}

fn assert_nothing_selected(doc: &str) {
    for scope in [DrainScope::InSessionLoop, DrainScope::Supervisor] {
        assert_eq!(
            queue_continuation::pending_head_prompt_text(doc, scope),
            None,
            "{scope:?}"
        );
        assert_eq!(
            queue_continuation::drainable_head_prompt_for_scope(doc, scope),
            None,
            "{scope:?}"
        );
        assert_eq!(
            queue_continuation::live_drainable_continuation_head(doc, scope),
            None,
            "{scope:?}"
        );
    }
    assert!(crate::queue_heads::active_queue_heads(doc).is_empty());
    assert!(crate::queue_heads::active_free_text_queue_heads(doc).is_empty());
    assert!(
        crate::queue_schedule::live_heads_for_queue(doc, "main")
            .unwrap()
            .is_empty()
    );
    let components = agent_doc_element::element::parse(doc).unwrap();
    let graph = QueueGraph::parse(doc, &components).unwrap();
    assert_eq!(graph.block("main").unwrap().live_prompt_count(), 0);
    assert!(dispatch_items(doc).is_empty(), "{:?}", dispatch_items(doc));
}

#[test]
fn welded_stray_text_is_never_a_steering_candidate() {
    let doc = welded("t");
    let rows = queue_continuation::live_executable_rows(&doc).unwrap_or_default();
    assert!(
        rows.iter()
            .all(|(raw, lineage)| raw.trim() != "t" && lineage.trim() != "t"),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|(raw, _)| raw.contains("#adversarial-pr")),
        "{rows:?}"
    );
}

#[test]
fn stray_text_is_not_folded_into_the_row_above() {
    // An indented stray prefix would otherwise read as a continuation line of
    // the free-text row above it and be dispatched as part of that row.
    let doc = concat!(
        "<!-- agent:run id=\"main\" go -->\n",
        "- investigate the flaky route test\n",
        "  t<!-- /agent:run -->\n",
    );
    let entries = document_queue::parse(run_body(doc)).unwrap();
    assert_eq!(
        entries,
        vec![
            document_queue::parse_fragment("- investigate the flaky route test\n")
                .unwrap()
                .remove(0),
            QueueEntry::StrayCloseText("  t".to_string()),
        ]
    );
}

#[test]
fn render_keeps_stray_text_welded_through_mutations() {
    let doc = welded("t");
    let body = run_body(&doc);
    let mut entries = document_queue::parse(body).unwrap();
    let rendered = document_queue::render(&entries);
    assert!(rendered.ends_with("  - #adversarial-pr\nt"), "{rendered:?}");
    assert_eq!(
        document_queue::render(&document_queue::parse(&rendered).unwrap()),
        rendered,
        "render is a fixpoint and keeps the stray text"
    );

    // A mutation that appends work after the stray entry still renders the
    // stray text last and unterminated, so it stays glued to the closer.
    entries.extend(document_queue::parse_fragment("- do [#appended]\n").unwrap());
    let rendered = document_queue::render(&entries);
    assert!(
        rendered.ends_with("- ### do [#appended]\nt"),
        "{rendered:?}"
    );
    let reparsed = document_queue::parse(&rendered).unwrap();
    assert_eq!(
        reparsed.last(),
        Some(&QueueEntry::StrayCloseText("t".to_string()))
    );
    assert_eq!(document_queue::render(&reparsed), rendered, "fixpoint");
}

#[test]
fn fragments_without_a_trailing_newline_keep_their_last_row() {
    // Only whole component bodies carry the welded-closer shape; a fragment's
    // unterminated final line is ordinary queue text.
    let entries = document_queue::parse_fragment("- do [#a]\n- do [#b]").unwrap();
    assert_eq!(document_queue::prompts(&entries).len(), 2, "{entries:?}");
}

#[test]
fn stray_text_blocks_a_lossless_clear_like_operator_text() {
    let entries = document_queue::parse("- ~~do [#done]~~\nt").unwrap();
    assert!(!crate::queue_convergence::queue_body_clear_is_lossless(
        &entries
    ));
    let drained = document_queue::parse("- ~~do [#done]~~\n").unwrap();
    assert!(crate::queue_convergence::queue_body_clear_is_lossless(
        &drained
    ));
}
