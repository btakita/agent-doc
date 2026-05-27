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
### Re: #gsemrows — gpt-5
Implemented `#gsemrows` and marked it done.

What changed:
- `graph-db refresh/status --json` now includes a semantic `readiness` object. Empty summary caches report `status=blocked`, `fail_closed=true`, `reason=summary_cache_empty`, diagnostics, and next commands for `tsift summarize --extract ...` plus graph refresh.
- `communities --json` now includes `community_diagnostics.tagpath_readiness`. Missing or stale tagpath state is blocked/actionable; fresh tagpath state is ready.
- Added conformance coverage that starts from empty semantic rows, seeds a current summary cache, runs `tsift summarize --extract main.rs --json`, refreshes graph-db, and proves neighborhood/evidence/conflict-matrix semantic ranking improves.
- Updated `SPEC.md` for the new readiness contracts.

Commits pushed:
- `btakita/tsift@95044d5` `feat: surface graph effectiveness readiness`
- `btakita/agent-loop@0238f5662` `chore: update tsift graph readiness`

Verification:
- `cargo check`
- Targeted tests for communities readiness and graph semantic readiness
- `make check`
- `cargo install --path .`
- GitHub Actions `CI` run `26487656719` completed `success` for the pushed tsift commit

I left the pre-existing unrelated dirty files in `src/tsift` unstaged.
### Re: #gnbrank — gpt-5

Implemented `#gnbrank`.

`tsift graph-db neighborhood --json` now emits additive `ranked_neighbors` entries for returned neighborhood nodes. The ranking scores neighbors by traversal depth, edge kind, community co-membership signal, semantic relation signal, fresh source-handle evidence, duplicate-name precision, and page handle coverage.

The existing `nodes` output remains stable node-id ordered for cursor pagination. I added a `neighborhood_ranking_gate` that records `ranked_output_default=false` and ties any future default-order change to the community-search quality gate: real plus synthetic_multi_module workloads, handle coverage, duplicate-name precision, stale/no-tagpath behavior, top-community stability, and latency regression thresholds.

Updated coverage:

- `src/main.rs` unit coverage for ranked neighborhood fields and the ranking gate.
- `tests/graph_db_conformance.rs` CLI coverage for neighborhood JSON and SQLite/Convex parity.
- `tests/exit_code.rs` coverage that the community-search gate fixture exposes the quality metrics consumed by the neighborhood default-change gate.
- `SPEC.md` documents additive ranked output and the default-order hold.

Verification:

- `cargo check` passed.
- `cargo test graph_db_api_queries_sqlite_neighborhood_and_schema` passed.
- `cargo test --test graph_db_conformance graph_db_cli_conformance_matches_sqlite_and_convex_snapshot_queries` passed.
- `cargo test --test graph_db_conformance graph_db_semantic_readiness_clears_after_summarize_extract_and_refresh` passed.
- `cargo test --test exit_code metric_digest_reports_community_search_gate_fixture` passed.
- `make check` passed.
- `cargo install --path .` succeeded.
- GitHub Actions `CI` run `26488163004` passed for tsift commit `b4bba5d`: `clippy + test` finished green in 2m48s and `live Convex graph acceptance` finished green in 59s. The run only reported the existing Node.js 20 deprecation/config annotations.

Commits pushed:

- `btakita/tsift` `b4bba5d` (`feat: rank graph neighborhoods`)
- `btakita/agent-loop` `67d8d612d` (`chore: update tsift neighborhood ranking`)
### Re: #gtombops — gpt-5

Completed the graph tombstone cleanup workflow against `src/tsift/.tsift/graph.db` and recorded the evidence in `src/tsift/plans/gdbvacproof-evidence.md`.

What ran:

- Captured baseline `graph-db status`, `doctor`, compact dry-run, file size, page/freelist, and tombstone counts under `/tmp/gtombops/`.
- Started a temporary self-hosted Convex backend, deployed the graph app, and ran `tsift convex-sync . --remote-snapshot --apply --chunk-size 50 --json`.
- Verified the remote snapshot as current before pruning; after `graph-db refresh`, verified again that `local_hash == snapshot_hash` with zero missing/stale rows.
- Ran `tsift graph-db --path . --json compact --apply --prune-tombstones --confirmed-convex-reconciled`.
- Captured post-prune status/doctor/file-size/page/tombstone evidence, then removed the generated `.env.local`, stopped the temporary container, and removed the temporary Docker volume.

Key results:

| Metric | Queue baseline | Reconciled pre-prune | Post-prune |
| --- | ---: | ---: | ---: |
| Live rows | 27,375 | 27,562 | 27,562 |
| Tombstone rows | 184,722 | 198,840 | 0 |
| Tombstone scan rows | 184,722 | 198,840 | 0 |
| File size | 73,248,768 bytes | 76,115,968 bytes | 37,093,376 bytes |
| Doctor | ok, retention warning | ok, retention warning | ok, no retention warning |
| Compaction policy | recommended | recommended | not_needed |

The compact report pruned 198,840 tombstones and reclaimed 39,022,592 bytes from the reconciled pre-prune state. From the queue-prompt baseline, the graph DB ended 36,155,392 bytes smaller with the tombstone scan cost cleared.

Commits pushed:

- `src/tsift`: `0d19076` (`docs: record graph tombstone cleanup evidence`)
- root: `60ed884c0` (`chore: update tsift tombstone evidence`)

Verification:

- `git diff --check -- plans/gdbvacproof-evidence.md` passed.
- GitHub Actions `CI` run `26488560277` passed: `clippy + test` in 3m02s and `live Convex graph acceptance` in 1m00s. The run only showed the existing Node.js 20 deprecation/configuration annotations.
<!-- agent:boundary:385abc00 -->
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
- ~do [#gsemrows]~
- ~do [#gnbrank]~
- ~do [#gtombops]~
- do [#gbenchneigh]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
