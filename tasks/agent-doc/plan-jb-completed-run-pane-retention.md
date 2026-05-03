# Plan — JetBrains completed run should not remove the tmux pane from the `agent-doc` window

**Pending:** `#jbpdrop`
**Status:** Draft

## Problem

User report from May 2, 2026:

- the run completed
- immediately after completion, the tmux pane was removed from the `agent-doc` tmux window

That is a distinct failure from `#jbptrk`.

- `#jbptrk` is about startup/restart losing track of a still-visible pane and replacing it
- this new bug is about a *successful or apparently successful* run mutating tmux layout by removing the pane from the `agent-doc` window

The invariant should be stricter here: completing a run must not make the pane disappear from the `agent-doc` window unless agent-doc is performing one of the very few explicitly destructive pane-management operations such as duplicate-pane reaping.

## Desired Behavior

1. A normal completed run leaves its pane present in the `agent-doc` window.
2. If the harness process exits, tmux state must still remain diagnosable and non-destructive:
   - either the pane stays visible for recovery/inspection
   - or agent-doc records and explains the exact deliberate cleanup path
3. Editor/plugin automation must not treat a completed run as permission to reap, relocate, or supersede that pane.
4. Pane-closing behavior must be limited to a very small, explicitly-audited set of operations.

## Likely Root-Cause Surface

- `sync.rs`
  - `capture_dead_pane_diagnostics(...)`
  - `repair_missing_registered_pane(...)`
  - any path that records `registered_pane_dead` / `registered_pane_missing`
  - any post-completion cleanup path that calls `kill_pane` or recreates the `agent-doc` window
- `route.rs`
  - fresh-restart / `respawn-pane -k` paths that may reap the current pane after a completed cycle
  - stash rescue / join-pane paths that may leave the window without the user-visible pane
- JetBrains plugin automation
  - `PluginLifecycleListener`
  - `EditorTabSyncListener`
  - `LayoutChangeDetector`
  - any post-run focus/layout event that triggers `sync --no-autostart` and mutates tmux after completion
- tmux harness policy
  - whether the pane is expected to survive process exit
  - whether the agent-doc window relies on tmux defaults that remove panes/windows on exit instead of retaining them for recovery

## Implementation Steps

- [ ] Reproduce the exact post-completion disappearance with tmux/log evidence:
  - pane id before run
  - window membership before run
  - session log events during completion
  - tmux pane/window state immediately after completion
- [ ] Audit every code path that can remove or effectively remove the pane after a run ends:
  - explicit `kill-pane`
  - `respawn-pane -k`
  - `kill-window`
  - `join-pane` / rescue paths that relocate the pane out of the `agent-doc` window
  - any session rebind that leaves the original pane untracked and then cleaned up
- [ ] Decide and document the completion invariant:
  - whether completed panes must remain visible in-place
  - whether dead panes should be retained for diagnosis until explicit cleanup
  - which exact operations are allowed to close panes
- [ ] Harden sync/route against post-success cleanup:
  - do not classify a just-completed pane as disposable from weak evidence
  - do not let plugin-driven `sync`/layout events reap the only pane for a document just because the run ended
  - if ownership is ambiguous, fail closed and log instead of mutating tmux
- [ ] Narrow destructive tmux actions behind explicit guards:
  - duplicate-pane reap only
  - proven dead-pane cleanup only after stronger evidence
  - no generic "completion implies cleanup" behavior
- [ ] Improve diagnostics:
  - record the reason whenever a pane leaves the `agent-doc` window
  - log source path: route, sync, resync, plugin startup, plugin layout sync, duplicate reap, or manual recovery
  - capture before/after window membership for the affected pane
- [ ] Add regression coverage for:
  - successful completed run keeps pane in `agent-doc` window
  - plugin-triggered post-run sync does not remove the pane
  - a completed pane is not reaped unless it matches an allowed destructive path

## Files Likely To Change

- `src/agent-doc/src/sync.rs`
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/sessions.rs`
- `src/agent-doc/src/resync.rs`
- `src/agent-doc/specs/supervisor.md`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/editors/jetbrains/SPEC.md`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/LayoutChangeDetector.kt`

## Exit Criteria

- completing a JetBrains-triggered run no longer removes the pane from the `agent-doc` window
- any pane removal that still occurs is limited to a short explicit allowlist of destructive operations
- logs clearly state why a pane left the window and which subsystem initiated it
- isolated tmux regression coverage proves post-completion pane retention across the route/sync/plugin boundary
