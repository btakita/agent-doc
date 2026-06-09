---
agent_doc_session: tsift-v0.1
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260609-030135.md`*

Compacted content:
- Archived 1 response topic(s): release + publish
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: next-steps - gpt-5

The next efficiency work for MemGraphRAG should start with measurement, then remove the obvious O(N) query/projection paths before considering a new backend.

Observed hot spots from the current implementation:
- `rank_memory_events` tokenizes the query once, but lowercases/scans every event text, clones every event into `ScoredMemoryEvent`, sorts the full candidate set, then truncates.
- `read_memory_events` reads by `ORDER BY COALESCE(observed_at_unix, created_at_unix), id LIMIT ?`; for traversal projection this favors oldest rows and has no observed-at/text index beyond kind/session/import indexes.
- `append_memory_events_as_traversal_rows_with_limit` projects up to `DEFAULT_TRAVERSAL_MEMORY_EVENT_LIMIT = 600` rows and recomputes labels, text previews, content hashes, semantic handles, and hash embeddings on each projection.
- `semantic_related_report_from_store` scans every `semantic_concept` / `semantic_entity`, parses the comma-separated embedding property per row, computes cosine for all rows, sorts all rows, then truncates.
- `graph_db_semantic_seeded_neighborhood` caps expansion after fetching and sorting all outgoing plus incident edges for each expanded node.

Recommended next steps, in order:

1. Add a MemGraphRAG performance baseline fixture before optimizing. Use a synthetic memory corpus with at least 10k events plus the existing `fixtures/memory-retrieval-eval.json`, and measure `memory query`, `memory project-graph`, `graph-db related`, and semantic seeded neighborhood latency. Wire this into the existing backend-eval/perf-gate style so every later change proves both latency and result parity.

2. Make memory candidate retrieval indexed. Add indexes on `observed_at_unix` / `created_at_unix`, add FTS5 or a materialized token table for `memory_events.text`, and make query ranking pull a bounded lexical+recent candidate set from SQLite. In Rust, keep only a top-K heap or `select_nth_unstable` window instead of cloning and sorting every event.

3. Store semantic vectors in a query-efficient shape. Keep the current `embedding` property for compatibility if needed, but materialize a typed `graph_node_embeddings` or equivalent table/blob keyed by node id and model. Then add a GraphStore query API for top semantic candidates so `semantic_related_report_from_store` no longer calls `nodes_by_kind` and reparses every vector string per query.

4. Make memory graph projection incremental. Track a memory source watermark or per-event content hash so unchanged memory rows do not reproject. Also split `read_memory_events` into explicit policies: recent-first for traversal/related readiness, oldest-first only where replay order is required, and query-relevant for retrieval packets.

5. Fold memory decay into the shared graph ranking path. Extend `NeighborhoodScoring` or add a `MemoryAware` scoring mode using `observed_at_unix`, `memory_kind`, semantic edge strength, and token budget. This closes the roadmap gap where decay-weighted memory ranking and `GraphStore::ranked_neighborhood` are still separate, and it lets memory nodes prune low-signal graph expansion before work is spent.

6. Push semantic seeded expansion caps into GraphStore/SQLite. Add a batched API that fetches the highest-scoring incident/outgoing edges per node with SQL-side limits and batched node lookup. That avoids the current fetch-all, sort-all, truncate-later behavior while keeping the existing result ordering and diagnostics.

I would not start by swapping SQLite for another graph backend. The current bottlenecks are candidate selection, vector materialization, projection invalidation, and cap placement. A backend change should come only after the baseline proves SQLite is still the limiting factor.

Backlog capture: added the six recommendations above to `agent:backlog` in priority order. Top backlog item: #kx6s.
Changed paths: `tasks/software/tsift.md` only; no code edits in this turn.
Commands: `agent-doc preflight tasks/software/tsift.md`; `tsift status`; bounded `tsift --envelope source-read` and `tsift --envelope search` reads for `tsift-memgraphrag`, `tsift-memory`, `tsift-core`, `tsift-cli`, and the MemGraphRAG/graph specs; `agent-doc repair tasks/software/tsift.md` for the interrupted write recovery.
Verification: read-only planning response; no tests run because no code changed.
Confidence: medium-high. The recommendations are grounded in current source paths, but exact priority should be validated by the proposed perf baseline.
Escalation: none.
<!-- agent:boundary:6a72db36 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#kx6s] Add a MemGraphRAG performance baseline fixture and gate covering memory query, memory project-graph, graph-db related, and semantic seeded neighborhood latency.
- [ ] [#qszc] Make memory candidate retrieval indexed with observed-at/create-time indexes plus FTS5 or a materialized token table, and rank a bounded candidate set instead of all memory events.
- [ ] [#qs5v] Materialize semantic vectors in a typed table/blob and add a GraphStore top-candidate query so semantic related does not scan and parse every semantic node property.
- [ ] [#zkft] Make memory graph projection incremental with memory watermarks/content hashes and explicit recent-first, oldest-first, and query-relevant read policies.
- [ ] [#58p8] Fold observed-at decay and memory node signals into shared GraphStore ranked_neighborhood scoring so memory and code prune through one graph-ranking path.
- [ ] [#kn0d] Push semantic seeded neighborhood caps into GraphStore/SQLite with SQL-side edge ranking limits and batched node lookup.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
