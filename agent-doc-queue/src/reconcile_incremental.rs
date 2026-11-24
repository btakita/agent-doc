//! `#incrementalreconcile`: the queue and closeout document passes as one
//! chain whose work follows the components that changed.
//!
//! A background reconciler (the controller) calls [`reconcile_incremental`]
//! once per document revision with a [`ReconcileCache`] it keeps per
//! document. The chain is the queue-sync order:
//!
//! 1. [`crate::followup_moved_state::reconcile_moved_followup_states`]
//! 2. [`crate::self_repair::self_repair_document_with_provenance`]
//! 3. [`crate::response_tree_sync::sync_response_trees_striking`] (detached
//!    tallies, slimmed hand-backs, Remaining lists, finished-tree strikes,
//!    tree-scoped guidance, bare struck rows, 🗑️/🔒 tree glyphs)
//! 4. [`agent_doc_document::status_projection::refresh_stale_queue_marker_legend`]
//! 5. [`crate::open_count::reconcile_open_counts`]
//! 6. [`crate::loose_ends::reconcile_loose_ends`]
//!
//! # What is incremental
//!
//! - **The changeset, not a reparse** (`#incrementalreconcile`). The cache keeps
//!   the revision it reconciled last with its component layout and an index
//!   of its `agent:exchange` ([`crate::exchange_index`]). A new revision is
//!   matched against it as up to three splices (before the exchange, inside
//!   its body, after it). A caller holding the revision's changeset (the
//!   controller's CRDT text journal) hands it in as [`TextSplice`]s
//!   ([`reconcile_incremental_changeset`]): the splices are mapped to the
//!   regions directly, reading only the inserted bytes, so no byte of the
//!   exchange is compared ([`ChangesetSource::Splices`]). Without one, each
//!   region's splice is the common prefix/suffix of its region, a `memcmp`
//!   ([`ChangesetSource::ByteCompare`]). The dirty components are the ones
//!   those splices touch.
//!   Only the exchange chunks a splice touches are read; the layout outside
//!   comes from parsing the skeleton, never the full text. A splice that may
//!   move a component boundary in a way the cache cannot follow (an edited
//!   opener or closer, marker text or an unclosed block in a touched chunk)
//!   rebuilds from one full parse, and [`ReconcileReport::rebuilt`] says why.
//! - **`agent:exchange` is opaque.** The chain runs on the skeleton: the
//!   document with the exchange body replaced by its digest (the lines the
//!   passes read there), completion cells answered from the index
//!   ([`ReconcileMode::Skeleton`]). An exchange-only revision that adds no
//!   evidence (a `/agent-doc compact exchange`, an appended note) runs no
//!   pass: the previous result outside the exchange stands, with the new
//!   exchange spliced in ([`ReconcileMode::ExchangeOnly`]). A document
//!   without one opaque exchange runs on the whole text, as before
//!   ([`ReconcileMode::Whole`]).
//! - Derived state is cached per component revision, keyed by the exact
//!   bytes of the component ([`crate::derived_memo`]): a queue component's
//!   item overlay is parsed again only when that component changed.
//!   Whole-revision derivations (the queue tree, the follow-up row index)
//!   are shared by every pass of the chain instead of being rebuilt by each
//!   one.
//! - A chain input the cache already reconciled (same skeleton and exchange
//!   cells, or same whole text), whose external answers (claim ledger,
//!   response index, committed copies) are unchanged, returns the recorded
//!   result without running a pass: every external query the run made is
//!   replayed and must answer the same.
//!
//! # Exactness
//!
//! Every revision the chain runs (or replays) on is byte-identical to
//! [`reconcile_full`], which runs the same passes on the whole document
//! with the component layer off: the skeleton's digest and the index's
//! cells are exactly the exchange facts the passes read, and every cache is
//! keyed by exact bytes or an exact cells revision (never a CRDT state
//! vector, which does not change on a delete). An
//! [`ReconcileMode::ExchangeOnly`] revision equals [`reconcile_full`] of the
//! revision the chain last ran on, with the new exchange: removing exchange
//! evidence is not reconciled until the next revision the chain runs on.
//! Where the document's shape does not guarantee a component-scoped
//! derivation equals the whole-document one, the whole-document derivation
//! runs (see [`crate::derived_memo`]).
//!
//! The passes are pure: the closures in [`ReconcileInputs`] are their only
//! inputs besides the document. A caller that reads the claim ledger or the
//! archive index must hand the current answers on every call.

use std::cell::RefCell;
use std::sync::Arc;

use crate::self_repair::SelfRepair;

/// The external answers the chain reads (the same closures the passes take).
#[derive(Clone, Copy)]
pub struct ReconcileInputs<'a> {
    /// Whether the claim ledger still holds attempt `(owner, generation)`
    /// ([`crate::self_repair::self_repair_document`]).
    pub is_live: &'a dyn Fn(&str, u64) -> bool,
    /// Whether a `[#r-…]` id's original response is in `agent:exchange` or
    /// the response index.
    pub response_proven: &'a dyn Fn(&str) -> bool,
    /// Whether a committed copy holds a row struck at column 0 (`#e107moved`).
    pub struck_provenance: &'a dyn Fn(&str) -> bool,
    /// Whether a live worker claims a row (`#treefinish`).
    pub is_claimed: &'a dyn Fn(&str) -> bool,
}

/// What the response-tree pass changed
/// ([`crate::response_tree_sync::ResponseTreeSync`] without the document).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResponseTreeCounts {
    pub slimmed: usize,
    pub guided: usize,
    pub struck_trees: usize,
    pub indicators: usize,
}

/// One chain's result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reconciled {
    /// The reconciled document (the input when nothing changed).
    pub document: String,
    /// Struck rows whose response now says its action items moved
    /// (`#followupmovedstate`).
    pub moved_followup_rows: usize,
    /// Self-repairs, in the order they ran (`#selfrepaircleanup`).
    pub repairs: Vec<SelfRepair>,
    /// The response-tree pass's counts, when it changed anything.
    pub response_trees: Option<ResponseTreeCounts>,
    /// The generated `Queue markers:` line was refreshed.
    pub legend_refreshed: bool,
    /// A top-level row's open-work count changed (`#looseends`).
    pub open_counts_changed: bool,
    /// The `agent:loose-ends` digest changed (`#looseends`).
    pub loose_ends_changed: bool,
}

