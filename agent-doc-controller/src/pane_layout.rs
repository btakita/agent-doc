//! Pure latest-wins ownership for the pane-layout projection worker.
//!
//! The IO layer supplies the effect worker. This state owns the race-sensitive
//! decision: a newer input revision published while the current effect is
//! finishing must keep one worker active.

pub use agent_doc_editor_surface::editor_view_policy::MainLayoutEligibility;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LatestProjectionWorkerState {
    pending_revision: u64,
    active: bool,
}

impl LatestProjectionWorkerState {
    /// Record an exact-input revision and return whether the caller must start
    /// the single worker. An already-active worker observes the newer revision
    /// through the same retained state.
    pub fn schedule(&mut self, revision: u64) -> bool {
        self.pending_revision = self.pending_revision.max(revision);
        if self.active {
            false
        } else {
            self.active = true;
            true
        }
    }

    pub fn pending_revision(&self) -> u64 {
        self.pending_revision
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn is_superseded(&self, revision: u64) -> bool {
        self.pending_revision > revision
    }

    /// Retire only when no newer retained input revision is waiting. Holding
    /// the IO layer's mutex around this decision closes the publish-vs-exit race.
    pub fn retire_if_current(&mut self, completed_revision: u64) -> bool {
        if self.is_superseded(completed_revision) {
            return false;
        }
        self.active = false;
        true
    }

    pub fn deactivate(&mut self) {
        self.active = false;
    }

    /// Release a worker that exited without retiring normally (for example,
    /// after an effect panic) and report whether a newer retained revision
    /// must be picked up by a replacement worker.
    ///
    /// This transition is performed while the IO layer holds its worker-state
    /// mutex. A publication therefore either lands before this check and is
    /// inherited by the replacement, or lands after deactivation and starts a
    /// worker itself. The failed revision is deliberately not retried here: a
    /// deterministic effect panic must not become a hot respawn loop.
    pub fn recover_after_failure(&mut self, failed_revision: u64) -> bool {
        self.active = false;
        if self.pending_revision <= failed_revision {
            return false;
        }
        self.active = true;
        true
    }
}

/// True when a pane's *recorded* window binding no longer matches the window the
/// pane actually lives in right now.
///
/// The actor record and the durable registry each carry a `window` captured when
/// the pane was bound, and nothing re-derives it. When a pane is later moved into
/// the `stash` window, both records keep naming the visible `agent-doc` window, so
/// every record-vs-record comparison still agrees and the drift is invisible — the
/// pane-layout projection then counts a stashed pane as a co-visible column and
/// mirrors editor focus onto a pane the operator cannot see.
///
/// The live window is the only authority for where a pane *is*; a recorded window
/// is only evidence of where it *was*.
///
/// Strict tightening, mirroring the cross-repo owner guard: an unknown or empty
/// value on either side is never drift. A pane we cannot locate must not be
/// reported as misplaced, or a transient tmux read turns into a false repair.
/// True when a pane currently lives in a stash window.
///
/// `#stashfocusleg`: the second, layout-window-independent refusal leg.
/// [`pane_window_binding_drifted`] can only answer when the layout's target
/// window is known, and `pane_layout_target_window_id` returns `None` whenever
/// the invocation carries no `@`-prefixed window id AND the project has no
/// resolvable configured session or no window named `agent-doc`. At that point
/// the co-visibility guard was skipped entirely and focus was mirrored onto
/// whatever pane the file→pane record named, stash window included. Reported
/// 2026-09-20: navigating the editor to `sample-session.md` pulled tmux over
/// to the `stash` window.
///
/// The stash window is agent-doc's own parking area, so "this pane is stashed"
/// is a sufficient refusal on its own and needs no layout window to compare
/// against. Same strictness as the drift check: an unknown or empty name is
/// never a refusal, so a transient tmux read cannot suppress legitimate focus.
pub fn pane_is_stashed(live_window_name: Option<&str>) -> bool {
    live_window_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .is_some_and(crate::dispatch::is_stash_window_name)
}

pub fn pane_window_binding_drifted(recorded_window: &str, live_window: Option<&str>) -> bool {
    let recorded = recorded_window.trim();
    if recorded.is_empty() {
        return false;
    }
    let Some(live) = live_window.map(str::trim).filter(|live| !live.is_empty()) else {
        return false;
    };
    recorded != live
}

/// GH #136 follow-up (a): where a desired layout publication's column COUNT
/// comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutWidthAuthority {
    /// The publication is itself a positive observation of the editor's split
    /// at this width: a plugin publication, an `exact` route, an editor-surface
    /// `Sync`, or an explicit operator command. It may widen the window — an
    /// operator opening a split publishes `observed + 1` by definition — and
    /// its realisation is bounded by the stale-column gate instead (a pane that
    /// is not proven fresh never adds a column, from any window).
    EditorSplitObservation,
    /// The column count was recomputed from retained controller state: a focus
    /// escalation, an `ensure` route, or a recycle republish. Such a
    /// publication observes nothing new about the split, so it may never be
    /// wider than what the last layout pass accounted for.
    Derived,
}

