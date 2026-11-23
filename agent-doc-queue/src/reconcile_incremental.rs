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
//! - Derived state is cached per component revision, keyed by the exact
//!   bytes of the component ([`crate::derived_memo`]): a queue component's
//!   item overlay is parsed again only when that component changed, and
//!   `agent:exchange` is never parsed for queue items. Whole-revision
//!   derivations (the queue tree, the follow-up row index) are shared by
//!   every pass of the chain instead of being rebuilt by each one.
//! - A revision the cache already reconciled, whose external answers (claim
//!   ledger, response index, committed copies) are unchanged, returns the
//!   recorded result without running a pass: every external query the run
//!   made is replayed and must answer the same.
//! - The report names the components that changed since the previous
//!   revision ([`ReconcileReport::dirty_components`]), so a caller can log or
//!   schedule by them.
//!
//! # Exactness
//!
//! The result is byte-identical to [`reconcile_full`], which runs the same
//! passes with the component layer off. Every cache is keyed by exact bytes
//! (never a CRDT state vector, which does not change on a delete), so an entry
//! can never be stale; where the document's shape does not guarantee a
//! component-scoped derivation equals the whole-document one, the
//! whole-document derivation runs (see [`crate::derived_memo`]).
//!
//! The passes are pure: the closures in [`ReconcileInputs`] are their only
//! inputs besides the document. A caller that reads the claim ledger or the
//! archive index must hand the current answers on every call.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
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

/// [`reconcile_incremental`]'s result and what it reused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    pub reconciled: Reconciled,
    /// Components (`name` or `name#id`, then `@n` for the n-th repeat) whose
    /// bytes differ from the previous revision, added or removed, in
    /// document order (removed ones last). Every component when there is no
    /// previous revision.
    pub dirty_components: Vec<String>,
    /// The revision was already reconciled and replayed from the cache.
    pub reused: bool,
    /// Queue component overlays parsed by this call (the rest were reused).
    pub slice_parses: u64,
    /// Whole-document queue overlays parsed by this call.
    pub document_parses: u64,
}

/// The per-document state [`reconcile_incremental`] keeps between revisions.
#[derive(Default)]
pub struct ReconcileCache {
    layout: Vec<(String, u64)>,
    memo: Option<ChainMemo>,
}

struct ChainMemo {
    input: Arc<str>,
    trace: Vec<Query>,
    output: Reconciled,
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
/// response lifecycle markers, superseded operator blockers, self-repair,
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
/// the caller last saw, when it knows it; without it the cache's previous
/// revision names the dirty components. Byte-identical to
/// [`reconcile_full`]`(next, inputs)`.
pub fn reconcile_incremental(
    prev: Option<&str>,
    next: &str,
    inputs: &ReconcileInputs<'_>,
    cache: &mut ReconcileCache,
) -> ReconcileReport {
    let layout = component_layout(next);
    let previous = match prev {
        Some(prev) => component_layout(prev),
        None => std::mem::take(&mut cache.layout),
    };
    let dirty_components = dirty(&previous, &layout);
    cache.layout = layout;

    if let Some(memo) = &cache.memo
        && *memo.input == *next
        && memo.trace.iter().all(|query| query.replays(inputs))
    {
        return ReconcileReport {
            reconciled: memo.output.clone(),
            dirty_components,
            reused: true,
            slice_parses: 0,
            document_parses: 0,
        };
    }

    let before = crate::derived_memo::derived_stats();
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
    let reconciled = crate::derived_memo::with_component_scope(|| run_chain(next, &traced));
    let after = crate::derived_memo::derived_stats();
    cache.memo = Some(ChainMemo {
        input: Arc::from(next),
        trace: trace.into_inner(),
        output: reconciled.clone(),
    });
    ReconcileReport {
        reconciled,
        dirty_components,
        reused: false,
        slice_parses: after.slice_parses - before.slice_parses,
        document_parses: after.document_parses - before.document_parses,
    }
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

/// `(identity, content hash)` of every component, in document order.
fn component_layout(document: &str) -> Vec<(String, u64)> {
    let Ok(components) = agent_doc_element::element::parse(document) else {
        return Vec::new();
    };
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
            let mut hasher = DefaultHasher::new();
            document[component.open_start..component.close_end].hash(&mut hasher);
            (identity, hasher.finish())
        })
        .collect()
}

fn dirty(previous: &[(String, u64)], next: &[(String, u64)]) -> Vec<String> {
    let before: std::collections::HashMap<&str, u64> = previous
        .iter()
        .map(|(identity, hash)| (identity.as_str(), *hash))
        .collect();
    let after: std::collections::HashSet<&str> =
        next.iter().map(|(identity, _)| identity.as_str()).collect();
    let mut out: Vec<String> = next
        .iter()
        .filter(|(identity, hash)| before.get(identity.as_str()) != Some(hash))
        .map(|(identity, _)| identity.clone())
        .collect();
    out.extend(
        previous
            .iter()
            .filter(|(identity, _)| !after.contains(identity.as_str()))
            .map(|(identity, _)| identity.clone()),
    );
    out
}

#[cfg(test)]
#[path = "reconcile_incremental_tests.rs"]
mod tests;
