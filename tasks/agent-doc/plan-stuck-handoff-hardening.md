# Plan — stuck-handoff hardening (#stuckhandoff2)

Root-cause + defense-in-depth for controllers wedged in `--handoff-state preparing`
that survive IDE restart and keep re-corrupting the working tree over `ipc.sock`.

## Root cause

The controller handoff is a two-phase RPC driven by the invoking client:

- `prepare_handoff` → writes `handoff_state = Preparing`, `handoff_started_at = now`
  (`project_controller.rs:3307`).
- `promote_handoff` → writes `Stable`, clears `handoff_started_at` (`:3318`).

If the **client driving the handoff dies / is interrupted / the RPC fails** between
`prepare_handoff` and `promote_handoff`, the controller process stays `Preparing`
forever. There is:

1. **No controller-side deadline** — the serve loop has no self-watchdog; a
   `Preparing` controller never self-terminates.
2. **An external reaper that is record-scoped** —
   `terminate_stale_preparing_controllers_for_caller` reads a single
   `read_bootstrap(project_root)` record and only kills `bootstrap.pid`. An
   **orphaned** wedged controller whose PID is no longer the current bootstrap
   record is invisible to it (a newer clean controller overwrote the record;
   the old process keeps running and writing).
3. **No cross-project sweep** — the reaper fires only for the current invoke's
   `project_root` (`connect_or_launch` + gc). A wedged `boost-client` controller
   is never reaped during an `agent-loop` invoke, so it survives until something
   invokes agent-doc *in boost-client*.
4. **No timer trigger** — the reaper only runs on a new bind/gc, not on a clock.

The reaper (`#kqr6`/`#sjwm`/`#stuckhandoff`, 45s threshold) closed the common
single-record same-project case but not orphaned / cross-project / no-new-invoke
cases. The operator's recovery (`pkill -f 'controller serve ... --handoff-state
preparing'`) is a process-scan kill — exactly what the binary lacks.

## Mitigations (defense-in-depth, ordered by value)

### M1 — controller self-watchdog (suicide timer) [strongest]
On entering `Preparing`, the controller arms an internal deadline; if it has not
observed `promote_handoff`/`retire` within `stale_preparing_controller_threshold()`,
it self-SIGTERMs and marks the bootstrap `Failed`. Removes all dependency on an
external reaper tick — the wedged process cleans *itself* up even with no further
invokes and across project roots. SimWorld: prepare_handoff with no promote →
process exits within threshold.

### M2 — gate worktree/ipc writes on `Stable` [symptom-killer, overlaps #pcwc]
A controller whose `handoff_state != Stable` is non-authoritative for working-tree
writes. Even a still-wedged `Preparing` controller cannot corrupt the worktree
between wedge and reap. Directly shrinks the `#pcwc` post-commit-drift window too.
SimWorld: Preparing controller attempts worktree write → refused/no-op.

### M3 — process-scan reaper [orphan catcher]
Enumerate `/proc` for `controller serve --project-root <root> --handoff-state
preparing` whose start age > threshold and reap them, independent of the bootstrap
`pid`. Reuses the existing `/proc/{pid}` + cmdline + not-self verification in
`reap_verified_controller_pid`. Catches orphaned processes the record-scoped reaper
misses.

### M4 — client handoff drop-guard [prevent at source]
RAII guard around the two-phase handoff on the invoking side: on drop (client
interrupted / RPC error) without a completed `promote_handoff`, send
`retire_after_handoff`/`shutdown` so an aborted handoff never leaves `Preparing`.

### M5 — cross-project gc sweep [breadth]
gc enumerates all known project roots (actor/projection store) and runs the reaper
per root, so a wedged controller in any project is reaped on the next gc tick
regardless of which project triggered it.

## Sequencing
M1 + M2 are the durable core (self-clearing + harmless-while-wedged) and should
land together with SimWorld coverage. M3/M4/M5 are incremental hardening.

## Constraints
- Touches `agent-doc-orchestration/src/project_controller.rs` controller lifecycle
  — must run in a CLEAN session (the submodule currently carries uncommitted
  #fintol work in `write.rs`/`sim_world.rs`/`finalize_integration.rs`; do not
  entangle).
- Every rung needs deterministic SimWorld coverage before commit (no live-only
  proof). Build + `make install` + recycle controllers after landing.

## Status — 2026-06-12 (submodule `08dfc27b`, pushed to origin/main)

