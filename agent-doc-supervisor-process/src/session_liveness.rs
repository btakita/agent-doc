//! Agent-foreground session liveness (`#adsessreap1` / `#adsessreap2`).
//!
//! Historically every reaper measured a session binding's liveness by proxies
//! that *outlive the agent process*: tmux `#{pane_dead}=0` (a pane that dropped
//! `claude → zsh` still reports alive), the recorded launcher/supervisor pid, or
//! an open editor session-log. A `claude → zsh` degradation therefore stranded
//! the `sessions.json` binding — every reaper treated the doc as alive even
//! though no agent owned it, forcing a manual `agent-doc claim --force`.
//!
//! The fix ties record reaping to the thing whose death we actually care about —
//! the **agent process** — via `#{pane_current_command}`. A pane that is alive
//! but whose foreground command is a bare interactive shell no longer owns a live
//! agent, so its stale binding is reapable.
//!
//! This predicate is used **only** to reap a stale *record*. It must never gate a
//! path that *terminates* an agent: the "never kill a maybe-live session" safety
//! bias stays intact for termination. The bare-shell allowlist
//! (`pane_current_command_is_bare_shell`) is deliberately conservative — an
//! unknown/foreign foreground command is treated as still-owning (false-alive over
//! false-dead) so we never reap a live session whose shell we failed to recognize.

use tmux_router::Tmux;

/// Pure liveness predicate over already-sampled signals.
///
/// Returns `true` when the pane should be treated as still owning a live agent
/// (i.e. must **not** be reaped). Returns `false` only when the pane is dead, or
/// alive but running a *known* bare interactive shell as its foreground command.
///
/// - `pane_alive`: tmux `#{pane_dead}=0`.
/// - `current_command`: tmux `#{pane_current_command}`, or `None` when it could
///   not be sampled. A missing sample is treated conservatively as still-owning,
///   so a transient read failure never triggers a false reap.
pub fn pane_owns_live_agent_from_signals(pane_alive: bool, current_command: Option<&str>) -> bool {
    if !pane_alive {
        return false;
    }
    match current_command {
        Some(cmd) => !agent_doc_tmux::pane_current_command_is_bare_shell(cmd.trim()),
        None => true,
    }
}

/// Reap predicate: does `pane` still own a live agent process?
///
/// Combines tmux `#{pane_dead}` with `#{pane_current_command}` so a pane that
/// degraded `claude → zsh` is recognized as ownerless even though it is still
/// alive. Use this in place of a bare `pane_alive` check on record-reaping paths
/// (stale actor close, stale claim removal, dead-entry prune, session-log
/// re-ownership). Never use it on a path that terminates an agent.
pub fn pane_owns_live_agent(tmux: &Tmux, pane: &str) -> bool {
    if !tmux.pane_alive(pane) {
        return false;
    }
    let current_command = agent_doc_tmux_io::target_current_command(tmux, pane);
    pane_owns_live_agent_from_signals(true, current_command.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_pane_never_owns_agent() {
        assert!(!pane_owns_live_agent_from_signals(false, Some("claude")));
        assert!(!pane_owns_live_agent_from_signals(false, None));
        assert!(!pane_owns_live_agent_from_signals(false, Some("zsh")));
    }

    #[test]
    fn alive_bare_shell_pane_is_ownerless() {
        // The exact `lazily-rs.md`/`%3` scenario: claude exited to a bare shell.
        for shell in [
            "zsh", "-zsh", "bash", "-bash", "sh", "fish", "dash", "ksh", "tcsh", "csh",
        ] {
            assert!(
                !pane_owns_live_agent_from_signals(true, Some(shell)),
                "bare shell {shell} should be reapable"
            );
        }
    }

    #[test]
    fn alive_agent_pane_is_never_reaped() {
        // Guard test: a genuinely-live agent pane must survive every phase.
        for cmd in ["claude", "codex", "node", "agent-doc"] {
            assert!(
                pane_owns_live_agent_from_signals(true, Some(cmd)),
                "agent command {cmd} must be kept alive"
            );
        }
    }

    #[test]
    fn unknown_foreground_command_is_conservatively_kept() {
        // False-alive over false-dead: never reap a foreground command we do not
        // recognize as a bare shell.
        assert!(pane_owns_live_agent_from_signals(true, Some("vim")));
        assert!(pane_owns_live_agent_from_signals(true, Some("python")));
        assert!(pane_owns_live_agent_from_signals(true, Some("ssh")));
    }

    #[test]
    fn missing_current_command_sample_is_kept() {
        // A transient `#{pane_current_command}` read failure must not reap a live
        // session.
        assert!(pane_owns_live_agent_from_signals(true, None));
    }

    #[test]
    fn surrounding_whitespace_does_not_defeat_shell_detection() {
        assert!(!pane_owns_live_agent_from_signals(true, Some("  zsh  ")));
    }

    /// Live-tmux regression for the exact `lazily-rs.md`/`%3` scenario: a pane
    /// whose agent exited to a bare shell is alive but ownerless, and a pane
    /// running a live non-shell foreground process is still owned.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn pane_owns_live_agent_tracks_foreground_command() {
        use std::time::Duration;
        use tmux_router::IsolatedTmux;

        let iso = IsolatedTmux::new("session-liveness-fg");
        let cwd = std::env::current_dir().unwrap();
        let pane = iso.new_session("liveness", &cwd).unwrap();

        // A freshly-spawned pane is a bare interactive shell — the `claude → zsh`
        // end state — so it owns no agent and its record is reapable.
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !pane_owns_live_agent(&iso, &pane),
            "bare-shell pane must not be treated as owning a live agent"
        );

        // Replace the shell foreground with a live non-shell process; the pane is
        // now conservatively treated as still-owning (never reaped). Poll to ride
        // past transient shell-startup commands before the `exec` takes over.
        iso.send_keys(&pane, "exec sleep 300").unwrap();
        let mut owned = false;
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(200));
            if pane_owns_live_agent(&iso, &pane) {
                owned = true;
                break;
            }
        }
        assert!(
            owned,
            "pane with a live non-shell foreground process must be kept"
        );

        // A pane tmux can no longer address is dead and reapable.
        assert!(
            !pane_owns_live_agent(&iso, "%999999"),
            "nonexistent pane must not be treated as owning a live agent"
        );
    }
}
