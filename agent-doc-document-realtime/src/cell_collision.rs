//! Cell-precise collision detection for concurrent agent writes (`#cellcollide`).
//!
//! A response write owns only the document cells (components) it changes. An
//! operator who keeps typing in a *different* cell while the write is in flight
//! (the queue, the backlog, any other named component) has not collided with
//! it: the editor merges both edits, and the controller canonical carries both.
//!
//! The two decisions here replace whole-document byte equality at the two
//! places that used to report such an unrelated edit as a failure:
//!
//! - [`candidate_drift_confined_to_unowned_cells`]: the live-prompt-drift guard
//!   over the editor's visible-write receipt. Drift confined to cells the agent
//!   did not write is preserved, not reported as
//!   `live_prompt_drift_after_preflight` / `visible_repair_required`.
//! - [`decide_editor_receipt_cells`]: the editor-receipt check against the
//!   controller canonical. The receipt must agree with canonical on every cell
//!   the write owns; other cells may differ (canonical is newer there).
//!
//! Only a genuine same-cell overlap fails closed. The `exchange` cell, the
//! unscoped text outside components, and the component structure are always
//! compared exactly: they are where prompts and directives live, and they keep
//! their existing (stricter) handling.

use std::collections::{BTreeMap, BTreeSet};

use agent_doc_document::transient_markers::{
    strip_boundary_markers, strip_legacy_queue_active_frontmatter,
};

/// Pseudo-cell holding text outside every top-level component.
pub const UNSCOPED_CELL: &str = "@unscoped";
/// Pseudo-cell holding the component order and the marker lines themselves.
pub const STRUCTURE_CELL: &str = "@structure";
const EXCHANGE_CELL: &str = "exchange";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Cell {
    name: String,
    text: String,
}

/// Decompose a document into cells keyed by occurrence label (`queue`,
/// `queue#2`, ...), plus [`UNSCOPED_CELL`] and [`STRUCTURE_CELL`].
fn document_cells(doc: &str) -> Option<BTreeMap<String, Cell>> {
    let parsed = agent_doc_element::element::parse(doc).ok()?;
    let mut cells = BTreeMap::new();
    let mut occurrences = BTreeMap::<&str, usize>::new();
    let mut structure = String::new();
    for component in &parsed {
        let occurrence = occurrences.entry(component.name.as_str()).or_default();
        let label = if *occurrence == 0 {
            component.name.clone()
        } else {
            format!("{}#{}", component.name, *occurrence + 1)
        };
        *occurrence += 1;
        structure.push_str(&label);
        structure.push('\n');
        structure.push_str(&doc[component.open_start..component.open_end]);
        structure.push_str(&doc[component.close_start..component.close_end]);
        cells.insert(
            label,
            Cell {
                name: component.name.clone(),
                text: component.content(doc).to_string(),
            },
        );
    }
    let mut top_level: Vec<&agent_doc_element::element::Component> = Vec::new();
    for component in &parsed {
        if top_level.iter().any(|parent| {
            parent.open_start <= component.open_start && component.close_end <= parent.close_end
        }) {
            continue;
        }
        top_level.push(component);
    }
    let mut unscoped = String::new();
    let mut cursor = 0;
    for component in top_level {
        unscoped.push_str(&doc[cursor..component.open_start]);
        cursor = component.close_end;
    }
    unscoped.push_str(&doc[cursor..]);
    cells.insert(
        UNSCOPED_CELL.to_string(),
        Cell {
            name: UNSCOPED_CELL.to_string(),
            text: unscoped,
        },
    );
    cells.insert(
        STRUCTURE_CELL.to_string(),
        Cell {
            name: STRUCTURE_CELL.to_string(),
            text: structure,
        },
    );
    Some(cells)
}

fn cell_text<'a>(cells: &'a BTreeMap<String, Cell>, label: &str) -> Option<&'a str> {
    cells.get(label).map(|cell| cell.text.as_str())
}

fn cell_name(label: &str) -> &str {
    label.split_once('#').map(|(name, _)| name).unwrap_or(label)
}

/// Cells that must match exactly whenever they differ: prompts, directives and
/// component structure live there.
fn strict_cell(name: &str) -> bool {
    name == EXCHANGE_CELL || name.starts_with('@')
}

