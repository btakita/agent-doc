# Fresh Queue Head Authority Plan

## Problem

During a `tasks/monsterrodholders.md` cycle, the operator typed fresh queue
heads while recovery was in progress. The agent misclassified those heads as
stale/noise, attempted to delete them manually, and then `agent-doc` recovery
reintroduced or conflicted with them through stale snapshot/CRDT state.

This is adjacent to `tasks/agent-doc/plan-monsterrodholders-queue-preservation.md`,
but the missing invariant is stronger: fresh operator queue edits are
authoritative prompts until a specific consume/strike/remove proof exists.

## Hypothesis

The current closeout and recovery paths expose enough low-level state
(`session-check` drift, stale snapshots, queue consume refusal, reset sidecars)
that agents infer queue ownership themselves. That lets an agent treat
recently typed operator heads as stale operational artifacts instead of
prompt-bearing user edits.

## Implementation

1. Add a SimWorld or integration repro with an active session document where
   the operator adds queue heads during a recovery/finalize interruption.
2. Assert that preflight/doctor/plan classify those heads as user-authored
   prompt-bearing changes, with wording that discourages "stale/noise"
   deletion unless proof exists.
3. Harden `queue consume`, `session-check`, and reset/finalize diagnostics so
   they name the safe next operation for each live queue head:
   consume free-text, mark a matching backlog item done/gated, or leave the
   head alone.
4. Add a recovery path for the case where a queue head is a correction about
   an open backlog id: the operator can acknowledge/edit the backlog text
   without being forced to mark the underlying work done.
5. Add logging for any queue head resurrection/removal that records base hash,
   source component, operation, and explicit proof.

## Acceptance

- Fresh operator queue heads typed during recovery are never described as
  stale/noise by agent-doc diagnostics unless an exact stale/noise predicate is
  proven.
- A correction queue head tied to an open backlog id can be acknowledged
  without closing the backlog item.
- `reset --from-current`, `finalize`, and `write --commit` do not resurrect old
  queue heads or erase fresh heads without explicit proof.
- Focused regression tests plus `make check` pass before install/push.
