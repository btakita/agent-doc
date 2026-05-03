# Plan — JetBrains tab selection sync must not replace live agent-doc panes

**Pending:** `#jbselsync`
**Status:** Draft

## Problem

Switching editor focus to `src/boost-client/tasks/monsterrodholders.md` can make `agent-doc` "lose" the currently running tmux pane, but the persisted evidence shows repeated registry rebinds rather than a true pane crash.

Concrete evidence:

- `src/boost-client/.agent-doc/logs/a9421282-fd96-4943-9af5-3561ed5cb799.log` records repeated `session_superseded` and `session_end origin=registry_rebind` events for `tasks/monsterrodholders.md` (`%529 -> %533 -> %536 -> %540 -> ...`) instead of child crash provenance.
- Later in the same log, sync-driven recovery records `supervisor_exit code=missing_pane ... origin=sync_missing_pane` for panes such as `%71`, then immediately starts newer panes `%72` and `%73`.
- The JetBrains tab-selection listener currently runs `agent-doc sync` on every markdown selection change, with `noAutostart = false`, from the focused file's nearest root.
- The editor plugin spec requires a non-destructive split:
  - selection change should prefer `agent-doc focus <file>`
  - visible layout changes should use `agent-doc sync --no-autostart ...`
  - cross-root layouts should run sync from the workspace root, not the focused submodule root

So the likely failure is editor-driven pane replacement churn: tab selection is still allowed to provision/rebind panes in the exact path that should only refocus or perform report-only layout reconciliation.

## Desired Behavior

1. Selecting `monsterrodholders.md` must not auto-start or replace its tmux pane when a live owner already exists.
2. Automatic tab-selection handling must distinguish focus-only changes from visible-layout changes.
3. Cross-root splits that include both workspace-root and submodule documents must reuse the workspace-root sync scope consistently across manual and automatic sync paths.
4. If ownership is ambiguous, the editor path must fail closed or surface diagnostics instead of provisioning a replacement pane.

## Implementation Steps

- [ ] Change `EditorTabSyncListener.selectionChanged` to follow the spec contract:
  - focus-only path: call `agent-doc focus <file>` or equivalent non-provisioning path
  - layout-change path: call `agent-doc sync --no-autostart ...`
- [ ] Reuse `SyncLayoutAction.chooseSyncProjectRoot(...)` in the selection listener so cross-root splits run from the workspace root.
- [ ] Add dedup state so repeated selection churn does not fire full sync when the visible markdown set is unchanged.
- [ ] Audit the selection listener's interaction with `LayoutChangeDetector` so the two paths do not double-trigger conflicting sync work.
- [ ] Add regression coverage for:
  - workspace-root doc + `src/boost-client/tasks/monsterrodholders.md` split, then focus switch to `monsterrodholders.md`
  - selection change with unchanged visible markdown set
  - automatic sync commands containing `--no-autostart`
  - cross-root selection sync choosing the workspace root instead of the focused submodule root

## Files Likely To Change

- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/SyncLayoutAction.kt`
- `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/SyncLayoutActionTest.kt`
- `src/agent-doc/editors/jetbrains/SPEC.md`
- `src/agent-doc/editors/SPEC.md`

## Exit Criteria

- Switching focus to `monsterrodholders.md` no longer triggers replacement-pane churn for an existing live session.
- Automatic JetBrains sync never provisions new panes on a pure tab-selection change.
- Cross-root split focus changes preserve both panes and keep ownership stable.
