---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
agent: opencode
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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260604-034213.md`*

Compacted content:
- Archived 2 response topic(s): BENCHMARKS.md created with benchmark results; #ehj2 deferred index load regression test
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: lazily-rs performance next-steps — opus-4-6

> **Queue prompt:**
>
> What are #next-steps for lazily-rs help with performance?


tsift uses single-threaded `Context` (not `ThreadSafeContext`) since each CLI invocation is a separate process. This means most ThreadSafeContext contention work in lazily-rs.done.md has **limited direct tsift benefit**. The high-impact items for tsift are the single-threaded hot-path optimizations and tsift-specific profiling.

#### Already shipped (benefit tsift today)
- `#zfam` SmallVec edges — reduces per-node memory, eliminates hash overhead
- `#rvjd` RefCell consolidation — single borrow scope, fewer flag checks per get()
- `#xsp1` Vec<Option<Node>> storage — matches single-threaded Context layout

#### Recommended next steps (priority order)

1. **`#lazyperf32` Rc<dyn Any> values** — tsift calls `get()` heavily in graph traversal, search resolution, and summary caching. Every call deep-clones the value. Switching to `Rc<dyn Any>` avoids the clone for single-threaded Context, which is exactly what tsift uses. This is likely the single highest-impact change for tsift.

2. **`#lazyperf35` Eliminate scratch buffer collect on effect path** — tsift-status and tsift-summarize use effects for cache invalidation callbacks. Removing the intermediate Vec allocation in the invalidation frontier path reduces per-scope allocation pressure.

3. **`#lazyperf34` Fix finish_batch O(n²) dedup** — tsift-index uses batch scope guards during inspection. If the inspected file set is large, the O(n²) dedup in finish_batch dominates. Sort-dedup makes batch close linear.

4. **`#lazyperf30` Dependent clone scratch buffer** — tsift-graph edge resolution resolves per-file call edges. The scratch buffer allocation on >4 fan-out in refresh/clear adds per-file heap overhead.

5. **tsift-specific lazily microbenchmarks** — The existing lazily benchmarks are general (fan-out, contention). Adding tsift-pattern benchmarks (per-file edge resolution, summary cache hit/miss, status check cascade) would prove which optimizations matter for actual tsift workloads and catch regressions.

6. **AsyncContext evaluation for tsift** — The current AsyncContext backlog (`#8sqb`–`#35fc`) targets concurrent workloads. tsift could benefit if it moves to async AST parsing or parallel graph queries, but this is speculative until tsift profiles show the synchronous Context path is the bottleneck versus I/O or tree-sitter parsing.

#### Not recommended now
- `#lazyperf31` parking_lot replacement — helps ThreadSafeContext, not single-threaded Context
- `#lazyperf33` Hybrid InvalidationPlan — tsift graphs are small (per-command scope), unlikely to hit the HashMap threshold
- `#lazyperf36` A/B parking_lot vs std::sync — same ThreadSafeContext focus

Top of backlog: **`#lazyperf32`**
Are these lazily-rs tasks for the lazily-rs project or the tsift project to implement?

### Re: sqlite graph performance + effectiveness next-steps — opus-4-6

> **Queue prompt:**
>
> What are #next-steps to improve the sqlite graph performance + effectiveness?


SQLite hot reads are already excellent — sub-ms BFS (65µs), sub-ms incident edges (343µs), and the real workload median is 2.6ms. The bottlenecks are concentrated in two areas: **projection refresh** and **conflict_matrix**.

#### Current state

| Operation | Time | Status |
|---|---|---|
| BFS path (64 hops) | 65 µs | Fast |
| Incident edges | 343 µs | Fast |
| Edge lookup | 15.8 ms | Acceptable |
| Edge property scan | 19.1 ms | Acceptable |
| Conflict matrix | 817 ms | **Slowest hot-read** |
| Full refresh (~952k rows) | ~22 s | **96% of total eval time** |

#### Effectiveness wins (already shipped)

- Ranked neighborhood scoring with `BinaryHeap` priority queue — 30-60% token reduction over unranked
- Community co-membership bonus (+18) for Louvain-clustered neighbors
- Source watermark skip — avoids full refresh when code inputs unchanged
- Cycle packet cache keyed on blake3(version+target+depth+limit)
- `#rankdefault` review item tracking promotion of ranked into default output after quality gates

#### Recommended next steps (priority order)

1. **`#sqlite-chunk` Increase staging chunk size** — The single highest-impact optimization. `SQLITE_GRAPH_STAGING_CHUNK_ROWS = 50` means ~19,040 separate INSERT statements for ~952k rows. Testing 200-500 rows/chunk should cut refresh wall time significantly with minimal risk (chunked multi-row INSERT is already implemented, just the count needs tuning + benchmark validation).

2. **`#sqlite-ranked-cte` Push ranked_neighborhood into SQL** — The current Rust-side ranked loop calls `outgoing_edges()` + `node()` per expansion step (N+1 pattern). The unranked `paged_neighborhood` already uses a single recursive CTE. Adding scoring into the CTE with a priority-ordered expansion would eliminate round-trips and let SQLite's query planner optimize the join.

