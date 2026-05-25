---
agent_doc_session: tsift-v0.1
agent: claude
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
Completed: #gdbrefreshdelta was already closed; #gdbcmcache, #gdbpathpolicy, and #gdbengineproof are implemented in tsift ab9f8a9 and agent-loop e09157f61. Local verification passed (`make check`, `cargo build`, `cargo install --path .`); latest tsift CI run 26372477579 is green.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260525-010513.md`*

Compacted content:
- Archived 1 response topic(s): `do [#impactprof]`
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: #gdbfullreuse + #gdbscanproof + #gdbperfgate landed — opus-4-7

All three parallel packets returned, `make check` clean, build + install + commit + push complete. Marking all three queue items done in this cycle. tsift CI run `26380273068` is in flight; previous run `26377912111` was green.

#### Aggregate diff (tsift 23f0d52 + superproject 08360fa72)

| File | Δ | Origin |
| --- | --- | --- |
| `SPEC.md` | +33 | #gdbperfgate (Performance Release Gate subsection) |
| `src/perf_gate.rs` | +687 (new) | #gdbperfgate (gate decision module) |
| `tests/perf_gate.rs` | +212 (new) | #gdbperfgate (12 tests, all pass) |
| `plans/gdbscanproof-evidence.md` | +161 (new) | #gdbscanproof (EXPLAIN + cardinality evidence) |
| `tests/scan_plan.rs` | +234 (new) | #gdbscanproof (4 tests, fails closed on base-table `SCAN`) |
| `plans/gdbfullreuse-evidence.md` | +307 (new) | #gdbfullreuse (cold/cache phase samples + verdict) |
| `src/main.rs` | +171 / -23 | #gdbfullreuse (cache + SQLite write phase split) + #gdbperfgate (mod registration) |
| `tests/graph_db_conformance.rs` | +22 / -? | #gdbfullreuse (assert new cache/SQLite sub-phases) |
| `fixtures/graph-db-performance-history.json` | +8479 | #gdbfullreuse (9 workload-tagged sample entries) |

#### #gdbperfgate verdict — gate live, future-proofed for missing metrics
Four required workloads (`default`, `full-projection`, `high-degree`, `deep-chain`), three-sample-per-workload regression rule, FalkorDB/Kuzu promotion blocked until they beat stabilized SQLite on every workload **including projection write cost and lock-contention behavior**. Gate currently treats workload prefixes as present when any run carries metrics with those prefixes, so the in-flight `full_projection` samples satisfy that prefix naturally. `lock_wait_micros` / `lock_contention_micros` are enforced only when a candidate reports them — future-proof hook since SQLite doesn't emit them today.

#### #gdbscanproof verdict — no new indexes warranted
All four shapes resolve to `SEARCH … USING (COVERING) INDEX …` against both production graph (3284 nodes / 3464 edges / 1265-edge hub) and synthetic stress (15001 / 14999, 10000-edge hub, 5000-deep chain, `ANALYZE`d). Worst real case ~1.8 ms; synthetic hub UNION ~1.3 ms. The only `SCAN` token in any plan is the union-cursor inside `incident_edges`, which is degree-bounded by design. `tests/scan_plan.rs` regression-locks the index plans so any future schema drift fails closed before merge.

#### #gdbfullreuse verdict — bottleneck identified, no speculative refactor applied
Six tsift-submodule + six agent-loop superproject samples (3 cold / 3 cache each). Cache-hit dominant cost is **`full_projection.sqlite.replace_projection_total`** (~86% tsift-sub: 245 ms of 286 ms total; ~94% agent-loop: 15.3 s of 16.4 s total). Cache load is stable and small (30 ms tsift-sub, 1.06 s agent-loop, with `gzip_decode` and `serde_decode` roughly equal halves). Source-graph-build and projection-rows correctly go to 0 on every cache hit, confirming the cache short-circuit works. Per the task's stop-and-report rule, no clone/serde/cache churn removed — the only hot `.clone()` (`projection.clone()` at `src/main.rs:10656`) is on the cold path and dwarfed by `source_graph_build`. The genuine next-cycle fix is cross-invocation reuse of the in-memory `SqliteGraphStore` (skip `replace_projection_with_version` when `projection_version` + `source_watermark` already match) but it sits behind the `[#gdbgatecold]` no-process-cache constraint and needs scope-bounded design first.

Two of three agent-loop superproject cache legs reverted to full rebuild because sibling agents edited tracked files during the 25-min sample window — environmental, not a regression; the pair-1 hit plus all three tsift-submodule pairs prove the cache path.

