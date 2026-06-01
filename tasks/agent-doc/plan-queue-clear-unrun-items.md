# Plan: queue heads must not be deleted without execution proof

## Problem

Live repro from `src/boost-client/tasks/monsterrodholders.md` on 2026-05-31:

1. The document had an active `agent:queue auto` with six backlog-backed heads:
   `#convqa-rerun`, `#hydroapproval`, `#nbapproval`, `#shopcachewatch`,
   `#shoplabelgate`, and `#accessorymargin`.
2. A same-pane/direct recovery path hit the recursive-dispatch guard.
3. The visible closeout refreshed only the conversion QA gate. It did not execute
   or close the six queued backlog items.
4. The queue was cleared back to just the preset and `queue_active: false`, while
   all six backlog items remained open.

Expected behavior: an active queue head is executable user intent. Agent-doc can
remove a queue line only when it has durable proof that the exact line was
consumed, completed, gated, struck as already done, or explicitly removed by a
newer user edit.

## Distinction From Related Bugs

- Related to `#queue-user-edit-overwrite`, but this case is not only preserving a
  newly added queue edit. It is about proving that deletion of existing runnable
  queue heads is valid.
- Related to `#codex-owned-pane-auto-queue-stuck`, because the same-pane guard
  path produced the bad state, but the invariant should protect every
  reset/repair/write/commit path.
- Related to `#queue-audit-partial-completion`, because partial progress on one
  head must not justify deleting unrelated unrun heads.

## Desired Behavior

For every queue closeout or maintenance mutation, agent-doc should produce a
per-line removal reason:

- `consumed`: the current response targeted and advanced this exact head.
- `done`: the referenced id is in live done/archive state.
- `gated`: the referenced backlog item was moved to a gated state by this cycle.
- `explicit_user_removal`: the visible queue deletion is newer than snapshot/HEAD
  and classified as user-authored.
- `stale_generated_residue`: the line is generated residue with a reproducible
  stale source.

If a queue head references an open backlog/review item and none of the above
proofs exists, deletion must fail closed. Recovery should preserve the queue body
and keep `queue_active: true`, or emit a precise repair instruction.

## Implementation Plan

1. Add queue-removal accounting in the queue maintenance layer:
   - compare preflight baseline, visible file, HEAD, and intended post-write
     queue bodies;
   - compute removed queue heads;
   - attach a removal reason to each removed line.
2. Extend cycle state with queue-removal proof:
   - consumed head id/free-text prompt;
   - ids marked done or gated by the cycle;
   - lines struck because done/archive already contains the id;
   - visible user queue edit hash/epoch when the user removed a line.
3. Add a session-check guard:
   - if HEAD after commit deleted a runnable queue head without proof, interrupt;
   - name each lost id and say whether the matching backlog/review item is still
     open.
4. Harden reset/repair/commit paths:
   - `reset --from-current --preserve-session` must not normalize an active queue
     to empty unless it records explicit user removal proof;
   - `agent-doc commit` must re-check queue deletion proof before committing a
     parent/submodule pointer.
5. Add diagnostics for same-pane recursive guard:
   - when dispatch cannot recurse, hand the queue head back to the owning pane or
     leave the queue unchanged;
   - never clear the queue as a fallback for "broad" or blocked items.

## Tests

- Fixture based on `monsterrodholders.md`: six queued `do [#id]` lines reference
  open backlog items. A closeout updates only status/review text and proposes an
  empty queue. Assert session-check fails and lists all six removed ids.
- Consumed-head regression: response targets `do [#convqa-rerun]` and marks it
  done or gated. Assert only that head may be removed and the remaining five stay
  queued.
- User-removal regression: visible editor state deletes two queue heads after
  baseline with a newer queue hash. Assert the deletion is classified as
  explicit user removal and preserved.
- Reset/repair regression: `reset --from-current --preserve-session` on an active
  queue with open backlog ids preserves the queue body and `queue_active: true`.

## Acceptance

- Agent-doc cannot commit a state where open backlog-backed queue heads disappear
  without a named proof reason.
- Same-pane recursive recovery leaves unexecuted queue heads runnable.
- The Monster Rod Holders repro would have stopped before the six queue items
  were cleared, or would have restored them automatically with an explicit
  diagnostic.
