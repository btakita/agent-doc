# Monsterrodholders Backlog Priority Queue Sync Plan

## Problem

`tasks/monsterrodholders.md` declares `agent:backlog priority queue`, but
active backlog items are not being added to `agent:queue`.

## Hypothesis

The backlog-to-queue sync path is either skipping the document because of
ownership/session state, misclassifying the backlog items as already queued or
undrainable, or failing to persist queue convergence after detecting the
`priority queue` attribute.

## Implementation

1. Reproduce with a managed document containing `agent:backlog priority queue`
   and unqueued active backlog items.
2. Trace the queue-sync decision: document ownership, queue activity,
   drainability classification, existing queue-head de-duplication, and final
   persistence path.
3. Add diagnostics that name each skipped backlog id and the exact skip reason.
4. Fix the sync path so eligible backlog ids materialize as `do [#id]` or the
   current canonical queue shorthand and persist through session-check.

## Acceptance

- Active eligible backlog items in `tasks/monsterrodholders.md` appear in
  `agent:queue` when the backlog component has `priority queue`.
- Skipped backlog items have explicit diagnostic reasons.
- Queue convergence persists to the document and snapshot.
- Focused regression coverage plus `make check` pass before install/push.
