# Plan — JetBrains detached editor terminal ownership

Status: post-release implementation branch (`postrelease/jb-detached-window-surface`)

## API feasibility proof (IntelliJ Platform 2024.2 / build 242)

`Show Tab in New Window` creates a `DockWindow` containing a
`DockableEditorTabbedContainer`; it is not a second Project. The 242 platform
does provide a tool-window pane for that detached frame:

- `DockWindow.setupToolWindowPane()` constructs a `ToolWindowPane` whose
  `paneId` is the detached window's persisted dimension key, installs the
  editor dock container as its document component, and registers it with the
  project's `ToolWindowManagerImpl`.
- `ToolWindowManagerImpl.getToolWindowPanes$intellij_platform_ide_impl()`
  enumerates the main and detached panes. `ToolWindowPane` exposes both
  `paneId` and its owning `JFrame`, so an editor component's AWT window can be
  resolved to a stable IDE surface identity without using component identity
  as the identity.
- `ToolWindowManagerImpl.setSideToolAndAnchor$intellij_platform_ide_impl(...)`
  atomically changes a registered tool window's `toolWindowPaneId` and anchor.
  This is an internal 242 API, so the adapter must capability-probe it and fail
  closed when a later IDE removes it.
- `TerminalToolWindowManager.newTab(customToolWindow, widget)` and
  `detachWidgetAndRemoveContent(content)` are public Terminal-plugin APIs for
  re-hosting one terminal widget. Therefore agent-doc can move only its own
  terminal content instead of moving every unrelated tab in the stock Terminal
  tool window.

This proves the requested UI is feasible. The unsupported case is an IDE build
where the internal pane-placement method is absent; the boundary capability
probe logs that condition and leaves the existing terminal presentation
unchanged.

## Architecture contract

**Invariant:** For each project and agent document, at most one live editor
surface owns presentation of its tmux pane, and the agent-doc terminal is
mounted only in that owning surface's tool-window pane.

**Policy owner:** `SurfaceTerminalOwnership` in `agent-doc-editor-surface` owns
the exhaustive ownership transition. The Project Controller ProcessScope owns
its live instance. JetBrains observes IDE frames and applies the controller's
placement receipt; it never independently chooses an owner.

**Transition table:**

| Event/facts | Decision |
|---|---|
| Main surface reports focused document, no prior owner | `Mount(main, document)` |
| Document moves main → detached and detached is focused | atomically `Unmount(main)` + `Mount(detached, document)` |
| Old surface republishes after transfer | `Exclude(old)`; ownership remains detached |
| Two live surfaces show the same document | focused surface wins; otherwise retain the current owner; cold ties use `(generation, sequence, surface_id)` deterministically |
| Focus changes to another document in the same surface | transfer the single terminal presentation to the new document after its pane-focus receipt |
| Owning surface closes/rejoins another surface | `Return(fallback)` when another live surface shows the document, otherwise `Stash` |
| Remote Dev reconnect with a newer generation | retire prior-generation surfaces before accepting the replacement facts |
| Observation/receipt names stale generation or sequence | `Stale`; no UI or tmux effect |
| Surface exists but has no tool-window pane/Terminal capability | preserve controller ownership; boundary effect logs and does not mutate UI |
| Placement already matches | `AlreadyMounted`; no effect |

**Evidence inputs:** `client_id`, `generation`, `sequence`, stable `surface_id`,
per-surface `visible`, per-surface `focused`, an exact surface-retirement fact,
and the controller's exact tmux focus receipt (`document`, `pane_id`,
window/session). Terminal pane capability is read only at the UI effect boundary.

**Reactive topology:**

```text
JetBrains frame/editor/lifecycle events
  → per-surface EditorSurfaceObservation Source (controller ProcessScope)
  → visible/focused projections + SurfaceTerminalOwnership fold
  → terminal placement Computed
  → controller pane-focus Effect
  → tmux focus receipt Source
  → JetBrains mount/unmount Effect
  → capability-probed UI placement Effect + diagnostic
```

Reconnect is event-driven by socket lifecycle and plugin generation. Frame
creation/disposal is event-driven by editor/docking/AWT lifecycle. There is no
timer poller.

**Imperative extraction audit:** The project-wide `FileEditorManagerEx.windows`
fold, singleton `agent-doc` terminal lookup, and project-frame-only focus check
are derived values currently recomputed at unrelated call sites. This change
extracts them into per-surface snapshots and one ownership transition. The
remaining one-shot calls are (a) capture of a JetBrains event's current AWT
frame and (b) applying one already-derived tool-window relocation; both are
boundary reads/effects with no independent policy.

**Allowed edit surfaces:**

- `agent-doc-editor-surface`: typed surface identity and pure exhaustive owner transition.
- `agent-doc-controller-io`: ProcessScope retention, generation fencing, receipts.
- JetBrains `EditorTabSyncListener`, focus adapter, controller client, terminal
  host/coordinator, and focused tests.
- JetBrains/editor/tmux specs and deterministic SimWorld/policy tests.

No document-session markdown, release version, installer, published artifact,
or unrelated editor is changed on this post-release branch.

**Verification:** Pure transition tests cover atomic transfer/exclusion,
duplicate views, return/stash, stale generations/sequences, and same-surface
focus changes. Controller tests cover independent retained surface roots and
exact retirement. JetBrains deterministic tests cover receipt parsing, exact
forget payloads, and closed-surface endpoint selection; Gradle compilation
proves the 242 API boundary. Focused Rust tests plus the repository check are
the integration gate. A real `Show Tab in New Window` observation remains tagged
`[operator-verify]` because headless fixtures cannot display a DockWindow.

**Out of scope:** General multi-terminal UX, moving unrelated stock Terminal
tabs, persistence of a surface identity outside JetBrains' own DockWindow state,
and changing VS Code window behavior.

## Post-release integration and operator verification

1. Rebase `postrelease/jb-detached-window-surface` onto the first commit after
   the current release tag and rerun the focused Rust/Gradle gates below.
2. Merge only after that release is complete; build/install the JetBrains plugin
   through the normal post-release workflow (this branch performs no install).
3. `[operator-verify]` Open an agent-doc session and its IDE terminal, choose
   **Show Tab in New Window**, and verify `Agent Doc Terminal` appears in the
   detached frame and is absent from the main frame while unrelated Terminal
   tabs remain in place.
4. Focus the same session from each frame and then two different sessions;
   verify only the focused frame owns the agent terminal and its shell remains
   attached to the selected document's tmux pane.
5. Close the detached frame, then repeat across a Remote Dev reconnect. Verify
   the terminal returns to the remaining frame (or stays hidden if none shows
   the document) and a stale pre-reconnect frame never reclaims it.

Focused gates:

```text
cargo test -p agent-doc-editor-surface
cargo test -p agent-doc-controller-io controller_editor_surface_graph
cargo test -p agent-doc-editor-surface-io
cargo clippy -p agent-doc-editor-surface -p agent-doc-editor-surface-io -p agent-doc-controller-io --all-targets -- -D warnings
cd editors/jetbrains && ./gradlew test --tests com.github.btakita.agentdoc.JetBrainsEditorSurfaceTest --tests com.github.btakita.agentdoc.CpRouteClientCommandPlaneTest --tests com.github.btakita.agentdoc.EditorTabSyncListenerTest
```