/// How [`reconcile_incremental`] produced a revision's result
/// (`#incrementalreconcile`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReconcileMode {
    /// The chain ran (or replayed) on the whole document: the document has
    /// no single opaque `agent:exchange` (see [`crate::exchange_index`]).
    #[default]
    Whole,
    /// The chain ran (or replayed) on the skeleton: the document with the
    /// exchange body replaced by its digest, completion cells served from
    /// the exchange index.
    Skeleton,
    /// Only the exchange changed and it added no evidence the chain reads,
    /// so no pass ran: the previous result outside the exchange stands.
    ExchangeOnly,
}

impl ReconcileMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Whole => "whole",
            Self::Skeleton => "skeleton",
            Self::ExchangeOnly => "exchange_only",
        }
    }
}

/// One splice of a revision's changeset (`#incrementalreconcile`): replace
/// `removed_len` bytes at byte `start` with `inserted`. A changeset is a
/// sequence of splices, each addressing the text the splices before it
/// produced (a CRDT text journal's order). Offsets are char boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSplice {
    pub start: usize,
    pub removed_len: usize,
    pub inserted: String,
}

/// Where a revision's changeset came from (`#incrementalreconcile`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChangesetSource {
    /// The common prefix/suffix of each region against the cached revision.
    #[default]
    ByteCompare,
    /// The caller's [`TextSplice`]s, mapped to the regions without comparing
    /// the texts ([`reconcile_incremental_changeset`]).
    Splices,
}

impl ChangesetSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ByteCompare => "byte_compare",
            Self::Splices => "splices",
        }
    }
}

/// [`reconcile_incremental`]'s result and what it reused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    pub reconciled: Reconciled,
    /// `reconciled.document` differs from the revision (decided on the
    /// skeleton, without comparing the exchange, whenever the chain ran or
    /// replayed on one).
    pub changed: bool,
    /// Where the changeset came from.
    pub changeset: ChangesetSource,
    /// Why a caller's changeset was not used (the texts were compared
    /// instead), if one was handed in and refused.
    pub changeset_fallback: Option<&'static str>,
    /// Bytes of the previous revision's `agent:exchange` body the changeset
    /// discovery compared (0 for [`ChangesetSource::Splices`]; a rebuild
    /// reads the whole body, counted in `exchange_bytes_indexed`).
    pub exchange_bytes_compared: usize,
    /// Components (`name` or `name#id`, then `@n` for the n-th repeat) the
    /// revision's changeset touched, added or removed, in document order
    /// (removed ones last). Every component when there is no previous
    /// revision.
    pub dirty_components: Vec<String>,
    /// The chain result was replayed from the cache (same chain input, same
    /// external answers) or kept for an exchange-only revision: no pass ran.
    pub reused: bool,
    /// How the result was produced.
    pub mode: ReconcileMode,
    /// Bytes the chain's input held (the skeleton, or the whole document).
    pub chain_bytes: usize,
    /// `agent:exchange` bytes this revision read to index (the touched
    /// chunks; the whole body only when the layout was rebuilt).
    pub exchange_bytes_indexed: usize,
    /// Why the cached layout was rebuilt from a full parse, if it was.
    pub rebuilt: Option<&'static str>,
    /// Queue component overlays parsed by this call (the rest were reused).
    pub slice_parses: u64,
    /// Whole-document queue overlays parsed by this call.
    pub document_parses: u64,
}

/// The per-document state [`reconcile_incremental`] keeps between revisions.
#[derive(Default)]
pub struct ReconcileCache {
    revision: Option<Revision>,
    memo: Option<ChainMemo>,
}

struct ChainMemo {
    /// The chain's input: the skeleton or the whole document.
    input: Arc<str>,
    trace: Vec<Query>,
    /// The chain's output (on the skeleton for a skeleton run).
    output: Reconciled,
    /// For a skeleton run, the digest's span in `output.document`, where the
    /// revision's own exchange body goes.
    digest_span: Option<std::ops::Range<usize>>,
    /// For a skeleton run, the exchange index's cells revision it read.
    cells_revision: u64,
}

/// One reconciled revision: its text, its component layout, and its
/// exchange index (`#incrementalreconcile`).
struct Revision {
    text: Arc<str>,
    /// `(identity, open_start, close_end)` of every component, in order.
    layout: Vec<(String, usize, usize)>,
    exchange: Option<ExchangeSpan>,
}

/// The `agent:exchange` component the chain treats as opaque.
struct ExchangeSpan {
    /// Start of the opener's line.
    line_start: usize,
    /// The body: from past the opener to the closer.
    body: std::ops::Range<usize>,
    /// Past the closer (and its newline).
    close_end: usize,
    index: crate::exchange_index::ExchangeIndex,
}

