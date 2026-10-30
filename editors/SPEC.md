# Editor Plugin Specification

Common behavior required of all `agent-doc` editor plugins.

## 1. Run (Submit)

- **Trigger:** `Ctrl+Shift+Alt+A` (configurable)
- **Behavior:** Save the active `.md` file, idempotently submit the nonblocking `agent-doc tmux start <FILE>` command, then send a Project Controller `editor_route` request for the relative path from the project root. This action must send the plain `agent-doc <FILE>` reopen into the owning live session; it must not restart Codex just because the latest tracked prompt was `/clear`. While the editor reports active typing for the document, the binary-owned supervisor must defer idle-queue `/clear` and `agent-doc <FILE>` continuation submits so it never races a half-edited queue head.
- **Feedback:** Show an immediate in-flight info notification while the Project Controller `editor_route` request is running, then finish with an inline hint near the cursor. Error notifications persist. An explicit send already delivered to the owning turn (`ui_outcome=steering_delivered_to_owner`) is a routine success: record it as proven, clear any saved route-failure diagnostic, and show no dialog, notification, or hint. Keep a dialog only when the action is destructive and this spec already requires that confirmation. If an editor persists exact route failures to disk, a later successful route for the same document must clear that saved diagnostic so obsolete startup/proof failures are not surfaced after recovery.
- **Availability:** Only enabled when a `.md` file is active.

## 1a. Typing / Live-Buffer Tracking

- **Behavior:** On every markdown document change, plugins must apply the incremental edit to their CRDT replica and report typing/sync epochs through the shared FFI. The Lazily/CP CRDT is the only live-text authority; plugins must not create a full-buffer filesystem sidecar.

## 1b. Start Agent Doc

- **Availability:** Command palette and the Agent Doc popup for an active `.md` file.
- **Behavior:** Save the active document and invoke `agent-doc tmux start <relative-path> --client-id <editor-client>`. The CLI resolves or provisions the correct pane, dispatches `agent-doc start` there, and returns immediately even when the editor extension host inherited `$TMUX`. An already-live supervisor is an idempotent success. Editors must not invoke blocking `agent-doc start` directly.

## 2. Claim for Tmux Pane

- **Trigger:** `Ctrl+Shift+Alt+C` (configurable)
- **Behavior:** Detect which editor split the file is in (left/right/top/bottom), call `agent-doc claim <relative-path> --position <pos>`. Falls back to no `--position` if split is not detected. If that target is already owned by another document, the binary provisions a distinct pane instead of replacing it. On a cross-session reject, both editors offer **New Pane in This Session**, which invokes `agent-doc claim <relative-path> --new-pane` without positional/force flags; the binary owns authoritative-session selection and pane allocation.
- **Feedback:** Inline hint near cursor. After a successful claim, trigger a layout sync (silent). A failed claim must leave the active editor document selected: do not run a layout sync after the failure, and do not let reverse tmux-to-editor focus mirroring reopen the previously focused tmux document as a side effect of the failed claim.

## 2a. Initialize Session

- **Availability:** Active-editor and project/explorer context menus for `.md` files.
- **Behavior:** Save the selected file and run `agent-doc init-session <relative-path>`. The binary initializes a plain file as a template session using a filename-derived title, preserves existing markdown in the exchange, and starts it. If the file already has a session ID, the binary claims it instead of scaffolding it again.

## 3. Sync Tmux Layout

