# Monsterrodholders Queue Preservation Plan

## Problem

During an active `tasks/monsterrodholders.md` session, user-added queue items
were deleted while the turn was running.

## Hypothesis

A queue convergence or write-closeout path is treating a stale queue snapshot
as authoritative over live user queue edits. The fix should preserve
concurrent queue additions unless there is explicit consume/strike/remove
proof for that exact head.

## Implementation

1. Reproduce the active-turn queue edit with a SimWorld or integration test:
   start from an active queue, add live queue heads concurrently, then run the
   closeout/convergence path that previously deleted them.
2. Audit queue convergence, queue consume, and CRDT merge paths for stale
   snapshot replacement of `agent:queue`.
3. Add a proof rule that user-added queue nodes survive unless consumed,
   struck, or explicitly removed by a matching queue operation.
4. Add diagnostics that name deleted queue ids/free-text heads and the proof
   source authorizing deletion.

## Acceptance

- Concurrently added queue items in `tasks/monsterrodholders.md` survive an
  active turn closeout.
- Queue deletion requires explicit consume/strike/remove proof.
- Logs identify any queue node removed by convergence.
- Focused regression tests plus `make check` pass before install/push.
