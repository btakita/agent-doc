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
<!-- agent:boundary:4a52b431 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#wctx]
- do [#mxfx]
- do [#gobx]
- do [#semx]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gsch] [recommended] Stabilize graph orchestration JSON contracts: publish schema/versioned examples for graph-db evidence, conflict-matrix worker_prompt_packets, context-pack graph_orchestration, and session-review follow-up commands so agent-doc can consume them without scraping.
- [ ] [#grev] [recommended] Add graph evidence replay/freshness fixtures: persist evidence packet ids/projection hashes for queued backlog runs and prove stale graph.db/conflict-matrix inputs fail closed with repair commands.
- [ ] [#wres] [recommended] Add worker-result rows to the graph projection: ingest completed/blocked worker packets as worker_result nodes linked to backlog/job/source handles, including touched files/tests and follow-up ids.
- [ ] [#srnk] [recommended] Calibrate semantic dispatch ranking: add fixture-driven weights/explanations for semantic_concept/entity matches so semantic relevance improves ordering without overriding file/symbol/test conflict gates.
- [ ] [#gops] [recommended] Add operator examples for graph-backed dispatch: end-to-end docs/fixtures showing refresh -> evidence -> conflict-matrix -> context-pack/session-review commands for a multi-worker session.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
