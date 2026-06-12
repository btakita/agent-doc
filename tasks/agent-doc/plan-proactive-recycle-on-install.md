# Plan — proactive recycle-on-install (`#ctlrecycle`)

Stop long-lived agent-doc processes from serving OLD code after a `cargo install`
/ `lib-install` until something happens to restart them. Successor to the
`#ctlstalebin` dispatch backstop (shipped fb2fb204), which only catches the
cross-process dispatch path and cannot recycle the in-process supervisor that
actually drove the observed ~1h churn.

## Root cause

Installing replaces a *file*; the bug lives in *processes*.

1. **Replacing the binary does not touch running processes.** `cargo install` /
   `cargo build --release` / `agent-doc lib-install` overwrite the on-disk
   binary + cdylib. A running process keeps executing the old mapped inode until
   it actually restarts. Every controller, `start --route-owned` supervisor, and
   `mcp serve` that was already up keeps pre-install behavior.
2. **Recycle is reactive, not proactive.** The only promotion onto a fresh binary
   is `connect_or_launch` (`controller_status_matches_current_binary` →
   two-phase handoff), and it fires only when a NEW cross-process invoke arrives.
   `cargo install` sends no signal to running daemons. M1's self-watchdog covers
   only handoff-`Preparing` staleness, not binary staleness.
3. **Many independent old-binary islands.** A per-project `controller serve`, a
   `start --route-owned` supervisor per live document, `mcp serve`, plus the same
   set under sibling project roots. Recycling one (because it got a fresh invoke)
   never recycles the others.
4. **The 08b in-process supervisor is immune.** `start --route-owned` hosts the
   agent child in-process (`InProcessSupervisor`, `supervisor_host_gate
   hosting=in-process`) and persists via `write_authority` without ever
   re-entering `connect_or_launch`. The one process driving the churn is the one
   that never re-checks its binary.
5. **Dogfooding amplifies it.** agent-doc fixes itself, so the live session
   exhibiting a bug is always one binary-generation behind the fix just shipped —
   the evidence trails the fix by one recycle.

## Design principle — self-stat, no coordination artifact

Each long-lived process is responsible for recycling *itself*. On an idle tick it
compares its **recorded launch identity** (`ControllerBinaryIdentity` captured at
spawn) against `current_binary_identity()`, which `std::fs::metadata`s the install
*path* — so it returns the freshly-installed binary's len/mtime even though the
process is still running the old mapped inode. Different → stale. This is exactly
the `#ctlstalebin` predicate (`controller_binary_is_stale`), generalized.

- No marker file, no IPC fan-out, no install-time coordination, no cross-project
  registry walk — `/proc` self-knowledge is the index.
- Fail-open: any stat/resolution error means "not stale" (never block live work on
  an unreadable path).
- Generalize `controller_binary_is_stale(bootstrap)` into
  `process_binary_is_stale(recorded: &ControllerBinaryIdentity) -> bool` shared by
  the controller and the supervisor.

## Hard invariant — recycle only when provably idle

A recycle must NEVER interrupt in-flight work. "Idle" is process-type specific and
must be proven, plus a debounce so we never recycle mid-rapid-cycle:

- **Controller idle:** no open in-flight dispatch for ANY document
  (`has_open_in_flight_dispatch` swept over the actor set), no Preparing handoff,
  no active client request thread.
- **Supervisor idle:** between cycles — no agent turn in flight (no open
  `preflight_started`/`response_captured`/`write_applied` cycle), queue drained or
  paused (`queue_active: false`), child at an idle prompt.
- **Debounce:** stale AND idle continuously for `recycle_idle_grace` (default ~5s)
  before acting, so a brief lull between queue items does not trigger a recycle.

## Mitigations (ordered by value / ascending risk)

