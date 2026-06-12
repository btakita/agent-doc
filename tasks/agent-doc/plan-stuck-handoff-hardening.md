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

**DEFERRED: M2 / M4 / M5** —

- **M2 (gate worktree/ipc writes on `Stable`)** needs operator direction. Investigation
  found the controller's IPC request handlers do NOT write the session-document worktree
  — they write the state DB / bootstrap-state file. M2's real worktree-write surface is
  the finalize/commit/reposition path, which overlaps the contested **#pcwc** carry-forward
  invariant (memory: "don't re-wire #fintol/#pcwc without operator go + live Phase-0
  re-baseline"). M1 already bounds the Preparing window, so the wedge can't persist long
  enough to corrupt much before self-reap — partially achieving M2's goal.
- **M4 (client handoff drop-guard)** and **M5 (cross-project gc sweep)** remain as
  incremental hardening; not blocked, just unstarted.
