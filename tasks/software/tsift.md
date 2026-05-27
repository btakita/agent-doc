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

Answered why the session-share tsift DB appeared stale. No source changes; refreshed indexes with tsift status --fix and confirmed session-share is now fresh while workspace remains stale only because the FDE-Project-Interview scope is missing.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260527-021317.md`*

Compacted content:
- Archived 3 response topic(s): graph + neighborhood next steps; stale index auto-update; session-share stale index
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: #gneighgate — gpt-5

Implemented and closed `#gneighgate`.

What changed:
- Reworked SQLite `paged_neighborhood` so node paging and page-edge materialization share one recursive reachable-set CTE instead of repeating the bounded walk.
- Added a stable page diagnostic: `neighborhood nodes and page edges share one recursive reachable-set CTE`.
- Added backend-eval `neighborhood` operation coverage and required performance-gate metrics for real, synthetic high-degree, synthetic deep-chain, and opt-in full-projection workloads.
- Updated the spec and conformance tests to lock SQLite query-plan diagnostics plus SQLite/Convex snapshot parity.

Commits/pushes:
- tsift submodule: `9cd12f3 perf: gate graph neighborhood paging`, pushed to `btakita/tsift:main`.
- agent-loop parent pointer: `d9474229f chore: update tsift neighborhood gate`, pushed to `btakita/agent-loop:main`.

Verification:
- `cargo check`
- `cargo test --test graph_db_conformance graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite`
- `cargo test --test graph_db_conformance graph_db_cli_conformance_matches_sqlite_and_convex_snapshot_queries`
- `cargo test --test graph_db_conformance graph_db_scale_caps_pagination_paths_doctor_and_sqlite_plans`
- `make check`
- `cargo install --path .`
- GitHub Actions CI run `26487042618`: success for `clippy + test` and `live Convex graph acceptance`.

No `tmux-test` workflow exists in either the root `agent-loop` repo or the `tsift` submodule, so there was no tmux CI leg to inspect. Unrelated dirty files in the root checkout and in the `tsift` submodule were left unstaged.
### Re: closeout repair — gpt-5

Binary closeout repair: the `#gneighgate` implementation response was already committed, and this follow-up write only reopens the agent-doc write/commit path so the post-closeout queue and `(HEAD)` annotations are not left as an interrupted cycle.
<!-- agent:boundary:e60937a0 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#gneighgate]~
- do [#gsemrows]
- do [#gnbrank]
- do [#gtombops]
- do [#gbenchneigh]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gsemrows] Make semantic rows and tagpath handles part of graph effectiveness: when `graph-db refresh` warns the summary cache is empty or `communities` reports `tagpath_state=missing`, surface a fail-closed/actionable readiness state, then run `summarize --extract` + refresh and prove neighborhood/evidence ranking improves.
- [ ] [#gnbrank] Add ranked neighborhood output: score neighbors by depth, edge kind, community co-membership, semantic relation, source-handle freshness, duplicate-name precision, and handle coverage; gate with community-search quality metrics before changing defaults.
- [ ] [#gtombops] Run and document the graph tombstone cleanup workflow on the tsift graph DB: Convex reconcile, refresh, compact, then record status/doctor/file-size/tombstone-scan deltas from the current 184,722 tombstone / 27,375 live-row state.
- [ ] [#gbenchneigh] Refresh the graph performance history with dedicated neighborhood rows so future graph-backend or hop-cap decisions compare `neighborhood`, `path_max_hops`, evidence, conflict-matrix, and dispatch-trace together instead of optimizing one surface blind.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
