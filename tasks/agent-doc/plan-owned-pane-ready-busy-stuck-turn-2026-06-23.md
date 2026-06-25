# Plan: owned-pane ready/busy stuck turn

## Problem

On 2026-06-23, the Monster Rod Holders `agent-doc` session appeared stuck
after a Codex turn completed in chat.

`agent-doc session status src/boost-client/tasks/monsterrodholders.md` reported
an internally contradictory state:

- `actor.state=ready`
- `supervisor.health=healthy`
- `supervisor.actor_state=ready`
- `controller_lease.runtime_state=ready`
- `live_pane.state=alive-busy`
- `live_pane.prompt_ready=false`
- `current_command=agent-doc`
- session log decision `keep_alive`
- reason `live_pane_busy_blocked_prompt`
- recent event `commit_already_current`

That combination is a liveness bug. If the controller and actor lease both say
the owned pane is ready, route/idle-watch should not keep the session blocked
forever solely because the tmux pane probe still reads as busy.

## Evidence

- Document:
  `src/boost-client/tasks/monsterrodholders.md`
- Session:
  `b04ec6ec-0a5c-4afb-b92e-c5448bce5969`
- Pane:
  `%43`, window `@37`
- Supervisor instance:
  `cf67dead-6e2f-4714-bf88-06675e5fbf01`
- Status command:
  `agent-doc session status /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md`
- Observed status:
  actor/controller ready while live pane probe is `alive-busy` and route keeps
  it alive because of `live_pane_busy_blocked_prompt`.
- Immediate precursor:
  `agent-doc commit` returned `commit_already_current` / staged snapshot already
  matched HEAD, while local document drift remained.

## Hypothesis

The route-owned reap/dispatch path is treating the tmux live-pane busy probe as
authoritative even when the controller lease and actor state are already ready.

This can happen after a commit/current closeout path or direct-chat closeout:
the binary-side state machine reaches ready, but the pane scanner still sees
`current_command=agent-doc` and `prompt_ready=false`. Because the keep-alive
reason has no bounded reconcile, the session can remain stuck in a state that
looks busy to route but ready to the supervisor.

## Non-Goals

- Do not remove the recursive direct-invocation deadlock guard.
- Do not auto-kill a pane as the first recovery action.
- Do not classify a real active Codex/OpenCode/Claude turn as idle merely
  because one status source says ready.
- Do not solve direct-chat response writeback here; that is tracked separately
  by `#directchatpb2`.

## Fix Plan

- [ ] Add a state-consistency check for the tuple:
  controller ready + actor ready + healthy supervisor + live pane busy.
- [ ] Add a bounded re-probe/reconcile path before `route_owned_reap_decision`
  keeps the pane alive for `live_pane_busy_blocked_prompt`.
- [ ] If the controller remains ready after the bounded re-probe, downgrade the
  stale pane-busy probe to a recoverable conflict rather than an indefinite
  keep-alive.
- [ ] Emit an explicit ops/status marker, for example
  `owned_pane_ready_busy_conflict`, with actor state, controller runtime state,
  prompt probe, current command, and latest commit/session event.
- [ ] Teach `agent-doc session status` to print an actionable recovery hint for
  this exact conflict, preferring a non-destructive reconcile/clear over pane
  restart.
- [ ] Add regression coverage for:
  - controller ready + actor ready + live pane busy after `commit_already_current`
    does not block forever;
  - live pane truly busy with an active in-flight turn still remains protected;
  - repeated status checks do not oscillate between ready and busy.
- [ ] Add a live verification recipe that captures the status contradiction,
  then proves the fixed route path either accepts the next prompt or prints a
  bounded recovery command.

## Acceptance Criteria

- A session cannot remain indefinitely in `actor ready` / `controller ready` /
  `live_pane alive-busy` without either reconciling to ready or surfacing a
  concrete recovery action.
- `route_owned_reap_decision` no longer uses `live_pane_busy_blocked_prompt` as
  an unbounded keep-alive reason when the authoritative controller lease is
  ready.
- Genuine active turns are still protected from accidental reap/clear.
- `agent-doc session status` makes the inconsistency obvious and names the next
  operator-safe command.
- Regression coverage proves the `commit_already_current` closeout path cannot
  strand the pane in a ready/busy split-brain.
