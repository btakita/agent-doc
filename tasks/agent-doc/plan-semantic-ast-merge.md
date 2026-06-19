# Plan: `#semmerge` — semantic (node-keyed) operator↔agent merge

**Status:** design / not started
**Filed:** 2026-06-18 (boost-client/monsterrodholders.md dogfooding session)
**Depends on:** `#md-ast-document-model` (typed AST + structured CRDT substrate)
**Sibling of:** `#docdriftgrace` (unrelated-drift silent rebase). `#docdriftgrace`
handles drift *unrelated* to the in-flight turn; `#semmerge` handles the case
where operator and agent changes **overlap or conflict** and both must be
considered. Together they remove the manual `reset --from-current` recovery.

## Problem (firsthand repro this session)

Draining the `[#cf-txn-email]` queue head on `boost-client/tasks/monsterrodholders.md`:
the agent produced three changes (append a `### Re:` exchange turn, gate
`#cf-txn-email` and migrate it backlog→review, strike/consume the queue head)
**while the operator's editor buffer concurrently** flipped frontmatter
`queue: start → stop` and restructured the queue block.

Closeout could not commit:

```
[write] IPC proof insufficient ... invariant=live_prompt_drift_after_preflight
        recovery=content_ours_snapshot_next_cycle
Error: queue consume: refused direct disk write ... while editor IPC listener
       is active (reason=ack_mismatch)
```

The current policy is **fail-closed, last-writer-wins**: when the editor buffer
drifts from the agent's baseline, `converge.rs::try_editor_converge()` refuses
the disk write and adopts `content_ours` (the editor buffer) for the next cycle.
The agent's three changes are **dropped on the floor**, and the operator had to
run `agent-doc reset --from-current` + `agent-doc commit` by hand to land the
combined state. In this case the two change-sets were almost entirely
**node-disjoint** (operator: frontmatter + queue block; agent: a *different*
queue head + review migration + a new exchange turn) — a correct merge could
have applied both with zero conflict.

### Why it happens (code)

- `agent-doc-core/src/crdt.rs::merge()` is a **line-based** 3-way merge (stale-base
  detection + shared-prefix advancement + block dedup). It has **no model of
  document components** (exchange / queue / backlog / review) or item identity,
  so concurrent edits to *different* items present as a text conflict.
- `agent-doc-orchestration/src/write/converge.rs::try_editor_converge()` owns the
  fail-closed policy: IPC listener active + any negative ack outcome
  (`ack_mismatch`, `no_ack_content`, `no_ack`, `send_failed`) ⇒ `bail` and refuse
  the disk write. `guard_ipc_snapshot_adoption_against_live_prompt_drift()` +
  `live_prompt_drift_auto_recovery_safe()` then adopt `content_ours`.
- `write/queue_consume.rs::consume_queue_prompts_with_outcome()` delegates its
  write to the same converge path, so a drifted buffer aborts the head strike.
- A **typed component AST already exists** (`agent-doc-markdown-ast`: Phase 1 block
  tree, Phase 2 `Component { items: Vec<Item{ id, struck, pinned, kind, ... } }>`,
  Phase 3 `OverlayCrdtDoc` yrs schema) but the **merge/converge path does not use
  it** — Phase 4 semantic merge is planned, unimplemented.

## Fix

Add a **semantic, node-keyed three-way merge** over the component AST, consumed by
the converge path *before* it fail-closes. Most concurrent edits are node-disjoint
and merge cleanly ("most CRDT merges should be successful"); only true same-node
conflicts surface, and even those have deterministic, mostly-non-blocking
outcomes. This is the **merge-policy** layer on top of `#md-ast-document-model`'s
node-keyed substrate — not a second CRDT.

### Required Contract (architecture-first)

- **Invariant:** When operator (editor buffer) and agent both have pending
  changes, closeout applies **both** change-sets; the only ways an agent change is
  not applied verbatim are (a) the operator **deleted** the node the agent edited
  → the deletion stands and the agent emits an exchange turn acknowledging it, or
  (b) operator and agent edited the **same** node's content → operator content
  wins and the agent's change is recorded as an exchange note. The agent's
  committed changes are **never** silently discarded by adopting `content_ours`,
  and an operator-deleted node is **never** resurrected.
