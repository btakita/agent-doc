//! `#incrementalreconcile`: the facts the queue/closeout chain reads from
//! `agent:exchange`, kept per exchange chunk and maintained from the
//! revision's changeset instead of rescanning the exchange.
//!
//! `agent:exchange` is the unbounded part of a session document (about 85%
//! of a 1 MB sdk-dev cut). The chain ([`crate::reconcile_incremental`]) runs
//! on a *skeleton*: the document with the exchange body replaced by a digest
//! of exactly the lines its passes read there, while completion cells are
//! answered from this index. Which passes read the exchange, and what
//! replaced each read:
//!
//! | Pass | Exchange read | Replaced by |
//! |---|---|---|
//! | `followup_moved_state` (follow-up row index) | `>` quotes that are follow-up rows | digest: `>` lines mentioning `follow` |
//! | `response_remaining` (quoted follow-up rows) | the same quotes | the same digest lines |
//! | `self_repair` (claim-label residue evidence) | lines whose bullet or code span holds an owner label | digest: those lines ([`crate::self_repair::exchange_label_candidate`]) |
//! | `loose_ends` → `finished_duplicate` (queue-prompt echoes) | `### ` headings and `> **Queue prompt:**` echoes, with the quoted line after an empty prompt | digest: those lines, keeping every line while an echo is awaited |
//! | `response_tree_sync`, `queue_consume` hand-back/stopped-row lookups | completion cells (`completion_evidence`/`completion_response`) | this index's cells, answered through [`crate::derived_memo`] |
//! | `self_repair` (`response_proven`) | `[#r-…]` originals | unchanged external input: the caller's closure reads them lazily, only when a misplaced response tree asks |
//!
//! # Chunks
//!
//! The body is split before every line that starts with `### ` (each cell's
//! heading). Every chunk is indexed on its own: its digest lines, its
//! completion cells (evidence, and the response of the first cell per key),
//! whether it holds component-marker-like text, and whether CommonMark block
//! state is neutral at its end (a heading right after it parses as a
//! heading, so no fence, HTML block or container runs on into the next
//! chunk). With every chunk neutral, a chunk parses the same alone as in
//! place, so the facts of the whole body are the facts of its chunks in
//! order, and a splice re-indexes only the chunks it touches.
//!
//! The chunking is exactly the from-scratch chunking after every splice: a
//! touched region grows until both its ends are chunk boundaries of the new
//! body.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use agent_doc_document::subagent_completion::{
    CompletionEvidence, completion_evidence, completion_response,
};

/// The first line of a non-blank exchange digest. A chain document holding
/// it is a skeleton whose exchange cells come from the index
/// ([`ExchangeCells`]). It is no component marker (`agent-doc:` is not
/// `agent:`), no evidence line of any pass, and a complete HTML block.
pub(crate) const DIGEST_SENTINEL: &str = "<!-- agent-doc:exchange-digest -->\n";

/// A completion key and the response its first cell yields.
type KeyedResponse = (String, Option<String>);

/// One indexed chunk of the exchange body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chunk {
    len: usize,
    digest: String,
    cells: Vec<CompletionEvidence>,
    /// The response the first cell holding each key yields, per key, in
    /// first-appearance order.
    responses: Vec<KeyedResponse>,
    /// Holds `<!-- agent:` / `<!-- /agent:` text (boundary markers aside).
    markers: bool,
    /// The chunk and its digest end in neutral block state and the digest
    /// holds no marker-like text.
    neutral: bool,
}

impl Chunk {
    fn index(text: &str) -> Self {
        let digest = digest_lines(text);
        let cells = completion_evidence(text);
        let mut responses: Vec<(String, Option<String>)> = Vec::new();
        for cell in &cells {
            if !responses.iter().any(|(key, _)| *key == cell.key) {
                responses.push((cell.key.clone(), completion_response(text, &cell.key)));
            }
        }
        let markers = has_marker_like_text(text);
        let neutral =
            block_neutral(text) && block_neutral(&digest) && !has_marker_like_text(&digest);
        Self {
            len: text.len(),
            digest,
            cells,
            responses,
            markers,
            neutral,
        }
    }
}

