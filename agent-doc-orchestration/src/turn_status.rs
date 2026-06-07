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
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Pane-border title shown while a turn is in flight.
pub const TURN_ACTIVE_PANE_TITLE: &str = "⟳ agent-doc: turn in progress";

/// Project-relative path of the readable turn-state marker
/// (`#claude-busy-status-during-active-turn`). Written by `turn-status active`,
/// removed by `turn-status idle`. Lets route/supervisor *read* whether the
/// agent is mid-turn — the bridge toward a hard busy-lease (gated on
/// `#subagent-blocks-session`) without yet wiring a fail-closed block.
pub const TURN_ACTIVE_MARKER: &str = ".agent-doc/turn-active.json";

/// Self-expiry window. A missed `idle`/`Stop` hook must not wedge the session
/// as perpetually busy, so a marker older than this is read as stale (idle).
/// Sized well above any realistic single turn (including a long backgrounded
/// build) while still self-healing within an hour.
pub const TURN_ACTIVE_TTL_SECS: u64 = 3600;

/// Readable turn-state marker contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnActiveMarker {
    /// The tmux pane the turn is running in (`$TMUX_PANE`), best-effort.
    pub pane: String,
    /// Unix seconds the turn went active — used for self-expiry.
    pub written_at: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn marker_path(base: &Path) -> PathBuf {
    base.join(TURN_ACTIVE_MARKER)
}

/// Write the readable turn-active marker under `base/.agent-doc/`.
pub fn write_turn_active_marker(base: &Path, pane: &str) -> Result<()> {
    write_turn_active_marker_at(base, pane, now_secs())
}

fn write_turn_active_marker_at(base: &Path, pane: &str, written_at: u64) -> Result<()> {
    let path = marker_path(base);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let marker = TurnActiveMarker {
        pane: pane.to_string(),
        written_at,
    };
    let json = serde_json::to_string_pretty(&marker).context("serialize turn-active marker")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Remove the readable turn-active marker (turn idle / superseded). Absent is OK.
pub fn clear_turn_active_marker(base: &Path) -> Result<()> {
    let path = marker_path(base);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("remove {}", path.display())),
    }
}

/// Read the turn-active marker if it exists and is not expired. A marker older
/// than [`TURN_ACTIVE_TTL_SECS`] (relative to `now`) is treated as absent so a
/// missed `idle` hook self-heals instead of wedging the session busy.
pub fn read_turn_active_marker_at(base: &Path, now: u64) -> Option<TurnActiveMarker> {
    let path = marker_path(base);
    let content = std::fs::read_to_string(&path).ok()?;
    let marker: TurnActiveMarker = serde_json::from_str(&content).ok()?;
    if now.saturating_sub(marker.written_at) >= TURN_ACTIVE_TTL_SECS {
        return None;
    }
    Some(marker)
}

/// True when a non-expired turn-active marker is present under `base`.
pub fn turn_active(base: &Path) -> bool {
    read_turn_active_marker_at(base, now_secs()).is_some()
}

/// Title to set for a turn state. `active` → the busy title; `idle` → empty, so
/// the pane returns to its default border title.
pub fn pane_title_for_state(active: bool) -> &'static str {
    if active { TURN_ACTIVE_PANE_TITLE } else { "" }
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

    // Also maintain the readable turn-state marker so route/supervisor can tell
    // the agent is mid-turn (the bridge to a future hard busy-lease). The hook
    // runs in the agent's CWD = project root; if there is no `.agent-doc`
    // ancestor, skip the marker (the pane title still updated). Best-effort —
    // never fail the turn.
    if let Some(base) = resolve_marker_base() {
        let result = if active {
            write_turn_active_marker(&base, &pane)
        } else {
            clear_turn_active_marker(&base)
        };
        if let Err(e) = result {
            eprintln!("[turn-status] warning: failed to update turn-active marker: {e:#}");
        }
    }
    Ok(())
}

/// Resolve the project root for the turn-active marker from the current working
/// directory (the harness hook runs there). `None` when there is no
/// `.agent-doc` ancestor.
fn resolve_marker_base() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    crate::fs_util::find_project_root(&cwd)
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

    #[test]
    fn turn_active_marker_write_read_clear_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();

        assert!(
            read_turn_active_marker_at(base, 1000).is_none(),
            "no marker before active"
        );

        write_turn_active_marker_at(base, "%7", 1000).unwrap();
        let marker = read_turn_active_marker_at(base, 1000).expect("present after write");
        assert_eq!(marker.pane, "%7");
        assert_eq!(marker.written_at, 1000);

        clear_turn_active_marker(base).unwrap();
        assert!(
            read_turn_active_marker_at(base, 1000).is_none(),
            "absent after clear"
        );
        // Clearing an absent marker is a no-op, not an error.
        clear_turn_active_marker(base).unwrap();
    }

    #[test]
    fn turn_active_marker_self_expires_after_ttl() {
        // A missed `idle`/Stop hook must not wedge the session busy forever: a
        // marker older than the TTL reads as absent (idle).
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();

        write_turn_active_marker_at(base, "%7", 1000).unwrap();
        // Just inside the window → still active.
        assert!(read_turn_active_marker_at(base, 1000 + TURN_ACTIVE_TTL_SECS - 1).is_some());
        // At/after the window → expired, treated as idle.
        assert!(read_turn_active_marker_at(base, 1000 + TURN_ACTIVE_TTL_SECS).is_none());
    }
}
