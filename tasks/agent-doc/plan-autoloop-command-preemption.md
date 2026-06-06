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
- **Phase 2b GATED** — auto-deliver the deferred clear from the supervisor
  idle-queue watch on the next idle gap (instead of the operator re-running it),
  then resume by restoring `queue: start`; and change the JB plugin message from
  "blocked" to "deferred until the turn finishes" (JB build + version bump).
  Touches the live supervisor watch thread + JB plugin. Honor
  `#codex-interrupt-clear-ctrl-g-opens-editor`.
- **Phase 3 GATED** — operator live-verify on a busy auto-looping Codex pane.

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
