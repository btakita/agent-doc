# Plan: JB `Run Agent Doc` errors when authoritative actor is still `starting`

**Bug class:** route / starting-actor handoff / JB plugin UX
**Related:** [#jbresstart] (refuse cross-file rescue + extend Starting-actor wait), [#jbrsrbusyjb] (JB Restart Supervisor force path)
**Severity:** medium — `Run Agent Doc` is the user's primary IDE-side dispatch and currently fails when the supervisor is mid-start

## Problem

JetBrains "Run Agent Doc" on `tasks/docs.md` produced:

```
resync: stash pane %23 is registered in its own project root — skipping kill
resync: stash pane %11 is registered in its own project root — skipping kill
resync: stash pane %28 is registered in its own project root — skipping kill
resync: stash pane %33 is registered in its own project root — skipping kill
resync: stash pane %13 is registered in its own project root — skipping kill
resync: stash pane %26 is registered in its own project root — skipping kill
[route] debounce OK — file idle for 22.1s and typing indicator idle
[route] target tmux session: 0
Error: authoritative actor generation 56 for tasks/docs.md owns pane %39 but
route will not inject a new trigger because the authoritative actor is still
starting (waited 10s for claude startup). Wait for the pane to show a
dispatch-ready prompt (`prompt_ready=true`), then rerun `agent-doc tasks/docs.md`.
If the pane stays stuck, restart the owner with `agent-doc start tasks/docs.md`.
```

The current CLAUDE.md rule is:

> **Starting actor reroutes promote only proven idle panes** — when the project controller still reports the authoritative actor as `starting`, route may promote it to `ready` only after the live pane shows a harness-specific dispatch-ready prompt, then use the normal managed/dispatch-only send path. If the prompt never becomes dispatch-ready, the route path must fail closed before sending tmux or supervisor input.

The fail-closed path is correct, but the IDE-side UX is broken: clicking `Run Agent Doc` once and getting "wait + rerun" leaves the user babysitting the action. From the JB plugin's perspective, "Run Agent Doc" should either succeed once the prompt becomes ready or fail loudly with a recovery action — not require the user to manually poll.

## Root cause hypotheses

1. **`route` blocks for only 10s on `starting` actors.** Real Claude Code startup (especially after a Restart Supervisor) routinely takes longer than 10s when MCP servers or hooks are warming up. The bounded wait is too tight for the common case.
2. **JB plugin has no retry-on-starting policy.** When the binary exits with the `still starting` message, the IDE notification is terminal — there is no "I'll keep watching for prompt-ready and re-fire when ready" behavior.
3. **`route --wait-for-ready <SECONDS>` doesn't exist** (or isn't surfaced to the JB plugin). The escape hatch is in the error message ("rerun `agent-doc tasks/docs.md`"), not in the action.

## Plan

### Phase 1: Add `--wait-for-ready <SECONDS>` to `route` / top-level `agent-doc <FILE>` [done]

**File:** `src/route.rs`, `src/main.rs` (landed)

1. Accept `--wait-for-ready <SECONDS>` (default: 10, max bounded to e.g. 120) on the route entrypoint. When the authoritative actor is `starting`, poll prompt-ready up to that bound before failing closed.
2. Emit `route_wait_for_ready_elapsed` and `route_wait_for_ready_timeout` ops_log events so the wait outcome is observable.
3. Keep the existing fail-closed behavior at the bound — do not silently send to a non-ready pane.

**Implementation:** `Commands::Route` gained a `wait_for_ready: Option<u64>` clap arg (capped at 600s). `route::run` / `run_with_tmux` accept `wait_for_ready: Option<Duration>` and install a thread-local override via `WaitForReadyOverrideGuard` (RAII, restores prior value on drop, supports nesting). `wait_for_authoritative_actor_ready` reads the override and replaces the harness-specific `authoritative_actor_ready_retry_budget` timeout when set. On success the success path emits `route_wait_for_ready_elapsed`; on timeout the failure path emits `route_wait_for_ready_timeout`. The existing fail-closed semantics are unchanged — the override only widens the bound; it does not promote a non-ready actor.

**Tests:** `route::tests::wait_for_ready_override_guard_sets_and_restores_thread_local` covers the guard mechanics (set, nest, unset, restore).

### Phase 2: JB plugin passes a longer wait on user-initiated actions

**File:** `editors/jetbrains/` Run Agent Doc action

1. When the action is user-initiated (not a background reconcile), invoke `agent-doc <FILE> --wait-for-ready 60` (or a project-configurable value).
2. If the binary still fails closed at the bound, surface a `Retry` button on the IDE notification that re-invokes the same command with the same wait.
3. Optional: integrate the wait progress as a busy spinner so the user does not see a silent 60s wait.

### Phase 3: Telemetry + regression coverage

1. `route` unit tests for `--wait-for-ready` honoring the bound and exiting clean on prompt-ready transition mid-wait.
2. SimWorld scenario: route while authoritative actor is `starting`, prompt becomes ready at 5s — assert success without rerun.
3. JB plugin test (where feasible) for the action-level retry behavior.

## Testing

- `agent-doc <FILE>` against a deliberately slow-starting actor → fails at the new bound with the same message; passing `--wait-for-ready 60` succeeds when prompt becomes ready in <60s.
- JB plugin Run Agent Doc against a starting actor → spinner shows for up to 60s, then dispatch succeeds; if it times out, the Retry button re-fires.
- `make check` clean (clippy + full test suite).

## Open questions

- Is 60s the right user-initiated default, or should the IDE poll indefinitely with a Cancel action?
- Should `--wait-for-ready` be supported on `route --dispatch-only` and other route subcommands, or just the top-level invocation?
