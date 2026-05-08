---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. #follow-up-backlog
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': #follow-up-backlog Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- normalization -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260508-025423.md`*

Active backlog:
- [ ] [#pcctrl] Implement Phase A from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: add the project-local controller shell, singleton socket/launch lock, lazy connect-or-launch helper, controller status command, and bootstrap epoch/launch-mode persistence without changing route/start behavior yet.
- [ ] [#pcstore] Implement Phase B from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: move actor-record load/store behind controller commands, write SQLite first, emit session-actors.json and sessions.json as projections, and add projection drift diagnostics.
- [ ] [#pcsuper] Implement Phase C from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: make Harness Supervisor startup register with the controller and report ready/busy/waiting-input/blocked/closed lifecycle facts through controller-owned actor transitions.
- 2 more active item(s)
I'm concerned about the lag introduced since we started moving toward Project Controller -> Session Actor -> Harness Supervisor -> Harness Child.
`agent-doc sync` takes >10s for the selected tmux pane to respond and `agent-doc <file>` is also slow.
❯ Is this architecture inheriently slow or will Phase A-E improve the performance? 
### Re: Project Controller latency — gpt-5


Short answer: the architecture is not inherently slow, but the current transition path is paying a lot of "prove who owns this pane" cost on every route/sync. Phase A-E should improve that, with the biggest speedup coming in Phase D, provided we treat performance as an explicit acceptance criterion rather than assuming the cleanup automatically makes things faster.

The local logs support your concern. Recent safe-passive syncs for this workspace show `post-window-resolution` at `1778208718` and `post-prune` at `1778208730`, then another at `1778208838` to `1778208849`. In both cases the actual tmux-router reconcile immediately hit `FAST_PATH: layout already correct`. So the >10s sync lag is not layout math. It is happening before tmux-router, in the current prune / ownership-proof / projection-safety path.

There is a separate route symptom too: `ops.log` repeatedly records `route_dispatch_start_unproven_but_accepted ... timeout_secs=10` and `route_dispatch_only_submit_unproven ... timeout_secs=10` for this document. That means the command was typed into the pane, but Codex did not produce the proof the route path now waits for. From the user's seat that feels like the selected pane took >10s to respond, even when the terminal submit may already have happened.

Phase-by-phase expectation:

- Phase A probably will not fix the lag by itself. It adds the controller shell, singleton socket/lock, and status path. It must be kept cheap, but it mostly creates the place where the later optimization can live.
- Phase B can help if actor state reads become one authoritative lookup instead of loading multiple JSON/session projections and re-deriving ownership. Dual-writing projections must not stay on the hot path longer than necessary.
- Phase C should help route readiness because the supervisor reports `ready`, `busy`, `waiting_input`, `blocked`, and `closed` directly. That should reduce pane scraping and ambiguous "is this alive?" checks.
- Phase D is the main performance win: `start`, `route`, and `sync` stop electing owners from tmux/process/log evidence on the normal path. `sync` should consume controller bindings and then ask tmux-router only to reconcile layout. That is the version where no-op sync should be sub-second again.
- Phase E mainly improves operator commands and diagnostics. It helps explain slowness, but it is not the primary fast path fix.

❯ So I would treat the current >10s behavior as a transitional regression, not as evidence that Project Controller -> Session Actor -> Harness Supervisor -> Harness Child is too slow. The design should reduce work because it replaces repeated distributed inference with one local IPC lookup plus a cached actor state transition. The follow-up is already captured as `#pcperf`: if no-op safe-passive sync still spends 10s in prune/proof after Phase D, then the implementation failed the architecture's purpose.
### Re: Project Controller latency — gpt-5


Short answer: the architecture is not inherently slow, but the current transition path is paying a lot of "prove who owns this pane" cost on every route/sync. Phase A-E should improve that, with the biggest speedup coming in Phase D, provided we treat performance as an explicit acceptance criterion rather than assuming the cleanup automatically makes things faster.

The local logs support your concern. Recent safe-passive syncs for this workspace show `post-window-resolution` at `1778208718` and `post-prune` at `1778208730`, then another at `1778208838` to `1778208849`. In both cases the actual tmux-router reconcile immediately hit `FAST_PATH: layout already correct`. So the >10s sync lag is not layout math. It is happening before tmux-router, in the current prune / ownership-proof / projection-safety path.

There is a separate route symptom too: `ops.log` repeatedly records `route_dispatch_start_unproven_but_accepted ... timeout_secs=10` and `route_dispatch_only_submit_unproven ... timeout_secs=10` for this document. That means the command was typed into the pane, but Codex did not produce the proof the route path now waits for. From the user's seat that feels like the selected pane took >10s to respond, even when the terminal submit may already have happened.

Phase-by-phase expectation:

- Phase A probably will not fix the lag by itself. It adds the controller shell, singleton socket/lock, and status path. It must be kept cheap, but it mostly creates the place where the later optimization can live.
- Phase B can help if actor state reads become one authoritative lookup instead of loading multiple JSON/session projections and re-deriving ownership. Dual-writing projections must not stay on the hot path longer than necessary.
- Phase C should help route readiness because the supervisor reports `ready`, `busy`, `waiting_input`, `blocked`, and `closed` directly. That should reduce pane scraping and ambiguous "is this alive?" checks.
- Phase D is the main performance win: `start`, `route`, and `sync` stop electing owners from tmux/process/log evidence on the normal path. `sync` should consume controller bindings and then ask tmux-router only to reconcile layout. That is the version where no-op sync should be sub-second again.
- Phase E mainly improves operator commands and diagnostics. It helps explain slowness, but it is not the primary fast path fix.

So I would treat the current >10s behavior as a transitional regression, not as evidence that Project Controller -> Session Actor -> Harness Supervisor -> Harness Child is too slow. The design should reduce work because it replaces repeated distributed inference with one local IPC lookup plus a cached actor state transition. The follow-up is already captured as `#pcperf`: if no-op safe-passive sync still spends 10s in prune/proof after Phase D, then the implementation failed the architecture's purpose.
<!-- agent:boundary:33dd5953 -->
<!-- /agent:exchange -->
###

<!--
"❯ Commit / push:" should not have the `❯ ` prefix since this was the agent response. #agent-doc-bug
---
Please optimize the AGENTS.md + SKILL.md to reduce context usage yet maintain effectiveness for Claude Code + Codex
---
Does the supervisor actor ever block on `Run Agent Doc`? I noticed that /home/brian/work/btakita/agent-loop/tasks/software/tsift did not respond to `Run Agent Doc` in a previous turn. It seems to run ok now but I'm concerned that `Run Agent Doc` was non-responsive.
---
"Press Enter to restart, or 'q' to exit."
-->

## Pending / Not Built

<!-- agent:backlog -->

- [ ] [#pcperf] [recommended] Add sync/route latency instrumentation and budgets to the Project Controller rollout: measure prune/proof/router/dispatch-ack phases, keep no-op safe-passive sync on the fast path, and surface when a 10s dispatch-proof wait is the reason a pane appears non-responsive.
- [ ] [#pcctrl] Implement Phase A from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: add the project-local controller shell, singleton socket/launch lock, lazy connect-or-launch helper, controller status command, and bootstrap epoch/launch-mode persistence without changing route/start behavior yet.
- [ ] [#pcstore] Implement Phase B from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: move actor-record load/store behind controller commands, write SQLite first, emit session-actors.json and sessions.json as projections, and add projection drift diagnostics.
- [ ] [#pcsuper] Implement Phase C from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: make Harness Supervisor startup register with the controller and report ready/busy/waiting-input/blocked/closed lifecycle facts through controller-owned actor transitions.
- [ ] [#pcroutes] Implement Phase D from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: convert start, route, and sync to read/dispatch through controller authority and keep tmux-router as layout reconciliation only.
- [ ] [#pcops] Implement Phase E from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: wire status/history/attach/restart/clear/doctor through controller clients, keep destructive recovery behind doctor --repair, and expose stage-specific editor diagnostics.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:pending-done -->
- 2026-05-07 [#backlogreap] [recommended] Make completed/reaped backlog items visible in the session document or closeout response so items like #commenttail, #tmuxcoord, and #coordspike do not look silently removed after --pending-done.
- 2026-05-07 [#sqliterepros] [recommended] Before prototyping .agent-doc/state.db, capture 2-3 current agent-doc failures where cross-file authority drift or non-atomic state writes would have been fixed by one transactional store rather than by the single-owner actor/supervisor model alone.
- 2026-05-07 [#sqliteeval] [recommended] Review plan tasks/agent-doc/plan-sqlite-session-store-concurrency.md against the single-owner actor rollout and decide whether to prototype .agent-doc/state.db as the authoritative session store.
<!-- /agent:pending-done -->



<!--
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->
