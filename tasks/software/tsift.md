---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
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
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260521-014846.md`*

Compacted content:
- Archived 1 response topic(s): do #802b
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps for graph db features & subagent support?
### Re: graph db features & subagent support — gpt-5

The graph DB base is already fairly complete: SQLite correctness store, Convex projection/snapshot path, conformance coverage, `graph-db evidence`, and `conflict-matrix` are all represented in the completed work. The next useful layer is making those graph outputs directly drive agent-doc/subagent orchestration instead of remaining operator-facing tsift commands.

I added the follow-up backlog in priority order. Top backlog item: `#agbr`.

1. `#agbr` — wire `graph-db evidence` and `conflict-matrix` into agent-doc planning/orchestration, with fail-closed stale/missing-target checks.
2. `#wctx` — promote conflict-matrix worker prompt packets into first-class subagent context: owned files/symbols, read-only context, forbidden files, expected tests, expansion commands, and token budgets.
3. `#mxfx` — add a deterministic multi-worker acceptance fixture proving overlapping targets are blocked and non-overlapping worker prompts are safe to dispatch.
4. `#gobx` — expose graph orchestration observability through `context-pack` / `session-review`, including freshness, evidence ids, dispatch decisions, and next commands.
5. `#semx` — fold semantic graph rows into dispatch ranking so related concept/entity paths improve worker-scope recommendations without replacing hard file/symbol conflict gates.
❯ Any backlog work that should be done on the agent-doc codebase should be transferred to agent-doc-bugs2.md
### Re: transfer agent-doc backlog — gpt-5

Transferred the agent-doc-owned work out of this tsift backlog and into `tasks/agent-doc/agent-doc-bugs2.md` as `#agbr`.

I treated `#agbr` as the agent-doc-codebase item because it changes `agent-doc plan/orchestrate` behavior. The remaining items stay here because they are tsift-owned graph/reporting work: `#wctx`, `#mxfx`, `#gobx`, and `#semx`.

Top tsift backlog item is now `#wctx`. Top transferred agent-doc item id is `#agbr`.
### Re: graph-backed worker dispatch — gpt-5
Completed `#wctx`, `#mxfx`, `#gobx`, and `#semx`.

Implemented first-class conflict-matrix worker prompt packets in `src/tsift`: owned files/symbols, read-only context, forbidden files, expected tests, expansion commands, and token budgets now flow from graph-db evidence into conflict-matrix output and worker ownership blocks. Graph evidence now includes semantic concept/entity rows, semantic refs contribute to dispatch ranking after hard conflict gates, and session-level graph orchestration observability is surfaced through context-pack/session-review follow-up commands.

Added deterministic multi-worker conformance coverage for overlapping vs. non-overlapping backlog targets, including prompt packet fields and fail-closed shared-file decisions. Updated `README.md`, `SPEC.md`, and `VERSIONS.md` for the new graph evidence, worker packet, semantic ranking, and observability behavior.

Verification:
- `make check` passed: clippy, unit tests, exit-code tests, and graph-db conformance.
- `cargo test --test graph_db_conformance conflict_matrix_multi_worker_fixture_blocks_shared_files_and_emits_prompt_packets` passed after fixture whitespace cleanup.
- `cargo build` passed.
- `cargo install --path .` replaced local `tsift`; `tsift --version` reports `tsift 0.1.42`.
- Latest GitHub Actions `CI` run for `main` was `completed/success` at `2026-05-21T00:44:42Z`.

Commits:
- `btakita/tsift` pushed `f0b1582` (`feat: add graph-backed worker prompt packets`).
- Superproject submodule pointer committed as `f3c227010` (`chore: update tsift submodule`); root push follows the document closeout ordering.
❯ What are #next-steps for agent orchestration and graph db features?
### Re: next graph orchestration steps — gpt-5

The next useful work is to harden the contract between tsift's graph outputs and agent orchestration, then add feedback loops so completed worker dispatch becomes graph evidence for later cycles.

I added the tsift-owned follow-ups in priority order. Top tsift backlog item: `#gsch`.

1. `#gsch` — stabilize versioned JSON contracts for `graph-db evidence`, `conflict-matrix` worker prompt packets, `context-pack` graph orchestration, and `session-review` follow-up commands.
2. `#grev` — add replay/freshness fixtures proving evidence packet ids, projection hashes, stale graph DBs, and conflict-matrix inputs fail closed with repair commands.
3. `#wres` — add `worker_result` graph rows so completed/blocked worker packets become queryable evidence linked to backlog/job/source handles.
4. `#srnk` — calibrate semantic dispatch ranking with fixtures and explanations so semantic relevance improves ordering without weakening hard conflict gates.
5. `#gops` — add end-to-end operator examples for the graph-backed dispatch path: refresh, evidence, conflict-matrix, context-pack, and session-review.

