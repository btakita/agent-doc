//! `#incrementalreconcile`: derived queue state cached per component revision.
//!
//! The queue and closeout passes ([`crate::self_repair`],
//! [`crate::response_tree_sync`], [`crate::finished_tree`],
//! [`crate::tree_indicator`], [`crate::open_count`], [`crate::loose_ends`],
//! [`crate::response_remaining`], [`crate::followup_moved_state`]) each
//! re-derived the same whole-document state: the queue item overlay
//! (`item_nodes_all(document, "queue")`, a tree-sitter parse of the whole
//! document, `agent:exchange` included), the queue tree, and the follow-up row
//! index. On a 6.5k-line session document that was ~1.4 s per closeout, almost
//! all of it parsing the exchange to throw its items away.
//!
//! Two layers make that work follow the components that changed:
//!
//! - **Per document revision** (always on). Each derivation is memoized by the
//!   exact document bytes ([`agent_doc_element::content_memo`]), so the passes
//!   of one chain share one derivation per revision. A changed byte is a
//!   different key, so an entry can never be stale. The completion-cell
//!   index a hand-back lookup reads is memoized the same way, so a pass that
//!   looks up many rows' hand-backs reads the exchange once per revision.
//! - **Per component revision** (inside [`with_component_scope`], which
//!   [`crate::reconcile_incremental`] enters). The queue items are assembled
//!   from each queue component's own overlay, memoized by the exact bytes of
//!   that component (its opener line through its closer line). Only a
//!   component whose bytes changed is parsed again, and `agent:exchange` is
//!   never parsed for queue items at all. A CRDT state vector is not a key:
//!   it does not change on a delete.
//!
//! The component layer is used only when the document's shape guarantees the
//! slice overlay equals the whole-document overlay restricted to that
//! component; otherwise the whole-document derivation runs, as before:
//!
//! - every queue component's opener line starts at column 0 with `<!--` or
//!   `#`, follows a blank line (or starts the document), and its closer line
//!   starts at column 0 with `<!--`. A blank line ends every open paragraph,
//!   list item, block quote and HTML block of kind 6/7, and a column-0 marker
//!   line can be neither lazy continuation nor indented code, so the block
//!   parse of the component is the same standalone as in place;
//! - the slice overlay is exactly one `queue` component that ends at the slice
//!   end (a fence or comment left open inside the component would run past
//!   its closer in the document, and does not close the slice either);
//! - no line outside the queue slices opens a runnable component (a queue the
//!   element parser does not see, such as one nested or quoted elsewhere);
//! - no HTML block of kind 1 (`<pre`, `<script`, `<style`, `<textarea`), which
//!   a blank line does not end, starts anywhere in the document;
//! - queue slices do not overlap.
//!
//! The component layer returns the overlay [`Item`]s only, never node keys
//! (a node key numbers every overlay component in the document); a pass that
//! mutates by node key reads the keys from the whole-document overlay, which
//! it needs only when it has something to change.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use agent_doc_element::content_memo::{ContentMemo, memoized};
use agent_doc_markdown_ast::overlay::Item;

/// Documents at least this large are memoized.
const MIN_DOCUMENT_BYTES: usize = 4 * 1024;
/// Revisions each per-document memo keeps: the chain's current document and
/// the few before it. Each memo holds its own copy of its keys, so the
/// budget bounds this thread's memory per memo.
const DOCUMENT_ENTRIES: usize = 4;
const DOCUMENT_KEY_BYTES: usize = 8 * 1024 * 1024;
/// Every non-empty component slice is memoized, so an unchanged component is
/// never parsed again.
const MIN_SLICE_BYTES: usize = 1;

type Items = Option<Arc<Vec<Item>>>;
type SliceItems = Option<Arc<Vec<Item>>>;
type TreeNodes = Arc<Vec<crate::queue_tree::QueueTreeNode>>;
type QueueNodes = Arc<Vec<agent_doc_markdown_ast::mutations::MutationItemNode>>;
type Evidence = Arc<Vec<agent_doc_document::subagent_completion::CompletionEvidence>>;
/// One text revision's completion responses, filled per key as they are read.
type Responses = std::rc::Rc<RefCell<std::collections::HashMap<String, Option<String>>>>;

