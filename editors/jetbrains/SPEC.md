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
- `WrapLayout` for multi-row button overflow when labels are long.
- Labels truncated to 80 characters with tooltip for full text.

### Tab Sync Listener

- Registered as `FileEditorManagerListener` in `plugin.xml`.
- Split orientation detected by walking the Swing component tree for `Splitter` nodes.
- Dedup cache (`lastFileSet` + `lastActiveFile`) prevents redundant CLI calls.

### Auto-Save Before Poll

- `PromptPoller` saves tracked files via `FileDocumentManager` before each poll cycle.
- 3-way merge via `git merge-file` when both disk and editor have changed.

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
| Claim | `Ctrl+Shift+Alt+C` |
| Sync Layout | `Ctrl+Shift+Alt+L` |
| Popup Menu | `Alt+Enter` |
| Prompt Select | `Alt+1..9` |
| Prompt Toggle | `Alt+Esc` |
| Prompt Dismiss | `Esc` |

## Context Menu

Run, Claim, and Sync Layout are available in:
- Tools menu
- Editor right-click context menu
- Project view right-click context menu (Run and Claim only)