/// An [`ExchangeSpan`]'s offsets alone.
struct ExchangeSpanAt {
    line_start: usize,
    body: std::ops::Range<usize>,
    close_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Query {
    IsLive(String, u64, bool),
    ResponseProven(String, bool),
    StruckProvenance(String, bool),
    IsClaimed(String, bool),
}

impl Query {
    fn replays(&self, inputs: &ReconcileInputs<'_>) -> bool {
        match self {
            Self::IsLive(owner, generation, answer) => {
                (inputs.is_live)(owner, *generation) == *answer
            }
            Self::ResponseProven(id, answer) => (inputs.response_proven)(id) == *answer,
            Self::StruckProvenance(work, answer) => (inputs.struck_provenance)(work) == *answer,
            Self::IsClaimed(row, answer) => (inputs.is_claimed)(row) == *answer,
        }
    }
}

impl ReconcileCache {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Every pass of the chain on the whole document (the reference
/// [`reconcile_incremental`] equals).
pub fn reconcile_full(document: &str, inputs: &ReconcileInputs<'_>) -> Reconciled {
    crate::derived_memo::outside_component_scope(|| run_chain(document, inputs))
}

/// What [`queue_sync_response_chain`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueSyncResponses {
    /// The document after every pass (the input when nothing changed).
    pub document: String,
    /// Self-repairs, in the order they ran (`#selfrepaircleanup`).
    pub repairs: Vec<SelfRepair>,
    /// Finished trees struck from the root down (`#treefinish`).
    pub struck_trees: usize,
    /// The generated `Queue markers:` line was refreshed.
    pub legend_refreshed: bool,
}

/// The response passes `agent-doc queue sync` runs, in its order: retained
/// response lifecycle markers, superseded operator blockers, moved follow-up
/// states (`#followupmovedstate`, `#staleguidance`), self-repair,
/// the response-tree sync (`#treefinish`), then the `Queue markers:` legend.
/// One pure entry point, so the command and the tests run the same passes
/// in the same order (`#answeredentry`).
pub fn queue_sync_response_chain(
    document: &str,
    inputs: &ReconcileInputs<'_>,
) -> QueueSyncResponses {
    let normalized =
        agent_doc_markdown_ast::overlay::normalize_generated_response_lifecycle_markers(document);
    let normalized = crate::queue_consume::reconcile_superseded_response_blockers(&normalized)
        .unwrap_or(normalized);
    // `#staleguidance`: the chain's first pass, as in [`run_chain`].
    let normalized = crate::followup_moved_state::reconcile_moved_followup_states(&normalized)
        .map_or(normalized, |(moved, _)| moved);
    let mut out = QueueSyncResponses {
        document: normalized,
        ..QueueSyncResponses::default()
    };
    if let Some((repaired, repairs)) = crate::self_repair::self_repair_document_with_provenance(
        &out.document,
        inputs.is_live,
        inputs.response_proven,
        inputs.struck_provenance,
    ) {
        out.document = repaired;
        out.repairs = repairs;
    }
    if let Some(synced) =
        crate::response_tree_sync::sync_response_trees_striking(&out.document, inputs.is_claimed)
    {
        out.document = synced.document;
        out.struck_trees = synced.struck_trees;
    }
    if let Some(refreshed) =
        agent_doc_document::status_projection::refresh_stale_queue_marker_legend(&out.document)
    {
        out.document = refreshed;
        out.legend_refreshed = true;
    }
    out
}

/// Reconcile revision `next` (see the module docs). `prev` is the revision
/// the caller last saw, when it knows it; the cache keeps the revision it
/// reconciled last, and a `prev` it does not hold starts the cache from
/// `prev`. Byte-identical to [`reconcile_full`]`(next, inputs)`, except for
/// a [`ReconcileMode::ExchangeOnly`] revision, which equals it on the
/// exchange the chain last ran with.
pub fn reconcile_incremental(
    prev: Option<&str>,
    next: &str,
    inputs: &ReconcileInputs<'_>,
    cache: &mut ReconcileCache,
) -> ReconcileReport {
    if let Some(prev) = prev
        && cache
            .revision
            .as_ref()
            .is_none_or(|revision| *revision.text != *prev)
    {
        cache.revision = Some(Revision::build(Arc::from(prev)).0);
    }
    reconcile_advanced(advance(cache.revision.take(), next, None), inputs, cache)
}

/// [`reconcile_incremental`] from the revision's changeset: `changeset`
/// turns the revision the cache reconciled last into `next`
/// (`#incrementalreconcile`). The splices are mapped to the regions before,
/// inside and after `agent:exchange` without comparing the texts; only the
/// inserted bytes and the exchange's opener and closer are read from
/// `next`. A changeset that does not spell `next` (its inserted bytes or
/// its length disagree, or it splits a char), or a cache holding no
/// revision, falls back to comparing the texts
/// ([`ReconcileReport::changeset_fallback`]). Byte-identical to
/// [`reconcile_incremental`]`(None, next, ..)` whenever the changeset is
/// that revision's.
pub fn reconcile_incremental_changeset(
    changeset: &[TextSplice],
    next: &str,
    inputs: &ReconcileInputs<'_>,
    cache: &mut ReconcileCache,
) -> ReconcileReport {
    let prev = cache.revision.take();
    let hunks = match &prev {
        None => Err("no_cached_revision"),
        Some(prev) => changeset_hunks(prev.text.len(), changeset, next),
    };
    let advance = match hunks {
        Ok(hunks) => advance(prev, next, Some(&hunks)),
        Err(reason) => {
            let mut advance = advance(prev, next, None);
            advance.changeset_fallback = Some(reason);
            advance
        }
    };
    reconcile_advanced(advance, inputs, cache)
}

/// Run the chain on an advanced revision and keep it in the cache.
fn reconcile_advanced(
    advance: Advance,
    inputs: &ReconcileInputs<'_>,
    cache: &mut ReconcileCache,
) -> ReconcileReport {
    let revision = advance.revision;
    let report = ReconcileReport {
        dirty_components: advance.dirty,
        exchange_bytes_indexed: advance.indexed,
        exchange_bytes_compared: advance.compared,
        changeset: advance.source,
        changeset_fallback: advance.changeset_fallback,
        rebuilt: advance.rebuilt,
        ..ReconcileReport::default()
    };
    let report = reconcile_revision(
        &revision,
        advance.exchange_only_without_evidence,
        inputs,
        &mut cache.memo,
        report,
    );
    cache.revision = Some(revision);
    report
}

/// The chain's result for `revision` (see [`reconcile_incremental`]).
fn reconcile_revision(
    revision: &Revision,
    exchange_only_without_evidence: bool,
    inputs: &ReconcileInputs<'_>,
    memo: &mut Option<ChainMemo>,
    mut report: ReconcileReport,
) -> ReconcileReport {
    let before = crate::derived_memo::derived_stats();
    let skeleton = revision.exchange.as_ref().and_then(|exchange| {
        exchange
            .index
            .neutral()
            .then(|| (exchange, exchange.index.digest()))
    });
    match skeleton {
        Some((exchange, digest)) => {
            let text = &*revision.text;
            let body = &text[exchange.body.clone()];
            // An exchange-only revision that adds no evidence keeps the last
            // result outside the exchange: no pass runs.
            if exchange_only_without_evidence
                && let Some(memo) = memo.as_ref()
                && let Some(span) = &memo.digest_span
                && memo.trace.iter().all(|query| query.replays(inputs))
            {
                report.reconciled = splice_body(&memo.output, span, body);
                report.changed = changed_on_skeleton(&memo.output, &memo.input, &report, text);
                report.reused = true;
                report.mode = ReconcileMode::ExchangeOnly;
                report.chain_bytes = 0;
                return report;
            }
            let input = format!(
                "{}{digest}{}",
                &text[..exchange.body.start],
                &text[exchange.body.end..]
            );
            report.chain_bytes = input.len();
            report.mode = ReconcileMode::Skeleton;
            if let Some(memo) = memo.as_ref()
                && let Some(span) = &memo.digest_span
                && memo.cells_revision == exchange.index.cells_revision()
                && *memo.input == *input
                && memo.trace.iter().all(|query| query.replays(inputs))
            {
                report.reconciled = splice_body(&memo.output, span, body);
                report.changed = changed_on_skeleton(&memo.output, &memo.input, &report, text);
                report.reused = true;
                return report;
            }
            let cells = std::rc::Rc::new(exchange.index.cells(digest.len()));
            let ((output, trace), tainted) =
                crate::exchange_index::with_exchange_cells(cells, || run_traced(&input, inputs));
            let span = (!tainted)
                .then(|| digest_span(&output.document, &digest))
                .flatten();
            if let Some(span) = span {
                report.reconciled = splice_body(&output, &span, body);
                report.changed = changed_on_skeleton(&output, &input, &report, text);
                *memo = Some(ChainMemo {
                    input: Arc::from(input),
                    trace,
                    output,
                    digest_span: Some(span),
                    cells_revision: exchange.index.cells_revision(),
                });
            } else {
                // A pass changed the digest: the skeleton cannot stand for
                // the document, so the whole document runs.
                report.rebuilt = Some("skeleton_output");
                whole(revision, inputs, memo, &mut report);
            }
        }
        None => whole(revision, inputs, memo, &mut report),
    }
    let after = crate::derived_memo::derived_stats();
    report.slice_parses = after.slice_parses - before.slice_parses;
    report.document_parses = after.document_parses - before.document_parses;
    report
}

/// The chain on the whole document (replayed when the cache holds it).
fn whole(
    revision: &Revision,
    inputs: &ReconcileInputs<'_>,
    cache: &mut Option<ChainMemo>,
    report: &mut ReconcileReport,
) {
    let next = &*revision.text;
    report.mode = ReconcileMode::Whole;
    report.chain_bytes = next.len();
    if let Some(memo) = cache.as_ref()
        && memo.digest_span.is_none()
        && *memo.input == *next
        && memo.trace.iter().all(|query| query.replays(inputs))
    {
        report.reconciled = memo.output.clone();
        report.changed = report.reconciled.document != next;
        report.reused = true;
        return;
    }
    let (reconciled, trace) = run_traced(next, inputs);
    report.reconciled = reconciled.clone();
    report.changed = report.reconciled.document != next;
    *cache = Some(ChainMemo {
        input: revision.text.clone(),
        trace,
        output: reconciled,
        digest_span: None,
        cells_revision: 0,
    });
}

/// Whether a skeleton run's result differs from the revision `text`. The
/// result is the chain's skeleton `output` with the revision's exchange body
/// in place of the digest, and the revision is the chain's `input` skeleton
/// the same way, so equal skeletons mean an unchanged revision without
/// reading the exchange. Unequal ones compare the documents.
fn changed_on_skeleton(
    output: &Reconciled,
    input: &str,
    report: &ReconcileReport,
    text: &str,
) -> bool {
    output.document != input && report.reconciled.document != text
}

/// `output` with the digest at `span` replaced by the exchange `body`.
fn splice_body(output: &Reconciled, span: &std::ops::Range<usize>, body: &str) -> Reconciled {
    let document = &output.document;
    let mut spliced = String::with_capacity(document.len() - span.len() + body.len());
    spliced.push_str(&document[..span.start]);
    spliced.push_str(body);
    spliced.push_str(&document[span.end..]);
    Reconciled {
        document: spliced,
        ..output.clone()
    }
}

/// The digest's span in the chain's output skeleton: its single exchange
/// body, still exactly the digest. `None` when a pass changed it.
fn digest_span(output: &str, digest: &str) -> Option<std::ops::Range<usize>> {
    let components = agent_doc_element::element::parse(output).ok()?;
    let mut exchanges = components
        .iter()
        .filter(|component| component.name == "exchange");
    let exchange = exchanges.next()?;
    if exchanges.next().is_some() {
        return None;
    }
    let span = exchange.open_end..exchange.close_start;
    (output.get(span.clone())? == digest).then_some(span)
}

/// Run the chain on `input`, recording every external answer it read.
fn run_traced(input: &str, inputs: &ReconcileInputs<'_>) -> (Reconciled, Vec<Query>) {
    let trace = RefCell::new(Vec::new());
    let is_live = |owner: &str, generation: u64| {
        let answer = (inputs.is_live)(owner, generation);
        trace
            .borrow_mut()
            .push(Query::IsLive(owner.to_string(), generation, answer));
        answer
    };
    let response_proven = |id: &str| {
        let answer = (inputs.response_proven)(id);
        trace
            .borrow_mut()
            .push(Query::ResponseProven(id.to_string(), answer));
        answer
    };
    let struck_provenance = |work: &str| {
        let answer = (inputs.struck_provenance)(work);
        trace
            .borrow_mut()
            .push(Query::StruckProvenance(work.to_string(), answer));
        answer
    };
    let is_claimed = |row: &str| {
        let answer = (inputs.is_claimed)(row);
        trace
            .borrow_mut()
            .push(Query::IsClaimed(row.to_string(), answer));
        answer
    };
    let traced = ReconcileInputs {
        is_live: &is_live,
        response_proven: &response_proven,
        struck_provenance: &struck_provenance,
        is_claimed: &is_claimed,
    };
    let reconciled = crate::derived_memo::with_component_scope(|| run_chain(input, &traced));
    (reconciled, trace.into_inner())
}

fn run_chain(document: &str, inputs: &ReconcileInputs<'_>) -> Reconciled {
    let mut out = Reconciled {
        document: document.to_string(),
        ..Reconciled::default()
    };
    if let Some((moved, rows)) =
        crate::followup_moved_state::reconcile_moved_followup_states(&out.document)
    {
        out.document = moved;
        out.moved_followup_rows = rows;
    }
    if let Some((repaired, repairs)) = crate::self_repair::self_repair_document_with_provenance(
        &out.document,
        inputs.is_live,
        inputs.response_proven,
        inputs.struck_provenance,
    ) {
        out.document = repaired;
        out.repairs = repairs;
    }
    if let Some(synced) =
        crate::response_tree_sync::sync_response_trees_striking(&out.document, inputs.is_claimed)
    {
        out.document = synced.document;
        out.response_trees = Some(ResponseTreeCounts {
            slimmed: synced.slimmed,
            guided: synced.guided,
            struck_trees: synced.struck_trees,
            indicators: synced.indicators,
        });
    }
    if let Some(refreshed) =
        agent_doc_document::status_projection::refresh_stale_queue_marker_legend(&out.document)
    {
        out.document = refreshed;
        out.legend_refreshed = true;
    }
    // A pass that cannot parse the document leaves it as it is, as the
    // callers of these passes do.
    if let Ok(Some(counted)) = crate::open_count::reconcile_open_counts(&out.document) {
        out.document = counted;
        out.open_counts_changed = true;
    }
    if let Ok(Some(digest)) = crate::loose_ends::reconcile_loose_ends(&out.document) {
        out.document = digest;
        out.loose_ends_changed = true;
    }
    out
}

/// `(identity, open_start, close_end)` of `components`, in order.
fn layout_of(components: &[agent_doc_element::element::Component]) -> Vec<(String, usize, usize)> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    components
        .iter()
        .map(|component| {
            let base = match component.attrs.get("id") {
                Some(id) => format!("{}#{id}", component.name),
                None => component.name.clone(),
            };
            let occurrence = seen.entry(base.clone()).or_insert(0);
            let identity = if *occurrence == 0 {
                base
            } else {
                format!("{base}@{occurrence}")
            };
            *occurrence += 1;
            (identity, component.open_start, component.close_end)
        })
        .collect()
}

/// Start of the line holding byte `at`.
fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |newline| newline + 1)
}