/// Whether `line` (without its newline) is a `>` quote one of the
/// follow-up row readers takes: `followup_moved_state`'s follow-up row index
/// or `response_remaining`'s quoted follow-up rows.
fn quoted_followup_line(line: &str) -> bool {
    let Some(quoted) = line.trim().strip_prefix('>') else {
        return false;
    };
    let quoted = quoted.trim_start();
    let row = quoted
        .strip_prefix("**Queue prompt:**")
        .unwrap_or(quoted)
        .trim_start();
    (crate::followup_moved_state::is_quoted_followup_row(row)
        && crate::followup_owner::mentions_followup_row(row))
        || crate::followup_owner::followup_row_action_item(&crate::response_remaining::plain_text(
            row,
        ))
        .is_some()
}

/// Whether `line` is one `finished_duplicate`'s echo reader takes or that
/// changes its state: a `> **Queue prompt:**` echo.
fn echo_line(line: &str) -> bool {
    line.strip_prefix('>')
        .is_some_and(|quoted| quoted.trim().starts_with("**Queue prompt:**"))
}

/// Whether a line leaves `finished_duplicate`'s echo reader awaiting the
/// quoted prompt on a later line (`> **Queue prompt:**` with nothing after).
fn echo_awaits(line: &str, awaiting: bool) -> bool {
    if line.starts_with("### ") {
        return false;
    }
    let Some(quoted) = line.strip_prefix('>') else {
        return false;
    };
    let quoted = quoted.trim();
    if let Some(rest) = quoted.strip_prefix("**Queue prompt:**") {
        return rest.trim().is_empty();
    }
    awaiting && quoted.is_empty()
}

/// The lines of `text` (one chunk) the chain's exchange readers use,
/// verbatim and in order: quoted follow-up rows, claim-label evidence, and
/// queue-prompt echoes with every line while an echo is awaited (so the echo
/// reader's state machine steps the same way). The chunk's `### ` heading
/// names its echoes, so it is kept when the chunk has one. A chunk that ends
/// awaiting an echo ends its digest with a blank line, which resets the
/// reader as the next chunk's heading does.
fn digest_lines(text: &str) -> String {
    let mut out = String::new();
    let mut heading: Option<&str> = None;
    let mut echoes = false;
    let mut awaiting = false;
    for (index, raw) in text.split_inclusive('\n').enumerate() {
        let line = raw.trim_end_matches(['\n', '\r']);
        if index == 0
            && line.starts_with("### ")
            && !crate::self_repair::exchange_label_candidate(line)
        {
            heading = Some(raw);
            continue;
        }
        let echo = awaiting || echo_line(line);
        if echo || quoted_followup_line(line) || crate::self_repair::exchange_label_candidate(line)
        {
            out.push_str(raw);
        }
        echoes |= echo;
        awaiting = echo_awaits(line, awaiting);
    }
    if awaiting {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    match heading {
        Some(heading) if echoes => format!("{heading}{out}"),
        _ => out,
    }
}

/// Whether `text` holds an `<!-- agent:…` or `<!-- /agent:…` marker-like
/// span (in code or not) other than a boundary marker, which the component
/// parser skips.
fn has_marker_like_text(text: &str) -> bool {
    let mut rest = text;
    while let Some(at) = rest.find("<!--") {
        let after = rest[at + 4..].trim_start();
        if let Some(name) = after
            .strip_prefix("agent:")
            .or_else(|| after.strip_prefix("/agent:"))
            && !name.starts_with("boundary:")
        {
            return true;
        }
        rest = &rest[at + 4..];
    }
    false
}

/// Whether a column-0 ATX heading right after `text` parses as a heading:
/// no fenced or indented code, HTML block, list, or block quote that `text`
/// opens runs on past its end. `text` is empty or ends with a newline.
pub(crate) fn block_neutral(text: &str) -> bool {
    use pulldown_cmark::{Event, Parser, Tag};
    if text.is_empty() {
        return true;
    }
    if !text.ends_with('\n') {
        return false;
    }
    let probe = format!("{text}### agent-doc-neutral\n");
    Parser::new(&probe)
        .into_offset_iter()
        .any(|(event, range)| {
            matches!(event, Event::Start(Tag::Heading { .. })) && range.start == text.len()
        })
}

/// Start offsets of the chunks of `body` (0, then every line starting with
/// `### `).
fn chunk_starts(body: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let mut offset = 0;
    for raw in body.split_inclusive('\n') {
        if offset > 0 && raw.starts_with("### ") {
            starts.push(offset);
        }
        offset += raw.len();
    }
    starts
}

fn index_region(text: &str) -> Vec<Chunk> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut starts = chunk_starts(text);
    starts.push(text.len());
    starts
        .windows(2)
        .map(|pair| Chunk::index(&text[pair[0]..pair[1]]))
        .collect()
}

