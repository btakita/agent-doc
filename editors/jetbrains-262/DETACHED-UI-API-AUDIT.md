# Build 262 detached editor UI API audit

## Decision

The public IntelliJ Platform API cannot place a terminal in one exact detached
`EditorWindow`. Per operator direction, this distribution implements the feature
through a deliberately isolated internal adapter and is clamped to
`since-build="262" until-build="262.*"`. This is a maintenance-risk exception,
not a claim that the APIs are supported beyond 262.

The frontend advertises `EXACT_262_INTERNAL` only after a runtime probe verifies
every class, constructor, and method it invokes. Any mismatch advertises
`SNAPSHOT_ONLY`; controller policy freezes and no editor or terminal is mutated.
There is no focus-based or shared-main fallback.

## Exact 262 implementation

`Exact262DetachedPresentationAdapter` is the sole internal-API boundary. It:

- targets `FileEditorManagerEx.openFile(VirtualFile, EditorWindow,
  FileEditorOpenOptions)` and `closeFile(VirtualFile, EditorWindow)`;
- resolves the one tagged split containing the projected logical document;
  missing or ambiguous matches are refused;
- creates a frontend terminal using `TerminalToolWindowTabsManager`, with
  `shouldAddToToolWindow(false)`, wraps its `TerminalView` in a
  `TerminalViewVirtualFile`, and opens it only in that exact split;
- attaches with `tmux attach-session -t <view_session>`, where `view_session`
  comes only from the controller's verified durable `Bound` identity;
- installs a non-writable placeholder virtual file for a main-owned or
  other-detached-owned duplicate; and
- restores the original file and closes the terminal on empty projection,
  close, rejoin, generation replacement, or disposal.

The probe covers `FileEditorManagerEx.getInstanceEx/openFile/closeFile`, the
`FileEditorOpenOptions` constructor/mutator shape,
`TerminalToolWindowTabsManager.getInstance/createTabBuilder/closeTab`, every
builder method used, `TerminalToolWindowTab.getView`, and the compatible
two-argument `TerminalViewVirtualFile` constructor.

## Typed and fenced flow

The frontend sends one complete, frame-tagged snapshot. The backend authenticates
the thin-client identity and fences lease, connection generation, snapshot
sequence, surface generation, and project before the controller atomically folds
the snapshot. The controller responds with an explicit presentation list (never
a map-shaped JSON key), including presentation revision and the durable Bound
session. Applied/refused/stale receipts repeat the complete identity and revision;
the backend rejects stale receipts before the controller performs its logging-only
receipt acknowledgement.

`BindPending` and `ReleasePending` project a noninteractive placeholder and a
retry hint. The frontend performs bounded, receipt-driven recapture (60 attempts,
250 ms); it does not poll ambient focus or route through legacy
`editor_surface_observe`. AWT focus selects which frame is active but never erases
the selected document of another surface.

## API evidence and risk

The internal types are present in IU `262.8665.258`:

- `com.intellij.openapi.fileEditor.impl.EditorWindow`
- `com.intellij.openapi.fileEditor.ex.FileEditorManagerEx`
- `com.intellij.openapi.fileEditor.impl.FileEditorOpenOptions`
- `com.intellij.terminal.frontend.toolwindow.TerminalToolWindowTabsManager`
- `com.intellij.terminal.frontend.toolwindow.TerminalToolWindowTabBuilder`
- `com.intellij.terminal.frontend.editor.TerminalViewVirtualFile`

JetBrains advises plugins not to use implementation classes or
`@ApiStatus.Internal` APIs:

- <https://plugins.jetbrains.com/docs/intellij/explore-api.html#refrain-from-using-internal-classes>
- <https://plugins.jetbrains.com/docs/intellij/api-internal.html>
- <https://plugins.jetbrains.com/docs/intellij/verifying-plugin-compatibility.html#plugin-verifier>

Consequently every 262 update must rerun Plugin Verifier and real Remote Dev
two-process tests. Static/unit/split-sandbox tests prove packaging and fences,
but they do not prove actual JetBrains Client detached-frame behavior. Issue
#218 must remain open until a real backend + Client run demonstrates main-wins,
two clients, detach/rejoin, reconnect, and capability-loss scenarios.