impl Revision {
    /// Parse `text` from scratch: its layout and, when it has one opaque
    /// exchange, the exchange index (a full read of the exchange). Returns
    /// the revision and the exchange bytes read.
    fn build(text: Arc<str>) -> (Self, usize) {
        let Ok(components) = agent_doc_element::element::parse(&text) else {
            return (
                Self {
                    text,
                    layout: Vec::new(),
                    exchange: None,
                },
                0,
            );
        };
        let layout = layout_of(&components);
        let exchange = opaque_exchange(&text, &components);
        let indexed = exchange.as_ref().map_or(0, |exchange| exchange.body.len());
        (
            Self {
                text,
                layout,
                exchange,
            },
            indexed,
        )
    }
}

/// The document's `agent:exchange` when the chain can treat it as opaque:
/// exactly one, holding no other component, its opener ending a column-0
/// heading or comment line and its closer starting a line.
fn opaque_exchange(
    text: &str,
    components: &[agent_doc_element::element::Component],
) -> Option<ExchangeSpan> {
    let mut exchanges = components
        .iter()
        .filter(|component| component.name == "exchange");
    let exchange = exchanges.next()?;
    if exchanges.next().is_some() {
        return None;
    }
    let body = exchange.open_end..exchange.close_start;
    if components
        .iter()
        .any(|component| component.open_start >= body.start && component.close_end <= body.end)
    {
        return None;
    }
    let line = line_start(text, exchange.open_start);
    let opener = &text[line..body.start];
    if !(opener.starts_with('#') || opener.starts_with("<!--")) || !opener.ends_with('\n') {
        return None;
    }
    if line_start(text, body.end) != body.end {
        return None;
    }
    Some(ExchangeSpan {
        line_start: line,
        index: crate::exchange_index::ExchangeIndex::build(&text[body.clone()]),
        body,
        close_end: exchange.close_end,
    })
}

