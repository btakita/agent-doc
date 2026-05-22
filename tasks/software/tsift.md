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
Completed: #gdbret, #gdbbfs, #gdbpush, and #gdbdelta shipped in tsift bd402c5 and superproject ea61810cb; SQLite graph refreshes are delta-based, graph reads push batching/filtering into SQLite, tombstone retention diagnostics are surfaced, and both repos are pushed.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260522-014020.md`*

Compacted content:
- Archived 1 response topic(s): SQLite graph DB optimization batch
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are the #next-steps to improve sqlite graph performance?
### Re: SQLite graph performance next steps — gpt-5

Top backlog item: #gdbperfbase.

I added the next performance work in this priority order:

1. #gdbperfbase: refresh and migrate the real agent-loop `.tsift/graph.db`, then capture the baseline numbers before more tuning. This is first because current `graph-db status` still reports a stale schema-v1 graph missing the new `row_hash` and `source_watermark` columns, so performance data against that file is not clean.
2. #gdbbenchhist: make graph performance measurement repeatable and bounded across the real projection plus synthetic high-degree/deep-chain graphs, with metric history suitable for regression review.
3. #gdbpathbatch: batch evidence shortest-path and worker-context path expansion so evidence/conflict workflows do not regress into per-pair path queries.
4. #gdbpropidx: only promote JSON-property filters into materialized indexed rows or columns after query-plan and benchmark evidence shows JSON extraction scans are the dominant cost.
5. #gdbcompact: add a safe compaction policy for tombstone-heavy graph stores after Convex reconciliation, since current status reports 1,025,144 tombstones versus 929,474 live rows and a 605 MB graph DB.

Evidence gathered this cycle: `tsift graph-db --path /home/brian/work/btakita/agent-loop --json status` completed and showed the stale schema/tombstone numbers above. I also tried `backend-eval --target #orchdog`; it produced no output after roughly 90 seconds, so I stopped it and treated bounded/progress-visible benchmarking as part of the follow-up work. No code changed, so I did not run the project test suite.
<!-- agent:boundary:34c5b4b0 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbret]
- do [#gdbbfs]
- do [#gdbpush]
- do [#gdbdelta]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbperfbase] [recommended] Refresh and migrate the real agent-loop `.tsift/graph.db` through the new SQLite graph schema, then capture a before/after performance baseline for refresh, status, doctor, evidence, conflict-matrix, dispatch-trace, file size, freelist bytes, live rows, tombstones, and pruned stale-live tombstones.
- [ ] [#gdbbenchhist] [recommended] Make graph performance measurement repeatable and bounded: add a checked-in metric history or fixture around `graph-db refresh/status/evidence/backend-eval` on the real projection plus synthetic high-degree and deep-chain graphs, then route output through `metric-digest` for regression review.
- [ ] [#gdbpathbatch] [recommended] Batch evidence shortest-path and worker-context path expansion across many source handles with one multi-target recursive CTE per target family, preserving max-hop caps and deterministic ordering while avoiding per-pair path queries.
- [ ] [#gdbpropidx] [recommended] Promote hot graph JSON-property filters into a materialized property table or dedicated columns only after query-plan and benchmark evidence shows JSON extraction scans dominate kind/neighborhood filters.
- [ ] [#gdbcompact] [recommended] Add a safe post-Convex-reconciliation graph compaction policy for tombstone-heavy stores, including prune, rebuild, or VACUUM guidance plus status/doctor checks that prove file-size and scan-cost improvements.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
