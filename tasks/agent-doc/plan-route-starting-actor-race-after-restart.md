# Plan: Route Fails on Freshly Started Actor Still in Starting State

**Bug class:** route/start race condition
**Related:** `#startgcleak`, `#startingdispatch`, `#stalegenfix`
**Severity:** medium — user must wait and retry manually after `agent-doc start`

## Problem

After the user kills an agent-doc supervisor and runs `agent-doc start <file>`, the JB
plugin's `Run Agent Doc` (route) fails with:

```
Error: authoritative actor generation N for <file> owns pane %1 but route will not
inject a new trigger because the authoritative actor is still starting.
```

### Root cause

`agent-doc start` creates the actor record in state `Starting`, then spawns the child
harness process (OpenCode / Claude Code / Codex). The actor only transitions to `Ready`
when the pty filter thread detects a dispatch-ready prompt in the child output.

When the JB plugin routes before the child harness boots:

1. `promote_starting_authoritative_actor_if_dispatch_ready` checks the pane with a 1.2s
   timeout — the child is still booting, so no ready prompt is found.
2. `wait_for_authoritative_actor_ready` polls for 5s — still not ready.
3. Route bails with the "still starting" error.

The 5s wait is insufficient for harnesses that take 10+ seconds to boot (especially
OpenCode in sandboxed environments).

### Evidence

- User killed the agent-doc supervisor for `tasks/root.md`
- Ran `agent-doc start .../root.md` in the console
- JB plugin `Run Agent Doc` triggered immediately
- Error: generation 18, pane %1, actor_state=Starting

## Plan

### Phase 1: Extend route wait with harness-aware timeout

**File:** `src/route.rs`

1. Replace the fixed 5s `dispatch_only_starting_pane_recovery_timeout()` with a
   harness-aware budget:
   - OpenCode: 15s (sandbox + network capability proof can take 10+ seconds)
   - Claude Code: 10s
   - Codex: 8s
   - Default: 5s
2. Log the elapsed wait time when the timeout expires so ops.log shows whether the
   child was close to ready.
3. Keep the hard bail for safety but include the elapsed time in the error message.

### Phase 2: Start command wait-for-ready exit flag

**File:** `src/start.rs`

1. Add a `--wait-for-ready` flag (or always wait when started from the JB plugin /
   non-interactive context).
2. After `register_supervisor`, block until the shared actor state transitions to
   `Ready` or until a budget expires (same harness-aware timeout).
3. Print a status line like `[start] waiting for harness ready prompt... (3/15s)` so
   the user sees progress.
4. Return exit code 0 only when `Ready`, exit code 75 (EX_TEMPFAIL) if timeout.

### Phase 3: JB plugin retry with backoff

**File:** `editors/jetbrains/` (Kotlin plugin)

1. When route fails with "still starting", automatically retry up to 3 times with
   exponential backoff (2s, 4s, 8s).
2. Show a progress notification in the IDE: "Waiting for agent to start..."
3. Surface the final error only after all retries are exhausted.

### Phase 4: Ops-log visibility

**File:** `src/route.rs`, `src/start.rs`

1. Log `route_starting_actor_wait_elapsed_ms` on timeout.
2. Log `start_actor_ready_after_ms` when the transition completes.
3. Add a `agent-doc session status <file>` summary line showing
   `actor_state=Starting wait_elapsed=...` for diagnostics.

## Testing

- Unit: `dispatch_only_starting_pane_recovery_timeout` returns harness-specific values.
- Integration: `agent-doc start --wait-for-ready <file>` returns 0 when the harness
  boots within budget, 75 when it times out.
- SimWorld: route-into-starting-actor schedule that exercises the wait budget and
  verifies the bail message includes elapsed time.
