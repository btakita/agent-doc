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
        // The last revision the chain ran on.
        let mut ran: Option<String> = None;
        for step in 0..steps {
            let report = reconcile_incremental(prev.as_deref(), &document, &INPUTS, &mut cache);
            let full = expected(&report, ran.as_deref(), &document);
            assert_eq!(
                report.reconciled, full,
                "{name} seed={seed} step={step} mode={:?}\n--- document ---\n{document}",
                report.mode
            );
            assert_eq!(report.changed, report.reconciled.document != document);
            if report.mode != ReconcileMode::ExchangeOnly {
                ran = Some(document.clone());
            }
            prev = Some(document.clone());
            document = if rng.below(3) == 0 {
                report.reconciled.document
            } else {
                random_edit(&document, &mut rng)
            };
        }
    }
}

/// The minimal splice turning `a` into `b` (the byte compare's own).
fn minimal_splices(a: &str, b: &str) -> Vec<TextSplice> {
    match splice(a, b) {
        None => Vec::new(),
        Some((removed, inserted)) => vec![TextSplice {
            start: removed.start,
            removed_len: removed.len(),
            inserted: b[inserted].to_string(),
        }],
    }
}

/// A changeset turning `a` into `b` the way a CRDT journal may spell it:
/// the minimal splice, sometimes widened over unchanged bytes to whole
/// lines, sometimes split into a delete and one or two inserts.
fn journal_splices(a: &str, b: &str, rng: &mut Rng) -> Vec<TextSplice> {
    let Some((removed, inserted)) = splice(a, b) else {
        return Vec::new();
    };
    let (mut start, mut old_end, mut new_end) = (removed.start, removed.end, inserted.end);
    if rng.below(2) == 0 {
        // Widen over the common prefix and suffix to line boundaries.
        start = a[..start].rfind('\n').map_or(0, |at| at + 1);
        let grow = a[old_end..]
            .find('\n')
            .map_or(a.len() - old_end, |at| at + 1)
            .min(b.len() - new_end);
        assert!(
            a.is_char_boundary(old_end + grow) && b.is_char_boundary(new_end + grow),
            "a common suffix grows on equal bytes"
        );
        old_end += grow;
        new_end += grow;
    }
    let text = b[start..new_end].to_string();
    let removed_len = old_end - start;
    match rng.below(3) {
        0 => vec![TextSplice {
            start,
            removed_len,
            inserted: text,
        }],
        1 => {
            let mut cut = rng.below(text.len() + 1);
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            vec![
                TextSplice {
                    start,
                    removed_len,
                    inserted: String::new(),
                },
                TextSplice {
                    start,
                    removed_len: 0,
                    inserted: text[cut..].to_string(),
                },
                TextSplice {
                    start,
                    removed_len: 0,
                    inserted: text[..cut].to_string(),
                },
            ]
        }
        _ => vec![
            TextSplice {
                start,
                removed_len: 0,
                inserted: text.clone(),
            },
            TextSplice {
                start: start + text.len(),
                removed_len,
                inserted: String::new(),
            },
        ],
    }
}

/// `#incrementalreconcile`: [`check_chain`] with every revision handed in as
/// its changeset, as the controller hands in its CRDT journal. A revision is
/// one to three random edits (or the reconciled document written back), each
/// spelled as journal splices and concatenated. The chain stays
/// byte-identical to the full chain and never falls back to comparing.
fn check_changeset_chain(name: &str, text: &str, seeds: u64, steps: usize) {
    for seed in 0..seeds {
        let mut rng = Rng::new(seed.wrapping_add(name.len() as u64 * 57));
        let mut cache = ReconcileCache::new();
        let mut document = text.to_string();
        let mut changeset: Vec<TextSplice> = Vec::new();
        let mut ran: Option<String> = None;
        for step in 0..steps {
            let report =
                reconcile_incremental_changeset(&changeset, &document, &INPUTS, &mut cache);
            if step > 0 {
                assert_eq!(
                    report.changeset,
                    ChangesetSource::Splices,
                    "{name} seed={seed} step={step}"
                );
                assert_eq!(
                    report.changeset_fallback, None,
                    "{name} seed={seed} step={step}"
                );
                assert_eq!(
                    report.exchange_bytes_compared, 0,
                    "{name} seed={seed} step={step}"
                );
            }
            let full = expected(&report, ran.as_deref(), &document);
            assert_eq!(
                report.reconciled, full,
                "{name} seed={seed} step={step} mode={:?}\n--- document ---\n{document}",
                report.mode
            );
            assert_eq!(report.changed, report.reconciled.document != document);
            if report.mode != ReconcileMode::ExchangeOnly {
                ran = Some(document.clone());
            }
            changeset.clear();
            let mut next = document.clone();
            if rng.below(3) == 0 {
                let written = report.reconciled.document;
                changeset.extend(journal_splices(&next, &written, &mut rng));
                next = written;
            } else {
                for _ in 0..1 + rng.below(3) {
                    let edited = random_edit(&next, &mut rng);
                    changeset.extend(journal_splices(&next, &edited, &mut rng));
                    next = edited;
                }
            }
            document = next;
        }
    }
}