thread_local! {
    static QUEUE_ITEMS: RefCell<ContentMemo<Items>> =
        const { RefCell::new(ContentMemo::new(MIN_DOCUMENT_BYTES, DOCUMENT_ENTRIES, DOCUMENT_KEY_BYTES)) };
    // Kept apart from `QUEUE_ITEMS` so the component layer never answers a
    // caller outside its scope.
    static SCOPED_QUEUE_ITEMS: RefCell<ContentMemo<Items>> =
        const { RefCell::new(ContentMemo::new(MIN_DOCUMENT_BYTES, DOCUMENT_ENTRIES, DOCUMENT_KEY_BYTES)) };
    static SLICE_ITEMS: RefCell<ContentMemo<SliceItems>> =
        const { RefCell::new(ContentMemo::new(MIN_SLICE_BYTES, 128, 8 * 1024 * 1024)) };
    static QUEUE_TREE: RefCell<ContentMemo<TreeNodes>> =
        const { RefCell::new(ContentMemo::new(MIN_DOCUMENT_BYTES, DOCUMENT_ENTRIES, DOCUMENT_KEY_BYTES)) };
    static SCOPED_QUEUE_TREE: RefCell<ContentMemo<TreeNodes>> =
        const { RefCell::new(ContentMemo::new(MIN_DOCUMENT_BYTES, DOCUMENT_ENTRIES, DOCUMENT_KEY_BYTES)) };
    static FOLLOWUP_ROWS: RefCell<ContentMemo<Arc<Vec<String>>>> =
        const { RefCell::new(ContentMemo::new(MIN_DOCUMENT_BYTES, DOCUMENT_ENTRIES, DOCUMENT_KEY_BYTES)) };
    static COMPLETION_EVIDENCE: RefCell<ContentMemo<Evidence>> =
        const { RefCell::new(ContentMemo::new(MIN_DOCUMENT_BYTES, DOCUMENT_ENTRIES, DOCUMENT_KEY_BYTES)) };
    static COMPLETION_RESPONSES: RefCell<ContentMemo<Responses>> =
        const { RefCell::new(ContentMemo::new(MIN_DOCUMENT_BYTES, DOCUMENT_ENTRIES, DOCUMENT_KEY_BYTES)) };
    static QUEUE_NODES: RefCell<ContentMemo<Option<QueueNodes>>> = const {
        RefCell::new(ContentMemo::new(MIN_DOCUMENT_BYTES, DOCUMENT_ENTRIES, DOCUMENT_KEY_BYTES))
    };
    static COMPONENT_SCOPE: Cell<bool> = const { Cell::new(false) };
    static BYPASS: Cell<bool> = const { Cell::new(false) };
    static STATS: Cell<DerivedStats> = const { Cell::new(DerivedStats::ZERO) };
}

/// Work this thread's derivations did: whole-document overlay parses and
/// component slice parses (memo hits are neither).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DerivedStats {
    /// Whole-document queue overlay parses (`item_nodes_all`).
    pub document_parses: u64,
    /// Queue component slice overlay parses.
    pub slice_parses: u64,
    /// Documents whose shape sent the component layer back to the
    /// whole-document overlay.
    pub slice_fallbacks: u64,
}

impl DerivedStats {
    const ZERO: Self = Self {
        document_parses: 0,
        slice_parses: 0,
        slice_fallbacks: 0,
    };
}

/// [`memoized`] for an infallible derivation.
fn memo<T: Clone + 'static>(
    memo: &'static std::thread::LocalKey<RefCell<ContentMemo<T>>>,
    text: &str,
    eval: impl FnOnce(&str) -> T,
) -> T {
    let Ok(value) = memoized(memo, text, |text| {
        Ok::<_, std::convert::Infallible>(eval(text))
    });
    value
}

/// This thread's counters so far.
pub fn derived_stats() -> DerivedStats {
    STATS.with(Cell::get)
}

fn count(update: impl FnOnce(&mut DerivedStats)) {
    STATS.with(|stats| {
        let mut value = stats.get();
        update(&mut value);
        stats.set(value);
    });
}

/// Run `f` with the per-component layer on (see the module docs). Nested
/// scopes are fine; the previous setting is restored even on unwind.
pub fn with_component_scope<T>(f: impl FnOnce() -> T) -> T {
    set_component_scope(true, f)
}

/// Run `f` with the per-component layer off.
pub(crate) fn outside_component_scope<T>(f: impl FnOnce() -> T) -> T {
    set_component_scope(false, f)
}