/// What [`advance`] found for one revision.
struct Advance {
    revision: Revision,
    dirty: Vec<String>,
    indexed: usize,
    /// Old exchange-body bytes the changeset discovery compared.
    compared: usize,
    source: ChangesetSource,
    changeset_fallback: Option<&'static str>,
    rebuilt: Option<&'static str>,
    /// Only the exchange body changed, and the change added no evidence the
    /// chain reads.
    exchange_only_without_evidence: bool,
}

/// Common prefix length of `a` and `b` from `from` on (both equal before
/// it), snapped to a char boundary of both.
fn common_prefix(a: &str, b: &str, from: usize) -> usize {
    let (left, right) = (&a.as_bytes()[from..], &b.as_bytes()[from..]);
    let mut prefix = from
        + left
            .iter()
            .zip(right)
            .take_while(|(left, right)| left == right)
            .count();
    while prefix > 0 && (!a.is_char_boundary(prefix) || !b.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    prefix
}

/// Common suffix length of `a` and `b`, at most `max`, snapped so both
/// suffix starts are char boundaries.
fn common_suffix(a: &str, b: &str, max: usize) -> usize {
    let mut suffix = a
        .as_bytes()
        .iter()
        .rev()
        .zip(b.as_bytes().iter().rev())
        .take(max)
        .take_while(|(left, right)| left == right)
        .count();
    while suffix > 0
        && (!a.is_char_boundary(a.len() - suffix) || !b.is_char_boundary(b.len() - suffix))
    {
        suffix -= 1;
    }
    suffix
}

/// The one splice turning `a` into `b`: `(range in a, range in b)`, `None`
/// when they are equal.
fn splice(a: &str, b: &str) -> Option<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    if a == b {
        return None;
    }
    let prefix = common_prefix(a, b, 0);
    let suffix = common_suffix(a, b, a.len().min(b.len()) - prefix);
    Some((prefix..a.len() - suffix, prefix..b.len() - suffix))
}

/// Identities of `layout` entries a changed range touches. An empty range
/// touches the component strictly around it.
fn touched(layout: &[(String, usize, usize)], range: &std::ops::Range<usize>) -> Vec<String> {
    layout
        .iter()
        .filter(|(_, start, end)| {
            if range.is_empty() {
                *start < range.start && range.start < *end
            } else {
                range.start < *end && range.end > *start
            }
        })
        .map(|(identity, _, _)| identity.clone())
        .collect()
}

/// Dirty components: those the changes touch in either revision, and those
/// added or removed, in `next` order then removed ones.
fn dirty_between(
    prev: &[(String, usize, usize)],
    next: &[(String, usize, usize)],
    prev_ranges: &[std::ops::Range<usize>],
    next_ranges: &[std::ops::Range<usize>],
) -> Vec<String> {
    let mut hit: std::collections::HashSet<String> = std::collections::HashSet::new();
    for range in prev_ranges {
        hit.extend(touched(prev, range));
    }
    for range in next_ranges {
        hit.extend(touched(next, range));
    }
    let before: std::collections::HashSet<&str> = prev
        .iter()
        .map(|(identity, _, _)| identity.as_str())
        .collect();
    let after: std::collections::HashSet<&str> = next
        .iter()
        .map(|(identity, _, _)| identity.as_str())
        .collect();
    let mut out: Vec<String> = next
        .iter()
        .filter(|(identity, _, _)| hit.contains(identity) || !before.contains(identity.as_str()))
        .map(|(identity, _, _)| identity.clone())
        .collect();
    out.extend(
        prev.iter()
            .filter(|(identity, _, _)| !after.contains(identity.as_str()))
            .map(|(identity, _, _)| identity.clone()),
    );
    out
}

