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
❯ But all of these processes use the actor pattern?
### Re: actor pattern boundary — gpt-5

They can all use actor-pattern mechanics, but I would not name all of them `Actor`.

The distinction I would keep:

- `Session Controller`: may be implemented as an actor. It has one mailbox/API for fleet-level commands like lookup, create, attach, recover, and shutdown.
- `Session Actor`: definitely an actor. It is the serialized authority for one document session and owns generation/state transitions.
- `Session Supervisor`: can be actor-like internally, but should be named for its job: supervising one harness process. It should not be an ownership authority unless the design explicitly moves the session actor role into it.

So yes, the implementation can be actor-pattern throughout: message in, serialized state transition, event out. But the names should communicate responsibility, not implementation technique.

A clean model would be:

`Session Controller` receives fleet-level requests.
`Session Controller` routes a request to the right `Session Actor`.
`Session Actor` decides the authoritative transition.
`Session Supervisor` executes child-process work requested by that actor and reports observations back.

That keeps the important invariant intact: only the session actor decides ownership. The controller coordinates actors; the supervisor performs runtime work.
❯ How about "Fleet Commander", "Session Actor", "Session Supervisor"?
### Re: Fleet Commander naming — gpt-5

I would not use `Fleet Commander` in the code or specs.

It is memorable, but it adds metaphor where the system needs precision. In this area the names should make failure analysis boring and exact: who owns state, who routes requests, who supervises a process, who is allowed to mutate tmux/session records.

I would keep:

- `Session Controller` for the fleet-level daemon/API.
- `Session Actor` for the single authoritative owner of one document session.
- `Session Supervisor` for the runtime process around one Codex/Claude child.

If you want a slightly stronger fleet-level name than `Controller`, I would choose `Session Coordinator` over `Fleet Commander`, but only if the docs are explicit that it is not a passive coordinator. Still, `Session Controller` is cleaner because it sounds authoritative without being colorful.

Recommended final vocabulary:

`Session Controller` -> manages the fleet and routes requests.
`Session Actor` -> owns one session's state and generation.
`Session Supervisor` -> executes and observes one live harness process.

That vocabulary should age better in logs, errors, specs, and code review.
❯ How about "Fleet Actor", "Session Actor", "Agent Actor" or "Agent Supervisor"?
### Re: Fleet Actor / Agent Actor naming — gpt-5

I would avoid `Fleet Actor` and `Agent Actor`.

`Fleet Actor` has the same problem as `Fleet Commander`, just with actor terminology attached: it tells us implementation style, but not responsibility. It also risks implying one actor owns every state transition for every document. That can be true as a top-level mailbox, but the important design invariant is still per-session ownership.

`Agent Actor` is ambiguous in this codebase. `agent` can mean Codex/Claude, the agent-doc session document agent, the harness process, or the user-facing assistant. I would not use it for architecture boundaries.

`Agent Supervisor` is better than `Agent Actor` if the process is specifically supervising Codex/Claude, but `Session Supervisor` is still clearer because the supervised thing is the live session, not the model identity. It also keeps the name stable if the harness is Codex today, Claude tomorrow, or another agent later.

I would keep the final set as:

`Session Controller` -> fleet-level API/mailbox and lifecycle coordinator.
`Session Actor` -> one authoritative owner for one document session.
`Session Supervisor` -> runtime wrapper for the live harness process owned by that session actor.

If we want to emphasize actor-pattern implementation in docs, phrase it as:

`The Session Controller and each Session Actor are implemented as actors. Session Supervisors are subordinate runtime workers that may expose an actor-like command interface.`

That avoids putting `Actor` into every noun while still documenting the concurrency model precisely.
❯ I'm confused over the Controller, Actor, Supervisor hierarchy. All 3 have "Session" as the prefix. Please investigate better alternatives.
### Re: naming hierarchy — gpt-5

Yes, the repeated `Session` prefix is the problem. It makes the names look like peers when they are actually different scopes.

I would switch from role-only names to scope-first names:

- `Project Controller` -> one project/workspace-level service. It owns the singleton socket/launch lock, finds or creates per-document session owners, handles global repair policy, and protects project-wide invariants.
- `Session Actor` -> one authoritative owner for one document session. It owns `document_id`, `session_id`, `generation`, pane/window binding, harness, state, and transitions.
- `Harness Supervisor` -> one runtime wrapper around one Codex/Claude child. It owns the pty, restart policy, IPC socket, CWD/env setup, and reports lifecycle observations back to the session actor.

That hierarchy reads by scope:

`Project Controller` manages the project fleet.
`Session Actor` owns one document session.
`Harness Supervisor` runs one live harness child for that session.

I prefer this over `Session Controller / Session Actor / Session Supervisor` because the prefixes now communicate boundaries: project, session, harness. It also avoids the misleading idea that the supervisor owns the session. The supervisor owns a child process; the session actor owns authority.

Other alternatives I considered:

- `Session Manager`: clear enough, but it keeps the same `Session` prefix as `Session Actor` and can be confused with the per-session owner.
- `Session Directory`: good if the top layer only does lookup, too weak if it owns launch locks and recovery policy.
- `Fleet Controller`: technically accurate, but `fleet` adds a metaphor that will look noisy in logs and specs.
- `Agent Supervisor`: better than `Session Supervisor`, but `agent` is overloaded here. It can mean Codex/Claude, the assistant, or the agent-doc document agent.
- `Harness Runner`: good if it only spawns and forwards I/O, but weaker than `Supervisor` once restart policy and halted/waiting-input state are part of the contract.

Recommended final vocabulary:

```text
Project Controller -> Session Actor -> Harness Supervisor -> Harness Child
```

And the implementation note should be:

```text
The Project Controller and each Session Actor are implemented as actors. Harness Supervisors are subordinate runtime workers that expose a control socket and report lifecycle events back to their owning Session Actor.
```

I also updated `#coordspike` to use this terminology so the next plan does not keep carrying `coordinator actor` forward. That item remains open; this turn only clarified the naming direction.
❯ Agreed. Update plans and backlog with `Project Controller -> Session Actor -> Harness Supervisor -> Harness Child`.
### Re: hierarchy updates — gpt-5

Agreed. I updated the active planning surfaces to use:

```text
Project Controller -> Session Actor -> Harness Supervisor -> Harness Child
```

Changes made:

- Updated `tasks/agent-doc/plan-single-owner-session-actor-transition.md` so the target model starts with the accepted hierarchy and defines the scope of each layer.
- Updated phase 3 in that plan from vague supervisor ownership language to harness-supervisor lifecycle reporting into the session actor.
- Updated `tasks/agent-doc/plan-sqlite-session-store-concurrency.md` so sqlite evaluation keeps authority on the `Session Actor` record and does not blur it into the `Project Controller` or `Harness Supervisor`.
- Updated the live `#coordspike` backlog item to name the exact hierarchy in the MVP plan title.

I left the old `coordinator actor` wording inside the compacted exchange summary alone because that is historical transcript context, not the active backlog. There is no new follow-up backlog item from this turn; this was an update to the existing plan and backlog surfaces.
<!-- agent:boundary:89051e86 -->
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
- [ ] [#coordspike] [recommended] Draft a Project Controller -> Session Actor -> Harness Supervisor -> Harness Child MVP plan after #tmuxcoord: singleton socket/launch lock, lazy vs managed lifecycle, SQLite journal, harness-supervisor client protocol, and tmux reconciliation tests.
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
