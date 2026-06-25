# Plan: Actor-switch route defer needs an operator-actionable recovery path (`#actorswitchdefer`)

Operator ask (agent-doc-bugs2.md queue 2026-06-24): JB `Run Agent Doc` on
`tasks/professional/equityfundingsource.md` after switching the doc from
`agent: codex` to `agent: opencode` printed:

```
resync: pruned 2 stale session(s)
[route] target tmux session: 0
Error: authoritative actor record for tasks/professional/equityfundingsource.md is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane
```

…and then **nothing happened**. The session was stranded: route refused to
replace the live codex pane, and no boundary restart brought up opencode.

## Root cause (this is intended behavior that dead-ends)

The bail is **not** a regression — it is the `#agentreloadrestart` contract
(VERSIONS.md:223, plan-restart-agent.md):

- `route/authoritative_actor.rs:40-95`: when the authoritative actor record's
  stored `harness` (codex) differs from the frontmatter-resolved harness
  (opencode) AND `frontmatter_harness_changed == true`, route calls
  `mismatched_authoritative_actor_can_be_replaced`. That helper returns `false`
  while the old actor is a **healthy, non-closed** authority
  (`authoritative_actor.rs:147-154`, test
  `mismatched_authoritative_actor_can_be_replaced_only_when_not_live_authority`).
  Route then logs `route_authoritative_actor_harness_mismatch_deferred
  action=defer_to_boundary_restart` and bails with the message above instead of
  cold-replacing the live pane (this is the kill-guard added in plan-restart-agent
  Phase 1 after the equityfundingsource mid-turn kill).
- The deferred work is owned by the **supervisor idle-queue watch**
  (`start/idle_watch.rs:715-803`): each tick it re-resolves the harness from
  current frontmatter and, when `agent_change_restart_decision` returns `Restart`
  (requires `harness_changed && agent_change_restart_enabled && prompt_visible &&
  !turn_active`), requests a fresh restart (`restart_requested=true`,
  `restart_mode="fresh"`), and `run.rs` respawns the new harness
  (`agent_restart_performed`).

So the mechanism is fully shipped (Phase 1a detection + Phase 1b execution).
**The bug is that the defer is a dead-end when the idle watch cannot reach the
restart boundary, and route gives the operator no way to see or force it.**

## Why it dead-ended here

The supervisor idle-watch only fires the restart when it can tick into a
`prompt_visible && !turn_active` boundary. That fails to happen when any of:

1. **Supervisor paused / stale.** This very session is `queue_paused: true`
   (stale `#rt83/#qflood` pause). A paused or stale-binary supervisor does not
   tick the idle watch, so `harness_change_detected` is never emitted and the
   restart never fires.
2. **Pane never returns to a proven dispatch-ready prompt.** A busy/closed/
   stuck codex pane keeps `turn_active` true or `prompt_visible` false, so the
   decision stays `WaitForBoundary` forever.
3. **`agent_change_restart_enabled` resolved false** (env / frontmatter /
   project knob off).

In all three the route bail is the last thing the operator sees; nothing tells
them the deferred restart is stuck, and nothing hands them a command to force
it.

## The fix (two parts)

### Part A — route surfaces recovery + supervisor state in the defer bail (binary)

In `route/authoritative_actor.rs:69-89`, before bailing, query the supervisor
runtime that is already being read (`query_supervisor_runtime` at line 44) and
enrich the bail so it is operator-actionable instead of a dead-end:

- If supervisor `health != Healthy` **or** queue `paused`: append
  `; supervisor is {unhealthy|paused} — the boundary restart will not fire until it is healthy and running. Run: agent-doc session restart-supervisor <FILE>` to the message.
- If the actor is `closed`/`blocked` or the pane is not dispatch-ready: append
  `; pane is not at a dispatch-ready boundary — run: agent-doc session restart-supervisor <FILE> --force`.
- Otherwise (healthy + ready): keep the existing wording but append
  `; the supervisor idle-watch will restart the harness at the next idle boundary. To force it now: agent-doc session restart-supervisor <FILE>`.

The `session restart-supervisor <FILE>` command already re-resolves the harness
(plan-restart-agent Phase 1), so it is the correct single-shot recovery for
every branch. This is a message/diagnostic change + a small read of runtime
fields already in scope — no routing-contract change, low risk.

### Part B — deterministic proof the boundary restart fires when deferred (binary + tests)

The route-side defer asserts "the supervisor will restart at the boundary."
Today nothing proves that. Add:

- A route-side check at the defer site: if `agent_change_restart_enabled` is
  false, the bail must say so explicitly (`agent_change_restart is disabled;
  Run Agent Doc will not switch harnesses until re-enabled or
  agent: reverts`), because in that state the defer will *never* self-heal.
- SimWorld test: doc switches `agent: codex → opencode` with a healthy codex
  authoritative actor; assert route bails with the defer message AND the
  supervisor idle-watch emits `harness_change_detected` +
  `agent_restart_triggered` within the bounded boundary window, reaching
  `agent_restart_performed old=codex new=opencode`.
- SimWorld test (regression for this bug): same switch but supervisor `paused`
  or pane stuck `busy`; assert route bail includes the paused/stuck suffix and
  the `session restart-supervisor <FILE>` recovery hint, and assert the
  supervisor does **not** silently drop the deferred restart.

## Spec / doc alignment

- `SPEC.md:141` says an explicit harness switch must "fall back to the normal
  create/rebind path." The code intentionally does NOT do that for a healthy
  old-harness actor (it defers). Reconcile the spec wording to state the defer
  contract and the `session restart-supervisor` recovery, so the spec matches
  the shipped kill-guard behavior instead of contradicting it.
- `specs/supervisor.md:101` already references the boundary restart path;
  extend it with the route-side enriched diagnostic and the paused/stuck
  recovery contract.
- VERSIONS entry under the next release.

## Scope / sequencing

- Part A is small, safe, and unblocks operators today — do it first, in one
  focused cycle, with `make check` + `make tmux-ci`.
- Part B (deterministic SimWorld proof) lands with or right after Part A so the
  defer contract becomes provable, not aspirational.
- No JB plugin change required for recovery (`session restart-supervisor` is
  CLI-driven); a later follow-up can wire the existing `Restart Agent` menu
  command (plan-restart-agent Phase 2) as the one-click path.

## Out of scope

- Changing the `mismatched_authoritative_actor_can_be_replaced` fail-closed
  rule for healthy actors (that is the kill-guard; do not weaken it).
- Auto-unpausing a stale/paused supervisor from inside route (operator action;
  route should only surface the recovery command).
