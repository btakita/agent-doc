//! Pure turn-status vocabulary and pane-title policy.
//!
//! Callers own tmux commands, project-root resolution, and state.db IO.

use serde::{Deserialize, Serialize};

/// Pane-border title shown while a turn is in flight.
pub const TURN_ACTIVE_PANE_TITLE: &str = "⟳ agent-doc: turn in progress";

/// Leading marker prepended to the pane title when the route-owned supervisor is
/// running a stale binary.
pub const STALE_SUPERVISOR_PANE_MARKER: &str = "⚠ STALE SUPERVISOR";

/// Self-expiry window. A missed idle hook must not wedge the session busy.
pub const TURN_ACTIVE_TTL_SECS: u64 = 3600;

/// Projected turn-state contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnActiveMarker {
    /// The tmux pane the turn is running in (`$TMUX_PANE`), best-effort.
    pub pane: String,
    /// Unix seconds the turn went active, used for self-expiry.
    pub written_at: u64,
}

/// True when a turn-active fact is inside the freshness window.
pub fn turn_active_marker_is_fresh(marker: &TurnActiveMarker, now: u64) -> bool {
    now.saturating_sub(marker.written_at) < TURN_ACTIVE_TTL_SECS
}

/// True when a turn-active fact belongs to `pane`.
pub fn turn_active_marker_matches_pane(marker: &TurnActiveMarker, pane: &str) -> bool {
    marker.pane == pane
}

/// Title to set for a turn state. `active` uses the busy title; `idle` clears
/// the pane title so tmux returns to its default border title.
pub fn pane_title_for_state(active: bool) -> &'static str {
    if active { TURN_ACTIVE_PANE_TITLE } else { "" }
}

/// Single-marker title for a busy pane whose supervisor is stale (GH #124).
///
/// A pane title holds ONE marker. The pre-#124 composition welded the stale
/// marker onto the busy marker (`⚠ STALE SUPERVISOR ⟳ agent-doc: turn in
/// progress`), so a stale pane carried the live turn's own `⟳` marker as well
/// as the warning. The stale verdict replaces the busy glyph instead: the title
/// leads with the one warning marker and names the busy state as plain text.
pub const STALE_SUPERVISOR_TURN_ACTIVE_PANE_TITLE: &str = "⚠ STALE SUPERVISOR: turn in progress";

/// Compose the pane-border title for a turn state, decorated with the stale
/// supervisor marker when `stale` is true. Always exactly one marker.
pub fn pane_title_for_status(active: bool, stale: bool) -> String {
    compose_pane_title(pane_title_for_state(active), stale)
}

/// The undecorated title under any stale-supervisor decoration: the busy title,
/// empty (idle), or an operator-owned custom title. Reads every shape agent-doc
/// has written, including the pre-#124 welded `⚠ STALE SUPERVISOR ⟳ …` form.
pub fn undecorated_pane_title(title: &str) -> &str {
    if title == STALE_SUPERVISOR_TURN_ACTIVE_PANE_TITLE {
        return TURN_ACTIVE_PANE_TITLE;
    }
    match title.strip_prefix(STALE_SUPERVISOR_PANE_MARKER) {
        Some(rest) => rest.strip_prefix(' ').unwrap_or(rest),
        None => title,
    }
}

fn compose_pane_title(base: &str, stale: bool) -> String {
    match (stale, base) {
        (false, _) => base.to_string(),
        (true, "") => STALE_SUPERVISOR_PANE_MARKER.to_string(),
        (true, TURN_ACTIVE_PANE_TITLE) => STALE_SUPERVISOR_TURN_ACTIVE_PANE_TITLE.to_string(),
        (true, custom) => format!("{STALE_SUPERVISOR_PANE_MARKER} {custom}"),
    }
}

/// Refresh only the supervisor decoration; adoption must preserve the child turn title.
pub fn pane_title_with_freshness(title: &str, stale: bool) -> String {
    compose_pane_title(undecorated_pane_title(title), stale)
}

