# Plan: completed queue residue regression

## Problem

Completed queue items can still remain visible in `agent:queue` after a queue
turn is finished. This regresses or bypasses the prior
`#drained-done-queue-clear` fix, which expected fully resolved queue runs to
clear the queue body, strip `auto`, and persist `queue_active: false`.

The new report is narrower than inactive context pollution: the user is seeing
items that should be completed still present in the queue itself, making the
document look executable even after the work has already closed.

### Fresh repro — 2026-06-23

`tasks/professional/equityfundingsource.md` still shows a live free-text queue
item:

```md
<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- deploy
<!-- /agent:queue -->
```

That `deploy` head was answered in a committed cycle that reported successful
deploy, `session-check ok`, and no queue continuation. A later EFS queue cycle
also committed successfully, but the old answered `deploy` head remained live
instead of being struck/removed. This is a regression against the `#ftstrike` /
`#qheadstrike` contract: an answered free-text head must not reappear as ready
work after a proven closeout.

## Likely Gap

The existing tests cover finalize/preflight drain behavior for some happy-path
and no-diff cases, but a live route/IDE/IPC or queue-continuation path may leave
the editor buffer, snapshot, or HEAD with stale queue body state after the
completion ledger has advanced.

Likely failure modes:

- queue drain mutates disk but live IPC convergence preserves the old queue
- session-check sees committed closeout but does not reject completed queue
  residue
- `agent-doc run` continuation records completed ids but skips the final drain
  cleanup before launching or stopping
- repair/preflight normalizes `queue_active` while leaving completed prompt rows
  in place
- recovery/finalize can consume the current new prompt while preserving a stale,
  already-answered free-text head from an older snapshot or live editor buffer

## Desired Behavior

When a queue item is completed:

1. The completed head is removed if the queue is drained.
2. Later pending queue items remain only when they are still executable.
3. Stale completed rows are not left struck through in an inactive/drained
   queue.
4. Snapshot, live editor buffer, and HEAD converge on the same queue body.
5. `session-check` fails closed when a committed document still contains
   completed queue residue that the binary can prove should have drained.

## Implementation Plan

1. Add a regression for a completed queue item that remains in `agent:queue`
   after a successful closeout, including the route/IPC convergence path if
   SimWorld can model it.
2. Extend session-check with a committed-state guard for provable completed
   queue residue.
3. Audit finalize, write --commit, preflight queue maintenance, and run
   continuation for inconsistent drain ordering.
4. Ensure IPC queue convergence sends the fully drained queue body after
   closeout, not only the `auto` attribute or `queue_active` state.
5. Add a targeted regression that models a prior answered free-text head
   (`deploy`) plus a later manually-added free-text head in the same queue:
   the later closeout must not revive the older answered head.
6. Add live verification against the reported session document before marking
   the fix complete.

## Verification

- focused queue-drain/session-check regression
- `cargo test -p agent-doc-orchestration queue`
- `make check`
- live replay of a completed queue turn through the same route used in the
  report
