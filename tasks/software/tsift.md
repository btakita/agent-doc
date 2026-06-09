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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260609-042037.md`*

Compacted content:
- Archived 1 response topic(s): next-steps - gpt-5
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: kx6s — gpt-5

> **Queue prompt:**
>
> do [#kx6s]


Implemented `#kx6s`.

Changed paths:
- `src/tsift/packages/tsift-digest/src/metric_digest.rs`: added `memgraphrag_performance_gate` detection/evaluation for `memory_query`, `memory_project_graph`, `graph_db_related`, and `semantic_seeded_neighborhood` latency metrics.
- `src/tsift/fixtures/memgraphrag-performance-history.json`: added the canonical baseline/current four-surface fixture.
- `src/tsift/packages/tsift-cli/src/commands/digests.rs`, `src/tsift/packages/tsift-cli/src/lib.rs`, `src/tsift/packages/tsift-cli/tests/exit_code.rs`: added CLI compact/human output and fixture integration coverage.
- `src/tsift/specs/digests-sessions.md`, `src/tsift/specs/memgraphrag-direction.md`, `src/tsift/VERSIONS.md`: documented the fixture/gate.

Commits:
- `src/tsift`: `0a3757a` (`metric-digest: add MemGraphRAG performance gate`), pushed to `origin/main`.
- `agent-loop`: `dfc10eb12` (`tsift: advance MemGraphRAG performance gate`), pushed to `origin/main`.

Verification:
- `cargo test -p tsift-digest memgraphrag_performance_gate` passed.
- `cargo test -p tsift-cli --test exit_code metric_digest_reports_memgraphrag_performance_gate_fixture` passed.
- `make check` passed after the final scoped diff cleanup: workspace clippy, workspace tests, doctests, OpenCode package tests, and npm pack dry-run.
- `cargo install --path .` completed and replaced the local `tsift` binary.
- Installed-binary smoke check `tsift metric-digest --input fixtures/memgraphrag-performance-history.json --json` emitted `memgraphrag_performance_gate.decision=pass`.
- Latest tsift GitHub Actions `CI` run for commit `0a3757a` was `in_progress` at `2026-06-09T04:36:52Z`; local verification is green. The parent `agent-loop` repo reported no workflow named `CI`.

Confidence: high.
Escalation: none.

### Re: qszc — gpt-5

