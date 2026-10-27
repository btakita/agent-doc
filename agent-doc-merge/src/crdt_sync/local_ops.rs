//! Edit-proportional local minting, remote integration and delta extraction
//! for [`super::ReplicaState`] (`#replicadiffcost`, `#replicaremoteflush`).
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
//! - [`OrderIndex`] keeps every element of the operation graph in lazily's
//!   projection order, tombstones included (chunked, so a splice moves at most
//!   one chunk). A local edit reads its origin at visible `index - 1` and mints
//!   exactly the ids, origins and tombstones that `TextCrdt::insert_str` /
//!   `delete_range` would have minted, then hands them to
//!   `apply_delta_unprojected`. A remote update is folded in as it arrives (see
//!   [`OrderIndex::integrate_remote`]), so the first local flush after it does
//!   not rebuild the order from the graph.
//! - [`LocalJournal`] records every element whose stored state changed since an
//!   exact version-vector base, by the op ids that changed it. A peer whose
//!   state vector dominates that base has already seen every element outside the
//!   journal, so its delta is the journal's unseen events filtered by
//!   `delta_since`'s own predicate — the same operation set, found without
//!   scanning the document.
//!
//! An index whose visible projection disagrees with the replica's text is
//! refused, so the lazily path keeps running unchanged.

use lazily::{OpId, TextOp, TextVersionVector};
use std::collections::{BTreeMap, HashMap, HashSet};

/// One element of the operation graph, with exactly the state lazily stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OrderElem {
    pub(super) id: OpId,
    pub(super) origin: Option<OpId>,
    pub(super) ch: char,
    pub(super) deleted: Option<OpId>,
}

impl OrderElem {
    pub(super) fn from_op(op: &TextOp) -> Self {
        Self {
            id: op.id,
            origin: op.origin,
            ch: op.ch,
            deleted: op.deleted,
        }
    }

    pub(super) fn to_op(self) -> TextOp {
        TextOp {
            id: self.id,
            ch: self.ch,
            origin: self.origin,
            deleted: self.deleted,
        }
    }

    fn is_visible(&self) -> bool {
        self.deleted.is_none()
    }
}

/// lazily's tombstone merge (`apply_delta_unprojected` / `merge`): sticky, and
/// concurrent deletes converge to the smaller delete id.
pub(super) fn merge_deleted(known: Option<OpId>, incoming: Option<OpId>) -> Option<OpId> {
    match (known, incoming) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

const CHUNK_TARGET: usize = 1024;
const CHUNK_MAX: usize = 2 * CHUNK_TARGET;

/// A remote batch of more ops than this (or than an eighth of the index) is
/// cheaper to re-project from the graph than to fold in one by one, so
/// [`OrderIndex::integrate_remote`] declines it.
const REMOTE_FOLD_LIMIT: usize = 4096;

#[derive(Debug, Clone)]
struct Chunk {
    /// Stable identity, so [`OrderIndex::home`] survives chunk splits elsewhere.
    key: u32,
    elems: Vec<OrderElem>,
    /// Visible (non-tombstoned) elements in `elems`.
    visible: usize,
}

/// A position in the full (tombstones included) order: chunk index, offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pos {
    chunk: usize,
    offset: usize,
}

/// What [`OrderIndex::integrate_remote`] changed in the stored graph.
#[derive(Debug, Default)]
pub(super) struct RemoteEffect {
    /// The resulting stored state of every element the batch inserted or whose
    /// tombstone it changed, as `delta_since` would report it.
    pub(super) touched: Vec<TextOp>,
    /// Tombstone ids the batch replaced with a smaller concurrent delete. Such
    /// an id is no longer stored, so the version vector may have to shrink.
    pub(super) replaced_deletes: Vec<OpId>,
}

/// Every element of the replica's operation graph, in lazily's projection
/// order.
///
/// lazily projects the origin tree in pre-order from the document start,
/// same-origin siblings by descending `OpId`, tombstones kept for traversal and
/// dropped from the text, and an element whose origin is absent never reached.
/// The index keeps that full order (tombstones included, since a remote insert
/// may hang off one), the count of visible elements per chunk for visible-index
/// lookups, each element's chunk, and the elements lazily cannot reach yet.
#[derive(Debug, Clone, Default)]
pub(super) struct OrderIndex {
    chunks: Vec<Chunk>,
    next_key: u32,
    /// Reachable element id -> key of the chunk holding it.
    home: HashMap<OpId, u32>,
    /// Visible elements in the whole order.
    visible: usize,
    /// Stored elements lazily's traversal never reaches: their origin is
    /// absent (out-of-order delivery) or itself unreachable.
    unreachable: HashMap<OpId, OrderElem>,
    /// Origin id -> unreachable elements waiting on it.
    waiting: HashMap<OpId, Vec<OpId>>,
    /// Whether the visible projection was proven equal to lazily's text since
    /// the last remote fold. Local minting only ever runs on a verified index.
    verified: bool,
}

impl OrderIndex {
    /// An empty index (an empty graph), trivially verified.
    pub(super) fn empty() -> Self {
        Self {
            verified: true,
            ..Self::default()
        }
    }

    /// Visible elements: the length of the projected text.
    pub(super) fn len(&self) -> usize {
        self.visible
    }

