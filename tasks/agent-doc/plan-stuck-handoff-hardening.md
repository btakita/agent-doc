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

## REOPENED — 2026-06-13 (M1 self-watchdog is dead in production)

**#stuckhandoff2 is NOT actually complete.** Live-repro this session: a controller wedged
in `--handoff-state preparing` (pid `2434403`, gen 1060) survived **13.5 minutes** and had
to be `pkill`ed by hand. Binary install at `03:57:27`; that controller started `03:57:41`
(14s AFTER install) — so it ran the **fresh M1-equipped binary**, ruling out the "stale
running binary" explanation.

**Evidence (superproject `.agent-doc/logs/ops.log`):**
- `stale_preparing_controller_self_reaped` (M1 self-watchdog, `caller=self_watchdog`):
  **ZERO occurrences, ever.**
- `orphaned_preparing_controller_reaped_cross_project … caller=gc`: fires **repeatedly**
  (pids 401211, 1151060, 1324526, 1663915, 2011236, 2214912, …) at ages **442–1286s**,
  all `threshold_secs=45`.

So orphaned `Preparing` controllers are cleaned up **only** by the slow external gc/M5
sweep — at 7–21 minutes, not 45s — which is exactly the external-reaper-tick dependency
the plan claimed M1 eliminated. M1 (`controller_self_watchdog_should_suicide` /
`controller_self_watchdog_suicide`, `serve_with_options` `WouldBlock` branch) is present,
unit-tested as a pure predicate, and reached by the handoff-replacement launch path
(`write_bootstrap_with_options` stamps `handoff_started_at` when launched `Preparing`), yet
the suicide **never triggers at runtime**.

**Root cause NOT yet pinned** (investigated read-only; not fixed — deliberately deferred a
speculative edit to clean-session live-write-path code in a corrupting live session).
Narrowed hypotheses for the next clean-session cycle, with a deterministic repro:
1. The orphaned replacement's serve loop never reaches the `WouldBlock` idle branch (so the
   watchdog poll never runs) — e.g. blocked earlier (`ControllerRuntime::new` →
   `recover_controller_after_restart`), or `listener.accept()` not returning `WouldBlock`
   as expected for a non-blocking listener that owns a handoff-specific socket.
2. The in-memory `handoff_state` is flipped off `Preparing`/`Promoted` (e.g. a partial
   `promote_handoff`) so `preparing_controller_is_stale` returns false, while the frozen
   `/proc` cmdline still reads `--handoff-state preparing` (which is what gc/M5 keys off) —
   i.e. M1 (in-memory truth) and M5 (cmdline) disagree about the same process.
3. The watchdog deadline is computed once at loop entry and the orphan's poll cadence /
   should_stop wiring prevents the branch from being evaluated after 45s.

**Next-cycle task (clean session):** add a deterministic test that drives a launched-Preparing
controller's serve loop to the watchdog branch and asserts a `stale_preparing_controller_self_reaped`
within threshold; then fix whichever of (1)–(3) holds. Until M1 actually fires, the wedge
window is gc-cadence (minutes), not 45s. (Could not be re-filed into `agent:backlog` this
session — the doc's fenced `<!-- agent:queue -->` examples break the granular-write parser,
`#lintfence`.)

## RESOLVED — 2026-06-15 (root cause = hypothesis #2; M1b structural watchdog landed)

**Root cause confirmed in code (hypothesis #2).** `promote_handoff` (`rpc.rs` handler)
flips a replacement straight to `handoff_state = Stable` and clears `handoff_started_at`
the instant the client asks — `ControllerHandoffState::Promoted` is parsed/serialized/
tested but **never written as a production transition**. The handoff client then does
`std::fs::rename(temp_sock → public_sock)` (`handoff_stale_controller`). A client that dies
*after* `promote_handoff` but *before* that rename leaves a `Stable`-in-memory controller
**stranded on its temp `controller-handoff-*` socket**. M1's
`controller_self_watchdog_should_suicide` predicate only fires on `Preparing`/`Promoted`,
so it cannot see a `Stable` orphan — exactly why `stale_preparing_controller_self_reaped`
had zero occurrences while the `/proc`-cmdline gc/M5 sweep (which keys off the frozen
`--handoff-state preparing` cmdline) reaped them at 7–21 min. The wedged orphan races the
IDE/ipc.sock buffer during that window (observed live 2026-06-15: it injected `❯ ` prompt
glyphs and spliced duplicated/reordered response lines into agent-doc-bugs2.md mid-finalize).

**Fix (M1b — structural stranded-replacement watchdog), `rpc.rs`:**
- `controller_handoff_replacement_is_stranded(handoff_temp_socket, launched_elapsed, threshold)`
  — independent of in-memory `handoff_state`. `serve_with_options` captures the launch
  socket as `handoff_temp_socket` whenever it differs from the public socket (a
  `controller-handoff-*` temp socket = a replacement). A *completed* handoff removes that
  path via the promote rename, so a temp socket that **still exists past the threshold**
  proves the promotion never finished → self-reap. The `launched_elapsed > threshold` guard
  spares a healthy young handoff. Wired into the serve-loop `WouldBlock` branch as
  `should_suicide || is_stranded`.
- Hardened `controller_self_watchdog_suicide` with a generation-ownership guard: it now only
  supersedes the **shared per-project** bootstrap state file with `Failed` while that file
  still names THIS controller (pid + generation). A stranded replacement can be a stale
  generation a newer clean controller already recorded — clobbering it to `Failed` would
  churn the next bind into relaunching over a healthy controller. When the record is no
  longer ours, the process still exits (that stops the buffer race) but leaves the on-disk
  record correct.
- Coverage (6 new deterministic unit tests in `project_controller.rs`):
  stranded-when-temp-persists, not-stranded-after-rename, not-stranded-within-threshold,
  none-socket-never-stranded, suicide-marks-failed-for-stranded-`Stable`,
  suicide-preserves-superseded-generation.

Net effect: a wedged handoff orphan now self-terminates within `threshold` (45s default)
regardless of whether it died before or after `promote_handoff`, instead of surviving on
the minutes-scale gc cadence. The binary clears the invalid state and the next bind promotes
a clean controller — no manual `pkill` / `git checkout` recovery needed.