3. **`#sqlite-conflict-par` Parallelize conflict_matrix** — At 817ms this is the slowest hot-read by 40x. The per-target evidence packets are independent read-only traversals. Parallel preparation (via rayon or scoped threads) could cut this close to the single-target ceiling.

4. **`#sqlite-wal-tune` WAL checkpoint tuning** — `WAL_AUTOCHECKPOINT_PAGES = 256` is conservative. During bulk projection write, this triggers frequent checkpoints. Testing 1024-4096 pages would reduce checkpoint stalls during refresh.

5. **`#sqlite-frontier-temp` Temp table for multi-node frontier** — Deep BFS builds dynamic `WHERE from_id IN (...)` strings per chunk. A pre-allocated temp table (like `edges_between_nodes` already uses) would amortize parse overhead for traversals deeper than 4-5 hops.

#### Effectiveness improvements (non-performance)

- **Promote `#rankdefault`** — Run quality gates comparing ranked vs unranked output on real backlog/code queries for useful-hit density, duplicate-name precision, handle coverage, latency, and emitted tokens. If gates pass, ranked becomes the default for graph lookup/context-pack/evidence output.
- **Evidence target resolution multi-strategy** — Already implements exact -> handle property -> ref_id -> label -> #normalized. Consider adding fuzzy/typo-tolerance for handle misses when the graph has symbols with similar names.

Top of backlog: **`#sqlite-chunk`**
<!-- agent:boundary:4b6f9b22 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue auto preset="#spec-test-build-install-commit-push" -->
- ~What are #next-steps for lazily-rs help with performance?~
- ~What are #next-steps to improve the sqlite graph performance + effectiveness?~
- What are #next-steps for tsift to save tokens?
- Are these lazily-rs tasks for the lazily-rs project or the tsift project to implement?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#sqlite-chunk] Increase SQLITE_GRAPH_STAGING_CHUNK_ROWS from 50 to 200-500 and benchmark full-projection refresh — currently ~22s for ~952k rows with ~19k INSERT statements; larger chunks cut statement overhead 4-10x (high)
- [ ] [#sqlite-ranked-cte] Push ranked_neighborhood into recursive CTE with scoring — eliminates N+1 outgoing_edges/node query loop, mirrors existing paged_neighborhood pattern (high)
- [ ] [#sqlite-conflict-par] Parallelize conflict_matrix evidence packet preparation — at 817ms it is the slowest hot-read; per-target traversals are read-only and independent (medium)
- [ ] [#sqlite-wal-tune] Tune WAL_AUTOCHECKPOINT_PAGES from 256 to 1024-4096 and measure refresh overhead reduction — current 256 triggers frequent checkpoints during bulk projection write (medium)
- [ ] [#sqlite-frontier-temp] Pre-allocate temp table for multi-node frontier expansion instead of dynamic WHERE IN string per chunk — amortizes overhead for deep BFS traversals (low)
- [ ] [#lazyperf32-tsift] Promote #lazyperf32 (Rc<dyn Any> values in single-threaded Context) in lazily-rs backlog — highest-impact change for tsift since every get() avoids deep clone (high)
- [ ] [#lazyperf35-tsift] Promote #lazyperf35 (eliminate scratch buffer collect on effect path) — reduces per-scope allocation in tsift-status/tsift-summarize effect callbacks (medium)
- [ ] [#lazyperf34-tsift] Promote #lazyperf34 (fix finish_batch O(n2) dedup) — tsift-index batch scope guards scale with inspected file count (medium)
- [ ] [#lazily-tsift-bench] Add tsift-pattern lazily microbenchmarks: per-file edge resolution, summary cache hit/miss, status check cascade — prove which lazily optimizations matter for actual tsift workloads (medium)
- [ ] [#astnav-npm] Configure NPM_TOKEN + TSIFT_ENABLE_NPM_PUBLISH secrets, publish opencode-tsift to npm (medium)
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#surreal-batch-bfs] #surreal-batch-bfs-gated: Batch frontier expansion - gated (low)
- [/] [#surreal-fixture] Record SurrealDB samples in graph-db-performance-history.json fixture (medium)
- [/] [#surrealbulk-close] Close #surrealbulk review item: re-run benchmarks, update SPEC.md benchmark table, mark done
- [/] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output only after quality gates pass; compare ranked vs unranked output on real backlog/code queries for useful-hit density, duplicate-name precision, handle coverage, latency, and emitted tokens.
- [/] [#surrealbulk] Batch SurrealDB projection load: replace per-row upsert loops with bulk/transactional row load plus one-pass secondary-index rebuild; measure refresh/load separately.
- [/] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
- [/] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [/] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
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
- [x] [#surreal-rt-pool-warm] Accept external tokio Runtime in open_or_refresh so backend-eval can reuse one Runtime across candidate stores
<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