    pub(super) fn is_verified(&self) -> bool {
        self.verified
    }

    pub(super) fn mark_verified(&mut self) {
        self.verified = true;
    }

    /// Every stored element, reachable or not.
    fn stored(&self) -> usize {
        self.home.len() + self.unreachable.len()
    }

    fn contains(&self, id: &OpId) -> bool {
        self.home.contains_key(id) || self.unreachable.contains_key(id)
    }

    fn new_chunk(&mut self, elems: Vec<OrderElem>) -> Chunk {
        let key = self.next_key;
        self.next_key += 1;
        for elem in &elems {
            self.home.insert(elem.id, key);
        }
        Chunk {
            key,
            visible: elems.iter().filter(|elem| elem.is_visible()).count(),
            elems,
        }
    }

    /// Project the order of a complete op set exactly as lazily's
    /// `TextCrdt::ordered_ids(true)` does. The result is unverified.
    pub(super) fn from_ops(ops: &[TextOp]) -> Self {
        let mut children: HashMap<Option<OpId>, Vec<usize>> = HashMap::new();
        for (index, op) in ops.iter().enumerate() {
            children.entry(op.origin).or_default().push(index);
        }
        for list in children.values_mut() {
            list.sort_unstable_by(|a, b| ops[*b].id.cmp(&ops[*a].id));
        }
        let mut index = Self::default();
        let mut reached = vec![false; ops.len()];
        let mut run = Vec::with_capacity(CHUNK_TARGET);
        let mut stack: Vec<usize> = children
            .get(&None)
            .map(|roots| roots.iter().rev().copied().collect())
            .unwrap_or_default();
        while let Some(at) = stack.pop() {
            reached[at] = true;
            run.push(OrderElem::from_op(&ops[at]));
            if run.len() == CHUNK_TARGET {
                let chunk = index.new_chunk(std::mem::replace(
                    &mut run,
                    Vec::with_capacity(CHUNK_TARGET),
                ));
                index.visible += chunk.visible;
                index.chunks.push(chunk);
            }
            if let Some(kids) = children.get(&Some(ops[at].id)) {
                stack.extend(kids.iter().rev().copied());
            }
        }
        if !run.is_empty() {
            let chunk = index.new_chunk(run);
            index.visible += chunk.visible;
            index.chunks.push(chunk);
        }
        for (at, op) in ops.iter().enumerate() {
            if !reached[at] {
                index.park(OrderElem::from_op(op));
            }
        }
        index
    }

    /// Record an element lazily cannot reach yet under its origin.
    fn park(&mut self, elem: OrderElem) {
        if let Some(origin) = elem.origin {
            self.waiting.entry(origin).or_default().push(elem.id);
        }
        self.unreachable.insert(elem.id, elem);
    }

    /// Whether the visible elements spell exactly `text`.
    pub(super) fn spells(&self, text: &str) -> bool {
        let mut chars = text.chars();
        for chunk in &self.chunks {
            for elem in chunk.elems.iter().filter(|elem| elem.is_visible()) {
                if chars.next() != Some(elem.ch) {
                    return false;
                }
            }
        }
        chars.next().is_none()
    }

    /// The position of visible element `index` (`index < len`).
    fn locate_visible(&self, index: usize) -> Pos {
        let mut remaining = index;
        for (chunk_index, chunk) in self.chunks.iter().enumerate() {
            if remaining < chunk.visible {
                for (offset, elem) in chunk.elems.iter().enumerate() {
                    if elem.is_visible() {
                        if remaining == 0 {
                            return Pos {
                                chunk: chunk_index,
                                offset,
                            };
                        }
                        remaining -= 1;
                    }
                }
            }
            remaining -= chunk.visible;
        }
        unreachable!(
            "visible index {index} is outside the order (len {})",
            self.visible
        )
    }

    pub(super) fn visible_at(&self, index: usize) -> Option<OrderElem> {
        (index < self.visible).then(|| {
            let pos = self.locate_visible(index);
            self.chunks[pos.chunk].elems[pos.offset]
        })
    }

    /// The position of a reachable element.
    fn position_of(&self, id: &OpId) -> Option<Pos> {
        let key = *self.home.get(id)?;
        let chunk = self.chunks.iter().position(|chunk| chunk.key == key)?;
        let offset = self.chunks[chunk]
            .elems
            .iter()
            .position(|elem| elem.id == *id)?;
        Some(Pos { chunk, offset })
    }

    /// The element at `pos`, moving a past-the-end offset to the next chunk.
    fn elem_at(&self, pos: &mut Pos) -> Option<OrderElem> {
        while pos.chunk < self.chunks.len() && pos.offset == self.chunks[pos.chunk].elems.len() {
            if pos.chunk + 1 == self.chunks.len() {
                return None;
            }
            pos.chunk += 1;
            pos.offset = 0;
        }
        self.chunks
            .get(pos.chunk)
            .and_then(|chunk| chunk.elems.get(pos.offset))
            .copied()
    }

