# JetBrains Plugin Specification

Extends `editors/SPEC.md` with JetBrains-specific behavior.

## Plugin Metadata

- **ID:** `com.github.btakita.agent-doc`
- **Name:** Agent Doc
- **Restart:** Not required (`require-restart="false"`)

## Implementation Details

### Claim — Split Position Detection

Two strategies for detecting the file's position in the editor split:

1. **Splitter tree walk:** Get the `EDITOR` component from action context, walk the Swing `Splitter` tree to determine if it's in the first child (left/top) or second child (right/bottom).
2. **Window index fallback:** If no EDITOR context (e.g., context menu), enumerate `FileEditorManagerEx.windows`, find which window contains the file, determine position from the Splitter tree or use window index + orientation as heuristic.

### Prompt Panel

- Rendered as a `JLayeredPane` overlay at `POPUP_LAYER` — no `JDialog` (avoids WM leaks and focus-loss dismissal).
- Uses IDE editor font via `EditorColorsManager`.
- Single-row, non-wrapping layout. The panel height is fixed to one prompt row and does not grow on narrow windows.
- Question and option labels truncate with ellipsis; full text is preserved in tooltips.
- Secondary detail (hotkeys, pending-count context) lives in tooltip/secondary UI instead of the main prompt row.

### Tab Sync Listener

- Registered as `FileEditorManagerListener` in `plugin.xml`.
- Split orientation detected by walking the Swing component tree for `Splitter` nodes.
- Single-document tab-selection changes call `agent-doc focus <file>` from the focused document's own project root.
- Split-layout tab-selection changes stay on `agent-doc sync --no-autostart ...` so a selected visible document can be rescued back out of stash into the `agent-doc` window without replacing a live owner. That passive path should hand off quickly by preferring the matching pane first, then an alive exclusive registered pane, and only cold-starting when neither exists.
- Visible markdown set changes call `agent-doc sync --no-autostart ...`, reusing the workspace-root chooser for cross-root layouts.
- Dedup state tracks the visible markdown signature plus the active file so repeated selection churn does not rerun the same command.

### Auto-Save Before Poll

- `PromptPoller` saves tracked files via `FileDocumentManager` before each poll cycle.
- 3-way merge via `git merge-file` when both disk and editor have changed.

### Run Feedback

- `Run Agent Doc` saves and dispatches immediately with `agent-doc route --dispatch-only`, without editor-side typing debounce or local "already running" inference.
- Repeating `Run Agent Doc` while an older plugin-spawned route process is still alive cancels the stale process and immediately starts a fresh dispatch for the same document.
- The action is silent on route progress/success. Failures are logged to the IDE Event Log / notification tool window instead of showing bottom-right balloon popups.
- A failed route persists the exact `agent-doc route` output under `.agent-doc/state/editor-route-errors/` and the notification exposes copy/open actions so startup-miss and pending-drift diagnostics remain inspectable after the toast moment.
- Route session targeting follows the same root-aware chooser as sync: a nested document reroute uses that file's nearest `.agent-doc` root, while a mixed-root visible layout stays pinned to the shared workspace root instead of the focused child repo.

### Safe Sync Surface

- JetBrains startup uses report-only `agent-doc resync`; it does not auto-run `resync --fix`.
- Editor-driven layout syncs report absolute file paths to `agent-doc sync`, preserve empty column placeholders for mixed markdown/non-markdown splits, and keep cross-root markdown siblings in the reported layout even when the focused file lives in a nested submodule.
- When the visible markdown layout spans multiple nested agent-doc roots, JetBrains runs `agent-doc sync` from the workspace root `.agent-doc/` instead of the focused file's nearest submodule root. This keeps shared column memory (`.agent-doc/last_layout.json`) stable when focus moves from a workspace session doc to an unmanaged spec/doc file inside a child repo.
- The Rust binary owns passive autostart, ambiguity handling, remembered-column restoration, and tmux window targeting.

### Action Promoter

- `AgentDocActionPromoter` ensures `AgentDocPopupAction` (Alt+Enter) takes priority over the built-in `ShowIntentionActions`.

### Logging

- Uses `com.intellij.openapi.diagnostic.Logger` (IntelliJ platform logger).
- Enable debug output: `Help > Diagnostic Tools > Debug Log Settings` → add `#com.github.btakita.agentdoc`.
- Output appears in `idea.log`. No temp files.

### Dynamic Lifecycle

- `PluginLifecycleListener` handles `projectOpened`/`projectClosing`.
- `disposeAll()` cleans up prompt panels and pollers on project close or plugin unload.

## Keybindings

| Action | Default Shortcut |
|--------|-----------------|
| Run | `Ctrl+Shift+Alt+A` |
| Fix Document | none |
| Claim | `Ctrl+Shift+Alt+C` |
| Sync Layout | `Ctrl+Shift+Alt+L` |
| Popup Menu | `Alt+Enter` |
| Prompt Select | `Alt+1..9` |
| Prompt Toggle | `Alt+Esc` |
| Prompt Dismiss | `Esc` |

## Context Menu

Run, Fix Document, Claim, and Sync Layout are available in:
- Tools menu
- Editor right-click context menu
- Project view right-click context menu (Run, Fix Document, and Claim)
