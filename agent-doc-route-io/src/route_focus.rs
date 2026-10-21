//! Route-level tmux focus effects (`#focussynccurrent`).
//!
//! A route runs for seconds (preflight repair, closeout, ready wait, submit).
//! While it runs the operator can switch the editor to another document, and
//! the controller layout plane — which sequences every editor selection by a
//! monotonic layout generation — swaps the newer document's pane in and parks
//! the routed pane in the `stash` window. A route that then calls
//! `Tmux::select_pane` (which batches `select-window` + `select-pane`) acts on
//! the selection it started with, not the current one: it drags the attached
//! client into `stash` and focuses a document the editor no longer shows.
//!
//! Observed 2026-10-11: `rpg.md` routed at 06:43:53 and its layout converged
//! (`focus_applied:rpg.md:%182`); the editor switched to `sdk.md` at 06:43:59
//! (generation 42 swapped `%180` in, `%182` to stash); the rpg route's
//! `route_stash_rescue_skipped ... window=stash` correctly left topology alone
//! but its direct-pane submit still focused `%182`, so tmux showed rpg.md in
//! `1:stash` while the editor showed sdk.md.
//!
//! Every route focus effect therefore goes through [`focus_route_pane`]:
//!
//! - a route that must preserve the caller's surface (background recovery,
//!   cross-document child route) never focuses;
//! - a route whose layout is owned by the controller (`editor_route`) never
//!   focuses either — the tmux layout effect already applied the route's focus
//!   at convergence, and any later focus belongs to the newest editor
//!   selection, which only the generation-sequenced layout plane knows;
//! - a standalone route keeps immediate focus only when the target is proven
//!   outside stash; stash and unproven placement fail closed.

use anyhow::Result;
use std::path::Path;
use tmux_router::Tmux;

use agent_doc_controller::dispatch::is_stash_window_name;

/// Authority carried by one route invocation for foreground tmux focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteFocusAuthority {
    /// Background recovery/re-entry and cross-document delivery preserve the
    /// foreground surface.
    Background,
    /// The generation-sequenced controller layout plane owns foreground focus.
    ControllerLayout,
    /// A standalone foreground route may focus a proven foreground target.
    StandaloneForeground,
}

/// Placement proof for the focus target at the instant of the effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteTargetPlacement {
    Foreground,
    Stash,
    Unproven,
}

/// What a route may do with tmux focus for its target pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteFocusDecision {
    /// Standalone route: it owns focus and selects its pane.
    Apply,
    /// Background / cross-document route: the caller's surface is preserved.
    SkipPreserveLayout,
    /// Controller-owned layout: focus is sequenced by the layout plane, so a
    /// late route focus could only override a newer editor selection.
    SkipControllerLayoutOwnsFocus,
    /// The target currently lives in a stash window. Selecting it would issue
    /// `select-window` and surface a background document.
    SkipStashedTarget,
    /// Target placement could not be proven. Foreground authority fails closed.
    SkipUnprovenTarget,
}

impl RouteFocusDecision {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Apply => "standalone_route_owns_focus",
            Self::SkipPreserveLayout => "preserve_route_layout",
            Self::SkipControllerLayoutOwnsFocus => "controller_layout_owns_focus",
            Self::SkipStashedTarget => "target_is_stashed",
            Self::SkipUnprovenTarget => "target_placement_unproven",
        }
    }
}

/// Pure focus policy for a route invocation.
///
/// Formal authority invariant:
///
/// `Apply ⇔ authority = StandaloneForeground ∧ placement = Foreground`.
///
/// All other rows are non-effects. In particular, background work may still
/// inject into its proven pane, but it never selects a pane/window.
pub fn route_focus_decision(
    authority: RouteFocusAuthority,
    placement: RouteTargetPlacement,
) -> RouteFocusDecision {
    match authority {
        RouteFocusAuthority::Background => RouteFocusDecision::SkipPreserveLayout,
        RouteFocusAuthority::ControllerLayout => RouteFocusDecision::SkipControllerLayoutOwnsFocus,
        RouteFocusAuthority::StandaloneForeground => match placement {
            RouteTargetPlacement::Foreground => RouteFocusDecision::Apply,
            RouteTargetPlacement::Stash => RouteFocusDecision::SkipStashedTarget,
            RouteTargetPlacement::Unproven => RouteFocusDecision::SkipUnprovenTarget,
        },
    }
}

/// The focus authority for the current route invocation (thread-local state).
pub fn current_route_focus_authority() -> RouteFocusAuthority {
    if crate::invocation::preserve_route_layout() {
        RouteFocusAuthority::Background
    } else if crate::invocation::layout_owned_by_controller() {
        RouteFocusAuthority::ControllerLayout
    } else {
        RouteFocusAuthority::StandaloneForeground
    }
}