fn set_component_scope<T>(on: bool, f: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            COMPONENT_SCOPE.with(|scope| scope.set(self.0));
        }
    }
    let _restore = Restore(COMPONENT_SCOPE.with(|scope| scope.replace(on)));
    f()
}

/// Run `f` with every memo here bypassed: each derivation runs on every
/// call, as before these memos existed (the baseline a measurement compares).
pub fn without_memos<T>(f: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            BYPASS.with(|bypass| bypass.set(self.0));
        }
    }
    let _restore = Restore(BYPASS.with(|bypass| bypass.replace(true)));
    outside_component_scope(f)
}

fn bypassed() -> bool {
    BYPASS.with(Cell::get)
}

fn component_scope() -> bool {
    COMPONENT_SCOPE.with(Cell::get)
}

/// Whether the per-component layer is on for this thread.
pub(crate) fn in_component_scope() -> bool {
    component_scope()
}

/// The overlay items of every queue component of `document`, in document
/// order: exactly the [`Item`]s of `item_nodes_all(document, "queue")`, or
/// `None` where that is an error (no queue component).
pub(crate) fn queue_items(document: &str) -> Items {
    if bypassed() {
        return whole_document_items(document);
    }
    if component_scope() {
        memo(&SCOPED_QUEUE_ITEMS, document, |document| {
            sliced_queue_items(document).unwrap_or_else(|| {
                count(|stats| stats.slice_fallbacks += 1);
                whole_document_items(document)
            })
        })
    } else {
        memo(&QUEUE_ITEMS, document, whole_document_items)
    }
}

fn whole_document_items(document: &str) -> Items {
    queue_item_nodes_result(document)
        .map(|nodes| Arc::new(nodes.iter().map(|node| node.item.clone()).collect()))
}

/// The keyed item nodes of every queue component of `document`: exactly
/// `item_nodes_all(document, "queue")`, empty where that is an error (as
/// `all_item_nodes` filtered to `queue` is), memoized by the document bytes.
pub(crate) fn queue_item_nodes(document: &str) -> QueueNodes {
    queue_item_nodes_result(document).unwrap_or_default()
}

/// `item_nodes_all(document, "queue")`, `None` where it is an error.
fn queue_item_nodes_result(document: &str) -> Option<QueueNodes> {
    let build = |document: &str| {
        count(|stats| stats.document_parses += 1);
        agent_doc_markdown_ast::mutations::item_nodes_all(document, "queue")
            .ok()
            .map(Arc::new)
    };
    if bypassed() {
        return build(document);
    }
    memo(&QUEUE_NODES, document, build)
}

/// Start of the line holding byte `offset`.
fn line_start(document: &str, offset: usize) -> usize {
    document[..offset]
        .rfind('\n')
        .map_or(0, |newline| newline + 1)
}

/// End of the line holding byte `offset`, past its `\n`.
fn line_end(document: &str, offset: usize) -> usize {
    document[offset..]
        .find('\n')
        .map_or(document.len(), |newline| offset + newline + 1)
}

/// Whether `line` starts an HTML block of kind 1, which only its own end tag
/// ends (a blank line does not).
fn starts_raw_html_block(line: &str) -> bool {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return false;
    }
    let rest = &line.as_bytes()[indent..];
    ["<pre", "<script", "<style", "<textarea"]
        .iter()
        .any(|tag| {
            rest.len() >= tag.len()
                && rest[..tag.len()].eq_ignore_ascii_case(tag.as_bytes())
                && rest
                    .get(tag.len())
                    .is_none_or(|next| matches!(next, b' ' | b'\t' | b'>' | b'\r' | b'\n'))
        })
}