    /// Insert `run` so its first element lands at `pos`, returning where it
    /// landed (a split moves it).
    fn insert_at(&mut self, pos: Pos, run: Vec<OrderElem>) -> Pos {
        debug_assert!(!run.is_empty());
        if self.chunks.is_empty() {
            let chunk = self.new_chunk(Vec::new());
            self.chunks.push(chunk);
        }
        let Pos { chunk, offset } = if pos.chunk >= self.chunks.len() {
            let last = self.chunks.len() - 1;
            Pos {
                chunk: last,
                offset: self.chunks[last].elems.len(),
            }
        } else {
            pos
        };
        let added_visible = run.iter().filter(|elem| elem.is_visible()).count();
        self.visible += added_visible;
        let key = self.chunks[chunk].key;
        if self.chunks[chunk].elems.len() + run.len() <= CHUNK_MAX {
            for elem in &run {
                self.home.insert(elem.id, key);
            }
            let target = &mut self.chunks[chunk];
            target.visible += added_visible;
            target.elems.splice(offset..offset, run);
            return Pos { chunk, offset };
        }
        let target = &mut self.chunks[chunk];
        let tail = target.elems.split_off(offset);
        let mut merged = std::mem::take(&mut target.elems);
        merged.extend(run);
        merged.extend(tail);
        let mut pieces = merged.chunks(CHUNK_TARGET);
        let first = pieces.next().expect("a split has elements").to_vec();
        let rest = pieces.map(<[OrderElem]>::to_vec).collect::<Vec<_>>();
        for elem in &first {
            self.home.insert(elem.id, key);
        }
        let target = &mut self.chunks[chunk];
        target.visible = first.iter().filter(|elem| elem.is_visible()).count();
        target.elems = first;
        let rest = rest
            .into_iter()
            .map(|elems| self.new_chunk(elems))
            .collect::<Vec<_>>();
        self.chunks.splice(chunk + 1..chunk + 1, rest);
        Pos {
            chunk: chunk + offset / CHUNK_TARGET,
            offset: offset % CHUNK_TARGET,
        }
    }

    /// Insert a locally minted chain at visible `offset`.
    ///
    /// lazily gives the chain's first element the largest id in the graph and
    /// the visible element before `offset` (or the document start) as origin,
    /// so it is that origin's first child: it lands immediately after the
    /// origin in the full order, ahead of any tombstones that follow it.
    pub(super) fn insert_local_run(&mut self, offset: usize, run: Vec<OrderElem>) {
        if run.is_empty() {
            return;
        }
        let pos = match offset.checked_sub(1) {
            None => Pos {
                chunk: 0,
                offset: 0,
            },
            Some(before) => {
                let at = self.locate_visible(before);
                Pos {
                    chunk: at.chunk,
                    offset: at.offset + 1,
                }
            }
        };
        self.insert_at(pos, run);
    }

    /// Tombstone `count` visible elements from visible `offset` (the caller
    /// clamps the range), minting each delete id in visible order as lazily's
    /// `delete_range` does, and return their new states.
    pub(super) fn delete_visible(
        &mut self,
        offset: usize,
        count: usize,
        mut mint: impl FnMut() -> OpId,
    ) -> Vec<TextOp> {
        debug_assert!(offset + count <= self.visible);
        let mut deleted = Vec::with_capacity(count);
        if count == 0 {
            return deleted;
        }
        let mut pos = self.locate_visible(offset);
        while deleted.len() < count {
            let chunk = &mut self.chunks[pos.chunk];
            for elem in &mut chunk.elems[pos.offset..] {
                if deleted.len() == count {
                    break;
                }
                if elem.is_visible() {
                    elem.deleted = Some(mint());
                    chunk.visible -= 1;
                    self.visible -= 1;
                    deleted.push(elem.to_op());
                }
            }
            pos = Pos {
                chunk: pos.chunk + 1,
                offset: 0,
            };
        }
        deleted
    }

    /// The stored state of `id`, reachable or not.
    fn state_of(&self, id: &OpId) -> Option<OrderElem> {
        if let Some(elem) = self.unreachable.get(id) {
            return Some(*elem);
        }
        let pos = self.position_of(id)?;
        Some(self.chunks[pos.chunk].elems[pos.offset])
    }

