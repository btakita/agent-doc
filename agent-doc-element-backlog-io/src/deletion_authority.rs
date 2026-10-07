//! Provenance adapter for operator-deleted backlog rows.
//!
//! The pure guard policy lives in `agent-doc-element-backlog`. This adapter
//! decides whether a visible deletion belongs to the operator: either captured
//! editor operations reproduce the cut exactly, or the cut was already visible
//! while no response cycle was open and committed history still retained every
//! reported row.

use std::collections::HashSet;
use std::path::Path;

use agent_doc_element_backlog::backlog::DroppedBacklogReport;
use agent_doc_element_backlog::guard_policy::DroppedBacklogAuthority;
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorDeletionEvidence {
    CapturedEditorOps,
    IdleVisibleCut,
    Unproven,
}

impl OperatorDeletionEvidence {
    pub const fn authority(self) -> DroppedBacklogAuthority {
        match self {
            Self::CapturedEditorOps | Self::IdleVisibleCut => DroppedBacklogAuthority::Operator,
            Self::Unproven => DroppedBacklogAuthority::Unproven,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CapturedEditorOps => "captured_editor_ops",
            Self::IdleVisibleCut => "idle_visible_cut",
            Self::Unproven => "unproven",
        }
    }
}

/// Classify the authority for the rows in `report`.
///
/// `operator_cut` is the document observed at command entry, before preflight
/// maintenance can mutate it. `committed_content` is the current Git projection.
/// A terminal/no-cycle cut may authorize deletion only when committed history
/// still carries all reported rows; a deletion already landed in Git remains an
/// unproven loss. An exact editor-op replay is stronger evidence and also covers
/// an operator deletion made while a turn is open.
pub fn classify_operator_deletion(
    file: &Path,
    baseline_content: &str,
    operator_cut: &str,
    committed_content: Option<&str>,
    report: &DroppedBacklogReport,
) -> Result<OperatorDeletionEvidence> {
    if report.dropped.is_empty() {
        return Ok(OperatorDeletionEvidence::Unproven);
    }

    let mut bases = vec![baseline_content];
    if let Some(committed) = committed_content
        && committed != baseline_content
    {
        bases.push(committed);
    }
    for base in bases {
        if agent_doc_op_capture_io::operator_text_for_base(file, base)?.as_deref()
            == Some(operator_cut)
        {
            return Ok(OperatorDeletionEvidence::CapturedEditorOps);
        }
    }

    if agent_doc_cycle_state_io::load(file)?.is_some_and(|state| state.phase.is_open()) {
        return Ok(OperatorDeletionEvidence::Unproven);
    }
    let Some(committed) = committed_content else {
        return Ok(OperatorDeletionEvidence::Unproven);
    };
    if committed == operator_cut {
        return Ok(OperatorDeletionEvidence::Unproven);
    }

    let target_ids: HashSet<&str> = report.dropped.iter().map(|item| item.id.as_str()).collect();
    let empty = HashSet::new();
    let operator_report = agent_doc_element_backlog::dropped_from_history_report(
        operator_cut,
        baseline_content,
        &empty,
        &empty,
    )?;
    let operator_dropped: HashSet<&str> = operator_report
        .dropped
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    if !target_ids.is_subset(&operator_dropped) {
        return Ok(OperatorDeletionEvidence::Unproven);
    }

    let committed_report = agent_doc_element_backlog::dropped_from_history_report(
        committed,
        baseline_content,
        &empty,
        &empty,
    )?;
    let committed_dropped: HashSet<&str> = committed_report
        .dropped
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    if target_ids.is_disjoint(&committed_dropped) {
        Ok(OperatorDeletionEvidence::IdleVisibleCut)
    } else {
        Ok(OperatorDeletionEvidence::Unproven)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASELINE: &str = concat!(
        "<!-- agent:backlog -->\n",
        "- [ ] [#keep1] keep\n",
        "- [ ] [#remove1] remove\n",
        "<!-- /agent:backlog -->\n",
    );
    const OPERATOR_CUT: &str = concat!(
        "<!-- agent:backlog -->\n",
        "- [ ] [#keep1] keep\n",
        "<!-- /agent:backlog -->\n",
    );

    fn report() -> DroppedBacklogReport {
        agent_doc_element_backlog::dropped_from_history_report(
            OPERATOR_CUT,
            BASELINE,
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap()
    }

    #[test]
    fn idle_visible_cut_is_operator_authoritative() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let file = dir.path().join("sample.md");
        std::fs::write(&file, OPERATOR_CUT).unwrap();

        assert_eq!(
            classify_operator_deletion(&file, BASELINE, OPERATOR_CUT, Some(BASELINE), &report(),)
                .unwrap(),
            OperatorDeletionEvidence::IdleVisibleCut
        );
    }

    #[test]
    fn deletion_already_absent_from_committed_history_is_unproven() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let file = dir.path().join("sample.md");
        std::fs::write(&file, OPERATOR_CUT).unwrap();

        assert_eq!(
            classify_operator_deletion(
                &file,
                BASELINE,
                OPERATOR_CUT,
                Some(OPERATOR_CUT),
                &report(),
            )
            .unwrap(),
            OperatorDeletionEvidence::Unproven
        );
    }

    #[test]
    fn open_cycle_deletion_requires_exact_editor_op_proof() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let file = dir.path().join("sample.md");
        std::fs::write(&file, OPERATOR_CUT).unwrap();
        agent_doc_cycle_state_io::start_preflight(&file, Some(BASELINE), Some(OPERATOR_CUT))
            .unwrap();

        assert_eq!(
            classify_operator_deletion(&file, BASELINE, OPERATOR_CUT, Some(BASELINE), &report(),)
                .unwrap(),
            OperatorDeletionEvidence::Unproven
        );

        let removed = "- [ ] [#remove1] remove\n";
        let offset = BASELINE.find(removed).unwrap();
        agent_doc_op_capture_io::record_editor_op(
            &file,
            &agent_doc_hash::content_hash(BASELINE),
            agent_doc_merge::crdt::EditorOp::Delete {
                offset,
                len: removed.len(),
            },
        )
        .unwrap();

        assert_eq!(
            classify_operator_deletion(&file, BASELINE, OPERATOR_CUT, Some(BASELINE), &report(),)
                .unwrap(),
            OperatorDeletionEvidence::CapturedEditorOps
        );
    }
}