impl LayoutWidthAuthority {
    pub fn label(self) -> &'static str {
        match self {
            Self::EditorSplitObservation => "editor_split_observation",
            Self::Derived => "derived",
        }
    }
}

/// The local facts a width bound is measured against, read inside the
/// controller at publication time. None of them is an editor message, so the
/// bound cannot depend on the order, timing, or delivery of publications.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutWidthExtent {
    /// Panes in the target window observed by the pass that realised the
    /// RETAINED generation (`None` when that generation has not been observed:
    /// a publication racing the effect never reads a stale generation's count).
    pub observed_panes: Option<usize>,
    /// Columns that same pass deliberately did not realise (the GH #136
    /// stale-column gate). They are accounted for, not missing: a derived
    /// republish that keeps them is not a widening.
    pub gated_columns: usize,
    /// Columns of the retained desired layout this publication replaces.
    pub retained_columns: usize,
    /// Columns the publication itself asserts (an `ensure` route's own
    /// `--col` arguments, a focus escalation's document). A derived
    /// publication may always realise what it asserts.
    pub asserted_columns: usize,
}

impl LayoutWidthExtent {
    /// The widest layout a DERIVED publication may publish. It never widens
    /// the retained layout it derives from, and once the pass realising that
    /// layout has observed tmux, it never re-requests a column tmux did not
    /// realise and the gate did not account for:
    ///
    /// `max(min(retained, observed + gated), asserted, 1)`, or
    /// `max(retained, asserted, 1)` while the retained generation is unobserved.
    pub fn derived_bound(&self) -> usize {
        let accounted = match self.observed_panes {
            Some(panes) => self
                .retained_columns
                .min(panes.saturating_add(self.gated_columns)),
            None => self.retained_columns,
        };
        accounted.max(self.asserted_columns).max(1)
    }

    /// `max(observed_panes, 1)`, the bound the issue measures every
    /// publisher against (`unknown` before the first observation).
    pub fn observed_bound(&self) -> Option<usize> {
        self.observed_panes.map(|panes| panes.max(1))
    }
}

/// What [`bound_layout_width`] did to one publication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutWidthDecision {
    /// Within `max(observed_panes, 1)` (or nothing observed yet): unchanged.
    Within,
    /// A positive editor split observation wider than the observed window:
    /// published unchanged and attributable (`pane_layout_publication_widened`).
    Widened { observed_bound: usize },
    /// A derived publication within its derived bound but wider than the
    /// observed panes because the extra columns were gated, asserted, or
    /// retained before any observation: unchanged.
    DerivedWithinAccounted { derived_bound: usize },
    /// A derived publication wider than its derived bound: the rightmost
    /// columns that do not hold `focus` were dropped.
    Trimmed {
        derived_bound: usize,
        dropped: Vec<String>,
    },
}

fn column_holds(column: &str, document: &str) -> bool {
    column
        .split(',')
        .map(str::trim)
        .any(|file| !file.is_empty() && file == document)
}

