# Plan — JetBrains restart loses visible tmux pane tracking and replaces the active session

**Pending:** `#jbptrk`
**Status:** Draft

## Problem

On May 2, 2026, restarting IntelliJ with the JetBrains plugin reopened `src/session-share/tasks/buildparty-investor-demo/dev.md` and `agent-doc` replaced the registered tmux pane even though the user reports the original panes were still visible.

Persisted evidence from `src/session-share/.agent-doc/logs/586ffeea-ca3a-490b-80bd-883dd0e95673.log` shows:

- `supervisor_exit code=missing_pane pane=%19 reason=registered_pane_missing last_known_window=@8`
- `session_end origin=sync_missing_pane`
- two seconds later, `session_start file=tasks/buildparty-investor-demo/dev.md pane=%25 session=586ffeea`

`src/session-share/.agent-doc/sessions.json` now binds that same document/session to `%25`.

So the failure is not just "startup should be less aggressive." The more serious bug is that pane ownership/tracking diverged from what the user could still see in tmux, and the startup automation treated that uncertain state as authoritative enough to replace the active session.

## Desired Behavior

1. A pane that is still visible on the tmux server must not be classified as `registered_pane_missing` from weak or partial evidence.
2. JetBrains startup/restore automation must not replace or supersede a document session while pane ownership is uncertain or a turn may still be active.
3. If tracking is ambiguous, `agent-doc` should either recover/rebind to the visible owner or fail closed with diagnostics, not start a fresh pane era.
4. Every automatic replacement decision must record enough provenance to explain why the old pane was treated as gone and why the new pane was allowed.

## Implementation Steps

- [ ] Reconstruct the exact May 2 restart sequence end-to-end:
  - plugin `projectOpened()` `resync --fix`
  - layout/tab restore `sync`
  - the `sync_missing_pane` decision for `%19`
  - the fresh `session_start` on `%25`
- [ ] Audit every place that can classify a registered pane as missing:
  - `sync.rs` `tmux.pane_alive(...)` and any wrapper/provenance helpers
  - `startup_miss.rs` session-end / replacement path
  - any session/window scoping assumptions that could make a still-visible pane look absent
- [ ] Verify whether "visible pane but missing to agent-doc" can happen because of:
  - stale tmux server/session targeting
  - pane-id reuse or parse mismatch
  - looking only in the wrong tmux session/window scope
  - startup ordering where the registry is checked before authoritative pane recovery finishes
- [ ] Strengthen ownership recovery before any missing-pane replacement:
  - reuse supervisor-PID recovery
  - reuse file/path/process-tree recovery
  - include broader associated-pane search by project root / session provenance when the exact pane id is not trusted
  - treat multiple plausible candidates as ambiguous and fail closed
- [ ] Add a startup-time no-replace guard:
  - if JetBrains restore triggers `sync`/`resync` while the existing owner is uncertain, do not auto-start a replacement pane
  - surface a durable diagnostic instead of mutating tmux state
- [ ] Improve provenance logging for this family:
  - log the tmux session/window scope used for the alive check
  - log whether the pane id existed anywhere on the server
  - log all candidate recovery panes considered before replacement
  - log whether the replacement was plugin-triggered startup/restore work
- [ ] Add regression coverage for:
  - registered pane id lookup failing while a visible/recoverable owner still exists
  - JetBrains startup calling `sync`/`resync` on an active document without replacing it
  - ambiguous multi-pane recovery failing closed instead of starting fresh
  - `session_end origin=sync_missing_pane` only after the system has exhausted stronger ownership proofs

## Files Likely To Change

- `src/agent-doc/src/sync.rs`
- `src/agent-doc/src/startup_miss.rs`
- `src/agent-doc/src/tmux.rs`
- `src/agent-doc/src/sessions.rs`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/LayoutChangeDetector.kt`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/specs/supervisor.md`

## Exit Criteria

- Restarting IntelliJ with the JetBrains plugin can no longer replace `tasks/buildparty-investor-demo/dev.md` when the original pane is still visible/recoverable
- `registered_pane_missing` is only emitted after stronger ownership recovery proves the pane is truly gone
- ambiguous startup-time ownership states fail closed with durable diagnostics instead of creating a fresh pane
- session logs clearly distinguish true pane disappearance from registry/tracking loss