/// Component names a transition `from -> to` changes (occurrences collapse to
/// the name, the same ownership rule `#percellconverge` uses).
pub fn owned_cell_names(from: &str, to: &str) -> Option<BTreeSet<String>> {
    let from = document_cells(from)?;
    let to = document_cells(to)?;
    Some(
        from.keys()
            .chain(to.keys())
            .filter(|label| cell_text(&from, label) != cell_text(&to, label))
            .map(|label| cell_name(label).to_string())
            .collect(),
    )
}

/// Whether `current` carries the agent's `base -> target` change for one cell.
/// Exact equality always agrees; a non-strict cell also agrees when `current`
/// contains the agent's delta plus non-overlapping operator edits.
fn owned_cell_agrees(
    name: &str,
    base: Option<&str>,
    target: Option<&str>,
    current: Option<&str>,
) -> bool {
    if target == current {
        return true;
    }
    if strict_cell(name) {
        return false;
    }
    match (base, target, current) {
        (Some(base), Some(target), Some(current)) => {
            agent_doc_merge::captured_splice::current_contains_delta(base, target, current)
        }
        _ => false,
    }
}

fn normalize(doc: &str) -> String {
    strip_legacy_queue_active_frontmatter(&strip_boundary_markers(doc))
}

/// The cells in which an operator drifted, when that drift cannot collide with
/// the agent's write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnownedCellDrift {
    /// Cells the agent did not write and the operator changed.
    pub drifted_cells: Vec<String>,
    /// Cells the agent wrote that also carry a non-overlapping operator edit.
    pub merged_owned_cells: Vec<String>,
}

/// `#cellcollide`: decide whether every difference between the editor's
/// visible-write candidate and the agent's target (`content_ours`) is either
/// confined to a non-strict cell the agent did not write, or a non-overlapping
/// operator edit already merged into a cell the agent wrote.
///
/// Returns `None` (not confined; the caller keeps its existing drift handling)
/// when the documents do not parse, when nothing differs, or when any strict
/// cell (`exchange`, unscoped text, structure) differs between the candidate
/// and `content_ours`.
pub fn candidate_drift_confined_to_unowned_cells(
    baseline: &str,
    candidate: &str,
    content_ours: &str,
) -> Option<UnownedCellDrift> {
    let base = document_cells(&normalize(baseline))?;
    let cand = document_cells(&normalize(candidate))?;
    let ours = document_cells(&normalize(content_ours))?;
    let owned: BTreeSet<&str> = base
        .keys()
        .chain(ours.keys())
        .filter(|label| cell_text(&base, label) != cell_text(&ours, label))
        .map(|label| cell_name(label))
        .collect();
    let mut drift = UnownedCellDrift {
        drifted_cells: Vec::new(),
        merged_owned_cells: Vec::new(),
    };
    let labels: BTreeSet<&String> = cand.keys().chain(ours.keys()).collect();
    for label in labels {
        let name = cell_name(label);
        let (cand_c, ours_c) = (cell_text(&cand, label), cell_text(&ours, label));
        if cand_c == ours_c {
            continue;
        }
        if strict_cell(name) {
            return None;
        }
        if !owned.contains(name) {
            drift.drifted_cells.push(label.clone());
        } else if owned_cell_agrees(name, cell_text(&base, label), ours_c, cand_c) {
            drift.merged_owned_cells.push(label.clone());
        } else {
            return None;
        }
    }
    if drift.drifted_cells.is_empty() && drift.merged_owned_cells.is_empty() {
        return None;
    }
    Some(drift)
}

/// Verdict for an editor receipt compared against the controller canonical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorReceiptCellVerdict {
    /// Byte-identical.
    Exact,
    /// Every owned cell agrees; canonical differs only in cells the write did
    /// not own (operator edits newer than the receipt). Canonical is adopted.
    OwnedCellsConverged { drifted_cells: Vec<String> },
    /// An owned cell in canonical still equals the pre-write cut: the editor's
    /// delta has not reached canonical yet. Not a divergence; keep waiting.
    OwnedCellsPending { pending_cells: Vec<String> },
    /// An owned cell in canonical matches neither the receipt nor the pre-write
    /// cut (a genuine same-cell overlap), or the cells cannot be compared.
    Diverged { cells: Vec<String> },
}

