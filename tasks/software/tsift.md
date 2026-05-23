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
Completed: #gdbrefreshcache, #gdbtargetsnap, and #gdbpathprobe shipped in tsift c1c7668 and superproject f321eabef; GitHub Agent Doc CI is externally blocked before runner startup by the billing/spending-limit annotation.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260523-222948.md`*

Compacted content:
- Archived 2 response topic(s): sqlite graph performance next steps; gdbfalkorreal location
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:5c741580 -->
<!-- /agent:exchange -->
###
<!--
#spec-test-build-install-commit-push
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbcurbase]
- do [#gdbprepcache]
- do [#gdbfullreg]
- do [#gdbfalkorreal]
- Please run a benchmark on the performance improvements for this queue run
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbcurbase] [recommended] Capture the current post-c1c7668 SQLite graph baseline with three backend-eval samples: refresh the workspace/index first, run the bounded tasks/software/tsift.md target and the opt-in full-projection dataset, feed output through metric-digest, update fixtures/graph-db-performance-history.json, and record the top costs before another optimization pass.
- [ ] [#gdbprepcache] [recommended] Remove the remaining conflict-matrix preparation hotspot by making prepared context, evidence, staged-diff, and impact packets reusable across CLI invocations by source/document/staged-diff watermark, then gate on backend-eval conflict_matrix_preparation and dispatch_trace timings.
- [ ] [#gdbfullreg] [recommended] Add a full-projection graph performance regression gate: keep the small session-hinted run as smoke coverage, but require opt-in full agent-loop metric-digest evidence before changing SQLite graph query plans or evaluating backend promotion.
- [ ] [#gdbfalkorreal] [recommended] Keep FalkorDB behind backend-eval and only promote it after a production adapter beats SQLite on full-projection conflict-matrix, evidence, dispatch-trace, path_max_hops, install portability, and lock behavior; current bounded numbers are promising for conflict-matrix but not enough for a backend switch.
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