/// One changed range of a changeset: `old` bytes of the previous revision
/// became `new` bytes of the next one. Hunks are sorted and disjoint.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hunk {
    old: std::ops::Range<usize>,
    new: std::ops::Range<usize>,
}

/// A piece of the text a changeset builds: a range of the previous
/// revision, or inserted text.
enum Piece<'a> {
    Old(std::ops::Range<usize>),
    New(std::borrow::Cow<'a, str>),
}

impl Piece<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Old(range) => range.len(),
            Self::New(text) => text.len(),
        }
    }
}

/// The hunks `changeset` makes of a previous revision of `old_len` bytes,
/// checked against `next`: each inserted run must be `next`'s bytes there
/// and the result must be `next`'s length. Reads only the inserted bytes of
/// `next`. `Err` names why the changeset cannot stand for the revision.
fn changeset_hunks(
    old_len: usize,
    changeset: &[TextSplice],
    next: &str,
) -> Result<Vec<Hunk>, &'static str> {
    let mut pieces: Vec<Piece<'_>> = vec![Piece::Old(0..old_len)];
    let mut len = old_len;
    for splice in changeset {
        let end = splice
            .start
            .checked_add(splice.removed_len)
            .filter(|end| *end <= len)
            .ok_or("splice_out_of_range")?;
        // Cut the pieces at `start` and `end`, replacing what lies between.
        let mut out: Vec<Piece<'_>> = Vec::with_capacity(pieces.len() + 2);
        let mut at = 0usize;
        let mut placed = false;
        for piece in pieces {
            let piece_len = piece.len();
            let (piece_start, piece_end) = (at, at + piece_len);
            at = piece_end;
            let before = splice.start.clamp(piece_start, piece_end) - piece_start;
            let after = end.clamp(piece_start, piece_end) - piece_start;
            let place = !placed && piece_start <= splice.start && splice.start <= piece_end;
            if !place && (before == piece_len || after == 0) {
                // Wholly before the splice or wholly after it.
                out.push(piece);
                continue;
            }
            let (head, tail) = split_piece(piece, before, after)?;
            out.extend(head);
            if place {
                if !splice.inserted.is_empty() {
                    out.push(Piece::New(std::borrow::Cow::Borrowed(&splice.inserted)));
                }
                placed = true;
            }
            out.extend(tail);
        }
        pieces = out;
        len = len - splice.removed_len + splice.inserted.len();
    }
    if len != next.len() {
        return Err("length_mismatch");
    }
    let mut hunks = Vec::new();
    let (mut old_at, mut new_at) = (0usize, 0usize);
    let mut open: Option<(usize, usize)> = None;
    for piece in &pieces {
        match piece {
            Piece::Old(range) if range.is_empty() => {}
            Piece::Old(range) => {
                if range.start != old_at && open.is_none() {
                    open = Some((old_at, new_at));
                }
                if let Some((old_start, new_start)) = open.take() {
                    hunks.push(Hunk {
                        old: old_start..range.start,
                        new: new_start..new_at,
                    });
                }
                old_at = range.end;
                new_at += range.len();
            }
            Piece::New(text) => {
                if open.is_none() {
                    open = Some((old_at, new_at));
                }
                let end = new_at + text.len();
                if next.as_bytes().get(new_at..end) != Some(text.as_bytes()) {
                    return Err("inserted_bytes_mismatch");
                }
                new_at = end;
            }
        }
    }
    if old_at != old_len && open.is_none() {
        open = Some((old_at, new_at));
    }
    if let Some((old_start, new_start)) = open {
        hunks.push(Hunk {
            old: old_start..old_len,
            new: new_start..new_at,
        });
    }
    if hunks
        .iter()
        .any(|hunk| !next.is_char_boundary(hunk.new.start) || !next.is_char_boundary(hunk.new.end))
    {
        return Err("splice_splits_char");
    }
    Ok(hunks)
}

/// `piece`'s bytes before `before` and from `after` on (`before <= after`),
/// moving the piece when one side is all of it.
fn split_piece(
    piece: Piece<'_>,
    before: usize,
    after: usize,
) -> Result<(Option<Piece<'_>>, Option<Piece<'_>>), &'static str> {
    let len = piece.len();
    if before == len {
        return Ok(((len > 0).then_some(piece), None));
    }
    if after == 0 {
        return Ok((None, (len > 0).then_some(piece)));
    }
    Ok(match piece {
        Piece::Old(range) => (
            (before > 0).then(|| Piece::Old(range.start..range.start + before)),
            (after < len).then(|| Piece::Old(range.start + after..range.end)),
        ),
        Piece::New(text) => {
            let part = |from: usize, to: usize| -> Result<Option<Piece<'static>>, &'static str> {
                if from >= to {
                    return Ok(None);
                }
                let part = text.get(from..to).ok_or("splice_splits_char")?;
                Ok(Some(Piece::New(std::borrow::Cow::Owned(part.to_string()))))
            };
            (part(0, before)?, part(after, len)?)
        }
    })
}

/// Bytes of `body` inside `range`.
fn overlap(range: std::ops::Range<usize>, body: &std::ops::Range<usize>) -> usize {
    range
        .end
        .min(body.end)
        .saturating_sub(range.start.max(body.start))
}

/// Where the exchange sits in `next` and the changeset around it, relative
/// to the previous revision's exchange (see [`advance`]).
struct Regions {
    line_start: usize,
    body_start: usize,
    body_end: usize,
    close_end: usize,
    /// Changed ranges before the exchange's opener line (absolute).
    head: Vec<Hunk>,
    /// The changed range of the body, relative to each revision's body.
    body: Option<Hunk>,
    /// Changed ranges after the exchange's closer (absolute).
    tail: Vec<Hunk>,
    compared: usize,
}

