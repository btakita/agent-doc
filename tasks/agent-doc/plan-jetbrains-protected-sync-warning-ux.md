# Plan: JetBrains protected sync warning UX

**Pending:** `#jbsyncwarn`
**Status:** Draft

## Problem

When the user navigates JetBrains from one agent-doc session document to another while the first document still owns an open closeout cycle, `agent-doc sync` can correctly preserve the current tmux layout:

```text
[sync] sync preserved the current tmux layout because missing requested pane(s) ... while visible protected pane(s) ... cannot be detached safely because those panes still own open closeout cycle(s)
```

The guard is intentional: the sync layer must not detach or replace a visible pane that is still between preflight and closeout. The UX problem is that the JetBrains plugin can surface this as an error-looking popup during ordinary editor navigation, making a safe deferred state look like a failure.

This is adjacent to `tasks/agent-doc/plan-passive-sync-existing-pane-handoff.md`, but narrower: it focuses on how JetBrains classifies and presents protected-layout preservation, and on proving that normal navigation retries or hands off focus once the protected closeout completes.

## Desired Behavior

1. Protected-layout preservation remains fail-closed at the tmux layer.
2. Automatic tab sync treats this output as deferred, not applied and not a terminal error.
3. Manual sync shows a warning, but the copy should explain that sync was deferred because another pane is mid-closeout.
4. Automatic retries or a later generation must converge to the requested file after the protected closeout finishes.
5. The user-facing text must not imply data loss, a plugin crash, or a failed agent turn.

## Implementation Steps

- [ ] Add JetBrains tests that distinguish manual `Sync Tmux Layout` warning presentation from automatic tab-sync deferred retry handling.
- [ ] Ensure automatic tab sync never routes protected preserve-layout output through a persistent error notification surface.
- [ ] Normalize the preserve-layout warning text for JetBrains notifications into a concise deferred-sync message, while keeping full CLI output in logs.
- [ ] Add a regression for navigation from `agent-doc-bugs2.md` to `tasks/software/tsift.md` while the first pane is in `preflight_started`.
- [ ] Verify that once the protected pane reaches a committed closeout state, the retry path focuses or syncs `tsift.md` without requiring a manual action.
- [ ] Update editor plugin specs to define protected preserve-layout as a deferred sync state, not an error, for automatic navigation.

## Files Likely To Change

- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/SyncLayoutAction.kt`
- `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/EditorTabSyncListenerTest.kt`
- `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/SyncLayoutActionTest.kt`
- `src/agent-doc/editors/SPEC.md`
- `src/agent-doc/editors/jetbrains/SPEC.md`

## Verification

- JetBrains plugin tests for manual warning extraction and automatic deferred retry.
- Rust sync regression only if the repro shows the CLI skipped a visible focus handoff.
- Live JetBrains validation with:
  - `agent-doc-bugs2.md` mid-closeout and visible in tmux
  - editor navigation to `tasks/software/tsift.md`
  - no persistent error notification
  - automatic convergence after closeout

## Exit Criteria

- The plugin presents protected preserve-layout during navigation as deferred sync, not a hard error.
- The full diagnostic remains available in logs.
- The selected document eventually gains focus automatically after the open closeout releases the protected pane.
