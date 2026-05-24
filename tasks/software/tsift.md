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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260524-141331.md`*

Compacted content:
- Archived 3 response topic(s): sqlite graph performance next steps; inactive queue residue bug; graph performance backlog next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:aca7dcc9 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbrefreshfast]
- do [#gdbedgescan]
- do [#gdbhopcaps]
- do [#gdbfalkorgate]
- do [#gdbpropscan]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbpropscan] [recommended] Isolate the slow SQLite edge_property_scan path from #gdbedgescan: reproduce the latest roughly 3s fixture, capture EXPLAIN plus row-count/cardinality evidence, then prove any materialized edge-property index or scan-shape change with backend-eval and metric-digest before broader backend work.
- [ ] [#gdbcoldfull] [recommended] Split and reduce full-projection cold rebuild cost: profile full_projection.source_graph_build and projection_rows separately, cache or incrementally rebuild unchanged session/source/summary rows, and prove the change with three backend-eval --full-projection samples plus metric-digest.
- [ ] [#gdbprepwm] [recommended] Stabilize conflict-matrix preparation cache reuse: key prepared context/evidence/staged-diff/impact packets by source/document/staged-diff watermarks, exclude .tsift/conflict-matrix-cache artifacts from source watermarks, and gate on repeated conflict_matrix_preparation/dispatch_trace timings.
- [ ] [#gdbhopguard] [recommended] Keep hop-cap increases behind path-tier guardrails: add or refresh 64/128/256/512-hop high-fanout/deep-chain fixtures with truncation and query-plan checks before raising user-facing defaults.
- [ ] [#gdbbackendgate] [recommended] Re-evaluate FalkorDB/Kuzu only after SQLite hot paths are measured: require native projection writes/load, parity, lock behavior, install portability, and full-projection wins before considering backend promotion.
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
