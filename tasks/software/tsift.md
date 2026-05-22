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
queue_active: true
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260522-222729.md`*

Compacted content:
- Archived 1 response topic(s): graph performance next steps
- Prior summary/contet: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: graph performance queue — gpt-5

Completed the graph performance queue items that were locally actionable.

Implemented in `src/tsift`:
- Graph refresh now streams materialized `graph_node_properties` rows while node rows are staged, avoiding the extra `json_each(next_graph_nodes.properties_json)` pass over the staged projection.
- Context-pack exploration rows now use one batched SQLite projection transaction for `source_handle`, `worker_context`, and relationship rows instead of per-row autocommit writes.
- Added regression coverage for the batched SQLite projection upsert path and updated README/SPEC/VERSIONS plus `fixtures/graph-db-performance-history.json` with the fresh real agent-loop baseline.

Commits:
- `src/tsift` `c0ebfd5` (`perf: batch graph projection property writes`)
- superproject `851d29aad` (`tsift: batch graph projection writes`)

Performance proof from real root `backend-eval` after installing the rebuilt CLI:
- Immediate before -> after: SQLite refresh `28.02s -> 24.41s`, source graph build `16.59s -> 14.20s`, projection rows `2.86s -> 2.24s`, SQLite delta write `8.57s -> 7.97s`, conflict prep `6.09s -> 4.80s`, dispatch trace `1.58s -> 1.49s`, path_max_hops `1.10s -> 1.03s`, SQLite total `33.00s -> 29.20s`.
- Candidate totals after: DuckDB/DuckPGQ `6.53s`, FalkorDB `6.17s`, Ladybug `6.41s`; all remain `hold` because real parity differed from SQLite, and Ladybug also missed the SQLite `path_max_hops` time.
- Metric digest against the prior fixture shows candidate totals improved about 87%, while SQLite still records a larger-source-graph regression versus the older baseline; the new run is now recorded as `real-agent-loop-batched-projection-2026-05-22`.

Compaction / Convex proof:
- Local Convex dry-run planned `351703` node upserts, `586611` edge upserts, and `9385` chunks; no network mutation was attempted.
- Real `convex-sync --remote-snapshot --apply` is externally blocked because neither `--endpoint` nor `TSIFT_CONVEX_GRAPH_URL` is configured in this shell.
- `graph-db compact --apply` reclaimed storage locally: file size `1012051968 -> 975118336` bytes and freelist `618496 -> 0`; tombstones remain retained pending Convex reconciliation.

Verification:
- Focused tests for projection refresh, batched projection upsert, and context-pack graph materialization passed.
- `make check` passed after the final fixture update: `653` unit tests passed (`1` ignored), `108` exit-code tests passed, and `30` graph-db conformance tests passed (`1` live Convex test ignored).
- `cargo install --path . --locked` replaced the local `tsift` binary; `tsift --version` reports `0.1.42`.
- Latest `btakita/tsift` GitHub `CI` before this new push was green (`26314205965`). Latest root `Agent Doc` run `26315225081` failed with empty `check` and `tmux-ci` job step lists and `gh run view --log-failed` returned `log not found`, so I classified it as the same external CI-start blocker rather than a code or tmux regression.

Per agent-doc closeout ordering, the local commits are ready and the pushes follow after this document response is finalized.
<!-- agent:boundary:907871ed -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#gdbpostbase]~
- do [#gdbrowstream]
- do [#gdbprepstage]
- do [#gdbvacproof]
- do [#gdbenginegate]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