/// Whether `at` starts a chunk of `body` (the start, or a `### ` line).
fn is_chunk_start(body: &str, at: usize) -> bool {
    at == 0 || (body.as_bytes()[at - 1] == b'\n' && body[at..].starts_with("### "))
}

/// What one splice changed in the index.
#[derive(Debug, Default)]
pub(crate) struct ChunkDelta {
    pub(crate) removed: Vec<Chunk>,
    pub(crate) added: Vec<Chunk>,
}

impl ChunkDelta {
    /// Exchange bytes the splice read to index.
    pub(crate) fn added_bytes(&self) -> usize {
        self.added.iter().map(|chunk| chunk.len).sum()
    }

    /// Whether a removed or added chunk holds marker-like text, or an added
    /// chunk is not neutral: the component layout may have changed.
    pub(crate) fn structural(&self) -> bool {
        self.removed.iter().any(|chunk| chunk.markers)
            || self
                .added
                .iter()
                .any(|chunk| chunk.markers || !chunk.neutral)
    }

    /// Whether the added chunks hold any digest line or completion cell the
    /// removed chunks did not (as multisets): new evidence for the chain.
    pub(crate) fn adds_evidence(&self) -> bool {
        let mut lines: HashMap<&str, isize> = HashMap::new();
        type CellKey<'a> = (&'a str, &'a str, &'a str, Option<&'a String>);
        let mut cells: HashMap<CellKey<'_>, isize> = HashMap::new();

        for chunk in &self.removed {
            for line in chunk.evidence_lines() {
                *lines.entry(line).or_default() += 1;
            }
            for cell in &chunk.cells {
                *cells
                    .entry((
                        &cell.owner,
                        &cell.item,
                        &cell.key,
                        chunk.response(&cell.key),
                    ))
                    .or_default() += 1;
            }
        }
        for chunk in &self.added {
            for line in chunk.evidence_lines() {
                let count = lines.entry(line).or_default();
                *count -= 1;
                if *count < 0 {
                    return true;
                }
            }
            for cell in &chunk.cells {
                let count = cells
                    .entry((
                        &cell.owner,
                        &cell.item,
                        &cell.key,
                        chunk.response(&cell.key),
                    ))
                    .or_default();
                *count -= 1;
                if *count < 0 {
                    return true;
                }
            }
        }
        false
    }
}

impl Chunk {
    /// The digest lines that are evidence: none when the digest holds only
    /// headings (a heading only names the echoes after it).
    fn evidence_lines(&self) -> impl Iterator<Item = &str> {
        let any = self
            .digest
            .split_inclusive('\n')
            .any(|line| !line.starts_with("### "));
        self.digest.split_inclusive('\n').filter(move |_| any)
    }

    fn response(&self, key: &str) -> Option<&String> {
        self.responses
            .iter()
            .find(|(have, _)| have == key)
            .and_then(|(_, response)| response.as_ref())
    }
}

/// The exchange body's chunks, in order.
#[derive(Debug, Clone, Default)]
pub(crate) struct ExchangeIndex {
    chunks: Vec<Chunk>,
    /// Changes whenever the completion cells may have changed: unique per
    /// process, so a memo keyed by it is never stale across rebuilds.
    cells_revision: u64,
    /// Whether the body is blank (empty or whitespace only); its digest is
    /// then the body itself.
    blank: Option<String>,
}

impl ExchangeIndex {
    /// Index `body` from scratch (a full read of the exchange: only when the
    /// cache has no previous revision or the layout changed).
    pub(crate) fn build(body: &str) -> Self {
        Self {
            chunks: index_region(body),
            cells_revision: next_cells_revision(),
            blank: body.trim().is_empty().then(|| body.to_string()),
        }
    }

    fn offsets(&self) -> Vec<usize> {
        let mut offset = 0;
        self.chunks
            .iter()
            .map(|chunk| {
                let at = offset;
                offset += chunk.len;
                at
            })
            .collect()
    }

