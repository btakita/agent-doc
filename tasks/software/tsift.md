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
<!-- agent:boundary:259d47bb -->
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
- [ ] [#impactprof] [recommended] After conflict_matrix_preparation.session_review_compute dropped from 3719ms to 272ms (#gdbprephotreuse, tsift 0.1.44) and status_index_gate observability landed (#gdbprephotgate, tsift 0.1.45), conflict_matrix_preparation.impact is now the dominant single sibling at 774-810ms median. Profile impact::compute by adding sub-phase timers (mirror the session_review_compute and status_index_gate decomposition pattern), capture a 3-sample baseline under conflict_matrix_preparation.impact.<sub>, then implement the proven inner-call reduction with a metric-digest regression gate.
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