    /// Fold a remote batch into the index exactly as lazily's
    /// `apply_delta_unprojected` folds it into the graph, or decline (`None`,
    /// index untouched) a batch too large to fold cheaply or one the ordering
    /// argument below does not cover.
    ///
    /// Ops are taken in batch order like lazily: an unknown id becomes a new
    /// element with the op's tombstone, a known id merges its tombstone with
    /// [`merge_deleted`]. New elements are then placed in ascending id order,
    /// so an origin minted in the same batch is placed before its children.
    ///
    /// Placement (RGA integration): a new element `x` with origin `o` goes
    /// after `o`, past every following element whose id is greater than
    /// `x.id`, and before the first one whose id is smaller. That is exactly
    /// its place in lazily's pre-order with descending siblings, because every
    /// valid insert has a larger id than its origin (`validate_text_ops`):
    /// - a sibling `s > x` and its whole subtree (all descendants exceed `s`)
    ///   precede `x` and are skipped;
    /// - a sibling `s < x` is where the scan stops, and no descendant of a
    ///   larger sibling can stop it first;
    /// - the first element after `o`'s subtree hangs off a proper ancestor
    ///   branch below `o`'s ancestor `c`, so its id is below `c <= o < x`
    ///   and the scan stops there.
    ///
    /// An element whose origin is absent or unreachable waits in
    /// `unreachable`, exactly as lazily never reaches it, and is placed (with
    /// everything waiting on it) once its origin is placed.
    pub(super) fn integrate_remote(&mut self, ops: &[TextOp]) -> Option<RemoteEffect> {
        // Every op costs a lookup, known or not, so the whole batch is bounded.
        if ops.len() > REMOTE_FOLD_LIMIT.max(self.stored() / 8) {
            return None;
        }
        if ops
            .iter()
            .any(|op| !self.contains(&op.id) && op.origin.is_some_and(|origin| origin >= op.id))
        {
            return None;
        }
        let mut effect = RemoteEffect::default();
        let mut touched: Vec<OpId> = Vec::new();
        let mut fresh: HashMap<OpId, OrderElem> = HashMap::new();
        for op in ops {
            if let Some(elem) = self.unreachable.get_mut(&op.id) {
                let merged = merge_deleted(elem.deleted, op.deleted);
                if merged != elem.deleted {
                    effect.replaced_deletes.extend(elem.deleted);
                    elem.deleted = merged;
                    touched.push(op.id);
                }
            } else if let Some(pos) = self.position_of(&op.id) {
                let chunk = &mut self.chunks[pos.chunk];
                let elem = &mut chunk.elems[pos.offset];
                let merged = merge_deleted(elem.deleted, op.deleted);
                if merged != elem.deleted {
                    match elem.deleted {
                        None => {
                            chunk.visible -= 1;
                            self.visible -= 1;
                        }
                        Some(replaced) => effect.replaced_deletes.push(replaced),
                    }
                    elem.deleted = merged;
                    touched.push(op.id);
                }
            } else if let Some(elem) = fresh.get_mut(&op.id) {
                elem.deleted = merge_deleted(elem.deleted, op.deleted);
            } else {
                fresh.insert(op.id, OrderElem::from_op(op));
                touched.push(op.id);
            }
        }
        if !touched.is_empty() {
            self.verified = false;
        }
        let mut fresh = fresh.into_values().collect::<Vec<_>>();
        fresh.sort_unstable_by_key(|elem| elem.id);
        // The element placed last and where it landed: a typed run hangs each
        // character off the previous one, so its origin needs no lookup.
        let mut cursor: Option<(OpId, Pos)> = None;
        for elem in fresh {
            let origin_placed = match elem.origin {
                None => true,
                Some(origin) => self.home.contains_key(&origin),
            };
            if !origin_placed {
                self.park(elem);
                continue;
            }
            let mut pending = vec![elem];
            while let Some(next) = pending.pop() {
                cursor = Some((next.id, self.place(next, cursor)));
                if let Some(children) = self.waiting.remove(&next.id) {
                    pending.extend(
                        children
                            .iter()
                            .filter_map(|child| self.unreachable.remove(child)),
                    );
                }
            }
        }
        effect.touched = touched
            .iter()
            .filter_map(|id| self.state_of(id))
            .map(OrderElem::to_op)
            .collect();
        Some(effect)
    }

    /// Place one element whose origin is reachable (see
    /// [`Self::integrate_remote`]) and return where it landed.
    fn place(&mut self, elem: OrderElem, cursor: Option<(OpId, Pos)>) -> Pos {
        let mut pos = match elem.origin {
            None => Pos {
                chunk: 0,
                offset: 0,
            },
            Some(origin) => {
                let at = match cursor {
                    Some((id, at)) if id == origin => at,
                    _ => self
                        .position_of(&origin)
                        .expect("a placed origin has a position"),
                };
                Pos {
                    chunk: at.chunk,
                    offset: at.offset + 1,
                }
            }
        };
        while let Some(next) = self.elem_at(&mut pos) {
            if next.id < elem.id {
                break;
            }
            pos.offset += 1;
        }
        self.insert_at(pos, vec![elem])
    }
}

#[cfg(test)]
impl OrderIndex {
    /// The whole order, tombstones included.
    pub(super) fn full_order(&self) -> Vec<OrderElem> {
        let order = self
            .chunks
            .iter()
            .flat_map(|chunk| chunk.elems.iter().copied())
            .collect::<Vec<_>>();
        for chunk in &self.chunks {
            assert_eq!(
                chunk.visible,
                chunk.elems.iter().filter(|elem| elem.is_visible()).count(),
                "chunk visible count"
            );
            for elem in &chunk.elems {
                assert_eq!(self.home.get(&elem.id), Some(&chunk.key), "element home");
            }
        }
        assert_eq!(self.home.len(), order.len(), "home covers the order");
        order
    }

    pub(super) fn unreachable_ids(&self) -> Vec<OpId> {
        let mut ids = self.unreachable.keys().copied().collect::<Vec<_>>();
        ids.sort();
        let mut waiting = self.waiting.values().flatten().copied().collect::<Vec<_>>();
        waiting.sort();
        assert_eq!(
            waiting, ids,
            "every unreachable element waits on its origin"
        );
        ids
    }
}

/// Above this many events the journal is rebased at the next local flush or
/// remote fold, so a diff never walks more than a bounded suffix. A peer behind
/// the new base gets lazily's full `delta_since` instead (always correct,
/// merely O(document)).
const JOURNAL_REBASE_EVENTS: usize = 32_768;

