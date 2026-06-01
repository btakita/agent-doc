# Plan: JetBrains navigation and Sync Tmux Layout do not surface tsift pane

**Pending:** `#jb-tsift-pane-sync`
**Trigger:** `#agent-doc-bug` report from `tasks/software/tsift.md`
**Related:** `#autosync`, `#tmuxfocus`, `#stash-pane-promote-on-focus`, `#jb-load-tmux-window-verify`

## Problem

When JetBrains navigated to `tasks/software/tsift.md`, the corresponding
agent-doc tmux pane did not appear immediately. Running the manual JetBrains
`Sync Tmux Layout` action also did not bring that pane into the visible
`agent-doc` tmux window.

This is stronger than the older automatic-navigation latency class because the
explicit manual repair action failed as well. The expected operator model is:

- normal editor selection should make an existing pane visible or focused
  quickly
- if automatic selection cannot safely mutate layout, the manual sync action
  should either make the target pane visible or return a precise deferred /
  blocked reason
- a visible failure must name whether the pane was absent, stashed, protected by
  an open closeout, blocked by the local sync guard, or rejected by ownership
  proof

## Working Theory

The likely failure is at the boundary between JetBrains request scheduling and
the binary sync/focus handoff:

1. Automatic navigation issues an immediate `agent-doc focus <file>` plus a
   guarded `agent-doc sync --no-autostart --exact-visible ...`.
2. A stashed or hidden but live owner may require focus promotion or safe-passive
   attach behavior that the fast path does not complete.
3. Manual `Sync Tmux Layout` shares the same local FFI sync guard as background
   autosync. If a background sync is still running or retrying, the manual action
   can skip instead of becoming a foreground "latest request wins" repair.
4. The operator then sees no pane movement and may receive no actionable
   diagnostic distinguishing "deferred" from "not attempted".

There is also documentation/spec drift to settle: some editor docs describe
navigation as passive and non-autostart, while newer sync behavior allows safe
cold-start/reuse on the passive path. The live behavior and operator-facing
actions need one clear contract.

## Implementation Plan

### Phase 1 - capture the failing command path

- Add a live trace recipe for this exact repro:
  - selected editor file: `tasks/software/tsift.md`
  - visible markdown files and detected editor layout columns
  - immediate focus command, automatic sync command, and manual sync command
  - working directory chosen by JetBrains
  - sync guard state and generation
  - tmux window/pane state before and after each command
  - relevant `.agent-doc/logs/ops.log` sync/focus lines
- Make the JetBrains logs include whether manual sync skipped due to the local
  sync guard, completed with preserved-layout output, timed out, or applied focus.

### Phase 2 - make manual Sync Tmux Layout authoritative

- Treat manual `Sync Tmux Layout` as a foreground repair request:
  - it must supersede queued automatic sync requests for the same project
  - if another sync is running, either wait briefly and replay the manual request
    as the latest generation or surface "still syncing" with the target file
  - it must not silently drop the operator's explicit repair attempt
- Ensure manual sync always reports the post-condition for the focused file:
  focused/visible, deferred with protected pane details, missing owner, ownership
  conflict, or command failure.

### Phase 3 - close the focus/sync handoff gap

- Audit the `agent-doc focus` fast path for `tsift.md`:
  - if the live owner is in `stash`, promotion to `agent-doc` must be attempted
  - if registry points at a stale pane but a newer live owner exists, focus must
    select the newer owner or defer with proof
- Audit `sync --no-autostart --exact-visible` and full `sync`:
  - a hidden/stashed target pane must be attached when safe
  - preserve-layout exits must still reselect an already-visible target pane
  - exact-visible dedup must not mark a request applied unless the focused pane
    is actually active

### Phase 4 - regression coverage and specs

- Add JetBrains tests for:
  - manual sync while an automatic sync is already running
  - manual sync replaying the latest selected file after guard contention
  - preserved-layout output remaining deferred unless focus proof is present
- Add Rust sync/focus regressions for:
  - focused `tsift.md` pane hidden in stash
  - focused pane visible but not active after preserve-layout
  - manual full sync attaching a hidden target pane with protected unrelated panes
- Align `editors/SPEC.md`, `editors/jetbrains/SPEC.md`, and
  `specs/07-session-tmux-commands.md` on the final contract for navigation,
  Load Tmux Window, passive sync, and manual Sync Tmux Layout.

## Acceptance Criteria

- Navigating to `tasks/software/tsift.md` focuses or surfaces an existing live
  pane without requiring manual tmux navigation.
- Manual JetBrains `Sync Tmux Layout` either makes the `tsift.md` pane visible
  or reports one specific actionable blocker.
- A manual sync request cannot be lost behind a background autosync guard.
- Logs identify the exact command path and post-condition for the focused file.
