//! The visible text's changeset between text revisions
//! (`#incrementalreconcile`), for [`super::ReplicaState::text_changes_since`].
//!
//! A reader that keeps derived state per document revision (the controller's
//! queue reconcile cache) needs the changeset from the revision it last read
//! to the current one. Comparing the two texts reads both whole, and on a
//! session document most of those bytes are `agent:exchange`, which the
//! reader does not even keep. The replica already knows each change where it
//! makes it: a local edit is one splice, and a folded remote batch changes
//! exactly the elements it inserted or tombstoned. The journal keeps those
//! splices, keyed by the text revision each change produced.

use std::collections::VecDeque;

/// One splice of the visible text: replace `removed_len` bytes at byte
/// `start` with `inserted`. A changeset is a sequence of splices, each
/// addressing the text the splices before it produced. Offsets are char
/// boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSplice {
    pub start: usize,
    pub removed_len: usize,
    pub inserted: String,
}

/// Changes kept before the oldest are dropped. A reader asks once per
/// maintenance run, so the bound only matters for a reader that stopped
/// asking; it then compares texts once and restarts.
const MAX_ENTRIES: usize = 16_384;
/// Inserted bytes kept (plus a small per-splice charge) before the oldest
/// changes are dropped.
const MAX_BYTES: usize = 8 * 1024 * 1024;
const SPLICE_CHARGE: usize = 32;

/// Splices per text revision, from `base` on.
#[derive(Debug)]
pub(super) struct ChangeJournal {
    /// The revision the first entry starts from (the last one when empty).
    base: u64,
    /// `(revision produced, splices)` in order; each entry starts from the
    /// revision the one before it produced.
    entries: VecDeque<(u64, Vec<TextSplice>)>,
    bytes: usize,
}

fn charge(splices: &[TextSplice]) -> usize {
    splices
        .iter()
        .map(|splice| splice.inserted.len() + SPLICE_CHARGE)
        .sum()
}

impl ChangeJournal {
    pub(super) fn new(base: u64) -> Self {
        Self {
            base,
            entries: VecDeque::new(),
            bytes: 0,
        }
    }

    /// The revision the journal has reached.
    fn head(&self) -> u64 {
        self.entries
            .back()
            .map_or(self.base, |(revision, _)| *revision)
    }

    /// Record the change from revision `from` to `to`. A change that does not
    /// start where the journal stands restarts it at `to`.
    pub(super) fn record(&mut self, from: u64, to: u64, splices: Vec<TextSplice>) {
        if from != self.head() {
            self.restart(to);
            return;
        }
        self.bytes += charge(&splices);
        self.entries.push_back((to, splices));
        while self.entries.len() > MAX_ENTRIES || self.bytes > MAX_BYTES {
            let Some((revision, splices)) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= charge(&splices);
            self.base = revision;
        }
    }

    /// Forget every change: the text at `revision` is known only whole.
    pub(super) fn restart(&mut self, revision: u64) {
        self.base = revision;
        self.entries.clear();
        self.bytes = 0;
    }

    /// The splices from `revision` to the head, dropping the changes before
    /// `revision`; `None` when the journal does not reach back to it.
    pub(super) fn since(&mut self, revision: u64) -> Option<Vec<TextSplice>> {
        let skip = if revision == self.base {
            0
        } else {
            self.entries
                .iter()
                .position(|(produced, _)| *produced == revision)?
                + 1
        };
        for (produced, splices) in self.entries.drain(..skip) {
            self.bytes -= charge(&splices);
            self.base = produced;
        }
        Some(
            self.entries
                .iter()
                .flat_map(|(_, splices)| splices.iter().cloned())
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(start: usize, text: &str) -> Vec<TextSplice> {
        vec![TextSplice {
            start,
            removed_len: 0,
            inserted: text.to_string(),
        }]
    }

    #[test]
    fn the_journal_answers_from_any_revision_it_still_holds() {
        let mut journal = ChangeJournal::new(1);
        journal.record(1, 2, insert(0, "a"));
        journal.record(2, 3, insert(1, "b"));
        journal.record(3, 4, insert(2, "c"));
        assert_eq!(journal.since(9), None);
        let from_two = journal.since(2).unwrap();
        assert_eq!(from_two, [insert(1, "b"), insert(2, "c")].concat());
        // Changes before the revision asked from are gone.
        assert_eq!(journal.since(1), None);
        assert_eq!(journal.since(2), Some(from_two));
        assert_eq!(journal.since(4), Some(Vec::new()));
        assert_eq!(journal.since(2), None);
    }

    #[test]
    fn a_change_from_elsewhere_restarts_the_journal() {
        let mut journal = ChangeJournal::new(1);
        journal.record(1, 2, insert(0, "a"));
        journal.record(7, 8, insert(0, "b"));
        assert_eq!(journal.since(1), None);
        assert_eq!(journal.since(8), Some(Vec::new()));
        journal.restart(9);
        assert_eq!(journal.since(8), None);
        assert_eq!(journal.since(9), Some(Vec::new()));
    }

    #[test]
    fn the_journal_is_bounded() {
        let mut journal = ChangeJournal::new(0);
        for revision in 0..(MAX_ENTRIES as u64 + 10) {
            journal.record(revision, revision + 1, insert(0, "x"));
        }
        assert_eq!(journal.entries.len(), MAX_ENTRIES);
        assert_eq!(journal.since(0), None);
        assert_eq!(
            journal.since(10).map(|splices| splices.len()),
            Some(MAX_ENTRIES)
        );
    }
}