> **Queue prompt:**
>
> do [#qszc]


Implemented indexed MemGraphRAG memory candidate retrieval and persisted it in git.

Changed paths:
- `src/tsift/packages/tsift-memory/src/lib.rs`
- `src/tsift/packages/tsift-memgraphrag/src/lib.rs`
- `src/tsift/packages/tsift-cli/src/commands/memory.rs`
- `src/tsift/packages/tsift-cli/tests/exit_code.rs`
- `src/tsift/specs/memgraphrag-direction.md`
- `src/tsift/VERSIONS.md`
- parent submodule pointer `src/tsift`

What changed:
- Bumped `tsift-memory` schema to v2 and added observed/create-time indexes, `memory_internal_state`, and an FTS5 `memory_events_fts` table with insert/update/delete sync triggers.
- Added one-time FTS rebuild/backfill on `MemoryStore::open_or_create` so existing memory DB rows become queryable after schema initialization.
- Added `read_memory_event_candidates(memory_db, query, limit)`, which returns a deduplicated, caller-bounded set from FTS matches plus recent fallback rows instead of forcing callers to rank every memory event.
- Added `rank_memory_event_candidates` and `memory_rank_candidate_limit` in `tsift-memgraphrag`; `MemoryQueryPlan` now reports `candidate_limit` and documents the indexed candidate-set contract.
- Updated memory CLI status/query-plan surfaces and compiled CLI coverage for the new `candidate_limit` field.
- Updated MemGraphRAG direction docs and `VERSIONS.md`.

Verification:
- `cargo test -p tsift-memory memory_event_candidates` passed.
- `cargo test -p tsift-memory memory_schema_creates_candidate_indexes_and_fts_table` passed.
- `cargo test -p tsift-memgraphrag rank_memory_event_candidates` passed.
- `cargo test -p tsift-memgraphrag plan_memory_query_carries_default_decay_config` passed.
- `cargo test -p tsift-cli --test exit_code memory_query_plan_uses_graph_db_related_with_parent_json_flag` passed.
- `cargo test -p tsift-cli --test exit_code memory_status_prefers_graph_db_related_with_claude_mem_fallback` passed.
- `cargo test -p tsift-memory` passed.
- `cargo test -p tsift-memgraphrag` passed.
- `make check` passed.
- `cargo install --path .` passed and replaced the local `tsift` binary.
- Installed smoke: `tsift memory query-plan "memory retrieval" --json` reports `candidate_limit: 80` and the indexed candidate output contract.
- Installed smoke: fresh `tsift memory init/status` reports `schema_version: 2`, `memory_events_fts`, `memory_internal_state`, and `query_api.candidate_limit: 80`.
- `git diff --check` passed.
- `cargo fmt --check` was probed and still reports broad pre-existing workspace formatting drift in unrelated files; I did not run a workspace formatter. The required project gate is `make check`, which passed.

CI:
- `src/tsift`: `gh run list --workflow CI --limit 1` reports run `27184774293` in progress for commit `56ca910` (`memgraphrag: index memory candidates`) as of 2026-06-09 04:56:15Z.
- Parent repo: `gh run list --workflow CI --limit 1` reports no workflow named `CI`.

Commits pushed:
- `src/tsift`: `56ca910` `memgraphrag: index memory candidates`, pushed to `origin/main`.
- Parent `agent-loop`: `8fced3a8b` `tsift: advance indexed memory candidates`, pushed to `origin/main`.

Confidence: high. Local clippy/tests/build/install/smokes passed, and the only unresolved external signal is the in-progress GitHub Actions run.

### Re: qs5v — gpt-5

Implemented the semantic-vector materialization path and committed it.

changed_paths:
- `src/tsift/VERSIONS.md`
- `src/tsift/packages/tsift-core/src/lib.rs`
- `src/tsift/packages/tsift-core/src/store.rs`
- `src/tsift/packages/tsift-core/src/types.rs`
- `src/tsift/packages/tsift-sqlite/src/lib.rs`
- `src/tsift/packages/tsift-cli/src/lib.rs`
- `src/tsift/specs/graph.md`
- `src/tsift/specs/memgraphrag-direction.md`

commits:
- `src/tsift`: `de09326 graph-db: materialize semantic vectors`
- superproject: `18754108e tsift: advance semantic vector materialization`

summary:
- Added SQLite graph schema v6 with `graph_node_semantic_vectors`, typed little-endian `f64` vector blobs, model/kind/dimension metadata, an indexed kind/dimension lookup path, and migration backfill from legacy `properties_json.embedding`.
- Added write-path maintenance for direct node upserts, projection upserts, and full projection replacement so the typed table stays in sync with semantic node properties.
- Added `GraphStore::semantic_top_candidates` with a core property-scan fallback and a SQLite override that scores top candidates from the materialized vector table, so semantic related no longer parses every semantic node property.
- Updated the semantic-related CLI path, graph specs, direction notes, and release notes.

commands:
- `cargo test -p tsift-sqlite sqlite_semantic_top_candidates_use_materialized_vector_table -- --nocapture`
- `cargo test -p tsift-sqlite sqlite_schema_migration_backfills_materialized_node_properties -- --nocapture`
- `cargo test -p tsift-cli semantic_related_query_uses_persisted_graph_embeddings -- --nocapture`
- `cargo test -p tsift-cli graph_db_related_query_uses_semantic_seeds_and_incident_neighborhoods -- --nocapture`
- `cargo test -p tsift-sqlite`
- `cargo test -p tsift-core`
- `git diff --check`
- `make check`
- `cargo install --path .`
- `tsift graph-db --path . --json related "semantic vector" --limit 2`
- `gh run watch 27185579881 --exit-status`

verification:
- Targeted and package tests passed.
- `make check` passed for the workspace.
- `cargo install --path .` passed.
- Installed smoke command exited 0; the current repo has no semantic summary rows, so it reported the expected `summary_cache_empty` readiness warning rather than semantic matches.
- `src/tsift` GitHub Actions CI run `27185579881` passed: `live Convex graph acceptance` and `clippy + test` were both green.
- Parent repo `gh run list --workflow CI --limit 1` reported no workflow named `CI`.
- `cargo fmt --check --package tsift-core --package tsift-sqlite --package tsift-cli` still fails on broad pre-existing formatting drift outside the touched change; touched sections were kept formatted and `git diff --check` passed.

confidence: high

escalation: none

### Re: zkft - gpt-5

> **Queue prompt:**
>
> :pushpin: :round_pushpin: do [#zkft]


Implemented `#zkft`.

Summary:
- Made memory reads explicit and watermarkable in `tsift-memory` with `oldest_first`, `recent_first`, and `query_relevant` policies, plus selection content hashes and source high-water marks.
- Switched memory graph projection to default `recent_first`, added policy/query CLI controls, and persisted a `memory_projection` metadata node carrying the read policy, limit, event counts, source watermark, and content hash.
- Made SQLite graph upserts incremental for unchanged node/edge row hashes so repeated identical projections do not rewrite materialized rows.
- Updated README/spec/version notes for the policy and watermark behavior.

Changed paths:
- `src/tsift/packages/tsift-memory/src/lib.rs`
- `src/tsift/packages/tsift-memory/README.md`
- `src/tsift/packages/tsift-memgraphrag/src/lib.rs`
- `src/tsift/packages/tsift-memgraphrag/README.md`
- `src/tsift/packages/tsift-cli/src/cli.rs`
- `src/tsift/packages/tsift-cli/src/commands/memory.rs`
- `src/tsift/packages/tsift-cli/src/lib.rs`
- `src/tsift/packages/tsift-sqlite/src/lib.rs`
- `src/tsift/specs/memgraphrag-direction.md`
- `src/tsift/VERSIONS.md`

Commands:
- `cargo test -p tsift-memory read_memory_events_with_policy_orders_explicitly -- --nocapture`
- `cargo test -p tsift-memory query_relevant_read_policy_requires_query_and_hashes_selection -- --nocapture`
- `cargo test -p tsift-cli project_memory_into_graph_persists_memory_nodes -- --nocapture`
- `cargo test -p tsift-sqlite sqlite_upsert_projection_batches_rows_and_properties -- --nocapture`
- `cargo test -p tsift-memory`
- `cargo test -p tsift-memgraphrag`
- `cargo test -p tsift-sqlite`
- `cargo test -p tsift-cli`
- `cargo test -p tsift-cli cli_parses_memory_project_graph_read_policy -- --nocapture`
- `git diff --check`
- `make check`
- `cargo install --path .`
- `tsift memory project-graph /tmp/tsift-zkft-smoke.wCTBzr --graph-db /tmp/tsift-zkft-smoke.wCTBzr/graph.db --read-policy query-relevant --query semantic --limit 3 --json`
- `gh run watch 27186383876 --exit-status`

Verification:
- Local targeted and package tests passed.
- Workspace `make check` passed.
- Installed CLI smoke test passed and returned `read_policy`, `source_watermark`, `content_hash`, `events_available`, and a `memory_projection` node kind in JSON.
- Pushed `src/tsift` CI run `27186383876` passed: `live Convex graph acceptance` and `clippy + test` both green.
- Parent repo has no workflow named `CI` to query with `gh run list --workflow CI --limit 1`; the applicable `src/tsift` workflow was checked instead.

Commits:
- `src/tsift`: `503978c memgraphrag: make memory projection incremental`
- parent: `7d95ad685 tsift: advance incremental memory projection`

Escalation: none.
Confidence: high.

### Re: 58p8 - gpt-5

> **Queue prompt:**
>
> :pushpin: :round_pushpin: do [#58p8]


Implemented `#58p8`.

Summary:
- Folded `observed_at` decay and memory-node signals into the shared `GraphStore::ranked_neighborhood` scoring path.
- Added public `RankedNeighborhoodOptions` controls for observed-at reference time, half-life, observed-at weight, and memory-node boost.
- Added shared core scoring helpers and mirrored the scoring in the SQLite recursive neighborhood query so in-memory and SQLite pruning use the same ranking model.
- Changed sibling expansion pruning to score/sort candidates before applying `max_nodes`, so recent/high-signal memory nodes survive same-depth pruning.
- Documented the scoring contract in `specs/graph.md` and moved the MemGraphRAG `#rankdefault/#58p8` note from pending to implemented.

Changed paths:
- `src/tsift/packages/tsift-core/src/lib.rs`
- `src/tsift/packages/tsift-core/src/store.rs`
- `src/tsift/packages/tsift-core/src/types.rs`
- `src/tsift/packages/tsift-sqlite/src/lib.rs`
- `src/tsift/specs/graph.md`
- `src/tsift/specs/memgraphrag-direction.md`
- `src/tsift/VERSIONS.md`

Verification:
- `tsift status`
- `cargo test -p tsift-core ranked_neighborhood_prefers_recent_memory_nodes_when_pruning -- --nocapture`
- `cargo test -p tsift-core compute_ranked_neighborhood_score_applies_decay_and_memory_signal -- --nocapture`
- `cargo test -p tsift-sqlite sqlite_ranked_neighborhood_prefers_recent_memory_nodes_when_pruning -- --nocapture`
- `cargo test -p tsift-core`
- `cargo test -p tsift-sqlite`
- `cargo test -p tsift-cli`
- `cargo test -p tsift-memgraphrag`
- `git diff --check`
- `make check`
- `gh run watch 27187123858 --exit-status` passed for `src/tsift` main CI (`clippy + test` and `live Convex graph acceptance`).
- Parent repo `gh run list --workflow CI --limit 1` reported no workflow named `CI`.

Commits:
- `src/tsift`: `8f74f0b graph: rank memory nodes in neighborhoods` pushed to `origin/main`.
- Parent repo: `0a5e25225 tsift: advance memory-aware graph ranking` pushed to `origin/main`; the current parent `HEAD` has a later unrelated commit, and the `src/tsift` pointer remains at `8f74f0b`.

Escalation: none.

Confidence: high; local workspace checks, package tests, full `make check`, and pushed CI are green.

### Re: kn0d - gpt-5

> **Queue prompt:**
>
> :pushpin: :round_pushpin: do [#kn0d]


Implemented `#kn0d`.

Summary:
- Added `GraphStore::semantic_seeded_neighborhood`, `semantic_seeded_expansion_edges`, and `nodes_by_ids` so semantic seeded graph expansion now lives behind the store API instead of the CLI-only helper path.
- Added the semantic seeded neighborhood option/result types plus shared graph edge scoring helpers in `tsift-core`.
- Added SQLite overrides for SQL-side ranked edge expansion using the existing from/to edge indexes, SQL `LIMIT` edge scan caps, exact skipped-edge accounting, and batched node lookup by id.
- Updated the CLI `graph-db semantic-seeded-neighborhood` path to delegate into the store implementation while preserving current diagnostics for missing seeds, skipped caps, and truncation.
- Added regression coverage proving edge scoring happens before caps in core, SQLite, and the CLI conformance path; updated graph/memgraphrag docs and `VERSIONS.md`.

Changed paths in `src/tsift`:
- `VERSIONS.md`
- `packages/tsift-cli/src/lib.rs`
- `packages/tsift-core/src/lib.rs`
- `packages/tsift-core/src/store.rs`
- `packages/tsift-core/src/types.rs`
- `packages/tsift-sqlite/src/lib.rs`
- `specs/graph.md`
- `specs/memgraphrag-direction.md`

Verification:
- `cargo test -p tsift-core semantic_seeded_neighborhood_scores_before_caps -- --nocapture`
- `cargo test -p tsift-sqlite sqlite_semantic_seeded_neighborhood_scores_before_sql_edge_cap -- --nocapture`
- `cargo test -p tsift-cli graph_db_semantic_seeded_neighborhood_scores_before_caps -- --nocapture`
- `cargo test -p tsift-core`
- `cargo test -p tsift-sqlite`
- `cargo test -p tsift-cli`
- `cargo test -p tsift-memgraphrag`
- `git diff --check`
- `make check`
- GitHub Actions `CI` run `27188140396` passed for the pushed `src/tsift` commit.

Commits pushed:
- `src/tsift`: `513a9e1 graph: push semantic seeded caps into store`
- parent repo: `2a350c63a tsift: advance semantic seeded graph caps`

Notes:
- `cargo fmt --check` still reports unrelated pre-existing workspace formatting drift in other files, so I did not use it as the completion gate; `make check` passed cleanly.
- Parent repo unrelated dirty files were left untouched.

Confidence: high.

### Re: next-steps prompt caching - gpt-5

> **Queue prompt:**
>
> :pushpin: What are #next-steps to improve prompt caching?


The best next step is not another broad compression pass. `session-cost` already has the useful first layer: prompt-cache analytics, ratio-drop diagnostics, creation-spike diagnostics, read/create regression warnings, and provider-specific action hints. The next work should make cache misses attributable, then make the prompt assembly honor those findings.

Recommended order:
1. Add real-session prompt-cache fixtures and a gate. Capture representative Claude/Codex/agent-doc sessions and fail closed on bad cached-input ratio, negative net cached tokens, or read/create regressions. This turns prompt caching from a recommendation into a measurable contract.
2. Emit a stable-prefix fingerprint per model call. Record hashes for the stable system/developer/tool prefix, cache key, provider, breakpoint placement, compaction boundary, and routing/affinity hint. That gives `session-cost` enough evidence to say exactly why a cache miss happened.
3. Split agent-doc prompt assembly into cached prefix and volatile suffix. Stable repo instructions, tool shape, and long-lived session state should stay byte-identical; queue heads, timestamps, pane state, compaction nudges, and one-off directives should land after the cache boundary.
4. Enforce provider adapters. Anthropic needs explicit `cache_control` breakpoints; OpenAI needs stable `prompt_cache_key` derivation; self-hosted/edge paths need same-replica routing when cache locality is replica-bound.
5. Add compact operator output. Keep the existing `prompt-cache-diagnostic` and `prompt-cache-action` lines, but add a one-line fingerprint delta summary so the operator can see whether churn came from prefix bytes, tools, key, boundary, or routing.

I added those as backlog recommendations so they can be picked up as implementation tasks.

Changed paths: `tasks/software/tsift.md` only.

Commands/evidence:
- `agent-doc preflight tasks/software/tsift.md`
- `tsift status`
- `rg -n "prompt cache|prompt-cache|prompt caching|cache_control|cache-control|ephemeral|context pack|context-pack" .`
- `tsift --envelope search "prompt cache analytics actions" --budget normal`
- `tsift --envelope source-read packages/tsift-agent-doc/src/session_cost.rs --style window --start 600 --lines 280 --budget normal`
- `tsift --envelope source-read packages/tsift-agent-doc/src/session_cost.rs --style window --start 786 --lines 94 --budget normal`

Verification: no code changes or tests were needed for this planning response.
Push: none for this question.
Escalation: none.
Confidence: high.
<!-- agent:boundary:c3a076f5 -->
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
- [ ] [#pcache-measure] Prompt caching: add real-session cache effectiveness fixtures and a fail-closed gate for cached-input ratio, net cached tokens, and read/create regressions.
- [ ] [#pcache-fingerprint] Prompt caching: emit per-call stable-prefix fingerprints plus provider/cache-key/breakpoint metadata so cache misses are attributable.
- [ ] [#pcache-boundary] Prompt caching: refactor agent-doc prompt assembly into stable cached prefix blocks and volatile suffix blocks, keeping queue/status/compaction churn after the cache boundary.
- [ ] [#pcache-adapters] Prompt caching: enforce provider-specific adapter behavior for Anthropic cache_control, OpenAI prompt_cache_key, and replica-local routing affinity.
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
