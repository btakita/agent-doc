//! Edit-proportional local minting and delta extraction for [`super::ReplicaState`]
//! (`#replicadiffcost`).
//!
//! lazily's [`lazily::TextCrdt`] resolves a local edit's left origin by
//! projecting the WHOLE origin tree (`ordered_ids`: a children map, a sort, and a
//! DFS over every retained element) on each `insert_str` / `delete_range`, and
//! answers `delta_since` by scanning every retained element. An editor flush
//! therefore cost O(document) or worse whatever the burst size: about 48 ms on a
//! 125 KB session document for a one-keystroke diff.
//!
//! Two side structures make that proportional to the edit without changing a
//! single minted operation:
//!
//! - [`VisibleOrder`] keeps the visible elements in document order (chunked, so
//!   a splice moves at most one chunk). A local edit reads its origin at
//!   `index - 1` and mints exactly the ids, origins and tombstones that
//!   `TextCrdt::insert_str` / `delete_range` would have minted, then hands them
//!   to `apply_delta_unprojected`.
//! - [`LocalJournal`] records every element touched by local minting since an
//!   exact version-vector base. A peer whose state vector dominates that base
//!   has already seen every element outside the journal, so its delta is the
//!   journal's matching suffix filtered by `delta_since`'s own predicate — the
//!   same operation set, found without scanning the document.
//!
//! Remote updates invalidate both (they are rebuilt from the graph on the next
//! local edit / state vector), and an index whose projection disagrees with the
//! replica's text is refused so the lazily path keeps running unchanged.

use lazily::{OpId, TextOp, TextVersionVector};
use std::collections::{HashMap, HashSet};

/// One visible character with the identity a delete op must carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct VisibleElem {
    pub(super) id: OpId,
    pub(super) origin: Option<OpId>,
    pub(super) ch: char,
}

impl VisibleElem {
    pub(super) fn into_op(self, deleted: Option<OpId>) -> TextOp {
        TextOp {
            id: self.id,
            ch: self.ch,
            origin: self.origin,
            deleted,
        }
    }
}

const CHUNK_TARGET: usize = 1024;
const CHUNK_MAX: usize = 2 * CHUNK_TARGET;

/// The replica's visible elements in projection order.
///
/// Chunked so an insert or delete moves at most one chunk's elements rather
/// than shifting the whole document. Locating an index walks chunk lengths,
/// which is ~120 steps for a 125 KB document.
#[derive(Debug, Clone, Default)]
pub(super) struct VisibleOrder {
    chunks: Vec<Vec<VisibleElem>>,
    len: usize,
}

impl VisibleOrder {
    pub(super) fn len(&self) -> usize {
        self.len
    }

    /// Project the visible order of a complete op set exactly as lazily's
    /// `TextCrdt::ordered_ids(false)` does: the pre-order traversal of the
    /// origin tree from the document start, same-origin siblings by descending
    /// `OpId`, tombstones kept for traversal and dropped from the output, and an
    /// element whose origin is absent never reached.
    pub(super) fn from_ops(ops: &[TextOp]) -> Self {
        let mut children: HashMap<Option<OpId>, Vec<usize>> = HashMap::new();
        for (index, op) in ops.iter().enumerate() {
            children.entry(op.origin).or_default().push(index);
        }
        for list in children.values_mut() {
            list.sort_unstable_by(|a, b| ops[*b].id.cmp(&ops[*a].id));
        }
        let mut order = Self::default();
        let mut run = Vec::with_capacity(CHUNK_TARGET);
        let mut stack: Vec<usize> = children
            .get(&None)
            .map(|roots| roots.iter().rev().copied().collect())
            .unwrap_or_default();
        while let Some(index) = stack.pop() {
            let op = &ops[index];
            if op.deleted.is_none() {
                run.push(VisibleElem {
                    id: op.id,
                    origin: op.origin,
                    ch: op.ch,
                });
                if run.len() == CHUNK_TARGET {
                    order.len += run.len();
                    order.chunks.push(std::mem::replace(
                        &mut run,
                        Vec::with_capacity(CHUNK_TARGET),
                    ));
                }
            }
            if let Some(kids) = children.get(&Some(op.id)) {
                stack.extend(kids.iter().rev().copied());
            }
        }
        if !run.is_empty() {
            order.len += run.len();
            order.chunks.push(run);
        }
        order
    }

