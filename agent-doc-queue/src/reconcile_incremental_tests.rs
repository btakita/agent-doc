use super::*;
use crate::incremental_corpus::{FIXTURES, Rng, fixture, random_edit, session_shaped};

fn is_live(_owner: &str, generation: u64) -> bool {
    generation.is_multiple_of(2)
}
fn response_proven(id: &str) -> bool {
    id.len().is_multiple_of(2)
}
fn struck_provenance(work: &str) -> bool {
    work.contains("probe")
}
fn is_claimed(row: &str) -> bool {
    row.contains("running probe")
}

const INPUTS: ReconcileInputs<'static> = ReconcileInputs {
    is_live: &is_live,
    response_proven: &response_proven,
    struck_provenance: &struck_provenance,
    is_claimed: &is_claimed,
};

/// `#incrementalreconcile`: across random edit sequences of `text`, with one
/// cache kept across revisions (and the reconciled document sometimes written
/// back as the next revision, as a background reconciler does), the
/// incremental chain is byte-identical to the full chain.
fn check_chain(name: &str, text: &str, seeds: u64, steps: usize) {
    for seed in 0..seeds {
        let mut rng = Rng::new(seed.wrapping_add(name.len() as u64 * 31));
        let mut cache = ReconcileCache::new();
        let mut prev: Option<String> = None;
        let mut document = text.to_string();
        for step in 0..steps {
            let report = reconcile_incremental(prev.as_deref(), &document, &INPUTS, &mut cache);
            let full = reconcile_full(&document, &INPUTS);
            assert_eq!(
                report.reconciled, full,
                "{name} seed={seed} step={step}\n--- document ---\n{document}"
            );
            prev = Some(document.clone());
            document = if rng.below(3) == 0 {
                report.reconciled.document
            } else {
                random_edit(&document, &mut rng)
            };
        }
    }
}

fn check_fixture_chain(name: &str) {
    check_chain(name, fixture(name), 4, 6);
}

mod fixture_chains {
    use super::check_fixture_chain;
    crate::incremental_corpus::per_fixture_tests!(check_fixture_chain);
}

#[test]
fn incremental_chain_equals_the_full_chain_on_a_session_shaped_document() {
    check_chain("session_shaped", &session_shaped(1, 400), 1, 4);
}

/// A revision already reconciled replays from the cache while every external
/// answer is unchanged, and runs again once one changes.
#[test]
fn an_unchanged_revision_replays_until_an_external_answer_changes() {
    let (_, document) = FIXTURES
        .iter()
        .find(|(name, _)| *name == "claim_label_residue_session")
        .unwrap();
    let mut cache = ReconcileCache::new();
    let first = reconcile_incremental(None, document, &INPUTS, &mut cache);
    assert!(!first.reused);
    let again = reconcile_incremental(Some(document), document, &INPUTS, &mut cache);
    assert!(again.reused);
    assert!(again.dirty_components.is_empty(), "{again:?}");
    assert_eq!(again.reconciled, first.reconciled);

    // Every attempt live now: a label the first run stripped stays.
    let all_live = |_: &str, _: u64| true;
    let changed = ReconcileInputs {
        is_live: &all_live,
        ..INPUTS
    };
    let rerun = reconcile_incremental(Some(document), document, &changed, &mut cache);
    assert_eq!(rerun.reconciled, reconcile_full(document, &changed));
    if first
        .reconciled
        .repairs
        .iter()
        .any(|repair| repair.kind == crate::self_repair::SelfRepairKind::StaleClaimLabel)
    {
        assert!(!rerun.reused);
        assert_ne!(rerun.reconciled, first.reconciled);
    }
}

/// The report names exactly the components an edit changed.
#[test]
fn the_report_names_the_changed_components() {
    let document = session_shaped(1, 400);
    let mut cache = ReconcileCache::new();
    let first = reconcile_incremental(None, &document, &INPUTS, &mut cache);
    assert!(first.dirty_components.len() > 3, "{first:?}");
    let edited = document.replace(
        "<!-- /agent:exchange -->",
        "### Re: one more cell\n<!-- /agent:exchange -->",
    );
    let second = reconcile_incremental(None, &edited, &INPUTS, &mut cache);
    assert_eq!(second.dirty_components, vec!["exchange".to_string()]);
    assert_eq!(second.slice_parses, 0, "{second:?}");
    assert_eq!(second.reconciled, reconcile_full(&edited, &INPUTS));
}

fn time<T>(mut f: impl FnMut() -> T) -> (T, std::time::Duration) {
    let start = std::time::Instant::now();
    let value = f();
    (value, start.elapsed())
}