/// Number of agent-doc status markers (`⚠` stale, `⟳` busy) in a pane title.
/// GH #124: an agent-doc-composed title carries at most one.
pub fn pane_title_status_marker_count(title: &str) -> usize {
    title.matches('⚠').count() + title.matches('⟳').count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adoption_refresh_preserves_active_idle_and_custom_titles() {
        for title in ["", TURN_ACTIVE_PANE_TITLE, "custom title"] {
            let stale = pane_title_with_freshness(title, true);
            assert_eq!(pane_title_with_freshness(&stale, true), stale);
            assert_eq!(pane_title_with_freshness(&stale, false), title);
            assert_eq!(pane_title_with_freshness(title, false), title);
        }
    }

    #[test]
    fn pane_title_active_names_turn_in_progress() {
        assert_eq!(pane_title_for_state(true), TURN_ACTIVE_PANE_TITLE);
        assert!(pane_title_for_state(true).contains("turn in progress"));
    }

    #[test]
    fn pane_title_idle_clears_to_default() {
        assert_eq!(pane_title_for_state(false), "");
    }

    #[test]
    fn turn_active_marker_self_expires_after_ttl() {
        let marker = TurnActiveMarker {
            pane: "%7".to_string(),
            written_at: 1000,
        };
        assert!(turn_active_marker_is_fresh(
            &marker,
            1000 + TURN_ACTIVE_TTL_SECS - 1
        ));
        assert!(!turn_active_marker_is_fresh(
            &marker,
            1000 + TURN_ACTIVE_TTL_SECS
        ));
    }

    #[test]
    fn turn_active_marker_matches_only_marker_pane() {
        let marker = TurnActiveMarker {
            pane: "%7".to_string(),
            written_at: 1000,
        };
        assert!(turn_active_marker_matches_pane(&marker, "%7"));
        assert!(!turn_active_marker_matches_pane(&marker, "%8"));
    }

    #[test]
    fn pane_title_active_stale_leads_with_warning() {
        let title = pane_title_for_status(true, true);
        assert!(
            title.contains(STALE_SUPERVISOR_PANE_MARKER),
            "stale active title must contain the warning: {title}"
        );
        assert!(
            title.contains("turn in progress"),
            "stale active title must keep the turn-in-progress text: {title}"
        );
        assert!(
            title.starts_with(STALE_SUPERVISOR_PANE_MARKER),
            "warning must lead the title: {title}"
        );
    }

    #[test]
    fn gh124_stale_busy_title_holds_exactly_one_marker() {
        // A pane already carrying the stale marker that goes busy must not weld
        // the busy marker on (`⚠ STALE SUPERVISOR ⟳ agent-doc: turn in progress`).
        let busy_on_stale = pane_title_for_status(true, true);
        assert_eq!(pane_title_status_marker_count(&busy_on_stale), 1, "{busy_on_stale}");
        assert!(!busy_on_stale.contains(TURN_ACTIVE_PANE_TITLE), "{busy_on_stale}");
        let refreshed = pane_title_with_freshness(STALE_SUPERVISOR_PANE_MARKER, true);
        assert_eq!(pane_title_status_marker_count(&refreshed), 1, "{refreshed}");
        for active in [true, false] {
            for stale in [true, false] {
                let title = pane_title_for_status(active, stale);
                assert!(pane_title_status_marker_count(&title) <= 1, "{title}");
            }
        }
    }

    #[test]
    fn gh124_legacy_welded_title_normalises_to_one_marker() {
        let welded = format!("{STALE_SUPERVISOR_PANE_MARKER} {TURN_ACTIVE_PANE_TITLE}");
        assert_eq!(undecorated_pane_title(&welded), TURN_ACTIVE_PANE_TITLE);
        let stale = pane_title_with_freshness(&welded, true);
        assert_eq!(stale, STALE_SUPERVISOR_TURN_ACTIVE_PANE_TITLE);
        assert_eq!(pane_title_status_marker_count(&stale), 1);
        assert_eq!(pane_title_with_freshness(&welded, false), TURN_ACTIVE_PANE_TITLE);
        assert_eq!(
            pane_title_with_freshness(STALE_SUPERVISOR_TURN_ACTIVE_PANE_TITLE, false),
            TURN_ACTIVE_PANE_TITLE
        );
    }

    #[test]
    fn pane_title_active_fresh_has_no_warning() {
        let title = pane_title_for_status(true, false);
        assert_eq!(title, TURN_ACTIVE_PANE_TITLE);
        assert!(!title.contains(STALE_SUPERVISOR_PANE_MARKER));
    }

    #[test]
    fn pane_title_idle_stale_still_warns() {
        assert_eq!(
            pane_title_for_status(false, true),
            STALE_SUPERVISOR_PANE_MARKER
        );
    }

    #[test]
    fn pane_title_idle_fresh_clears() {
        assert_eq!(pane_title_for_status(false, false), "");
    }
}