fn check_fixture_changeset_chain(name: &str) {
    check_changeset_chain(name, fixture(name), 4, 6);
}

mod fixture_changeset_chains {
    use super::check_fixture_changeset_chain;
    crate::incremental_corpus::per_fixture_tests!(check_fixture_changeset_chain);
}

#[test]
fn changeset_chain_equals_the_full_chain_on_a_session_shaped_document() {
    check_changeset_chain("session_shaped", &session_shaped(1, 400), 2, 6);
}

/// A changeset's sequential splices become sorted hunks of the previous
/// revision, read against the next one only where text was inserted.
#[test]
fn a_changeset_becomes_the_hunks_it_spells() {
    let old = "abcdefghij";
    let next = "aXcdefgYYj";
    let changeset = [
        // b -> X
        TextSplice {
            start: 1,
            removed_len: 1,
            inserted: "X".into(),
        },
        // hi -> YY, addressed after the first splice
        TextSplice {
            start: 7,
            removed_len: 2,
            inserted: "YY".into(),
        },
    ];
    assert_eq!(
        changeset_hunks(old.len(), &changeset, next),
        Ok(vec![
            Hunk {
                old: 1..2,
                new: 1..2
            },
            Hunk {
                old: 7..9,
                new: 7..9
            },
        ])
    );
    // A later splice inside an earlier insert stays one hunk.
    let typed = [
        TextSplice {
            start: 3,
            removed_len: 0,
            inserted: "xyz".into(),
        },
        TextSplice {
            start: 4,
            removed_len: 1,
            inserted: "Q".into(),
        },
    ];
    assert_eq!(
        changeset_hunks(old.len(), &typed, "abcxQzdefghij"),
        Ok(vec![Hunk {
            old: 3..3,
            new: 3..6
        }])
    );
    assert_eq!(changeset_hunks(old.len(), &[], old), Ok(Vec::new()));
    assert_eq!(
        changeset_hunks(old.len(), &changeset, "aZcdefgYYj"),
        Err("inserted_bytes_mismatch")
    );
    assert_eq!(
        changeset_hunks(old.len(), &changeset, "aXcdefgYYjk"),
        Err("length_mismatch")
    );
    assert_eq!(
        changeset_hunks(
            old.len(),
            &[TextSplice {
                start: 9,
                removed_len: 2,
                inserted: String::new()
            }],
            "abcdefghi"
        ),
        Err("splice_out_of_range")
    );
}

/// The exchange body of `document` (its single `agent:exchange`).
fn exchange_body(document: &str) -> std::ops::Range<usize> {
    let components = agent_doc_element::element::parse(document).unwrap();
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")
        .unwrap();
    exchange.open_end..exchange.close_start
}

