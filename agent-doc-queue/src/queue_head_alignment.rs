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
//!    identity partner are paired greedily by text similarity (containment, or
//!    a character ratio of at least 0.5). Id-backed heads never pair this way:
//!    a tracked id IS the item, so `do [#a]` -> `do [#b]` is a different item
//!    however alike the text.
//!
//! A line deleted in one hunk and a line added in another never pair, so a
//! consumed head followed by an operator addition elsewhere is a delete plus an
//! add, not an edit.

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
    /// edit (similarity-paired) rather than an identity match.
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

/// Greedy best-match pairing of a replaced block: `(old_offset, new_offset)`
/// for every pair whose texts are similar enough to be one edited item
/// (containment, or a character similarity ratio of at least 0.5).
pub fn pair_replaced_lines(old: &[String], new: &[String]) -> Vec<(usize, usize)> {
    let mut scored: Vec<(f32, usize, usize)> = Vec::new();
    for (o, old_text) in old.iter().enumerate() {
        let old_identity = queue_head_identity(old_text);
        for (n, new_text) in new.iter().enumerate() {
            // A tracked id IS the item's identity: two different ids are two
            // different items however alike their text (`do [#ridernavp4]`
            // vs `do [#ridernavp6route]`), and neither is the same item as a
            // free-text line. Only free text, whose identity is its text, can
            // be edited into a different key.
            if old_identity.is_id_backed() || queue_head_identity(new_text).is_id_backed() {
                continue;
            }
            let score =
                if new_text.contains(old_text.as_str()) || old_text.contains(new_text.as_str()) {
                    1.0
                } else {
                    similar::TextDiff::from_chars(old_text.as_str(), new_text.as_str()).ratio()
                };
            if score >= 0.5 {
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

    #[test]
    fn a_different_id_or_a_distant_add_is_not_an_edit() {
        let pairing = pair_queue_heads(&heads(&["do [#a]"]), &heads(&["do [#b]"]));
        assert_eq!(
            pairing.new_index_of(0),
            None,
            "a different id is a different item"
        );

        // The claimed line is deleted and a similar line is added elsewhere:
        // two hunks, never one edit.
        let old = heads(&["inline task", "#gh-fix https://x.test/issues/7"]);
        let new = heads(&["#gh-fix https://x.test/issues/8", "inline task"]);
        let pairing = pair_queue_heads(&old, &new);
        assert_eq!(pairing.new_index_of(1), None);
        assert_eq!(pairing.new_index_of(0), Some(1));
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
