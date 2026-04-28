# Plan — diagnose `agent-doc-bugs2.md` tmux session crash / orphan state

**Pending:** `#bugs2tmux`
**Status:** Draft

## Problem

The last tmux-backed `agent-doc` session for `tasks/agent-doc/agent-doc-bugs2.md` was reported as crashed, but the persisted evidence does not yet cleanly distinguish between:

- an actual Codex child or supervisor crash
- a tmux pane replacement or detachment that left the user looking at the wrong shell
- an orphaned `agent-doc` pane that remained registered without recording a clean `codex_exit` / `session_end`

Current evidence from the local session state on 2026-04-28:

- document session UUID: `b26b9957-8de6-4eae-bb98-5d7fe4d6b781`
- session log: `.agent-doc/logs/b26b9957-8de6-4eae-bb98-5d7fe4d6b781.log`
- the latest log tail ends with `session_start ... pane=%275` followed by `codex_start mode=fresh restart_count=0`
- that same tail does **not** record `codex_exit`, `restart_eval`, `user_quit*`, or `session_end`
- tmux still reports pane `%275` as alive with `cmd=agent-doc`

That combination means the current failure is not just “pane died”; it may be a stuck or mis-routed live session whose observable state diverged from what the user saw.

## Goal

Make tmux-backed `agent-doc` sessions fail visibly and diagnosably when the user-facing session disappears, hangs, or gets orphaned, instead of leaving ambiguous “it crashed” states with partial liveness.

## Desired Behavior

- if the active pane truly dies, `agent-doc` records the exact exit boundary and reason
- if the pane stays alive but the child harness is gone, stuck, or no longer owns the document, `agent-doc` surfaces that explicitly instead of looking healthy
- if route/sync swaps or detaches a pane, the claim/session record reflects the replacement deterministically
- rerunning `agent-doc` on the document either resumes the healthy live pane or fails closed with a precise diagnostic state

## Implementation Steps

- [ ] Reconstruct the exact final state for the `b26b9957` session from logs, tmux metadata, and claim/session state:
  - last supervisor log lines
  - current pane id / tmux target
  - registered owner for `agent-doc-bugs2.md`
  - whether a Codex child process is still attached beneath the live `agent-doc` supervisor
- [ ] Audit the supervisor lifecycle in `src/agent-doc/src/start.rs` for paths where the pane can remain alive without emitting `codex_exit`, `restart_eval`, or `session_end`
- [ ] Audit route/claim reconciliation in `src/agent-doc/src/route.rs`, `src/agent-doc/src/claim.rs`, and `src/agent-doc/src/sync.rs` for cases where the visible pane changes but ownership/log state does not
- [ ] Define a new invariant for “live but stranded” sessions:
  - a registered pane with no child progress and no clean closeout markers must not be treated as healthy indefinitely
  - the next route/preflight/session-check pass must classify that state precisely
- [ ] Add explicit provenance logging for this boundary:
  - pane id
  - tmux target
  - supervisor pid
  - child pid if present
  - last observed harness state
  - reason for resume, restart, replacement, or fail-closed outcome
- [ ] Decide and implement the user-facing recovery contract:
  - auto-resume if the pane/session is provably healthy but detached
  - fail closed if the pane is registered yet stranded or ambiguous
  - reclaim / restart only when the old owner is provably dead or invalid
- [ ] Add regression coverage for:
  - supervisor alive but child missing
  - pane replaced while claim still points at the old owner
  - interrupted session with no terminal exit markers
  - rerun on a stranded pane producing a precise diagnostic instead of silent reuse
- [ ] Verify against a real tmux-backed run on `tasks/agent-doc/agent-doc-bugs2.md` before closing the item

## Files Likely To Change

- `src/agent-doc/src/start.rs`
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/claim.rs`
- `src/agent-doc/src/sync.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/specs/supervisor.md`

## Exit Criteria

- the `agent-doc-bugs2.md` crash/orphan scenario can be classified from logs and live state without ambiguity
- a stranded live pane is no longer treated as healthy by default
- rerunning `agent-doc` after this failure either resumes correctly or fails closed with a concrete reason