/// `#cellcollide`: compare an editor receipt to the controller canonical cell
/// by cell, judging only the cells the write owns.
///
/// An empty `owned` set falls back to exact equality: with no proof of what the
/// write touched, nothing can be called unrelated.
pub fn decide_editor_receipt_cells(
    pre_write: Option<&str>,
    receipt: &str,
    canonical: &str,
    owned: &BTreeSet<String>,
) -> EditorReceiptCellVerdict {
    if receipt == canonical {
        return EditorReceiptCellVerdict::Exact;
    }
    let (Some(rec), Some(can)) = (document_cells(receipt), document_cells(canonical)) else {
        return EditorReceiptCellVerdict::Diverged {
            cells: vec!["@parse".to_string()],
        };
    };
    if owned.is_empty() {
        return EditorReceiptCellVerdict::Diverged {
            cells: vec!["@unowned_write".to_string()],
        };
    }
    let pre = pre_write.and_then(document_cells);
    let mut drifted = Vec::new();
    let mut pending = Vec::new();
    let mut diverged = Vec::new();
    let labels: BTreeSet<&String> = rec.keys().chain(can.keys()).collect();
    for label in labels {
        let name = cell_name(label);
        let (rec_c, can_c) = (cell_text(&rec, label), cell_text(&can, label));
        if rec_c == can_c {
            continue;
        }
        let pre_c = pre.as_ref().and_then(|pre| cell_text(pre, label));
        let owned_cell = owned.contains(name) || name == STRUCTURE_CELL;
        if !owned_cell || owned_cell_agrees(name, pre_c, rec_c, can_c) {
            drifted.push(label.clone());
        } else if pre.is_some() && pre_c == can_c {
            pending.push(label.clone());
        } else {
            diverged.push(label.clone());
        }
    }
    if !diverged.is_empty() {
        EditorReceiptCellVerdict::Diverged { cells: diverged }
    } else if !pending.is_empty() {
        EditorReceiptCellVerdict::OwnedCellsPending {
            pending_cells: pending,
        }
    } else {
        EditorReceiptCellVerdict::OwnedCellsConverged {
            drifted_cells: drifted,
        }
    }
}