/// Elements whose stored state changed since an exact version-vector `base`.
#[derive(Debug, Clone)]
pub(super) struct LocalJournal {
    /// The replica's exact version vector when the journal started.
    base: TextVersionVector,
    /// `(minted counter, element)` per locally minted op, in minting order.
    /// Local minting always takes a fresh counter, so counters strictly
    /// increase.
    events: Vec<(u64, OpId)>,
    /// `peer -> counter -> elements` for every op id a remote fold stored (an
    /// insert id, or the tombstone id an element now carries). Remote ops
    /// arrive in no particular counter order, hence the ordered map.
    remote_events: BTreeMap<u64, BTreeMap<u64, Vec<OpId>>>,
    remote_event_count: usize,
    /// Current state of every element an event names.
    elements: HashMap<OpId, TextOp>,
}

impl LocalJournal {
    pub(super) fn new(base: TextVersionVector) -> Self {
        Self {
            base,
            events: Vec::new(),
            remote_events: BTreeMap::new(),
            remote_event_count: 0,
            elements: HashMap::new(),
        }
    }

    pub(super) fn needs_rebase(&self) -> bool {
        self.events.len() + self.remote_event_count > JOURNAL_REBASE_EVENTS
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

    /// Record the new stored state of an element a remote fold inserted or
    /// re-tombstoned, under every op id that state carries.
    pub(super) fn record_remote(&mut self, op: &TextOp) {
        for id in std::iter::once(op.id).chain(op.deleted) {
            self.remote_events
                .entry(id.peer())
                .or_default()
                .entry(id.counter())
                .or_default()
                .push(op.id);
            self.remote_event_count += 1;
        }
        self.elements.insert(op.id, op.clone());
    }

    /// The ops `their_vv` has not observed — the same set as
    /// `TextCrdt::delta_since(their_vv)` — or `None` when `their_vv` does not
    /// dominate the base, so some unobserved op may predate the journal.
    ///
    /// Exactness: every element outside the journal holds only ops present at
    /// the base, whose counters are at most `base[peer]`; a dominating
    /// `their_vv` has seen them, so `delta_since` would skip them too. An
    /// element inside the journal is unseen only through an op stored after the
    /// base, which a local event (counter past `their_vv[local_peer]`) or a
    /// remote event (counter past `their_vv[peer]`) names; each candidate is
    /// tested with `delta_since`'s predicate on its current state.
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
        let seen_through = |peer: u64| their_vv.get(&peer).copied().unwrap_or(0);
        let seen = |id: OpId| id.counter() <= seen_through(id.peer());
        let floor = seen_through(local_peer);
        let start = self
            .events
            .partition_point(|(counter, _)| *counter <= floor);
        let local = self.events[start..].iter().map(|(_, element)| element);
        let remote = self.remote_events.iter().flat_map(|(peer, by_counter)| {
            by_counter
                .range(seen_through(*peer).saturating_add(1)..)
                .flat_map(|(_, elements)| elements)
        });
        let mut emitted = HashSet::new();
        let mut delta = Vec::new();
        for element in local.chain(remote) {
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

    /// The fold-maintained structures must describe the graph exactly whenever
    /// they claim to: a held version vector equals lazily's, and a held order
    /// (with no local edit pending) spells lazily's projection over the same
    /// full sequence lazily's own traversal produces.
    fn assert_derived_structures_exact(replica: &ReplicaState, context: &str) {
        // A full re-projection per step is quadratic on a large document, so
        // large documents re-project every few checks.
        thread_local!(static CHECKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) });
        let check = CHECKS.with(|checks| {
            checks.set(checks.get() + 1);
            checks.get()
        });
        if replica.text.borrow().len() > 8 * 1024 && !check.is_multiple_of(6) {
            return;
        }
        if let Some(vv) = replica.cached_version_vector.borrow().as_ref() {
            assert_eq!(
                vv,
                &replica.text.borrow().version_vector(),
                "{context}: maintained version vector"
            );
        }
        if !replica.pending_local_edits.borrow().is_empty() {
            return;
        }
        if let Some(order) = replica.visible_order.borrow().as_ref() {
            let text = replica.text.borrow();
            assert!(order.spells(&text.text()), "{context}: order spelling");
            // lazily's `len` also counts visible elements it cannot reach.
            assert_eq!(
                order.len(),
                text.text().chars().count(),
                "{context}: visible length"
            );
            let rebuilt = OrderIndex::from_ops(&text.delta_since(&TextVersionVector::new()));
            assert_same_order(order, &rebuilt, context);
        }
    }

    fn assert_same_order(order: &OrderIndex, rebuilt: &OrderIndex, context: &str) {
        assert_eq!(
            order.full_order(),
            rebuilt.full_order(),
            "{context}: full order (tombstones included)"
        );
        assert_eq!(
            order.unreachable_ids(),
            rebuilt.unreachable_ids(),
            "{context}: unreachable elements"
        );
    }

