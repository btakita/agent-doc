---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_model: opus
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
Completed: #gdbret, #gdbbfs, #gdbpush, and #gdbdelta shipped in tsift bd402c5 and superproject ea61810cb; SQLite graph refreshes are delta-based, graph reads push batching/filtering into SQLite, tombstone retention diagnostics are surfaced, and both repos are pushed.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260522-040703.md`*

Compacted content:
- Archived 1 response topic(s): graph performance next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:d4f563e3 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbperfbase]
- do [#gdbbenchhist]
- do [#gdbpathbatch]
- do [#gdbpropidx]
- do [#gdbcompact]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbrefreshhot] [recommended] Profile and split the real graph-db refresh cost by phase: session/markdown scan, source-handle construction, semantic summary reads, SQLite delta writes, tombstone pruning, and conflict-matrix preparation. Use the last backend-eval evidence as the target: refresh was 125.2s of a 145.8s real run, so refresh remains the first performance bottleneck to attack.
- [ ] [#gdbmetricreal] [recommended] Replace the placeholder graph performance history fixture with real post-batch backend-eval metrics, then make metric-digest/CI compare current real + synthetic graph runs against that checked-in history so regressions are visible before another tuning pass.
- [ ] [#gdbtracehot] [recommended] Profile and optimize the post-refresh operator read path, especially dispatch-trace and conflict-matrix. Last recorded read-side timings were dispatch-trace 10.9s, conflict-matrix 3.34s, evidence 0.057s, and status 6.2s inside backend-eval; current standalone status is fast, so isolate backend-eval/prepared-input overhead from query overhead.
- [ ] [#gdbhopcaps] [recommended] Evaluate higher or adaptive max-hop caps for graph-db path/evidence/dispatch workflows. Prove with synthetic deep-chain and real session fixtures that larger caps preserve recursive-CTE bounds, deterministic ordering, truncation warnings, and query-plan diagnostics before raising defaults.
- [ ] [#gdbcompactrun] [recommended] Run the operational Convex reconciliation + refresh + compact sequence on the real agent-loop graph.db, then record file-size, freelist, tombstone scan, and status/doctor timing deltas. Current status is current but still recommends compaction: 169,109 tombstones, 1.136 GB graph.db, and 135.8 MB freelist.
- [ ] [#gdbfalkorgate] [recommended] Add FalkorDB only as a backend-eval candidate behind the GraphStore promotion gate, then compare it against SQLite on parity, real refresh-equivalent load, evidence, conflict-matrix, dispatch-trace, synthetic high-degree/deep-chain runs, install portability, and lock behavior before considering any backend switch.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->
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
