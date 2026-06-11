# Plan: SimWorld editor + tmux integration harness (#swint)

Operator request (2026-06-11): extend the SimWorld harness to emulate **real editor
integration** — JB plugin and VS Code plugin — plus tmux, so the *entire integrated
system* (editor buffer ⇄ FFI/IPC ⇄ controller ⇄ document model ⇄ tmux dispatch) can be
tested deterministically, including multi-editor + document-model sync scenarios.

This is the deterministic-coverage answer to the recurring File-Cache-Conflict /
IPC-drift / queue-flood classes: today those only reproduce in a live IDE, which is why
their backlog items are all "operator-drive live verify." A SimWorld editor emulator turns
each into a regression test.

## Why now

The realtime-model rungs (#rtwatch: rungs 1/2/3a/3b landed) and the stuck-handoff reaper +
drain-owner gate (#kqr6/#sjwm/#kp5z, commit 23a3ce5b) all ship with unit/truth-table
coverage but their *integration* surface — the editor buffer racing the agent's writes —
has no deterministic harness. #rtwbcast (multi-editor CRDT broadcast) cannot be verified at
all without two emulated editors. This plan unblocks live-verify-only items.

## Slices

### Slice 1 — `SimEditor` buffer actor (foundation)
A deterministic in-harness actor that speaks the same IPC/FFI buffer protocol the JB/VSCode
plugins speak: holds a buffer (content, dirty flag, monotonic generation), can apply an
"unsaved edit," "save," "close," and answers the debounced buffer-state query that
`realtime_model::reconcile_current_doc` consumes (rung 2 `#rtwfeed`). One `SimEditor`
exercises the editor-buffer-vs-disk recency reconcile without a live IDE.
- Acceptance: a SimWorld scenario types an unsaved buffer edit, runs a finalize cycle, and
  asserts ops.log `realtime_doc_resolve authority=editor_buffer` + the edit survives commit
  (the exact #rtwverify proof, now deterministic).

### Slice 2 — JB + VSCode protocol parity fixtures
Two `SimEditor` configurations (`SimEditor::jetbrains`, `SimEditor::vscode`) that model the
protocol differences that actually bite: JB's WatchService read-only file watch vs VSCode's
buffer events, save-timing, and cache-conflict semantics. Drives the File-Cache-Conflict
classes (#w42v JB Compact Exchange) deterministically.

### Slice 3 — multi-editor sync (#rtwbcast harness)
Two `SimEditor`s open on the same doc. Scenario: edit buffer A → assert the realtime model
merges + broadcasts the CRDT change to buffer B with zero File-Cache-Conflict. This is the
*only* way to test #rtwbcast without two live IDEs.

### Slice 4 — tmux + integrated system
A `SimTmux` (or extend the existing tmux test seam) so dispatch/route + queue-drain +
drain-owner lease (#kp5z) can be driven end-to-end: editor edit → queue trigger → route to
pane → controller drain → document model update → broadcast back to editors. Asserts the
drain-owner gate (`SkipSelfDrivingLoopOwner`) and the stuck-handoff reaper under emulated
multi-owner contention — the integrated-system regression suite.

## Pointers (fill in during a clean-session implementation turn)
- Editor/IPC seam: `ipc_socket.rs`, `ffi.rs`, `realtime_model::reconcile_current_doc`.
- Reaper/drain: `terminate_stale_preparing_controllers`, `idle_queue_drain_decision`,
  `drain_owner` lease.
- Existing SimWorld harness module(s) — locate with `tsift --envelope search 'SimWorld' --budget normal`.

## Constraint
Implement from a CLEAN session (cwd `src/agent-doc`), NOT via `/agent-doc` on this doc —
same constraint as plan-stuck-handoff-reaper.md: this doc's controller is part of the system
under test and races every write.
