//! Edit alignment between two snapshots of a queue's heads (`#claimfollowedit`).
//!
//! One rule decides which line of a newer queue "is" a line of an older one,
//! shared by mid-turn steering (which reports `previous` -> `verbatim` edits)
//! and the queue-claim ledger (whose claims follow those same edits). Before
//! this was shared, steering forwarded an in-place edit of a claimed line to
//! its owner while the ledger, keyed by the line's text, dropped the claim and
//! re-offered the edited line as new dispatch work (GH #272 line, 2026-10-15).
//!
//! Pairing runs in two passes:
//!
//! 1. **Identity.** Myers over the heads' work identities
//!    ([`queue_head_identity`]) aligns the untouched run; a head whose identity
//!    survives outside that run (a moved line) still pairs with itself.
//! 2. **Edit.** Inside each Myers replace hunk, the lines left without an
//!    identity partner pair by text similarity ([`pair_replaced_lines`]): one
//!    contains the other, or their character-diff ratio is at least
//!    [`EDIT_SIMILARITY_THRESHOLD`]. Two rows that carry *different* tracked
//!    ids never pair, and a row that gains or loses its id pairs only by
//!    containment.
//!
//! Anything else is a replacement, not an edit (`#krwr`): an old row with no
//! partner is a deletion and a new row with no partner is new work, even when
//! the new row sits in the old row's slot of an equal-count hunk. Position is
//! not identity. A claimed row replaced this way ends its claim through the
//! deletion path (`cancel_claimed`); the new row is unclaimed. A line deleted
//! in one hunk and a line added in another never pair either, so a consumed
//! head followed by an operator addition elsewhere is a delete plus an add.

use agent_doc_element_queue::queue_head_identity;

/// Normalize a queue head for comparison: progress, pin and priority markers
/// are cosmetic and never read as an operator edit.
pub fn normalize_queue_head_text(text: &str) -> String {
    agent_doc_document::queue_projection::strip_priority_markers(
        &agent_doc_document::queue_projection::strip_in_progress_marker(text),
    )
    .replace("\r\n", "\n")
    .trim()
    .to_string()
}

/// The comparable work-identity key of a normalized head.
fn identity_key(norm: &str) -> String {
    format!("{:?}", queue_head_identity(norm))
}

/// How the heads of a newer queue snapshot pair with an older one.
#[derive(Debug, Clone)]
pub struct QueueHeadPairing {
    /// Per new head: the old head it continues, and whether the pair is an
    /// edit (replace-hunk paired) rather than an identity match.
    pub pair_of_new: Vec<Option<(usize, bool)>>,
    /// Per old head: whether some new head continues it.
    pub old_paired: Vec<bool>,
    /// The Myers ops over the identity keys, in order, for callers that emit
    /// the alignment as events.
    pub ops: Vec<similar::DiffOp>,
}

impl QueueHeadPairing {
    /// The new head that continues old head `old`, if any.
    pub fn new_index_of(&self, old: usize) -> Option<usize> {
        self.pair_of_new
            .iter()
            .position(|pair| matches!(pair, Some((o, _)) if *o == old))
    }
}

/// Pair the heads of `new` with the heads of `old` (both normalized with
/// [`normalize_queue_head_text`]).
pub fn pair_queue_heads(old: &[String], new: &[String]) -> QueueHeadPairing {
    let old_keys: Vec<String> = old.iter().map(|norm| identity_key(norm)).collect();
    let new_keys: Vec<String> = new.iter().map(|norm| identity_key(norm)).collect();
    let ops = similar::capture_diff_slices(similar::Algorithm::Myers, &old_keys, &new_keys);
    let mut pair_of_new: Vec<Option<(usize, bool)>> = vec![None; new.len()];
    let mut old_paired = vec![false; old.len()];
    for op in &ops {
        if let similar::DiffOp::Equal {
            old_index,
            new_index,
            len,
        } = *op
        {
            for offset in 0..len {
                pair_of_new[new_index + offset] = Some((old_index + offset, false));
                old_paired[old_index + offset] = true;
            }
        }
    }
    for n in 0..new.len() {
        if pair_of_new[n].is_some() {
            continue;
        }
        if let Some(o) = (0..old.len()).find(|&o| !old_paired[o] && old_keys[o] == new_keys[n]) {
            pair_of_new[n] = Some((o, false));
            old_paired[o] = true;
        }
    }
    for op in &ops {
        let similar::DiffOp::Replace {
            old_index,
            old_len,
            new_index,
            new_len,
        } = *op
        else {
            continue;
        };
        let olds: Vec<usize> = (old_index..old_index + old_len)
            .filter(|&o| !old_paired[o])
            .collect();
        let news: Vec<usize> = (new_index..new_index + new_len)
            .filter(|&n| pair_of_new[n].is_none())
            .collect();
        let old_texts: Vec<String> = olds.iter().map(|&o| old[o].clone()).collect();
        let new_texts: Vec<String> = news.iter().map(|&n| new[n].clone()).collect();
        for (po, pn) in pair_replaced_lines(&old_texts, &new_texts) {
            pair_of_new[news[pn]] = Some((olds[po], true));
            old_paired[olds[po]] = true;
        }
    }
    QueueHeadPairing {
        pair_of_new,
        old_paired,
        ops,
    }
}