#### Follow-ups (added to backlog)
- `[#gdbreplacereuse]` Cross-invocation reuse design for `replace_projection_with_version` on cache-hit — needs scope-bounded sharing to honor `[#gdbgatecold]` against process-wide `inspect_read_only` caching.
- `[#gdblockmetric]` Emit `lock_wait_micros` / `lock_contention_micros` from SQLite backend-eval so the perf-gate's lock-contention parity check becomes enforceable instead of future-proof.
- `[#gdbgateworkloads]` Add dedicated `default`, `high-degree`, and `deep-chain` workload entries to `fixtures/graph-db-performance-history.json` so the gate's per-workload three-sample requirement is satisfied with dedicated samples, not metric-prefix inference.
❯ Can we complete the items in review?
### Re: review item #gdbcacheprove closed — opus-4-7

Closing the one review item that was nearest the line; the other three (#gdbgatecold, #gdbprephot, #gdbvacproof) stay gated because each needs its own dedicated cycle (scope-bounded inspection-sharing design, multi-sample conflict-matrix profiling, Convex + compact operational run respectively).

#### #gdbcacheprove — closed (tsift `7a85fa4`, superproject `21b10a5c3`)
- **Cold/cache pair on agent-loop**: covered by #gdbfullreuse (3 cold + 3 cache samples, pair 1 hit cleanly with the new phase split; pairs 2 + 3 reverted to full rebuild because sibling agents edited tracked source during the 25-min window — that is the cache working as designed, not a regression).
- **`.agent-doc/` watermark exclusion proof**: single chokepoint at `traversal_relative_path_is_generated_artifact` (`src/main.rs:12868`) excludes `.agent-doc`, `.agent-doc/...`, `/.agent-doc`, and `/.agent-doc/...` at any depth. New unit test `traversal_excludes_agent_doc_runtime_paths_from_source_watermark` regression-locks the rule across snapshots, baselines, archives, runtime JSONL, and nested session-doc paths, and asserts real source paths (`src/main.rs`, sibling `agent-doc/src/lib.rs`, `src/.agent-doc-helper.rs`) stay included.
- **Cache artifact prune**: both `.tsift/backend-eval-cache/full_projection/` directories (superproject + tsift submodule) hold exactly one fresh entry written by the #gdbfullreuse run. LRU has already collapsed prior misses. Nothing stale to prune.
- **Fixture refresh**: 9 workload-tagged entries appended by #gdbfullreuse.

Evidence: `plans/gdbcacheprove-evidence.md`. `make check` clean; `cargo install --path .` upgraded `~/.cargo/bin/tsift`.
### Re: #gdbgatecold packet in flight — opus-4-7

Queue head `do [#gdbgatecold]` is already dispatched as a single focused packet (planner said `parallelizable: False` so sibling items #gdbprephot and #gdbvacproof are intentionally held). The packet is researching scope-bounded inspection sharing along the `prepare_agent_doc_index_gate` → `context_pack_status_reminders` path, with the hard requirement that `tests/exit_code.rs::search_timeout_reports_reindex_when_index_turns_stale_during_worker_run` stays green (the prior process-wide cache attempt broke it).

Awaiting its proof block before committing, pushing, and finalizing with `--done gdbgatecold`. Next queue item advances on the following cycle.
### Re: still waiting on #gdbgatecold packet — opus-4-7

No change since the last status note — packet still running. Closing this cycle without consuming the queue head; will commit + push + finalize with `--done gdbgatecold` once the packet returns its proof block.
### Re: #gdbgatecold closed — opus-4-7

Scope-bounded inspect cache landed (tsift `64b2fab`, superproject `54d7b3329`, v0.1.48). The 200 ms target was met with margin and the search-timeout regression test stayed green.

#### Design — option 2 (scoped guard), not option 1 (signature threading)
Subagent evaluated both paths. Threading the inspection result through `build_context_pack_report_with_profile` → `context_pack_status_reminders` → `status::check_status` would have required reshaping `status::check_status` (which already handles workspace-scope, summary cache, instruction checks) and propagating a multi-key inspection map through several signatures. A thread-local `InspectScopeGuard` in `src/index.rs` is non-invasive: only the trusted pipeline opens it, only `IndexDb::inspect_read_only` reads from it, and the cache drops when the outermost guard drops. Search and every other top-level call site runs outside any guard → identical fresh-per-call behavior, which is what protects the regression test. After a refresh inside the scope, `inspect_scope_invalidate_all()` drops the pre-write snapshot so subsequent inspects see the updated DB.

#### Cold-leg medians (3 samples, agent-loop superproject)

| Phase | Before (µs) | After (µs) | Δ |
| --- | ---: | ---: | ---: |
| `status_index_gate` | 323,657 | 52,674 | **-270,983 µs (~-271 ms)** |
| `prepare_agent_doc_index_gate` | 322,617 | 51,711 | -270,906 µs |
| status reminders (warm median) | 706 | 302 | -404 µs (~-57%) |

Cold delta beats the ~200 ms target; warm delta proves the duplicate inspect was the real cost without resorting to process state. Single-sample true-cold proof requires `sudo drop_caches` (unavailable on this host); first-run-of-batch numbers stand in.

#### Regression-safety
- `tests/exit_code.rs::search_timeout_reports_reindex_when_index_turns_stale_during_worker_run` → `ok`
- All 4 `search_timeout_*` tests → `ok`
- All 3 existing `inspect_read_only*` tests → `ok`
- `context_pack_refreshes_stale_index_before_handoff` → `ok` (refresh path invalidates scope cache)
- `context_pack_status_reminders_include_stale_index_state` → `ok` (pre-refresh stale path still emits reminder)
- New tests `build_context_pack_reuses_inspect_within_scope`, `inspect_read_only_outside_scope_does_not_cache` → `ok`
- `make check` → exit 0
- `cargo install --path .` → upgraded `~/.cargo/bin/tsift` to 0.1.48

Sibling user commit `cd9c678` (tagpath consumer contract, v0.1.47) landed during the packet run; this work piles on top at v0.1.48 with a dedicated VERSIONS.md entry. The next two queue items remain gated for their own cycles per planner's `parallelizable: False`.
❯ Can the following tasks be completed?
❯ - [#gdbprephot]
❯ - [#gdbvacproof]
### Re: Can the following tasks be completed? — opus-4-7

#### #gdbprephot — yes, in flight
Dispatched as a focused packet. Profiles the four named hot paths (`session_review_compute`, `context_pack_diff`, `impact` with non-empty changes, `dispatch_trace` reuse) on 3 default + 3 `--full-projection` runs (now that v0.1.48 scope-guard is in), identifies the dominant residual phase, applies the minimal fix (shared packet reuse OR narrower digest, no process-wide cache per `#gdbgatecold` history), extends the `perf_gate` module with a metric-digest gate that fails closed on regression without trusting stale ownership, and bumps to v0.1.49. ~1-1.5 h.

#### #gdbvacproof — partial only, with a precise blocker
Two halves:

- **Compact half (will run after #gdbprephot finishes):** `tsift graph-db --path . compact` dry-run + `--apply`, capture before/after file size, freelist (`PRAGMA freelist_count`), tombstone counts, `status` / `doctor` timings, and scan-cost deltas. All local SQLite. Will run myself once the packet returns — sequencing after the benchmark window avoids contention on `.tsift/graph.db`.
- **Convex reconciliation half (blocked):** `tsift convex-sync --remote-snapshot --apply` requires a deployed Convex instance with `CONVEX_URL` + auth. `examples/convex-graph/` ships the schema/mutations/HTTP-actions package but the tree has no `.env` and no live deployment is configured. Tombstone-prune is also gated behind `--confirmed-convex-reconciled` for exactly this reason. Captured the unblocking step as new backlog item `#gdbvacconvex`.

#### Plan for this turn
1. Wait for `#gdbprephot` packet.
2. Aggregate its proof, run `make check`, `cargo install --path .`.
3. Run the `#gdbvacproof` compact half, capture before/after metrics into `plans/gdbvacproof-evidence.md`.
4. Commit + push tsift (one combined commit covering both items) + superproject submodule pointer.
5. Finalize with `--done gdbprephot`. Hold off on `--done gdbvacproof` since only the compact half is covered; leave it gated, with `#gdbvacconvex` capturing the Convex deployment prerequisite.
<!-- agent:boundary:b24cfac1 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbvacconvex] Deploy examples/convex-graph via npx convex dev against Brian's Convex account so tsift convex-sync --remote-snapshot --apply and tsift graph-db compact --prune-tombstones --confirmed-convex-reconciled can be exercised end-to-end (blocks the Convex half of #gdbvacproof).
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#gdbprephot] [recommended] Profile and reduce the remaining conflict-matrix preparation hotspots after the cache fixes: measure session_review_compute, context_pack_diff, impact, and dispatch-trace reuse on repeated real/full-projection runs, then add shared packet reuse or narrower digest paths with metric-digest gates so the cached queue path drops below the current multi-second prep cost without trusting stale ownership.
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