/// GH #136 follow-up (a): bound a desired layout publication's width.
///
/// Total and pure. Invariant: the result is never wider than
/// `extent.derived_bound()` for a [`LayoutWidthAuthority::Derived`]
/// publication, it keeps every column that holds `focus`, and it preserves
/// column order. An editor split observation is never changed here.
pub fn bound_layout_width(
    columns: &[String],
    focus: Option<&str>,
    authority: LayoutWidthAuthority,
    extent: LayoutWidthExtent,
) -> (Vec<String>, LayoutWidthDecision) {
    let observed_bound = extent.observed_bound();
    let wider_than_observed = observed_bound.is_some_and(|bound| columns.len() > bound);
    match authority {
        LayoutWidthAuthority::EditorSplitObservation => {
            let decision = match observed_bound {
                Some(bound) if wider_than_observed => LayoutWidthDecision::Widened {
                    observed_bound: bound,
                },
                _ => LayoutWidthDecision::Within,
            };
            (columns.to_vec(), decision)
        }
        LayoutWidthAuthority::Derived => {
            let derived_bound = extent.derived_bound();
            if columns.len() <= derived_bound {
                let decision = if wider_than_observed {
                    LayoutWidthDecision::DerivedWithinAccounted { derived_bound }
                } else {
                    LayoutWidthDecision::Within
                };
                return (columns.to_vec(), decision);
            }
            // Keep the focus columns first, then the leftmost others, until
            // the bound; emit in the original order.
            let focus_columns = columns
                .iter()
                .filter(|column| focus.is_some_and(|focus| column_holds(column, focus)))
                .count();
            let mut others_budget = derived_bound.saturating_sub(focus_columns);
            let mut kept = Vec::with_capacity(derived_bound);
            let mut dropped = Vec::new();
            for column in columns {
                let holds_focus = focus.is_some_and(|focus| column_holds(column, focus));
                if holds_focus {
                    kept.push(column.clone());
                } else if others_budget > 0 {
                    others_budget -= 1;
                    kept.push(column.clone());
                } else {
                    dropped.push(column.clone());
                }
            }
            (
                kept,
                LayoutWidthDecision::Trimmed {
                    derived_bound,
                    dropped,
                },
            )
        }
    }
}