### R1 — controller idle self-recycle [low risk, do first]
Extend the `serve_with_options` idle-poll `WouldBlock` branch (where M1's
self-watchdog already lives) with a binary-staleness check. When the controller's
binary is stale AND no dispatch is in flight for any document AND debounced,
self-terminate: drop the listener, remove the socket, log
`controller_binary_stale_self_recycled pid=… old=… new=…`, exit. The next
`connect_or_launch` relaunches the fresh binary; SQLite state is on disk, so a
coordinator exit loses nothing. Safe because it is the same "exit when idle"
shape M1 already ships.

### R2 — `agent-doc admin recycle [DOCUMENT] [--all-projects]` [explicit, release-workflow]
A graceful operator-facing command (controller-mediated, IPC-safe — the same
class as `admin queue pause`). Marks the target controller(s) "recycle when idle":
on the next idle tick they self-terminate as in R1. `--all-projects` enumerates
known project roots (reuse the M5 `/proc` sweep) and signals each. This makes the
release workflow deterministic — `cargo install && agent-doc admin recycle
--all-projects` — instead of relying on the automatic tick or a manual `pkill`.
No live session is killed mid-turn; the mark is honored only at an idle boundary.

### R3 — supervisor idle self-recycle via re-exec [the real fix, flag-gated]
The `start --route-owned` process runs the agent child in a persistent restart
loop. Add a binary-staleness check at the restart-loop boundary AND on the idle
queue-drain tick (the existing idle signal at `start.rs:346`):

- When stale AND idle AND debounced, recycle by **re-exec** (`execv` with the same
  argv) so the PID is preserved and the fresh `start` re-establishes hosting. Do
  it at the child-restart boundary where the child is already being torn
  down/respawned, so no live turn is lost; if a child is currently adopted and
  alive-idle, kill+respawn it under the new image (the supervisor already owns a
  spawn/adopt path).
- Log `supervisor_binary_stale_self_recycled pid=… old=… new=… via=reexec`.
- **Flag-gated opt-in initially** (`AGENT_DOC_SUPERVISOR_AUTO_RECYCLE=1`): killing
  /re-exec'ing a process that hosts the operator's live agent conversation is
  intrusive, so default OFF until validated. When OFF, still surface staleness:
  log `supervisor_binary_stale_detected` and (optionally) a JB notification so the
  operator can restart deliberately.

### R4 — close the install loop [glue]
Have `cargo install`-adjacent flows hint the recycle: `agent-doc lib-install` and
the documented release step print "running controllers/supervisors still serve
the prior binary — run `agent-doc admin recycle --all-projects` (or restart
sessions) to promote it." Optionally auto-invoke R2 from `lib-install`. Keep
`SKILL.md`/`README.md`/`SPEC.md`/release runbook aligned on the new step.

## Sequencing
R1 + R2 are the durable, low-risk core (idle-gated coordinator exit + explicit
trigger) and should land together with sentinel/unit coverage. R3 is the
behavioral heart but touches the live-supervisor exit path — land it flag-gated
with the detect-only fallback first, validate against live sessions, then flip the
default. R4 is doc/glue, land alongside R2.

## Testing
Process recycle cannot be modeled in SimWorld (same constraint as M1/M3 — see
`plan-stuck-handoff-hardening.md`). Surface the deterministic seams as **pure
predicates** + **sentinel-process unit tests**:
- `process_binary_is_stale` — pure, unit-tested (matching identity → false;
  differing len/mtime/version → true; stat error → false). Reuse the
  `#ctlstalebin` test shape.
- `controller_should_self_recycle(stale, in_flight_dispatch, grace_elapsed)` —
  pure decision, unit-tested across the truth table (stale+idle+debounced → true;
  stale+in-flight → false; fresh → false).
- `supervisor_should_self_recycle(stale, turn_in_flight, queue_active,
  grace_elapsed, flag)` — pure decision, unit-tested.
- Controller exit + relaunch: sentinel-process test (spawn a fake same-project
  `controller serve`, mark stale, assert reaped/exited) mirroring the M3 reaper
  tests.