    /// Replace prev-body bytes `removed` with `inserted` bytes, `next_body`
    /// being the whole new body. Only the touched chunks are read (from
    /// `next_body`); every other chunk keeps its facts.
    pub(crate) fn apply(
        &mut self,
        removed: Range<usize>,
        inserted: usize,
        next_body: &str,
    ) -> ChunkDelta {
        let blank = next_body.trim().is_empty();
        self.blank = blank.then(|| next_body.to_string());
        if self.chunks.is_empty() {
            self.chunks = index_region(next_body);
            self.cells_revision = next_cells_revision();
            return ChunkDelta {
                removed: Vec::new(),
                added: self.chunks.clone(),
            };
        }
        let offsets = self.offsets();
        let prev_len = offsets
            .last()
            .map_or(0, |at| at + self.chunks.last().map_or(0, |c| c.len));
        if removed.start == prev_len && removed.is_empty() && is_chunk_start(next_body, prev_len) {
            // A cell appended after the last chunk: index only the new bytes.
            let added = index_region(&next_body[prev_len..]);
            if added.iter().any(|chunk| !chunk.cells.is_empty()) {
                self.cells_revision = next_cells_revision();
            }
            self.chunks.extend(added.iter().cloned());
            return ChunkDelta {
                removed: Vec::new(),
                added,
            };
        }
        let containing = |at: usize| {
            offsets
                .partition_point(|start| *start <= at)
                .saturating_sub(1)
                .min(self.chunks.len() - 1)
        };
        // The chunks holding the removed bytes; a chunk the splice only
        // borders is taken in below when the new body does not start a chunk
        // where it starts.
        let boundary = |at: usize| offsets.binary_search(&at).is_ok();
        let mut first = if boundary(removed.start) {
            containing(removed.start)
        } else {
            containing(removed.start.saturating_sub(1))
        };
        let mut last = if removed.end > removed.start && boundary(removed.end) {
            containing(removed.end - 1)
        } else {
            containing(removed.end)
        }
        .max(first);
        let delta = inserted as isize - removed.len() as isize;
        loop {
            let start = offsets[first];
            if start == 0 || is_chunk_start(next_body, start) {
                break;
            }
            first -= 1;
        }
        loop {
            let prev_end = offsets[last] + self.chunks[last].len;
            let next_end = (prev_end as isize + delta) as usize;
            if last + 1 == self.chunks.len() || is_chunk_start(next_body, next_end) {
                break;
            }
            last += 1;
        }
        let start = offsets[first];
        let next_end = (offsets[last] as isize + self.chunks[last].len as isize + delta) as usize;
        let added = index_region(&next_body[start..next_end]);
        let removed: Vec<Chunk> = self
            .chunks
            .splice(first..=last, added.iter().cloned())
            .collect();
        fn cells(chunks: &[Chunk]) -> Vec<(&CompletionEvidence, &[KeyedResponse])> {
            chunks
                .iter()
                .flat_map(|chunk| {
                    chunk
                        .cells
                        .iter()
                        .map(move |cell| (cell, chunk.responses.as_slice()))
                })
                .collect()
        }
        if cells(&removed) != cells(&added) {
            self.cells_revision = next_cells_revision();
        }
        ChunkDelta { removed, added }
    }

    /// Identifies the completion cells: equal revisions hold equal cells.
    pub(crate) fn cells_revision(&self) -> u64 {
        self.cells_revision
    }

    /// The body the chain sees in place of the exchange body.
    pub(crate) fn digest(&self) -> String {
        if let Some(blank) = &self.blank {
            return blank.clone();
        }
        let mut out = String::from(DIGEST_SENTINEL);
        for chunk in &self.chunks {
            out.push_str(&chunk.digest);
        }
        out
    }

    /// Whether every chunk is neutral, so the skeleton parses like the
    /// document outside the exchange.
    pub(crate) fn neutral(&self) -> bool {
        self.chunks.iter().all(|chunk| chunk.neutral)
    }

    /// The completion cells, for [`crate::derived_memo`].
    pub(crate) fn cells(&self, digest_len: usize) -> ExchangeCells {
        let mut responses: HashMap<String, Option<String>> = HashMap::new();
        let mut evidence = Vec::new();
        for chunk in &self.chunks {
            evidence.extend(chunk.cells.iter().cloned());
            for (key, response) in &chunk.responses {
                responses
                    .entry(key.clone())
                    .or_insert_with(|| response.clone());
            }
        }
        ExchangeCells {
            digest_len,
            evidence,
            responses,
        }
    }
}

fn next_cells_revision() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl PartialEq for ExchangeIndex {
    /// Same chunks (the cells revision is an identity, not content).
    fn eq(&self, other: &Self) -> bool {
        self.chunks == other.chunks && self.blank == other.blank
    }
}

impl Eq for ExchangeIndex {}

/// The exchange's completion cells, served to the chain's lookups on a
/// skeleton (a text holding [`DIGEST_SENTINEL`]).
#[derive(Debug, Default)]
pub(crate) struct ExchangeCells {
    /// Bytes of the digest starting at the sentinel.
    pub(crate) digest_len: usize,
    pub(crate) evidence: Vec<CompletionEvidence>,
    pub(crate) responses: HashMap<String, Option<String>>,
}

