# Plan — JetBrains passive sync should stay inside the document's own agent-doc root

**Pending:** `#jbsubroot`
**Status:** Draft

## Problem

JetBrains passive sync and manual sync were still deriving their execution context from the top-level IDE workspace instead of the active document's nearest `.agent-doc` root. For nested repos like `src/boost-client/` and `src/session-share/`, that let the plugin:

- send workspace-relative `--col` paths while running `agent-doc sync` from the wrong cwd
- resolve the first global `agent-doc` tmux window instead of a project-local live window
- disable passive autostart entirely even when no conflicting pane existed

That combination can bias sync toward the wrong tmux session/window and sets up cross-project misroutes like `monsterrodholders.md` preferring the `buildparty-investor-demo/dev.md` pane.

## Desired Behavior

1. JetBrains sync/claim actions must resolve the nearest `.agent-doc` root for the active document and run from that root.
2. Visible-file and editor-layout arguments passed to `agent-doc sync` must be relative to that resolved root, not the outer IDE workspace.
3. Passive sync may autostart when there is no conflicting live `agent-doc` window, but it must fail safe when the only available evidence is a foreign or ambiguous partial match.
4. A foreign `agent-doc` window must never be silently reused as this document's authoritative target.

## Validation Steps

- [ ] Replay the original JetBrains `Run Agent Doc` flow on `src/boost-client/tasks/monsterrodholders.md` while `src/session-share/tasks/buildparty-investor-demo/dev.md` is also open and confirm passive sync stays in the `boost-client` root.
- [ ] Confirm passive tab/layout sync now emits root-relative `--col` / `--focus` paths for nested repos instead of workspace-relative ones.
- [ ] Confirm the plugin only passes `--window` when a live tmux window is proven from the nested repo's own `.agent-doc/sessions.json`.
- [ ] Confirm the plugin allows passive autostart when no conflicting `agent-doc` window exists, and suppresses passive autostart with a warning when only foreign windows are visible.
- [ ] Verify claim + force-claim followed by sync can provision the pane for a nested repo without reusing a sibling repo window.

## Files Touched

- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/SyncLayoutAction.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/ClaimAction.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/ForceClaimAction.kt`

## Exit Criteria

- Nested-repo JetBrains sync/claim actions always run in the active document's own agent-doc root.
- Foreign `agent-doc` windows are treated as warnings/non-destructive partial matches, not as implicit authoritative targets.
- Passive autostart is restored for the non-conflicting case and covered by plugin tests plus a live replay.