/// [`Regions`] from comparing `old` and `next`: the common prefix/suffix of
/// the documents, then of each region. `Err` is a rebuild reason.
fn regions_by_compare(
    old: &str,
    next: &str,
    exchange: &ExchangeSpanAt,
) -> Result<Regions, &'static str> {
    let prefix = common_prefix(old, next, 0);
    let suffix = common_suffix(old, next, old.len().min(next.len()) - prefix);
    let mut compared = overlap(0..prefix + 1, &exchange.body)
        + overlap(old.len() - suffix.min(old.len())..old.len(), &exchange.body);
    let old_diff_end = old.len() - suffix;
    let opener = &old[exchange.line_start..exchange.body.start];
    let closer = &old[exchange.body.end..exchange.close_end];
    // The opener and closer in `next`.
    let line_start = if prefix >= exchange.body.start {
        exchange.line_start
    } else if prefix <= exchange.line_start {
        match next[prefix..].find(opener) {
            Some(at) => prefix + at,
            None => return Err("opener_moved"),
        }
    } else {
        return Err("opener_edited");
    };
    let body_start = line_start + opener.len();
    let body_end = if old_diff_end <= exchange.body.end {
        next.len() - (old.len() - exchange.body.end)
    } else if old_diff_end >= exchange.close_end {
        match next
            .get(body_start..next.len() - suffix)
            .and_then(|region| region.rfind(closer))
        {
            Some(at) => body_start + at,
            None => return Err("closer_moved"),
        }
    } else {
        return Err("closer_edited");
    };
    if body_end < body_start || next.get(body_end..body_end + closer.len()) != Some(closer) {
        return Err("closer_moved");
    }
    let close_end = body_end + closer.len();
    let hunk = |pair: Option<(std::ops::Range<usize>, std::ops::Range<usize>)>,
                old_base: usize,
                new_base: usize| {
        pair.map(|(a, b)| Hunk {
            old: a.start + old_base..a.end + old_base,
            new: b.start + new_base..b.end + new_base,
        })
    };
    let head = hunk(
        splice(&old[..exchange.line_start], &next[..line_start]),
        0,
        0,
    );
    let tail = hunk(
        splice(&old[exchange.close_end..], &next[close_end..]),
        exchange.close_end,
        close_end,
    );
    let old_body = &old[exchange.body.clone()];
    let new_body = &next[body_start..body_end];
    let body = hunk(splice(old_body, new_body), 0, 0);
    compared += match &body {
        Some(hunk) => old_body.len() - hunk.old.len(),
        None => old_body.len(),
    };
    Ok(Regions {
        line_start,
        body_start,
        body_end,
        close_end,
        head: head.into_iter().collect(),
        body,
        tail: tail.into_iter().collect(),
        compared: compared.min(2 * old_body.len()),
    })
}

/// [`Regions`] from a changeset's hunks, reading only the opener and closer
/// of `next`. `Err(Ok(reason))` is a rebuild reason (the changeset edits the
/// opener or closer); `Err(Err(reason))` means the hunks do not place the
/// exchange in `next`, so the texts are compared instead.
fn regions_by_hunks(
    old: &str,
    next: &str,
    exchange: &ExchangeSpanAt,
    hunks: &[Hunk],
) -> Result<Regions, Result<&'static str, &'static str>> {
    let (mut head, mut body, mut tail) = (Vec::new(), Vec::<&Hunk>::new(), Vec::new());
    for hunk in hunks {
        if hunk.old.end <= exchange.line_start {
            head.push(hunk.clone());
        } else if hunk.old.start >= exchange.body.start && hunk.old.end <= exchange.body.end {
            body.push(hunk);
        } else if hunk.old.start >= exchange.close_end {
            tail.push(hunk.clone());
        } else if hunk.old.start < exchange.body.start {
            return Err(Ok("opener_edited"));
        } else {
            return Err(Ok("closer_edited"));
        }
    }
    let delta = |hunk: &Hunk| hunk.new.len() as isize - hunk.old.len() as isize;
    let head_delta: isize = head.iter().map(delta).sum();
    let body_delta: isize = body.iter().map(|hunk| delta(hunk)).sum();
    let opener = &old[exchange.line_start..exchange.body.start];
    let closer = &old[exchange.body.end..exchange.close_end];
    let line_start = (exchange.line_start as isize + head_delta) as usize;
    let body_start = line_start + opener.len();
    let body_end = (exchange.body.end as isize + head_delta + body_delta) as usize;
    let close_end = body_end + closer.len();
    if next.get(line_start..body_start) != Some(opener)
        || next.get(body_end..close_end) != Some(closer)
    {
        return Err(Err("exchange_not_placed"));
    }
    let body = match (body.first(), body.last()) {
        (Some(first), Some(last)) => Some(Hunk {
            old: first.old.start - exchange.body.start..last.old.end - exchange.body.start,
            new: first.new.start - body_start..last.new.end - body_start,
        }),
        _ => None,
    };
    Ok(Regions {
        line_start,
        body_start,
        body_end,
        close_end,
        head,
        body,
        tail,
        compared: 0,
    })
}

