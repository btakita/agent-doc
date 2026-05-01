# Plan — fix remaining routed Codex busy-substate and cycle-ack gaps

**Pending:** `#rbgap` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

The May 1, 2026 `Run Agent Doc` failures in `tasks/agent-doc/agent-doc-bugs2.md` show that routed Codex recovery is still failing across three related shapes even after the scoped-fix, fresh-restart, and bounded-interrupt work landed:

- `src/boost-client/tasks/monsterrodholders.md` still ends in `interactive shell reverse-i-search` after route applies `agent-doc fix`, keeps the pane authoritative, and retries the reopen.
- `src/session-share/tasks/claudescore-3.md` can accept the bare `agent-doc <FILE>` reopen and still miss the new-cycle acknowledgment window, then fall back into the same busy-session family on the next reroute.
- `tasks/agent-doc/agent-doc-bugs2.md` itself can recover the live pane, detect the same `reverse-i-search` blocker, and still fail closed after the final reroute attempt.

That means the remaining bug is no longer just one document-specific replay. The route stack still lacks a deterministic contract between:

1. dispatch-readiness classification for live Codex panes
2. bounded remediation when the pane is in an interactive shell substate
3. proof that the routed reopen was actually consumed and started a new document cycle

## Goal

Make same-document Codex reroutes either recover in one bounded path or fail once with a precise stage-specific reason:

- pane was not dispatch-ready
- reopen was sent but never consumed
- reopen was consumed but no new cycle started

## Repro Evidence To Preserve

- `monsterrodholders.md`: `%366` and `%365` both report `reverse-i-search` after the scoped-fix ladder, proving the current readiness/remediation path can keep retrying the wrong live pane state.
- `claudescore-3.md`: `%364` and `%372` show both halves of the gap:
  - route can send the reopen successfully and still never observe a new-cycle acknowledgment
  - later reroutes can still degrade back into the same busy-pane family
- `agent-doc-bugs2.md`: `%360` proves the same failure can happen in the owning repo, so this is not a sibling-repo-only environment quirk.

## Implementation Steps

- [ ] Reconstruct the three May 1 routed failures from logs, pane captures, and any persisted hook/session state so the exact state transition is visible for each stage.
- [ ] Audit the busy-pane readiness classifier in `src/agent-doc/src/harness.rs` and `src/agent-doc/src/route.rs`:
  - which terminal substates are treated as "busy but recoverable"
  - which should force an immediate fresh restart or fail-closed result
  - whether `reverse-i-search` should bypass the current scoped-fix ladder entirely
- [ ] Audit the same-document recovery ladder in `route.rs`:
  - scoped `agent-doc fix`
  - supervisor-backed fresh restart
  - bounded interrupt attempt
  - final reroute
  - ensure each step requires a real state transition instead of reusing the same blocked pane state
- [ ] Prove how route currently decides that a bare reopen was consumed:
  - tmux send acceptance
  - Codex hook advancement
  - document cycle-state change
  - identify where the current proof collapses distinct failures into the same timeout bucket
- [ ] Add a second-stage routed proof or clearer failure partitioning so "sent but not consumed" and "consumed but no cycle started" stop looking identical.
- [ ] Tighten operator-facing diagnostics so JetBrains/tmux reports which stage failed without requiring a full log scrape.
- [ ] Add regressions for:
  - live pane stuck in `reverse-i-search`
  - reopen accepted by tmux but never consumed by Codex
  - reopen consumed without a new document cycle
  - same-document reroute that must abandon the old pane and stay with the fresh replacement

## Files Likely To Change

- `src/agent-doc/src/harness.rs`
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/codex_hook.rs`
- `src/agent-doc/src/start.rs`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/specs/supervisor.md`

## Exit Criteria

- `Run Agent Doc` no longer spends a full recovery loop on panes that already prove a blocked interactive shell substate.
- Routed reopen failures clearly distinguish:
  - not dispatch-ready
  - not consumed
  - no cycle start
- Re-running the `monsterrodholders.md`, `claudescore-3.md`, and `agent-doc-bugs2.md` repros either succeeds end-to-end or fails once with deterministic stage-specific diagnostics.
