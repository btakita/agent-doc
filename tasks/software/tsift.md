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
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
Completed: #gdbrefreshcache, #gdbtargetsnap, #gdbpathprobe, and #gdbpropscan shipped and pushed in tsift e8823da and agent-loop e8e6a8b0a; tsift CI run 26367244295 is green.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260524-200742.md`*

Compacted content:
- Archived 2 response topic(s): graph performance next steps; graph backend-eval cache
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to improve graph performance?
### Re: graph performance next steps — gpt-5

I added four follow-up items to the backlog in priority order. Top backlog item: `#gdbrefreshdelta`.

The strongest next move is refresh cost, not higher hop caps or a backend swap. The current three-sample history shows the largest remaining costs in `real.sqlite.refresh`, `real.refresh_phase.source_graph_build`, full-projection source graph build, and conflict-matrix preparation. SQLite path probes are already cheap at 128/256/512 on the real and full-projection samples, while the prototype backends are still slower on those path tiers and are held by the existing promotion gate.

Priority order:

1. `#gdbrefreshdelta` - reduce source-graph-build / refresh cost first and prove it against `fixtures/graph-db-performance-history.json`.
2. `#gdbcmcache` - make the conflict-matrix preparation cache evidence actionable, focused on `session_review_compute`, `status_index_gate`, `context_pack_diff`, and `impact`.
3. `#gdbpathpolicy` - do not raise the default max-hop cap blindly; use adaptive or opt-in high-hop behavior with 128/256/512 regression gates.
4. `#gdbengineproof` - keep FalkorDB and the other prototypes behind backend-eval until a native production adapter beats SQLite across real/full-projection operations without weakening install or locking behavior.

The existing gated review items `#gdbcacheprove` and `#gdbvacproof` still look like evidence gates rather than new backlog work.
### Re: #gdbrefreshdelta — gpt-5

Implemented and pushed the bounded graph-refresh path for task-document backend eval.

What changed:
- `src/tsift/src/main.rs` now infers a matching workspace scope from agent-doc task paths like `tasks/software/tsift.md`, so backend eval uses the `tsift` scoped index/source watermark instead of falling back to the root-wide graph.
- Session-hinted traversal projections skip global indexed call-edge materialization. The normal `--full-projection` guard still exercises the complete graph path.
- Backend-eval phase detail now records the bounded session projection decision and whether global indexed edges were skipped.
- Added focused coverage for task-path scope inference and skipped global call edges, plus updated graph-db backend eval conformance expectations.
- Updated `SPEC.md` to document the path-hinted workspace-scope behavior and full-projection guard boundary.

Performance evidence from real runs:
- Scoped real run after the index refresh produced `real_graph_rows=6786`; the first `sqlite_refresh` was `21642852us` because it cleaned up the previous root-wide graph tombstones.
- Steady-state scoped real run reused the current projection with `real_sqlite_refresh=302us`, `real_source_graph_build=302us`, and `real_projection_rows=0`.
- Cold full-projection guard still built the complete graph: `full_projection_graph_rows=953330`, `full_projection_source_graph_build=63533412us`, `full_projection_sqlite_refresh=41844703us`.
- Cached full-projection guard hit the disk cache: `full_projection_cache_hit=1`, `full_projection_source_graph_build=0`, `full_projection_projection_rows=0`.

Verification:
- `cargo test agent_doc_task_path_infers_matching_workspace_scope`
- `cargo test session_hinted_traversal_skips_global_call_edges`
- `cargo test graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite`
- `make check`
- `cargo build`
- `cargo install --path .`
- GitHub Actions `tsift` CI run `26372477579` passed both `live Convex graph acceptance` and `clippy + test`. The root repo has no workflow named `CI`. The CI job list did not include a separate tmux leg.

Pushed commits:
- `src/tsift`: `7dad614` (`perf: bound graph refresh to task scope`)
- `agent-loop`: `1e57bc565` (`tsift: bound graph refresh to task scope`)

Non-blocking CI annotations remain for GitHub Actions Node 20 deprecation and the optional live Convex graph URL configuration message.
<!-- agent:boundary:71917f49 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- do [#gdbrefreshdelta]
- do [#gdbcmcache]
- do [#gdbpathpolicy]
- do [#gdbengineproof]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbcmcache] [recommended] Convert conflict-matrix preparation cache evidence into the next optimization pass: prove cache hits across repeated CLI invocations, then reduce session_review_compute, status_index_gate, context_pack_diff, and impact timings without broadening stale ownership trust.
- [ ] [#gdbpathpolicy] [recommended] Do not raise the default path hop cap blindly; add an adaptive or opt-in high-hop policy backed by real/full-projection and synthetic deep-chain 128/256/512 samples, with a regression gate before any higher default is accepted.
- [ ] [#gdbengineproof] [recommended] Keep FalkorDB/Kuzu/DuckDB/Ladybug behind backend-eval until a native production adapter proves projection writes, freshness/parity, lock behavior, install cost, and faster real/full-projection conflict-matrix/evidence/dispatch-trace/path metrics than SQLite without weakening bundled install semantics.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#gdbcacheprove] [recommended] Run a post-fix real graph-db backend-eval --full-projection cold/cache pair on agent-loop, prove .agent-doc runtime markdown no longer changes the source watermark, prune stale pre-fix .tsift/backend-eval-cache miss artifacts, and refresh fixtures/graph-db-performance-history.json with a cache-hit sample.
- [/] [#gdbvacproof] [recommended] Run the real Convex reconciliation plus graph-db compact workflow and capture before/after file size, freelist, tombstone counts, status/doctor timings, and scan-cost deltas so compaction policy is backed by operational evidence.
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