    /// Whether this order's characters spell exactly `text`.
    pub(super) fn spells(&self, text: &str) -> bool {
        let mut chars = text.chars();
        for chunk in &self.chunks {
            for elem in chunk {
                if chars.next() != Some(elem.ch) {
                    return false;
                }
            }
        }
        chars.next().is_none()
    }

    /// The chunk holding `index` (`index < len`), and the offset inside it.
    fn locate(&self, index: usize) -> (usize, usize) {
        let mut remaining = index;
        for (chunk_index, chunk) in self.chunks.iter().enumerate() {
            if remaining < chunk.len() {
                return (chunk_index, remaining);
            }
            remaining -= chunk.len();
        }
        unreachable!(
            "visible index {index} is outside the order (len {})",
            self.len
        )
    }

    pub(super) fn get(&self, index: usize) -> Option<VisibleElem> {
        (index < self.len).then(|| {
            let (chunk, offset) = self.locate(index);
            self.chunks[chunk][offset]
        })
    }

    /// Insert `run` so its first element lands at visible `index` (`<= len`).
    pub(super) fn insert_run(&mut self, index: usize, run: Vec<VisibleElem>) {
        if run.is_empty() {
            return;
        }
        debug_assert!(index <= self.len);
        if self.chunks.is_empty() {
            self.chunks.push(Vec::new());
        }
        let (chunk_index, offset) = if index == self.len {
            let last = self.chunks.len() - 1;
            (last, self.chunks[last].len())
        } else {
            self.locate(index)
        };
        self.len += run.len();
        let chunk = &mut self.chunks[chunk_index];
        if chunk.len() + run.len() <= CHUNK_MAX {
            chunk.splice(offset..offset, run);
            return;
        }
        let tail = chunk.split_off(offset);
        let mut merged = std::mem::take(chunk);
        merged.extend(run);
        merged.extend(tail);
        let pieces = merged
            .chunks(CHUNK_TARGET)
            .map(<[VisibleElem]>::to_vec)
            .collect::<Vec<_>>();
        self.chunks.splice(chunk_index..=chunk_index, pieces);
    }

    /// Remove `count` visible elements starting at `index`, in order. The
    /// caller clamps `index + count` to `len`.
    pub(super) fn remove_range(&mut self, index: usize, count: usize) -> Vec<VisibleElem> {
        debug_assert!(index + count <= self.len);
        let mut removed = Vec::with_capacity(count);
        while removed.len() < count {
            let (chunk_index, offset) = self.locate(index);
            let chunk = &mut self.chunks[chunk_index];
            let take = (count - removed.len()).min(chunk.len() - offset);
            removed.extend(chunk.drain(offset..offset + take));
            if chunk.is_empty() {
                self.chunks.remove(chunk_index);
            }
            self.len -= take;
        }
        removed
    }
}

/// Above this many events the journal is rebased at the next local flush, so a
/// diff never walks more than a bounded suffix. A peer behind the new base gets
/// lazily's full `delta_since` instead (always correct, merely O(document)).
const JOURNAL_REBASE_EVENTS: usize = 32_768;

/// Elements touched by local minting since an exact version-vector `base`.
#[derive(Debug, Clone)]
pub(super) struct LocalJournal {
    /// The replica's exact version vector when the journal started.
    base: TextVersionVector,
    /// `(minted counter, element)` per minted op, in minting order. Every event
    /// is a local mint, so counters strictly increase.
    events: Vec<(u64, OpId)>,
    /// Current state of every element an event names.
    elements: HashMap<OpId, TextOp>,
}