/// The component-scoped queue items, or `None` when the document's shape does
/// not guarantee they equal the whole-document overlay's (module docs).
fn sliced_queue_items(document: &str) -> Option<Items> {
    let components = agent_doc_element::element::parse(document).ok()?;
    let mut slices: Vec<std::ops::Range<usize>> = Vec::new();
    for component in components.iter().filter(|c| c.name == "queue") {
        let start = line_start(document, component.open_start);
        let opener = &document[start..];
        if !(opener.starts_with("<!--") || opener.starts_with('#')) {
            return None;
        }
        if start > 0 {
            let previous = line_start(document, start - 1);
            if !document[previous..start].trim().is_empty() {
                return None;
            }
        }
        if line_start(document, component.close_start) != component.close_start
            || !document[component.close_start..].starts_with("<!--")
        {
            return None;
        }
        let end = line_end(document, component.close_start);
        if slices.last().is_some_and(|last| last.end > start) {
            return None;
        }
        slices.push(start..end);
    }
    // Lines outside the slices: no runnable opener, no kind-1 HTML block.
    let mut offset = 0;
    let mut next_slice = slices.iter().peekable();
    for line in document.split_inclusive('\n') {
        let at = offset;
        offset += line.len();
        while next_slice.peek().is_some_and(|slice| slice.end <= at) {
            next_slice.next();
        }
        let inside = next_slice
            .peek()
            .is_some_and(|slice| slice.start <= at && at < slice.end);
        if starts_raw_html_block(line) {
            return None;
        }
        if inside || !line.contains("<!--") {
            continue;
        }
        let content = line.trim_end_matches(['\n', '\r']);
        if let Some((name, _)) = agent_doc_markdown_ast::overlay::parse_open_marker_line(content)
            && agent_doc_markdown_ast::overlay::projected_component_name(&name) == "queue"
        {
            return None;
        }
    }
    if slices.is_empty() {
        return Some(None);
    }
    let mut items = Vec::new();
    for slice in &slices {
        let relative = slice_items(&document[slice.clone()])?;
        items.extend(relative.iter().cloned().map(|mut item| {
            item.start_byte += slice.start;
            item.end_byte += slice.start;
            item
        }));
    }
    Some(Some(Arc::new(items)))
}

/// The overlay items of one queue component slice, relative to the slice, or
/// `None` when the slice is not exactly one closed `queue` component.
fn slice_items(slice: &str) -> SliceItems {
    memo(&SLICE_ITEMS, slice, |slice| {
        count(|stats| stats.slice_parses += 1);
        match agent_doc_markdown_ast::overlay::components(slice).as_slice() {
            [component]
                if component.name == "queue"
                    && component.start_byte == 0
                    && component.end_byte == slice.len() =>
            {
                Some(Arc::new(component.items.clone()))
            }
            _ => None,
        }
    })
}

/// The unselected nodes of `queue_tree(document, ..)`, memoized by the
/// document bytes.
pub(crate) fn queue_tree_nodes(
    document: &str,
    build: impl FnOnce(&str) -> anyhow::Result<Vec<crate::queue_tree::QueueTreeNode>>,
) -> anyhow::Result<Vec<crate::queue_tree::QueueTreeNode>> {
    if bypassed() {
        return build(document);
    }
    // The tree may read queue items, so the component layer keeps its own.
    let memo = if component_scope() {
        &SCOPED_QUEUE_TREE
    } else {
        &QUEUE_TREE
    };
    memoized(memo, document, |document| build(document).map(Arc::new))
        .map(|nodes| nodes.as_ref().clone())
}

/// [`crate::followup_moved_state`]'s follow-up row index, memoized by the
/// document bytes.
pub(crate) fn followup_rows(
    document: &str,
    build: impl FnOnce(&str) -> Vec<String>,
) -> Arc<Vec<String>> {
    if bypassed() {
        return Arc::new(build(document));
    }
    memo(&FOLLOWUP_ROWS, document, |document| {
        Arc::new(build(document))
    })
}

/// [`agent_doc_document::subagent_completion::completion_evidence`],
/// memoized by the text's bytes.
pub(crate) fn completion_evidence(text: &str) -> Evidence {
    if bypassed() {
        return Arc::new(agent_doc_document::subagent_completion::completion_evidence(text));
    }
    memo(&COMPLETION_EVIDENCE, text, |text| {
        Arc::new(agent_doc_document::subagent_completion::completion_evidence(text))
    })
}

/// [`agent_doc_document::subagent_completion::completion_response`],
/// memoized per text revision and key.
pub(crate) fn completion_response(text: &str, key: &str) -> Option<String> {
    let lookup = || agent_doc_document::subagent_completion::completion_response(text, key);
    if bypassed() {
        return lookup();
    }
    let responses = memo(&COMPLETION_RESPONSES, text, |_| Responses::default());
    if let Some(hit) = responses.borrow().get(key) {
        return hit.clone();
    }
    let value = lookup();
    responses
        .borrow_mut()
        .insert(key.to_string(), value.clone());
    value
}

#[cfg(test)]
#[path = "derived_memo_tests.rs"]
mod tests;