/// Minimum character-diff ratio at which two id-less rows of one replace hunk
/// read as one row's text edit rather than a delete plus an add.
pub const EDIT_SIMILARITY_THRESHOLD: f32 = 0.5;

/// Pair a replaced block as row edits: `(old_offset, new_offset)`.
///
/// Two rows pair when one's text contains the other's (an annotation, a
/// trailing note) or their character-diff ratio is at least
/// [`EDIT_SIMILARITY_THRESHOLD`] (a typo fix, a reworded clause). The most
/// similar pairs win, one-to-one. Two rows that both carry a tracked id never
/// pair here: equal ids already paired by identity, so their ids differ and
/// they name different work. A row that gains or loses its tracked id pairs
/// only by containment (`[#a]` -> `[#a]: note`, `fix x` -> `do [#x] fix x`):
/// a character ratio between prose and an id slug is noise.
///
/// There is no positional fallback (`#krwr`). Every row left unpaired is a
/// deletion (old) or new work (new), even in an equal-count hunk where a new
/// row took a deleted row's slot. Deleting a claimed row while adding a
/// different one in its place therefore cancels the claim instead of handing
/// it to the new row as an edit; only a text edit of the same row, or the
/// same tracked id with rewritten prose, steers the owner.
pub fn pair_replaced_lines(old: &[String], new: &[String]) -> Vec<(usize, usize)> {
    let mut scored: Vec<(f32, usize, usize)> = Vec::new();
    for (o, old_text) in old.iter().enumerate() {
        let old_identity = queue_head_identity(old_text);
        for (n, new_text) in new.iter().enumerate() {
            let id_backed = [
                old_identity.is_id_backed(),
                queue_head_identity(new_text).is_id_backed(),
            ];
            if id_backed == [true, true] {
                continue;
            }
            let contains =
                new_text.contains(old_text.as_str()) || old_text.contains(new_text.as_str());
            let score = if contains {
                1.0
            } else if id_backed.contains(&true) {
                // A character ratio between prose and an id slug measures
                // nothing (`release + publish` vs `do [#ridernavp6republish]`
                // scores above the threshold).
                continue;
            } else {
                similar::TextDiff::from_chars(old_text.as_str(), new_text.as_str()).ratio()
            };
            if score >= EDIT_SIMILARITY_THRESHOLD {
                scored.push((score, o, n));
            }
        }
    }
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
    });
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (_, o, n) in scored {
        if !pairs.iter().any(|(po, pn)| *po == o || *pn == n) {
            pairs.push((o, n));
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heads(lines: &[&str]) -> Vec<String> {
        lines
            .iter()
            .map(|line| normalize_queue_head_text(line))
            .collect()
    }

    #[test]
    fn an_in_place_url_edit_pairs_as_an_edit() {
        let old = heads(&[
            "#gh-fix https://github.com/btakita/agent-doc/issues/271",
            "#gh-fix https://github.com/btakita/agent-foc/issues/272",
            "release + publish",
        ]);
        let new = heads(&[
            "#gh-fix https://github.com/btakita/agent-doc/issues/271",
            "#gh-fix https://github.com/btakita/agent-doc/issues/272",
            "release + publish",
        ]);
        let pairing = pair_queue_heads(&old, &new);
        assert_eq!(pairing.pair_of_new[1], Some((1, true)));
        assert_eq!(pairing.new_index_of(1), Some(1));
        assert_eq!(pairing.new_index_of(0), Some(0));
        assert_eq!(pairing.new_index_of(2), Some(2));
    }

    #[test]
    fn markers_do_not_break_identity_pairing() {
        let old = heads(&["do [#a]", "#gh-fix https://x.test/issues/1"]);
        let new = heads(&["🚧 do [#a]", "🚧 #gh-fix https://x.test/issues/1"]);
        let pairing = pair_queue_heads(&old, &new);
        assert_eq!(
            pairing.pair_of_new,
            vec![Some((0, false)), Some((1, false))]
        );
    }

    /// `#krwr`: a different row in a deleted row's slot is a delete plus an
    /// add, never an edit, whether the ids or the prose differ.
    #[test]
    fn a_same_slot_replacement_is_a_delete_plus_an_add() {
        let pairing = pair_queue_heads(&heads(&["do [#a]"]), &heads(&["do [#b]"]));
        assert_eq!(pairing.pair_of_new, vec![None], "a different tracked id");
        assert_eq!(pairing.old_paired, vec![false]);

        let pairing = pair_queue_heads(
            &heads(&["rewrite the deployment guide"]),
            &heads(&["replace this with a wholly different request"]),
        );
        assert_eq!(pairing.pair_of_new, vec![None], "unrelated prose");
        assert_eq!(pairing.old_paired, vec![false]);

        let pairing = pair_queue_heads(
            &heads(&["current task", "do [#a]", "later work"]),
            &heads(&[
                "current task",
                "#gh-fix https://x.test/issues/380",
                "later work",
            ]),
        );
        assert_eq!(pairing.pair_of_new[1], None, "an id row replaced by prose");
        assert_eq!(pairing.new_index_of(1), None);
        assert_eq!(pairing.new_index_of(0), Some(0));
        assert_eq!(pairing.new_index_of(2), Some(2));
    }

    /// `#krwr`: the edits that still continue their row.
    #[test]
    fn same_row_edits_still_pair() {
        // Same tracked id, prose rewritten: identity.
        let pairing = pair_queue_heads(
            &heads(&["do [#a]"]),
            &heads(&["do [#a]: actually rewrite it as a migration instead"]),
        );
        assert_eq!(pairing.pair_of_new, vec![Some((0, false))]);

        // A free-text typo fix or reworded clause: similarity.
        let pairing = pair_queue_heads(
            &heads(&["fix the login redirect bug"]),
            &heads(&["fix the login redirect loop bug on Safari"]),
        );
        assert_eq!(pairing.pair_of_new, vec![Some((0, true))]);

        // A row that gains an annotation and so loses its tracked id.
        let pairing = pair_queue_heads(
            &heads(&["[#a]"]),
            &heads(&["[#a]: also cover the sibling queue"]),
        );
        assert_eq!(pairing.pair_of_new, vec![Some((0, true))]);
    }

    #[test]
    fn a_distant_add_is_not_an_edit() {
        // The claimed line is deleted and a similar line is added elsewhere:
        // two hunks, never one edit.
        let old = heads(&["inline task", "#gh-fix https://x.test/issues/7"]);
        let new = heads(&["#gh-fix https://x.test/issues/8", "inline task"]);
        let pairing = pair_queue_heads(&old, &new);
        assert_eq!(pairing.new_index_of(1), None);
        assert_eq!(pairing.new_index_of(0), Some(1));
    }

    #[test]
    fn a_semantic_pair_never_drags_its_neighbours_along() {
        let old = heads(&["current task", "later work"]);
        let new = heads(&["later work, plus docs", "new subagent work"]);
        let pairing = pair_queue_heads(&old, &new);
        assert_eq!(pairing.pair_of_new[0], Some((1, true)));
        assert_eq!(pairing.pair_of_new[1], None);
        assert_eq!(pairing.new_index_of(0), None);
    }

    #[test]
    fn duplicate_lines_pair_one_to_one() {
        let old = heads(&[
            "#gh-fix https://x.test/a/272",
            "#gh-fix https://x.test/a/272",
        ]);
        let new = heads(&[
            "#gh-fix https://x.test/b/272",
            "#gh-fix https://x.test/a/272",
        ]);
        let pairing = pair_queue_heads(&old, &new);
        let mut continued: Vec<usize> = (0..old.len())
            .filter_map(|o| pairing.new_index_of(o))
            .collect();
        continued.sort();
        assert_eq!(
            continued,
            vec![0, 1],
            "each copy continues exactly one line"
        );
    }
}