/// `#incrementalreconcile` measurement (run with `--release --ignored
/// --nocapture`): per-edit chain time on a ~6k-line session-shaped document,
/// full versus incremental. `AGENT_DOC_RECONCILE_BENCH_DOC` names a document
/// to measure instead.
#[test]
#[ignore]
fn bench_chain_per_edit() {
    let document = match std::env::var("AGENT_DOC_RECONCILE_BENCH_DOC") {
        Ok(path) => std::fs::read_to_string(path).unwrap(),
        Err(_) => session_shaped(1, 6000),
    };
    eprintln!(
        "document: {} lines, {} bytes",
        document.lines().count(),
        document.len()
    );
    // Settle the document the way a reconciler leaves it.
    let mut settled = reconcile_full(&document, &INPUTS).document;
    for _ in 0..3 {
        settled = reconcile_full(&settled, &INPUTS).document;
    }
    let first_queue_close = settled.find("<!-- /agent:run -->").unwrap();
    let last_queue_close = settled.rfind("<!-- /agent:run -->").unwrap();
    let edits: Vec<(&str, String)> = vec![
        (
            "append exchange cell",
            settled.replace(
                "<!-- /agent:exchange -->",
                "### Re: one more cell\n> **Queue prompt:** probe\n<!-- /agent:exchange -->",
            ),
        ),
        ("add row to first queue", {
            let mut doc = settled.clone();
            doc.insert_str(first_queue_close, "- one more operator row\n");
            doc
        }),
        ("add row to last queue", {
            let mut doc = settled.clone();
            doc.insert_str(last_queue_close, "- one more operator row\n");
            doc
        }),
        ("strike a queue row", {
            let queue = settled.find("<!-- agent:run").unwrap();
            let at = queue + settled[queue..].find("\n- ").unwrap() + 1;
            let end = at + settled[at..].find('\n').unwrap();
            let mut doc = settled.clone();
            let row = doc[at + 2..end].to_string();
            doc.replace_range(at..end, &format!("- ~~{row}~~"));
            doc
        }),
        ("unchanged revision", settled.clone()),
    ];
    let mut cache = ReconcileCache::new();
    reconcile_incremental(None, &settled, &INPUTS, &mut cache);
    for (label, edited) in &edits {
        // Every pass deriving its own whole-document state, as before.
        let (baseline, full_time) =
            time(|| crate::derived_memo::without_memos(|| reconcile_full(edited, &INPUTS)));
        let (report, incremental_time) =
            time(|| reconcile_incremental(Some(&settled), edited, &INPUTS, &mut cache));
        let reference = reconcile_full(edited, &INPUTS);
        assert_eq!(report.reconciled, reference, "{label}");
        assert_eq!(baseline, reference, "{label}");
        eprintln!(
            "{label:>24}: full {full_time:>10.2?}  incremental {incremental_time:>10.2?}  \
             reused={} slice_parses={} document_parses={} dirty={:?}",
            report.reused, report.slice_parses, report.document_parses, report.dirty_components
        );
        // The next edit starts from the settled document again.
        reconcile_incremental(Some(edited), &settled, &INPUTS, &mut cache);
    }
}

/// Per-pass breakdown of [`bench_chain_per_edit`] (`--release --ignored
/// --nocapture`): each pass on an edited revision, inside the component
/// scope with the previous revision's memos warm.
#[test]
#[ignore]
fn bench_chain_stages() {
    let document = match std::env::var("AGENT_DOC_RECONCILE_BENCH_DOC") {
        Ok(path) => std::fs::read_to_string(path).unwrap(),
        Err(_) => session_shaped(1, 6000),
    };
    let settled = reconcile_full(&document, &INPUTS).document;
    let close = settled.find("<!-- /agent:run -->").unwrap();
    let mut edited = settled.clone();
    edited.insert_str(close, "- one more operator row\n");
    crate::derived_memo::with_component_scope(|| {
        let _ = run_chain(&settled, &INPUTS);
        let doc = edited.as_str();
        type Stage<'a> = (&'static str, Box<dyn Fn() -> bool + 'a>);
        let stages: Vec<Stage<'_>> = vec![
            (
                "moved_followup_states",
                Box::new(|| {
                    crate::followup_moved_state::reconcile_moved_followup_states(doc).is_some()
                }),
            ),
            (
                "self_repair",
                Box::new(|| {
                    crate::self_repair::self_repair_document_with_provenance(
                        doc,
                        INPUTS.is_live,
                        INPUTS.response_proven,
                        INPUTS.struck_provenance,
                    )
                    .is_some()
                }),
            ),
            (
                "drop_detached",
                Box::new(|| {
                    crate::response_remaining::drop_detached_response_guidance(doc).is_some()
                }),
            ),
            (
                "slim",
                Box::new(|| {
                    crate::response_tree_sync::slim_handback_response_details(doc).is_some()
                }),
            ),
            (
                "remaining",
                Box::new(|| crate::response_remaining::reconcile_response_remaining(doc).is_some()),
            ),
            (
                "finished",
                Box::new(|| {
                    crate::finished_tree::strike_finished_trees(doc, INPUTS.is_claimed).is_some()
                }),
            ),
            (
                "guidance",
                Box::new(|| {
                    crate::response_tree_sync::reconcile_tree_scoped_guidance(doc).is_some()
                }),
            ),
            (
                "bare",
                Box::new(|| crate::response_tree_sync::guide_bare_struck_rows(doc).is_some()),
            ),
            (
                "indicators",
                Box::new(|| crate::tree_indicator::reconcile_tree_indicators(doc).is_some()),
            ),
            (
                "legend",
                Box::new(|| {
                    agent_doc_document::status_projection::refresh_stale_queue_marker_legend(doc)
                        .is_some()
                }),
            ),
            (
                "open_counts",
                Box::new(|| crate::open_count::reconcile_open_counts(doc).is_ok()),
            ),
            (
                "loose_ends",
                Box::new(|| crate::loose_ends::reconcile_loose_ends(doc).is_ok()),
            ),
            (
                "queue_tree",
                Box::new(|| crate::queue_tree::queue_tree(doc, &Default::default()).is_ok()),
            ),
        ];
        for round in 0..2 {
            for (label, stage) in &stages {
                let (_, elapsed) = time(stage);
                eprintln!("round {round} {label:>22}: {elapsed:>10.2?}");
            }
        }
    });
}
