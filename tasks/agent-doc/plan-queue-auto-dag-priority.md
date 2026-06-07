# Plan — auto-dag priority ordering for queue/backlog/icebox (`#queue-auto-dag-priority`)

**Backlog:** `#queue-auto-dag-priority` in `tasks/agent-doc/agent-doc-bugs2.md`
**Requested:** operator wants the `priority` attribute to use an auto-DAG so execution order is derived from a dependency graph, updated per turn, efficiently, with manual pins (`:pushpin:`) as flags and blockers respected.

## Operator spec (verbatim intent)

- `priority` attribute in queue/backlog/icebox should use **auto-dag** to determine execution order.
- Include **graph ordering** and **linear ordering** projections.
- `priority` updates the queue **per turn**.
- Efficient — **localized insertion** (graph / linked-list analogy): a newly added item's priority is determined without reprocessing already-prioritized items.
- Manual pins flag prioritized items (`:pushpin:` operator / `:round_pushpin:` agent — already shipped, `#queue-manual-priority-override`).
- Pins float to top **except when a blocker is determined by auto-dag** (a dependency that must run first wins over the pin).

## Design decision — dependency declaration (`after=#id`)

The genuine design fork is *how an item declares a dependency*. Decision (sensible default, documented for redirect): an item declares hard dependencies with an **`after=#id`** token in its text (repeatable; comma/space lists: `after=#a,#b`). This mirrors the existing `priority=N` token convention and the `--pending-add-after <ID>` flag semantics. `after=#dep` means **dep must be ordered before this item**.

## Ordering algorithm — priority-weighted topological sort

`queue::sort_prompts_by_dag(entries, rank, deps)`:

1. Nodes = queue prompts (by resolved id; free-text prompts have no id and no edges).
2. Edges = `after=#dep` → edge `dep → item` (only when `dep` is also a present node; dangling deps are ignored for ordering but recorded as blockers).
3. **Kahn's algorithm** with a priority-ordered ready set: among items whose dependencies are already emitted, pick the best by the existing tie-break `(pin_tier, priority_rank, document_order)` — operator pin → agent pin → unpinned-by-rank, stable on document order.
4. Result is the **linear ordering projection** (the executable queue order). Degenerates exactly to today's pin+priority sort when there are no edges (backward compatible).

**Blocker semantics:** because deps are emitted first, a pinned item that `after=#dep`s an unfinished item cannot float above `dep` — the blocker wins over the pin, exactly as the operator specified. A dep that is open in `agent:backlog` (not done) keeps its dependent below it; a dangling/absent dep is logged as a blocker annotation.

**Efficiency:** Kahn's is O(V+E); queue sizes are small. Stable tie-break (document order) means items without new edges keep their relative order, so adding one item is effectively a localized move. True incremental DAG maintenance is a deferred optimization — current sizes do not need it.

## Projections

- **Linear** (shipped here): the topologically-sorted queue body (what drains).
- **Graph** (this increment: ops-log + `agent-doc queue dag <FILE>` debug emit grouping items by topological level/depth; richer visualization deferred).

## Acceptance

- An item with `after=#a` always sorts after `#a` in the queue, regardless of pin/priority.
- A pinned item blocked by an open dependency does not float above that dependency.
- No edges → identical to the current pin+priority ordering (no churn).
- Deterministic + idempotent; covered by unit tests.

## Increment status

- **v1 (this cycle):** `after=#id` parsing, `sort_prompts_by_dag` priority-weighted topological sort wired into preflight queue ordering, blocker-respecting pins, tests, spec. Linear projection live.
- **Deferred:** rich graph-visualization projection; incremental (non-recompute) DAG maintenance; inferred (non-explicit) dependencies.
