# Plan: idle-watch injects the drain trigger during an active turn (`#idlewatch-turn-active-false-idle`)

## Symptom (operator report, 2026-07-12)

During an `equityfundingsource.md` turn, `agent-doc tasks/equityfundingsource.md`
is **periodically re-added to the pane input while the turn is still running**.
The supervisor idle-queue watch dispatches the file drain trigger into a busy
pane, repeatedly.

## Diagnosis (code-confirmed, incident-log pending)

The pure dispatch decision is correct — `idle_queue_drain_decision`
(`agent-doc-queue/src/queue.rs:132`) already returns `SkipTurnActive` when
`turn_active`, `SkipSelfDrivingLoopOwner` when a fresh `/loop` drain lease
exists, and `SkipAlreadyDispatched` when `last_dispatched == head`. So the defect
is an **input false-negative**: `turn_active` evaluates `false` while a turn is
genuinely running.

`turn_active` comes from `turn_active_for_owned_pane_with_idle_evidence`
(`agent-doc-start-runtime-io/src/lib.rs:331`), which reads the on-disk
turn-active marker (`.agent-doc/turn-active.json`, written by the
`agent-doc turn-status active` harness hook). It returns `false` in several
branches, any of which can misfire:

1. **Marker missing / stale.** `read_turn_active_marker_for_file` returns `None`
   when no marker exists or it is older than `TURN_ACTIVE_TTL_SECS = 3600`
   (`agent-doc-turn/src/turn_status.rs:21`). A harness whose `UserPromptSubmit`
   hook never fired `turn-status active` (or a turn running > 1h) reads as idle.
2. **Pane mismatch.** `marker.pane != owned_pane_id(shared)` → `Some(_) => false`
   (lib.rs:353). After a reroute or when the supervisor's owned-pane id differs
   from the `$TMUX_PANE` the hook recorded, a live turn reads as idle.
3. **Premature ready-prompt clear.** When `prompt_visible && actor_state_is_ready`
   the gate *clears* the marker and returns `false` (lib.rs:342-350). A harness
   whose composer momentarily looks ready mid-turn (e.g. Claude Code between tool
   calls) can clear the marker, after which every later tick reads idle and the
   watch re-injects.

The "periodic" (not one-shot) nature points at branch 2 or 3: after a false-idle
dispatch, `last_dispatched` is set, but the marker stays cleared/mismatched, and
the trigger fires again whenever the head text or a `last_dispatched` reset
(SkipNoActiveHead on a torn head read) re-opens the gate.

## Why not blind-fix now

Each false-idle branch exists deliberately (TTL self-heals a missed `Stop` hook;
pane-mismatch fails safe on reroute; ready-prompt is a legitimate idle proof).
Tightening any of them without the incident `ops.log` risks regressing the
supervisor idle-detection that many invariants depend on. The fix needs the
`ops.log` from a real incident to identify which branch fired.

## Next step (forensic)

From an `equityfundingsource.md` incident, capture:

```
tsift --envelope digest-runner --kind log --path . \
  --shell-command 'rg -n "idle_queue_watch_drain|turn_active|turn-active|owned_pane_ready_busy|read_turn_active" .agent-doc/logs/ops.log | tail -200'
```

Correlate each `idle_queue_watch_drain` dispatch with the concurrent
`turn_active=` value and the marker pane vs `owned_pane` to pin the branch, then:

- Branch 1 → have the running cycle **refresh** the marker on a heartbeat (not
  just write-once at turn start) so long turns stay busy; keep the TTL self-heal.
- Branch 2 → resolve the marker/owned-pane comparison through the same pane
  identity the router uses (canonical `%id`), or fall back to `true` on a
  proven-live owned pane when the marker pane is a known alias.
- Branch 3 → require the ready-prompt clear to also observe pane-idle evidence
  (busy-cue absent for N ticks) before clearing, so a mid-turn composer redraw
  cannot clear a live marker.

Add deterministic SimWorld coverage for the chosen branch (the fixture already
exercises `supervisor_idle_queue_tick` / `turn_active_for_owned_pane_with_idle_evidence`).

## Not a lazily-rs bug

This is supervisor / tmux turn-state detection in `agent-doc-start-runtime-io` +
`agent-doc-turn(-status-io)`. No lazily-rs / lazily-spec / lazily-formal change.
