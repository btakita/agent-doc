# Proposal: Top-Level Follow-Up Rows For Nested Operator Replies

> **Status:** implemented (`#nestedreplyfollowup`). The normative behavior is
> in [`SPEC.md`](../../SPEC.md) and
> [`specs/07-orchestration-commands.md`](../07-orchestration-commands.md);
> this note keeps the original design and records what changed in the build
> (see "As built").

## Request

> If the operator responds in a subtree, break out a top-level follow-up
> tree...chain top-level tree together.

Today an operator reply typed under a response tree is admitted where it
stands. It becomes a nested leaf whose work identity is its nest vector
(`root: parent: leaf`, `#nestsubmit`, `#claimdispatchidentity`). Each round of
replies nests one level deeper.

## Why this was held back

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

## As built

- **Code:** `agent_doc_queue::nested_reply_followup` (pure planner),
  `agent_doc_queue_io::nested_reply_followup` (gates, ledgers).
- **Identity risk:** handled by the held gate.
  - A reply moves only when no claim, lineage attempt, or release receipt
    records its nest identity or a text containing it.
  - Its owning row must not have a claim or receipt.
  - An unreadable ledger holds every reply.
  - So no claim needs migrating. The follow-up root-provenance and
    owner-classification ledgers carry the reply row's records to the new row
    (the `07ebf13745` carry-over), and the new row's root is recorded.
- **Operator-text risk:** handled by where and when the move runs.
  - `queue sync`: before the response-tree passes, judged against the
    baseline the sync loaded first, written with `converge_document_or_disk`.
  - Idle self-heal: only after `QUEUE_PROJECTION_SELF_HEAL_QUIET` with no
    newer document edge, inside its one generation-fenced CAS. A newer
    operator cut supersedes it.
  - Preflight does not move replies.
  - Stable means the exact line is in that baseline and
    `edit_settle::completion_signal` does not call it unfinished.
  - When disk differs from the authority, the same plan over disk must give
    the same moves.
- **Context risk:** the moved row's prompt-context packet names the response
  (`r-…` and its Summary) the link row sits under; the title names the
  origin id.
- **Changes from the design above:**
  - Reopened owner: a struck row is derived open while a reply is its live
    leaf (`#struckparentkids`), so by the time a reply is in a baseline its
    owner is usually live. Such an owner counts when it has no lifecycle
    marker or attempt annotation and its response `**State:**` is `Complete.`
    or `Follow-up work remains.`. It is struck again in the same edit once the
    reply was its last live leaf, so it never becomes a new leaf.
  - Reply id: preflight gives the reply its own `[#q-…]`. The new row keeps
    that id; only an id-less reply gets a fresh mint.
  - Link row: `- ↪ [#q-…] <!-- agent-doc:followup-link -->`. The marker is
    reserved generated response structure, so the link is never queue work.
  - Origin without an id: a struck origin cannot be admitted an id, so the
    reply stays nested.
  - Also left nested: a reply with children, a reply to another reply, a
    loop row, directives, presets, commands, `[#id]` references, `status?`,
    and `operator:` / `human (…):` items.
- **Tests:** every case listed above, plus a live claim on the reply and on
  its owner, the reopened owner (restruck only when its last reply leaves), a
  second pass, byte-for-byte text, `queue sync` end to end, and the
  self-heal CAS.

Completion: implemented; no open design items.
