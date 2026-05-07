# Plan — compact does not clear the session-accretion guard

**Pending:** `#cmpclr` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

`agent-doc` can tell the user to run `agent-doc compact <FILE> --commit`, the
compact can succeed, and the very next `preflight` / `plan` cycle can still
fail closed on the same session-accretion blocker.

The current contract effectively traps the document in a loop:

1. `preflight` or `plan` reports a block-level session-accretion reason and
   tells the user to compact.
2. `agent-doc compact ... --commit` runs successfully and rewrites the
   document/exchange state.
3. the next ordinary turn still calls `session_accretion::inspect(file)`,
   sees the same hot recent-cycle counters from the local log window, and
   blocks again before any new substantive work can happen.

That means compaction is not acting as a real recovery step for closeout-churn
driven accretion. The tool is telling the user to do something that does not
actually satisfy the blocker.

## Repro / Evidence

The May 6, 2026 `tasks/software/tsift.md` session exposed the exact loop:

- `preflight` auto-compacted the document multiple times with messages such as
  `session accretion auto-compact due to 6 recent no-op closeouts`
- the follow-up `plan` call still failed closed with blockers like:
  - `document closed 11 cycles in the last 30 minutes (6 no-op closeouts)`
  - later `13 cycles in the last 30 minutes (7 no-op closeouts)`
  - later `16 cycles in the last 30 minutes (8 no-op closeouts)`
- each retry increased the same recent-cycle metrics, which made the lockout
  worse even though the exchange had already been compacted

The code path explains why:

- `preflight.rs` step `2c` runs `compact::run(...)` from
  `maybe_auto_compact_for_session_accretion(...)`
- `plan.rs` then independently calls `session_accretion::inspect(file)?`
- `plan.rs` only suppresses the blocker when
  `exchange_compaction_requested == true`, which is tied to the current diff
  asking for compaction, not to a compaction that already completed on the
  immediately preceding turn
- `session_accretion.rs` computes `recent_committed_cycles` and
  `recent_noop_closeouts` from the recent log window, so a successful compact
  does not by itself cool the metric

## Goal

Make successful compaction a real recovery path for closeout-churn accretion so
the next turn can proceed, while still preserving the fail-closed behavior for
truly unhealthy sessions.

## Desired Behavior

- If `agent-doc compact <FILE> --commit` just completed successfully for this
  document, the next `preflight` / `plan` cycle should not immediately re-raise
  the same closeout-churn blocker unless a new large-turn risk has actually
  reappeared.
- Auto-compaction during `preflight` should either:
  - satisfy the compact/recovery requirement for the next dispatch phase, or
  - emit a stronger handoff that explains why compaction alone cannot clear the
    current blocker.
- Repeated retries after a successful compact must not make the lockout worse
  just because the recovery attempt itself adds more committed/no-op cycles to
  the recent window.

## Investigation / Implementation Plan

- [ ] Reproduce the loop with a focused regression fixture:
  - seed a document with block-level closeout churn
  - run the compaction path
  - prove that the next `plan` currently still blocks on the same churn family
- [ ] Decide the recovery contract:
  - whether successful compact should set explicit state that temporarily
    satisfies the accretion blocker
  - or whether closeout-churn counters should decay / exclude the compact
    recovery window
  - or whether `plan` should recognize "compaction already completed for this
    document" instead of only "compaction requested in this diff"
- [ ] Audit the interaction between:
  - `session_accretion::inspect`
  - `preflight::maybe_auto_compact_for_session_accretion`
  - `plan`'s `exchange_compaction_requested` gate
  - any cycle/ops-log accounting that makes recovery retries increment the same
    hot counters
- [ ] Pin the invariant:
  - a successful compact must either lower/suppress the relevant blocker for at
    least one follow-up turn, or the tool must stop recommending compact as the
    recovery step for that blocker family
- [ ] Add regressions for:
  - successful compact followed by one ordinary turn does not immediately
    re-block on the same closeout-churn reason
  - auto-compact in `preflight` and explicit `agent-doc compact --commit`
    both satisfy the same recovery contract
  - truly restart-heavy or still-growing sessions can still fail closed after
    compact when the unhealthy signal is independent of exchange size

## Likely Files

- `src/agent-doc/src/session_accretion.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/plan.rs`
- `src/agent-doc/src/compact.rs`
- `src/agent-doc/tests/`
- `src/agent-doc/SPEC.md`
- `src/agent-doc/README.md`
- `src/agent-doc/runbooks/planning-dispatch.md`

## Exit Criteria

- Running `agent-doc compact <FILE> --commit` is a meaningful recovery step for
  the accretion blocker it recommends.
- The next ordinary turn after a successful compact can proceed unless a
  distinct unhealthy signal still justifies the block.
- Regression coverage proves the tool no longer traps the user in a
  compact-then-block loop on the same document.
