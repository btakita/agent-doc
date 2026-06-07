# Plan: JB `Run Agent Doc` should preempt the autoloop (priority head-insert), not tail-append

**Bug id:** `#jb-run-preempt-autoloop-priority`

## Symptom (monsterrodholders.md, 2026-06-06)

Operator reports: autoloop may have been spinning on an **empty queue**. Typing into the
queue did nothing visible. `Run Agent Doc` produced no observable effect for ~60s. Expected:
`Run Agent Doc` interrupts the autoloop, runs the document now, then the autoloop resumes.

## Current behavior (grounded)

- Busy-pane `Run Agent Doc` → `route.rs` `AuthoritativeActorDispatchAction::DispatchOnlyBusyQueue`:
  **appends** the prompt to `agent:queue auto` (tail), sets `queue_active: true`, returns `Ok`.
  Drain is delegated to the supervisor **idle-queue watch** thread on a busy→idle transition
  (`specs/supervisor.md` §Idle-queue watch, `#jb-run-agent-doc-busy-queue-dispatch-deadlock`).
- The literal "pause autoloop / unpause autoloop" does not map to Claude's autoloop: `/loop`
  self-paces **inside the Claude session**; there is no external pause signal. Codex drains via
  the `Stop` hook (event-driven, no continuous loop). The cross-harness serializer is the pane
  itself + the idle-queue watch — not a controllable daemon.
- `busy_dispatch_only_should_wait_for_ready` waits up to `--wait-for-ready 60` **only when
  there is no queue fallback** and no live active-turn cue. A stale/transient Busy projection
  with no queue fallback is the path that yields the silent ~60s stall.

## Gaps to close

1. **Priority preempt:** busy-pane `Run Agent Doc` should insert at the **head** of
   `agent:queue` (or a dedicated priority lane), so a manual run jumps ahead of pending auto
   items instead of landing behind them. This is the real "preempt" the operator wants.
2. **Empty-queue autoloop suppression:** if the autoloop is active on an empty queue it keeps
   the pane looking Busy and starves the manual run. Confirm `/loop` terminates on drain;
   align with `#post-clear-auto-queue-suppress` / `plan-backlog-queue-sync-no-populate.md`.
3. **60s stall audit:** confirm the enqueue-immediately path (`has_queue_fallback == true`)
   actually fired for this scenario; if a queue fallback existed the route should not have
   blocked 60s. Add SimWorld coverage for "Busy-with-queue-fallback enqueues without waiting".
4. **Recursion-guard vs. real deadlock (operator addendum, 2026-06-06):** after the 60s the
   operator saw `Error: recursive direct invocation would deadlock`, and observed the autoloop
   was *already* stuck — so the guard's premise ("this WOULD deadlock") is moot when the loop
   is already wedged; it just blocks the one `Run Agent Doc` that could break the stall. The
   guard refuses a *direct* re-invocation from inside an in-progress turn (correct in the
   normal case), but a wedged-loop state needs an escape hatch: detect a stale/wedged active
   turn (no live spinner cue past a budget) and let the manual run reclaim the pane instead of
   refusing. Tie this to the head-insert + idle-watch drain so the reclaim has somewhere to go.

## Verification

- Deterministic SimWorld: busy active-turn pane + `Run Agent Doc` → head-insert, no tail-append,
  idle-watch drains the manual head first.
- Live: real busy Claude pane spinning autoloop → `Run Agent Doc` runs the manual prompt next.
