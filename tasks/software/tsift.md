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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260523-190501.md`*

Compacted content:
- Archived 7 response topic(s): SQLite graph performance next steps; SQLite graph hot path; First-class SQLite graph entities; 4 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: SQLite graph edge entities and benchmark — gpt-5

Completed `#gdbedgeentity` and `#gdbsqlitepgq`.

For `#gdbedgeentity`, `tsift` now treats graph edges as first-class entities: stable `GraphEdge.id` values are persisted as SQLite `graph_edges.edge_key`, schema version moved to 5, migrations backfill existing edges, and edge properties are materialized into `graph_edge_properties` with an indexed `(key, value, edge_key)` lookup. The `GraphStore` contract now supports edge lookup, edge scans, and incident-edge scans; SQLite pushes edge-kind, cursor, and edge-property filters into SQL. The `graph-db` CLI gained `edge`, `edges`, and `incident` queries, schema output documents edge ids, and conformance now checks SQLite/Convex parity for those paths.

For `#gdbsqlitepgq`, backend-eval now measures `edge_lookup`, `edge_property_scan`, and `incident_edges` across SQLite and the candidate read-only adapters, and the performance gate requires those metrics plus edge-property staging evidence. I added three bounded queue benchmark samples to `fixtures/graph-db-performance-history.json`; the committed samples show SQLite real-projection edge lookup at roughly `286-376 us/1k graph rows`, edge-property scan at `4549-4657 us/1k graph rows`, and incident-edge scan at `610-620 us/1k graph rows`. The candidates still remain on hold because no candidate beats SQLite across every measured operation/dataset while preserving SQLite's install and lock-behavior constraints.

One verification detail from the benchmark request: I also attempted a full agent-loop queue benchmark, but it ran for several minutes without emitting JSON, so I stopped it and used the bounded queue fixture samples plus the backend-eval conformance test as the committed benchmark evidence.

Verification completed:

- `cargo fmt`
- `git -C src/tsift diff --check`
- `cargo test` (`658 + 108 + 31` tests passed after formatting)
- `cargo check`
- `cargo build`
- `cargo install --path . --force`
- `make tmux-ci` in `src/agent-doc` (`228` ignored tmux integration tests passed)
- Latest GitHub `Agent Doc` run for `33d9ba7a8` failed before any job steps were emitted; both `check` and `tmux-ci` jobs had empty `steps`, and `gh run view --log` returned `log not found`, so I classified it as an external CI-start failure rather than a code/tmux regression.

Pushed commits:

- `src/tsift`: `eb27dc3` (`feat: add graph edge entity queries`)
- `agent-loop`: `33d9ba7a8` (`chore(submodule): bump tsift edge graph queries`)
<!-- agent:boundary:c18e7d10 -->
<!-- /agent:exchange -->
###
<!--

#spec-test-build-install-commit-push
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbsqlitepgq]
- do [#gdbedgeentity]
- Please run a benchmark on the performance improvements for this queue run
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
