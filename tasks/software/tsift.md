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
Completed: #impactdag and #dagfix shipped in tsift ed28437 and superproject 189dcac98; graph orchestration now exposes scheduler fields and regenerates dependency-DAG replay fixtures.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260522-010840.md`*

Compacted content:
- Archived 1 response topic(s): sqlite graph optimization next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:23451336 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbret] [recommended] Add SQLite graph tombstone retention, compaction, and doctor diagnostics: current graph.db status reports more tombstones than live rows, so define a safe prune or rebuild policy that preserves Convex tombstone semantics and tracks file-size/query impact.
- [ ] [#gdbbfs] [recommended] Batch SQLite graph traversal reads for evidence, conflict-matrix, and dispatch-trace: replace per-node outgoing_edges/node lookup loops with frontier queries or recursive CTEs, preserve max-depth/max-hops semantics, and lock parity against the real #orchdog packet.
- [ ] [#gdbpush] [recommended] Push graph-db kind/neighborhood cursor pagination and property filters into the SQLite GraphStore query layer instead of hydrating full node sets and filtering in memory; add query-plan assertions before adding JSON-property indexes or materialized property rows.
- [ ] [#gdbdelta] [recommended] Replace full SQLite graph projection refresh with a measured delta refresh: persist per-row content hashes/source watermarks, skip unchanged node/edge writes, delete via temp anti-join tables, and prove backend-eval refresh timing improves on the current roughly 929k-row projection without breaking drift/doctor/Convex parity.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