/// What `report` must equal: [`reconcile_full`] of `document`, or for an
/// exchange-only revision, [`reconcile_full`] of the revision the chain last
/// ran on (`ran`, the same outside the exchange) with `document`'s exchange.
fn expected(report: &ReconcileReport, ran: Option<&str>, document: &str) -> Reconciled {
    if report.mode != ReconcileMode::ExchangeOnly {
        return reconcile_full(document, &INPUTS);
    }
    let ran = ran.expect("an exchange-only revision follows a run");
    let ran_body = exchange_body(ran);
    let body = exchange_body(document);
    assert_eq!(&ran[..ran_body.start], &document[..body.start]);
    assert_eq!(&ran[ran_body.end..], &document[body.end..]);
    let mut full = reconcile_full(ran, &INPUTS);
    let out_body = exchange_body(&full.document);
    full.document
        .replace_range(out_body, &document[body.clone()]);
    full
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

/// A session-shaped document the chain has settled (a fixed point).
fn settled_session() -> String {
    let mut settled = reconcile_full(&session_shaped(1, 1500), &INPUTS).document;
    for _ in 0..3 {
        settled = reconcile_full(&settled, &INPUTS).document;
    }
    settled
}

/// `#incrementalreconcile`: compacting the exchange (most of it removed behind a
/// summary) runs no pass and reads only the chunks the splice touched; the
/// result is the settled result with the compacted exchange.
#[test]
fn an_exchange_compaction_runs_no_pass_and_reads_only_the_touched_chunks() {
    let settled = settled_session();
    let mut cache = ReconcileCache::new();
    let first = reconcile_incremental(None, &settled, &INPUTS, &mut cache);
    assert_eq!(first.mode, ReconcileMode::Skeleton, "{first:?}");
    let compaction = compacted(&settled);
    let report = reconcile_incremental(None, &compaction, &INPUTS, &mut cache);
    assert_eq!(
        report.mode,
        ReconcileMode::ExchangeOnly,
        "{:?}",
        report.rebuilt
    );
    assert!(report.reused);
    assert_eq!(report.rebuilt, None);
    assert_eq!(report.dirty_components, vec!["exchange".to_string()]);
    assert_eq!((report.slice_parses, report.document_parses), (0, 0));
    assert!(
        report.exchange_bytes_indexed < 1024,
        "read {} exchange bytes; only the summary is new",
        report.exchange_bytes_indexed,
    );
    assert_eq!(
        report.reconciled,
        expected(&report, Some(&settled), &compaction)
    );
    // Settled and compacted, the full chain agrees too: the removed cells
    // were no evidence the settled queues still needed.
    assert_eq!(report.reconciled, reconcile_full(&compaction, &INPUTS));
}

/// A revision that edits a queue and appends to the exchange (an agent
/// turn) follows both splices: the chain runs on the skeleton, the exchange
/// is read only where it grew, and the result equals the full chain.
#[test]
fn a_queue_edit_with_an_exchange_append_runs_on_the_skeleton() {
    let settled = settled_session();
    let mut cache = ReconcileCache::new();
    reconcile_incremental(None, &settled, &INPUTS, &mut cache);
    let close = settled.find("<!-- /agent:run -->").unwrap();
    let mut edited = settled.clone();
    edited.insert_str(close, "- one more operator row\n");
    let exchange_close = edited.rfind("<!-- /agent:exchange -->").unwrap();
    edited.insert_str(
        exchange_close,
        "### Re: one more cell\n> **Queue prompt:** one more operator row\nDone.\n",
    );
    let report = reconcile_incremental(None, &edited, &INPUTS, &mut cache);
    assert_eq!(report.mode, ReconcileMode::Skeleton);
    assert_eq!(report.rebuilt, None);
    assert!(!report.reused);
    let body = exchange_body(&edited);
    assert!(
        report.chain_bytes < edited.len() - body.len() / 2,
        "the skeleton ({} bytes) must leave out most of the {}-byte exchange of the {}-byte document",
        report.chain_bytes,
        body.len(),
        edited.len()
    );
    assert!(
        report.exchange_bytes_indexed < 4096,
        "read {} exchange bytes",
        report.exchange_bytes_indexed
    );
    assert_eq!(
        report.dirty_components.len(),
        2,
        "{:?}",
        report.dirty_components
    );
    assert!(report.dirty_components.contains(&"exchange".to_string()));
    assert_eq!(report.reconciled, reconcile_full(&edited, &INPUTS));
}

/// `#incrementalreconcile`: the same queue edit and exchange append handed in
/// as the controller's changeset compares no exchange byte (the byte compare
/// reads through the exchange to find the queue edit), and indexes only the
/// appended cell.
#[test]
fn a_queue_edit_with_an_exchange_append_from_the_changeset_compares_no_exchange_byte() {
    let settled = settled_session();
    let close = settled.find("<!-- /agent:run -->").unwrap();
    let row = "- one more operator row\n";
    let cell = "### Re: one more cell\n> **Queue prompt:** one more operator row\nDone.\n";
    let mut edited = settled.clone();
    edited.insert_str(close, row);
    let exchange_close = edited.rfind("<!-- /agent:exchange -->").unwrap();
    edited.insert_str(exchange_close, cell);
    let changeset = [
        TextSplice {
            start: close,
            removed_len: 0,
            inserted: row.to_string(),
        },
        TextSplice {
            start: exchange_close,
            removed_len: 0,
            inserted: cell.to_string(),
        },
    ];

    let mut compared = ReconcileCache::new();
    reconcile_incremental(None, &settled, &INPUTS, &mut compared);
    let by_compare = reconcile_incremental(None, &edited, &INPUTS, &mut compared);
    assert_eq!(by_compare.changeset, ChangesetSource::ByteCompare);
    assert!(
        by_compare.exchange_bytes_compared > exchange_body(&settled).len() / 2,
        "{by_compare:?}"
    );

    let mut cache = ReconcileCache::new();
    reconcile_incremental(None, &settled, &INPUTS, &mut cache);
    let report = reconcile_incremental_changeset(&changeset, &edited, &INPUTS, &mut cache);
    assert_eq!(report.changeset, ChangesetSource::Splices);
    assert_eq!(report.changeset_fallback, None);
    assert_eq!(report.exchange_bytes_compared, 0);
    assert_eq!(report.exchange_bytes_indexed, cell.len());
    assert_eq!(report.mode, ReconcileMode::Skeleton);
    assert_eq!(report.rebuilt, None);
    assert_eq!(report.dirty_components, by_compare.dirty_components);
    assert_eq!(report.reconciled, reconcile_full(&edited, &INPUTS));
    assert_eq!(report.reconciled, by_compare.reconciled);
}

/// A changeset that does not spell the next revision is not trusted: the
/// texts are compared instead and the result is still exact.
#[test]
fn a_changeset_that_does_not_spell_the_revision_falls_back_to_comparing() {
    let settled = settled_session();
    let close = settled.rfind("<!-- /agent:exchange -->").unwrap();
    let mut edited = settled.clone();
    edited.insert_str(close, "### Re: appended cell\n> **Queue prompt:** probe\n");
    let wrong = [TextSplice {
        start: close,
        removed_len: 0,
        inserted: "### Re: appendXd cell\n> **Queue prompt:** probe\n".to_string(),
    }];
    let mut cache = ReconcileCache::new();
    reconcile_incremental(None, &settled, &INPUTS, &mut cache);
    let report = reconcile_incremental_changeset(&wrong, &edited, &INPUTS, &mut cache);
    assert_eq!(report.changeset, ChangesetSource::ByteCompare);
    assert_eq!(report.changeset_fallback, Some("inserted_bytes_mismatch"));
    assert_eq!(report.reconciled, reconcile_full(&edited, &INPUTS));

    // No cached revision: the first revision is a rebuild either way.
    let mut empty = ReconcileCache::new();
    let report = reconcile_incremental_changeset(&wrong, &edited, &INPUTS, &mut empty);
    assert_eq!(report.changeset_fallback, Some("no_cached_revision"));
    assert_eq!(report.rebuilt, Some("first_revision"));
    assert_eq!(report.reconciled, reconcile_full(&edited, &INPUTS));
}

/// An exchange-only revision that adds evidence (a queue-prompt echo, a
/// completion cell) runs the chain on the skeleton.
#[test]
fn an_exchange_append_with_new_evidence_runs_the_chain() {
    let settled = settled_session();
    let mut cache = ReconcileCache::new();
    reconcile_incremental(None, &settled, &INPUTS, &mut cache);
    let close = settled.rfind("<!-- /agent:exchange -->").unwrap();
    let mut edited = settled.clone();
    edited.insert_str(close, "### Re: appended cell\n> **Queue prompt:** probe\n");
    let report = reconcile_incremental(None, &edited, &INPUTS, &mut cache);
    assert_eq!(report.mode, ReconcileMode::Skeleton);
    assert_eq!(report.dirty_components, vec!["exchange".to_string()]);
    assert_eq!(report.reconciled, reconcile_full(&edited, &INPUTS));

    // An appended note with no evidence runs nothing.
    let mut noted = edited.clone();
    let close = noted.rfind("<!-- /agent:exchange -->").unwrap();
    noted.insert_str(close, "A plain note.\n");
    let report = reconcile_incremental(None, &noted, &INPUTS, &mut cache);
    assert_eq!(report.mode, ReconcileMode::ExchangeOnly);
    assert_eq!(report.reconciled, reconcile_full(&noted, &INPUTS));
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

/// An sdk-dev-scale session document (about 960 KB, mostly `agent:exchange`).
fn sdk_scale_document() -> String {
    let mut lines = 2000;
    loop {
        let document = session_shaped(1, lines);
        if document.len() >= 960 * 1024 {
            return document;
        }
        lines = lines * 960 * 1024 / document.len().max(1) + 100;
    }
}

/// The exchange compacted: its body replaced by the last tenth of it behind
/// a one-line summary, as `/agent-doc compact exchange` leaves it.
fn compacted(document: &str) -> String {
    let open = document.find("<!-- agent:exchange").unwrap();
    let body_start = open + document[open..].find('\n').unwrap() + 1;
    let close = document.rfind("<!-- /agent:exchange -->").unwrap();
    let body = &document[body_start..close];
    let mut keep = body.len() - body.len() / 10;
    while !body.is_char_boundary(keep) {
        keep += 1;
    }
    let keep = body[keep..]
        .find("\n### ")
        .map_or(body.len(), |at| keep + at + 1);
    format!(
        "{}### Compacted\n> Earlier exchange archived.\n{}{}",
        &document[..body_start],
        &body[keep..],
        &document[close..]
    )
}

/// `#incrementalreconcile` measurement (`--release --ignored --nocapture`): per
/// revision chain time at sdk-dev scale for a one-line queue edit and an
/// exchange compaction, full versus incremental (cache warm on the settled
/// revision).
#[test]
#[ignore]
fn bench_sdk_scale_revisions() {
    let document = match std::env::var("AGENT_DOC_RECONCILE_BENCH_DOC") {
        Ok(path) => std::fs::read_to_string(path).unwrap(),
        Err(_) => sdk_scale_document(),
    };
    let mut settled = reconcile_full(&document, &INPUTS).document;
    for _ in 0..3 {
        settled = reconcile_full(&settled, &INPUTS).document;
    }
    eprintln!(
        "document: {} lines, {} bytes",
        settled.lines().count(),
        settled.len()
    );
    let close = settled.find("<!-- /agent:run -->").unwrap();
    let mut queue_edit = settled.clone();
    queue_edit.insert_str(close, "- one more operator row\n");
    let compaction = compacted(&settled);
    eprintln!("compacted: {} bytes", compaction.len());
    let row = "- one more operator row\n";
    let cell = "### Re: one more cell\n> **Queue prompt:** one more operator row\nDone.\n";
    let mut queue_and_append = queue_edit.clone();
    let exchange_close = queue_and_append.rfind("<!-- /agent:exchange -->").unwrap();
    queue_and_append.insert_str(exchange_close, cell);
    let append_changeset = vec![
        TextSplice {
            start: close,
            removed_len: 0,
            inserted: row.to_string(),
        },
        TextSplice {
            start: exchange_close,
            removed_len: 0,
            inserted: cell.to_string(),
        },
    ];
    for (label, edited, changeset) in [
        (
            "one-line queue edit",
            &queue_edit,
            minimal_splices(&settled, &queue_edit),
        ),
        (
            "exchange compaction",
            &compaction,
            minimal_splices(&settled, &compaction),
        ),
        (
            "queue edit + exchange append",
            &queue_and_append,
            append_changeset,
        ),
    ] {
        let mut cache = ReconcileCache::new();
        reconcile_incremental(None, &settled, &INPUTS, &mut cache);
        // The pre-`#incrementalreconcile` incremental path: the chain on the whole
        // text inside the component scope, its memos warm on `settled`.
        crate::derived_memo::with_component_scope(|| run_chain(&settled, &INPUTS));
        let (_, whole_time) =
            time(|| crate::derived_memo::with_component_scope(|| run_chain(edited, &INPUTS)));
        let (reference, full_time) = time(|| reconcile_full(edited, &INPUTS));
        let (report, incremental_time) =
            time(|| reconcile_incremental(Some(&settled), edited, &INPUTS, &mut cache));
        assert_eq!(report.reconciled, reference, "{label}");
        // The same revision from the changeset (the controller's CRDT
        // journal), the cache warm on `settled` again.
        let mut from_changeset = ReconcileCache::new();
        reconcile_incremental(None, &settled, &INPUTS, &mut from_changeset);
        let (spliced, changeset_time) = time(|| {
            reconcile_incremental_changeset(&changeset, edited, &INPUTS, &mut from_changeset)
        });
        assert_eq!(spliced.reconciled, reference, "{label}");
        assert_eq!(spliced.changeset, ChangesetSource::Splices, "{label}");
        assert_eq!(spliced.exchange_bytes_compared, 0, "{label}");
        for (path, report, elapsed) in [
            ("byte compare", &report, incremental_time),
            ("changeset", &spliced, changeset_time),
        ] {
            eprintln!(
                "{label:>28} [{path:>12}]: full {full_time:>10.2?}  whole-text chain {whole_time:>10.2?}  \
                 incremental {elapsed:>10.2?}  mode={:?} reused={} changed={} chain_bytes={} \
                 exchange_bytes_compared={} exchange_bytes_indexed={} rebuilt={:?} \
                 slice_parses={} document_parses={} dirty={:?}",
                report.mode,
                report.reused,
                report.changed,
                report.chain_bytes,
                report.exchange_bytes_compared,
                report.exchange_bytes_indexed,
                report.rebuilt,
                report.slice_parses,
                report.document_parses,
                report.dirty_components
            );
        }
    }
}
