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

Completed: #gdbrwbatch, #gdbprepfast, #gdbcandpush, #gdbpathbudget, and #gdbperfgate shipped in tsift aaac32a and superproject e25a1c50b; graph refresh now delta-upserts materialized property rows, conflict-matrix reuses a symbol-by-file graph index, experimental backend adapters use indexed outgoing/kind traversal, backend-eval reports adaptive path budgets plus performance gates, and promotion stays held until every measured operation beats SQLite with parity.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260522-214606.md`*

Compacted content:
- Archived 1 response topic(s): graph backend eval gates
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are the #next-steps to improve graph performance?
### Re: graph performance next steps — gpt-5

Top backlog item: `#gdbrwbatch`.

I added the next graph-performance backlog in priority order. The order follows the current fixture: real SQLite is still the best production baseline at about 24.7s total; its refresh is the largest remaining cost at about 19.6s, with SQLite delta writes at about 12.9s and conflict-matrix preparation at about 5.2s. FalkorDB is not a promotion candidate yet: its refresh-equivalent load is faster, but real conflict-matrix is about 40.4s and total runtime is about 49.5s, roughly 2x SQLite.

The next pass should attack write batching first, then repeated operator preparation, then candidate backend query pushdown. Hop-budget tuning and performance regression gates should follow once those hot paths stop dominating.
### Re: graph performance queue — gpt-5

Completed and pushed the five queued graph-performance items.

Changed paths: `src/tsift/src/substrate.rs`, `src/tsift/src/main.rs`, `src/tsift/tests/graph_db_conformance.rs`, `src/tsift/SPEC.md`, `src/tsift/README.md`, plus the `src/tsift` submodule pointer in the superproject.

What changed:
- `#gdbrwbatch`: SQLite refresh no longer deletes and reinserts all `graph_node_properties`; it now delta-upserts/deletes materialized property rows and reports property upsert/unchanged/delete counts.
- `#gdbprepfast`: conflict-matrix construction now builds a symbol-by-file graph index once per store instead of scanning all graph nodes per target.
- `#gdbcandpush`: experimental backend-eval row adapters now keep node-kind and outgoing-edge indexes and batch reachable-kind traversal, removing the worst full-edge-scan behavior.
- `#gdbpathbudget`: backend-eval now publishes explicit adaptive path probe config: 64-hop deep-chain coverage and 1-hop direct/high-degree probes.
- `#gdbperfgate`: backend-eval now emits a `performance_gate` section with the baseline fixture, CI-safe synthetic profile, opt-in real profile, 10% allowed regression threshold, required metrics, and metric-digest command; promotion now requires parity plus lower time for every measured operation on every dataset.

Verification:
- Initial multi-filter `cargo test ...` command failed because Cargo accepts only one positional test filter; reran the focused filters separately and all passed.
- `cargo test graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite` passed.
- `cargo test sqlite_projection_refresh_tracks_versions_watermarks_and_tombstones` passed.
- `cargo test sqlite_projection_refresh_handles_bulk_row_replacement` passed.
- `make check` passed: clippy, 652 unit tests with 1 ignored, 108 exit-code tests, and 30 graph conformance tests with 1 ignored live Convex test.
- `cargo build` passed.
- `cargo install --path .` replaced the local `tsift`; `tsift --version` reports `0.1.42`.
- Real backend-eval plus metric-digest ran against `tasks/software/tsift.md` and the checked-in history. Selected deltas: SQLite delta write improved from 12.94s to 7.91s, SQLite refresh improved from 19.60s to 14.61s, and SQLite total improved from 24.72s to 19.06s for the target run.

CI/tmux evidence: GitHub `Agent Doc` run `26314220159` for the superproject push failed with empty step lists for both `check` and `tmux-ci` after about three seconds. I classified this as the existing external CI-start blocker, not a code or tmux regression; local verification above is clean.

Commits pushed: tsift `aaac32a` (`perf: tighten graph backend evaluation`) and superproject `e25a1c50b` (`chore: update tsift graph performance`). No escalated commands were used.
<!-- agent:boundary:c9e9ae87 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbrwbatch]
- do [#gdbprepfast]
- do [#gdbcandpush]
- do [#gdbpathbudget]
- do [#gdbperfgate]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
