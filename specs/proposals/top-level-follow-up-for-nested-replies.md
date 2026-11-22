# Proposal: Top-Level Follow-Up Rows For Nested Operator Replies

> **Status:** proposal, non-normative. Nothing here is implemented. Normative
> behavior stays in [`SPEC.md`](../../SPEC.md) and the numbered `specs/` files.
> It becomes normative only when its queue item lands spec + tests.

## Request

> If the operator responds in a subtree, break out a top-level follow-up
> tree...chain top-level tree together.

Today an operator reply typed under a response tree is admitted where it
stands. It becomes a nested leaf whose work identity is its nest vector
(`root: parent: leaf`, `#nestsubmit`, `#claimdispatchidentity`). Each round of
replies nests one level deeper.

## Why this is not implemented yet

- **Identity:** a nested leaf's claim, lineage and `#reportedrelease`
  receipts are keyed on its nest vector. Moving the line changes that key.
  - A claimed or running attempt would turn `Lost` and be dispatched again.
  - A reported receipt would stop matching its row.
- **Operator text authority:** the move deletes an operator-typed line and
  writes it elsewhere, in the live editor buffer.
  - Precedents: `#queuelineclobber` (preflight deleted operator queue text)
    and editor-truth roll-forward recovery.
  - A move needs one atomic edit through the editor-aware write path. Doing it
    from preflight queue maintenance is not proven safe today.
- **Context:** the reply's meaning comes from its parent response tree. A
  top-level row must still hand the worker that context.

Completion: these three must be solved together, so this change is a design
note plus an agent follow-up, not a partial move.

## Design

- **Trigger:** one operator-authored line, all of:
  - nested (at any depth) under a `**Response [#r-…]**` row of a queue row;
  - not inside a Waiting-on-you packet (those keep `#waitingitemstatus`
    answer semantics);
  - present in the stable preflight baseline, not first typed this cycle
    (same conservatism as `#qstrikeexplain`);
  - no claim, attempt, lineage or receipt exists for its nest identity.
- **Effect:** one atomic queue edit:
  - Insert a top-level row right after the origin's top-level tree, after any
    earlier follow-ups chained to the same origin:
    `- [#<origin>] follow-up: <verbatim operator text> [#q-<hash>]`.
  - Replace the nested line with an inert link row naming the new id
    (`- ↪ [#q-<hash>]` plus a generated-row marker), never a copy of the text.
  - The title form matches the shorter follow-up titles
    (`[#<origin>] follow-up:`).
- **Identity:** the new row's identity is its `[#q-…]` id. Its prompt-context
  packet records the origin row id and response id, so `#nestsubmit` context
  still reaches the worker.
- **Chaining:** a reply under a follow-up row's own response opens the next
  follow-up with the same origin id, so the chain stays flat and in order.
- **Origin without an id:** mint the origin's `[#q-…]` id first (the existing
  `queue follow-up --after` path already resolves origins).
- **Idempotency:** a line already replaced by a link row is never moved again.
  A line whose text equals an existing chained follow-up is linked, not
  duplicated. A second pass changes nothing.
- **Fail closed:** skip the move (and leave today's nested admission) when the
  editor and disk differ, the replica is quarantined, or any gate above fails.

## Tests to land with it

- Nested reply under a struck row's response: one top-level row after the
  origin tree, a link in place, verbatim text, second pass unchanged.
- Claimed nested leaf: never moved.
- Reply typed this cycle: not moved until the next stable baseline.
- Waiting-item packet answer: not moved.
- Reply under a follow-up's response: chained after the previous follow-up.
- Dispatch: the new row dispatches once; the link row never does.