/// A skeleton's completion evidence: the cells before the digest, the
/// index's, then the cells after it, and the keys of the cells before it.
type SkeletonEvidence = (Arc<Vec<CompletionEvidence>>, Rc<HashSet<String>>);

thread_local! {
    static SCOPE: std::cell::RefCell<Option<Rc<ExchangeCells>>> = const { std::cell::RefCell::new(None) };
    static TAINTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static SKELETON_EVIDENCE: std::cell::RefCell<Vec<(Arc<str>, SkeletonEvidence)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Skeletons whose evidence is memoized within one scope.
const SKELETON_EVIDENCE_ENTRIES: usize = 4;

/// Run `f` with `cells` answering completion lookups on skeleton texts.
/// Returns `f`'s value and whether a lookup met a skeleton whose digest
/// was changed (the caller then runs the whole-document chain instead).
pub(crate) fn with_exchange_cells<T>(cells: Rc<ExchangeCells>, f: impl FnOnce() -> T) -> (T, bool) {
    struct Restore(Option<Rc<ExchangeCells>>, bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            SCOPE.with(|scope| *scope.borrow_mut() = self.0.take());
            TAINTED.with(|tainted| tainted.set(self.1));
        }
    }
    SKELETON_EVIDENCE.with(|memo| memo.borrow_mut().clear());
    let previous = SCOPE.with(|scope| scope.borrow_mut().replace(cells));
    let _restore = Restore(previous, TAINTED.with(|tainted| tainted.replace(false)));
    let value = f();
    SKELETON_EVIDENCE.with(|memo| memo.borrow_mut().clear());
    (value, TAINTED.with(std::cell::Cell::get))
}

/// For a skeleton `text` inside [`with_exchange_cells`]: the scope's cells
/// and the text before and after the digest.
pub(crate) fn split_skeleton(text: &str) -> Option<(Rc<ExchangeCells>, &str, &str)> {
    let cells = SCOPE.with(|scope| scope.borrow().clone())?;
    let at = text.find(DIGEST_SENTINEL)?;
    let end = at + cells.digest_len;
    if end > text.len() || !text.is_char_boundary(end) {
        TAINTED.with(|tainted| tainted.set(true));
        return None;
    }
    Some((cells, &text[..at], &text[end..]))
}

fn skeleton_evidence(text: &str) -> Option<(Rc<ExchangeCells>, &str, SkeletonEvidence)> {
    let (cells, head, tail) = split_skeleton(text)?;
    let hit = SKELETON_EVIDENCE.with(|memo| {
        memo.borrow()
            .iter()
            .find(|(key, _)| key.len() == text.len() && **key == *text)
            .map(|(_, value)| value.clone())
    });
    if let Some(hit) = hit {
        return Some((cells, head, hit));
    }
    let before = completion_evidence(head);
    let keys: HashSet<String> = before.iter().map(|cell| cell.key.clone()).collect();
    let mut all = before;
    all.extend(cells.evidence.iter().cloned());
    all.extend(completion_evidence(tail));
    let value = (Arc::new(all), Rc::new(keys));
    SKELETON_EVIDENCE.with(|memo| {
        let mut memo = memo.borrow_mut();
        memo.insert(0, (Arc::from(text), value.clone()));
        memo.truncate(SKELETON_EVIDENCE_ENTRIES);
    });
    Some((cells, head, value))
}

/// `completion_evidence(text)` of the document a skeleton stands for, when
/// `text` is one inside [`with_exchange_cells`].
pub(crate) fn skeleton_completion_evidence(text: &str) -> Option<Arc<Vec<CompletionEvidence>>> {
    skeleton_evidence(text).map(|(_, _, (all, _))| all)
}

/// `completion_response(text, key)` of the document a skeleton stands for,
/// when `text` is one inside [`with_exchange_cells`]: the first cell holding
/// `key` decides, before the digest, in the exchange, then after it.
pub(crate) fn skeleton_completion_response(text: &str, key: &str) -> Option<Option<String>> {
    let (cells, head, (_, head_keys)) = skeleton_evidence(text)?;
    if head_keys.contains(key) {
        return Some(completion_response(head, key));
    }
    if let Some(response) = cells.responses.get(key) {
        return Some(response.clone());
    }
    let (_, _, tail) = split_skeleton(text)?;
    Some(completion_response(tail, key))
}

#[cfg(test)]
#[path = "exchange_index_tests.rs"]
mod tests;
