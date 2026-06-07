# Plan: JB `Run Agent Doc` busy-queue dispatch deadlock (monsterrodholders.md)

## Status

**Update 2026-06-02:** operator issued `do [#jb-busy-inactive-queue-dispatch]`,
superseding the plan-only status for the **deterministic** scope. Split the work:

- **Implementable + unit-testable now (no live pane):**
  (a) Fix-direction bullet 2 — explicit `Run Agent Doc` that finds a runnable
  `agent:queue auto` head while `queue_active:false` on a busy owned pane must
  activate the queue + arm the idle-drain watcher, OR fail closed with a typed
  deferred/busy diagnostic; it must never record `keep_alive` +
  `queue_continuation_required=false` as a silent no-op.
  (b) Fix-direction bullet 3 — emit the deferred-queued operator feedback.
  Both are route.rs / session-check / queue_continuation.rs logic with
  deterministic SimWorld coverage.
- **Still gated (live two-session Codex pane):** the Verification → Live repro.

The sibling idle-drain watcher already shipped for the `queue_active:true`
route-appended case (`spawn_idle_queue_watch_thread` / `idle_queue_drain_decision`,
submodule 4b3f4835); this item extends the *same* watcher to the
`queue_active:false` pre-existing-head case rather than building a new mechanism.

## Report

Operator ran JB `Run Agent Doc` on `monsterrodholders.md` (a **Codex** session:
`agent: codex`, `required_ssh_targets: [monsterrodholders-server]`) while its
authoritative actor was mid-turn. Route log:

```
[route] debounce OK — file idle for 0.5s and typing indicator idle
[route] target tmux session: 0
[route] authoritative actor generation 245 for tasks/monsterrodholders.md is busy on pane %40; queued pending dispatch "Are there any items in review that are actionable right now?" in agent:queue auto (appended=true, already_present=false, superseded=false) instead of injecting a duplicate trigger
```

Symptom: the dispatch never runs ("deadlock") — the prompt is queued but is not
delivered after the pane returns to idle.

## Fresh Repro: Pre-existing inactive queue head

2026-06-02 correction: the live report was on
`/home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md`,
not `agent-doc-bugs2.md`.

Observed state after JB `Run Agent Doc`:

- Session `a9421282-fd96-4943-9af5-3561ed5cb799`, harness `codex`,
  actor generation 255, pane `%61`.
- The document had an uncommitted user-added queue head:
  `- [#shipstationaudit]. #spec-test-commit-push` under `agent:queue auto`.
- Frontmatter still had `queue_active: false`.
- `agent-doc session status` reported the actor projection as `ready`, but the
  live pane as `alive-busy` / `prompt_ready=false`.
- The latest session log event was
  `route_owned_reap_decision policy=auto decision=keep_alive
  reason=live_pane_busy_blocked_prompt`.
- `agent-doc session-check` returned ok with `queue_continuation_required=false`.
- `tmux capture-pane -pt %61` showed a real active Codex turn, not stale idle
  scrollback.

This is a sibling of the route-appended prompt case, but the queue head already
exists and is inactive. The route-owned busy keep-alive path neither activates the
queue nor installs an idle-drain continuation, so the operator sees "Run Agent
Doc" do nothing even though there is a runnable queue head in the document.

## What the code does (grounded)

- `route.rs` `AuthoritativeActorDispatchAction::DispatchOnlyBusyQueue`
  (`route.rs:4003`): when the authoritative actor is `busy` and there IS a
  `prompt_context`, route calls `enqueue_route_dispatch_prompt(...)` and returns
  `Ok(dispatch_pane)`. It does **not** inject into the busy pane (correct — the
  no-inject-into-active-turn invariant) and does **not** itself arrange any
  follow-up dispatch.
- `enqueue_route_dispatch_prompt` (`route.rs:1806`): merges `queue_active: true`
  into frontmatter and appends the prompt to `agent:queue auto` (or supersedes the
  sole auto prompt). Here it `appended=true`, so the queue now has the prompt and
  is marked active.
- Drain after idle is **harness-delegated**: nothing in the route call schedules
  the dispatch. The queue only drains when a *new* cycle runs against the document:
  - Codex: the `Stop` hook (`codex_hook.rs`) on turn-end.
  - Claude Code: the auto-loop (`/loop`) while a session is actively looping.
  - Any editor `Run Agent Doc` / file edit / `agent-doc route` that re-runs preflight.

## Candidate root causes (to confirm during implementation — not yet proven)

1. **No guaranteed idle-drain trigger.** Route enqueues + returns `Ok`, but if the
   busy Codex turn (gen 245) never cleanly reaches a `Stop` that re-runs the cycle,
   the queued prompt sits forever. A turn blocked on `required_ssh_targets`
   (monsterrodholders-server), a permission prompt, or a Codex hook-review is
   "busy" but may never emit the normal turn-end Stop that drains `agent:queue auto`.
2. **Stop-hook drain skips a route-appended prompt.** `codex_hook.rs:293` guards
   re-dispatch with `stop_hook_active && last_auto_queue_head == Some(prompt)`. Need
   to verify the Stop hook reliably drains a prompt that was *appended by route*
   (not one the hook itself queued), and that the loop-guard / dedup does not skip it.
3. **Operator-facing silence.** Route returns `Ok` after queuing, so JB reports a
   successful run with no "deferred — queued, will dispatch when the session goes
   idle" feedback. The operator perceives a hang/no-op (the "deadlock" they see),
   even when the logical queue state is correct. This is the same feedback-gap
   family as `#jb-run-agent-doc-command-route-miss`.

## Fix direction (when unblocked)

- Guarantee a drain trigger for a route-enqueued busy prompt: when route appends to
  `agent:queue auto` because the pane is busy, ensure the busy session's idle
  transition (Codex Stop hook / Claude auto-loop) is the authoritative drain, and
  add a watch/idle re-dispatch fallback for the prompt-bearing case — the
  prompt-bearing sibling of `#opencode-active-turn-reopen-auto-retry-on-idle`
  (which covers only the plain, no-prompt reopen). Must keep the
  no-inject-into-active-turn invariant.
- Cover the pre-existing inactive-head variant: when explicit `Run Agent Doc`
  sees a runnable `agent:queue auto` head while `queue_active:false` and the owned
  pane is busy, it must either activate the queue and arm the same idle-drain
  watcher, or fail closed with a typed deferred/busy diagnostic. It must not
  silently keep the pane alive with `queue_continuation_required=false`.
- Surface explicit operator feedback from the JB plugin that the run was **deferred
  and queued** (not dispatched), with the queue position, so a busy-pane run does
  not read as a silent success/hang.
- Add deterministic coverage: a SimWorld / unit test that a busy authoritative
  actor with a route-appended `agent:queue auto` prompt drains on the next idle
  Stop, plus a regression that a blocked/never-idle turn surfaces the deferred
  state instead of silently stranding the prompt.

## Verification

- Deterministic: Stop-hook drain of a route-appended busy-queue prompt; superseded
  vs appended shapes; loop-guard does not skip a freshly route-appended prompt.
- Live (gated, operator-present): real Codex `monsterrodholders.md` pane mid-turn,
  run JB `Run Agent Doc`, confirm the prompt dispatches once the turn ends (or
  surfaces a clear deferred-queued notification), with no duplicate injection into
  the active turn.

## Related

- `#opencode-active-turn-reopen-auto-retry-on-idle` — plain (no-prompt) reopen
  auto-retry-on-idle; this item is the prompt-bearing sibling.
- `#jb-run-agent-doc-command-route-miss` — busy-pane Run Agent Doc operator-feedback gap.
