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
//!   focuses either — the layout projection already applied the route's focus
//!   at convergence, and any later focus belongs to the newest editor
//!   selection, which only the generation-sequenced layout plane knows;
//! - a standalone route keeps its immediate-focus behavior.

use std::path::Path;
use tmux_router::Tmux;

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
}

impl RouteFocusDecision {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Apply => "standalone_route_owns_focus",
            Self::SkipPreserveLayout => "preserve_route_layout",
            Self::SkipControllerLayoutOwnsFocus => "controller_layout_owns_focus",
        }
    }
}

/// Pure focus policy for a route invocation.
pub fn route_focus_decision(
    preserve_route_layout: bool,
    layout_owned_by_controller: bool,
) -> RouteFocusDecision {
    if preserve_route_layout {
        RouteFocusDecision::SkipPreserveLayout
    } else if layout_owned_by_controller {
        RouteFocusDecision::SkipControllerLayoutOwnsFocus
    } else {
        RouteFocusDecision::Apply
    }
}

/// The focus policy for the current route invocation (thread-local state).
pub fn current_route_focus_decision() -> RouteFocusDecision {
    route_focus_decision(
        crate::invocation::preserve_route_layout(),
        crate::invocation::layout_owned_by_controller(),
    )
}

/// Focus `pane` for this route if, and only if, the route owns focus.
///
/// Returns the decision that was applied. A failed `select_pane` is reported
/// as a warning and never fails the route (matching the previous call sites).
pub fn focus_route_pane(tmux: &Tmux, file: &Path, pane: &str, site: &str) -> RouteFocusDecision {
    let decision = current_route_focus_decision();
    match decision {
        RouteFocusDecision::Apply => {
            if let Err(e) = tmux.select_pane(pane) {
                eprintln!(
                    "[route] warning: failed to focus pane {} ({}): {}",
                    pane, site, e
                );
            }
        }
        RouteFocusDecision::SkipPreserveLayout => {}
        RouteFocusDecision::SkipControllerLayoutOwnsFocus => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_focus_skipped file={} pane={} site={} reason={} focus_effect=none (#focussynccurrent)",
                    file.display(),
                    pane,
                    site,
                    decision.reason(),
                ),
            );
        }
    }
    decision
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invocation::LayoutOwnedByControllerGuard;

    #[test]
    fn standalone_route_owns_focus() {
        assert_eq!(
            route_focus_decision(false, false),
            RouteFocusDecision::Apply
        );
    }

    #[test]
    fn preserved_layout_never_focuses_even_when_controller_owned() {
        assert_eq!(
            route_focus_decision(true, false),
            RouteFocusDecision::SkipPreserveLayout
        );
        assert_eq!(
            route_focus_decision(true, true),
            RouteFocusDecision::SkipPreserveLayout
        );
    }

    #[test]
    fn controller_owned_layout_route_never_focuses() {
        assert_eq!(
            route_focus_decision(false, true),
            RouteFocusDecision::SkipControllerLayoutOwnsFocus
        );
    }

    #[test]
    fn current_decision_tracks_controller_layout_guard() {
        assert_eq!(current_route_focus_decision(), RouteFocusDecision::Apply);
        {
            let _guard = LayoutOwnedByControllerGuard::set(true);
            assert_eq!(
                current_route_focus_decision(),
                RouteFocusDecision::SkipControllerLayoutOwnsFocus
            );
        }
        assert_eq!(current_route_focus_decision(), RouteFocusDecision::Apply);
    }
}
