# Plan: stuck-handoff reaper + queue-flood drain gate (#kqr6 / #sjwm / #kp5z)

Two independent control-plane race fixes that both require a clean session to land and
verify. Slice A stops a wedged controller from re-corrupting the document; Slice B stops
the supervisor from flooding the live harness with `agent-doc <FILE>` triggers.

**Run this in a CLEAN Claude Code session, NOT via `/agent-doc`.** Open a normal session
with cwd `src/agent-doc` (the submodule). Do NOT run it against
`tasks/agent-doc/agent-doc-bugs2.md` — that session's controller is the very process these
fixes terminate/gate, and it races every write (it re-corrupts the working tree mid-edit).

---

## Turn efficiency (tsift) — read first

This codebase is large; use tsift, not raw cat/grep, to stay turn-efficient. The session
running this plan should:

1. `cd src/agent-doc` first so the harness loads the submodule's narrower instructions and
   tsift indexes the right root. Run `tsift status`; if it prints a `run:` recommendation
   (stale/missing index), run `tsift status --fix` before relying on results.
2. Navigate with these, preferring them over cat/head/rg:
   - `tsift --envelope symbol-read '<symbol>' --file '<path>' --budget normal` — token-budgeted
     symbol body + AST span + child refs (use for the functions named in Pointers below).
   - `tsift --envelope search '<query>' --budget normal` — AST-aware search (over rg).
   - `tsift --envelope explain '<symbol>' --budget normal` and
     `tsift graph '<symbol>' --callers` / `--callees` — find call sites before editing.
   - `tsift --envelope source-read '<file>' --style window --start N --lines M` — windowed
     source when you need surrounding lines (over `sed -n`).
3. For noisy verification output use the digest commands, not raw scrollback:
   - `tsift --envelope digest-runner --kind test --path . --shell-command 'make check'`
   - `tsift diff-digest` / `tsift diff-digest --cached` instead of `git diff` / `git show`.
   - If a `search` envelope reports `report.scale_guard`, run one of its `narrow_commands`
     before fanning out.
4. The symbol/line pointers in this plan were captured via tsift and may have drifted a few
   lines — resolve them by symbol name with `symbol-read`, not by hard line number.

---

## Slice A — stale-`Preparing` controller reaper (#kqr6 / #sjwm / #stuckhandoff)

### Problem

A controller can wedge in `ControllerHandoffState::Preparing` and keep writing the stale
buffer to disk (multi-owner race on `ipc.sock` vs. the IDEA listener). Observed live: PID
rotates `gen 1002 → 1004 → 1006`, each respawn re-entering `preparing`; every `agent-doc`
write in the affected session re-drifts the working tree (`queue_head_removal_guard_fired`,
`live_prompt_drift`, post-commit worktree mangling).

### Root cause (verified in code)

- `agent-doc-orchestration/src/project_controller.rs` — `enum ControllerHandoffState`
  (`Stable, Preparing, Promoted, Retiring, Failed`).
- `handoff_started_at: Option<u64>` is **stored** on the controller record but is **never
  read for a staleness comparison**. The only reads just copy it forward at bootstrap. No
  `now - handoff_started_at > threshold` check exists anywhere.
- `close_stale_starting_actors_for_caller` reaps stale **`starting` actor projections**
  (gated by `supervisor_lease_is_fresh_or_alive`), and `gc.rs` calls it with
  `Duration::from_secs(3600)`. This (a) targets the wrong state (`Starting`, not
  `Preparing`), (b) closes a **projection record**, not a live process — a closed projection
  does not stop a wedged controller process from writing the socket, and (c) 1h is far too
  long for a handoff that should complete in seconds.

So a wedged `Preparing` controller has **no detector, no reaper, and no process
termination**. Add all three.

### Fix

1. **Reaper.** Mirror `close_stale_starting_actors_for_caller` with a new
   `terminate_stale_preparing_controllers_for_caller(project_root, stale_after, dry_run, caller)`:
   - Enumerate controller records carrying `handoff_state` + `handoff_started_at` + `pid` +
     `controller_generation`.
   - For each `handoff_state == Preparing` (consider `Promoted`-but-never-finalized too):
     `age = now.saturating_sub(handoff_started_at?)`; skip if `age <= stale_after`.
   - Skip if a fresh/alive supervisor lease proves the handoff is legitimately in flight
     (reuse `supervisor_lease_is_fresh_or_alive`) — do not kill a healthy mid-handoff.
   - Otherwise **terminate the process** (`pid`): SIGTERM, verify exit. This is the critical
     difference from the projection reaper — the live proc must die so it stops writing.
   - Transition the record to `Failed` (or supersede with the newer generation) so the next
     invoke promotes a clean controller. `dry_run` prints `would terminate stale preparing
     controller: pid=… generation=… age_secs=…` and does not kill.
   - **Threshold:** seconds, not 1h. Named const + env override (e.g. 30–60s).
