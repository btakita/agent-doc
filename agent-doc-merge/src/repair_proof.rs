//! Pure proof that a settled document cut already carries a binary repair.
//!
//! `#preflightrepairdrift`: a repair is the binary-authored delta
//! `base -> repair_target`. The controller rolls a retained repair forward over
//! operator edits typed while it ran, so the settled cut can be
//! `repair_target` plus those edits rather than `repair_target` itself. This
//! module decides that shape without any merge heuristic: a CRDT/component
//! merge falls back to "stale base, keep theirs" on dissimilar texts, which
//! would accept a cut that never received the repair.

use similar::{Algorithm, DiffTag, capture_diff_slices};

/// Insert/delete line edit distance (`deleted + inserted` lines of an optimal
/// Myers script). It is the LCS distance, a true metric on line sequences.
fn line_distance(a: &[&str], b: &[&str]) -> usize {
    capture_diff_slices(Algorithm::Myers, a, b)
        .into_iter()
        .filter(|op| op.tag() != DiffTag::Equal)
        .map(|op| op.old_range().len() + op.new_range().len())
        .sum()
}

/// True when `settled` is `repair_target` plus concurrent edits: the repair is
/// fully embedded on an optimal edit path from `base` to `settled`.
///
/// Line edit distance `d` is a metric, so
/// `d(base, repair_target) + d(repair_target, settled) == d(base, settled)`
/// holds exactly when some optimal script from `base` to `settled` passes
/// through `repair_target` — every line the repair deleted is deleted in
/// `settled` and every line it inserted is present there. This does not depend
/// on how a diff happens to align repeated lines. A cut that lacks the repair
/// (the operator's edit alone, or an unrelated text) is strictly closer to
/// `base` along that path and fails. `settled == repair_target` and
/// `settled == base` are other shapes and return false.
pub fn settled_cut_contains_repair(base: &str, repair_target: &str, settled: &str) -> bool {
    if settled == repair_target || settled == base || repair_target == base {
        return false;
    }
    let base_lines: Vec<&str> = base.split_inclusive('\n').collect();
    let repair_lines: Vec<&str> = repair_target.split_inclusive('\n').collect();
    let settled_lines: Vec<&str> = settled.split_inclusive('\n').collect();
    let repair = line_distance(&base_lines, &repair_lines);
    let concurrent = line_distance(&repair_lines, &settled_lines);
    let total = line_distance(&base_lines, &settled_lines);
    repair > 0 && concurrent > 0 && repair + concurrent == total
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "a\nqueue\nb\nDone.\n\nDone.\nc\n";
    const TARGET: &str = "a\nqueue\nb\nDone.\nc\n";

    #[test]
    fn repair_plus_disjoint_operator_edit_is_contained() {
        let settled = "a\nqueue\nnew item\nb\nDone.\nc\n";
        assert!(settled_cut_contains_repair(BASE, TARGET, settled));
    }

    #[test]
    fn operator_edit_without_the_repair_is_not_contained() {
        let settled = "a\nqueue\nnew item\nb\nDone.\n\nDone.\nc\n";
        assert!(!settled_cut_contains_repair(BASE, TARGET, settled));
    }

    #[test]
    fn dissimilar_cut_is_not_contained() {
        assert!(!settled_cut_contains_repair("before", "target", "stale"));
        assert!(!settled_cut_contains_repair(
            "before",
            "target",
            "operator edit"
        ));
    }

    #[test]
    fn exact_target_and_unrepaired_base_are_other_shapes() {
        assert!(!settled_cut_contains_repair(BASE, TARGET, TARGET));
        assert!(!settled_cut_contains_repair(BASE, TARGET, BASE));
    }

    #[test]
    fn repeated_line_alignment_does_not_matter() {
        // The repair drops one of two identical lines; the operator edits next
        // to the surviving one. Any alignment of the duplicate is accepted.
        let settled = "a\nqueue\nb\nDone.\nc\nmore\n";
        assert!(settled_cut_contains_repair(BASE, TARGET, settled));
    }

    #[test]
    fn operator_edit_that_reintroduces_the_removed_line_is_not_the_repair() {
        // Same bytes as the pre-repair base around the repair: not embedded.
        let settled = "a\nqueue\nx\nb\nDone.\n\nDone.\nc\n";
        assert!(!settled_cut_contains_repair(BASE, TARGET, settled));
    }
}