    /// Concurrent peers, each a plain lazily replica with its own clock.
    struct Peers {
        crdts: Vec<TextCrdt>,
        /// Encoded per-burst updates not yet delivered to the local replicas,
        /// delivered in random order (so origins can arrive after children).
        in_flight: Vec<Vec<u8>>,
        /// Updates already delivered, replayed now and then as duplicates.
        delivered: Vec<Vec<u8>>,
    }

    impl Peers {
        fn fork(seed_state: &[u8], count: usize) -> Self {
            let ops = decode_update_ops(seed_state).unwrap();
            let crdts = (0..count as u64)
                .map(|index| {
                    let mut crdt = TextCrdt::new(0x5150 + index);
                    crdt.apply_delta(&ops);
                    crdt
                })
                .collect();
            Self {
                crdts,
                in_flight: Vec::new(),
                delivered: Vec::new(),
            }
        }

        /// A burst on one peer. With `hot`, every peer edits the same offset,
        /// so concurrent inserts share an origin and deletes overlap.
        fn burst(&mut self, rng: &mut Rng, hot: Option<usize>) {
            let who = rng.below(self.crdts.len());
            let crdt = &mut self.crdts[who];
            let before = crdt.version_vector();
            for _ in 0..1 + rng.below(4) {
                let len = crdt.len();
                let (offset, delete_len, insert) = random_edit(rng, len);
                let offset = hot.map_or(offset, |hot| hot.min(len) as u32);
                lazily_edit(crdt, offset, delete_len, &insert);
            }
            let delta = crdt.delta_since(&before);
            if !delta.is_empty() {
                self.in_flight.push(encode_update_ops(&delta).unwrap());
            }
        }

        /// Two peers exchange everything, so later bursts build on each other.
        fn gossip(&mut self, rng: &mut Rng) {
            let (a, b) = (rng.below(self.crdts.len()), rng.below(self.crdts.len()));
            let delta = self.crdts[b].delta_since(&self.crdts[a].version_vector());
            self.crdts[a].apply_delta(&delta);
        }

        /// Hand one peer what the local replica holds and it lacks.
        fn hear_local(&mut self, rng: &mut Rng, local: &ReplicaState) {
            let who = rng.below(self.crdts.len());
            let theirs = serde_json::to_vec(&self.crdts[who].version_vector()).unwrap();
            let delta = decode_update_ops(&local.diff(&theirs).unwrap()).unwrap();
            self.crdts[who].apply_delta(&delta);
        }

        /// The next update to deliver: a random in-flight one, or (rarely) a
        /// duplicate of a delivered one.
        fn next_delivery(&mut self, rng: &mut Rng) -> Option<Vec<u8>> {
            if !self.delivered.is_empty() && rng.below(8) == 0 {
                return Some(self.delivered[rng.below(self.delivered.len())].clone());
            }
            if self.in_flight.is_empty() {
                return None;
            }
            let update = self.in_flight.remove(rng.below(self.in_flight.len()));
            self.delivered.push(update.clone());
            if self.delivered.len() > 16 {
                self.delivered.remove(0);
            }
            Some(update)
        }
    }