fn target_placement(tmux: &Tmux, pane: &str) -> RouteTargetPlacement {
    match agent_doc_tmux_io::target_window_name(tmux, pane) {
        Some(window) if is_stash_window_name(&window) => RouteTargetPlacement::Stash,
        Some(_) => RouteTargetPlacement::Foreground,
        None => RouteTargetPlacement::Unproven,
    }
}

fn apply_focus_effect(
    decision: RouteFocusDecision,
    select: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if decision == RouteFocusDecision::Apply {
        select()
    } else {
        Ok(())
    }
}

/// Focus `pane` for this route if, and only if, the route owns focus.
///
/// Returns the decision that was applied. A failed `select_pane` is reported
/// as a warning and never fails the route (matching the previous call sites).
pub fn focus_route_pane(tmux: &Tmux, file: &Path, pane: &str, site: &str) -> RouteFocusDecision {
    let placement = target_placement(tmux, pane);
    let decision = route_focus_decision(current_route_focus_authority(), placement);
    if let Err(e) = apply_focus_effect(decision, || tmux.select_pane(pane)) {
        eprintln!(
            "[route] warning: failed to focus pane {} ({}): {}",
            pane, site, e
        );
    }
    if decision != RouteFocusDecision::Apply {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_focus_skipped file={} pane={} site={} authority={:?} placement={:?} reason={} focus_effect=none (#focussynccurrent) (#stashbackgroundreentry)",
                file.display(),
                pane,
                site,
                current_route_focus_authority(),
                placement,
                decision.reason(),
            ),
        );
    }
    decision
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invocation::{BackgroundExistingPaneOnlyGuard, LayoutOwnedByControllerGuard};
    use std::cell::Cell;

    #[test]
    fn standalone_route_owns_focus() {
        assert_eq!(
            route_focus_decision(
                RouteFocusAuthority::StandaloneForeground,
                RouteTargetPlacement::Foreground,
            ),
            RouteFocusDecision::Apply
        );
    }

    #[test]
    fn preserved_layout_never_focuses_even_when_controller_owned() {
        assert_eq!(
            route_focus_decision(
                RouteFocusAuthority::Background,
                RouteTargetPlacement::Foreground,
            ),
            RouteFocusDecision::SkipPreserveLayout
        );
        assert_eq!(
            route_focus_decision(RouteFocusAuthority::Background, RouteTargetPlacement::Stash,),
            RouteFocusDecision::SkipPreserveLayout
        );
    }

    #[test]
    fn controller_owned_layout_route_never_focuses() {
        assert_eq!(
            route_focus_decision(
                RouteFocusAuthority::ControllerLayout,
                RouteTargetPlacement::Foreground,
            ),
            RouteFocusDecision::SkipControllerLayoutOwnsFocus
        );
    }

    #[test]
    fn current_decision_tracks_controller_layout_guard() {
        assert_eq!(
            current_route_focus_authority(),
            RouteFocusAuthority::StandaloneForeground
        );
        {
            let _guard = LayoutOwnedByControllerGuard::set(true);
            assert_eq!(
                current_route_focus_authority(),
                RouteFocusAuthority::ControllerLayout
            );
        }
        assert_eq!(
            current_route_focus_authority(),
            RouteFocusAuthority::StandaloneForeground
        );
    }

    #[test]
    fn background_authority_dominates_controller_layout_authority() {
        let _layout = LayoutOwnedByControllerGuard::set(true);
        let _background = BackgroundExistingPaneOnlyGuard::set(true);
        assert_eq!(
            current_route_focus_authority(),
            RouteFocusAuthority::Background
        );
    }

    #[test]
    fn standalone_stash_and_unproven_targets_never_run_select_effect() {
        let calls = Cell::new(0);
        for placement in [RouteTargetPlacement::Stash, RouteTargetPlacement::Unproven] {
            let decision =
                route_focus_decision(RouteFocusAuthority::StandaloneForeground, placement);
            apply_focus_effect(decision, || {
                calls.set(calls.get() + 1);
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(calls.get(), 0, "no select-window/select-pane effect");
    }

    #[test]
    fn focus_authority_truth_table_has_exactly_one_apply_row() {
        let authorities = [
            RouteFocusAuthority::Background,
            RouteFocusAuthority::ControllerLayout,
            RouteFocusAuthority::StandaloneForeground,
        ];
        let placements = [
            RouteTargetPlacement::Foreground,
            RouteTargetPlacement::Stash,
            RouteTargetPlacement::Unproven,
        ];
        for authority in authorities {
            for placement in placements {
                assert_eq!(
                    route_focus_decision(authority, placement) == RouteFocusDecision::Apply,
                    authority == RouteFocusAuthority::StandaloneForeground
                        && placement == RouteTargetPlacement::Foreground,
                    "truth-table row: authority={authority:?} placement={placement:?}",
                );
            }
        }
    }
}
