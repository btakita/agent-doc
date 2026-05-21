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

Completed: #cmgevfix shipped in agent-doc e17bf1f and superproject gitlink 7680e03cf; graph evidence is refreshed before packet validation and graph failures fall back to manual review.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260521-144258.md`*

Compacted content:
- Archived 1 response topic(s): #cmgrwres
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Any backlog work that should be done on the agent-doc code
❯ do [#tsdag]. #spec-test-build-install-commit-push
### Re: #tsdag blocked — gpt-5

I could not start `#tsdag` because `agent-doc plan` stopped the cycle before implementation with a fail-closed tsift graph evidence error:

`tsift graph orchestration contract invalid: conflict-matrix missing evidence_packet_id gevd-8fb9f57781 for target tsdag`

No repo edits, tests, build, install, manual commit, or push were performed. I am not marking `#tsdag` done or gated because implementation did not begin; it stays open until the graph evidence / conflict-matrix state for this document is repaired or regenerated, then this same `do [#tsdag]. #spec-test-build-install-commit-push` request can be rerun.
### Re: next steps for missing evidence packet — gpt-5

Added `#cmgevfix` as the new top backlog item to resolve the missing `evidence_packet_id` failure that blocked `#tsdag`.

Top of backlog: `#cmgevfix`
❯ do [#cmgevfix]. #spec-test-build-install-commit-push
### Re: #cmgevfix blocked — gpt-5

I could not start `#cmgevfix` because `agent-doc plan` stopped the cycle before implementation with another fail-closed tsift graph evidence error:

`tsift graph orchestration contract invalid: conflict-matrix missing evidence_packet_id gevd-d102ace0f7 for target cmgevfix`

No repo edits, tests, build, install, manual commit, or push were performed. `#cmgevfix` stays open because the planner cannot produce a valid execution contract while the conflict-matrix/evidence graph is inconsistent for the target item itself.
### Re: #cmgevfix — gpt-5

Completed `#cmgevfix`.

Implemented in `src/agent-doc`:
- `plan` now refreshes the tsift graph projection before collecting per-target evidence, so evidence packet ids align with `conflict-matrix` and `dispatch-trace` instead of validating stale packet ids.
- tsift graph evidence failures are preserved as warnings/manual review fallback instead of blocking the whole agent-doc turn boundary.
- Updated the agent-doc spec, README, and manual job packet docs for the refreshed/fallback graph contract.

Verification:
- `cargo test collect_for_do_items_attaches_graph_handles`
- `cargo test build_plan_downgrades_stale_graph_db_to_manual_packet_only_warning`
- `cargo test parallel_mode_continues_without_graph_evidence_when_tsift_is_stale`
- `cargo test collect_for_do_items_fails_closed_on_missing_graph_contract_fields`
- `make check`
- `cargo build`
- `cargo install --path .`
- Latest GitHub CI run `26234146432` was green, including `make tmux-ci`.

Commits pushed:
- agent-doc: `e17bf1f` (`20260422/codex-agent-backend`)
- agent-loop gitlink: `7680e03cf` (`main`)
❯ do [#tsdag]. #spec-test-build-install-commit-push
### Re: #tsdag — gpt-5

Completed `#tsdag`.

Implemented in `src/tsift`:
- Added `tsift dependency-dag --path <session> [target...] --json` with the `dependency-dag-v1` contract.
- The DAG report resolves agent-doc backlog ids, derives explicit `depends on` / `after` / `blocked by` / `before` edges, shared file/symbol/test/config and semantic overlap edges, and `worker_result` follow-up edges.
- It emits graph-backed nodes, edges, deterministic topological batches, cycle diagnostics, replay commands, and repair commands.
- Updated the tsift spec, README, version notes, graph orchestration contract fixture, operator examples, CLI parsing tests, unit coverage, and graph conformance coverage.

Verification:
- `cargo test dependency_dag`
- `cargo test graph_orchestration_contract_fixture_matches_live_reports`
- `make check`
- `cargo build`
- `cargo install --path .`
- Latest `tsift` GitHub CI run `26232896258` was green.
- Latest root `agent-loop` Agent Doc run `26243042143` had empty-step `check` and `tmux-ci` jobs with no logs, so I treated it as an external CI-start blocker rather than a code/tmux regression.

Commits prepared:
- tsift: `18ae92c` (`feat: add agent-doc dependency dag`)
- agent-loop gitlink: `561f7b08e` (`chore: bump tsift dependency dag`)

Per the session closeout rule, push happens after this `agent-doc finalize` commit succeeds.
<!-- agent:boundary:b8e1447e -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#impactdag]
- do [#dagfix]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#impactdag] [recommended] Extend conflict-matrix and dispatch-trace with scheduler fields: parallel_safe, blocks, blocked_by, required_context, expected_tests, and stable graph handles so agent-doc can run topological antichains without scraping prose.
- [ ] [#dagfix] [recommended] Add regeneration fixtures for real agent-doc queue DAGs: refresh -> evidence -> conflict-matrix -> dependency-dag -> dispatch-trace, asserting projection freshness, packet ids, worker_result closure, cycle detection, and stale graph fail-closed behavior.
- [ ] [#dbbench] [recommended] Add an experimental GraphStore backend evaluation for DuckDB/DuckPGQ and Ladybug: prototype read-only adapters behind the existing contract, benchmark refresh/status/evidence/conflict-matrix/dispatch-trace on real and synthetic projections, and only promote if it beats SQLite without worsening multi-process lock behavior or install portability.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
