//! One source of truth for "how does the next `agent-doc <FILE>` admission
//! reconcile the three revision planes?".
//!
//! Preflight observes three planes — the recorded baseline, the registered live
//! editor authority, and the durable disk file — and picks the action that
//! reconciles them. Every classification is resolvable; none of them stalls.
//!
//! `session-check` must consult the SAME classification so the two halves cannot
//! drift apart.
//!
//! # Why a genuine split merges instead of refusing (`#admissionsplitmerge`)
//!
//! This module used to **refuse admission** when all three planes had durably
//! advanced and the authority did not already contain disk, on the reasoning that
//! "choosing a winner would silently drop one writer's text". That reasoning is
//! sound and its conclusion was wrong: the alternative to choosing a winner is not
//! refusing, it is **merging**, which drops nobody. agent-doc already owns a pure
//! node-identity three-way merge for exactly this input shape
//! (`agent_doc_merge::document_cell_merge`, `base`/`ours`/`theirs`), and the
//! baseline is by definition the common ancestor the merge needs.
//!
//! Refusing was also unachievable as a *stable* state. A realtime document has two
//! writers by design — the operator types in a registered editor while agent-doc
//! projects to disk — so two planes advancing independently is the normal condition,
//! not a fault to be prevented. The prescribed remedy ("save or close the editor
//! tab") asked a human to hand-converge planes the binary is built to merge, and it
//! stalled the queue until they did. Observed twice on 2026-09-27 within seven
//! hours (`tasks/agent-doc/agent-doc-bugs.md`, `tasks/software/lazily.md`) with
//! durable deltas of 10 and 17 bytes: in the first, disk carried the operator's
//! queue edits and the editor buffer carried a different revision of the same three
//! lines. Nothing about that is unmergeable.
//!
//! So the split classifies as `ThreeWaySplitMerged` and selects
//! `MergeDiskIntoAuthority`. The IO shell performs the merge and adopts the result
//! through the same compare-and-swap editor write that already adopts a proven
//! newer durable save — the merge cannot live here because `agent-doc-merge`
//! depends on this crate.
//!
//! # Why the comparison is normalized (`#admissionmarkersplit`)
//!
//! The planes are only comparable in the **durable** domain. Transient agent-doc
//! markers — `<!-- agent:boundary:… -->` lines, the ` (HEAD)` heading suffix, the
//! `❯ 🚧` active-prompt marker, per-cycle guard comments, and the managed
//! pipeline frontmatter block — live in the editor buffer but are stripped from
//! the durable baseline and the committed file *by design*. Comparing raw bytes
//! therefore let agent-doc's OWN transport state manufacture the third revision:
//! observed live twice on `tasks/agent-doc/agent-doc-bugs.md` (baseline
//! checkpointed at 31112 bytes against a 31220-byte authority — a 108-byte
//! marker delta) with no concurrent writer at all.
//!
//! That false refusal is **permanent**, which is what makes it worse than a
//! stall: the prescribed remedy is "save or close the editor tab", and a save
//! cannot converge the planes because agent-doc re-writes the markers into the
//! live buffer and strips them again on the durable path. Every other durable
//! comparison in the codebase already normalizes first — see
//! `agent_doc_capture_io::authoritative_current_monotonically_extends_capture_baseline`
//! ("Transient agent-doc markers are normalized before comparison so
//! transport-only state does not manufacture a conflict") and
//! `agent_doc_git_io::transient_cleanup`. This module was the outlier.
//!
//! # Why subsumption is resolvable (`#admissionancestorsplit`)
//!
//! "No unchanged branch" is not the same as "a writer would be dropped". When a
//! commit is blocked (`NativeSaveRequired`, `disk_projection_ready=false`) the
//! baseline is still checkpointed from the live snapshot while disk keeps the
//! older revision, and the operator keeps typing. All three planes then differ,
//! yet the live authority *contains every durable line already on disk* — disk is
//! an ancestor, not a rival. Admission can proceed on the authority without
//! losing anything, and the next commit converges disk. The same monotonic
//! proof already gates replay-baseline rebasing onto concurrent operator
//! steering (`agent_doc_workflow::capture::current_monotonically_extends_baseline`).

use crate::transient_markers::normalize_transient_agent_doc_markers;

