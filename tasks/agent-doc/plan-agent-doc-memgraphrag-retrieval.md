# Plan: agent-doc MemGraphRAG retrieval

Backlog: `#agent-doc-memgraphrag-retrieval`

## Goal

Give agent-doc a direct semantic-memory surface over session history and tracked work so it can answer two questions without scanning long exchanges:

- Is this new backlog/review item already tracked, already done, or probably a duplicate?
- Has this prompt or bug already been resolved in a prior session closeout?

The integration must depend on lean tsift library crates, not the `tsift` CLI. Heavy codebase graph indexing stays in tsift.

## Existing Substrate

- `src/tsift/packages/tsift-memory` owns `MemoryStore`, `MemoryEvent`, `project_memory_events`, capture handoff guards, and decay-weighted `rank_memory_events`.
- `src/tsift/specs/memgraphrag-direction.md` documents that core MemGraphRAG pieces are already present in tsift: memory events, graph projection, temporal decay, and ontology projection.
- `src/tsift/plans/shared-md-ast-memgraphrag-lib.md` already separates the tsift-side shared crate work from agent-doc integration.
- agent-doc now has a markdown-AST document model and node-keyed queue/backlog/watch events, so capture can anchor to stable document nodes instead of raw lines.

## Non-Goals

- Do not embed the tsift CLI or spawn it from the per-cycle hot path.
- Do not load or refresh the full code graph during normal `preflight` / `finalize`.
- Do not silently delete or auto-complete user work from semantic similarity alone.
- Do not require embeddings or network calls for the first slice. Start with deterministic lexical + recency ranking from `tsift-memory`; add embeddings later behind an explicit readiness gate.

## Design

Add an agent-doc memory adapter that depends on `tsift-memory` and only the graph/core crates needed for stable event records. The adapter should live behind a narrow agent-doc API, for example:

- `capture_closeout_memory(file, closeout_context) -> CaptureReport`
- `query_tracked_work_memory(root, query, scope, limit) -> MemoryRetrievalReport`
- `detect_tracked_work_overlap(root, item_text, component) -> OverlapReport`

The backing database should be the project `.tsift/memory.db` when available so tsift can later project the same events into `.tsift/graph.db`. If no `.tsift` directory exists, agent-doc can initialize it explicitly only when memory capture is enabled.

## Phase 1: Capture

Capture memory events after successful binary-owned closeouts:

- prompt target text and queue-head text
- response heading and short response summary
- closeout proof: commit hash, session document path, component, boundary id, queue consumption result
- tracked-work deltas from backlog/review/done/icebox, keyed by item id and markdown node key

Capture must be best-effort by default: failure to write memory emits an ops-log warning and does not fail the user closeout. A strict mode can be added later for CI-style evaluation.

## Phase 2: Query

Expose a small query surface:

- `agent-doc memory search <FILE> --query <TEXT> --json`
- `agent-doc memory status <FILE> --json`
- internal `preflight` / `plan` helper returning bounded citations

Results must cite the session file, commit hash, response heading, event kind, item id, and score. The output should be deterministic and token-bounded.

## Phase 3: Backlog And Review Dedup

When backlog/review items are added or edited, query memory for similar active/done items:

- exact id match remains authoritative
- high-confidence semantic overlap emits a warning or review gate
- done/archive matches can suggest "already fixed" but must not auto-mark done without a closeout proof
- duplicate detection should record the candidate source so later `#automated-completion-detection` can reuse it

This should run after normal syntactic/id-based checks. Semantic retrieval supplements the existing queue/backlog model; it does not replace it.

## Phase 4: Automated Completion Detection

Use the same memory index to answer whether an item was resolved in a past session:

- search completed response summaries and done/archive events
- require a cited closeout proof before suggesting completion
- surface the proof as a pending gate or review recommendation, not an automatic state flip

This phase is the bridge to `#automated-completion-detection`.

## Phase 5: Optional Code Retrieval

Preflight/plan may include tsift graph evidence when a fresh `.tsift/graph.db` already exists, but agent-doc should not refresh the heavy index itself. If the graph is stale, missing, locked, or over budget, agent-doc emits a bounded warning and continues without graph evidence.

## Configuration

Start opt-in:

```yaml
agent_doc_memory:
  capture: true
  retrieval: warn
  max_results: 5
  max_tokens: 1200
```

`retrieval: off | warn | strict`:

- `off`: no memory query
- `warn`: emit suggestions only
- `strict`: reserved for future CI/eval gates

## Tests

- Unit: closeout capture builds stable `MemoryEvent` records from a fixture response.
- Unit: tracked-work overlap ranks exact id, exact text, and similar text in that order.
- Unit: done/archive proof requires commit/session citation before "already resolved" is suggested.
- Integration: `finalize` with memory capture enabled writes `.tsift/memory.db` without invoking the `tsift` CLI.
- Integration: `preflight` with memory retrieval enabled emits bounded warning citations and remains clean when the memory DB is absent.
- Regression: a semantic duplicate never silently deletes, completes, or reorders queue/backlog items.

## First Implementation Slice

1. Add an `agent-doc-memory` crate or module with pure capture/query APIs around `tsift-memory`.
2. Wire closeout capture after successful `finalize` / `write --commit`, behind opt-in config.
3. Add `agent-doc memory status/search` JSON commands.
4. Add tests proving no `tsift` CLI subprocess is used in capture/query.
5. Document the config and privacy behavior in agent-doc specs.

This plan intentionally gates the backlog item after planning. Implementation should be a later queue item so it can be scoped, tested, built, installed, committed, and pushed as code.