2. **Observability.** `stale_preparing_controller_reaped pid=… generation=… age_secs=…
   caller=…` via `ops_log::log_op` on every kill; a `…_skipped reason=lease_fresh` /
   `reason=within_threshold` line for the non-kill branches. This is the live-verify proof.
3. **Wire into two paths:**
   - **gc sweep** — call alongside `close_stale_starting_actors` in `gc.rs`, short threshold.
   - **Self-heal on invoke (primary)** — call the reaper at the controller bind / handoff
     entry point so a wedged `Preparing` predecessor is killed **before** the new controller
     promotes. This breaks the `1002 → 1004 → 1006` respawn loop.
4. **Mitigation (defense in depth — answers "how does the binary mitigate stuck-handoff"):**
   - Predecessor self-terminates / transitions to `Failed` if its own handoff does not reach
     `Promoted`/`Stable` within the threshold (don't depend solely on an external reaper).
   - A controller still `Preparing` past the threshold **refuses to write** the session
     document (fail-closed) rather than racing the live owner.
   - Expose the reaper as an operator command (`agent-doc admin controller reap-stale` or
     extend `resync --fix`) to replace the manual `pkill`.

### Tests (mandatory, precommit)

- Unit: staleness predicate — `Preparing` + old `handoff_started_at` + no fresh lease ⇒
  reap; within threshold ⇒ keep; fresh-alive lease ⇒ keep; `Stable` ⇒ ignored. Model on the
  existing `close_stale_starting_actors` tests.
- SimWorld / integration: a `Preparing` record with old `handoff_started_at` + dead/stale
  lease ⇒ reaper terminates the pid (use a sentinel long-sleep child so termination is
  observable) and transitions to `Failed`; a subsequent invoke promotes a fresh controller.
  Regression guard for the respawn-into-`preparing` loop.

---

## Slice B — queue-flood dual-drain gate (#kp5z / #qflood)

### Problem

During queue-drain / finalize-while-typing, repeated `agent-doc <FILE>` triggers get
submitted into the running Claude Code session; the operator has to manually delete them
from the CC input queue. Cross-harness (repro'd on lazily-rs Codex too).

### Root cause (verified in code + spec)

- The injection path is the **supervisor** `idle_queue_watch_drain` (start.rs), logging
  `idle_queue_watch_drain payload_kind=trigger submit_mode=tmux_literal_cr reason=auto_trigger_inject`
  — **not** `project_controller::authorize_dispatch`. (That is why earlier
  `queue_dispatch_invoked` observability never logged — it was added to the wrong path.)
- The decision is `idle_queue_drain_decision` in
  `agent-doc-orchestration/src/start.rs` (function around line 410). Signature:
  ```
  fn idle_queue_drain_decision(
      clear_cooldown_active: bool,
      prompt_visible: bool,
      turn_active: bool,
      active_head: Option<&str>,
      last_dispatched: Option<&str>,
  ) -> IdleQueueDrainDecision
  ```
  variants: `SkipClearCooldown, SkipNoActiveHead, SkipNotIdle, SkipTurnActive,
  SkipAlreadyDispatched, Dispatch`.
- It already has the busy gate (`SkipNotIdle` / `SkipTurnActive`) and dedup
  (`SkipAlreadyDispatched`). The gap: when Claude `/loop` is **also** the active drain owner,
  there are **two** drain owners — `/loop` re-invokes `agent-doc <FILE>` from the harness AND
  the supervisor injects a trigger — so triggers pile up in the live CC input queue. The spec
  already states the intended ownership: `specs/07-session-tmux-commands.md` — "The supervisor
  idle-queue watch owns the later drain payload for that queued dispatch." The supervisor must
  **defer** when a self-driving harness loop owns the drain.

### Fix

1. **Pure-logic gate (unit-testable).** Add a `self_driving_loop_active: bool` parameter to
   `idle_queue_drain_decision` and a new `IdleQueueDrainDecision::SkipSelfDrivingLoopOwner`
   variant returned (early, before `Dispatch`) when it is set. This is the unit-testable
   core #kp5z calls out.
2. **Signal source (the harder half — wire the bool).** The supervisor needs to know `/loop`
   is the active drain owner. Implement a **drain-owner lease**: when the Claude Code
   auto-loop is driving (the agent-doc skill's `/loop` path), write/refresh a short-TTL
   marker (e.g. `.agent-doc/drain-owner/<pathhash>` with owner=`claude_loop`, heartbeat ts).
   `idle_queue_watch_drain` reads it; a fresh lease ⇒ `self_driving_loop_active = true` ⇒
   supervisor defers. Stale/absent lease ⇒ supervisor drains as today (so non-`/loop`
   harnesses, e.g. Codex Stop-hook loop, still get supervisor drive). Keep TTL short so a
   crashed loop hands ownership back to the supervisor quickly.
   - Tie-break must be single-owner at any instant: exactly one of {`/loop`, supervisor}
     drains. Prefer the harness loop when its lease is fresh; the supervisor is the fallback.
3. **Observability.** Log `idle_queue_drain_decision decision=SkipSelfDrivingLoopOwner
   owner=claude_loop lease_age=…` so the live verify can prove the supervisor deferred.

### Tests (mandatory, precommit)

- Unit: `idle_queue_drain_decision` truth table including the new param — fresh loop lease ⇒
  `SkipSelfDrivingLoopOwner`; no/stale lease + idle + fresh head ⇒ `Dispatch`; existing
  `SkipNotIdle`/`SkipTurnActive`/`SkipAlreadyDispatched` paths unchanged.
- SimWorld / integration: simulate `/loop` lease present → assert zero supervisor trigger
  injections during a drain; lease expired → supervisor resumes draining. Regression guard
  for the flood.

---

## Verify (in the clean session)

1. `cd src/agent-doc && tsift --envelope digest-runner --kind test --path . --shell-command 'make check'`
   — full suite green (do NOT waive reds as unrelated).
2. Slice A: reproduce a `Preparing` controller with old `handoff_started_at` (or a real
   wedged `--handoff-state preparing` proc), run the reaper path, confirm `ops.log` shows
   `stale_preparing_controller_reaped pid=… age_secs=…`, the pid is gone, and a fresh
   controller promotes to `Stable` with no respawn-into-`preparing`.
3. Slice B: drive a `/loop` queue drain, confirm `ops.log` shows
   `idle_queue_drain_decision decision=SkipSelfDrivingLoopOwner` and ZERO
   `idle_queue_watch_drain … reason=auto_trigger_inject` lines while the loop owns the drain;
   then expire the lease and confirm the supervisor resumes.
4. Ship to the live binary + plugin: `cargo install --path .` AND
   `cargo build --release && agent-doc lib-install` (cdylib hot-reload). The live
   `agent-doc-bugs2.md` session then stops re-drifting and stops flooding.
5. CI: `gh run list --workflow CI --limit 1`; fix any red.

## Pointers (resolve by symbol via tsift, not hard line numbers)

- `agent-doc-orchestration/src/project_controller.rs`
  - `enum ControllerHandoffState` (~`:470`); controller record / status structs with
    `handoff_started_at`, `pid`, generation (~`:110–144`).
  - `supervisor_lease_is_fresh_or_alive` (~`:1363`) — reuse.
  - `close_stale_starting_actors_for_caller` (~`:1378`) — template to mirror.
- `agent-doc-orchestration/src/gc.rs` (~`:180`) — sweep wiring site.
- `agent-doc-orchestration/src/start.rs`
  - `idle_queue_drain_decision` (~`:410`) + `IdleQueueDrainDecision` enum — Slice B gate.
  - `idle_queue_drain_payload` (~`:575`), `idle_queue_drain_payload_kind` (~`:590`),
    `idle_watch_active_queue_head` (~`:1926`) — drain payload/head context.
- `specs/07-session-tmux-commands.md` (~`:57`) — supervisor-owns-drain ownership statement
  to reconcile against the new gate.
- Live workaround until shipped:
  `pkill -f 'agent-doc controller serve.*--handoff-state preparing'` (respawns into
  `preparing` again — only the rebuilt binary holds).

## Related

`#kqr6` / `#sjwm` / `#stuckhandoff` / `#ipcdrift` / `#kp5z` / `#qflood` in
`tasks/agent-doc/agent-doc-bugs2.md`. Supersedes the "IPC section TBD" note in
`plan-queue-dispatch-flood.md` for the handoff-reaper + drain-gate slices.