Agent-doc-owned orchestration work is already transferred to `tasks/agent-doc/agent-doc-bugs2.md`; top agent-doc item remains `#agbr`.

No code changes or tests were run for this cycle; this was backlog triage only.
### Re: graph orchestration implementation — gpt-5
Completed `#gsch`, `#grev`, `#wres`, `#srnk`, and `#gops`.

Implemented versioned graph orchestration contracts across the tsift surfaces agent-doc needs to consume directly: `graph-db evidence` now emits `contract_version`, `packet_id`, `projection_hash`, replay commands, repair commands, and reachable `worker_result` rows; `conflict-matrix` now carries evidence packet summaries, packet ids on candidates, contract-marked `worker_prompt_packets`, projection hashes, and semantic ranking explanations; `context-pack` and `session-review --next-context` now expose explicit contract versions for orchestration/follow-up payloads.

Added worker-result graph projection support by parsing completed/blocked agent-doc response lines into `worker_result` nodes linked back to backlog/job/source context. Those rows preserve status, touched files, expected tests, and follow-up ids so later graph evidence can reason over completed or blocked worker packets.

Calibrated semantic dispatch ranking so semantic concept/entity matches produce score explanations such as owned-file/owned-symbol relevance, while the hard file/symbol/test/config conflict gates still control fail-closed dispatch. Added replay/freshness coverage proving stale evidence freshness fails closed with repair commands.

Updated operator docs and fixtures, including `fixtures/graph-db-operator-examples/graph-orchestration-contracts.json`, plus end-to-end refresh -> evidence -> conflict-matrix -> context-pack -> session-review examples.

Verification:
- `cargo check` passed.
- Targeted graph orchestration tests passed for evidence packets, worker prompt packets, semantic ranking, stale freshness repair, worker-result projection, and context-pack observability.
- `jq . fixtures/graph-db-operator-examples/graph-orchestration-contracts.json` passed.
- `git diff --check` passed.
- `make check` passed: clippy, unit tests, exit-code tests, and graph-db conformance.
- `cargo build` passed.
- `cargo install --path .` replaced the local binary; `tsift --version` reports `tsift 0.1.42`.
- Latest GitHub Actions `CI` run for `btakita/tsift` was `completed/success` at `2026-05-21T02:33:47Z`; jobs were `clippy + test` and `live Convex graph acceptance`. This repo has no tmux CI workflow or `tmux-ci` target.

Commits:
- `btakita/tsift` pushed `dbac74c` (`feat: version graph orchestration packets`).
- Superproject submodule pointer committed locally as `6c6a39ec1` (`chore: update tsift submodule`); the root push follows this document closeout boundary.
❯ Add #next-steps to agent-doc-bugs2.md for recently added tsift features.
❯ Add #next-steps to tsift.md for agent orchestration and graph db features.
### Re: next steps for graph orchestration — gpt-5
Added recommended next-step backlog items for both documents.

Top agent-doc backlog item added to `tasks/agent-doc/agent-doc-bugs2.md`: `#gxct` — validate tsift graph orchestration contracts in agent-doc plan/orchestrate before lower-agent dispatch.

Top tsift backlog item added here: `#gcfx` — add compiled contract-parity fixtures comparing the documented graph orchestration contracts with live CLI JSON output.

I kept agent-doc-owned work in `agent-doc-bugs2.md` and tsift-owned graph/reporting work in `tsift.md`. No code changes or tests were run; this was backlog triage only.
<!-- agent:boundary:789b7b50 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gsch]
- do [#grev]
- do [#wres]
- do [#srnk]
- do [#gops]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gcfx] [recommended] Add compiled contract-parity fixtures comparing graph-orchestration-contracts.json with live graph-db evidence, conflict-matrix, context-pack, and session-review JSON output, including contract_version, packet_id, projection_hash, replay_commands, and repair_commands.
- [ ] [#wrfb] [recommended] Add worker-result feedback reports: summarize completed/blocked worker_result rows, touched files/tests, follow-up ids, and outcome history so conflict-matrix can warn about repeated blockage without weakening hard conflict gates.
- [ ] [#gdtr] [recommended] Add graph-backed dispatch trace exports: compact JSON/HTML views linking backlog, job_packet, worker_result, source_handle, semantic rows, evidence packet ids, and worker_prompt_packets for operator review.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
