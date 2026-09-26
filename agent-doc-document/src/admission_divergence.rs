//! One source of truth for "will the next `agent-doc <FILE>` admission refuse?".
//!
//! Preflight refuses to open a cycle when the recorded baseline, the registered
//! live editor authority, and the durable disk file have ALL advanced to three
//! distinct revisions: there is no unchanged branch to fast-forward from, so
//! choosing a winner would silently drop one writer's text.
//!
//! `session-check` must consult the SAME classification before it prescribes a
//! remedy. It used to print "run `agent-doc <FILE>` to continue" unconditionally,
//! which in this state is a deadlock: the agent runs the trigger, the
//! `UserPromptSubmit` hook's preflight refuses it, session-check prints the same
//! advice again, and nothing names the operator-side action that actually clears
//! it. Keeping the predicate here — rather than re-deriving it on each side —
//! is what stops the two halves from drifting apart again.

/// How the three revision planes relate for admission purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionDivergence {
    /// At least one plane still matches another, so a bounded fast-forward or an
    /// ordinary diff is available. Admission may proceed.
    Resolvable,
    /// Baseline, authority, and disk are three distinct revisions. Admission
    /// refuses, and only convergence on the editor side clears it.
    UnmergeableThreeWaySplit,
}

impl AdmissionDivergence {
    /// `true` when the next admission will refuse on this observation.
    pub fn refuses_admission(self) -> bool {
        self == Self::UnmergeableThreeWaySplit
    }
}

/// Classify one coherent observation of the three planes.
///
/// `authority` is the registered live editor cut; pass `None` when no live
/// editor is registered, in which case there is no second writer to split
/// against and admission is never refused on this ground.
pub fn classify(baseline: Option<&str>, authority: Option<&str>, disk: &str) -> AdmissionDivergence {
    let (Some(baseline), Some(authority)) = (baseline, authority) else {
        return AdmissionDivergence::Resolvable;
    };
    if disk == authority || disk == baseline || authority == baseline {
        return AdmissionDivergence::Resolvable;
    }
    AdmissionDivergence::UnmergeableThreeWaySplit
}

/// The operator-side remedy for a refused admission.
///
/// Stated as the action that actually converges the planes. Re-invoking the
/// trigger is explicitly ruled out so the caller cannot hand back advice the
/// admission gate will refuse again.
pub fn unmergeable_split_remedy(file_display: &str) -> String {
    format!(
        "This document's recorded baseline, live editor buffer, and file on disk are three different \
revisions, so admission refuses rather than choose a winner and drop a writer's text. Re-invoking \
`agent-doc {file_display}` will keep refusing until the planes converge — do NOT retry it, and do NOT \
use `--force-disk` or hand-align disk to the authority (either one discards the other writer). \
Operator-side: save or close this document's editor tab so the live buffer and disk converge, and \
close any other agent-doc session still open on this same document. Admission then proceeds normally \
on the next invocation."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_advanced_branch_stays_resolvable() {
        // Only disk moved: preflight fast-forwards the proven durable save.
        assert_eq!(
            classify(Some("base"), Some("base"), "disk"),
            AdmissionDivergence::Resolvable
        );
        // Only the editor moved: an ordinary diff against the baseline.
        assert_eq!(
            classify(Some("base"), Some("live"), "base"),
            AdmissionDivergence::Resolvable
        );
        // Both moved to the SAME revision: already converged.
        assert_eq!(
            classify(Some("base"), Some("same"), "same"),
            AdmissionDivergence::Resolvable
        );
    }

    #[test]
    fn three_distinct_revisions_refuse_admission() {
        let divergence = classify(Some("base"), Some("live"), "disk");
        assert_eq!(divergence, AdmissionDivergence::UnmergeableThreeWaySplit);
        assert!(divergence.refuses_admission());
    }

    #[test]
    fn no_live_editor_is_never_an_unmergeable_split() {
        assert_eq!(
            classify(Some("base"), None, "disk"),
            AdmissionDivergence::Resolvable
        );
        assert_eq!(
            classify(None, Some("live"), "disk"),
            AdmissionDivergence::Resolvable
        );
    }

    /// The remedy must not prescribe the trigger the admission gate refuses, and
    /// must not prescribe the two recoveries that destroy a writer's text.
    #[test]
    fn the_remedy_never_prescribes_a_refused_or_destructive_action() {
        let remedy = unmergeable_split_remedy("tasks/api.md");
        assert!(remedy.contains("save or close this document's editor tab"));
        assert!(remedy.contains("will keep refusing"));
        assert!(remedy.contains("do NOT retry"));
        assert!(remedy.contains("--force-disk"));
        assert!(
            !remedy.contains("to continue"),
            "the deadlocking `run ... to continue` phrasing must not reappear: {remedy}"
        );
    }
}
