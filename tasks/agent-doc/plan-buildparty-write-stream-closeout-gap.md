# Plan — BuildParty synthetic `write_stream` closeout gap

**Pending:** `#devmdcmt` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

`agent-doc` previously left
`src/session-share/tasks/buildparty-investor-demo/dev.md` in a binary-owned but
nonterminal closeout state after the response had already landed in the
document.

This is narrower than the older "visible response but no commit proof" family.
In this repro, the closeout path already recorded a synthetic cycle and a
`write_stream` transition. The gap is that the response write landed first, but
no terminal commit/finalize step followed until a later explicit
`agent-doc commit`.

## Historical Evidence

The preserved May 2, 2026 evidence for the `dev.md` session is specific:

- `src/session-share/.agent-doc/logs/586ffeea-ca3a-490b-80bd-883dd0e95673.log`
  records:
  - `[1777883041] document_cycle phase=response_captured cycle=synthetic-1777883041000 event=response_captured capture_id=synthetic-1777883041000`
  - `[1777883041] document_cycle phase=write_applied cycle=synthetic-1777883041000 event=write_stream capture_id=synthetic-1777883041000`
  - `[1777883128] document_cycle phase=committed cycle=synthetic-1777883041000 event=commit_success capture_id=synthetic-1777883041000`
- `src/session-share/.agent-doc/captures/fbdde1a9f60dab3cf06c00e1e747fb607d46afdad89003acd3745214fff0dd30/synthetic-1777883041000.json`
  shows:
  - `captured_at = 1777883041`
  - `write_applied_at = 1777883041`
  - `committed_at = 1777883128`
  - a full visible response body already preserved in the capture record

That proves the response landed through the CRDT/template write path first and
only crossed the terminal commit boundary later.

## Why This Is Separate From `#rspcmt`

- `#rspcmt` is the broader "response was written but not committed" family.
- `#devmdcmt` is specifically about the synthetic `write_stream` path:
  - the cycle is already binary-owned
  - `response_captured` and `write_applied` are already recorded
  - the missing piece is the automatic terminal closeout after that write
- The comparison matters because the fix may live in different code:
  - `#rspcmt` can involve patchback provenance, owning-root detection, or
    generic finalize/repair success reporting
  - `#devmdcmt` may be a narrower CRDT/socket/sidecar closeout handoff bug

## Current Baseline

Later `dev.md` cycles on May 5, 2026 show the expected healthy shape:

- `cycle-1778001570532`
- `cycle-1778002260629`
- `cycle-1778003145856`

For those cycles, the same session log records `response_captured` and
`commit_success` in the same second, and the corresponding capture files record
`committed_at` immediately. That makes the synthetic `write_stream` gap a
historical but still distinct regression worth root-causing.

## Goal

Ensure that once the template/CRDT write path records `write_applied` for a
response that already landed in the document, the same closeout path must
either:

1. finish the terminal commit/finalize boundary before returning success, or
2. fail closed with explicit replayable state and a concrete phase-labeled
   reason.

The system must never leave a synthetic `write_stream` cycle as the effective
end of a "completed" turn.

## Investigation Questions

- Which exact path emits `event=write_stream` for synthetic cycles in template
  + CRDT mode?
- After that write lands, what is supposed to invoke the terminal
  `commit_success` or `commit_already_current` transition?
- Did the later explicit `agent-doc commit` simply finish an otherwise-correct
  interrupted closeout, or did it paper over a missing automatic handoff?
- Why did `session-check` still see the turn as unfinished before that later
  commit even though the response was already visible?
- Does the gap depend on:
  - sidecar normalization fallback
  - socket/IPC delivery
  - backlog mutation state
  - boundary cleanup
  - synthetic-cycle creation when no normal run/finalize cycle is open

## Implementation Steps

- [ ] Reconstruct the exact May 2 `dev.md` closeout chain from local evidence:
  - session log timeline
  - capture file timeline
  - any matching hook payloads under
    `src/session-share/.agent-doc/hooks/post_write/` and
    `src/session-share/.agent-doc/hooks/post_commit/`
  - whether the terminal `commit_success` came from the original write path or
    only from a later explicit `agent-doc commit`
- [ ] Trace the synthetic `write_stream` code path in:
  - `src/agent-doc/src/write.rs`
  - `src/agent-doc/src/git.rs`
  - `src/agent-doc/src/cycle_state.rs`
  - `src/agent-doc/src/session_check.rs`
  - `src/agent-doc/src/repair.rs`
  - any IPC/socket/sidecar path that can return after `write_applied`
- [ ] Compare this shape directly with `#rspcmt`:
  - what invariants should be shared
  - what extra guarantees synthetic `write_stream` should provide because the
    write is already binary-owned
- [ ] Define the invariant precisely:
  - synthetic `write_stream` is not a terminal success state
  - once the response lands, success requires a same-cycle terminal
    `commit_success` / `commit_already_current` proof or an explicit fail-closed
    interruption
- [ ] Audit the handoff between write completion and commit completion:
  - ensure no boundary/backlog/session-check branch can return success after
    `write_applied` without scheduling or executing the terminal closeout
  - ensure later bookkeeping cannot make the cycle look synthetic/open when the
    closeout should already have been terminal
- [ ] Add regression coverage for:
  - synthetic `write_stream` writes the response, then must automatically reach
    terminal commit
  - if commit cannot run, the command fails closed and preserves recovery state
  - `session-check` treats a lingering synthetic `write_applied` state as an
    open interrupted cycle, not a successful turn
  - the `dev.md` historical shape no longer requires a second explicit
    `agent-doc commit`

## Files Likely To Change

- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/git.rs`
- `src/agent-doc/src/cycle_state.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/tests/`
- `src/agent-doc/specs/07-closeout-commands.md`
- `src/agent-doc/SPEC.md`

## Exit Criteria

- A template/CRDT response that records synthetic `write_stream` cannot be left
  waiting for a later explicit `agent-doc commit`.
- The terminal commit/finalize step either happens in the same closeout path or
  the command fails closed with a concrete recovery message.
- `#devmdcmt` is covered by an executable regression that distinguishes it from
  the broader `#rspcmt` family.
