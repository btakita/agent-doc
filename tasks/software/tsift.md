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

Completed: #gdbpostbase, #gdbrowstream, #gdbprepstage, and #gdbenginegate shipped in tsift c0ebfd5 and superproject 851d29aad; #gdbvacproof is gated on missing live Convex transport credentials after local dry-run and compact proof.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260523-044812.md`*

Compacted content:
- Archived 4 response topic(s): graph performance benchmark normalization; queue closeout repair; evidence resolver latency; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to make sqlite graph faster?
### Re: SQLite graph performance next steps — gpt-5

I added three prioritized backlog items. The top backlog item is `#gdbrefreshcache`.

The reason that is first: the current code already has the main SQLite query optimizations in place for property filters, bounded neighborhoods, evidence target resolution, and frontier path search. The remaining measured cost is mostly refresh and broad graph loading. The latest checked-in fixture shows real SQLite refresh around `24.4s`, with `source_graph_build` around `14.2s`, `sqlite_delta_write` around `8.0s`, and `sqlite_property_row_staging` around `2.1s`; evidence itself was around `25ms`, so chasing evidence first would not move the main wall-clock number.

Priority order:

1. `#gdbrefreshcache` — reuse the source-watermark cached projection path outside backend-eval so normal refresh/conflict-matrix can skip unchanged source graph and SQLite staging work.
2. `#gdbtargetsnap` — stop conflict-matrix/dispatch-trace from loading the entire graph when a target-scoped evidence/ownership snapshot is enough.
3. `#gdbpathprobe` — remove full-edge/full-row scans from backend-eval probe selection so status/path metrics measure the operation being tested, not setup scans.

On max-hop caps: I would not raise them globally yet. Keep the cap conservative until `#gdbpathprobe` proves path measurement is clean and any higher cap is backed by synthetic deep-chain plus real repeated-sample metrics.

On FalkorDB: it stays behind the backend promotion gate. It is worth evaluating, but not as the next lever, because current real costs are dominated by projection build/write and broad snapshot loading; a different graph backend only earns promotion if it preserves SQLite parity and beats SQLite across the measured operations without losing SQLite's bundled install and lock-behavior advantages.

Changed paths: `tasks/software/tsift.md` only. Commands: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `tsift status --json`, targeted `rg`/`sed` source inspection, `git status --short`, and this `agent-doc finalize` with backlog mutations. Verification: no code changed, so I did not run the Rust test suite. Escalation: none. Confidence: high for backlog ordering; medium-high for performance prioritization because it is based on existing fixture metrics and source inspection, not a fresh benchmark run.
<!-- agent:boundary:a8249f1d -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbrefreshcache] [recommended] Promote backend-eval source-watermark cached projection reuse into normal graph-db refresh/conflict-matrix paths: when code-index, session, and summary watermarks match, skip source_graph_build/projection_rows/sqlite temp/property staging and open the existing graph.db read-only; prove with three backend-eval samples covering real.sqlite.refresh, source_graph_build, sqlite_delta_write, and conflict-matrix timings.
- [ ] [#gdbtargetsnap] [recommended] Make conflict-matrix and dispatch-trace target-scoped instead of loading every graph_nodes/graph_edges row: build the snapshot from resolved targets, evidence paths, owned files/symbols, and direct conflict neighborhoods; keep full-graph snapshots behind an explicit diagnostic path and gate on real.sqlite.conflict_matrix plus dispatch_trace normalized metrics.
- [ ] [#gdbpathprobe] [recommended] Remove full-edge/full-row scans from backend-eval probe selection by adding GraphStore count and sample-edge APIs backed by SQLite COUNT(*)/LIMIT 1 queries, then retarget path_max_hops/status metrics so they measure SQLite graph operations rather than all_nodes/all_edges setup cost.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
