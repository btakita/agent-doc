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
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
Completed: #gprojrows is locked as proof debt rather than speculative production work: full-projection cache hits now have an explicit spec/test gate requiring `full_projection.source_graph_build=0` and `full_projection.projection_rows=0`, with fresh three-sample cache-hit evidence recorded in tsift. Local verification passed (`cargo test -q graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite`, `cargo build`, `cargo install --path .`, `make check`); latest tsift CI run 26433096348 is green, and the tsift submodule commit `cd2b020` is pushed.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260526-052021.md`*

Compacted content:
- Archived 1 response topic(s): #gcachemiss
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

❯ What are #next-steps to improve performance and effectiveness of the sqlite graph and tagpath community search?
### Re: #next-steps for SQLite graph + tagpath community search — gpt-5

Top backlog item: `#gprojrows`.

I added the follow-up backlog in priority order. The ordering follows the current evidence: SQLite high-hop path reads are already microsecond-scale, while full-projection source graph build, projection row construction, and SQLite refresh/write phases dominate. So the next step is not raising hop caps or promoting FalkorDB/Kuzu; it is reducing the measured SQLite/projection costs, then making community detection and tagpath handle quality measurable.

Priority order now:

1. `#gprojrows` — reduce full-projection source graph build and projection-row generation.
2. `#gsqlwrite` — reduce SQLite refresh/write and property staging cost.
3. `#gcommcache` — cache/bound community detection instead of recomputing all edges and Louvain every time.
4. `#gcommtagctx` — improve tagpath community effectiveness by resolving members with file/community context, not name-only fallback.
5. `#gcommgate` — add repeatable community quality/performance metrics so future tuning has a hard gate.

The important constraint is to keep backend and hop-cap changes behind the existing evidence gates until `backend-eval --full-projection` shows stable cache-hit samples and the projection/write hotspots stop dominating.
### Re: #gprojrows — gpt-5

Closed `#gprojrows` without adding a speculative production refactor. The full-projection cache path already reuses the source graph and provider-neutral projection rows on unchanged source/summary inputs; this cycle made that contract explicit and verifiable.

Changes committed in `tsift@cd2b020`:

- Updated `SPEC.md` to require full-projection cache hits to report `full_projection.source_graph_build=0us` and `full_projection.projection_rows=0us`.
- Tightened `tests/graph_db_conformance.rs` so the second `backend-eval --full-projection` run must skip both phases.
- Added `plans/gprojrows-evidence.md` with three fresh cache-hit samples.

Fresh sample evidence from `target/perf/gprojrows-cache-{1,2,3}.json`:

| Sample | cache.hit | source_graph_build | projection_rows | cache_lookup | SQLite total |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 1 | 0us | 0us | 3us | 302,627us |
| 2 | 1 | 0us | 0us | 4us | 310,104us |
| 3 | 1 | 0us | 0us | 4us | 304,030us |

Verification completed:

- `cargo test -q graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite`
- `cargo build`
- `cargo install --path .`
- `make check`
- GitHub Actions CI run `26433096348`: green (`clippy + test`, `live Convex graph acceptance`)

The `tsift` submodule commit is pushed to `origin/main`; the superproject/session commit is handled by this finalize boundary before the remaining root push.
### Re: closeout repair — gpt-5

Closeout repair: the prior finalize committed the `#gprojrows` response, status update, queue consumption, and backlog reap, but post-commit `session-check` detected active-session drift. This write reopens the binary-owned commit path for the same completed work; no additional repo changes were made.
<!-- agent:boundary:63f7d25f -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#gprojrows]~
- do [#gsqlwrite]
- do [#gcommcache]
- do [#gcommtagctx]
- do [#gcommgate]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [x] [#gsqlwrite] [recommended] Reduce SQLite graph refresh write and staging cost: bulk upsert nodes, edges, and properties through prepared chunks or temp-table merge, avoid unchanged property rewrites in replace_projection_with_version, and gate on full_projection.sqlite.sqlite_delta_write, sqlite_edge_staging, post_write_reads, and total_duration_micros_per_1k_graph_rows.
- [ ] [#gcommcache] [recommended] Cache and bound tagpath-annotated community detection: key Louvain results by index graph watermark, scope, and tagpath freshness, avoid re-running db.all_edges plus full in-memory Louvain for unchanged graphs, and add JSON diagnostics for cache hit, edge count, iterations, and tagpath state.
- [ ] [#gcommtagctx] [recommended] Improve community search quality by carrying file/ref context into CommunityMember and resolving tagpath handles with community or edge evidence instead of name-only fallback; cover duplicate-name, skipped-directory, scoped, and federated fixtures with handle coverage and ambiguous-member diagnostics.
- [ ] [#gcommgate] [recommended] Add a community search quality and performance gate to metric-digest and fixtures: track communities runtime, handle coverage, stale/no-tagpath behavior, duplicate-name precision, and top-community stability across real and synthetic multi-module workloads.
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