- **Trigger:** `Ctrl+Shift+Alt+L` (configurable)
- **Start ordering:** Before its controller layout submission, manual Sync idempotently awaits `agent-doc tmux start` for the focused document. The standalone Start Agent Doc action, Run Agent Doc, and manual Sync all use this same nonblocking pane-start contract.
- **Behavior:** Collect all visible `.md` files, preserve one visible editor group per column (including empty split placeholders when the API exposes them), detect split orientation, and reconcile the existing tmux layout without replacing live or ambiguous owners. Manual sync submits `agent-doc.sync_tmux_layout.v1` to the Project Controller command plane and waits for its terminal receipt. The controller runs the full sync path so recoverable crash-left commit boundaries and window order are repaired to `0:agent-doc`, `1:stash`, and adjacent overflow `N:stash` windows. If a requested document has no pane, the same manual command must create and register one in the resolved target session, wait until its harness is dispatch-ready, and submit the document route before reporting success. It must not expand the visible `agent-doc` tmux window beyond the editor-authoritative snapshot when a protected open-cycle pane prevents safe detach; it should preserve the current cardinality and warn. Sync target-session resolution must prefer the current tmux session when it already has an `agent-doc` window, then fall back to the configured project `tmux_session`. Automatic editor sync paths should use the passive `sync --no-autostart ...` reconciliation path through the same Project Controller command-plane submit. A passive exact-visible pass must preserve the whole current layout when stale-supervisor admission removes any requested column; the recycle-settled generation retries the complete surface. Explicit focus-only moves submit `agent-doc.focus_document_pane.v1` to the Project Controller command plane. The `--no-autostart` contract is non-destructive, not "never start anything": on the fast happy path it should reuse the latest matching pane immediately, otherwise fall back to an alive exclusive registered pane, and only then cold-start a new pane when no owner remains.
- **Manual authority parity:** JetBrains and VS Code manual actions submit `caller_kind=manual` with a complete frontend snapshot. Only a positive, complete frontend column vector may replace structure. An `unknown`, partial, or selection-only capture preserves the last authoritative frontend vector or defers; it never becomes an operator assertion, fold basis, or inferred column. Controller tmux status is acknowledgement/presentation only, never desired-surface input (GH #338, GH #343).
- **Feedback:** Inline hint near cursor for applied sync. If a Project Controller-backed manual sync reports a preserved-layout outcome, show a concise deferred-sync warning instead of replacing it with a generic success hint. Pane creation, readiness, or route-submit failures must be surfaced from the terminal controller output in the IDE; the full controller receipt or automatic-sync CLI output should remain available in logs for diagnosis.
- **Acknowledgement:** For automatic layout sync, the plugin may trust the controller's terminal applied command receipt once the authoritative frontend snapshot generation is accepted; it need not wait for the later tmux effect receipt. Foreground routes instead wait for physical pane count/order and focused-pane convergence before dispatch. Repeating an identical foreground request is a fresh intent generation and physical observation edge, so focus or geometry drift cannot be acknowledged from stale `Converged` or terminal `OperatorOwned` state. This admission rule does not apply to document CRDT delivery, whose closeout barrier still requires the exact editor-visible content receipt.
- **Structural edge:** A controller-derived automatic `Sync` intent (first observation, changed columns, or observed tmux drift) must execute the tmux structural effect for that generation; it must not declare convergence from a structural pane assignment retained from an older editor generation. Pure focus changes remain separate focus effects, but their receipt is terminal only after the active pane in the target window matches the focused document's actor pane.
- **Controller-restart ownership:** A fresh controller may receive the next exact-visible editor snapshot before it has produced a structural effect receipt. A recovered live supervisor document→pane binding is controller-owned evidence for that initial survey, so the automatic snapshot may replace the prior controller-managed column instead of terminally classifying it as operator-owned. Without either a structural receipt or recovered supervisor binding, an unexpected live actor remains protected.

## 4. Tab-to-Pane Sync (Automatic)

- **Selection/layout coherence:** A selection event must not publish a surface while the selected-files view has advanced to the new document but the split-layout view still names the prior document. Adapters may reread that external editor snapshot on bounded later event-loop turns; after the budget, they may apply only the exact old→new event edge to each stale snapshot independently. Missing prior-file evidence remains fail-closed.
- **Restored-window coherence:** An adapter must not publish a contracted startup
  surface while the editor already exposes multiple restored windows but any
  window still has no selected file. File-open and structural edges retain the
  newest projection intent across bounded later event-loop turns.

Automatic tab-to-pane sync is **frontend-authored and backend-effected** (`#jbsurfaceswap` / `#jbpluginlazilyeffects`). A plugin publishes an exact ordered layout snapshot; the controller copies its column vector into a tmux effect target and observes convergence. Passive spanning-layout observation uses only an already-running Project Controller. Selection-only handoff may update focus intent but never structural membership or order.

- **Trigger:** Agent-doc session-document open/reopen, editor tab selection changes, visible-layout changes, and editor-specific activation (JetBrains focus/mouse activation, VS Code active-editor changes while clicking between already-open split editors).
- **Publisher liveness:** Every package topology an adapter ships must keep a live automatic publisher for each trigger above. A republish that only happens as a side effect of something else (a native-library reload during an install, a Run Agent Doc surface reference, a controller restart) is not that publisher: when it is the only one, a tab switch reaches tmux minutes later, at the next install or Run, and looks like an install-induced delay. JetBrains: a monolithic 262/263 IDE runs the modular frontend as a *local* lease that is never pane-layout authority (`local_client`), so its generated backend lifecycle must keep the classic tab-selection, split-focus, and IDE-activation edges; only a remote measured-layout lease may supersede them, and it does so by fencing the classic projection, not by uninstalling its edges.
- **Behavior:** Layout events publish the complete spanning editor surface through `agent_doc_editor_surface_observe_json(project_root, surface_json)` with `(client_id, generation, sequence)`. `surface_json` carries absolute paths and exact frontend-authored columns. Selection/component-focus events use a separate focus-intent lane and cannot promote, replace, insert, or reorder a structural column. Passive layout facts use the existing-controller socket without launching. Every accepted or rejected snapshot leaves a `controller_editor_surface_observed` receipt, and explicit retirement leaves `controller_editor_surface_forgotten`; tmux effect and survey receipts then make publication, application, and convergence independently diagnosable.
- **Open/reopen start ordering:** Opening an agent-doc session document is both a liveness edge and a surface edge. Before publishing that file-open surface, the adapter runs the binary-owned, nonblocking tmux start effect for the document and waits for its start receipt. Only then may it publish the surface. Publishing first is incorrect: the automatic layout effect can create and register an idle `layout-provision` pane, causing a later idempotent start to reuse the pane without starting agent-doc for the reopened document. This automatic edge does not create, select, or reveal an IDE terminal; the later surface effect still owns layout and focus. The binary owns target-pane resolution, startup serialization, and existing-owner idempotence; plugins must not infer process liveness or add a local duplicate-start cache. A non-session file-open remains a passive surface publication and starts nothing.
- **Desktop focus guard:** Automatic focus-only and layout-following effects may select a tmux pane only while the desktop editor is active (or when desktop focus cannot be observed). A delayed editor event received while the operator is working in a terminal or another application is retained as an observation but must not pull tmux focus into the right-hand agent pane. Explicit operator focus and manual layout commands remain authoritative.
- **What plugins must NOT do:** choose between `focus_document_pane` and `sync_tmux_layout`; hold a previous-observation / previous-signature / last-focused-file field; dedup repeat events; run a preserved-layout or lock-contention retry ladder; apply sync-timeout backoff; or report whether tmux has drifted. All of that is derived in `agent-doc-editor-surface` and driven by an `Effect` in the Project Controller, so the rule exists once rather than once per editor. An observation identical to the previous one is idle and costs nothing, which is why repeat events need no plugin-side guard.
- **Layout detection:** report the split layout the frontend detected, preserving column order and including empty split placeholders where the API exposes them. A frontend that could **not** detect a complete layout reports **no** columns rather than synthesizing a single column, so the controller preserves the last authoritative snapshot or defers. The layout is the split of the editor frame the operator is looking at (the frame of the focused editor window, else the main frame): editor windows of other frames, such as a detached editor window, are not columns of that split and must not be measured against its geometry (`#clearnoticeverify`: a detached scratch frame was published as an extra first column, turning a 2-split editor into 3 tmux panes).
- **Frontend layout derivation (GH #326, GH #343):** an adapter reports editor-window evidence and lets the shared native editor library derive columns before publication (`agent_doc_editor_derive_json`, derivation `window_layout` / `client_measured_layout`). The automatic surface derivation settles selection/readiness and produces the authoritative payload. The manual sync derivation captures the same complete frontend state and publishes the same exact vector. Remote Dev snapshot schema 3 sends `layoutColumns` directly from the JetBrains Client in left-to-right order; the backend may validate/filter session paths but must not group bounds, fold retained membership, reorder columns, or otherwise derive a second layout. Geometry-only schema 2 remains compatibility input, while incomplete evidence yields no structural update. VS Code reports `vscode.getEditorLayout`'s grid with its tab groups and visible editors to derivation `editor_group_layout` (`#vscodeupgradelayout`, § 13).
- **Drift:** whether tmux still matches the reported layout is the controller's observation, never the editor's. The controller fills that side of the mirror locally for multi-column surfaces; a plugin must not query it and report the answer back.
- **Layout + focus effect:** a frontend snapshot carries authoritative columns plus optional focused document. The controller first reconciles that exact vector, then applies focus through a generation-fenced `select-pane`. A focus-only update reuses the last accepted snapshot only when the document is already present; it never changes structure. Matching columns alone are not convergence when focus is required; the typed effect receipt must also prove that the final focus consequence landed. Identical targets retain their generation, rapid snapshots collapse to the newest generation, and stale work is cancelled at every safe structural-effect boundary.
- **Mixed-root layout:** one visible editor layout may contain documents
  owned by different project controllers. The parent layout effect retains
  the columns, validates the explicitly observed `agent-doc` window, and asks
  each nested controller only for its existing actor binding. Pane movement
  must not wait for editor-content authority, and the parent must not resume,
  provision, or durably register the foreign document.
- **Debounce:** 100ms, plus a generation guard, on an event loop or scheduler keyed by editor events — never hot polling, spinlocks, or thread-per-delay retry loops. This is the only coalescing a plugin owns: it exists so an event storm reports its final state, not so identical states are suppressed. Report off the UI thread; controller effects never run in the extension host.
- **Hot-reload continuity:** Native-library and dynamic plugin replacement are adapter-generation transitions, not editor-surface retirement. Quiesce the outgoing adapter's local listeners/workers, retain the controller's last acknowledged surface and focus Sources throughout the handoff, and publish the replacement generation over them after it is live. Only a real project close, editor shutdown, or plugin removal may forget those Sources. An aborted replacement must reinitialize the surviving generation without a null layout interval.
- **Project close and restart:** call `agent_doc_editor_surface_forget(project_root)` for every root the plugin observed. A fresh adapter process must also derive candidate controller roots from its open-document surface and retire non-authoritative retained roots before its replacement publish can block on reconciliation. Process-scoped clients request retirement for their own editor family; the controller stops every older-or-equal family generation at that root while preserving other editor families. The generation fence prevents a delayed callback from an older process from retiring or regaining authority over its replacement.
- **Feedback:** an accepted `sync` target may show the concise inline layout hint; `focus` and `idle` show nothing. The full receipt stays in the plugin log for diagnosis.
- **Manual `Sync Tmux Layout`** (§ 3) remains a direct controller submit and keeps its own preserved-layout warning; it is an explicit operator action, not an observation.

## 4a. Reactive Document Rename / Move

- A VFS rename or move is a retained old-path → new-path transition, not an implicit layout-sync request. The plugin first publishes a single reliable-sync liveness batch ordered as new-path `Open`, new-path `Register`, old-path `Close`; an enqueue or flush retry must replay the exact original OR-set tags.
- The existing Project Controller moves the live CRDT hub, aliases in-flight old-path requests, merges any raced old/new durable history, retires both hashes' ACK cursors, and rekeys the actor/session registry. Requests are effect attempts; the retained projection and convergence receipt are authority.
- The editor registers and seeds its new-path replica before retiring the old-path forwarder. Pane and window bindings move with the session registry entry. Rename handling must never invoke `sync_tmux_layout`, rebalance panes, or alter split orientation.

## 5. Prompt Polling Removed

- **First-party editor plugins:** No prompt-polling timer. JetBrains and VS Code must not poll `agent-doc prompt --all`, call `agent-doc prompt --answer`, auto-save tracked documents, refresh tracked files, or mutate editor buffers from a defensive prompt UI loop. Permission prompts stay in the owning agent/tmux surface.
- **Reactive prompt visibility:** The supervisor parses interactive harness output and projects
  `input_required` through the controller stream. Editor integrations may surface that state and
  focus the owning terminal, but must not duplicate the prompt's answer authority.

## 6. Popup Menu

- **Trigger:** a `.md` file is focused and the editor's popup shortcut is pressed — VS Code `Alt+Enter`; JetBrains `Ctrl+Shift+Alt+D` (also Tools menu and editor context menu → Agent Doc Actions). The default must be a keystroke that Windows, macOS, and Linux all deliver to the editor: never `Alt+Space` (Windows window system menu), `Alt+F4`, or `Alt+Tab`. A platform keymap may add a second keystroke its own window managers deliver: JetBrains' `Default for XWin` keymap (i3 and other bare X window managers) also binds `Alt+Space`, never the cross-platform default or the GNOME/KDE keymaps, which claim it. The shortcut stays user-rebindable through the editor's keymap settings.
- **Behavior:** Show numbered popup with Run, Claim, Compact Exchange, Sync Layout, Show Session Status, Recycle Supervisor, Restart Agent, Clear Session Context, Interrupt and Clear Session Context, Copy Session Diagnostics, and Dashboard actions. Every editor action that can be invoked on the document must be reachable from the popup (top level or overflow). Lower-frequency operator actions such as Run with Junie, Force Claim, and About Agent Doc stay available from a non-numbered overflow path instead of consuming top-level numeric shortcuts.

## 6b. About Agent Doc

- **Availability:** the Agent Doc popup's overflow (More Actions) path and the editor's command/menu surface (JetBrains Tools, editor, and project-view menus; VS Code command palette and markdown editor context menu). It needs an open project, not an agent-doc document.
- **Behavior:** show one dialog (modal message in VS Code) with the running editor plugin version, the agent-doc CLI binary actually in use, and the loaded native library, plus a one-click copy of the exact text.
  - **Plugin version:** the version the plugin registers with the controller (JetBrains plugin descriptor; VS Code `EDITOR_PLUGIN_VERSION`).
  - **CLI binary:** resolved the same way every other action resolves `agent-doc`, then queried with `agent-doc version --json` (`agent-doc-build-info-v1`: `version`, IPC `build_id` = `<version>+<source digest>`, `executable`, sibling `library`, and `expected_plugins.{jetbrains,vscode,zed}`). A binary without the `version` subcommand falls back to parsing `agent-doc --version`, and the build id is reported as unknown.
  - **Native library:** the generation the plugin already loaded: its canonical path, `agent_doc_version()`, and `agent_doc_build_info_json()` (same contract, `component = "native_library"`). A library without that symbol reports its build id as unknown. About never loads, reloads, or searches for a library; an unloaded library is shown as not loaded, which is not a mismatch.
- **Mismatch warnings:** the dialog uses a warning presentation and lists each mismatch with both sides named when (a) the CLI's `expected_plugins.<editor>` differs from the plugin version, (b) the loaded library expects a different plugin version than the plugin and the CLI disagree on, (c) the CLI and library build ids differ (the IPC handshake rejects that pair), or, when either build id is unknown, their versions differ, or (d) the CLI cannot be queried.
- **Zed:** the Zed extension is a language server with no editor action or menu surface; its server version is reported through LSP `serverInfo`, and `agent-doc version` covers the binary.

## 6a. Session Operator Actions

- **Show Session Status:** Run `agent-doc session status <relative-path>` and surface the full output in an IDE-owned diagnostics surface. A successful status command must clear any persisted route-error diagnostic for the same document.
- **Recycle Supervisor:** Run `agent-doc session restart-supervisor <relative-path>` (the legacy `session restart` alias remains valid) and show an inline success hint once the recycle request is accepted. If recycle is refused because the pane is busy or the authoritative actor is still starting, show an operator warning with `Interrupt and restart`, `Show status`, and `Copy details`; the confirmed interrupt path invokes `agent-doc session restart-supervisor --force <relative-path>`.
- **Clear Session Context:** Run `agent-doc session clear <relative-path>` so the authoritative session receives the harness-native clear command instead of the plugin pasting `/clear` directly into tmux. At an idle prompt the binary types that command into the same live harness process; it never restarts the agent for a plain Clear on a responsive pane, so the harness's background agents and shells keep running (`#idleclearrestart`). Restart Agent stays a separate action. The next Run action must still dispatch the bare reopen into that same session. Plugins must not block clear on plugin-local response-status or busy flags. The binary owns live pane evidence for status and diagnostics: stale busy projection is reconciled when `agent-doc session status` / `session clear` observes direct `alive-idle` evidence with `prompt_ready=true`, but direct `alive-busy` and `pane_current_command=agent-doc` alone are not Clear Session Context blockers because clear is an explicit operator action. If a non-interrupting clear meets a busy active auto-loop, the binary queues exactly one deferred clear for the next proven idle boundary and repeated clear clicks/commands report the existing queued clear without injecting another `/clear`. If it meets a proven-live harness turn (`harness_turn_live=true`) whose pane the document's supervisor owns, the binary likewise queues one clear behind that turn and exits 0 with `session_clear queued for <file> — a turn is live in pane <pane> ... ui_outcome=queued`; plugins must show an informational "queued until the current turn ends" notice for it, never a command failure (`#clearqueuedliveturn`). The notice's first line states the outcome and when it runs (a balloon collapses to its first lines and expires after a few seconds), and it relays what the binary reports: a repeat press that re-armed the one queued clear (`rearmed=true`) reads "re-armed … still one clear", and the live-turn evidence kind and its age (`evidence=<kind> evidence_age_secs=<n>`) are shown so a stale receipt absorbing presses is visible (`#clearnoticeverify`). When the supervisor later DROPS that queued clear because a newer prompt started on the pane before the turn ended, the controller's document-turn authority stream carries `deferred_clear_superseded` (`clear_epoch`, `pane`, `reason=newer_prompt`, `superseded_at`); plugins must surface it as an informational "queued Clear Session Context was not sent" notice naming the document and pane and telling the operator to press Clear Session Context again (or Interrupt and clear) if they still want a fresh context (`#clearsupersedenotice`). The notice is edge-triggered once per `clear_epoch`: the first projection a plugin observes for a document seeds the baseline without notifying (an old drop is not news on reopen or plugin restart), a reconnect or an `unavailable` frame does not reset it, and a projection without the field never retracts or re-raises it. Plugins render the stream field; they never poll for it, read supervisor logs, or infer the drop themselves. JetBrains (`DeferredClearSupersededNotices`, an information balloon from `TurnStateBannerRefresher` with a `Clear Session Context` action) and VS Code (`DeferredClearSupersededNotices` in `sessionUi.ts`, an information message with the same action, fed by the active document's authority stream) apply the same once-per-epoch rule and message. The live-turn refusal that remains when no supervisor owns the pane (`session_clear refused for <file> because a turn is live in pane <pane> (harness_turn_live=true evidence=<kind>, ...)`) is a busy-clear refusal like the alive-busy one and must take the same operator-warning surface, not the generic `agent-doc command failed` error. If clear is refused because the pane contains protected prompt input or an explicit busy cue that cannot be deferred, plugins must surface an operator warning with `Interrupt and clear`, `Show status`, and `Copy details` actions instead of showing the raw command failure. Codex dim placeholder text is not protected prompt input; the CLI owns that ANSI-aware distinction. `Interrupt and clear` must require explicit confirmation and then run `agent-doc session interrupt-clear <relative-path>`; it is available from the popup/menu as `Interrupt and Clear Session Context` as well as from clear-refusal notifications, and it is the only plugin action that intentionally discards protected prompt input. After that confirmation, editor plugins must not synthesize additional terminal keystrokes: the binary owns harness interrupts, Vim/Neovim prompt recovery, idle/closed waiting, clear retry, and exact recovery messages.
- **Clear settlement without document authority:** Once the owning pane reports the cleared harness prompt, the supervisor must settle the in-flight clear and return the actor to `ready` even when the open editor replica or queue authority is temporarily unavailable. Document attachment may recover independently; it must not leave a successful clear displayed as `agent-doc: awaiting response` or prevent a later Compact Exchange retry after reattachment.
- **Copy Session Diagnostics:** Run `agent-doc session doctor <relative-path>`, show the output in an IDE-owned diagnostics surface, and offer a one-click copy path for the exact text.
- **Verification floor:** editor-plugin tests must cover exact session-status display, `session clear` command routing, and a persistent route-dispatch failure surface with the exact stage-specific CLI output.

## 6c. Dashboard

- **Availability:** Project-scoped, not markdown-gated: the editor's Agent Doc popup (numbered, after the session actions), the command palette / action search (`Agent Doc: Dashboard` in VS Code, `Agent Doc Dashboard` in JetBrains), and the Tools / editor / project-view context menus in JetBrains.
- **Behavior:** Run `agent-doc dashboard --write` from the focused markdown document's project root (else the workspace/project base) and then open the generated projection `<root>/.agent-doc/dashboard.md` — VS Code opens its markdown preview, JetBrains opens the file (split preview per the IDE's markdown setting). Opening it is an explicit operator request, so taking focus is correct here. Writing the default path arms the project controller, which keeps the file current on state change; the editor shows the live view through its normal external-change reload.
- **Never a session document:** the projection carries no frontmatter, no `<!-- agent:` marker, and no `agent_doc_*` token, so content-based session classification (`isAgentDocDocumentTextUtil`, `agent_doc_is_session_document`) rejects it. Adapters must not register a CRDT replica for it, report it in the editor surface, or route it to tmux.
- **Feedback:** A failed or timed-out render surfaces the exact CLI output as a persistent error; success needs no extra notification beyond opening the view.
- **Verification floor:** plugin tests cover the exact command arguments, the projection path, popup/palette reachability, and that the rendered projection is not classified as a session document.

## 7. Notifications

- **Success:** Lightweight inline hint near cursor (auto-dismissing, ~1-2 seconds).
- **Error:** Persistent notification entry/tool-window output. Errors never auto-dismiss.
- **No temp files:** All diagnostic logging uses the IDE's built-in logger, not file I/O.

## 8. File Filtering

- All actions are only enabled/visible when a `.md` file is active or selected.
- Non-`.md` files are ignored by tab sync.

## 9. Boundary Marker Management

Boundary markers (`<!-- agent:boundary:{id} -->`) are transient UI elements that mark the insertion point for agent responses in the exchange component. Plugins must maintain the following invariants:

**Invariants:**
- At most ONE boundary marker exists in the document at any time (outside of code blocks)
- The boundary is always at the END of the exchange component content
- Boundaries inside fenced code blocks are never touched

**Boundary ID format:** 8-character hex string (e.g., `a0cfeb34`). Plugins generating boundary markers must use 8-char hex IDs, not full UUIDs.

**Reposition behavior:** When the plugin receives a `reposition` IPC signal (or `reposition_boundary: true` in a patch payload):
1. Remove ALL `<!-- agent:boundary:... -->` lines from the exchange component (not just the last one)
2. Insert a single boundary at the end of the exchange content.
   If the IPC payload includes `reposition_boundary_id` / `boundary_id`, reuse that exact ID.
   Otherwise generate a fresh 8-char hex ID.
3. Check the `preserve_head` field in the IPC JSON (default `false`):
   - **`preserve_head: false`** (clean variant) — strip all transient ` (HEAD)` heading annotations during cleanup. Use `agent_doc_reposition_boundary_to_end()` / `_with_id()` FFI.
   - **`preserve_head: true`** — keep ` (HEAD)` annotations on `### Re:` headings so the user sees which responses are new. Use `agent_doc_reposition_boundary_to_end_preserve_head()` / `_with_id()` FFI.
4. Skip boundary markers inside fenced code blocks

**FFI variant families:**
| Variant | FFI function | Behavior |
|---------|-------------|----------|
| Clean | `agent_doc_reposition_boundary_to_end()` | Strips `(HEAD)`, collapses boundaries |
| Clean + ID | `agent_doc_reposition_boundary_to_end_with_id(doc, id)` | Same + reuses explicit boundary ID |
| Preserve | `agent_doc_reposition_boundary_to_end_preserve_head()` | Keeps `(HEAD)`, collapses boundaries |
| Preserve + ID | `agent_doc_reposition_boundary_to_end_preserve_head_with_id(doc, id)` | Same + reuses explicit boundary ID |

**Compact full-content patches:** `agent-doc compact <file> --component exchange --commit` uses the guarded direct-write path rather than emitting editor IPC `fullContent`. Editor plugins must reject or delete any legacy/foreign `fullContent` payload without applying a whole-document replacement.

**Recommended implementation:** Call the appropriate FFI variant via JNA on the shared library (`libagent_doc.so` / `libagent_doc.dylib`). This ensures identical cleanup logic across all platforms and prevents divergence between plugin and binary behavior.

**When to reposition:**
- After applying an IPC patch (when `reposition_boundary` flag is set)
- After receiving a standalone reposition IPC signal (post-commit). Post-commit signals should carry the committed `boundary_id` so the editor normalizes back to `HEAD` instead of inventing a new local diff.
- After commit cleanup when only file-watch IPC is available. The CLI must enqueue a reposition-only JSON patch in `.agent-doc/patches/<hash>.json` with `patches: []`, `reposition_boundary: true`, `preserve_head: true`, and the committed `reposition_boundary_id` instead of rewriting the markdown file on disk. Editors must apply that patch through the native document API and preserve ` (HEAD)` markers.

**Response heading visibility:** During normal IPC patch application (before post-commit cleanup), plugins must preserve transient ` (HEAD)` markers on newly added `### Re:` headings in `agent:exchange`. That marker is part of the user-visible "fresh response is still uncommitted" state; only the explicit clean reposition/commit path is allowed to strip it.

**Visible-write structure guard:** Before publishing an IPC receipt for a component patch and before mutating the editor-visible document buffer, plugins must run the shared Rust template-structure guard (`agent_doc_normalize_template_structure`) on the final candidate document. The guard may repair safe duplicate scaffold shells, such as a duplicated queue/backlog scaffold between two `<!-- /agent:exchange -->` closers, but it must fail closed when that duplicated shell contains user text or other ambiguous content. A rejected guard means the plugin must not call `Document.setText`, `WorkspaceEdit`, or VFS binary-content replacement for that candidate.

**Active-typing visible-write guard:** Before any editor-visible document mutation, including socket patches, file-watch patches, and boundary reposition, plugins must wait for the shared typing debounce. If the bounded wait times out, the plugin must not call `Document.setText`, `WorkspaceEdit`, or VFS binary-content replacement. File-watch patches stay on disk for retry; socket writes fail closed so the CLI can take its normal retry/fallback path. Disabled full-content file-watch payloads are deleted as stale/foreign patches instead of retried.

**CRDT-only attached write convergence:** Unless the operator explicitly requested the `--force-disk` recovery escape hatch, a document with an attached CRDT replica must wait for typing quiescence and use the canonical CRDT document as the sole visible mutation plane. Every accepted candidate is first retained in the Lazily deferred-write lineage. Each editor publishes its complete visible-state hash after integrating deliveries; that projection cumulatively retires every represented pending frontier, and an identical retry is an idempotent canonical no-op. An accepted CRDT replacement must never be followed by component/full-content IPC for the same candidate. Disk materialization is only a projection of exact canonical text after every attached replica projects the delivery frontier, and it is refused if canonical text advances first. No replay, re-register, force-refresh, save, or build-skew recovery request is emitted from the convergence subscription. Explicit force-disk writes still pass through the per-document actor, retain their pre-write base and target in Lazily state, and intentionally skip the CRDT gate.

**Open-editor external-disk pending:** Any disk change observed while one or more editor buffers are open is retained in an independent Lazily external-disk slot; it must not replace the pending agent-response lineage or enter canonical CRDT state. Authority is editor/CRDT, then disk after the final editor closes, then Git only as historical recovery evidence. Accepting/reloading the exact disk candidate reboots the stale replica from the visible editor buffer and clears the candidate only after CRDT propagation. A newer editor mutation clears the candidate and propagates the editor cut without component merge. Saving the editor and flushing those exact bytes to disk also clears any older candidate and makes the saved cut authoritative. If several replicas are still converging and no exact editor cut is proven, the disk candidate remains mutation-free pending. JetBrains and VS Code must implement the same resolver/propagation FFI contract.

**Replica-churn provenance (`#falsetyping-guard`):** A `remoteCrdtApply` is not operator typing and must not be reported as a genuine unsaved operator edit. The compatibility FFI entrypoint `agent_doc_document_changed_digest_content_for_editor_v3(..., no_unsaved_operator_edits)` carries only that provenance; its legacy metadata and full-content arguments must never become a second registration or text channel. Current text is read from the CRDT. Each plugin tracks a per-document unsaved-local-edit flag: set on a non-`remoteCrdtApply` document change and cleared when saved, closed, or observed clean. JetBrains uses `Document`/`FileDocumentManager.isDocumentUnsaved`; VS Code uses `TextDocument.isDirty`.

**Monotone editor registration:** JetBrains and VS Code include identity, path, kind, version, and capabilities in the same durable reliable-sync batch as each `Open` fact. Registration is therefore once per open epoch, not a side effect of rate-shaped content reports. Text-change listeners use the matching lazily-kt/js `DebounceCore` KeepLatest primitive only for compatibility provenance reports and CRDT ops; superseding input cancels the old timer without losing the logical pending value. Close/dispose emits the observed-tag `Close`, so an old close cannot erase a newer open/registration epoch. A close is a document close only when no editor for the file remains open in any client (GH #241): an event that removes one of several editors for a still-open file — including a Remote Dev guest's duplicate editor — must not publish the closing cut, deregister the replica, report the per-editor close, or emit the liveness `Close`. See `specs/14-realtime-workflow.md` § Removing one editor is not closing the document. JetBrains gates `fileClosed` on that observation; VS Code's `onDidCloseTextDocument` is already document-scoped.

**CRDT replica transport:** First-party editor plugins send CRDT replica IPC only to the Project Controller socket (`.agent-doc/controller.sock`) using the controller `crdt_replica` envelope. Every editor payload carries the real editor process PID as `editor_pid`. A detached agent-doc `replica_register` is delegated to the relay so that live process-scoped proof may establish editor authority; generic/dead registrations, non-agent-doc markdown, and non-registration detached operations remain refused. Plugins must not connect to per-session supervisor sockets such as `.agent-doc/supervisor/*.sock` for replica register/update/pull/ack/deregister/current-text work. Remote CRDT delivery is event-driven through the shared targeted `EditorIntent` channel and Lazily-backed controller subscriptions; it never writes a per-document event file. Startup/open-document attach and local-delta forwarding may also queue one-shot drains, but plugins must not run a fixed interval pull loop, hot polling loop, spinlock, or thread-per-delay CRDT retry loop. Transport acceptance is provisional until retained-projection reconciliation commits: replacement registration keeps the predecessor membership authoritative, `replica_promote` atomically retires it against the candidate's exact accepted canonical frontier, forced refreshes and ambiguous holds preserve the existing retry projection, and only a committed endpoint clears its backoff state.

**Registration projection decision (GH #326 phase 5):** which direction a replica registration projects (adopt the controller canonical, publish the live buffer from its settled shadow, merge forward, defer to a captured local delta, or hold the buffer unchanged) is the native `registration_projection` derivation of `agent_doc_editor_derive_json`. An adapter compares its three text generations itself (settled shadow, live buffer, endpoint canonical) and reports only which are present and which are equal, with the native containment-proof and clean-merge answers; document text never crosses the derivation channel. The adapter executes the answer and keeps the publication-edge rechecks and their order. JetBrains keeps a frozen copy of the pre-#326 rules for a library that cannot answer and for an action it cannot execute; `editors/jetbrains/src/test/resources/editor-derivations/registration_projection.json` (named cases plus an exhaustive table) is answered by both.

**Attach-failure diagnosis (GH #326 phase 5b):** what a replica attach failure reason means for the operator (the remedy, the `attach_refused_<token>` ops-log status token, whether a projection hold is surfaced without an editor action, whether a stale-endpoint refusal republishes reliable liveness, and the hold notification) is the native `attach_failure_diagnosis` derivation of `agent_doc_editor_derive_json`. An adapter sends the recorded reason and the document's file name, asks off its UI thread, and executes the answer in its existing order. It also checks its live replica first and sends `live_replica_attached: true` when the document still has an attached, syncing replica; native then answers `hold_notification: "log_only"` and, for a projection hold, no remedy, because only a replacement registration was refused and the close/repair/reopen remedy would detach the healthy replica. Repair instructions are shown only when no replica is attached. JetBrains keeps a frozen copy of the pre-#326 rules for a library that cannot answer; `editors/jetbrains/src/test/resources/editor-derivations/attach_failure_diagnosis.json` is answered by both.

**Route outcome (`#jbshrinkrestarts`):** what a finished Run Agent Doc route's output means for the operator (the outcome kind, the paused-queue and deferred-harness-switch details, the notification text for every non-persistent kind, the latest route snapshot path, and the owned-by-other-lane and pane-not-visible success notices) is the native `route_outcome` derivation of `agent_doc_editor_derive_json`. An adapter sends the route output and the document's relative path from its route thread and executes the answer: the notification type and actions, ops-log lines, and state projection stay in the adapter. Steering delivered is the exception: its summary is not shown. JetBrains keeps a frozen copy of the pre-move `TerminalUtil` rules for a library that cannot answer and for a kind it cannot execute; `editors/jetbrains/src/test/resources/editor-derivations/route_outcome.json` is answered by both.

**Exchange prefix repair (`#jbshrinkrestarts`):** the JetBrains patch watcher's `❯ ` prompt-prefix repair of a patched document (the operator region before the last boundary, the target match, and the response-block heuristic) is the native `exchange_prefix_repair` derivation of `agent_doc_editor_derive_json`. The patch worker sends the document and the patch's `normalize_prefix_lines` and takes the repaired document; an unchanged answer does not carry it back. JetBrains keeps its pre-move `normalizeExchangePrefixesUtil` as the frozen floor; `editors/jetbrains/src/test/resources/editor-derivations/exchange_prefix_repair.json` is answered by both.

**Session command outcome (`#jbsessioncmdoutcome`):** what a finished Clear Session Context or Restart (agent or supervisor, plain or forced) session command's output means for the operator (the outcome kind: cleared now, queued behind a live turn, a settling prompt, or an accepted run, deferred under a busy auto-loop, refused, or a restart refused because a terminal editor holds the pane, the pane is busy, or the actor is still starting; the queued-clear and refusal details; and the notification text of each) is the native `session_command_outcome` derivation of `agent_doc_editor_derive_json`. An adapter sends the command, whether it exited zero, whether it was forced, the output, and the document's relative path from its command thread and executes the answer: the notification type and actions, the restart telemetry read, and the clear-command bookkeeping stay in the adapter. JetBrains keeps a frozen copy of the pre-move `TerminalUtil` rules for a library that cannot answer, for a kind it cannot execute, and for a refusal kind whose details cannot be read. VS Code asks the same derivation for Clear Session Context and Restart Supervisor (plain and Interrupt and restart) and keeps a frozen TypeScript port of the native rules (`sessionCommandOutcome.ts`, `#vscodesessionparsers`) under the same three fallbacks; it reads stdout and stderr combined, as JetBrains does, because a queued or deferred clear is reported on stderr with exit 0. `editors/jetbrains/src/test/resources/editor-derivations/session_command_outcome.json` is the one parity file: the Rust derivation, the Kotlin floor, and the TypeScript floor all read it in place and answer every case.

**Registration splice shaping (GH #326 phase 5c):** a replica registration's containment proof (`agent_doc_captured_splices_contained`) and clean merge (`agent_doc_rebase_captured_splices`) accept an optional `registration` input in place of a captured batch: `{"registration": {"buffer": ..., "operatorText": ...}}` beside the raw shadow (`base`) and canonical. Native then removes binary-owned markers when asked for operator text and builds the one code-point splice from shadow to buffer itself, so those rules change with a library hot reload. An adapter sends it only when the `capabilities` derivation lists `registration` in `captured_splice_inputs`; otherwise it shapes the batch itself. JetBrains keeps its pre-#326 shaping as that frozen floor; `editors/jetbrains/src/test/resources/editor-derivations/registration_splice_shaping.json` is answered by both.

**Cross-editor native harness:** `make cross-editor-simworld` runs the shipped JetBrains and VS Code forwarders, Project Controller transports, and native FFI nodes as headless peers through a real `agent-doc` Project Controller. `editors/plugin-parity.tsv` selects only peers whose required feature rows are `supported`; `staged` peers are excluded until their endpoint and receipts exist. This suite proves plugin transport/FFI behavior, while IDE-host Document/VFS wiring remains the responsibility of live editor smokes.

**Prompt steering and stale process state:** The Project Controller owns prompt-steering reconciliation for concurrent operator edits. Editor IPC apply, receipt, visible-write proof, and repair decisions must not be vetoed by legacy per-session supervisor freshness checks. Stale supervisor detection may remain an operator/session-management diagnostic, but it is not an editor-plugin transport guard and must not short-circuit Project Controller-owned editor delivery.

**Editor state signals:** Turn-state status is read from the Project Controller `state_subscribe` Lazily projection and mirrored in the plugin; first-party plugins do not read filesystem state for ordinary status/UI synchronization. If the Project Controller is unavailable, the editor status bar shows `agent-doc: Project Controller disconnected` rather than electing a fallback. CRDT remote delivery, VCS refresh, and native-library reload use the same targeted editor-intent channel and reliable-sync receipts. Turn-state projection refreshes are cached/coalesced and bounded with minimum refresh spacing so repeated editor events cannot monopolize an editor worker.

**Reactive persistence projection:** There is no editor save request. Applying a
controller CRDT projection and the editor platform's ordinary save lifecycle
publish visible-state and persisted-state Sources. The binary derives exact
convergence and resumes retained effects from those projections. Plugins must
not implement a `save_document` intent, save signal, ACK sidecar, reconnect
reread repair, or secondary full-content delivery channel.

**Editor apply proof:** After the debounce succeeds and before computing a visible mutation, plugins must capture an editor-local apply proof for the target buffer: the exact text plus the editor's generation signal (`Document.modificationStamp` in JetBrains, `TextDocument.version` in VS Code). Immediately before `Document.setText` or `WorkspaceEdit`, the plugin must re-check that both the text and generation still match that proof. If either changed, the intent is stale relative to live typing and must be rejected without a Lazily receipt. This applies to every named socket intent and exchange append operation; file IPC does not exist.

**Full-content IPC receiver disabled:** First-party CLI paths no longer emit full-content IPC payloads, and editor plugins must not apply any legacy/foreign `fullContent` payload. File-watch payloads with `fullContent` are logged and deleted so they cannot retry indefinitely; socket payloads fail closed. Source-buffer proof fields remain parseable only for diagnostics and are not authorization to replace the document.

**Cycle 1779845677327 receiver fixture:** The historical race packet combined a legacy `fullContent` payload with a post-exchange scratch HTML comment containing `#spec-test-build-install-commit-push` and `dispatch #spec-test-build-install-commit-push`. Both JetBrains and VS Code receivers must reject that payload before any visible write API call; the directive-looking lines inside the ordinary comment are not an editor-side authorization to replace, move, or delete user text.

**IPC repair decision contract:** When lazily visible-write normalization diverges or post-IPC dedupe changes the committed snapshot candidate, the binary must materialize a typed repair decision before touching the visible buffer. The decision records the repaired snapshot content, snapshot source (`lazily_visible_write_event`, `content_ours`, or `file_read`), disk-repair reason, normalization targets, editor bad-state length/hash fingerprint, and `redeliver_editor=true|false`. Snapshot-only repair is binary-owned: when `redeliver_editor=false` or full-content IPC is disabled, the binary may repair snapshot/disk state without sending editor `fullContent`, and plugins must not synthesize a visible replacement. Prefix-only visible-write divergence should be re-delivered as a narrow normalization/reposition patch (`patches: []`, `unmatched: ""`, `normalize_prefix_lines`, `reposition_boundary: true`) when the visible buffer still matches the rejected bad-state proof; plugins must treat that payload as normal patch work, not pure reposition. Full-content editor redelivery is disabled for all first-party editor integrations.

**Per-connection listener thread (`#jbacceptwedge`):** The shared FFI socket listener (`agent_doc_start_ipc_listener[_v2]` in `ipc_socket.rs`) must service each accepted connection on a fresh thread so a slow CRDT-merge / disk-I/O apply handler can never block the accept loop and pile connections up in the socket backlog (the "22 unaccepted connections" wedge). Both IntelliJ and VS Code consume that same listener through JNA/FFI, so accept-loop threading stays in the binary — the editor plugin owns only the per-message callback body. Each accepted connection records an `ipc_accept_thread_spawned inflight=N` ops.log marker; any live entry with `inflight>=2` proves the per-connection path is exercising concurrency that the old single-threaded loop could never reach.

## 10. CLI Dependency

- Plugins resolve `agent-doc` from: `~/bin/`, `~/.local/bin/`, `~/.cargo/bin/`, `/usr/local/bin/`, or `$PATH`.
- All commands run from the project root directory.
- Plugins are thin wrappers — business logic lives in the CLI.
- Native/FFI code embedded in an editor host must never treat `std::env::current_exe()` as the agent-doc CLI unless that executable basename is `agent-doc*`. In JetBrains and VS Code hosts, `current_exe()` is the editor process; controller, recycle, and handoff launches must resolve the real CLI through an explicit agent-doc binary override or the same install-path/PATH search used by plugin subprocess commands.

## 11. Visual Distinction

- Markdown editor integrations should visually distinguish agent-doc structures without requiring the user to switch document formats or install a separate grammar pack.
- Both JetBrains and VS Code must source their highlight ranges from the shared FFI surface (`agent_doc_visual_tokens_json`) so component markers, agent-managed component bodies, patch markers, boundary markers, `### Re:` headings, `❯` prompts, tracked `[#id]` tags, standalone bracket labels such as `[recommended]`, and ordinary HTML scratch comments plus their bodies stay in sync across editors. Every `[#subagent-operator-request-<unique-id>]` handback marker is a tracked ID and MUST use exactly the same token kind and styling as IDs in `agent:todo`, `agent:backlog`, `agent:run`, `agent:icebox`, and `agent:review`.
- `agent_doc_visual_tokens_json` returns UTF-16 document offsets, not raw UTF-8 byte positions. Plugins must treat those offsets as editor-ready range endpoints and pass them directly to native document APIs.
- Matches inside fenced code blocks or inline code are excluded from this highlighting contract; example markup in code samples must remain untouched.
- **Refresh cost (`#keepidesnappy`):** the highlight refresh is debounced off the editor event thread, and the event-thread apply must be incremental. An adapter whose editor API keeps one object per highlighted range (JetBrains `RangeHighlighter`) reconciles its live highlighters against the new tokens by range, layer, kind, and style, keeps every match untouched, and removes or adds only the difference. VS Code's `setDecorations` already replaces a whole decoration type in one call. A refresh after a pause in typing must not tear down and re-create every highlighter, because each one is a markup event and a repaint on the UI thread.

## 12. Component Outline (Folding, Gutter Markers, Structure View)

- **Scope:** agent-doc session documents only (the content classification in § 6c); any other Markdown file shows no folds, markers, or nodes. Plugin UX phases 5-7 (GH #19).
- **Boundaries:** sourced from the shared native parser (`agent_doc_parse_components`), never re-parsed in the editor. Its offsets are UTF-8 bytes; adapters convert them to their editor's offset unit. The parse runs off the editor event thread on the debounced visual refresh; folding/marker/outline callbacks read a cached outline that tracks edits and never call native code. An unavailable library or a mid-edit unparseable document keeps the last outline.
- **Items:** a component body's ATX headings when it has any, otherwise its unindented list items (checkbox prefix stripped); lines in fenced code and in nested components do not count. Line-local list comments whose complete payload is `[//]: # (...)` or `%%...%%` remain authored annotations and do not become outline items, item counts, or fold-summary counts.
- **Folding:** every multi-line component folds open marker through close marker to `<!-- agent:NAME · N items -->` (`1 item`, `empty`). Nothing is collapsed by default.
- **Gutter marker:** one icon per component open marker; tooltip = name, item count, inline attributes; click toggles the component's fold.
- **Structure view:** components (item count as secondary text) with nested components and items as children, each navigable; the editor's own Markdown outline is extended, not replaced.
- **Status:** JetBrains ships this (`editors/jetbrains/SPEC.md`). VS Code and Zed are staged. These are editor UI features, not `plugin-parity.tsv` capabilities.

## 13. Frontend/Backend Architecture Parity

The JetBrains plugins are the reference for three properties that matter
whenever an editor separates the process that renders the UI from the process
next to the workspace (GH #339, GH #376):

- **Restart-free upgrades:** a stable shell that the host loads once, plus a
  swappable implementation. In every editor here, the native library behind
  `agent_doc_editor_derive_json` and the other `*_json` entry points is the part
  that swaps today.
- **Activation receipts:** proof that every process taking part in a request
  runs the expected generation (`agent_doc_activation_receipt/v1`, designed in
  `editors/jetbrains/SPEC.md`).
- **Measured-layout routing:** tmux columns derived from layout the UI actually
  measured, not reconstructed from focus or ordinal evidence.

This section records where each host stands on those three properties. It
records facts about each host and its current agent-doc adapter. It is not a
capability declaration: `plugin-parity.tsv` remains the source of truth for wire
capabilities, and its `native_hot_reload_generation_v1` status is repeated in
the last column below.

### Parity matrix

| Host | Where agent-doc code runs | Upgrade mechanism and restart | Generation proof across the split | Measured-layout routing | agent-doc status |
|---|---|---|---|---|---|
| JetBrains classic (242-261, local IDE) | One IDE JVM: the plugin jar and the JNA-loaded native library. | `agent-doc plugin install` uses the classic attach launcher, which runs a `DynamicPlugins` unload and reload on compatible builds and falls back to a restart prompt. Native derivations hot-reload. | Single process. The controller registration carries the plugin version and one `agent_doc_activation_receipt/v1` line with `role=backend` (GH #339 R1); `#pluginactivationprobe` reads the loaded generation; a sticky restart balloon shows when the installed jar differs from the loaded one. | `window_layout` derivation from the frame's splitter coordinates. The rules are native, so changing them needs no restart. | supported; native hot reload conditional |
| JetBrains split mode (exact 262 / exact 263) | Two JVMs with one ZIP installed on both. The Remote Dev backend runs `agent.doc.backend` and the native library. The JetBrains Client runs `agent.doc.frontend`, which measures the main window. `agent.doc.shared` holds the Fleet RPC contract. | Restart-only: the 263.7031.23 module-graph refusal (GH #299), and no cross-JVM handoff to the frontend (GH #333, GH #339 decision). Marketplace is the only stock channel that updates both peers. Native derivations hot-reload while the source digest is unchanged. The proposed path is the shell plus child loader (R1-R4) under the `SHELL_ONLY` frontend policy. | Shipped: the backend holds `frontend_authority_absent_or_generation_mismatched` readiness. Implemented in GH #339 R1: one receipt per JVM, where the backend's `role=backend` line rides its controller registration and the frontend's `role=frontend` line rides the surface snapshot (schema 5). Designed, with a test-only spike: the backend/frontend activation fence (R4). | `client_measured_layout` (GH #320 option B), forwarded through `jetbrains-rd@<installation>`. Zero-column routes use it (GH #376). Flat backend evidence is focus/liveness-only. | supported; plugin restart-bound; receipts implemented, fence designed |
| VS Code local | One Node extension host on the local machine: the TypeScript extension and the koffi-loaded native library. The renderer belongs to VS Code itself. | VSIX through `agent-doc plugin install vscode` (`cursor`, `codium` or `code --install-extension`, plus any workspace host on the same machine, as in the next row). Since 1.88, VS Code offers to restart extensions instead of reloading the window after an update. The native library hot-reloads (per-mtime shadow copy and the `reload_library` intent). | Single process. Registration carries `EDITOR_PLUGIN_VERSION` and one `agent_doc_activation_receipt/v1` line with `role=workspace`: the sha256 of the loaded bundle as `shell`, the native library's version and sha256 as `impl_version` and `impl_sha256`, and the count of native loads as `generation` and `epoch`. A native reload re-sends the registration. `agent-doc reliable-sync-status` prints the parsed receipt. About shows the loaded native generation. The extension compares its running version with the agent-doc generation its own extensions directory's `extensions.json` lists (skipping `.obsolete` directories), on activation and on each change to that file, and offers the host's own reload: Restart Extensions on the extension page (VS Code has no command that loads the update in place), or Reload Window. `installed_plugin_version` in the binary still covers JetBrains only. | `vscode.getEditorLayout` measures the grid: orientation and nested group sizes give each group's origin, groups sharing a left edge stack in one column, and the claim position follows the root split (`left`/`right` or `top`/`bottom`). The rules are the native `editor_group_layout` derivation of `agent_doc_editor_derive_json`, so changing them needs no reload; `groupLayout.ts` is the frozen floor, answering the same shared fixture. Measured columns publish with order `editor`. A grid that does not match the tab groups (an auxiliary editor window) falls back to ordinal `viewColumn` columns with order `unknown`. | supported; native hot reload conditional |
| VS Code on Coder / remote | UI on the operator's machine (VS Code Desktop through the Coder Remote extension over Remote-SSH) or in the browser (code-server, VS Code Web). agent-doc must run in the remote (workspace) extension host, beside the agent-doc binary, `libagent_doc.so`, tmux and the controller. `package.json` declares `"extensionKind": ["workspace"]`, so VS Code runs it there. | Install into the workspace host: Coder documents `code --extensions-dir ~/.vscode-server/extensions --install-extension` and `code-server --install-extension`. The 1.88 notes say a remote connection still needs a window reload to update extensions. `agent-doc plugin install vscode`, `plugin update` and `plugin list` reach the workspace host: `code-server` on `PATH`, and the newest Remote-SSH server under `~/.vscode-server` (or `$VSCODE_AGENT_FOLDER`), `~/.vscodium-server` or `~/.cursor-server`, through that server's own CLI with `--extensions-dir <root>/extensions`. | As local: one `role=workspace` receipt, because there is no agent-doc frontend half and nothing has to be compared across processes. `vscode.env.remoteName` reaches the binary, and the `agent-doc` terminal the extension creates, as `AGENT_DOC_VSCODE_REMOTE_NAME`. The installed-versus-running check is the same as local, against the workspace host's extensions directory; a remote (`remoteName`) or web host is offered only Reload Window. | Same API as local. VS Code proxies tab and editor state, and the `vscode.getEditorLayout` command, from the UI into the workspace host, so no frontend forward is needed and the same measured derivation applies. | documented in the Coder runbook; remote smoke test scripted (`scripts/vscode-remote-smoke.py`); its install, register and remote-host steps passed against a local code-server 4.143.1 and the VS Code Server 1.143.1 CLI, and it has not yet run on a Coder workspace |
| Zed local | A sandboxed WASM extension (`wasm32-wasip2`) inside Zed, which only resolves the `agent-doc zed-lsp` command. The language server is a separate agent-doc process speaking LSP over stdio. | Zed watches its installed-extensions directory and reloads a changed extension, removing and re-registering its language servers, without restarting Zed. agent-doc ships only as a dev extension. A replaced agent-doc binary is picked up by an in-place handoff: `zed-lsp` watches the binary it started from and, once a replacement settles, `execve`s into it with the same PID and stdio. It carries its unparsed input, CRDT replicas and request ids, after the replacement has validated that exact handoff as a child process. If validation or `execve` fails, it keeps serving and asks for `editor: restart language server`. `zed-lsp` has no native hot reload. | The LSP `serverInfo.version` field. The `replica_register` registration carries `editor_kind` (`zed`), `editor_version` (the extension version), `editor_binary_version`, `capabilities` (none: every Zed capability is staged) and `layout_authority`. The controller reports them against its paired generations (`controller_crdt_replica_editor_generation`) and keeps them off the liveness plane, because `zed-lsp` has no editor endpoint. After a handoff, the registration still names the binary that registered. | None. Neither the extension API nor LSP exposes panes, splits or the focused editor. The controller routes Zed by document only: an editor route from a Zed source loses any layout evidence and ensures only the routed document (`controller_editor_route_document_only`). | staged (all capabilities); native hot reload staged |
| Zed remote | Zed UI locally. A headless Zed server on the remote, which must exactly match the local Zed version, runs language servers, tasks and terminals. Locally installed extensions (dev extensions by content fingerprint) are synced to the remote. `zed-lsp` resolves from the remote worktree `PATH`, so it runs beside the controller. | Zed syncs extensions by id and version and loads them on the remote host. Zed replaces its own remote server to match the client. The agent-doc binary on the remote behaves as in local, including the in-place handoff. | Zed's extension sync compares versions, not activation. The registration is the same as local and reaches the remote host's controller. | None (as local); document-only routing. | staged; not qualified: `editors/zed/REMOTE.md` and `scripts/zed_remote_qualify.py` exist, but no Zed remote host has run them |

The VS Code facts are from `editors/vscode/package.json`,
`editors/vscode/src/` (`groupLayout.ts`, `pluginUpgrade.ts`, `activationReceipt.ts`,
`reliableSyncLiveness.ts`, `extension.ts`), and the VS Code install targets in
`src/plugin.rs`. `agent-doc-document-realtime/src/editor_contract.rs`
pins the adapter facts this matrix states as gaps, so closing a gap fails that
test until the matrix is updated. The Zed facts are from `editors/zed`,
`agent-doc-zed-lsp-io`, and the controller's `replica_register` and
`editor_route` handlers.

### Operator questions

**Does VS Code on Coder have a similar architecture?** Only partly. VS Code
does split the UI from the workspace: a local or browser UI talks to a remote
extension host in VS Code Server. However, an extension is not divided into
frontend and backend modules that load on both sides, as in JetBrains split mode.
Each extension runs whole in one host, chosen by `extensionKind`, and VS Code's
own API carries editor, tab, terminal and command state across the boundary. For
agent-doc this means:

- The backend/frontend generation-skew problem that the JetBrains activation fence
  solves does not arise.
- Layout evidence already reaches the workspace host.

VS Code on Coder now declares its workspace placement, reports a
`role=workspace` activation receipt, and installs into the workspace host.
What it still lacks is the rest of the JetBrains parity work:

- a run of the remote smoke test on a real Coder workspace.

**What does Zed's overall architecture imply?** Zed is a native application, and
its extensions are sandboxed WASM. The extension API covers languages, language
servers, debuggers, themes, icon themes, snippets and MCP servers, with no
access to panes or commands. Remote development puts the whole UI locally and
runs language servers on a version-matched headless server. Zed already reloads
an extension without restarting, so the WASM extension acts as the stable
shell. Parity therefore depends on three things:

- **The implementation it launches:** the `zed-lsp` process must notice a
  replaced binary and hand over to it without losing its LSP session. It now
  does this with an in-place handoff (see "Zed binary pickup" below).
- **Identity:** `zed-lsp` must register an editor kind, version and
  capabilities so the controller can tell which generation is running. Its
  registration now carries them.
- **Layout:** measured-layout routing cannot be reached through any surface
  Zed exposes today. Zed routes by document only and cannot claim exact-layout
  parity. The controller enforces this.

### What each editor needs for parity

- **JetBrains:** the R1-R4 migration in `editors/jetbrains/SPEC.md` (shell,
  receipts, live swap). That work is already tracked there.
- **VS Code (local and Coder):**
  - done: declare `"extensionKind": ["workspace"]`;
  - done: detect that the installed extension differs from the running one,
    and offer the host's own reload (Restart Extensions locally, Reload Window
    remotely);
  - done: add a single-role `agent_doc_activation_receipt/v1` line
    (`role=workspace`) to the registration, parsed by the controller
    (`agent_doc_reliable_sync_io::liveness::ActivationReceipt`);
  - done: capture orientation and group sizes through `vscode.getEditorLayout`
    and move the grouping rules behind `agent_doc_editor_derive_json`;
  - done: make `agent-doc plugin install vscode` reach the workspace extension
    host (`code-server` and the Remote-SSH server);
  - scripted, not yet run on Coder: the remote smoke test
    (`scripts/vscode-remote-smoke.py`: install, register, remote host, tmux
    attach, `remoteName`, layout sync; procedure in
    `runbooks/coder-workspace.md`).
- **Zed (local and remote):**
  - done: register kind, version, capabilities and layout authority;
  - done: in-place binary handoff, with a restart prompt as the fallback;
  - done: document-only routing, recorded here and enforced by the controller;
  - open: run `editors/zed/REMOTE.md` on a real Zed remote host and record the
    result. That covers agent-doc on the remote `PATH`, dev-extension sync, and
    the controller on the remote host. The scripted check has run only on a
    local host.
  - open: graduate Zed capabilities in `plugin-parity.tsv` (all are staged).
    `ZED_LSP_CAPABILITIES` must change in the same edit.

### Zed binary pickup

Decision: in-place `execve` handoff that keeps the LSP session, with the
language-server restart prompt kept only as the fallback. Reasons:

- **Zed never restarts the server on its own.** At the cited commit, Zed's
  LSP client does not restart a language server that exits. A restart goes
  through `restart_language_servers_for_buffers`, which the operator reaches
  with `editor: restart language server`. So a prompt always costs the
  operator an action, and exiting in the hope of a respawn would leave Zed
  without a server.
- **The handoff can be proved safe, not just made to work in practice.**
  - `execve` keeps the PID, so the controller identity `zed-lsp:<pid>:<hash>`
    and its process-exit watch are untouched.
  - `execve` keeps the stdio pipes, so Zed's session is untouched.
  - The server reads standard input only on its serving thread, into a
    buffer it owns, and hands over only between handled messages. Every byte
    Zed wrote is therefore still in the pipe or travels in the handoff file.
  - Each document's replica travels as encoded state plus client id. The new
    image continues the same CRDT peer, so no re-registration happens.
  - Nothing is replaced until the replacement has parsed and validated that
    exact file as a child process and answered
    `agent_doc_zed_lsp_handoff/v1 ok`. An older binary answers nothing and is
    refused.
- **It reuses agent-doc's existing pattern.** The MCP server (`src/mcp.rs`)
  and the supervisor `start` re-exec already swap images in place through
  `execve`. The difference here is that the whole session state is carried
  and validated first.

Tests:

- `agent-doc-zed-lsp-io` unit tests cover the framing and the round trip of
  every handoff fact.
- Further unit tests cover CRDT continuity across the handoff, the pid, schema
  and text fences, settling and refusal of a replacement, and the restart
  prompt.
- `tests/cross_editor_simworld.rs`
  `zed_lsp_hands_its_session_to_a_replaced_binary_in_place` replaces the
  binary under a running `zed-lsp`. It checks that a request split across the
  handoff is answered by the new image in the same process.

### Sources (checked 2026-10-26)

- VS Code: [Supporting Remote Development](https://code.visualstudio.com/api/advanced-topics/remote-extensions) (UI versus workspace extensions; APIs "automatically run on the correct machine"; VS Code Server runs non-Electron Node), [Extension Host](https://code.visualstudio.com/api/advanced-topics/extension-host) (local, web and remote hosts; `extensionKind` preference order), [Extension Manifest](https://code.visualstudio.com/api/references/extension-manifest) (`extensionKind`), [1.88 release notes](https://code.visualstudio.com/updates/v1_88) ("restart extensions instead of having to reload the window"; a remote connection "still need[s] to reload the window"), [Built-in Commands](https://code.visualstudio.com/api/references/commands) (`vscode.getEditorLayout`).
- Coder: [Access your workspace](https://coder.com/docs/user-guides/workspace-access), [VS Code](https://coder.com/docs/user-guides/workspace-access/vscode) (Coder Remote extension; installing extensions into `~/.vscode-server/extensions` or through `code-server`), [Web IDEs](https://coder.com/docs/user-guides/workspace-access/web-ides), [coder/vscode-coder](https://github.com/coder/vscode-coder) (builds on the editor's Remote-SSH, `ms-vscode-remote.remote-ssh`), [coder/code-server](https://github.com/coder/code-server).
- JetBrains: [Split Mode and Remote Development](https://plugins.jetbrains.com/docs/intellij/split-mode-and-remote-development.html) (two processes; frontend, backend and shared modules; RPC through shared interfaces).
- Zed: [Remote Development](https://zed.dev/docs/remote-development) (local UI, headless server, exact version match, extensions propagated), [Developing Extensions](https://zed.dev/docs/extensions/developing-extensions) (`wasm32-wasip2`; capability list), and `crates/extension_host/src/{extension_host,headless_host}.rs` at zed-industries/zed `e30095febcfd25ae81d52e59d7d80c2bd21830c6` (installed-directory watcher that reloads changed extensions; `ReloadExtensions`; `SyncExtensions` to the remote by id, version and dev fingerprint), plus `crates/project/src/lsp_store.rs` and `crates/lsp/src/lsp.rs` at the same commit (`ShowMessageRequest` surfaces as a prompt; restart only through `restart_language_servers_for_buffers`; no restart when a server exits).