/// GH #136 follow-up (d): whether a supervisor recycle settling for one
/// document republishes the retained layout.
///
/// The last layout pass gated the document's column out because its
/// supervisor ran replaced bytes. Re-admission used to wait for the NEXT
/// editor publication — over Remote Dev + Zscaler that can be never (a dropped
/// or stalled frame), and on any backend it is unrelated to the event that
/// actually changed the answer. The settle itself is that event, so it
/// re-runs the pass, once per gated pass:
///
/// * only when the settling document was gated by the pass for the CURRENT
///   desired generation (a newer publication already re-runs the gate);
/// * at most once per gated pass (a retried or duplicated settle message is
///   idempotent: the republished pass records a fresh gated set, and a still
///   stale supervisor is simply gated again by a NEWER pass).
pub fn recycle_settle_republishes_layout(
    settled_document_was_gated: bool,
    gated_generation: u64,
    desired_generation: Option<u64>,
    republished_for_gated_generation: u64,
) -> bool {
    settled_document_was_gated
        && gated_generation > 0
        && desired_generation == Some(gated_generation)
        && republished_for_gated_generation < gated_generation
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GH #136 follow-up (d), exhaustive: republish exactly when the settling
    /// document was gated by the current generation's pass and that pass has
    /// not already been republished; a duplicate settle never republishes twice.
    #[test]
    fn gh136d_recycle_settle_republish_transition_table() {
        for gated in [false, true] {
            for gated_generation in 0..=3u64 {
                for desired in [None, Some(1u64), Some(2), Some(3)] {
                    for republished in 0..=3u64 {
                        let fire = recycle_settle_republishes_layout(
                            gated,
                            gated_generation,
                            desired,
                            republished,
                        );
                        let ctx = format!("{gated} {gated_generation} {desired:?} {republished}");
                        assert_eq!(
                            fire,
                            gated
                                && gated_generation > 0
                                && desired == Some(gated_generation)
                                && republished < gated_generation,
                            "{ctx}"
                        );
                        if fire {
                            // After republishing for this pass, the same settle
                            // (duplicated or retried) is a no-op.
                            assert!(
                                !recycle_settle_republishes_layout(
                                    gated,
                                    gated_generation,
                                    desired,
                                    gated_generation
                                ),
                                "{ctx}"
                            );
                        }
                    }
                }
            }
        }
    }

    fn cols(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    fn extent(
        observed: Option<usize>,
        gated: usize,
        retained: usize,
        asserted: usize,
    ) -> LayoutWidthExtent {
        LayoutWidthExtent {
            observed_panes: observed,
            gated_columns: gated,
            retained_columns: retained,
            asserted_columns: asserted,
        }
    }

    /// GH #136 follow-up (a), exhaustive over small instances: a derived
    /// publication is never wider than `max(observed + gated, asserted, 1)`
    /// (or the retained width before any observation), never loses its focus
    /// column, and keeps column order; an editor observation is never changed.
    #[test]
    fn gh136a_width_bound_transition_table_is_exhaustive() {
        let universe = cols(&["a", "b", "c", "d", "e"]);
        let mut cases = 0usize;
        for width in 1..=5usize {
            let columns = &universe[..width];
            for focus in [None, Some("a"), Some("c"), Some("e")] {
                for observed in [None, Some(0), Some(1), Some(2), Some(3)] {
                    for gated in 0..=2usize {
                        for retained in 0..=3usize {
                            for asserted in 0..=2usize {
                                let extent = extent(observed, gated, retained, asserted);
                                for authority in [
                                    LayoutWidthAuthority::Derived,
                                    LayoutWidthAuthority::EditorSplitObservation,
                                ] {
                                    cases += 1;
                                    let (kept, decision) =
                                        bound_layout_width(columns, focus, authority, extent);
                                    let ctx = format!(
                                        "{columns:?} focus={focus:?} {extent:?} {authority:?} -> {kept:?} {decision:?}"
                                    );
                                    // Order preserved, nothing invented.
                                    let mut cursor = columns.iter();
                                    for column in &kept {
                                        assert!(cursor.any(|c| c == column), "{ctx}");
                                    }
                                    if authority == LayoutWidthAuthority::EditorSplitObservation {
                                        assert_eq!(kept, columns, "{ctx}");
                                        continue;
                                    }
                                    let focus_held =
                                        focus.is_some_and(|f| columns.iter().any(|c| c == f));
                                    // Never wider than the derived bound, unless
                                    // the focus column alone exceeds it (it cannot:
                                    // the bound is at least 1).
                                    assert!(kept.len() <= extent.derived_bound(), "{ctx}");
                                    if focus_held {
                                        assert!(
                                            kept.iter().any(|c| Some(c.as_str()) == focus),
                                            "{ctx}"
                                        );
                                    }
                                    // Untouched whenever it fits.
                                    if columns.len() <= extent.derived_bound() {
                                        assert_eq!(kept, columns, "{ctx}");
                                    } else {
                                        assert_eq!(kept.len(), extent.derived_bound(), "{ctx}");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 5 * 4 * 5 * 3 * 4 * 3 * 2);
    }

    /// The GH #136 escalation shape: the retained layout holds 3 columns, tmux
    /// observed 2 panes and the last pass gated the third (a stale supervisor).
    /// Republishing the retained layout is accounted for, not a widening.
    #[test]
    fn gh136a_a_gated_column_is_accounted_for_not_a_widening() {
        let three = cols(&["b", "x", "a"]);
        let (kept, decision) = bound_layout_width(
            &three,
            Some("a"),
            LayoutWidthAuthority::Derived,
            extent(Some(2), 1, 3, 1),
        );
        assert_eq!(kept, three);
        assert_eq!(
            decision,
            LayoutWidthDecision::DerivedWithinAccounted { derived_bound: 3 }
        );
        // Nothing gated: the same republish would add a column. Trimmed, the
        // focus column kept, the rightmost other dropped.
        let (kept, decision) = bound_layout_width(
            &three,
            Some("a"),
            LayoutWidthAuthority::Derived,
            extent(Some(2), 0, 3, 1),
        );
        assert_eq!(kept, cols(&["b", "a"]));
        assert_eq!(
            decision,
            LayoutWidthDecision::Trimmed {
                derived_bound: 2,
                dropped: cols(&["x"])
            }
        );
    }

    /// The GH #120 route shape that recurred five times after #120 closed:
    /// `columns == observed_panes + 1` from an `ensure` route. Whatever merge
    /// produced it, the generic bound refuses the extra column.
    #[test]
    fn gh136a_ensure_route_cannot_publish_observed_plus_one() {
        let (kept, _) = bound_layout_width(
            &cols(&["a", "b", "c"]),
            Some("c"),
            LayoutWidthAuthority::Derived,
            extent(Some(2), 0, 3, 1),
        );
        assert_eq!(kept, cols(&["a", "c"]));
        // A positive split observation of three is published unchanged and
        // reported as a widening.
        let (kept, decision) = bound_layout_width(
            &cols(&["a", "b", "c"]),
            Some("c"),
            LayoutWidthAuthority::EditorSplitObservation,
            extent(Some(2), 0, 3, 3),
        );
        assert_eq!(kept.len(), 3);
        assert_eq!(decision, LayoutWidthDecision::Widened { observed_bound: 2 });
    }

    /// A derived publication never widens the retained layout, whatever tmux
    /// observed (an operator-owned extra pane is not room for a column).
    #[test]
    fn gh136a_derived_publication_never_widens_the_retained_layout() {
        let (kept, _) = bound_layout_width(
            &cols(&["a", "b", "c"]),
            Some("a"),
            LayoutWidthAuthority::Derived,
            extent(Some(3), 0, 2, 1),
        );
        assert_eq!(kept, cols(&["a", "b"]));
    }

    /// Before any tmux observation a derived publication cannot widen the
    /// retained layout it derives from; a multi-document column counts once.
    #[test]
    fn gh136a_unobserved_derived_publication_is_bounded_by_retained() {
        let (kept, _) = bound_layout_width(
            &cols(&["a,b", "c"]),
            Some("b"),
            LayoutWidthAuthority::Derived,
            extent(None, 0, 1, 1),
        );
        assert_eq!(kept, cols(&["a,b"]));
        let (kept, decision) = bound_layout_width(
            &cols(&["a"]),
            Some("a"),
            LayoutWidthAuthority::Derived,
            extent(None, 0, 0, 0),
        );
        assert_eq!(kept, cols(&["a"]));
        assert_eq!(decision, LayoutWidthDecision::Within);
    }

    #[test]
    fn newer_input_revision_prevents_the_active_worker_from_retiring() {
        let mut state = LatestProjectionWorkerState::default();
        assert!(state.schedule(7));
        assert!(!state.schedule(8));
        assert!(state.is_superseded(7));
        assert!(!state.retire_if_current(7));
        assert!(state.is_active());
        assert!(state.retire_if_current(8));
        assert!(!state.is_active());
    }

    #[test]
    fn failed_worker_releases_current_revision_for_the_next_navigation() {
        let mut state = LatestProjectionWorkerState::default();
        assert!(state.schedule(7));

        assert!(!state.recover_after_failure(7));
        assert!(!state.is_active());
        assert!(state.schedule(8));
        assert!(state.is_active());
    }

    #[test]
    fn failed_worker_hands_newer_retained_revision_to_a_replacement() {
        let mut state = LatestProjectionWorkerState::default();
        assert!(state.schedule(7));
        assert!(!state.schedule(8));

        assert!(state.recover_after_failure(7));
        assert!(state.is_active());
        assert_eq!(state.pending_revision(), 8);
    }

    /// The operator-reported shape: the record says the visible `agent-doc`
    /// window, tmux has the pane in `stash`. Every record-vs-record check agrees,
    /// so this comparison is the only one that can see it.
    #[test]
    fn stashed_pane_is_drift_even_when_records_agree() {
        assert!(pane_window_binding_drifted("@894", Some("@904")));
    }

    #[test]
    fn matching_window_is_not_drift() {
        assert!(!pane_window_binding_drifted("@894", Some("@894")));
        assert!(!pane_window_binding_drifted(" @894 ", Some("@894")));
    }

    /// Strict tightening: a pane we cannot locate is never reported as misplaced,
    /// so a transient tmux read cannot trigger a false repair.
    #[test]
    fn unknown_or_empty_on_either_side_is_never_drift() {
        assert!(!pane_window_binding_drifted("@894", None));
        assert!(!pane_window_binding_drifted("@894", Some("")));
        assert!(!pane_window_binding_drifted("@894", Some("   ")));
        assert!(!pane_window_binding_drifted("", Some("@904")));
        assert!(!pane_window_binding_drifted("   ", Some("@904")));
        assert!(!pane_window_binding_drifted("", None));
    }

}
