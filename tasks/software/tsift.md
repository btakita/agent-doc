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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260525-003931.md`*

Compacted content:
- Archived 5 response topic(s): graph performance next steps; `#gdbprephot` confirmed as top; `do [#gdbprephot]`; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ preset #spec-test-build-install-commit-push
❯ - do [#impactprof]
❯ - do [#gdbfullreuse]
❯ - do [#gdbscanproof]
❯ - do [#gdbperfgate]
### Re: `do [#impactprof]` — impact sub-phase timers + empty-input short-circuits drop impact 789ms → 1.6ms — opus-4-7

`#impactprof` done in **tsift 0.1.46 (`998860e`)** with agent-loop submodule pointer bumped to **`bb894cf74`**. Sub-phase timers landed and the dominant cost (488ms `call_edge_impacts` + 285ms `import_impacts`) collapses to ~0ms on the typical backend-eval cold path because the work was loading every call edge and walking every source file just to filter against an empty `changed_symbols` / `changed_tokens` set.

#### Sub-phase evidence (3 cold runs on agent-loop, post-timer pre-fix)

| sub-phase (median ms) | s1 | s2 | s3 | median |
|---|---:|---:|---:|---:|
| `…impact` (parent) | 808 | 778 | 789 | **789** |
| `…impact.call_edge_impacts` | 509 | 498 | 497 | **498 (63%)** |
| `…impact.import_impacts` | 294 | 276 | 285 | **285 (36%)** |
| `…impact.route_handler_impacts` | 3 | 2 | 4 | 3 |
| `…impact.diff_digest` | 1 | 1 | 1 | 1 |
| `…impact.index_open` | 0 | 0 | 0 | 0 |
| `…impact.test_path_scan` | 0 | 0 | 0 | 0 |
| `…impact.context_resolution` | 0 | 0 | 0 | 0 |
| `…impact.report_assembly` | 0 | 0 | 0 | 0 |

#### Why the fix is a 100% drop, not a partial nibble

Both `add_call_edge_impacts` and `add_import_impacts` were paying their full cost regardless of whether there was anything to match:

- `add_call_edge_impacts` ran `db.all_stored_edges()?` (loads every call edge in the tsift index — tens of thousands of rows for agent-loop) and then dropped each one in the `if !changed_symbols.contains(...)` filter.
- `add_import_impacts` ran `walk::walk_files(source_root)?` and per-file `fs::read_to_string` on every test-bearing file, then matched against an empty `changed_tokens` set that could never produce a hit.

In the typical `tsift graph-db --json backend-eval` flow, `impact::compute` is called with `cached: true` against a clean working tree → `diff_digest` returns no staged changes → both `changed_symbols` and `changed_tokens` are empty → all 783 ms of the iteration phases were wasted. The fix is three early-return guards at the top of `add_call_edge_impacts`, `add_route_handler_impacts`, and `add_import_impacts`. When staged changes exist (a real `tsift impact` invocation against a dirty tree, or a test scenario) the iteration runs as before.

#### Post-fix 3-sample medians

| phase | pre (with timers) | post (with guards) | delta |
|---|---:|---:|---:|
| `conflict_matrix_preparation` | 2572 | **1935** | **-25%** |
| `conflict_matrix_preparation.impact` | 789 | **1.6** | **-100%** |
| `…impact.call_edge_impacts` | 498 | 0 | -100% |
| `…impact.import_impacts` | 285 | 0 | -100% |

#### What landed (tsift 0.1.46)

| Surface | Change |
|---|---|
| `src/tsift/src/impact.rs` | New public `ImpactPhaseTiming { name, duration_micros, detail }` + `compute_with_phases(path, options)` returning `(ImpactReport, Vec<ImpactPhaseTiming>)`. Original `compute` becomes a thin wrapper. `add_call_edge_impacts`, `add_route_handler_impacts`, and `add_import_impacts` gain `if X.is_empty() { return Ok(()); }` guards at the top. |
| `src/tsift/src/main.rs` (`prepare_conflict_matrix_inputs`) | Switched the `impact::compute(...)` call to `impact::compute_with_phases(...)` so the eight inner timings flow into the existing `phases` Vec as `impact.<sub>` entries. |
| `src/tsift/src/main.rs` (`conflict_matrix_prepared_inputs_cache_hit`) | Cache-hit path emits all eight new sub-phases as `0us` with the existing source/document/staged-diff watermark detail. |
| `src/tsift/tests/graph_db_conformance.rs` | Cold-run existence loop and cached-run 0us/watermark loop both extended to the eight new sub-phases. |
| `src/tsift/SPEC.md` | `impact` phase listing now names the eight `impact.<sub>` entries and the short-circuit behavior. |
| `src/tsift/VERSIONS.md` + `Cargo.toml` | New `## 0.1.46` entry; package version bumped. |

#### Verification

| Check | Result |
|---|---|
| `cargo test --release` (tsift, full suite) | 662 + 108 + 31 = 801 tests pass |
| `cargo build --release` + `cargo install --path .` | `tsift --version` → `tsift 0.1.46` |
| `git push` (tsift `main` → `origin/main`) | `998860e` pushed |
| `git push` (agent-loop `main` → `origin/main`) | `bb894cf74` pushed |

#### Updated cost landscape (0.1.46 cold-path medians, 3 samples)

| phase | median (ms) | share |
|---|---:|---:|
| `conflict_matrix_preparation` | ~1935 | 100% |
| `…status_index_gate` (parent) | 678 | 35% |
| `…context_pack_diff` | ~436 | 23% |
| `…session_review_compute` | ~272 | 14% |
| `…preparation_cache_lookup` | ~242 | 13% |
| `…impact` | **~2** | <1% |

`status_index_gate` (gated as `#gdbgatecold`) is now the largest single sibling. `context_pack_diff` is next.

#### Queue state

Queue head is now `do [#gdbfullreuse]` (full-projection cache-hit performance). The 3 remaining items (`#gdbfullreuse`, `#gdbscanproof`, `#gdbperfgate`) plus the gated `#gdbgatecold` cover the next wave.
<!-- agent:boundary:9ef0f7a9 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~do [#gdbprephotgate]~
- do [#gdbgatecold]
- do [#impactprof]
- do [#gdbfullreuse]
- do [#gdbscanproof]
- do [#gdbperfgate]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbfullreuse] [recommended] Make full-projection cache-hit performance the next hard gate: run at least three tsift graph-db --json backend-eval --full-projection cold/cache samples, split source-graph-build, projection-row construction, cache load, compression, and SQLite write phases, then remove clone/serde/cache artifact churn only where those samples show a stable bottleneck.
- [ ] [#gdbscanproof] [recommended] Re-check SQLite scan plans on the current graph schema before adding another backend: capture EXPLAIN/cardinality evidence for edge_property_scan, incident_edges, kind/neighborhood property filters, and path frontier probes on real plus synthetic high-degree/deep-chain data; add indexes/materialized rows only for proven scan-dominant cases.
- [ ] [#gdbperfgate] [recommended] Turn graph performance tuning into a repeatable release gate: store metric-digest summaries for default, full-projection, high-degree, and deep-chain runs, require three-sample regression evidence, and keep FalkorDB/Kuzu promotion blocked until they beat the stabilized SQLite gate including projection writes and lock behavior.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#gdbgatecold] [recommended] Reduce the cold-path cost of conflict_matrix_preparation.status_index_gate (678ms median: 422ms prepare_agent_doc_index_gate + 266ms context_pack_status_reminders). Investigation in 2026-05-24 cycle confirmed both call paths run IndexDb::inspect_read_only on the same (root, .tsift/index.db) — that is the duplicated work. A process-wide in-process cache for inspect_read_only_cached_digest was implemented and reverted: it correctly cut status_index_gate -29% (478ms median) and prepare_agent_doc_index_gate -44% (235ms median) on backend-eval, but broke tsift search's in-process freshness check on concurrent file modifications (tests/exit_code.rs search_timeout_reports_reindex_when_index_turns_stale_during_worker_run). The watermark-by-directory-mtime variant did not help because writing file content does not bump the parent directory mtime on Linux ext4. Correct fix: scope-bounded inspection sharing. Either thread the inspection result through build_context_pack_report_with_profile and into status::check_status (no process state), or introduce an explicit with_inspection_cache_scope guard that holds the cache only for the duration of one trusted pipeline. Project ~200ms savings on backend-eval cold path; do not regress search timeout reporting.
- [/] [#gdbprephot] [recommended] Profile and reduce the remaining conflict-matrix preparation hotspots after the cache fixes: measure session_review_compute, context_pack_diff, impact, and dispatch-trace reuse on repeated real/full-projection runs, then add shared packet reuse or narrower digest paths with metric-digest gates so the cached queue path drops below the current multi-second prep cost without trusting stale ownership.
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