- **Policy owner:** a new pure function in the `markdown-ast` crate
  (`semantic_merge(base, ours_agent, theirs_operator) -> SemanticMerge`) returning
  a typed `SemanticMerge { merged_doc, outcomes: Vec<NodeOutcome>, requires_ack:
  Vec<AckRequest> }`. `write/converge.rs` becomes an **adapter**: it builds the
  three node-trees, calls the owner, applies `merged_doc`, and turns
  `requires_ack` into exchange-turn effects. `converge.rs` must not re-decide
  per-node policy by matching text or error strings.
- **Transition table** (per node, keyed by stable id, classified by component):

  | base | agent (ours) | operator (theirs) | outcome |
  |------|--------------|-------------------|---------|
  | present | unchanged | unchanged | keep (no-op) |
  | present | edited | unchanged | **apply agent edit** |
  | present | unchanged | edited | **apply operator edit** |
  | present | edited | edited (same text) | apply once (convergent) |
  | present | edited | edited (different text) | **operator wins + agent note in exchange** |
  | present | edited / struck | **deleted** | **keep deletion + emit `AckRequest`** (agent turn acknowledges) |
  | present | deleted (struck/consume) | edited | operator edit wins (un-strike), agent note in exchange |
  | absent | added | absent | **apply agent add** (e.g. new `### Re:` turn) |
  | absent | absent | added | **apply operator add** (e.g. new queue line, `queue: stop`) |
  | absent | added | added (different ids) | **apply both** (node-disjoint union) |
  | absent | added | added (same id, diff text) | operator wins + agent note |

  Frontmatter scalars (e.g. `queue: start/stop`) are nodes too: operator edit to a
  key the agent did not touch ⇒ apply operator value (this session's `queue: stop`
  would have been honored automatically).
- **Evidence inputs:** three `markdown-ast` component trees (base snapshot, agent
  result, operator buffer) with stable node ids + per-node `struck/pinned/kind`;
  the IPC ack-content sidecar (operator buffer truth); the agent's pending patch
  set. No low-level error-string matching in the owner.
- **Allowed edit surfaces:** `agent-doc-markdown-ast` (new `semantic_merge` +
  `NodeOutcome`/`AckRequest` types — Phase 4 area); `write/converge.rs` (adapter:
  call owner instead of line-merge + fail-close); `write/queue_consume.rs` (strike
  via node id through the merged tree); minimal `cycle_state.rs` plumbing to carry
  `AckRequest`s into the next response. Do **not** add a parallel merge in
  `crdt.rs` — `crdt.rs::merge` stays the text fallback only when AST parse fails.
- **Verification:** pure decision-table tests for every transition row in the
  owner crate; a SimWorld/integration test reproducing **this** session
  (operator flips `queue: stop` + edits queue block while agent strikes a
  *different* head + appends a `### Re:` turn) asserting a single clean commit with
  **both** change-sets present and **no** `reset --from-current`; an
  operator-delete-vs-agent-edit test asserting the deletion stands **and** an
  acknowledgement turn is emitted; a same-node-conflict test asserting operator
  content wins with an agent note. Reuse the existing harnesses in
  `tests/ipc_closeout_model.rs`, `tests/live_ipc_race_integration.rs`,
  `tests/finalize_integration.rs`.
- **Out of scope:** rich semantic diff of *prose* inside a single exchange turn
  (turns are append-only nodes — a new turn is a new node, never an in-place prose
  merge); 3-way merge of fenced-code bodies (treated as opaque node text);
  multi-operator concurrency. Falling back to the line-based `crdt.rs::merge` only
  when the AST cannot be parsed.

## Phases (one backlog id per actionable phase)

1. **`#semmerge-owner`** — implement `markdown_ast::semantic_merge` + the
   `SemanticMerge`/`NodeOutcome`/`AckRequest` types and the full transition table
   over the Phase 2 component overlay. Pure, exhaustively unit-tested, no
   agent-doc coupling. (Hard-depends on `#md-ast-component-overlay`; uses
   `#md-ast-crdt-schema` node ids where available.)
