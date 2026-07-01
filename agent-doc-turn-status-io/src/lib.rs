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

use agent_doc_turn::turn_status::{
    STALE_SUPERVISOR_MARKER, TURN_ACTIVE_MARKER, TurnActiveMarker, pane_title_for_status,
    turn_active_marker_is_fresh, turn_active_marker_matches_pane,
};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Read the turn-active marker if it exists and is not expired. An expired
/// marker is treated as absent so a missed `idle` hook self-heals instead of
/// wedging the session busy.
pub fn read_turn_active_marker_at(base: &Path, now: u64) -> Option<TurnActiveMarker> {
    let path = marker_path(base);
    let content = std::fs::read_to_string(&path).ok()?;
    let marker: TurnActiveMarker = serde_json::from_str(&content).ok()?;
    if !turn_active_marker_is_fresh(&marker, now) {
        return None;
    }
    Some(marker)
}

/// Read the non-expired turn-active marker under `base`, if present.
pub fn read_turn_active_marker(base: &Path) -> Option<TurnActiveMarker> {
    read_turn_active_marker_at(base, now_secs())
}

/// True when a non-expired turn-active marker is present under `base`.
pub fn turn_active(base: &Path) -> bool {
    read_turn_active_marker(base).is_some()
}

/// True when the non-expired marker belongs to `pane`.
pub fn turn_active_for_pane(base: &Path, pane: &str) -> bool {
    read_turn_active_marker(base)
        .is_some_and(|marker| turn_active_marker_matches_pane(&marker, pane))
}

fn stale_marker_path(base: &Path) -> PathBuf {
    base.join(STALE_SUPERVISOR_MARKER)
}

/// Publish the stale-supervisor flag as an on-disk marker under `base/.agent-doc/`
/// (`#suptmuxstale`). `stale = true` writes the marker; `stale = false` removes it.
/// Best-effort cross-process channel for the supervisor → turn-status hook. Idempotent.
pub fn set_supervisor_stale_marker(base: &Path, stale: bool) -> Result<()> {
    let path = stale_marker_path(base);
    if stale {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        std::fs::write(&path, "").with_context(|| format!("write {}", path.display()))?;
        Ok(())
    } else {
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err).with_context(|| format!("remove {}", path.display())),
        }
    }
}

/// True when the stale-supervisor marker is present under `base` (`#suptmuxstale`).
/// Read-only display probe — absent / unreadable reads as fresh (not stale).
pub fn supervisor_stale(base: &Path) -> bool {
    stale_marker_path(base).exists()
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
    // `#suptmuxstale` — decorate the pane title with a stale-supervisor warning when
    // the route-owned supervisor has published its `binary_stale` probe on disk.
    // Read-only display; absent/unreadable marker reads as fresh.
    let stale = resolve_marker_base()
        .map(|base| supervisor_stale(&base))
        .unwrap_or(false);
    let title = pane_title_for_status(active, stale);
    let tmux = tmux_router::Tmux::default_server();
    if let Err(e) = tmux
        .cmd()
        .args(["select-pane", "-t", &pane, "-T", &title])
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
    agent_doc_fs::find_project_root(&cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn supervisor_stale_marker_write_read_clear_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();

        assert!(!supervisor_stale(base), "fresh before any marker");

        set_supervisor_stale_marker(base, true).unwrap();
        assert!(supervisor_stale(base), "stale after marker write");

        set_supervisor_stale_marker(base, false).unwrap();
        assert!(!supervisor_stale(base), "fresh after marker clear");
        // Clearing an absent marker is a no-op, not an error.
        set_supervisor_stale_marker(base, false).unwrap();
    }

    #[test]
    fn turn_active_for_pane_matches_only_marker_pane() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();

        write_turn_active_marker_at(base, "%7", now_secs()).unwrap();

        assert!(turn_active_for_pane(base, "%7"));
        assert!(!turn_active_for_pane(base, "%8"));
    }
}
