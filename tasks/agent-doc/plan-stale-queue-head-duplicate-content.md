# Stale Queue Head Duplicate Content Corruption Plan

## Problem

A stale `agent:queue` head can cause duplicate content to be merged back into
the session document, corrupting the document instead of cleanly consuming or
reconciling the stale head.

## Hypothesis

The queue convergence or write recovery path can apply a stale queue snapshot
after the live buffer has already advanced. When the stale head is treated as
authoritative, the merge may replay old exchange/queue content and duplicate
managed sections.

## Implementation

1. Build a regression fixture with a queue head that is stale relative to the
   live editor buffer and snapshot.
2. Exercise the preflight/write recovery path that currently replays duplicate
   content.
3. Add stale-head detection that refuses to replay a queue snapshot unless the
   live head still matches or a consume/strike proof exists.
4. Add diagnostics naming the stale head, live head, snapshot head, and chosen
   recovery action.

## Acceptance

- A stale queue head does not duplicate exchange, queue, backlog, or review
  content.
- Recovery either preserves the live head or cleanly strikes a proven-resolved
  stale id/free-text head.
- Diagnostics include stale-head identity and mismatch details.
- Focused regression coverage plus `make check` pass before install/push.