2. **`#semmerge-converge-adapter`** — replace the fail-closed line-merge in
   `write/converge.rs` with: parse the three trees → call the owner → apply
   `merged_doc` → on parse failure only, fall back to `crdt.rs::merge`. Delete the
   `content_ours`-wins adoption for the node-disjoint case; keep fail-closed
   **only** when the owner returns an unrepresentable conflict.
3. **`#semmerge-queue-node-strike`** — ✅ DONE (`#smqstrike`, verified 0.34.14).
   The contract (strike on the merged node tree by id; a drifted operator queue
   edit never aborts/loses the head strike) is delivered across all three write
   paths: (a) editor IPC sends per-node `strike` patches keyed by `node_key`
   (`build_ipc_node_patches_json` → `queue_consume_node_patches`), so concurrent
   operator edits to other nodes are untouched; (b) the disk path reconciles a
   diverged snapshot instead of bailing (the merged document wins, head struck,
   concurrent add preserved — `queue_consume_reconciles_diverged_snapshot_instead_of_bailing`);
   (c) the `#smconv` content_ours convergence runs `semantic_merge`, which treats
   `struck` as a node-disjoint flag-edit so a struck head merges cleanly with a
   concurrent operator add. Added explicit merged-tree coverage
   (`smqstrike_struck_head_survives_concurrent_operator_queue_add`). No
   re-implementation needed — landed incrementally with the `#smconv`/divergence
   work. Overlaps `#qheadsync(C)`.
4. **`#semmerge-ack-turn`** — ✅ DONE (0.34.14, `#pk3f`). `write/ipc.rs` now
   persists each convergence `AckRequest` to `CycleState.pending_semantic_merge_acks`
   via `record_semantic_merge_acks`; `start_preflight` carries un-surfaced acks
   forward exactly one cycle (per-ack `surfaced` flag, not a millisecond-collidable
   `cycle_id` compare); preflight surfaces them as `semantic_merge_acks` plus a
   companion `semantic_merge_ack_pending` warning so the standard "surface warnings"
   skill path drives the acknowledgement. New stable `AckReason::token()` wire
   string. The merged document is unchanged (operator already won) — this only adds
   the courtesy acknowledgement.
5. **`#semmerge-simworld`** — ✅ DONE (`#smsim`, 0.34.14). Added deterministic
   SimWorld coverage of the operator↔agent concurrent-edit matrix through the
   `#smconv` convergence (`SimWorld::converge_semantic_merge` runs the production
   `semantic_merge_scoped`): node-disjoint auto-merge (agent strike + concurrent
   operator add both survive), same-node operator-wins + ack in the active area,
   the identical same-node conflict OUTSIDE the active area auto-resolving
   operator-wins with NO ack (`#smturnactive` gating), and
   operator-deleted-an-agent-edited-node keeping the deletion + raising the ack.
   Four `semmerge_sim_*` scenarios + four coverage counters. Locks the merge/IPC
   data-loss family against regression.

**All five semmerge phases are now complete.**

## Quick-win stopgap (independent of the rewrite)

Before the AST merge lands, narrow the fail-closed blast radius in
`converge.rs`: when the **only** drift between `content_ours` and the agent target
is in components/frontmatter keys the **agent did not touch** this cycle (cheap
component-boundary diff, not full AST), auto-rebase the agent patch onto the
operator buffer and commit instead of refusing. This is the line-level
approximation of the node-disjoint union row and would have auto-committed this
session. It shares the boundary-classification idea with `#docdriftgrace`.

## Bugs this subsumes / overlaps

`#qheadsync(C)` (manual queue strikes dropped by `content_ours`), `#docdriftgrace`
(unrelated-drift rebase — this is the *overlapping/conflicting* counterpart),
`#ipc-crdt-response-drift`, and the `live_prompt_drift_after_preflight` /
`reset --from-current` recovery class. Hard-depends on `#md-ast-document-model`.