- `make check` green; review the tmux CI leg per the test-bearing-turn rule.

## Constraints
- Touches `agent-doc-orchestration/src/project_controller.rs` (R1/R2) and
  `agent-doc-orchestration/src/start.rs` + `supervisor/in_process.rs` (R3) —
  controller + supervisor lifecycle. Run from a CLEAN session; R3's re-exec must
  not entangle the contested #pcwc finalize path.
- Every rung idle-gated + debounced; fail-open on stat errors.
- Build + `cargo install` + `lib-install` + recycle the running fleet after
  landing (this plan's own dogfood: the fix only proves out post-recycle).

## Relationship to `#ctlstalebin` (shipped, fb2fb204)
`#ctlstalebin` refuses a dispatch that reaches a *stale controller's*
`handle_dispatch` and retries once to promote the fresh binary — a reactive
backstop on the dispatch path only. This plan is the proactive complement: idle
processes recycle themselves (R1/R3) or on explicit operator signal (R2), so a
stale process is gone before it can churn, not merely refused per-dispatch.

## Open operator decisions
1. **R3 aggressiveness** — auto-recycle the supervisor when idle (intrusive: ends
   the live agent child), or detect-and-surface only (operator restarts). Recommend
   flag-gated auto OFF by default + detect-only fallback first.
2. **Recycle mechanism for the supervisor** — re-exec (preserve PID, re-adopt/
   respawn child) vs clean-exit-and-let-spawner-relaunch. Recommend re-exec at the
   restart boundary.
3. **Automatic vs explicit** — ship R1 automatic + R2 explicit (recommended), or
   explicit-only to start (least surprising).
4. **Default-on timeline** — when (if) R3 flips from opt-in to default.

## Status — LANDED (R1+R2+R3+R4, full flag-gated)

All rungs implemented in the agent-doc submodule (clean Claude session):

- **Foundation** — `process_binary_is_stale(recorded)` (generalized from
  `#ctlstalebin`), `recycle_idle_grace()` (`AGENT_DOC_RECYCLE_IDLE_GRACE_SECS`,
  default 5s), and the pure `recycle_debounce_decision(...)`. Unit tests
  `process_binary_is_stale_matches_and_differs`,
  `recycle_debounce_decision_requires_continuous_idle_grace`.
- **R1** — controller serve-loop idle self-recycle: `controller_wants_recycle`
  (stale OR `recycle_requested`) gated on `controller_recycle_idle`
  (`state_store::has_any_open_in_flight_dispatch` == false + `Stable`), debounced;
  exits with `controller_self_recycled reason=stale_binary|operator_request`. The
  idle DB probe runs only when a recycle is actually wanted, so the hot path stays
  an atomic load + one `stat`.
- **R2** — `agent-doc admin recycle [--project-root R] [--all-projects] [--json]`:
  a `recycle` controller RPC sets `recycle_requested` (idle-gated, never interrupts
  a turn); `--all-projects` walks `/proc` for controllers and recycles each.
- **R3** — `start` supervisor idle self-recycle in the idle-queue watch:
  `supervisor_stale_action(stale, idle, auto)` → `Recycle`/`Detect`/`None`. **Chose
  `process_exit`-and-relaunch over re-exec** (open decision #2) — simpler and safer
  than re-adopting an orphaned PTY child; the route-owned pane relaunches on the
  fresh binary. Default OFF behind `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE`; when off, a
  stale idle supervisor logs `supervisor_binary_stale_detected` once. Unit test
  `supervisor_stale_action_policy`.
- **R4** — `agent-doc lib-install` now prints the deterministic recycle hint
  (`admin recycle --all-projects`).

**Remaining follow-up:** R3 default-on timeline (decision #4) after live
validation; re-exec mechanism (decision #2) only if PID-preservation is later
wanted.