    fn run_differential(seed: u64, document_bytes: usize, steps: usize, peer_count: usize) {
        let mut rng = Rng(seed);
        let local_peer = 0x6B65_7901;
        let seed_text = seed_document(document_bytes);
        // Open + seed exactly as the editor (and the keystroke bench) does.
        let replica = ReplicaState::new(local_peer);
        let reference = lazily_reference(local_peer);
        for side in [&replica, &reference] {
            side.apply_local_edit(0, 0, &seed_text);
        }
        // Concurrent peers that fork from the seed and type anywhere.
        let mut peers = Peers::fork(&reference.encode_state(), peer_count);

        // Every state-bearing call goes to both sides: it is the flush boundary,
        // so calling one side alone would compose different pending edits.
        let seeded = replica.state_vector();
        assert_eq!(seeded, reference.state_vector(), "seed frontier");
        let mut frontiers = vec![
            serde_json::to_vec(&TextVersionVector::new()).unwrap(),
            seeded,
        ];
        let mut journal_answers = 0usize;
        let mut folds = 0usize;
        for step in 0..steps {
            let context = format!("seed {seed:#x} step {step}");
            match rng.below(24) {
                // Peer bursts, concurrent at one hot offset half the time.
                0..=2 => {
                    let len = peers.crdts[0].len();
                    let hot = (rng.below(2) == 0).then(|| rng.below(len + 1));
                    for _ in 0..1 + rng.below(peers.crdts.len()) {
                        peers.burst(&mut rng, hot);
                    }
                    if rng.below(3) == 0 {
                        peers.gossip(&mut rng);
                    }
                }
                // Deliver one update (possibly out of order, possibly a
                // duplicate) to both replicas.
                3..=5 => {
                    let Some(update) = peers.next_delivery(&mut rng) else {
                        continue;
                    };
                    replica.apply_update(&update).unwrap();
                    reference.apply_update(&update).unwrap();
                    folds += usize::from(replica.visible_order.borrow().is_some());
                    assert_derived_structures_exact(&replica, &context);
                }
                // A peer hears the local edits (so it can delete local text,
                // and the local side can later delete its echoes).
                6 => {
                    let frontier = replica.state_vector();
                    assert_eq!(frontier, reference.state_vector(), "{context}: frontier");
                    peers.hear_local(&mut rng, &reference);
                }
                // Snapshot a frontier the forwarder could later diff against.
                7 | 8 => {
                    let frontier = replica.state_vector();
                    assert_eq!(frontier, reference.state_vector(), "{context}: snapshot");
                    frontiers.push(frontier);
                    if frontiers.len() > 6 {
                        frontiers.remove(2);
                    }
                }
                // A full equivalence check (flushes the pending edits).
                9 => {
                    assert_equivalent(&replica, &reference, &frontiers, &context);
                    assert_derived_structures_exact(&replica, &context);
                }
                // The forwarder's flush: frontier, burst, diff against it.
                10..=16 => {
                    let before = replica.state_vector();
                    assert_eq!(before, reference.state_vector(), "{context}: frontier");
                    for _ in 0..1 + rng.below(16) {
                        let len = reference.text().chars().count();
                        let (offset, delete_len, insert) = random_edit(&mut rng, len);
                        replica.apply_local_edit(offset, delete_len, &insert);
                        reference.apply_local_edit(offset, delete_len, &insert);
                    }
                    let base_dominated = replica.local_journal.borrow().is_some();
                    let ours = decode_update_ops(&replica.diff(&before).unwrap()).unwrap();
                    let theirs = decode_update_ops(&reference.diff(&before).unwrap()).unwrap();
                    assert_eq!(sorted(ours), sorted(theirs), "{context}: flush delta");
                    journal_answers += usize::from(base_dominated);
                    assert_derived_structures_exact(&replica, &context);
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
        // Drain every update still in flight, then compare everything.
        peers.delivered.clear();
        while let Some(update) = peers.next_delivery(&mut rng) {
            replica.apply_update(&update).unwrap();
            reference.apply_update(&update).unwrap();
            assert_derived_structures_exact(&replica, "drain");
        }
        for crdt in &peers.crdts {
            frontiers.push(serde_json::to_vec(&crdt.version_vector()).unwrap());
        }
        assert_equivalent(&replica, &reference, &frontiers, "final");
        assert!(
            !replica.local_fast_path_refused.get(),
            "a consistent replica must keep the edit-proportional path"
        );
        assert!(
            journal_answers > 0,
            "the journal path must actually be exercised"
        );
        assert!(folds > 0, "remote updates must actually be folded");
    }

    #[test]
    fn local_minting_and_journal_deltas_match_lazily_on_small_documents() {
        for seed in 1..=24u64 {
            run_differential(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1, 256, 160, 1);
        }
    }

    #[test]
    fn local_minting_and_journal_deltas_match_lazily_on_large_documents() {
        for seed in [7u64, 1_234_567, 0xDEAD_BEEF] {
            run_differential(seed, 48 * 1024, 48, 1);
        }
    }

    /// Two or three concurrent peers, bursts at one shared offset (concurrent
    /// inserts at the same origin, overlapping deletes), out-of-order and
    /// duplicate delivery, multibyte text, and local deletes of remote text.
    fn run_concurrent_peers(peers: usize) {
        for seed in (1..=20u64).filter(|seed| 2 + (*seed as usize % 2) == peers) {
            run_differential(
                seed.wrapping_mul(0xD1B5_4A32_D192_ED03) | 1,
                384,
                200,
                peers,
            );
        }
    }

    #[test]
    fn remote_folds_match_lazily_with_two_concurrent_peers() {
        run_concurrent_peers(2);
    }

    #[test]
    fn remote_folds_match_lazily_with_three_concurrent_peers() {
        run_concurrent_peers(3);
    }

    #[test]
    fn remote_folds_match_lazily_on_a_large_document() {
        for seed in [3u64, 0xFEED_F00D] {
            run_differential(seed, 40 * 1024, 64, 3);
        }
    }

    /// Property: an index folded op batch by op batch equals lazily's own
    /// projection after EVERY batch — the full order, tombstones included, the
    /// unreachable set, and the spelled text — across random concurrent
    /// graphs with out-of-order and duplicate delivery.
    #[test]
    fn folded_order_equals_lazily_after_every_remote_batch() {
        for seed in 1..=60u64 {
            let mut rng = Rng(seed.wrapping_mul(0xA24B_AED4_963E_E407) | 1);
            let base = TextCrdt::from_str(1, "shared base é✓ text\n");
            let base_state =
                encode_update_ops(&base.delta_since(&TextVersionVector::new())).unwrap();
            let mut peers = Peers::fork(&base_state, 3);
            let mut observer = TextCrdt::new(99);
            observer.apply_delta(&decode_update_ops(&base_state).unwrap());
            let mut order = OrderIndex::from_ops(&observer.delta_since(&TextVersionVector::new()));
            for batch in 0..120 {
                let context = format!("seed {seed} batch {batch}");
                let len = peers.crdts[0].len();
                let hot = (rng.below(3) == 0).then(|| rng.below(len + 1));
                peers.burst(&mut rng, hot);
                if rng.below(3) == 0 {
                    peers.gossip(&mut rng);
                }
                if rng.below(2) == 0 {
                    continue;
                }
                let Some(update) = peers.next_delivery(&mut rng) else {
                    continue;
                };
                let ops = decode_update_ops(&update).unwrap();
                let before_graph = sorted(observer.delta_since(&TextVersionVector::new()));
                observer.apply_delta_unprojected(&ops);
                let effect = order
                    .integrate_remote(&ops)
                    .unwrap_or_else(|| panic!("{context}: a small batch must fold"));
                // The fold reports exactly the elements whose stored state changed.
                let after_graph = sorted(observer.delta_since(&TextVersionVector::new()));
                let changed = after_graph
                    .iter()
                    .filter(|op| !before_graph.contains(op))
                    .cloned()
                    .collect::<Vec<_>>();
                assert_eq!(sorted(effect.touched), changed, "{context}: touched states");
                // A replaced tombstone is the only way a stored id disappears.
                for replaced in &effect.replaced_deletes {
                    assert!(
                        after_graph.iter().all(|op| op.deleted != Some(*replaced)),
                        "{context}: replaced delete still stored"
                    );
                }
                assert!(order.spells(&observer.text()), "{context}: spelling");
                assert_eq!(
                    order.len(),
                    observer.text().chars().count(),
                    "{context}: length"
                );
                let rebuilt =
                    OrderIndex::from_ops(&observer.delta_since(&TextVersionVector::new()));
                assert_same_order(&order, &rebuilt, &context);
            }
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
                let order = OrderIndex::from_ops(&ops);
                assert!(
                    order.spells(&peer.text()),
                    "seed {seed}: derived order diverged"
                );
                assert_eq!(order.len(), peer.len());
            }
        }
    }

    /// Local splices across chunk boundaries and splits keep the full order a
    /// vector model predicts: an insert lands right after the visible element
    /// before it (ahead of following tombstones), a delete tombstones in place.
    #[test]
    fn chunked_order_splices_like_a_vector() {
        let mut rng = Rng(99);
        let mut order = OrderIndex::empty();
        let mut model: Vec<OrderElem> = Vec::new();
        let base = TextCrdt::from_str(1, &"x".repeat(400_000));
        let mut ids = base
            .delta_since(&TextVersionVector::new())
            .into_iter()
            .map(|op| op.id)
            .collect::<Vec<_>>();
        ids.sort();
        let mut ids = ids.into_iter();
        let visible_positions = |model: &[OrderElem]| -> Vec<usize> {
            (0..model.len())
                .filter(|at| model[*at].deleted.is_none())
                .collect()
        };
        for _ in 0..300 {
            let visible = visible_positions(&model);
            if rng.below(3) == 0 && !visible.is_empty() {
                let offset = rng.below(visible.len());
                let count = rng.below(3_000).min(visible.len() - offset);
                let mut expected = Vec::new();
                for at in &visible[offset..offset + count] {
                    model[*at].deleted = Some(ids.next().unwrap());
                    expected.push(model[*at].to_op());
                }
                let mut tombstones = expected.iter().map(|op| op.deleted.unwrap());
                let deleted = order.delete_visible(offset, count, || tombstones.next().unwrap());
                assert_eq!(deleted, expected);
            } else {
                let offset = rng.below(visible.len() + 1);
                let count = [1, 2, 5, 700, 2_500][rng.below(5)];
                let run = (0..count)
                    .map(|_| OrderElem {
                        id: ids.next().unwrap(),
                        origin: None,
                        ch: 'x',
                        deleted: None,
                    })
                    .collect::<Vec<_>>();
                let at = offset
                    .checked_sub(1)
                    .map_or(0, |before| visible[before] + 1);
                model.splice(at..at, run.clone());
                order.insert_local_run(offset, run);
            }
            assert_eq!(order.full_order(), model);
            assert_eq!(order.len(), visible_positions(&model).len());
            for probe in [0, order.len() / 2, order.len().saturating_sub(1)] {
                let expected = visible_positions(&model).get(probe).map(|at| model[*at]);
                assert_eq!(order.visible_at(probe), expected);
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

    /// A folded order is re-proven against lazily's projection before it may
    /// place an edit; one that misspells it (here: corrupted after a fold) is
    /// refused for the replica's lifetime, and the replica still mints exactly
    /// lazily's operations.
    #[test]
    fn a_folded_order_that_misspells_lazily_is_refused() {
        let replica = ReplicaState::new(21);
        let reference = lazily_reference(21);
        for side in [&replica, &reference] {
            side.apply_local_edit(0, 0, "alpha beta gamma\n");
        }
        let mut remote = TextCrdt::new(22);
        remote.apply_delta(&decode_update_ops(&reference.encode_state()).unwrap());
        let frontier = remote.version_vector();
        remote.insert_str(6, "remote ");
        let update = encode_update_ops(&remote.delta_since(&frontier)).unwrap();
        for side in [&replica, &reference] {
            side.apply_update(&update).unwrap();
        }
        {
            let mut order = replica.visible_order.borrow_mut();
            let order = order
                .as_mut()
                .expect("a small update is folded, not dropped");
            assert!(!order.is_verified(), "a fold needs a fresh proof");
            order.chunks[0].elems[0].ch = 'X';
        }
        for side in [&replica, &reference] {
            side.apply_local_edit(3, 1, "Z");
        }
        assert!(replica.local_fast_path_refused.get());
        assert!(replica.visible_order.borrow().is_none());
        assert_equivalent(&replica, &reference, &[], "after refusal");
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
