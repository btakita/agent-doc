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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260523-012319.md`*

Compacted content:
- Archived 4 response topic(s): sqlitedb graph performance next steps; SQLite graph performance queue; ignored tests and performance report; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are the #next-steps to improve the sqlite graphdb performance?
### Re: SQLite graph performance next steps — gpt-5

The next pass should move above the SQLite write path. The last queue already completed `#gdbrefreshperf`, `#gdbpathfrontier`, `#gdbstatscache`, and `#gdbresolveidx`; the archived benchmark says those changes cut SQLite delta writes and property staging, but total real runtime still regressed slightly because `source_graph_build` grew and now dominates the run.

I added these follow-ups in priority order:

1. `#gdbsourcebuild` — reduce source graph construction first: markdown/session scanning, source-handle construction, semantic summary reads, and projection row construction.
2. `#gdbprepreuse` — stop rebuilding shared graph orchestration preparation across backend-eval, conflict-matrix, dispatch-trace, and metric runs.
3. `#gdbbenchnorm` — normalize graph performance history by graph row counts and use repeated samples before treating small deltas as real.
4. `#gdbevidlat` — re-measure the indexed evidence resolver, since the last run showed a small evidence latency regression despite the resolver becoming more structurally correct.

Top backlog item: `#gdbsourcebuild`.

I would still keep backend replacement work behind the existing promotion gate. FalkorDB/Kuzu/DuckDB only become relevant after SQLite's current hot path is cheaper and the benchmark still shows an external backend beating SQLite across real refresh-equivalent load, evidence, conflict-matrix, dispatch-trace, synthetic high-degree/deep-chain cases, install portability, and lock behavior.

Changed paths: intended session document/backlog only. Commands: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, targeted `rg`/`sed`/`git status`/`git log` inspection, and `agent-doc finalize`. Verification: no code tests were run because this cycle changed no code; the recommendation is based on the archived real backend-eval report and the current tsift spec/done records. Escalation: none. Confidence: medium-high; the direction is clear, but the next implementation should start with fresh repeated metrics because the last report was one run against a growing graph.
<!-- agent:boundary:1340ba8b -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
- do [#gdbrefreshperf]
- do [#gdbpathfrontier]
- do [#gdbstatscache]
- do [#gdbresolveidx]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbsourcebuild] [recommended] Reduce source_graph_build as the next SQLite graph performance bottleneck: profile markdown/session scanning, source-handle construction, semantic summary reads, and projection row construction, then cache or incrementally rebuild unchanged portions before more backend replacement work.
- [ ] [#gdbprepreuse] [recommended] Remove repeated graph orchestration preparation from backend-eval and operator read paths by persisting or reusing shared evidence, worker_result/source_handle expansion, semantic rows, and dispatch-trace inputs across conflict-matrix, dispatch-trace, and metric runs.
- [ ] [#gdbbenchnorm] [recommended] Normalize graph performance history by graph row counts and run repeated backend-eval samples before judging small regressions, so source_graph_build growth, evidence latency noise, and candidate backend deltas are not mistaken for durable wins or losses from one real projection run.
- [ ] [#gdbevidlat] [recommended] Re-measure the indexed graph-db evidence resolver with repeated real and synthetic runs, because the last benchmark showed evidence target resolution moving from about 24.9ms to 36.5ms; keep the indexed lookup path only if repeated metric-digest output proves it is neutral or faster under the current graph size.
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
