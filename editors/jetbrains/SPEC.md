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
- Automatic tab sync may coalesce a short burst of selection events, but it must not suppress the first deliberate left/right split selection after a prior sync.
- During `selectionChanged`, JetBrains must treat the event's `newFile` as the authoritative active markdown target for dedup/snapshot planning instead of trusting `selectedTextEditor` to have already switched inside the same callback.
- If a newer automatic selection/layout request lands while another automatic sync is still running, JetBrains must queue only the latest request and replay it immediately after the running command finishes instead of dropping the event.
- If that older running sync finishes with a retryable preserved-layout or sync-lock-contention result after a newer request exists, JetBrains must not schedule a delayed retry for the older snapshot. It should let the older process finish silently and replay only the latest selected document.
- If manual `Sync Tmux Layout` output from an older `agent-doc` build reports that it preserved the current layout because a visible protected pane could not detach yet, JetBrains must show a concise deferred-sync warning visibly instead of only logging it; when the CLI output names protected pane details, the warning must include the blocked pane id, open-cycle phase, and document path so the user can tell which pane is delaying sync. Current `agent-doc` builds should attach/focus the requested document around the protected pane instead of emitting that deferred-sync marker.
- If passive `agent-doc sync --no-autostart ...` output from an older build reports that it preserved the current layout because a visible protected pane could not detach yet, JetBrains must treat that as deferred rather than complete for both the generic `[sync] sync preserved...` marker and the safe-passive `[sync] safe passive sync preserved...` marker: leave dedup state unchanged and schedule bounded retries until the requested selection applies or a newer request supersedes it.
- If passive `agent-doc sync --no-autostart ...` output contains `[sync] safe_passive_sync_lock_contention_retry`, JetBrains must treat the command as deferred, keep the dedup state unchanged, and retry the newest pending automatic selection/layout request rather than waiting for the CLI's full sync-lock budget. Manual `Sync Tmux Layout` shares the same editor-side sync guard as automatic sync; when a guard is already held, JetBrains shows a visible deferred warning instead of starting another sync process.

### Auto-Save Before Poll

- `PromptPoller` saves tracked files via `FileDocumentManager` before each poll cycle.
- 3-way merge via `git merge-file` when both disk and editor have changed.

### Run Feedback

- `Run Agent Doc` saves and dispatches immediately with `agent-doc route --dispatch-only`, without editor-side typing debounce or local "already running" inference. Even after `Clear Session Context`, this action still sends the bare `agent-doc <FILE>` reopen into the live session instead of restarting Codex.
- Repeating `Run Agent Doc` while an older plugin-spawned route process is still alive cancels the stale process and immediately starts a fresh dispatch for the same document.
- When Codex hook tracking is installed, `Run Agent Doc` must not report success from a live reroute that only proved tmux acceptance. If the bare reopen was accepted but Codex never records routed submission proof, the binary must fail once with that exact stage-specific reason and must not precede it with an optimistic success/progress line.
- The action is silent on route progress/success. Failures are logged to the IDE Event Log / notification tool window instead of showing bottom-right balloon popups.
- A failed route persists the exact `agent-doc route` output under `.agent-doc/state/editor-route-errors/` and the notification exposes copy/open actions so startup-miss and pending-drift diagnostics remain inspectable after the toast moment. A later successful `Run Agent Doc` for that document deletes the saved route-error file so the editor cannot keep showing an obsolete startup/proof failure after route recovery.
- Route session targeting follows the same root-aware chooser as sync: a nested document reroute uses that file's nearest `.agent-doc` root, while a mixed-root visible layout stays pinned to the shared workspace root instead of the focused child repo.

### Session Operator Actions

- `Show Session Status` runs `agent-doc session status <relative-path>` and surfaces the exact output in an IDE notification instead of re-deriving status inside the plugin. A successful status response deletes the saved route-error file for that document because the old failure is no longer the latest observed state.
- `Restart Supervisor Process` runs `agent-doc session restart-supervisor <relative-path>` and keeps restart ownership in the binary/supervisor path.
- `Clear Session Context` runs `agent-doc session clear <relative-path>` so Codex/Claude clear semantics stay aligned with the binary-owned clear command path while leaving the next `Run Agent Doc` dispatch-only reroute on the same live session.
- `Copy Session Diagnostics` runs `agent-doc session doctor <relative-path>`, copies the exact output, and keeps the binary-owned diagnostics text available for bug reports.
- Plugin verification must cover exact session-status display, `session clear` command wiring, and persistent route-failure retention for stage-specific dispatch diagnostics.

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

Run, Fix Document, Claim, Compact Exchange, Sync Layout, Show Session Status, Restart Supervisor Process, Clear Session Context, and Copy Session Diagnostics are available in:
- Tools menu
- Editor right-click context menu
- Project view right-click context menu (Run, Fix Document, Claim, and session operator actions)
