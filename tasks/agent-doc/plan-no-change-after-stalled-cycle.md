# Plan: no-change direct run after stalled cycle history

**Pending:** `#nochange-after-stall` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Base item planned/shipped separately; `#nochange-after-stall-breadth`
implemented 2026-06-02 in the session-check closeout guards.

## Report

Operator report from `src/boost-client/tasks/monsterrodholders.md` on 2026-06-02:

```text
The last run stalled. #agent-doc-bug
```

Immediate follow-up direct run from outside the managed pane:

```text
agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md
```

returned:

```text
[diff] no changes detected between snapshot and document (after comment stripping)
[run] Nothing changed since last run for .../tasks/monsterrodholders.md
```

Recent local evidence in `.agent-doc/logs/a9421282-fd96-4943-9af5-3561ed5cb799.log` for the same document shows repeated abandoned cycles:

```text
document_cycle phase=abandoned event=recursive_direct_invocation_blocked
recursive direct invocation would deadlock: `agent-doc .../monsterrodholders.md`
is running inside the Codex pane that already owns this document ...
```

The current `cycle_state` then records a committed repair/reap cycle, but the
operator-facing rerun only compared document content to the stored snapshot and
short-circuited as "nothing changed." That response loses the important context:
the previous run did not complete like a normal dispatch/response cycle.

## Relationship To Existing Work

This is adjacent to `#jbstalldrain` /
`tasks/agent-doc/plan-jb-run-agent-doc-stalled-closeout-drain.md`, but it covers
a different surface:

- `#jbstalldrain` targets JetBrains route progress while a run is waiting or
  blocked.
- This item targets a later CLI/direct-run no-change verdict that ignores recent
  stalled, abandoned, or recursive-owner cycle state.

It also relates to `#codex-owned-pane-auto-queue-stuck` and
`#codex-owned-pane-prompt-miss`, because the same recursive self-invocation guard
can prevent a child dispatch while leaving the user without a durable closeout
or an actionable next diagnostic.

## Goal

A direct `agent-doc <FILE>` run must not report plain "Nothing changed" when the
document's latest relevant cycle history indicates a stalled/abandoned run,
recursive self-invocation block, stale preflight, or unresolved consume/response
proof. It should surface the prior terminal state and the next recovery action.

## Fix Direction

1. Add a "last abnormal cycle" check before the no-change short-circuit.
   - Read the current document's `cycle_state` and recent ops/session log events.
   - Classify abnormal terminal states: `abandoned`, `recursive_direct_invocation_blocked`,
     stale `preflight_started`, cancelled/no-capture cycle, and committed repair
     cycles that only reaped bookkeeping.
   - Ignore old abnormal states once a later normal response+consume proof exists.

2. Define a typed no-change diagnostic.
   - For recursive owner-pane invocation, report that the managed owner already
     owns the document and name the pane/session/generation.
   - For abandoned/stale preflight, report the cycle id and recovery command.
   - For repair-only/reap-only closeout, report what was repaired and whether any
     active queue head remains unprocessed.

3. Gate the no-change short-circuit on durable proof.
   - Plain no-change is valid only if the latest relevant cycle has a committed
     assistant response or explicit consume/deferral proof, and no newer abnormal
     cycle exists for the document.
   - If queue heads changed only through repair/reap bookkeeping, require a
     consume proof for the prior active head before saying nothing changed.

4. Add session-check coverage.
   - A fixture with `document == snapshot` plus latest cycle
     `recursive_direct_invocation_blocked` must fail with the typed diagnostic.
   - A fixture with `document == snapshot` plus normal committed response/consume
     proof must still pass as no-change.
   - A fixture with repair-only completed-backlog reap and stale active queue-head
     metadata must name the stale state instead of reporting generic no-change.

5. Update route/JB surfaces only if needed.
   - Keep the primary logic binary-owned in the run/preflight path.
   - If route currently turns the same state into generic success/no-op, reuse the
     typed diagnostic from step 2 in plugin-visible output.

## Files Likely To Change

- `src/agent-doc/src/run.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/cycle_state.rs`
- `src/agent-doc/src/ops_log.rs`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/VERSIONS.md`

## Verification

- Reproduce with `src/boost-client/tasks/monsterrodholders.md` after an abandoned
  recursive direct invocation and confirm `agent-doc <FILE>` reports the prior
  abnormal cycle instead of plain no-change.
- Confirm a clean document with a normal committed response and no pending queue
  work still returns the existing no-change output.
- Confirm the diagnostic names the owner pane/session for recursive self-invocation
  and gives a copyable recovery command.
- Run the project verification suite and install the updated binary before live
  verification.

## Exit Criteria

- `agent-doc <FILE>` no longer hides the previous stalled/abandoned run behind
  a snapshot-only "Nothing changed" result.
- Operators get a typed explanation with the prior cycle id and recovery action.
- Normal no-change behavior remains unchanged for healthy completed sessions.

## 2026-06-02 Breadth Implementation

`#nochange-after-stall-breadth` extends the follow-up surface that the base
`run.rs` no-change classifier intentionally deferred to `session-check`:

- non-terminal stale-open cycles still interrupt even when document and snapshot
  bytes match;
- committed no-response repair/reap-only cycles interrupt when a recorded active
  `agent:queue` `do [#id]` head remains queued and open in `agent:backlog`
  without done/gate/reap/kept-open proof;
- healthy committed no-change states with no active queue head stay clean.

Implemented in `src/agent-doc/agent-doc-orchestration/src/session_check.rs`
with regression coverage, and documented in
`src/agent-doc/specs/07-closeout-commands.md`.
