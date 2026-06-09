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
queue: start
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
<!-- agent:boundary:9fbaa693 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~do [#kx6s]~~
- ~~do [#qszc]~~
- do [#qs5v]
- do [#zkft]
- do [#58p8]
- do [#kn0d]
- What are #next-steps to improve prompt caching?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
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