impl LocalJournal {
    pub(super) fn new(base: TextVersionVector) -> Self {
        Self {
            base,
            events: Vec::new(),
            elements: HashMap::new(),
        }
    }

    pub(super) fn needs_rebase(&self) -> bool {
        self.events.len() > JOURNAL_REBASE_EVENTS
    }

    /// Record a locally minted insert (`op.deleted == None`) or tombstone.
    pub(super) fn record(&mut self, op: &TextOp) {
        let minted = op.deleted.unwrap_or(op.id);
        debug_assert!(
            self.events
                .last()
                .is_none_or(|(last, _)| *last < minted.counter()),
            "journal events must be minted in counter order"
        );
        self.events.push((minted.counter(), op.id));
        self.elements
            .entry(op.id)
            .and_modify(|known| known.deleted = op.deleted.or(known.deleted))
            .or_insert_with(|| op.clone());
    }

    /// The ops `their_vv` has not observed — the same set as
    /// `TextCrdt::delta_since(their_vv)` — or `None` when `their_vv` does not
    /// dominate the base, so some unobserved op may predate the journal.
    ///
    /// Exactness: every element outside the journal holds only ops present at
    /// the base, whose counters are at most `base[peer]`; a dominating
    /// `their_vv` has seen them, so `delta_since` would skip them too. Every
    /// element inside the journal is tested with `delta_since`'s predicate on
    /// its current state. Elements whose only unseen op is a journal event are
    /// found by the event suffix past `their_vv[local_peer]`.
    pub(super) fn delta_for(
        &self,
        their_vv: &TextVersionVector,
        local_peer: u64,
    ) -> Option<Vec<TextOp>> {
        let dominates = self
            .base
            .iter()
            .all(|(peer, counter)| their_vv.get(peer).copied().unwrap_or(0) >= *counter);
        if !dominates {
            return None;
        }
        let seen = |id: OpId| id.counter() <= their_vv.get(&id.peer()).copied().unwrap_or(0);
        let floor = their_vv.get(&local_peer).copied().unwrap_or(0);
        let start = self
            .events
            .partition_point(|(counter, _)| *counter <= floor);
        let mut emitted = HashSet::new();
        let mut delta = Vec::new();
        for (_, element) in &self.events[start..] {
            if !emitted.insert(*element) {
                continue;
            }
            let op = &self.elements[element];
            if !seen(op.id) || op.deleted.is_some_and(|deleted| !seen(deleted)) {
                delta.push(op.clone());
            }
        }
        Some(delta)
    }
}

#[cfg(test)]
mod tests {
    //! Differential tests: every replica runs beside a reference replica pinned
    //! to lazily's own `insert_str` / `delete_range` / `delta_since` — exactly
    //! the paths `ReplicaState` ran before `#replicadiffcost` — fed the same
    //! local edits (so the same pending-edit composition) and remote updates.

