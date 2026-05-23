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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260523-002555.md`*

Compacted content:
- Archived 1 response topic(s): kuzu backend-eval prototype
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to improve the sqlitedb graph performance?
### Re: sqlitedb graph performance next steps — gpt-5

I would keep the next pass SQLite-first. The current spec and fixture already show SQLite has the right correctness/install/concurrency story, and the read-path bottlenecks are narrower than a full backend replacement. The fixture points at refresh/write cost as the main remaining problem: real SQLite refresh is around 24-29s, with `sqlite_delta_write` and source graph construction dominating, while evidence is already in the tens of milliseconds and conflict-matrix is under roughly a second after the batched projection work.

I added these backlog items in priority order:

1. `#gdbrefreshperf` — profile and optimize the SQLite refresh/write path first.
2. `#gdbpathfrontier` — replace recursive string-path shortest-path traversal with a bounded frontier strategy and explicit metric gates.
3. `#gdbstatscache` — cache refresh-produced operator counts/compaction inputs so status/doctor avoid repeated full scans.
4. `#gdbresolveidx` — push evidence target resolution into indexed SQLite lookups instead of Rust-side family scans.

I would not make FalkorDB/Kuzu the next move yet. The backend-eval gate is correctly strict: a replacement has to preserve SQLite parity, bundled install simplicity, and multi-process writer/read behavior while beating SQLite on every measured dataset and operation. Right now the lower-risk path is to make SQLite's hot paths cheaper, then rerun `tsift graph-db --path . backend-eval --json | tsift metric-digest --baseline fixtures/graph-db-performance-history.json` to see whether an external backend still has a real job to do.

Top backlog item: `#gdbrefreshperf`.

Proof: changed only the session document/backlog through `agent-doc write --commit` repair after the initial finalize stream carried pending additions but no response body. Commands used: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, targeted `rg`/`sed` inspection, `agent-doc finalize`, and `agent-doc write --commit`. Verification: no tests were run because this was a response/backlog update only. Escalation: none. Note: preflight also reported the existing legacy gated backlog item should eventually be migrated into `agent:review`.
<!-- agent:boundary:8771cf6a -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbrefreshperf] [recommended] Profile and optimize the SQLite graph refresh/write path against the real agent-loop queue fixture: split source_graph_build, projection_rows, sqlite_delta_write, property-row staging, and conflict_matrix_preparation timings; reduce sqlite_delta_write and total refresh before evaluating a replacement backend.
- [ ] [#gdbpathfrontier] [recommended] Replace SQLite shortest-path recursive string-path traversal with a bounded frontier strategy: prove a bidirectional or temp-frontier implementation on synthetic deep-chain/high-degree fixtures, keep --max-hops caps, and add EXPLAIN/metric-digest gates for path_max_hops.
- [ ] [#gdbstatscache] [recommended] Cache graph-db operator stats produced during refresh, including node/edge/tombstone counts, file/freelist proof, and compaction policy inputs, so status/doctor can avoid repeated full-table/tombstone scans while preserving fail-closed schema and freshness checks.
- [ ] [#gdbresolveidx] [recommended] Push graph-db evidence target resolution into indexed SQLite queries instead of scanning node families in Rust: resolve exact node id first, then use materialized property lookups for handle/ref_id plus kind+label filters, and keep parity with Convex snapshot resolution.
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
