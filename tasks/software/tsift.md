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
❯ Run the benchmark to assess the performance improvements in the last queue run.

❯ The duplicate content corrupt document bug occurred. Do we need more logic to prevent the full-document ipc?
❯ #spec-test-build-install-commit-push
### Re: benchmark and IPC payload guard — gpt-5

Ran the graph backend benchmark and added the IPC payload guard coverage.

Benchmark result:
- The first full superproject run against `/home/brian/work/btakita/agent-loop` measured the cold/current path: real SQLite total `27,515,453us`, refresh `26,953,139us`, conflict matrix `520,183us`, dispatch trace `2,380us` on `946,233` real graph rows.
- The gate-required repeated cached samples showed the intended improvement from the last queue run: real SQLite refresh was `317us`, `322us`, `320us`; real SQLite total was `519,721us`, `517,194us`, `538,326us`; dispatch trace stayed under `1ms` in all three samples.
- `metric-digest` versus `fixtures/graph-db-performance-history.json` reported the latest repeated sample as `real.sqlite.total_duration_micros` improved by `98.16%`, `real.sqlite.refresh.duration_micros` improved by `99.9987%`, and `real.refresh_phase.source_graph_build.duration_micros` improved by `99.9977%`.
- Candidate backend promotion remains on hold. The repeated digest still showed candidate conflict-matrix regressions (`falkordb` +83.5%, `ladybug` +66.8%, `duckdb-duckpgq` +64.4%), and the promotion gate still requires SQLite parity plus lower time for every measured operation.

Answer on the duplicate-content corruption: I do not think we need a broad new ban on full-document IPC. The narrower invariant is the right one: template response IPC must use component patches whenever explicit patches exist or unmatched response text can be synthesized into an `exchange`/`output` patch. `fullContent` should remain reserved for no-component append-mode fallbacks and operator-owned replacements such as Compact Exchange, with source-buffer proof. I made that contract explicit and added a regression that captures the file-IPC JSON payload for a synthesized exchange patch and proves it omits `fullContent`, clears consumed `unmatched`, and keeps `normalize_prefix_lines` in the patch payload.

Changed paths:
- `src/agent-doc/SPEC.md`
- `src/agent-doc/src/write.rs`
- `src/agent-doc` submodule pointer in the superproject

Verification:
- `tsift graph-db --json backend-eval`
- `tsift graph-db --path /home/brian/work/btakita/agent-loop --json backend-eval`
- `for sample in 1 2 3; do tsift graph-db --path /home/brian/work/btakita/agent-loop --json backend-eval; done`
- `tsift metric-digest --input /tmp/tsift-backend-eval-agent-loop-3samples-20260523.ndjson --baseline fixtures/graph-db-performance-history.json --json`
- `git -C src/agent-doc diff --check`
- `cargo fmt`
- `cargo test file_ipc_synthesized_exchange_patch_omits_full_content`
- `make check`
- `cargo build`
- `cargo install --path .`
- `agent-doc --version` -> `agent-doc 0.33.16`
- `make tmux-ci`

CI / tmux evidence:
- GitHub CI run `26334294637` completed successfully; both `Run make check` and `Run make tmux-ci` passed.
- Local `make tmux-ci` passed: 228 ignored/tmux integration tests, 0 failures.

Commit / push:
- agent-doc submodule commit `f9a74962` (`test: guard template ipc payload shape`) pushed to `btakita/agent-doc:20260422/codex-agent-backend`.
- superproject commit `0f1df940c` (`agent-doc: guard template ipc payload shape`) pushed to `btakita/agent-loop:main`.

Preflight warning carried forward: one legacy gated item still lives in `agent:backlog`; I did not migrate it because it was unrelated to this cycle.

Confidence: high.
Escalation: none.
<!-- agent:boundary:76c1983d -->
<!-- /agent:exchange -->
###
<!--

#spec-test-build-install-commit-push
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