    use super::super::{ReplicaState, decode_update_ops, encode_update_ops};
    use super::*;
    use lazily::TextCrdt;

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, bound: usize) -> usize {
            if bound == 0 {
                0
            } else {
                (self.next() % bound as u64) as usize
            }
        }
    }

    const ALPHABET: &[char] = &['a', 'b', ' ', '\n', '#', 'é', 'あ', '✓', '😀'];

    fn random_text(rng: &mut Rng, chars: usize) -> String {
        (0..chars)
            .map(|_| ALPHABET[rng.below(ALPHABET.len())])
            .collect()
    }

    /// A session-document-shaped seed of about `bytes` bytes.
    fn seed_document(bytes: usize) -> String {
        let mut text = String::with_capacity(bytes + 64);
        let mut line = 0usize;
        while text.len() < bytes {
            text.push_str(&format!(
                "- [#item{line:05}] queue line {line} — durable closeout note ✓\n"
            ));
            line += 1;
        }
        text
    }

    fn sorted(mut ops: Vec<TextOp>) -> Vec<TextOp> {
        ops.sort_by_key(|op| op.id);
        ops
    }

    /// A replica pinned to the pre-`#replicadiffcost` lazily paths.
    fn lazily_reference(peer: u64) -> ReplicaState {
        let reference = ReplicaState::new(peer);
        reference.local_fast_path_refused.set(true);
        reference.invalidate_local_indexes();
        reference
    }

    /// A plain lazily edit for the concurrent peer.
    fn lazily_edit(text: &mut TextCrdt, offset: u32, delete_len: u32, insert: &str) {
        let total = text.len();
        let start = (offset as usize).min(total);
        let end = start.saturating_add(delete_len as usize).min(total);
        if end > start {
            text.delete_range(start, end - start);
        }
        if !insert.is_empty() {
            text.insert_str(start, insert);
        }
    }

    fn random_edit(rng: &mut Rng, len: usize) -> (u32, u32, String) {
        let offset = rng.below(len + 2) as u32;
        let shape = rng.below(10);
        let (delete_len, insert_chars) = match shape {
            0..=4 => (0, 1 + rng.below(3)),
            5 | 6 => (1 + rng.below(4), 0),
            7 => (1 + rng.below(6), 1 + rng.below(8)),
            8 => (rng.below(40), rng.below(40)),
            _ => (0, 1),
        };
        (offset, delete_len as u32, random_text(rng, insert_chars))
    }

    fn graph(replica: &ReplicaState) -> Vec<TextOp> {
        let _ = replica.state_vector();
        sorted(replica.text.borrow().delta_since(&TextVersionVector::new()))
    }

    /// Every observable the fast path could change must equal the reference:
    /// the projection, the clock, the complete operation graph, the state
    /// vector bytes, and the delta set for every frontier the test holds.
    fn assert_equivalent(
        replica: &ReplicaState,
        reference: &ReplicaState,
        frontiers: &[Vec<u8>],
        context: &str,
    ) {
        assert_eq!(
            replica.state_vector(),
            reference.state_vector(),
            "{context}: state vector bytes"
        );
        assert_eq!(replica.text(), reference.text(), "{context}: projection");
        assert_eq!(
            replica.text.borrow().clock(),
            reference.text.borrow().clock(),
            "{context}: clock"
        );
        assert_eq!(
            graph(replica),
            graph(reference),
            "{context}: operation graph"
        );
        for (index, frontier) in frontiers.iter().enumerate() {
            let ours = decode_update_ops(&replica.diff(frontier).unwrap()).unwrap();
            let theirs = decode_update_ops(&reference.diff(frontier).unwrap()).unwrap();
            assert_eq!(
                sorted(ours),
                sorted(theirs),
                "{context}: delta for frontier {index}"
            );
        }
    }

    fn run_differential(seed: u64, document_bytes: usize, steps: usize) {
        let mut rng = Rng(seed);
        let local_peer = 0x6B65_7901;
        let seed_text = seed_document(document_bytes);
        // Open + seed exactly as the editor (and the keystroke bench) does.
        let replica = ReplicaState::new(local_peer);
        let reference = lazily_reference(local_peer);
        for side in [&replica, &reference] {
            side.apply_local_edit(0, 0, &seed_text);
        }
        // A concurrent peer that forks from the seed and types elsewhere.
        let mut remote = TextCrdt::new(0x5150);
        remote.apply_delta(&decode_update_ops(&reference.encode_state()).unwrap());

        // Every state-bearing call goes to both sides: it is the flush boundary,
        // so calling one side alone would compose different pending edits.
        let seeded = replica.state_vector();
        assert_eq!(seeded, reference.state_vector(), "seed frontier");
        let mut frontiers = vec![
            serde_json::to_vec(&TextVersionVector::new()).unwrap(),
            seeded,
        ];
        let mut journal_answers = 0usize;
        for step in 0..steps {
            match rng.below(20) {
                // A remote burst delivered to both replicas.
                0 => {
                    for _ in 0..1 + rng.below(4) {
                        let len = remote.len();
                        let (offset, delete_len, insert) = random_edit(&mut rng, len);
                        lazily_edit(&mut remote, offset, delete_len, &insert);
                    }
                    let frontier = replica.state_vector();
                    assert_eq!(
                        frontier,
                        reference.state_vector(),
                        "step {step}: remote frontier"
                    );
                    let theirs: TextVersionVector = serde_json::from_slice(&frontier).unwrap();
                    let update = encode_update_ops(&remote.delta_since(&theirs)).unwrap();
                    replica.apply_update(&update).unwrap();
                    reference.apply_update(&update).unwrap();
                    let remote_sv = serde_json::to_vec(&remote.version_vector()).unwrap();
                    let back = replica.diff(&remote_sv).unwrap();
                    let reference_back = reference.diff(&remote_sv).unwrap();
                    assert_eq!(
                        sorted(decode_update_ops(&back).unwrap()),
                        sorted(decode_update_ops(&reference_back).unwrap()),
                        "step {step}: delta for the concurrent peer"
                    );
                    remote.apply_delta(&decode_update_ops(&back).unwrap());
                }
                // Snapshot a frontier the forwarder could later diff against.
                1 | 2 => {
                    let frontier = replica.state_vector();
                    assert_eq!(frontier, reference.state_vector(), "step {step}: snapshot");
                    frontiers.push(frontier);
                    if frontiers.len() > 6 {
                        frontiers.remove(2);
                    }
                }
                // A full equivalence check (flushes the pending edits).
                3 => assert_equivalent(&replica, &reference, &frontiers, &format!("step {step}")),
                // The forwarder's flush: frontier, burst, diff against it.
                4..=9 => {
                    let before = replica.state_vector();
                    assert_eq!(before, reference.state_vector(), "step {step}: frontier");
                    for _ in 0..1 + rng.below(16) {
                        let len = reference.text().chars().count();
                        let (offset, delete_len, insert) = random_edit(&mut rng, len);
                        replica.apply_local_edit(offset, delete_len, &insert);
                        reference.apply_local_edit(offset, delete_len, &insert);
                    }
                    let base_dominated = replica.local_journal.borrow().is_some();
                    let ours = decode_update_ops(&replica.diff(&before).unwrap()).unwrap();
                    let theirs = decode_update_ops(&reference.diff(&before).unwrap()).unwrap();
                    assert_eq!(sorted(ours), sorted(theirs), "step {step}: flush delta");
                    journal_answers += usize::from(base_dominated);
                }
                // Local edits left pending across other steps.
                _ => {
                    for _ in 0..1 + rng.below(4) {
                        let len = reference.text().chars().count();
                        let (offset, delete_len, insert) = random_edit(&mut rng, len);
                        replica.apply_local_edit(offset, delete_len, &insert);
                        reference.apply_local_edit(offset, delete_len, &insert);
                    }
                }
            }
        }
        frontiers.push(serde_json::to_vec(&remote.version_vector()).unwrap());
        assert_equivalent(&replica, &reference, &frontiers, "final");
        assert!(
            !replica.local_fast_path_refused.get(),
            "a consistent replica must keep the edit-proportional path"
        );
        assert!(
            journal_answers > 0,
            "the journal path must actually be exercised"
        );
    }

    #[test]
    fn local_minting_and_journal_deltas_match_lazily_on_small_documents() {
        for seed in 1..=24u64 {
            run_differential(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1, 256, 160);
        }
    }

    #[test]
    fn local_minting_and_journal_deltas_match_lazily_on_large_documents() {
        for seed in [7u64, 1_234_567, 0xDEAD_BEEF] {
            run_differential(seed, 48 * 1024, 48);
        }
    }

    /// Random multi-peer graphs (concurrent inserts at one origin, interleaved
    /// deletes, merges): the derived order must spell lazily's projection.
    #[test]
    fn visible_order_matches_lazily_projection_on_concurrent_graphs() {
        for seed in 1..=40u64 {
            let mut rng = Rng(seed.wrapping_mul(0xA24B_AED4_963E_E407) | 1);
            let mut peers = (1..=3u64)
                .map(|peer| TextCrdt::from_str(peer, "shared base text\n"))
                .collect::<Vec<_>>();
            // Distinct peers share one seed graph only via a fork.
            let base = peers[0].clone();
            for (index, peer) in peers.iter_mut().enumerate().skip(1) {
                *peer = base.fork(index as u64 + 1);
            }
            for _ in 0..60 {
                let who = rng.below(peers.len());
                let len = peers[who].len();
                let (offset, delete_len, insert) = random_edit(&mut rng, len);
                lazily_edit(&mut peers[who], offset, delete_len, &insert);
                if rng.below(4) == 0 {
                    let (a, b) = (rng.below(peers.len()), rng.below(peers.len()));
                    let delta = peers[b].delta_since(&peers[a].version_vector());
                    peers[a].apply_delta(&delta);
                }
            }
            for peer in &peers {
                let ops = peer.delta_since(&TextVersionVector::new());
                let order = VisibleOrder::from_ops(&ops);
                assert!(
                    order.spells(&peer.text()),
                    "seed {seed}: derived order diverged"
                );
                assert_eq!(order.len(), peer.len());
            }
        }
    }

    #[test]
    fn chunked_order_splices_like_a_vector() {
        let mut rng = Rng(99);
        let mut order = VisibleOrder::default();
        let mut model: Vec<VisibleElem> = Vec::new();
        let base = TextCrdt::from_str(1, &"x".repeat(9_000));
        let ids = base
            .delta_since(&TextVersionVector::new())
            .into_iter()
            .map(|op| op.id)
            .collect::<Vec<_>>();
        let mut next = 0usize;
        for _ in 0..400 {
            if rng.below(3) == 0 && !model.is_empty() {
                let index = rng.below(model.len());
                let count = rng.below(3_000).min(model.len() - index);
                let expected = model.drain(index..index + count).collect::<Vec<_>>();
                assert_eq!(order.remove_range(index, count), expected);
            } else {
                let index = rng.below(model.len() + 1);
                let count = [1, 2, 5, 700, 2_500][rng.below(5)].min(ids.len() - next);
                let run = ids[next..next + count]
                    .iter()
                    .map(|id| VisibleElem {
                        id: *id,
                        origin: None,
                        ch: 'x',
                    })
                    .collect::<Vec<_>>();
                next += count;
                if next == ids.len() {
                    next = 0;
                }
                model.splice(index..index, run.clone());
                order.insert_run(index, run);
            }
            assert_eq!(order.len(), model.len());
            for probe in [0, model.len() / 2, model.len().saturating_sub(1)] {
                assert_eq!(order.get(probe), model.get(probe).copied());
            }
        }
    }

    /// A replica whose claimed projection disagrees with its graph never lets
    /// the derived order place an edit: it keeps lazily's path, and still
    /// produces lazily's exact operations.
    #[test]
    fn mismatched_cached_projection_refuses_the_fast_path() {
        let source = ReplicaState::from_text(3, "alpha beta gamma\n");
        let state = source.encode_state();
        let replica =
            ReplicaState::from_encoded_with_cached_text(4, &state, "not the graph\n").unwrap();
        replica.apply_local_edit(3, 0, "Z");
        assert!(replica.local_fast_path_refused.get());
        let _ = replica.state_vector();

        let mut reference = TextCrdt::new(4);
        reference.apply_delta(&decode_update_ops(&state).unwrap());
        reference.insert_str(3, "Z");
        assert_eq!(
            sorted(replica.text.borrow().delta_since(&TextVersionVector::new())),
            sorted(reference.delta_since(&TextVersionVector::new()))
        );
    }

    /// Past the rebase threshold, an old frontier falls back to lazily's full
    /// delta and a fresh one keeps the journal; both stay exact.
    #[test]
    fn oversized_journal_rebases_without_losing_old_frontiers() {
        let replica = ReplicaState::new(11);
        let mut reference = TextCrdt::new(11);
        let empty = TextVersionVector::new();
        let big = "z".repeat(JOURNAL_REBASE_EVENTS + 10);
        replica.apply_local_edit(0, 0, &big);
        reference.insert_str(0, &big);
        let after_big = reference.version_vector();
        assert_eq!(
            replica.state_vector(),
            serde_json::to_vec(&after_big).unwrap()
        );
        replica.apply_local_edit(5, 2, "keystroke");
        lazily_edit(&mut reference, 5, 2, "keystroke");
        let _ = replica.state_vector();
        assert!(
            replica
                .local_journal
                .borrow()
                .as_ref()
                .is_some_and(|journal| journal.events.len() < 64),
            "the oversized journal must rebase before recording the next batch"
        );
        for frontier in [empty, after_big] {
            let encoded = replica
                .diff(&serde_json::to_vec(&frontier).unwrap())
                .unwrap();
            assert_eq!(
                sorted(decode_update_ops(&encoded).unwrap()),
                sorted(reference.delta_since(&frontier))
            );
        }
    }

    /// Size scaling of one keystroke flush (state vector, one splice, diff
    /// against the pre-burst frontier), fast path against lazily's path.
    /// Timing only: `cargo test --release -p agent-doc-merge -- --ignored
    /// replica_flush_scaling --nocapture`.
    #[test]
    #[ignore = "timing report, not a correctness gate"]
    fn replica_flush_scaling() {
        for kib in [32usize, 125, 500] {
            let text = seed_document(kib * 1024);
            for refused in [true, false] {
                let replica = ReplicaState::new(5);
                replica.apply_local_edit(0, 0, &text);
                replica.local_fast_path_refused.set(refused);
                if refused {
                    replica.invalidate_local_indexes();
                }
                let mut anchor = text.chars().count() as u32 / 2;
                let mut samples = Vec::new();
                for round in 0..40 {
                    let started = std::time::Instant::now();
                    let before = replica.state_vector();
                    replica.apply_local_edit(anchor, 0, "x");
                    anchor += 1;
                    let update = replica.diff(&before).unwrap();
                    samples.push(started.elapsed());
                    if round == 0 {
                        assert_eq!(decode_update_ops(&update).unwrap().len(), 1);
                    }
                }
                samples.sort();
                // The first flush after a remote update, which rebuilds the
                // visible order from the graph (lazily's path re-projects too).
                let mut remote = TextCrdt::new(6);
                remote.apply_delta(&decode_update_ops(&replica.encode_state()).unwrap());
                let mut after_remote = Vec::new();
                for _ in 0..5 {
                    let frontier = remote.version_vector();
                    remote.insert_str(0, "r");
                    let update = encode_update_ops(&remote.delta_since(&frontier)).unwrap();
                    replica.apply_update(&update).unwrap();
                    let _ = replica.text();
                    let started = std::time::Instant::now();
                    let before = replica.state_vector();
                    replica.apply_local_edit(anchor, 0, "x");
                    anchor += 1;
                    replica.diff(&before).unwrap();
                    after_remote.push(started.elapsed());
                }
                after_remote.sort();
                println!(
                    "replica_flush_scaling kib={kib} path={} p50_us={:.1} after_remote_p50_us={:.1}",
                    if refused { "lazily" } else { "journal" },
                    samples[samples.len() / 2].as_secs_f64() * 1e6,
                    after_remote[after_remote.len() / 2].as_secs_f64() * 1e6
                );
            }
        }
    }
}
