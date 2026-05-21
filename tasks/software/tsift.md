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

Added graph/orchestration next steps; top tsift backlog item: #orchdog.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260521-050641.md`*

Compacted content:
- Archived 4 response topic(s): next steps; dogfood #orchdog; simpler #orchdog options; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: graph-db queue — gpt-5

Completed `#gdbevid`, `#cmgr`, and `#gdbidx`.

Implemented in `src/tsift` commit `8349ca9` and recorded from the root repo in `108977f79`.

What changed:
- `graph-db evidence` now gives backlog/job targets without code-token matches a bounded session-line `worker_context` + `source_handle`, so a known backlog id no longer looks like a broken empty projection. Evidence still warns if a resolved target truly has no projection-linked rows.
- `conflict-matrix` now separates per-target fail-closed state from cross-target overlap with `per_target_fail_closed` and `cross_target_parallel_safe`, while keeping legacy `can_parallel` conservative.
- `graph-db refresh` now warns clearly when the summary cache is empty and points operators at `tsift summarize --extract ...` before relying on semantic rows.
- Updated `SPEC.md`, graph orchestration contract fixtures, and conformance coverage for these behaviors.

Verification:
- `cargo test --test graph_db_conformance` passed: 25 passed, 1 ignored.
- `make check` passed: clippy, 642 unit tests with 1 ignored, 108 exit-code/integration tests, and graph conformance.
- `cargo build` passed.
- `cargo install --path . --locked` passed and replaced local `tsift 0.1.42`.
- Latest `btakita/tsift` GitHub `CI` run was green (`26205441698`). Latest root `agent-loop` Agent Doc run had empty-step `tmux-ci` / `check` failures, so I classified it as an external CI-start blocker rather than a tmux regression.
<!-- agent:boundary:2645272f -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#gdbevid]~
- do [#cmgr]
- do [#gdbidx]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.**
- [ ] [#gdbhard] [recommended] Harden graph-db operator docs/fixtures around the current feature set: refresh/status/doctor/evidence/drift/convex-sync replay paths, stale Convex repair commands, and real agent-loop session examples that can be regenerated in CI.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
