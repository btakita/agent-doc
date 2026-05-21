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
Completed: #impactdag and #dagfix shipped in tsift ed28437 and superproject 189dcac98; graph orchestration now exposes scheduler fields and regenerates dependency-DAG replay fixtures.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260521-212634.md`*

Compacted content:
- Archived 2 response topic(s): #impactdag and #dagfix; #dagfix queue closeout
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ do [#dbbench]. #spec-test-build-install-commit-push
### Re: #dbbench — gpt-5

Implemented `#dbbench` and marked it done.

Changed paths:
- `src/tsift/SPEC.md`
- `src/tsift/src/main.rs`
- `src/tsift/src/substrate.rs`
- `src/tsift/tests/graph_db_conformance.rs`
- `src/tsift/fixtures/graph-db-operator-examples/agent-orchestration-acceptance-pack.json`

What changed:
- Added `tsift graph-db backend-eval` for experimental DuckDB/DuckPGQ and Ladybug backend evaluation.
- Added a read-only experimental `GraphStore` harness over provider-neutral rows and parity/timing reports against SQLite on real and synthetic datasets.
- Covered refresh/load, status, evidence, conflict-matrix, and dispatch-trace operations with promotion decisions, lock/install notes, cached planner inputs, deterministic dispatch-trace traversal, and conformance coverage.

Verification:
- `cargo check`
- `cargo test graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite`
- `cargo test cli_accepts_global_compact_flag`
- `cargo test agent_orchestration_acceptance_pack_regenerates_from_real_queue_fixture --test graph_db_conformance`
- `make check`
- `cargo build`
- `cargo install --path .`
- `tsift --version` -> `tsift 0.1.42`
- GitHub tsift CI `26255514008`: success for live Convex acceptance and full clippy/test.
- Superproject Agent Doc CI `26255527494`: failed before any job steps or logs for `check` and `tmux-ci`; treated as an external CI-start blocker per runbook, not a tmux regression.

Commits and push:
- `btakita/tsift@e8fced7` (`feat: add graph backend evaluation gate`) pushed to `origin/main`.
- `btakita/agent-loop@db58d6c` (`chore: update tsift backend evaluation`) pushed to `origin/main`.

Note: an installed full-workspace smoke against this agent-loop session hit an existing SQLite `database is locked` state, and `tsift status` reported a stale workspace projection. I left that unrelated workspace index state untouched.

Confidence: high for the CLI surface, parity harness, and regression coverage. The DuckDB/DuckPGQ and Ladybug adapters intentionally remain prototype/read-only and promotion-held until a real engine adapter satisfies the gate.
<!-- agent:boundary:556a87fa -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
