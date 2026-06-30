//! Pure turn-status marker vocabulary and pane-title policy.
//!
//! Callers own tmux commands, project-root resolution, and marker file IO.

use serde::{Deserialize, Serialize};

/// Pane-border title shown while a turn is in flight.
pub const TURN_ACTIVE_PANE_TITLE: &str = "⟳ agent-doc: turn in progress";

/// Leading marker prepended to the pane title when the route-owned supervisor is
/// running a stale binary.
pub const STALE_SUPERVISOR_PANE_MARKER: &str = "⚠ STALE SUPERVISOR";

/// Project-relative path of the readable stale-supervisor marker.
pub const STALE_SUPERVISOR_MARKER: &str = ".agent-doc/supervisor-stale";

/// Project-relative path of the readable turn-state marker.
pub const TURN_ACTIVE_MARKER: &str = ".agent-doc/turn-active.json";

/// Self-expiry window. A missed idle hook must not wedge the session busy.
pub const TURN_ACTIVE_TTL_SECS: u64 = 3600;

/// Readable turn-state marker contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnActiveMarker {
    /// The tmux pane the turn is running in (`$TMUX_PANE`), best-effort.
    pub pane: String,
    /// Unix seconds the turn went active, used for self-expiry.
    pub written_at: u64,
}

/// True when a turn-active marker is inside the freshness window.
pub fn turn_active_marker_is_fresh(marker: &TurnActiveMarker, now: u64) -> bool {
    now.saturating_sub(marker.written_at) < TURN_ACTIVE_TTL_SECS
}

/// True when a turn-active marker belongs to `pane`.
pub fn turn_active_marker_matches_pane(marker: &TurnActiveMarker, pane: &str) -> bool {
    marker.pane == pane
}

/// Title to set for a turn state. `active` uses the busy title; `idle` clears
/// the pane title so tmux returns to its default border title.
pub fn pane_title_for_state(active: bool) -> &'static str {
    if active { TURN_ACTIVE_PANE_TITLE } else { "" }
}

/// Compose the pane-border title for a turn state, decorated with the stale
/// supervisor marker when `stale` is true.
pub fn pane_title_for_status(active: bool, stale: bool) -> String {
    let base = pane_title_for_state(active);
    match (stale, base.is_empty()) {
        (false, _) => base.to_string(),
        (true, true) => STALE_SUPERVISOR_PANE_MARKER.to_string(),
        (true, false) => format!("{STALE_SUPERVISOR_PANE_MARKER} {base}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
