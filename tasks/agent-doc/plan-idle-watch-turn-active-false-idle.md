# Plan: idle-watch injects the drain trigger during an active turn (`#idlewatch-turn-active-false-idle`)

## Symptom (operator report, 2026-07-12)

During an `sampleportal.md` turn, `agent-doc tasks/sampleportal.md`
is **periodically re-added to the pane input while the turn is still running**.
The supervisor idle-queue watch dispatches the file drain trigger into a busy
pane, repeatedly.

## Diagnosis (confirmed and fixed, 2026-07-16)

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

## Resolution

The live incident confirmed branch 3: a ready-prompt redraw can be visible while
the owned turn marker still names the active pane. Ready text alone is not a
terminal lifecycle event, so it must not erase stronger, matching turn evidence.

`turn_active_for_owned_pane_with_idle_evidence` now keeps a fresh marker active
when it belongs to the owned pane, even if a ready prompt is visible. The marker
retires on an explicit Stop/idle lifecycle transition; its TTL remains the
bounded fallback for a missed terminal hook. Pane mismatch continues to fail
safe, so rerouting semantics are unchanged.

Regression coverage
`idle_queue_turn_active_gate_keeps_owned_marker_despite_ready_prompt_redraw`
models the exact failure: matching live marker + ready-prompt redraw must remain
turn-active and suppress idle-watch queue injection. Existing tests continue to
cover actual idle retirement, TTL expiry, and pane mismatch.

## Not a lazily-rs bug

This is supervisor / tmux turn-state detection in `agent-doc-start-runtime-io` +
`agent-doc-turn(-status-io)`. No lazily-rs / lazily-spec / lazily-formal change.
