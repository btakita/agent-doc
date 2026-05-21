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

Completed #gdbhard; pushed tsift ef49229 and parent 4dc52dbab.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260521-054738.md`*

Compacted content:
- Archived 1 response topic(s): graph-db operator hardening
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:c91f146d -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tsdag] [recommended] Add graph-level dependency DAG extraction for agent-doc work: derive explicit and implicit edges among backlog ids from file/symbol/test/config overlap, semantic relations, worker_result follow-up ids, and depends-on text; emit nodes, edges, cycle diagnostics, topo batches, replay commands, and repair commands as a versioned JSON contract.
- [ ] [#impactdag] [recommended] Extend conflict-matrix and dispatch-trace with scheduler fields: parallel_safe, blocks, blocked_by, required_context, expected_tests, and stable graph handles so agent-doc can run topological antichains without scraping prose.
- [ ] [#dagfix] [recommended] Add regeneration fixtures for real agent-doc queue DAGs: refresh -> evidence -> conflict-matrix -> dependency-dag -> dispatch-trace, asserting projection freshness, packet ids, worker_result closure, cycle detection, and stale graph fail-closed behavior.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.**
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