/// Move `prev` to `next` from the changeset between them
/// (`#incrementalreconcile`). The changeset is the caller's `hunks` when it
/// has them, otherwise up to three splices found by comparing the texts:
/// before the exchange, inside its body, after it (each the common
/// prefix/suffix of its region). Only touched exchange chunks are read; the
/// layout outside comes from parsing the skeleton. Anything that may move a
/// component boundary in a way the cache cannot follow rebuilds from a full
/// parse.
fn advance(prev: Option<Revision>, next: &str, hunks: Option<&[Hunk]>) -> Advance {
    let source = if hunks.is_some() {
        ChangesetSource::Splices
    } else {
        ChangesetSource::ByteCompare
    };
    let rebuild = |prev: Option<&Revision>, reason: &'static str| {
        let (revision, indexed) = Revision::build(Arc::from(next));
        let dirty = match prev {
            None => revision
                .layout
                .iter()
                .map(|(identity, _, _)| identity.clone())
                .collect(),
            Some(prev) => {
                let (old_ranges, new_ranges): (Vec<_>, Vec<_>) = match hunks {
                    Some(hunks) => hunks
                        .iter()
                        .map(|hunk| (hunk.old.clone(), hunk.new.clone()))
                        .unzip(),
                    None => splice(&prev.text, next).into_iter().unzip(),
                };
                dirty_between(&prev.layout, &revision.layout, &old_ranges, &new_ranges)
            }
        };
        Advance {
            revision,
            dirty,
            indexed,
            compared: 0,
            source,
            changeset_fallback: None,
            rebuilt: Some(reason),
            exchange_only_without_evidence: false,
        }
    };
    let Some(prev) = prev else {
        return rebuild(None, "first_revision");
    };
    let unchanged = match hunks {
        Some(hunks) => hunks.is_empty(),
        None => *prev.text == *next,
    };
    if unchanged {
        let compared = match (hunks, &prev.exchange) {
            (None, Some(exchange)) => exchange.body.len(),
            _ => 0,
        };
        return Advance {
            revision: prev,
            dirty: Vec::new(),
            indexed: 0,
            compared,
            source,
            changeset_fallback: None,
            rebuilt: None,
            exchange_only_without_evidence: false,
        };
    }
    let Some(exchange) = &prev.exchange else {
        return rebuild(Some(&prev), "no_opaque_exchange");
    };
    let text = prev.text.clone();
    let old = &*text;
    let exchange = ExchangeSpanAt {
        line_start: exchange.line_start,
        body: exchange.body.clone(),
        close_end: exchange.close_end,
    };
    let regions = match hunks {
        Some(hunks) => match regions_by_hunks(old, next, &exchange, hunks) {
            Ok(regions) => regions,
            Err(Ok(reason)) => return rebuild(Some(&prev), reason),
            Err(Err(fallback)) => {
                let mut advance = advance(Some(prev), next, None);
                advance.changeset_fallback = Some(fallback);
                return advance;
            }
        },
        None => match regions_by_compare(old, next, &exchange) {
            Ok(regions) => regions,
            Err(reason) => return rebuild(Some(&prev), reason),
        },
    };
    let Regions {
        line_start,
        body_start,
        body_end,
        close_end,
        head,
        body,
        tail,
        compared,
    } = regions;
    let closer_len = exchange.close_end - exchange.body.end;

    let mut prev = prev;
    let old_span = exchange;
    let mut exchange = prev.exchange.take().expect("checked above");
    let mut indexed = 0;
    let mut evidence = false;
    if let Some(Hunk {
        old: removed,
        new: inserted,
    }) = &body
    {
        let delta =
            exchange
                .index
                .apply(removed.clone(), inserted.len(), &next[body_start..body_end]);
        indexed = delta.added_bytes();
        if delta.structural() {
            prev.exchange = Some(exchange);
            let mut advance = rebuild(Some(&prev), "exchange_structure");
            advance.compared = compared;
            return advance;
        }
        evidence = delta.adds_evidence();
    }
    let shift = |at: usize, from: usize, delta: isize| {
        if at >= from {
            (at as isize + delta) as usize
        } else {
            at
        }
    };
    let old_ranges: Vec<std::ops::Range<usize>> =
        head.iter()
            .map(|hunk| hunk.old.clone())
            .chain(body.as_ref().map(|hunk| {
                hunk.old.start + old_span.body.start..hunk.old.end + old_span.body.start
            }))
            .chain(tail.iter().map(|hunk| hunk.old.clone()))
            .collect();
    let new_ranges: Vec<std::ops::Range<usize>> = head
        .iter()
        .map(|hunk| hunk.new.clone())
        .chain(
            body.as_ref()
                .map(|hunk| hunk.new.start + body_start..hunk.new.end + body_start),
        )
        .chain(tail.iter().map(|hunk| hunk.new.clone()))
        .collect();
    let layout: Vec<(String, usize, usize)> = if head.is_empty() && tail.is_empty() {
        // Only the body changed: every component keeps its bytes except
        // those holding the body, whose ends move.
        let delta = (body_end - body_start) as isize - old_span.body.len() as isize;
        prev.layout
            .iter()
            .map(|(identity, start, end)| {
                (
                    identity.clone(),
                    *start,
                    shift(*end, old_span.body.end, delta),
                )
            })
            .collect()
    } else {
        // The outside changed: parse the skeleton (the outside around the
        // digest), whose layout outside equals the document's.
        let digest = exchange.index.digest();
        let skeleton = format!("{}{digest}{}", &next[..body_start], &next[body_end..]);
        let skeleton_layout = |prev: &mut Revision, exchange: ExchangeSpan| {
            prev.exchange = Some(exchange);
            let mut advance = rebuild(Some(prev), "skeleton_layout");
            advance.compared = compared;
            advance
        };
        let Ok(components) = agent_doc_element::element::parse(&skeleton) else {
            return skeleton_layout(&mut prev, exchange);
        };
        let digest_end = body_start + digest.len();
        let Some(found) = opaque_exchange_span(&skeleton, &components) else {
            return skeleton_layout(&mut prev, exchange);
        };
        if found != (line_start, body_start..digest_end, digest_end + closer_len) {
            return skeleton_layout(&mut prev, exchange);
        }
        let delta = (body_end - body_start) as isize - digest.len() as isize;
        layout_of(&components)
            .into_iter()
            .map(|(identity, start, end)| {
                (
                    identity,
                    shift(start, digest_end, delta),
                    shift(end, digest_end, delta),
                )
            })
            .collect()
    };
    let dirty = dirty_between(&prev.layout, &layout, &old_ranges, &new_ranges);
    exchange.line_start = line_start;
    exchange.body = body_start..body_end;
    exchange.close_end = close_end;
    Advance {
        exchange_only_without_evidence: head.is_empty() && tail.is_empty() && !evidence,
        revision: Revision {
            text: Arc::from(next),
            layout,
            exchange: Some(exchange),
        },
        dirty,
        indexed,
        compared,
        source,
        changeset_fallback: None,
        rebuilt: None,
    }
}

/// `(line_start, body, close_end)` of the skeleton's exchange when it is
/// opaque (see [`opaque_exchange`]), without indexing it.
fn opaque_exchange_span(
    text: &str,
    components: &[agent_doc_element::element::Component],
) -> Option<(usize, std::ops::Range<usize>, usize)> {
    let mut exchanges = components
        .iter()
        .filter(|component| component.name == "exchange");
    let exchange = exchanges.next()?;
    if exchanges.next().is_some() {
        return None;
    }
    let body = exchange.open_end..exchange.close_start;
    if components
        .iter()
        .any(|component| component.open_start >= body.start && component.close_end <= body.end)
    {
        return None;
    }
    Some((
        line_start(text, exchange.open_start),
        body,
        exchange.close_end,
    ))
}

#[cfg(test)]
#[path = "reconcile_incremental_tests.rs"]
mod tests;
