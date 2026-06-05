//! `agent-doc turn-status active|idle` — surface a turn-in-progress status on the
//! agent's own tmux pane (`#claude-busy-status-during-active-turn`, continuous
//! monitor).
//!
//! Designed to be driven by harness turn-lifecycle hooks: a `UserPromptSubmit`
//! hook calls `active` when a turn starts and a `Stop` hook calls `idle` when it
//! ends. Because the hook runs INSIDE the agent's own pane, it sets that pane's
//! border title via `$TMUX_PANE` — no supervisor poll thread and no document/pane
//! resolution. Crucially the harness fires `Stop` only after the WHOLE turn
//! completes, including any Bash it auto-backgrounded and re-invoked on, so the
//! status covers the backgrounded window that pane busy-cue detection (which only
//! reads visible pane content) fundamentally cannot see.
//!
//! Visibility note: the status rides the pane border title, shown when tmux
//! `pane-border-status` is enabled. The command is best-effort — it never fails
//! the turn: outside tmux, or on any tmux error, it succeeds quietly.

use crate::sessions;

/// Pane-border title shown while a turn is in flight.
pub const TURN_ACTIVE_PANE_TITLE: &str = "⟳ agent-doc: turn in progress";

/// Title to set for a turn state. `active` → the busy title; `idle` → empty, so
/// the pane returns to its default border title.
pub fn pane_title_for_state(active: bool) -> &'static str {
    if active {
        TURN_ACTIVE_PANE_TITLE
    } else {
        ""
    }
}

/// Set the current tmux pane's border title to reflect the turn state. No-op
/// (Ok) when not running inside a tmux pane so the hook never breaks the turn.
pub fn run(active: bool) -> anyhow::Result<()> {
    let Ok(pane) = std::env::var("TMUX_PANE") else {
        return Ok(());
    };
    let pane = pane.trim().to_string();
    if pane.is_empty() {
        return Ok(());
    }
    let title = pane_title_for_state(active);
    let tmux = sessions::Tmux::default_server();
    if let Err(e) = tmux
        .cmd()
        .args(["select-pane", "-t", &pane, "-T", title])
        .status()
    {
        // Best-effort UX surface — must never fail the turn.
        eprintln!("[turn-status] warning: failed to set pane {pane} title: {e}");
    }
    Ok(())
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
        // Idle resets to empty so the pane border returns to its default title
        // — the status must not linger after the turn ends.
        assert_eq!(pane_title_for_state(false), "");
    }
}
