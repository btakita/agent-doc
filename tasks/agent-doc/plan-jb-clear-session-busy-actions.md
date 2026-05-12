# Plan: JetBrains Clear Session Context Busy Actions

## Problem

JetBrains `Clear Session Context` now parses the protected `alive-busy` clear
refusal into a warning instead of surfacing the raw CLI failure, but the operator
still sees it as an error with no direct resolution path:

```text
Session is still running for tasks/root.md. Pane %2 is busy (agent-doc).
Wait for the turn to finish, then retry Clear Session Context.
```

That fail-closed guard is correct while a turn is actively running. The remaining
bug is the control flow around it: the IDE warning offers only retry/status/details,
does not distinguish "still running" from stale lifecycle projection, and does not
provide an intentional interrupt-and-clear action for users who want to discard the
running turn.

Current evidence also shows a split state after the prompt returned:

```text
live_pane: state=alive-idle pane=%2 source=authoritative_actor current_command=agent-doc prompt_ready=true
supervisor: health=healthy state=healthy actor_state=busy
controller_lease: generation=41 ... runtime_state=busy
```

So the fix should address both the IDE action set and the lifecycle reconciliation
that can leave supervisor/controller busy after direct pane evidence is idle.

## Plan

1. Add a JetBrains `Interrupt and clear` action for protected busy clear refusals.
   It should require explicit confirmation, send the harness-specific interrupt for
   the owning pane, wait for direct pane evidence to become `alive-idle` or closed,
   then retry `agent-doc session clear <FILE>`.
2. Add a `Refresh and retry` path that runs `agent-doc session status <FILE>` first.
   If direct pane evidence is already idle, retry clear immediately instead of
   making the user manually invoke Clear again.
3. Reconcile lifecycle state when direct pane evidence is idle but supervisor or
   controller projection remains busy. The status/clear path should either mark the
   actor ready or surface a precise stale-projection diagnostic with the lease and
   latest committed cycle.
4. Tighten the notification copy so the primary message is a protected running
   session state, not a generic error. Include pane id, current command, latest tail,
   and the two explicit choices: wait/retry or interrupt/discard.
5. Add regressions for:
   - busy clear refusal still fails closed by default;
   - IDE `Interrupt and clear` confirms, interrupts, waits, and retries;
   - `alive-idle` direct evidence repairs or supersedes stale controller/supervisor
     busy projection;
   - the warning text never includes `agent-doc command failed` for this typed case.

## Verification

- JetBrains unit coverage in `TerminalUtilTest` for the new notification actions and
  message text.
- Rust controller/session command coverage for idle direct-pane evidence with stale
  busy projection.
- A manual live replay on `src/session-share/tasks/root.md`: trigger Clear while the
  pane is running, confirm the warning appears, then test both wait/retry and
  interrupt-and-clear.
