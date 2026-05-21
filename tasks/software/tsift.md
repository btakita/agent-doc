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

Blocked: graph-db evidence for #tsdag timed out; queued DAG/lock follow-ups remain open.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260521-055801.md`*

Compacted content:
- Archived 2 response topic(s): dependency DAG queue blocker; post-closeout queue additions
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:1e32829f -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#tsdag]
- do [#impactdag]
- do [#dagfix]
- do [#cmgrwres]
- do [#gdblock]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#bfd1] [recommended] Fix tsift graph-db evidence timeout on agent-doc backlog ids by bounding target resolution/path traversal and adding regression coverage for evidence autodag --depth 3 --limit 8 against session backlog fixtures.
- [ ] [#w94s] [recommended] Add graph-evidence coverage for queue-head drift where consumed queue prompts revert from done do-id text to active do-id text, including repair guidance that does not reactivate completed queue items.
- [ ] [#tsdag] [recommended] Add graph-level dependency DAG extraction for agent-doc work: derive explicit and implicit edges among backlog ids from file/symbol/test/config overlap, semantic relations, worker_result follow-up ids, and depends-on text; emit nodes, edges, cycle diagnostics, topo batches, replay commands, and repair commands as a versioned JSON contract.
- [ ] [#impactdag] [recommended] Extend conflict-matrix and dispatch-trace with scheduler fields: parallel_safe, blocks, blocked_by, required_context, expected_tests, and stable graph handles so agent-doc can run topological antichains without scraping prose.
- [ ] [#dagfix] [recommended] Add regeneration fixtures for real agent-doc queue DAGs: refresh -> evidence -> conflict-matrix -> dependency-dag -> dispatch-trace, asserting projection freshness, packet ids, worker_result closure, cycle detection, and stale graph fail-closed behavior.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
- [ ] [#cmgrwres] [recommended] `tsift conflict-matrix` should downgrade per-target `fail_closed` when the target has a non-zero `worker_result` row count in graph-db. Observed 2026-05-21 dogfood against `tsift.md`: `gdbevid`, `cmgr`, `gdbidx` all have completed `worker_result` rows (1 per item, single-hop path from backlog node) but conflict-matrix still reported `fail_closed: 3 target(s)` because the items lacked live `source_handles`/`owned_files`. A completed worker_result IS evidence the item was successfully dispatched; the dogfood path otherwise cannot re-dispatch a completed item. Fix: downgrade `fail_closed` to a soft `info` signal when `worker_result_count > 0`; surface a new `previously_completed: bool` field per target so operators see both signals.
- [ ] [#gdblock] [recommended] Graph-db lock recovery path. Observed 2026-05-21 dogfood: `agent-doc jobs create tasks/software/tsift.md --audit` failed with `Error code 5: The database file is locked` because `tsift graph-db --json status` was blocked by a concurrent writer (hot journal + SQLite read-only condition). Operator has no documented way to release the lock short of finding/killing the holder. Same root cause as `agent-doc-bugs2.md` `#w25k`. Fix options: (a) add `tsift graph-db --release-lock` recovery command that inspects pid + hot-journal and rolls back safely; (b) add a documented runbook step ("when locked, check `lsof .tsift/graph.db` and wait for writer to exit"); (c) switch graph-db SQLite to WAL mode so readers don't block on writer journal. Surfaces in any concurrent-agent workflow (codex + claude-code running side-by-side on the same checkout).
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