/// `#appliedresponsefold`: the canonical text that carries an editor receipt's
/// owned cells which never reached canonical, keeping canonical's newer
/// operator edits in every other cell.
///
/// Live 2026-10-09 (contracts.md): the editor applied an agent response through
/// a component patch and acknowledged the full content, but its replica never
/// published that programmatic edit, so canonical's `exchange` cell stayed at
/// the pre-write cut while the operator kept typing in the queue. The receipt
/// verdict was [`EditorReceiptCellVerdict::OwnedCellsPending`] until the budget
/// ran out, the receipt was refused as divergent, and the captured response
/// was stranded in the editor while canonical (and every later replay) lacked
/// it.
///
/// Answers `Some(folded)` only when the verdict is exactly `OwnedCellsPending`
/// (no owned cell diverged), every pending cell is a named component present
/// exactly once in both texts, and the folded text then agrees with the
/// receipt on every owned cell. Anything else answers `None` and the caller
/// keeps failing closed.
pub fn fold_pending_owned_cells_from_receipt(
    pre_write: Option<&str>,
    receipt: &str,
    canonical: &str,
    owned: &BTreeSet<String>,
) -> Option<String> {
    let EditorReceiptCellVerdict::OwnedCellsPending { pending_cells } =
        decide_editor_receipt_cells(pre_write, receipt, canonical, owned)
    else {
        return None;
    };
    let receipt_components = agent_doc_element::element::parse(receipt).ok()?;
    let mut folded = canonical.to_string();
    for label in &pending_cells {
        // Occurrence labels (`queue#2`) and pseudo-cells are not folded.
        if label.contains('#') || label.starts_with('@') {
            return None;
        }
        let mut in_receipt = receipt_components.iter().filter(|c| &c.name == label);
        let receipt_cell = in_receipt.next()?;
        if in_receipt.next().is_some() {
            return None;
        }
        let canonical_components = agent_doc_element::element::parse(&folded).ok()?;
        let mut in_canonical = canonical_components.iter().filter(|c| &c.name == label);
        let canonical_cell = in_canonical.next()?;
        if in_canonical.next().is_some() {
            return None;
        }
        folded = canonical_cell.replace_content(&folded, receipt_cell.content(receipt));
    }
    match decide_editor_receipt_cells(pre_write, receipt, &folded, owned) {
        EditorReceiptCellVerdict::Exact | EditorReceiptCellVerdict::OwnedCellsConverged { .. } => {
            Some(folded)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(exchange: &str, queue: &str, backlog: &str) -> String {
        format!(
            "---\nagent_doc_session: t\n---\n\n<!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n{backlog}<!-- /agent:backlog -->\n"
        )
    }

    const PROMPT: &str = "❯ please reply\n";
    const REPLY: &str = "❯ please reply\n### Re: please reply\n\nAnswered.\n";

    fn owned(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn queue_only_operator_drift_is_confined_to_an_unowned_cell() {
        let base = doc(PROMPT, "- do #a\n", "- [ ] [#a] a\n");
        let ours = doc(REPLY, "- do #a\n", "- [ ] [#a] a\n");
        let cand = doc(REPLY, "- do #a typed more\n", "- [ ] [#a] a\n");
        let drift = candidate_drift_confined_to_unowned_cells(&base, &cand, &ours)
            .expect("queue-only drift must not collide with an exchange write");
        assert_eq!(drift.drifted_cells, vec!["queue".to_string()]);
        assert!(drift.merged_owned_cells.is_empty());
    }

    #[test]
    fn backlog_only_operator_drift_is_confined_to_an_unowned_cell() {
        let base = doc(PROMPT, "", "- [ ] [#a] a\n");
        let ours = doc(REPLY, "", "- [ ] [#a] a\n");
        let cand = doc(REPLY, "", "- [ ] [#a] a\n- [ ] [#b] new\n");
        let drift = candidate_drift_confined_to_unowned_cells(&base, &cand, &ours).unwrap();
        assert_eq!(drift.drifted_cells, vec!["backlog".to_string()]);
    }

    #[test]
    fn exchange_drift_at_the_response_spot_is_not_confined() {
        let base = doc(PROMPT, "", "");
        let ours = doc(REPLY, "", "");
        let cand = doc(&format!("{REPLY}❯ new prompt typed mid-write\n"), "", "");
        assert_eq!(
            candidate_drift_confined_to_unowned_cells(&base, &cand, &ours),
            None
        );
    }

    #[test]
    fn missing_response_in_candidate_is_not_confined() {
        let base = doc(PROMPT, "- do #a\n", "");
        let ours = doc(REPLY, "- do #a\n", "");
        let cand = doc(PROMPT, "- do #a more\n", "");
        assert_eq!(
            candidate_drift_confined_to_unowned_cells(&base, &cand, &ours),
            None
        );
    }

    #[test]
    fn owned_queue_strike_merges_with_disjoint_operator_queue_edit() {
        let base = doc(PROMPT, "- do #alpha\n- do #beta\n- do #gamma\n", "");
        let ours = doc(REPLY, "- do #beta\n- do #gamma\n", "");
        let cand = doc(REPLY, "- do #beta\n- do #gamma, then ship it\n", "");
        let drift = candidate_drift_confined_to_unowned_cells(&base, &cand, &ours).unwrap();
        assert_eq!(drift.merged_owned_cells, vec!["queue".to_string()]);
    }

    #[test]
    fn owned_queue_strike_conflicting_with_operator_edit_of_the_same_head_is_not_confined() {
        let base = doc(PROMPT, "- do #a\n- do #b\n", "");
        let ours = doc(REPLY, "- do #b\n", "");
        // The editor kept the struck head because the operator was rewriting it.
        let cand = doc(REPLY, "- do #a rewritten\n- do #b\n", "");
        assert_eq!(
            candidate_drift_confined_to_unowned_cells(&base, &cand, &ours),
            None
        );
    }

    #[test]
    fn unscoped_drift_keeps_strict_handling() {
        let base = doc(PROMPT, "", "");
        let ours = doc(REPLY, "", "");
        let cand = format!("{ours}<!--\ndispatch #x\n-->\n");
        assert_eq!(
            candidate_drift_confined_to_unowned_cells(&base, &cand, &ours),
            None
        );
    }

    #[test]
    fn receipt_with_newer_operator_queue_in_canonical_converges() {
        let pre = doc(PROMPT, "- do #a\n", "");
        let receipt = doc(REPLY, "- do #a typ\n", "");
        let canonical = doc(REPLY, "- do #a typed more\n", "");
        assert_eq!(
            decide_editor_receipt_cells(Some(&pre), &receipt, &canonical, &owned(&["exchange"])),
            EditorReceiptCellVerdict::OwnedCellsConverged {
                drifted_cells: vec!["queue".to_string()]
            }
        );
    }

    #[test]
    fn receipt_ahead_of_canonical_in_owned_cell_is_pending_not_divergent() {
        let pre = doc(PROMPT, "", "");
        let receipt = doc(REPLY, "", "");
        assert_eq!(
            decide_editor_receipt_cells(Some(&pre), &receipt, &pre, &owned(&["exchange"])),
            EditorReceiptCellVerdict::OwnedCellsPending {
                pending_cells: vec!["exchange".to_string()]
            }
        );
    }

    #[test]
    fn receipt_conflicting_with_canonical_in_owned_cell_diverges() {
        let pre = doc(PROMPT, "", "");
        let receipt = doc(REPLY, "", "");
        let canonical = doc(&format!("{PROMPT}❯ operator typed here\n"), "", "");
        assert_eq!(
            decide_editor_receipt_cells(Some(&pre), &receipt, &canonical, &owned(&["exchange"])),
            EditorReceiptCellVerdict::Diverged {
                cells: vec!["exchange".to_string()]
            }
        );
    }

    #[test]
    fn receipt_without_owned_cells_falls_back_to_exact_equality() {
        let pre = doc(PROMPT, "", "");
        let receipt = doc(REPLY, "x\n", "");
        let canonical = doc(REPLY, "y\n", "");
        assert!(matches!(
            decide_editor_receipt_cells(Some(&pre), &receipt, &canonical, &BTreeSet::new()),
            EditorReceiptCellVerdict::Diverged { .. }
        ));
    }

    #[test]
    fn owned_cell_names_reports_only_written_components() {
        let base = doc(PROMPT, "- do #a\n", "");
        let ours = doc(REPLY, "- do #a\n", "");
        assert_eq!(owned_cell_names(&base, &ours), Some(owned(&["exchange"])));
    }

    /// `#appliedresponsefold`: the editor applied the response, canonical never
    /// got it, and the operator typed in the queue meanwhile. The fold carries
    /// the response into canonical and keeps the operator's queue text.
    #[test]
    fn pending_owned_response_folds_into_canonical_keeping_operator_cells() {
        let pre = doc(PROMPT, "- do #a\n", "- [ ] [#a] a\n");
        let receipt = doc(REPLY, "- do #a\n", "- [ ] [#a] a\n");
        let canonical = doc(PROMPT, "- do #a\n- #b typed later\n", "- [ ] [#a] a\n");
        let folded = fold_pending_owned_cells_from_receipt(
            Some(&pre),
            &receipt,
            &canonical,
            &owned(&["exchange"]),
        )
        .expect("a pending owned cell must fold");
        assert_eq!(
            folded,
            doc(REPLY, "- do #a\n- #b typed later\n", "- [ ] [#a] a\n")
        );
    }

    /// An operator edit in the owned cell itself is a real overlap: no fold.
    #[test]
    fn diverged_owned_cell_never_folds() {
        let pre = doc(PROMPT, "", "");
        let receipt = doc(REPLY, "", "");
        let canonical = doc(&format!("{PROMPT}❯ operator typed here\n"), "", "");
        assert_eq!(
            fold_pending_owned_cells_from_receipt(
                Some(&pre),
                &receipt,
                &canonical,
                &owned(&["exchange"])
            ),
            None
        );
    }

    /// Without a pre-write cut nothing can be called pending: no fold.
    #[test]
    fn fold_requires_a_pre_write_cut() {
        let receipt = doc(REPLY, "", "");
        let canonical = doc(PROMPT, "", "");
        assert_eq!(
            fold_pending_owned_cells_from_receipt(
                None,
                &receipt,
                &canonical,
                &owned(&["exchange"])
            ),
            None
        );
    }

    /// Already converged: nothing to fold.
    #[test]
    fn converged_receipt_does_not_fold() {
        let pre = doc(PROMPT, "", "");
        let receipt = doc(REPLY, "", "");
        assert_eq!(
            fold_pending_owned_cells_from_receipt(
                Some(&pre),
                &receipt,
                &receipt,
                &owned(&["exchange"])
            ),
            None
        );
    }
}
