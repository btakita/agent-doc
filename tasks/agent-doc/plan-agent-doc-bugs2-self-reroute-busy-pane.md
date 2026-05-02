# Plan — fix same-document `Run Agent Doc` reroutes that still fail closed after the busy-pane recovery ladder

**Pending:** `#selfrt` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

The latest JetBrains `Run Agent Doc` failure for `tasks/agent-doc/agent-doc-bugs2.md` shows a same-document routed reopen that still fails after every currently-bounded recovery step:

- route recovers the live pane `%360` as authoritative for the same document
- it waits for an idle Codex prompt and sees the pane stay busy
- it applies scoped `agent-doc fix tasks/agent-doc/agent-doc-bugs2.md`
- it restarts the healthy supervisor fresh once
- it sends one interrupt recovery before the final reroute attempt
- it still fails closed with `registered pane %360 ... is still not showing an idle codex prompt`

That differs from the older sibling-repo `monsterrodholders.md` and `claudescore-3.md` replays. This repro stays inside the owning repo and still never transitions from "authoritative busy pane" to a deterministic next state.

## Goal

Make same-document reroutes for an authoritative live Codex pane do one of these deterministically:

1. recover and start a new document cycle
2. abandon the current pane and hand off to a replacement pane
3. fail once with a stage-specific reason that says why the authoritative pane could not be reused

## Repro Evidence To Preserve

- pane `%360` stayed authoritative for `tasks/agent-doc/agent-doc-bugs2.md` through scoped fix and fresh restart
- ready-wait never produced an idle Codex prompt, first at `Context 0% us`, then again at `Context 85% u`
- the current ladder treated that as one more generic busy-pane refusal instead of exposing which transition failed:
  - readiness never recovered
  - fresh restart did not hand off
  - interrupt did not clear the blocked substate

## Implementation Steps

- [ ] Reconstruct the exact `%360` reroute sequence from ops logs, supervisor state, and any preserved pane capture so each recovery stage has explicit before/after evidence.
- [ ] Audit the same-document busy-pane ladder in `src/agent-doc/src/route.rs`:
  - scoped fix
  - supervisor-backed fresh restart
  - handoff wait
  - bounded interrupt retry
  - final reroute
- [ ] Prove whether same-document reroutes are incorrectly allowed to keep targeting the original authoritative pane after a failed fresh restart when no real readiness transition occurred.
- [ ] Tighten the readiness classifier in `src/agent-doc/src/harness.rs` so the route layer can distinguish:
  - still booting
  - interactive shell substate
  - live Codex work in progress
  - stuck-but-authoritative pane that should be abandoned
- [ ] Decide whether a same-document authoritative pane that survives fix + fresh restart + interrupt should be marked unreusable for the current route attempt instead of falling into one more idle-prompt wait loop.
- [ ] Add stage-specific diagnostics so JetBrains and tmux surfaces say whether the failure was:
  - no idle prompt after fix
  - no handoff after fresh restart
  - no recovery after interrupt
- [ ] Add regressions for:
  - same-document authoritative pane that never leaves busy state
  - fresh restart that leaves ownership unchanged with no readiness improvement
  - bounded interrupt that still leaves the pane blocked
  - deterministic fail-closed diagnostics for each of those stages

## Files Likely To Change

- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/harness.rs`
- `src/agent-doc/src/start.rs`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/specs/supervisor.md`

## Exit Criteria

- The `agent-doc-bugs2.md` same-document reroute no longer exhausts the full recovery ladder only to end in the generic idle-prompt refusal.
- Route either recovers the document cycle or fails once with a stage-specific explanation tied to the recovery step that stalled.
- JetBrains `Run Agent Doc` reports enough state to tell whether the pane should be retried, replaced, or left alone.