/// How the three revision planes relate for admission purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionDivergence {
    /// At least one plane still matches another verbatim, so a bounded
    /// fast-forward or an ordinary diff is available. Admission may proceed.
    Resolvable,
    /// The planes differ only by transient agent-doc markers: in the durable
    /// domain a branch is still unchanged. Admission may proceed.
    TransientMarkersOnly,
    /// Three durably distinct revisions, but the live authority preserves every
    /// durable line already on disk. Disk is an ancestor, so admission may
    /// proceed on the authority and the next commit converges disk.
    AuthoritySubsumesDisk,
    /// Baseline, authority, and disk are three durably distinct revisions with
    /// content on disk the authority does not carry: two writers genuinely
    /// diverged. Admission reconciles them with a three-way merge against the
    /// baseline ancestor, so neither writer is dropped and nothing stalls.
    ThreeWaySplitMerged,
}

impl AdmissionDivergence {
    /// `true` when reconciling this observation needs a three-way merge rather
    /// than a fast-forward or a plain diff.
    ///
    /// No classification refuses admission: see `#admissionsplitmerge`.
    pub fn requires_disk_merge(self) -> bool {
        self == Self::ThreeWaySplitMerged
    }

    /// Stable log token naming WHY the observation classified this way.
    ///
    /// The merged split used to be indistinguishable in `ops.log` from the two
    /// benign shapes above — every case printed the same three raw hashes — so
    /// diagnosing one meant reconstructing byte lengths out of surrounding
    /// `commit_staging` / `document_baseline_checkpoint` lines. The token plus
    /// the normalized hashes make a single `grep admission_divergence` decide it.
    pub fn reason_token(self) -> &'static str {
        match self {
            Self::Resolvable => "verbatim_branch_unchanged",
            Self::TransientMarkersOnly => "transient_markers_only",
            Self::AuthoritySubsumesDisk => "authority_subsumes_disk",
            Self::ThreeWaySplitMerged => "three_way_split_merged",
        }
    }
}

/// What preflight must do with the durable disk revision for this observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskAdoption {
    /// Nothing durable on disk is missing from the live authority: proceed.
    ProceedOnAuthority,
    /// The durable authority branch is unchanged and disk durably advanced:
    /// fast-forward the live authority onto the proven newer durable save.
    FastForwardAuthorityToDisk,
    /// Both branches durably advanced: three-way merge disk into the live
    /// authority against the baseline ancestor and admit on the merged revision.
    MergeDiskIntoAuthority,
}

impl DiskAdoption {
    /// Stable log token for the action this observation selects.
    pub fn action_token(self) -> &'static str {
        match self {
            Self::ProceedOnAuthority => "proceed_on_authority",
            Self::FastForwardAuthorityToDisk => "fast_forward_authority_to_disk",
            Self::MergeDiskIntoAuthority => "merge_disk_into_authority",
        }
    }
}

/// One coherent observation of the three planes, classified and decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionAssessment {
    pub divergence: AdmissionDivergence,
    pub adoption: DiskAdoption,
}

impl AdmissionAssessment {
    /// The diagnostic field pair for an `ops.log` line, rendered HERE rather than
    /// at each IO site.
    ///
    /// The FlowCore hot-path gate exists to stop `reason=`/`proof=` tokens from
    /// accumulating as ad-hoc strings in IO shells instead of being owned by the
    /// enum that decides them. This is that ownership: the classifier names its
    /// own fields, and preflight/`session-check` interpolate the pair.
    pub fn diagnostic_fields(self) -> String {
        format!(
            "reason={} action={}",
            self.divergence.reason_token(),
            self.adoption.action_token()
        )
    }
}

/// Classify and decide one coherent observation of the three planes.
///
/// `authority` is the registered live editor cut; pass `None` when no live
/// editor is registered, in which case there is no second writer to split
/// against and admission is never refused on this ground.
///
/// The verbatim tests run first and keep their exact pre-normalization
/// behaviour, so this is a strict widening: an observation that resolved before
/// resolves the same way now, and only observations that previously refused can
/// change verdict — they now merge (`#admissionsplitmerge`).
pub fn assess(baseline: Option<&str>, authority: Option<&str>, disk: &str) -> AdmissionAssessment {
    let (Some(baseline), Some(authority)) = (baseline, authority) else {
        return AdmissionAssessment {
            divergence: AdmissionDivergence::Resolvable,
            adoption: DiskAdoption::ProceedOnAuthority,
        };
    };
    if disk == authority || disk == baseline {
        return AdmissionAssessment {
            divergence: AdmissionDivergence::Resolvable,
            adoption: DiskAdoption::ProceedOnAuthority,
        };
    }
    if authority == baseline {
        return AdmissionAssessment {
            divergence: AdmissionDivergence::Resolvable,
            adoption: DiskAdoption::FastForwardAuthorityToDisk,
        };
    }
    let durable_baseline = normalize_transient_agent_doc_markers(baseline);
    let durable_authority = normalize_transient_agent_doc_markers(authority);
    let durable_disk = normalize_transient_agent_doc_markers(disk);
    if durable_disk == durable_authority || durable_disk == durable_baseline {
        return AdmissionAssessment {
            divergence: AdmissionDivergence::TransientMarkersOnly,
            adoption: DiskAdoption::ProceedOnAuthority,
        };
    }
    if durable_authority == durable_baseline {
        return AdmissionAssessment {
            divergence: AdmissionDivergence::TransientMarkersOnly,
            adoption: DiskAdoption::FastForwardAuthorityToDisk,
        };
    }
    if authority_subsumes_disk(&durable_disk, &durable_authority) {
        return AdmissionAssessment {
            divergence: AdmissionDivergence::AuthoritySubsumesDisk,
            adoption: DiskAdoption::ProceedOnAuthority,
        };
    }
    AdmissionAssessment {
        divergence: AdmissionDivergence::ThreeWaySplitMerged,
        adoption: DiskAdoption::MergeDiskIntoAuthority,
    }
}

