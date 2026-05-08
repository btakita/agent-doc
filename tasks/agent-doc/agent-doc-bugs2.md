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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260508-014342.md`*

Active backlog:
- [ ] [#tmuxcoord] [recommended] Investigate tsift.md three-pane agent-doc tmux state and decide whether the fix belongs in tmux-router pane reconciliation, session ownership, or a coordinator daemon/actor model.
- [ ] [#coordspike] [recommended] Draft a coordinator actor MVP plan after #tmuxcoord: singleton socket/launch lock, lazy vs managed lifecycle, SQLite journal, supervisor client protocol, and tmux reconciliation tests.
- [ ] [#sqliterepros] [recommended] Before prototyping .agent-doc/state.db, capture 2-3 current agent-doc failures where cross-file authority drift or non-atomic state writes would have been fixed by one transactional store rather than by the single-owner actor/supervisor model alone.
- 1 more active item(s)
❯ do [#tmuxcoord]
### Re: #tmuxcoord — gpt-5

Completed `#tmuxcoord`.

I inspected the current `tsift.md` session state, tmux pane layout, the saved editor route error, the route/sync/session-actor code, and the existing route simplification / single-owner actor plans.

Findings:

- `tsift.md` is healthy now. `agent-doc session doctor tasks/software/tsift.md` reports actor generation `67`, pane `%208`, state `ready`, healthy supervisor, no startup-miss marker, and a committed latest document cycle.
- The saved editor-route error captures the old non-responsive shape: `authoritative actor generation 61 for tasks/software/tsift.md owns pane %188 but route will not inject a new trigger because the authoritative actor is still starting`.
- That points to stale or stuck session ownership state, not a tmux-router ordering failure. The visible/stash/three-pane shape was a symptom: different projections could disagree about which pane was authoritative, and route trusted the actor `starting` state enough to refuse `Run Agent Doc`.
- The tmux-router side should remain responsible for deterministic layout operations: ordered panes, split-left/split-right behavior, guarded join/stash behavior, and no destructive reconciliation. The current route tests already cover the important three-pane ordering cases.
- The real fix belongs in `agent-doc` session ownership: the actor record must be the only normal-path authority for document -> pane -> generation, and `starting` must either advance to `ready`, transition to `blocked`/`closed`, or be replaced through an explicit generation transition. A coordinator/daemon helps only if it becomes that authority; as a passive watcher it would just add another stale source.
- The current code and logs show the newer direction is working for tsift: recent `Run Agent Doc` events used `route_dispatch_only_via_actor_direct_pane_submit` against `%208` and recorded `proof=consumed`.

Decision: do not put the primary repair in tmux-router pane reconciliation. Keep tmux-router as a layout primitive, continue moving ownership into the single-owner session actor, and use the existing `#coordspike` item to design the coordinator/actor MVP around that authority boundary.

I did not add a duplicate backlog item. The next backlog item should be `#coordspike`.
❯ Is coordinator the best name or should it be something else? Router? Dispatcher? Something else?
### Re: coordinator naming — gpt-5

I would not use `router` or `dispatcher` for the owning process.

Best naming split:

- `session actor` for the core authority model.
- `session supervisor` for the per-session harness process that starts/restarts/observes the child agent, but this name is already in use and should stay scoped to that role.
- `session manager` or `session controller` for a daemon/process if we need a user-facing noun.

I would avoid these:

- `router`: too narrow. It sounds like it only chooses where a command goes. The thing we need owns generation, lifecycle, state transitions, restart policy, and projections.
- `dispatcher`: also too narrow. It sounds like command delivery, not authority over document/session/pane ownership.
- `coordinator`: acceptable for early design discussion, but it is a little vague. It can imply a passive mediator between existing authorities, which is exactly the failure mode we want to avoid.
- `orchestrator`: too broad and buzzwordy for this codebase.

My recommendation: rename the backlog concept from “coordinator actor” toward `session actor` / `session controller`.

Concretely, `#coordspike` should probably produce a plan titled something like:

`single-owner session actor and controller MVP`

That keeps the important invariant in the name: there is one owner. The controller may route and dispatch, but those are subordinate operations.
❯ The "Session Actor" manages all the sessions while the "Supervisor Actor" manager a single session?
### Re: session actor vs supervisor actor — gpt-5

Close, but I would make one important correction: `Session Actor` should not mean “one actor that manages all sessions.” That name should refer to the single owner for one document session.

Better hierarchy:

- `Session Controller`: optional process/daemon that manages the fleet of session actors, socket ownership, lookup, lifecycle bootstrapping, and global invariants.
- `Session Actor`: one authoritative actor per document/session binding. It owns the document -> session id -> generation -> pane/window -> state record.
- `Session Supervisor`: one runtime process per live harness session. It starts/restarts the child agent, observes readiness/exit, and reports transitions back to the session actor.

So the rule is:

`Session Controller` manages all sessions.
`Session Actor` owns one document session.
`Session Supervisor` runs one live harness process for that session actor.

I would avoid `Supervisor Actor` unless it is truly an actor with its own mailbox/protocol. In the current codebase, `supervisor` already means the runtime process around Codex/Claude, so renaming it to `Supervisor Actor` may blur the line between authority and execution. The authority should live in the session actor; the supervisor should remain subordinate.

For `#coordspike`, I would phrase the target as:

`session controller with single-owner session actors`

That makes the all-sessions vs one-session split explicit.
<!-- agent:boundary:bf1d5fc1 -->
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
- [ ] [#coordspike] [recommended] Draft a coordinator actor MVP plan after #tmuxcoord: singleton socket/launch lock, lazy vs managed lifecycle, SQLite journal, supervisor client protocol, and tmux reconciliation tests.
- [ ] [#sqliterepros] [recommended] Before prototyping .agent-doc/state.db, capture 2-3 current agent-doc failures where cross-file authority drift or non-atomic state writes would have been fixed by one transactional store rather than by the single-owner actor/supervisor model alone.
- [ ] [#sqliteeval] [recommended] Review plan tasks/agent-doc/plan-sqlite-session-store-concurrency.md against the single-owner actor rollout and decide whether to prototype .agent-doc/state.db as the authoritative session store.

<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->



<!--
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->
