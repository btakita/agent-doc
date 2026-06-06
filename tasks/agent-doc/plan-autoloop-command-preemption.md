# Plan: #autoloop-command-preemption

## Request

> I was unable to run JB `Clear Session Context` on monsterrodholders.md while it
> was autolooping. If the queue is empty, agent-doc should pause/stop the autoloop
> if a command is sent to it. The loop should then resume afterwards.

Operator commands (Clear Session Context / `interrupt-clear`, and similar
pane-targeting actions) must be able to **preempt** an active `agent:queue auto`
loop: pause the loop, run the command, then resume the loop — instead of being
hard-blocked while the pane is busy.

## Current behavior

- JB `Clear Session Context` → `agent-doc interrupt-clear` (TerminalUtil.kt:694).
- When the pane is busy on an active turn, the busy guard blocks it:
  `session_clear_live_busy_guard_blocked` and the JB message "Clear Session
  Context is blocked … Wait for the turn to finish, then retry"
  (TerminalUtil.kt:1287/1311).
- With a self-driving `queue: start` + `agent:queue auto go` loop, the pane is
  busy ~continuously, so the operator can never get a quiet window to run Clear.
  Net: the command is effectively unrunnable on an auto-looping doc.

Related but distinct: `#clear-stops-running-process` (Clear fails to stop/repair
a busy projection). This item is about *preempting the auto-loop* for an operator
command, not the busy-projection repair.

## Status

- **Decision (2026-06-06):** non-interrupt `Clear Session Context` on a busy
  auto-looping doc **defers** (pause → finish current turn → run → resume); the
  destructive immediate path stays behind explicit "Interrupt and Clear".
  (Operator did not object to the recommended default across two re-invokes.)
- **Phase 1 SHIPPED** — pure decision + durable pause/resume transforms in
  `agent-doc-orchestration/src/queue_preemption.rs`
  (`plan_operator_queue_preemption`, `pause_queue_for_operator_command`,
  `resume_queue_after_operator_command`), 7 unit tests, non-destructive, no
  command-path wiring yet.
- **Phase 2a SHIPPED** — wired into the `session clear` command path
  (`session_actor_cmd.rs`). The `reconcile_idle_projection_before_clear` busy
  branch now consults `queue_continuation::detect` + the new pure
  `queue_preemption::plan_busy_clear`: when a non-interrupting clear hits a busy
  pane driven by an active `agent:queue auto` loop it **pauses the loop** via the
  durable clear cooldown the idle-queue watch already honors, logs
  `session_clear_queue_preempt_deferred`, prints a "deferred until the turn
  finishes" message, and returns a new `ClearPreflightOutcome::DeferredPreempt`
  so `clear()` short-circuits *before* delivering the clear into the in-flight
  turn (no destructive mid-turn `/clear`). A busy pane with **no** active loop
  keeps the old fail-closed `session_clear_live_busy_guard_blocked` hard block.
  2 new pure unit tests on `plan_busy_clear`; `make check` green; both binaries
  rebuilt + installed. Net effect: the operator gets the quiet window the paused
  cooldown opens (retry `session clear` at the idle prompt) instead of an
  unrunnable hard block.
- **Phase 2b SHIPPED** — full pause → deliver-on-idle → resume loop, plus the JB
  message. Durable hand-off marker `DeferredOperatorClear` in
  `queue_continuation.rs` (`write_/read_/clear_deferred_operator_clear`), written
  by the Phase 2a defer branch alongside the clear cooldown. New pure
  `queue_preemption::plan_deferred_clear_step(pending, prompt_ready)` →
  `None`/`WaitForIdle`/`Deliver`. The supervisor `idle-queue-watch` thread
  (`start.rs`) consults it each tick BEFORE the drain decision: on `Deliver` it
  injects the recorded clear command via `auto_trigger_inject_command`, then
  drops both the deferred-clear marker and the pause cooldown to resume; on
  `WaitForIdle` it waits (no interrupt of in-flight work); on `None` it is a
  complete no-op so the existing drain path is unchanged (zero regression risk
  to the common auto-loop). `interrupt-clear` drops the deferred marker first so
  the destructive path supersedes a pending deferred clear. JB plugin recognizes
  the `session_clear deferred for …` outcome (`isDeferredQueuePreemptClear`) and
  shows a "deferred, will run at the next idle gap" notice instead of
  "Cleared"/"blocked"; plugin built `0.2.148` (both zips). 6 new unit tests
  (`plan_busy_clear` ×2 from 2a, `plan_deferred_clear_step` ×3, marker roundtrip
  ×1); `make check` green; both binaries rebuilt + installed.
- **Phase 3 GATED** — operator live-verify on a busy auto-looping Codex pane:
  start an auto-loop, run Clear Session Context, confirm the loop pauses, the
  clear runs at the idle gap, and the loop resumes — without losing queue items.
  Note: the live supervisor watch thread only picks up the new logic after the
  running `agent-doc start` supervisor restarts (long-running process); install
  alone does not hot-swap it.

## Design

Pause → run → resume, coordinated through the supervisor + queue state:

1. **Pause:** when an operator command targets a doc whose `agent:queue` is
   active (`queue_continuation::detect` is Some), first set `queue: stop`
   (durable) so the auto-loop will not start the next iteration after the current
   turn ends. Record the prior queue state (start/auto) for resume.
2. **Run the command:** apply the operator command. For `interrupt-clear` this is
   destructive (it interrupts the live turn) and must stay behind the explicit
   "Interrupt and Clear" action — a non-interrupting Clear should instead *defer*
   until the current turn ends (do not silently kill in-flight work). Honor the
   state-scoped Codex interrupt-key rules (`#codex-interrupt-clear-ctrl-g-opens-editor`).
3. **Resume:** after the command completes and the pane returns to a
   dispatch-ready prompt, restore the recorded queue state (`queue: start`) so the
   loop resumes draining.

Open design questions:
- Interrupting vs deferring: "Clear Session Context" (non-interrupt) should
  probably *queue the clear* to run at the next inter-iteration idle gap (pause
  the loop, let the current turn finish, clear, resume). "Interrupt and Clear"
  stays the explicit destructive path.
- Where the pause/resume lives: supervisor (owns the auto-loop) vs the
  `interrupt-clear` command path. Likely the command sets durable `queue: stop`,
  and the supervisor's idle-queue watch resumes on restored `queue: start`.
- Concurrency: a paused loop must not be resumed by a stale watcher mid-command.

## Tests

- Unit: a doc with active auto-queue, operator command sets `queue: stop` +
  records prior state; resume restores `queue: start`.
- SimWorld: command preemption on a busy auto-looping pane defers/runs then
  resumes the loop without losing queue items.

## Live verification (gating)

Reproduce on a busy auto-looping Codex pane (monsterrodholders): run Clear
Session Context, confirm the loop pauses, the clear runs, and the loop resumes —
without losing queue items or silently killing in-flight work.

## Related

- `#clear-stops-running-process` — Clear failing to stop/repair a busy projection.
- `#monsterrod-pane-cross-doc-contamination` (shipped 25ff1116) — single-owner
  panes are a prerequisite for clean command targeting.
- `#codex-interrupt-clear-ctrl-g-opens-editor` — state-scoped interrupt keys.
