# Plan: Stashed pane dies silently mid-cycle (#stshdie)

## Problem

When sync stashes a tmux pane to make room for a focused file's pane, the stashed pane's Claude Code process can die silently. Because `remain-on-exit` is `off`, tmux immediately destroys the pane, erasing all diagnostic information (exit status, signal, last output). Sync only discovers the death later via "no alive pane" with zero provenance.

This is especially damaging when the stashed pane was mid-cycle (`preflight_started` or later) — the cycle is orphaned with no way to determine the cause of death.

## Repro

From `src/session-share/tasks/claudescore.md` on Apr 29, 2026:

| Time | Event |
|------|-------|
| 19:44:25 | Pane `%511` auto-started for claudescore.md |
| 19:46:34 | Last successful commit from session in `%511` (commit `1040a3d`) |
| 19:49:10 | `cycle-1777506550775` started (`preflight_started`) — routed trigger ran preflight |
| 19:50:57 | Sync stashed `%511` (user switched focus to claudescore-3.md, `%512` auto-started) |
| 19:51:36 | Focus returns to claudescore.md. `%511` is DEAD. "no alive pane". `%514` auto-started. |

**Session log** (`a1580e76`): 119 lines, ends abruptly at `23:51:24 UTC` mid-tool-use (Read of `news/2026-04-29/README.md`). No error event, no graceful shutdown entry. Last assistant action was reading a file — completely benign.

**System logs**: No OOM killer, no dmesg signal, no journalctl `claude` entries in the 19:44–19:52 window.

**Possible causes** (not determined):
- Claude Code process hit an internal crash (node.js unhandled exception) while in background
- tmux killed the pane during a stash window resize operation
- Claude Code's session timeout or API connection dropped while stashed (no keepalive)
- OS-level signal (SIGHUP?) during pane detach that the process didn't handle

## Root Cause Analysis

The root cause of the pane death is **unknown** — that's the bug. `remain-on-exit off` means tmux destroys the pane before any diagnostic can be captured. The 39-second window between stash and discovery is too short for most timeout mechanisms but long enough for a crash.

## Proposed Fix

### Phase 1: Preserve pane exit status

Set `remain-on-exit on` for agent-doc managed panes. When sync detects a dead pane:

1. Read the pane's exit status via `tmux display-message -t %ID -p '#{pane_dead_status}'`
2. Read the last N lines of pane output via `tmux capture-pane -t %ID -p -S -50`
3. Log both to the sync log with the file association
4. Only then kill the dead pane

This gives provenance for every pane death without changing the user-visible behavior.

### Phase 2: Stashed pane liveness monitoring

Add a lightweight liveness check to stashed panes during sync:

1. On each sync tick, check if stashed panes are still alive
2. If a stashed pane dies mid-cycle, immediately capture diagnostics and log the event
3. Update the cycle state file with the death provenance before the next auto-start

### Phase 3: Mid-cycle recovery provenance

When a stashed pane dies with an open cycle:

1. Record the cycle phase at death time in the cycle state file
2. Capture the last assistant message from the session log (if readable)
3. Pass this provenance to `recover` so it can include it in the repair commit message

## Related

- `#snpsync` — snapshot desync after this crash's recovery (downstream consequence)
- `#tmuxcrash` — broader tmux crash stability and provenance classification
- `#dupfx` — duplicate response when session-check false-positives after crash
