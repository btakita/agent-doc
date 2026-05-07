# Plan: Sync layout cardinality and focus handoff

## Goal

Make JetBrains `Sync Tmux Layout` and automatic tab-to-pane sync converge on exactly the editor-visible agent-doc pane set, while reliably focusing the selected document pane.

The concrete repro is a two-editor-pane layout where manual `Sync Tmux Layout` expanded the `agent-doc` tmux window to 3-4 panes, and automatic sync did not switch focus when the editor landed on `src/boost-client/tasks/monsterrodholders.md`.

## Working Theory

These symptoms share the same projection boundary:

- The plugin's visible editor snapshot is supposed to be the complete desired layout projection.
- `agent-doc sync --no-autostart` must not treat protected or unresolved panes as permission to grow the visible tmux window beyond that projection.
- A preserve-layout or blocked-layout result can still be a valid non-destructive outcome, but it must not be reported as focus-applied unless the selected pane is actually active.

Existing adjacent plans cover parts of this surface:

- `tasks/agent-doc/plan-jetbrains-cross-root-split-sync-validation.md` validates cross-root visible layout reporting.
- `tasks/agent-doc/plan-monsterrodholders-empty-column-sync-replay.md` validates empty-column replay for mixed markdown/non-markdown splits.
- `tasks/agent-doc/plan-passive-sync-existing-pane-handoff.md` targets preserve-layout focus handoff.

This plan ties those together around pane cardinality and plugin completion semantics.

## Steps

1. Reproduce the exact two-editor-pane setup:
   - one visible editor pane on `tasks/agent-doc/agent-doc-bugs2.md`
   - one visible editor pane on `src/boost-client/tasks/monsterrodholders.md`
   - capture the JetBrains command, working directory, `--col` list, `--focus`, and whether the action was manual `Sync Tmux Layout` or automatic tab sync
   - capture `tmux list-panes` before and after, plus `/tmp/agent-doc-sync.log`

2. Add a regression fixture for the plugin projection:
   - JetBrains layout detection must emit two columns for the two editor panes
   - cross-root paths must keep the workspace-root `.agent-doc/last_layout.json` scope
   - empty or non-markdown sibling columns must remain positional placeholders instead of being dropped before the Rust sync layer can apply column memory

3. Add a Rust sync regression for pane cardinality:
   - start with two visible registered panes in the `agent-doc` window
   - include one requested cross-root document and one focus target
   - assert manual sync and safe-passive sync do not leave more visible tmux panes than the requested/remembered column projection
   - assert extra live/protected panes stay in stash or their existing non-target window instead of being joined into `agent-doc`

4. Tighten the Rust sync implementation:
   - audit `run_with_options` before `tmux_router::sync_with_options`
   - make blocked/unresolved/protected panes contribute to an explicit "preserve current visible layout" outcome instead of widening the desired layout
   - ensure `build_layout_state` and `apply_column_memory` preserve column slots without duplicating remembered docs into multiple visible panes
   - log desired pane count, actual visible pane count, blocked files, protected panes, and selected focus pane in one structured sync line

5. Tighten plugin completion semantics:
   - automatic sync should only update `lastVisibleSignature` and `lastFocusedFile` after a result proves focus applied
   - preserve-layout output should keep the request pending unless it also reports `safe_passive_layout_preserved_reselected_focus`
   - manual `Sync Tmux Layout` should surface a warning when the post-sync pane count exceeds the editor-visible projection

6. Update specs:
   - `src/agent-doc/editors/SPEC.md`
   - `src/agent-doc/editors/PLUGIN-SPEC.md`
   - `src/agent-doc/specs/07-session-tmux-commands.md`

## Likely Files

- `src/agent-doc/src/sync.rs`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/SyncLayoutAction.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt`
- `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/EditorTabSyncListenerTest.kt`
- `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/SyncLayoutActionTest.kt`
- `src/agent-doc/specs/07-session-tmux-commands.md`
- `src/agent-doc/editors/SPEC.md`
- `src/agent-doc/editors/PLUGIN-SPEC.md`

## Verification

- Focused Rust tests for column memory, safe-passive preserve-layout, and cross-root sync.
- JetBrains plugin tests for two-column layout detection, active-file replay, and preserve-layout retry behavior.
- A live validation run in the reported JetBrains layout:
  - before manual sync: 2 editor panes and 2 intended tmux panes
  - after manual sync: no more than 2 visible `agent-doc` tmux panes
  - after landing on `monsterrodholders.md`: the monsterrodholders pane is active without requiring manual sync

## Exit Criteria

- `Sync Tmux Layout` cannot expand a two-editor-pane projection into 3-4 visible tmux panes.
- Automatic sync focuses `monsterrodholders.md` when its pane is already visible or retries when preserve-layout prevented focus.
- Logs make the outcome unambiguous: desired columns, visible pane count, preserved panes, and active focus pane are all recorded.
