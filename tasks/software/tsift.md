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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260523-131258.md`*

Compacted content:
- Archived 1 response topic(s): SQLite graph performance next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: graph DB refresh cache, scoped snapshots, and path probes — gpt-5

Implemented #gdbrefreshcache, #gdbtargetsnap, and #gdbpathprobe.

Changed paths:
- `src/tsift/src/main.rs`
- `src/tsift/src/substrate.rs`
- `src/tsift/tests/graph_db_conformance.rs`
- `src/tsift/SPEC.md`
- `src/tsift/VERSIONS.md`
- `src/tsift/fixtures/graph-db-operator-examples/agent-orchestration-acceptance-pack.json`

What changed:
- Added cheap `GraphStore::graph_counts()` and `GraphStore::sample_edge()` probes, with SQLite `COUNT(*)` / `LIMIT 1` implementations and candidate-store parity coverage.
- Promoted source-watermark cached graph DB reuse into normal `graph-db refresh`, conflict-matrix, and dispatch-trace paths when source rows are unchanged; cached reuse stays disabled when user triggers are present so rollback/failure checks still execute.
- Replaced full-graph conflict/dispatch preparation with target-scoped snapshots built from target evidence paths, worker context, source handles, worker results, owned files/symbols, and bounded neighborhoods.
- Updated graph DB conformance tests, fixture counts, SPEC, and VERSIONS for the new behavior.

Verification:
- `cargo fmt`
- `cargo test sqlite_store_filters_edges_by_kind_and_paths`
- `cargo test --test graph_db_conformance` -> 31 passed
- `make check` -> clippy plus full cargo test passed, including 656 unit tests, 108 `exit_code` tests, and 31 graph DB conformance tests
- `cargo build`
- `cargo install --path .`
- `tsift --version` -> `tsift 0.1.42`

CI / tmux evidence:
- Checked GitHub Agent Doc run `26324160304`; both `check` and `tmux-ci` jobs failed before runner startup with the billing/spending-limit annotation and no logs. Classified as an external CI-start blocker, not a code or tmux regression.

Commit / push:
- tsift submodule commit `c1c7668` (`perf: cache scoped graph orchestration`) pushed to `btakita/tsift/main`.
- superproject submodule-pointer commit `f321eabef` (`tsift: cache scoped graph orchestration`) pushed to `btakita/agent-loop/main`.

Confidence: high; local verification and install passed, and the remote CI failure is external to this change.
Escalation: none.
<!-- agent:boundary:feddd54c -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbrefreshcache]
- do [#gdbtargetsnap]
- do [#gdbpathprobe]
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
