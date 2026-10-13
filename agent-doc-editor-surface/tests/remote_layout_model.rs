//! Executable refinement of `formal/tla/RemoteLayoutProvisional.tla` (GH #250).
//!
//! TLC checks the abstract transition system. This test exhaustively feeds the
//! corresponding finite state space through the production
//! [`RemoteLayoutMemory::advance`] fold: every ordered two-column base over four
//! documents, either focused slot, every outside selection, an optional second
//! selection while Coder's visible membership still lags, and every corrective
//! visible set containing the selected document.

use agent_doc_editor_surface::remote_layout::{
    RemoteClientEditors, RemoteLayoutEvidence, RemoteLayoutMemory,
};

const DOCS: [&str; 4] = ["a.md", "b.md", "c.md", "d.md"];
const TLA_SOURCE: &str = include_str!("../../formal/tla/RemoteLayoutProvisional.tla");

fn evidence(visible: &[&str], selected: &str) -> RemoteLayoutEvidence {
    RemoteLayoutEvidence {
        clients: vec![RemoteClientEditors {
            visible: visible.iter().map(|file| (*file).to_string()).collect(),
            selected: vec![selected.to_string()],
            open: DOCS.iter().map(|file| (*file).to_string()).collect(),
        }],
        focused: vec![selected.to_string()],
    }
}

fn columns(memory: &RemoteLayoutMemory) -> Vec<String> {
    memory
        .columns
        .iter()
        .map(|column| {
            assert_eq!(column.files.len(), 1, "model uses one file per column");
            column.files[0].clone()
        })
        .collect()
}

/// Reference `Ordered(base, visible)` from the TLA+ module, specialized to
/// same-width two-column layouts.
fn ordered(base: &[&str; 2], visible: &[&str; 2]) -> Vec<String> {
    let survivors = base.iter().filter(|file| visible.contains(file)).count();
    if survivors == 0 {
        return visible.iter().map(|file| (*file).to_string()).collect();
    }
    let added = visible.iter().find(|file| !base.contains(file)).copied();
    base.iter()
        .map(|file| {
            if visible.contains(file) {
                (*file).to_string()
            } else {
                added
                    .expect("one dropped slot has one added document")
                    .to_string()
            }
        })
        .collect()
}

#[test]
fn tla_contract_names_the_production_ancestry_invariants() {
    for contract in [
        "SelectionFirst ==",
        "SelectionAgain ==",
        "VisibleCorrection ==",
        "CorrectionUsesPreGuessBase ==",
        "ProvisionalPreservesAncestry ==",
        "ProvisionalClearsAfterCorrection ==",
    ] {
        assert!(
            TLA_SOURCE.contains(contract),
            "formal transition contract drifted: missing {contract}"
        );
    }
    assert!(TLA_SOURCE.contains("RemoteLayoutMemory.provisional_base_columns"));
}

#[test]
fn production_fold_refines_the_exhaustive_provisional_layout_model() {
    let mut checked_corrections = 0usize;
    let mut checked_repeated_selection_corrections = 0usize;

    for &left in &DOCS {
        for &right in &DOCS {
            if left == right {
                continue;
            }
            let base = [left, right];
            for focus_index in 0..2 {
                let focused = base[focus_index];
                for &first_selection in DOCS.iter().filter(|file| !base.contains(file)) {
                    let (stable, initial) =
                        RemoteLayoutMemory::default().advance(&evidence(&base, focused));
                    assert_eq!(
                        initial
                            .columns
                            .iter()
                            .map(|column| column.files[0].clone())
                            .collect::<Vec<_>>(),
                        base.iter()
                            .map(|file| (*file).to_string())
                            .collect::<Vec<_>>()
                    );

                    let (guessed, guess) = stable.advance(&evidence(&base, first_selection));
                    assert_eq!(
                        guess.reason.as_deref(),
                        Some("selected_visible_change_replaced_focus_column")
                    );
                    assert_eq!(
                        guessed.provisional_base_columns.as_ref(),
                        Some(&stable.columns),
                        "the first selection-only guess must retain its ancestry"
                    );

                    let mut guess_states = vec![(guessed.clone(), first_selection, false)];
                    for &second_selection in DOCS
                        .iter()
                        .filter(|file| !base.contains(file) && **file != first_selection)
                    {
                        let (repeated, repeat) =
                            guessed.advance(&evidence(&base, second_selection));
                        assert_eq!(
                            repeat.reason.as_deref(),
                            Some("selected_visible_change_replaced_focus_column")
                        );
                        assert_eq!(
                            repeated.provisional_base_columns.as_ref(),
                            Some(&stable.columns),
                            "a repeated lagging selection must not replace the original ancestry"
                        );
                        guess_states.push((repeated, second_selection, true));
                    }

                    for (memory, selected, repeated) in guess_states {
                        for &other in DOCS.iter().filter(|file| **file != selected) {
                            for visible in [[selected, other], [other, selected]] {
                                let (corrected, resolution) =
                                    memory.advance(&evidence(&visible, selected));
                                assert_eq!(
                                    columns(&corrected),
                                    ordered(&base, &visible),
                                    "base={base:?} focus={focused} selected={selected} visible={visible:?} resolution={resolution:?}"
                                );
                                assert!(
                                    corrected.provisional_base_columns.is_none(),
                                    "structural correction must consume provisional ancestry"
                                );
                                checked_corrections += 1;
                                checked_repeated_selection_corrections += usize::from(repeated);
                            }
                        }
                    }
                }
            }
        }
    }

    assert_eq!(checked_corrections, 576);
    assert_eq!(checked_repeated_selection_corrections, 288);
}
