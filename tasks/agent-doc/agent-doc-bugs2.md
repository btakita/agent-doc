---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Added auto-DAG next steps; top backlog item: #autodag.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260522-005124.md`*

Compacted content:
- Archived 1 response topic(s): duplicate prompt residue
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ When agent-doc first opens Claude Code or Codex, the first JB `Run Agent Doc` errors. After manually running agent-doc on the file, subsequent `Run Agent Doc` calls sometimes work. For /home/brian/work/btakita/agent-loop/tasks/recruit/acadian-asset-management.md, `Run Agent Doc`. #spec-test-build-install-commit-push
```
resync: stash pane %18 is registered in its own project root — skipping kill
resync: stash pane %36 is registered in its own project root — skipping kill
resync: stash pane %27 is registered in its own project root — skipping kill
resync: stash pane %17 is registered in its own project root — skipping kill
resync: stash pane %8 is registered in its own project root — skipping kill
resync: stash pane %38 is registered in its own project root — skipping kill
[route] target tmux session: 0
Error: dispatch-only claude reroute for tasks/recruit/acadian-asset-management.md refused before input because authoritative actor generation 1 on pane %2 has degraded supervisor state (supervisor health is no_socket, runtime_actor_state=missing). Restart or rebind the owner with `agent-doc start tasks/recruit/acadian-asset-management.md` and rerun the route after dispatch-start proof is available.
```
### Re: JetBrains degraded first-open reroute — gpt-5

Fixed the JetBrains first-open failure for dispatch-only Claude/Codex routes when the authoritative actor reports `supervisor health is no_socket` or `runtime_actor_state=missing`.

The route now keeps a degraded authoritative actor pane as the bounded recovery target when that pane is still the current registered/live owner. It records controller dispatch, logs `route_dispatch_only_authoritative_degraded_direct_pane`, and then uses the normal direct-pane readiness, blocker, and proof checks before submitting. Hook-backed Codex/OpenCode routes still fail closed if delivery remains accepted-only instead of producing dispatch-start proof, and busy or unready panes still stop at the existing typed guards.

Changed paths:

- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/flow/routed_reopen.rs`
- `src/agent-doc/src/route/tests.rs`
- `src/agent-doc/tests/test_cli.rs`
- `src/agent-doc/specs/07-session-tmux-commands.md`
- `src/agent-doc/specs/08a-session-actor-contract.md`
- `src/agent-doc/README.md`
- `src/agent-doc/VERSIONS.md`

Verification completed:

- `cargo test degraded_direct_submit_log_names_supervisor_reason`
- `cargo test dispatch_only_can_use_degraded_authoritative_actor`
- `cargo test authoritative_actor_dispatch_guard_reason`
- `cargo test resolve_or_create_pane_dispatch_only_reuses_registered_authoritative_actor_pane_when_supervisor_state_is_missing -- --ignored`
- `make check`
- `make install`
- `agent-doc --version` -> `agent-doc 0.33.16`
- CI/tmux review: latest `CI` run `26261819347` succeeded; its `Run make tmux-ci` step succeeded.

Commits made before this session-document closeout:

- `src/agent-doc` commit `90542dbc` (`fix: recover degraded dispatch-only actor panes`)
- parent commit `15841f690` (`agent-doc: recover degraded editor reroutes`)

Push is intentionally after this binary-owned session-document commit and `session-check`, per the agent-doc closeout ordering.

<!-- agent:boundary:bb5d9059 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#autodag]
- do [#dagdispatch]
- do [#dagrecover]
- do [#daglogs]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#autodag] [recommended] Add an agent-doc auto-DAG planner for queued do/compound directives: parse explicit depends-on text, backlog ids, graph/conflict evidence, and job packets into a topological schedule; fail closed on cycles, missing ids, stale graph evidence, or ambiguous ownership before dispatch.
- [ ] [#dagdispatch] [recommended] Execute auto-DAG batches through agent-doc orchestrate/jobs: run parallel-safe antichain batches, collect worker_result proof, unblock downstream nodes only after required proof/session-check passes, and stop deterministically on failed/gated nodes without launching dependents.
- [ ] [#dagrecover] [recommended] Add auto-DAG persistence/recovery semantics: write operation docs/job packets with schedule id, node state, attempt count, replay commands, and repair commands so interrupted DAG runs can resume idempotently after compact/finalize/restart.
- [ ] [#daglogs] [recommended] Use session-review guardrails as scheduler input: classify prompt-budget/cache-resend/restart-loop/noop-closeout log families before auto-DAG start and gate large or looping runs into compact/restart/fixture-fix steps.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