**LANDED: M1 + M3** (clean session — the #fintol WIP was stashed for the edit and
restored after; my commit touched only `project_controller.rs` + `gc.rs`, disjoint
from the #fintol files, so no entanglement).

- **M1 self-watchdog** — `serve_with_options` idle poll calls
  `controller_self_watchdog_should_suicide` (reuses `preparing_controller_is_stale`
  on the live in-memory bootstrap) and, when wedged, `controller_self_watchdog_suicide`
  marks the bootstrap `Failed` + logs `stale_preparing_controller_self_reaped` and the
  process exits. No external-reaper / bootstrap-PID / cross-project dependency.
- **M3 proc-scan reaper** — `reap_orphaned_preparing_controllers[_for_caller]` walks
  `/proc` for same-project `controller serve --handoff-state preparing` whose start age
  (`/proc/<pid>` dir mtime, via `process_start_age_secs`) exceeds the threshold and
  reaps via the existing verified-pid path. Wired into `connect_or_launch` and `gc`.
- Coverage: 7 new deterministic unit tests in `project_controller.rs` (sentinel-process
  pattern, mirroring the existing reaper tests — process reaping cannot be modeled in
  SimWorld, so the sentinel/unit pattern is the right deterministic surface here). Full
  suite green (4330 passed). Both binaries rebuilt + installed.

## Status — 2026-06-12 (UPDATE: M2 dispatch-slice / M4 / M5 all landed)

**LANDED: M4 + M5** (`2b643683`) and **M2 dispatch-admission slice** (`3e0503ec`).

- **M4 — HandoffDropGuard**: an RAII guard in `handoff_stale_controller` best-effort
  shuts down the half-launched Preparing replacement if the client aborts
  (early-return/panic) before `promote_handoff`; `complete()` on the success path
  spares the now-authoritative controller.
- **M5 — `reap_orphaned_preparing_controllers_all_projects`**: walks `/proc` for any
  `controller serve --handoff-state preparing` across all project roots and reaps each
  keyed to its own `--project-root` (cross-project equivalent of the operator `pkill`),
  called from `gc::run`.
- **M2 — gate dispatch admission on `Stable`** (the safe slice): `handle_dispatch`
  refuses any dispatch when `handoff_state != Stable`, recording a
  `controller_not_authoritative` receipt + `dispatch_refused_non_stable_controller`
  ops-log line. Read-only status/inspect and operator/handoff/admin RPCs stay available.

**RESOLVED: M2 worktree-write gate** (operator `go` + live Phase-0 re-baseline,
2026-06-12). The controller has no direct autonomous worktree-write path — every
controller-originated worktree mutation (external/operator dispatch AND the
supervisor's self-driving queue drain, `command_kind=idle_queue_continuation`) routes
through `handle_dispatch`, which the M2 dispatch-admission gate already refuses on a
non-Stable controller. Proven by
`autonomous_idle_queue_continuation_refused_when_controller_not_stable`. The remaining
`atomic_write` worktree writes run in the **agent CLI finalize** process; gating those
on the *controller's* `handoff_state` would wrongly block a legitimate finalize and is
the contested **#pcwc** surface, so no separate write_queue gate is added. With M1's
self-watchdog bounding the Preparing window, the wedge cannot corrupt the worktree.
All rungs (M1, M2, M3, M4, M5) are landed with deterministic coverage; **#stuckhandoff2
is complete.**

## Status — 2026-06-13 (M3 wired into the `admin recycle` path)

**Gap found + fixed.** M1–M5 were all present, but the operator-facing `admin recycle`
path did NOT invoke the M3 reaper: `recycle_controller` re-execs only the *authoritative*
controller reachable over the project socket (and early-returned on connect failure),
and `recycle_controllers_all_projects` only looped that per root — so an orphaned
`Preparing` zombie (pid no longer the bootstrap pid, invisible to the socket recycle)
**survived `admin recycle`** and cleared only on M1's later self-watchdog tick or the
next `gc`/`connect_or_launch`. That is exactly the "recycle leaves zombies behind" the
operator caveat described.

Fix: `recycle_controller` now runs the project-scoped M3 reaper
(`reap_orphaned_preparing_controllers_for_caller(..., "recycle")`) **unconditionally**
— even when no authoritative controller answers (an orphan can be the only process in
the root) and regardless of the recycle RPC result. `recycle_controllers_all_projects`
already collects every orphan's root from `/proc` (the orphan's own cmdline matches), so
each orphan's root gets a reaping `recycle_controller` call — no extra cross-project sweep
needed (and avoiding one keeps the recycle path project-scoped so it can't disturb
concurrent tests' sentinels). The shared `stale_preparing_controller_threshold()` spares a
healthy young handoff (including one a recycle just launched). Coverage:
`recycle_reaps_aged_orphaned_preparing_controller` (project-scoped sentinel, threshold
env-overridden to 1s). `make check` green.