/// Classify one coherent observation of the three planes.
pub fn classify(
    baseline: Option<&str>,
    authority: Option<&str>,
    disk: &str,
) -> AdmissionDivergence {
    assess(baseline, authority, disk).divergence
}

/// True when the live authority preserves every durable disk line in order, so
/// disk carries nothing the authority would drop.
///
/// This is the same rule as
/// `agent_doc_workflow::capture::current_monotonically_extends_baseline`, which
/// cannot be called from here: `agent-doc-workflow` depends on `agent-doc-turn`,
/// which depends on this crate, so importing it would close a dependency cycle.
/// `admission_subsumption_matches_the_capture_monotonic_rule` in
/// `agent-doc-capture-io` — which depends on both crates — pins the two
/// implementations to the same verdicts, so this copy cannot drift.
pub fn authority_subsumes_disk(durable_disk: &str, durable_authority: &str) -> bool {
    if durable_disk == durable_authority {
        return true;
    }
    let mut authority_lines = durable_authority.lines();
    durable_disk
        .lines()
        .all(|disk_line| authority_lines.by_ref().any(|line| line == disk_line))
}

/// What a three-way split means for the caller now that admission merges it.
///
/// There is no operator action to prescribe, and no action for the agent either:
/// the merge happens inside admission. This text exists so a diagnostic that
/// observes the split reports it as reconciled rather than as a fault, and so it
/// still rules out the two recoveries that destroy a writer's text — an agent
/// that reaches for `--force-disk` on seeing "three planes" is the failure mode
/// this wording has always been guarding against.
pub fn three_way_merge_notice(file_display: &str) -> String {
    format!(
        "This document's recorded baseline, live editor buffer, and file on disk are three different \
revisions whose durable content differs (not just transient agent-doc markers), so two writers \
genuinely diverged. Admission reconciles them with a three-way merge against the baseline ancestor \
and proceeds on the merged revision — no writer is dropped and there is nothing to converge by hand. \
Run `agent-doc {file_display}` normally. Do NOT use `--force-disk` and do NOT hand-align disk to the \
authority: either one discards the other writer's text that the merge preserves."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_advanced_branch_stays_resolvable() {
        // Only disk moved: preflight fast-forwards the proven durable save.
        assert_eq!(
            assess(Some("base"), Some("base"), "disk"),
            AdmissionAssessment {
                divergence: AdmissionDivergence::Resolvable,
                adoption: DiskAdoption::FastForwardAuthorityToDisk,
            }
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

    /// `#admissionsplitmerge`: three durably distinct revisions are a genuine
    /// two-writer divergence, which is the normal condition for a realtime
    /// document — not a fault. It resolves by merge, never by refusing.
    #[test]
    fn three_distinct_revisions_merge_instead_of_refusing() {
        let assessment = assess(Some("base\n"), Some("live only\n"), "disk only\n");
        assert_eq!(
            assessment.divergence,
            AdmissionDivergence::ThreeWaySplitMerged
        );
        assert!(assessment.divergence.requires_disk_merge());
        assert_eq!(assessment.adoption, DiskAdoption::MergeDiskIntoAuthority);
    }

    /// The invariant that replaces the refusal: **no** observation of the three
    /// planes may select an action that declines to reconcile them. A stall was
    /// reintroduced once as a "safe" default; this test is what makes reaching for
    /// one again a red suite rather than a wedged queue.
    #[test]
    fn no_observation_of_the_three_planes_declines_to_reconcile() {
        let observations = [
            ("base", "base", "base"),
            ("base", "base", "disk"),
            ("base", "live", "base"),
            ("base", "same", "same"),
            ("base\n", "live only\n", "disk only\n"),
            ("# D\n\ni\n", "# D\n\ni\nc\ns\n", "# D\n\ni\nc\n"),
            ("# D\n\ni\n", "# D\n\ni\ns\n", "# D\n\ni\ne\n"),
            (
                "# D\n\nb\n",
                "# D\n\n<!-- agent:boundary:a:d -->\nb\n",
                "# D\n\nb\nx\n",
            ),
        ];
        for (baseline, authority, disk) in observations {
            let adoption = assess(Some(baseline), Some(authority), disk).adoption;
            assert!(
                matches!(
                    adoption,
                    DiskAdoption::ProceedOnAuthority
                        | DiskAdoption::FastForwardAuthorityToDisk
                        | DiskAdoption::MergeDiskIntoAuthority
                ),
                "every observation must reconcile, got {adoption:?} for \
                 baseline={baseline:?} authority={authority:?} disk={disk:?}"
            );
        }
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

    /// The live wedge: the baseline is checkpointed in the durable (stripped)
    /// domain while the editor authority still carries agent-doc's own boundary
    /// marker. Raw bytes make that three revisions; durable content does not, and
    /// the durable authority branch is unchanged, so the newer disk save is
    /// fast-forwarded exactly as it would be without the marker.
    #[test]
    fn a_boundary_marker_only_authority_is_not_a_split() {
        let durable = "# Doc\n\nbody\n";
        let authority = "# Doc\n\n<!-- agent:boundary:fe1c0161:agent-doc-bugs -->\nbody\n";
        let disk = "# Doc\n\nbody\ndurable save\n";
        let assessment = assess(Some(durable), Some(authority), disk);
        assert_eq!(
            assessment.divergence,
            AdmissionDivergence::TransientMarkersOnly
        );
        assert!(!assessment.divergence.requires_disk_merge());
        assert_eq!(
            assessment.adoption,
            DiskAdoption::FastForwardAuthorityToDisk
        );
    }

    /// A ` (HEAD)` suffix is 7 bytes of transport state on a heading. It must
    /// never be the reason an operator is told to close their editor tab.
    #[test]
    fn a_head_marker_only_disk_is_not_a_split() {
        let baseline = "# Doc\n\nold\n";
        let authority = "## Re: topic (HEAD)\n\nnew\n";
        let disk = "## Re: topic\n\nnew\n";
        let assessment = assess(Some(baseline), Some(authority), disk);
        assert_eq!(
            assessment.divergence,
            AdmissionDivergence::TransientMarkersOnly
        );
        assert_eq!(assessment.adoption, DiskAdoption::ProceedOnAuthority);
    }

    /// The blocked-commit shape: the baseline advanced to the live snapshot while
    /// disk kept the older committed revision, then the operator kept typing.
    /// Three distinct revisions, no rival writer — the authority carries every
    /// durable disk line.
    #[test]
    fn an_authority_that_contains_disk_is_not_a_split() {
        let baseline = "# Doc\n\nintro\n";
        let disk = "# Doc\n\nintro\ncommitted response\n";
        let authority = "# Doc\n\nintro\ncommitted response\noperator steering\n";
        let assessment = assess(Some(baseline), Some(authority), disk);
        assert_eq!(
            assessment.divergence,
            AdmissionDivergence::AuthoritySubsumesDisk
        );
        assert!(!assessment.divergence.requires_disk_merge());
        // Disk is an ancestor: there is nothing to fast-forward INTO the
        // authority, and adopting disk would drop the operator's steering.
        assert_eq!(assessment.adoption, DiskAdoption::ProceedOnAuthority);
    }

    /// Subsumption must not swallow a real conflict: disk content the authority
    /// never received is a genuine second writer, so it must select the merge
    /// rather than silently proceeding on the authority and dropping that text.
    #[test]
    fn disk_content_missing_from_the_authority_selects_the_merge() {
        let baseline = "# Doc\n\nintro\n";
        let disk = "# Doc\n\nintro\nexternal edit only on disk\n";
        let authority = "# Doc\n\nintro\noperator steering\n";
        let assessment = assess(Some(baseline), Some(authority), disk);
        assert_eq!(
            assessment.divergence,
            AdmissionDivergence::ThreeWaySplitMerged
        );
        assert_eq!(assessment.adoption, DiskAdoption::MergeDiskIntoAuthority);
    }

    /// `ProceedOnAuthority` is the only safe action when disk is durably equal to
    /// the authority or has not durably advanced past the baseline: adopting disk
    /// in either case would rewrite the live buffer for no durable gain.
    #[test]
    fn a_marker_only_disk_advance_is_never_adopted() {
        // Durable disk == durable authority (markers differ only).
        let assessment = assess(
            Some("# Doc\n\nold\n"),
            Some("## Re: t (HEAD)\n\nnew\n"),
            "## Re: t\n\nnew\n",
        );
        assert_eq!(assessment.adoption, DiskAdoption::ProceedOnAuthority);
        // Durable disk == durable baseline (disk only gained a marker).
        let assessment = assess(
            Some("# Doc\n\nold\n"),
            Some("# Doc\n\nold\nlive edit\n"),
            "# Doc\n\n<!-- agent:boundary:ab:doc -->\nold\n",
        );
        assert_eq!(
            assessment.divergence,
            AdmissionDivergence::TransientMarkersOnly
        );
        assert_eq!(assessment.adoption, DiskAdoption::ProceedOnAuthority);
    }

    /// Every classification names itself in the log, and the two benign shapes
    /// are distinguishable from the refusal by token alone.
    #[test]
    fn every_classification_has_a_distinct_log_token() {
        let tokens = [
            AdmissionDivergence::Resolvable,
            AdmissionDivergence::TransientMarkersOnly,
            AdmissionDivergence::AuthoritySubsumesDisk,
            AdmissionDivergence::ThreeWaySplitMerged,
        ]
        .map(AdmissionDivergence::reason_token);
        let unique: std::collections::HashSet<_> = tokens.iter().collect();
        assert_eq!(
            unique.len(),
            tokens.len(),
            "duplicate log token: {tokens:?}"
        );
        assert_eq!(
            AdmissionDivergence::ThreeWaySplitMerged.reason_token(),
            "three_way_split_merged"
        );
        let actions = [
            DiskAdoption::ProceedOnAuthority,
            DiskAdoption::FastForwardAuthorityToDisk,
            DiskAdoption::MergeDiskIntoAuthority,
        ]
        .map(DiskAdoption::action_token);
        let unique: std::collections::HashSet<_> = actions.iter().collect();
        assert_eq!(unique.len(), actions.len());
    }

    /// Only `MergeDiskIntoAuthority` may pair with a merge-requiring
    /// classification, and a merge-requiring classification may never pair with
    /// an action that proceeds without merging. A mismatch here is how a "fixed"
    /// gate silently stops protecting a writer.
    #[test]
    fn the_merge_verdict_and_the_action_cannot_disagree() {
        let observations = [
            ("base", "base", "disk"),
            ("base", "live", "base"),
            ("base", "same", "same"),
            ("base\n", "live only\n", "disk only\n"),
            (
                "# D\n\nb\n",
                "# D\n\n<!-- agent:boundary:a:d -->\nb\n",
                "# D\n\nb\nx\n",
            ),
            ("# D\n\ni\n", "# D\n\ni\nc\ns\n", "# D\n\ni\nc\n"),
            ("# D\n\ni\n", "# D\n\ni\ns\n", "# D\n\ni\ne\n"),
        ];
        for (baseline, authority, disk) in observations {
            let assessment = assess(Some(baseline), Some(authority), disk);
            assert_eq!(
                assessment.divergence.requires_disk_merge(),
                assessment.adoption == DiskAdoption::MergeDiskIntoAuthority,
                "verdict/action disagree for {assessment:?}"
            );
        }
    }

    /// The notice must report the split as reconciled, must not send anyone off to
    /// hand-converge planes the binary merges, and must still rule out the two
    /// recoveries that destroy a writer's text.
    #[test]
    fn the_notice_reports_a_merge_and_never_prescribes_a_destructive_action() {
        let notice = three_way_merge_notice("tasks/api.md");
        assert!(notice.contains("three-way merge"));
        assert!(notice.contains("no writer is dropped"));
        assert!(notice.contains("--force-disk"));
        // The stalling remedy must not come back in any of its phrasings.
        for stall in [
            "save or close this document's editor tab",
            "will keep refusing",
            "do NOT retry",
            "refuses",
        ] {
            assert!(
                !notice.contains(stall),
                "the stalling remedy phrase {stall:?} must not reappear: {notice}"
            );
        }
        // Saying the divergence is durable is what stops a marker-only split from
        // being read as a genuine one.
        assert!(notice.contains("not just transient agent-doc markers"));
    }
}
