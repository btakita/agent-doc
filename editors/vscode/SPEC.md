# VS Code Plugin Specification

Extends `editors/SPEC.md` with VS Code-specific behavior.

## Plugin Metadata

- **ID:** `btakita.agent-doc`
- **Name:** Agent Doc
- **Activation:** Markdown documents

## Run Feedback

- `Run Agent Doc` saves the active markdown document, ensures the Project Controller is running, and dispatches a Project Controller `editor_route` request from the document's nearest agent-doc project root. The request carries the dispatch-only plain-trigger route payload, a 120-second ready wait, and relative `--col` / `--focus` layout arguments for the visible markdown projection, including empty physical split placeholders. The controller canonicalizes those facts and may materialize a missing focus only into one unique empty split before converging the exact-visible projection; route may not append the requested pane beside a stale layout.
- The synchronous command-plane route executes on a controller request thread
  with controller-local identity and the reactive document projection reader.
  Nested authority reads must not self-RPC through the controller socket; the
  generic 5-second external-client deadline is not a terminal `Run Agent Doc`
  result, and the whole route is not replayed after ambiguous dispatch.
- Repeating `Run Agent Doc` while an older `editor_route` request is still in flight must not cancel the older request. The first request owns the submit/proof window; later clicks for the same document are visibly deduped.
- `Run Agent Doc` and `Clear Session Context` are serialized per document in the VS Code extension layer before route/clear work is submitted. Repeated Run clicks dedupe behind the first alive `editor_route` request, and `Clear Session Context` pressed while a Run is still dispatching does not cancel it: the Run is a controller-side `editor_route` command the editor cannot cancel, so the extension keeps waiting for its outcome and runs the normal binary-owned `agent-doc session clear <relative-path>` path, which queues the clear behind that Run (`dispatch_turn_pending`, `#clearaftrun`) and reports it as queued. If Run is clicked while a normal clear command is already running, the latest Run intent is queued and starts only after the clear command and any chosen clear-refusal recovery action finish.
- Before Run dispatch and an autostarting layout sync, VS Code calls `agent-doc tmux ensure <relative-path> --json`. A live `agent-doc` terminal is focused or reused, a detached session is attached in a new `agent-doc` terminal rooted at the project, and a session already attached by an external client is left alone. Terminal API failures keep dispatch alive and offer the exact attach command for copying. Every CLI subprocess, and the `agent-doc` terminal the extension creates, reports `vscode.env.remoteName` through `AGENT_DOC_VSCODE_REMOTE_NAME` for terminal-host diagnostics.
- The manifest declares `"extensionKind": ["workspace"]`: on Coder, Remote-SSH or code-server the extension runs in the workspace extension host, beside the agent-doc binary, the native library and tmux. Each reliable-sync registration carries one `agent_doc_activation_receipt/v1` line with `role=workspace` (`src/activationReceipt.ts`): the loaded bundle's sha256, the native library's version and sha256, and the count of native loads as `generation` and `epoch`. A native reload, or the loaded library's digest becoming known, re-sends the registration of every open document.
- A route failure is persisted in the Agent Doc Route Failures output channel with the exact stage-specific output and surfaced as an error notification.
- An explicit send already delivered to the owning turn (`ui_outcome=steering_delivered_to_owner`) is successful even if a legacy transport wrapped the text in an error. VS Code records the route as proven and shows a concise status hint; it does not open the route-failure channel or emit an error notification.
- A successful route into an off-screen authoritative pane still includes the binary's `[route] pane-not-visible:` notice in route output, including its attach or exact reveal command. This covers an unattached session, an inactive window in an attached session, and a target hidden behind a zoomed sibling; VS Code does not mutate tmux layout to compensate.

## Session Operator Actions

- `Initialize Session` is available from the active editor, command palette, and Explorer context menu and delegates the initialize-or-claim branch to `agent-doc init-session`.
- `Show Session Status` runs `agent-doc session status <relative-path>` and surfaces the exact output in the Agent Doc Session output channel.
- `Fix Document` runs `agent-doc fix <relative-path>` from the nearest agent-doc project root.
- `Load Tmux Window` runs the explicit autostart layout sync path through Project Controller `sync_tmux_layout` with `no_autostart=false`, equivalent to full `agent-doc sync ...`, for the current visible markdown projection.
- `Clear Session Context` runs `agent-doc session clear <relative-path>` so Codex/Claude clear semantics stay aligned with the binary-owned clear command path while leaving the next `Run Agent Doc` dispatch-only reroute on the same live session. If a non-interrupting clear meets a busy active auto-loop, the binary queues exactly one deferred clear for the next proven idle boundary; repeated clears report the existing queued clear instead of injecting another `/clear`, and VS Code surfaces that output as accepted deferred work.
- `Clear Session Context` must not consult plugin-local response-status or busy flags before invoking the binary. Live pane idleness is binary-owned for status/diagnostics; stale editor-side status must not block clear.
- Protected prompt-input clear refusals show a typed warning with the relevant interrupt action, `Show status`, and `Copy details`. Legacy alive-busy refusals include `Refresh and retry`; protected prompt input does not.
- `Interrupt and clear` requires explicit VS Code confirmation and then runs `agent-doc session interrupt-clear <relative-path>`. It is the only action that intentionally discards protected prompt input.
- `Recycle Supervisor` runs `agent-doc session restart-supervisor <relative-path>` and keeps recycle ownership in the binary/supervisor path. The command ID remains `agentDoc.restartSession`. If the binary refuses because the pane is busy or the authoritative actor is still starting, VS Code shows typed restart recovery actions and the confirmed interrupt path invokes `agent-doc session restart-supervisor --force <relative-path>`.
- `Copy Session Diagnostics` runs `agent-doc session doctor <relative-path>` and copies the exact output for bug reports.
- `About Agent Doc` (`agentDoc.about`, command palette, markdown editor context menu, and the popup's More Actions) runs `agent-doc version --json` (falling back to `--version`) with the same `resolveAgentDoc()` binary as every other command, reads the already-loaded library through `native.nativeAboutSnapshot()` (`agent_doc_version()` and `agent_doc_build_info_json()`; never loads or reloads), and shows a modal message with a Copy action. The plugin version is `EDITOR_PLUGIN_VERSION`. Mismatch rules follow `editors/SPEC.md` section 6b.
- **Stale extension prompt (`#vscodeupgradelayout`):** on activation, on every
  change to its own extensions directory's `extensions.json` or `.obsolete`,
  and on `vscode.extensions.onDidChange`, the extension compares its running
  `packageJSON.version` with the agent-doc generation that manifest lists
  (directories `.obsolete` marks are skipped; an entry whose directory is gone
  is ignored). Any other installed version, including a downgrade, shows one
  warning per installed generation per extension-host lifetime. Locally it
  offers Restart Extensions (opening the extension page, where VS Code shows
  its own Restart Extensions action; VS Code has no command that loads the
  update in place, and Developer: Restart Extension Host restarts the stale
  generation) and Reload Window. A remote (`vscode.env.remoteName`, e.g. Coder
  Remote-SSH) or web host offers only Reload Window, the remedy VS Code 1.88
  documents for remote connections. `pluginUpgrade.ts` holds the pure verdict
  and prompt; `pluginUpgrade.test.ts` covers them.
- `Dashboard` (`agentDoc.dashboard`, `gvqv`) is a command-palette command with no markdown gate and popup item `[14]`. It runs `agent-doc dashboard --write` from the cleanup root (focused markdown document's project root, else the first workspace folder), then opens `markdown.showPreview` on `<root>/.agent-doc/dashboard.md`, falling back to the text editor. The project controller keeps the projection current and the preview follows its atomic rewrites; the native `agent_doc_is_session_document` classifier rejects it, so it is never registered or tab-synced. `dashboard.test.ts` covers the arguments, path, palette contribution, and popup reachability.

## Tab Sync Compatibility

- VS Code tab changes remain JetBrains-compatible for real tab/layout changes:
  the extension publishes the active file and visible `TabGroups` projection,
  including empty column placeholders, to the controller-owned editor-surface
  graph. A derived sync is exact-visible and autostart-capable under the
  binary's automatic-caller safety policy, so a missing visible pane is created
  without an explicit `Load Tmux Window`; background open files are not added
  to the desired columns.
- **Measured group layout (`#vscodeupgradelayout`, `editors/SPEC.md` § 13):**
  every layout capture (automatic surface, Run Agent Doc route, manual sync,
  rename sync, and the `--position` of Claim and Initialize Session) awaits
  `vscode.getEditorLayout` and reports evidence (the grid, the tab-group count,
  each visible editor's `viewColumn` and project markdown document, the focused
  group) to the native `editor_group_layout` derivation of
  `agent_doc_editor_derive_json`. The grid's orientation and nested sizes give
  each group's origin; groups sharing a left edge (within 1% of the editor
  width) stack top to bottom in one column, so a vertical split is one column
  and a claim position can be `top`/`bottom`. Every measured group is a column,
  including an empty placeholder. Measured columns publish with
  `column_order: editor`. A grid whose group count differs from the tab groups
  (an auxiliary editor window, whose groups `getEditorLayout` does not
  describe) or that misses a visible view column falls back to ordinal
  `viewColumn` columns with `column_order: unknown`; with no grid at all the
  ordinal columns keep their pre-measurement `editor` meaning. `groupLayout.ts`
  holds the frozen TypeScript floor for a library that cannot answer; both
  answer every case of
  `test-fixtures/editor-derivations/editor_group_layout.json`. A surface
  measurement that a newer editor edge superseded while awaiting the renderer
  is dropped by the generation guard. `tabGroups.onDidChangeTabGroups` is a
  layout trigger beside the active and visible editor changes.
- Repeated clicks between already-open split editors must continue to move tmux focus. The immediate focus handoff is deduped only against the last immediate active editor path, not against the last completed automatic sync state, so rapid A -> B -> A switches cannot be skipped by a stale debounced sync snapshot.
- Automatic sync subprocesses use a short timeout with exponential retry backoff. A slow tmux/controller repair must not cause repeated 30s automatic sync attempts while the user is only changing focus or tabs. Manual `Sync Tmux Layout` and `Load Tmux Window` use the Project Controller `sync_tmux_layout` receipt path.
- Native-library reload is one coalesced completion event. Manual `Sync Tmux Layout` waits for that event before entering the native bridge, and the reload owner republishes the current editor surface after every open CRDT replica has reattached. A tab switch in either direction during reload therefore converges after handoff instead of being dropped.

## Patch Application Safety

- VS Code must preserve the same no-replay safety boundary as JetBrains for stale visible editor state. Active-typing debounce timeouts may leave a file-watch patch queued for another idle attempt, but once an apply-proof check observes that the editor generation or text changed after patch planning, the extension must fail the payload back to binary retry accounting without scheduling a delayed replay of that same patch file.
- Lazily current is observed from open/change/save/heartbeat events. No filesystem signal participates in current-document authority or recovery.
- VS Code has no broad `save_document` or save-all command. The generation-fenced `persist_current` intent verifies the open buffer hash/byte length, calls that document's native `save()`, verifies the same bytes through the workspace filesystem, and publishes `disk_persisted` without applying an edit. If the save is deferred by VS Code's file-conflict UI, `onDidSaveTextDocument` publishes the receipt after the user-approved overwrite, provided disk, editor, and replica remain exact. A mismatch requests CRDT redelivery and never replaces the editor from disk.

## Project Controller Event Compatibility

- VS Code CRDT replica IPC must use `.agent-doc/controller.sock` with the controller `crdt_replica` envelope and include `process.pid` as `editor_pid`. That process-scoped proof lets a detached agent-doc registration establish editor authority through the relay; the controller must not require authority to pre-exist registration. It must not connect to `.agent-doc/supervisor/*.sock`.
- VS Code drains named-document CRDT deliveries from targeted `EditorIntent` events and the Lazily-backed controller subscription. It must not watch a filesystem event directory or use a fixed interval remote-update pull loop.
- A successful `replica_pull` envelope with `refused=true, reason=missing_replica` invalidates the cached pre-recycle client. VS Code must atomically re-register the stable visible cut before resuming pull/projection; it must not interpret the refusal as an idle empty delta batch. This matches JetBrains.
- VS Code reads turn-state refreshes from the Project Controller `state_subscribe` Lazily projection and mirrors the returned snapshot/delta locally. It does not read filesystem state for ordinary turn-state UI. If the Project Controller request fails, the status bar shows `agent-doc: Project Controller disconnected`; there is no compatibility fallback. Native-library reload arrives as the targeted `reload_library` editor intent. Turn-state refreshes are coalesced and use a minimum refresh interval; active-editor changes may force one immediate Project Controller refresh.
- The mirrored closeout payload's Rust-owned `realtime_steering` aggregate and
  body-identity-keyed `elements` set drive the active-turn status label/count,
  hover text, and durable directive membership. Its
  `observed_content_hash` is the controller's canonical CRDT generation
  receipt; a receipt-only empty set renders no steering label. TypeScript
  preserves that projection and must not derive steering from the visible
  buffer or disk.
- VS Code activation must not run automatic `agent-doc resync`, `resync --fix`, or a reconnect-reread scan over open buffers. Session repair/audit remains an explicit `Resync / Fix Sessions` operator action only.
- Prompt steering is Project Controller-owned. VS Code must not treat stale supervisor freshness as an editor-IPC apply/receipt/repair veto; supervisor recycle is only an explicit session action.
- `Compact Exchange` waits for native reload completion and force-refreshes the exact clean open document replica before invoking the CLI. A missing project-controller attachment is a terminal action error; the extension does not compact from a disk-only snapshot.

## External Disk Pending Parity

- Ordinary dirty editor notifications publish the host's bounded
  `contentChanges` under the captured projection epoch. A whole-buffer or clean
  cache notification is a non-operator projection that fences older queued
  deltas; reconnect and recovery never publish the full visible buffer as local
  operator input.
- Replica registration opens the controller bootstrap and projects it downstream. Dirty operator changes originate only from subsequent editor events; disk candidates are never component-merged or promoted while a controller-owned document remains attached.
- This is the same FFI and authority lifecycle as JetBrains. VS Code must not implement a private disk reread, Git fallback, or extension-local pending-response slot.

## Native String Ownership

Every C string the cdylib returns, whether as a function result or as a member of a `#[repr(C)]` result struct (`FfiPatchResult.text`/`.error`, `FfiJsonResult.json`/`.error`, `FfiProjectPath.project_root`/`.relative_path`), is a Rust allocation that must go back through `agent_doc_free_string` exactly once (`#vscodekoffisegv`):

- Koffi bindings declare those returns and struct members as the opaque `OWNED_C_STRING_POINTER` (`void*`), never `char*`/`str`. Koffi turns a `char*` value into a JS string copy, which leaks the Rust allocation and leaves nothing that can be freed. JetBrains binds the same members as JNA `Pointer`.
- The wrapper decodes the pointer with `koffi.decode(ptr, 'char', -1)` and then frees that pointer. Koffi accepts a JS string for a `void*` argument by passing its own temporary copy, so freeing a decoded string makes Rust `free()` memory it never allocated. Under code-server 4.143.1 (node v24) that crashed the extension host with SIGSEGV in `free`. Every free goes through `freeOwnedCString`, which hands only a pointer value (a BigInt under Koffi 3) to the native free. It refuses any other value and logs the refusal, accepting a leak rather than a crash.
- A regression test drives the real wrappers against the built `libagent_doc` in a child process, so a crash at this boundary fails the test instead of killing the runner.

## Editor Performance Parity

VS Code must match the JetBrains plugin's editor-hot-path discipline so neither plugin can slow the host editor:

- **Markdown-first filtering.** The visual highlighter (`scheduleRefresh`), the typing listener, and the CRDT local-change listener must short-circuit on `document.languageId !== 'markdown'` before any timer, map, or native work. Non-markdown documents must never pay per-keystroke churn for refresh scheduling, Lazily epoch reporting, or replica forwarding. (JetBrains parity: `VisualHighlighterManager` global listeners gate on markdown before scheduling.)
- **No project-wide cache invalidation on document change.** VS Code has no PSI layer, so it must not introduce an equivalent whole-workspace re-parse/re-tokenize on a per-document change. Visual tokens come from the document text via `native.visualTokens`, never from a cached workspace index that is dropped and rebuilt on each change.
- A `[#subagent-operator-request-<unique-id>]` handback marker arrives from `native.visualTokens` as `tracked_id`. VS Code MUST render it with the same tracked-ID decoration used for IDs in todo, backlog, queue, icebox, and review items.
- A queued Clear Session Context the supervisor later drops (a newer prompt started before the turn ended) arrives on the active document's authority stream as `deferred_clear_superseded`; VS Code shows one information message per dropped `clear_epoch` with a `Clear Session Context` action, using `DeferredClearSupersededNotices` (first observed projection per document seeds the baseline) — parity with JetBrains (`#clearsupersedenotice`).
- **No fixed-interval polling.** Turn state, CRDT remote delivery, tmux pane focus, and cdylib reload are event-driven Project Controller/Lazily intents. VS Code must not add a `setInterval`/`pollRemoteUpdates` loop for any of these, including tmux pane focus mirroring.

Static guards in `uiThreadBudget.test.ts` enforce the markdown-first filter, the deferred heavy work, and the absence of `setInterval`.

## Packaging

- The VSIX is universal: one package for every host. esbuild inlines every dependency into `out/extension.js` except `vscode` and the koffi FFI, which `src/native.ts` loads with `createRequire` from the extension directory. Without koffi the native library stays off and nothing registers with the Project Controller (`#vscodekoffivsix`).
- koffi 3 keeps its `.node` binaries in per-platform optional packages (`@koromix/koffi-<platform>-<arch>`), and `npm ci` installs only the build host's. `vscode:prepublish` runs `scripts/stage-koffi.mjs`, which copies the `<triplet>/koffi.node` directories of `linux-x64`, `linux-arm64`, `darwin-x64`, `darwin-arm64` and `win32-x64` (the libagent_doc release targets) into `node_modules/koffi/build/koffi/`, the bundled-prebuild root koffi's loader searches. The host's package is copied from `node_modules`; the others come from `npm pack`, and each tarball must match the `integrity` that `package-lock.json` pins. `AGENT_DOC_KOFFI_PLATFORMS` narrows the set for an offline local build.
- `.vscodeignore` ships only koffi's `package.json`, `LICENSE.txt`, CommonJS runtime (`index.cjs`, `src/koffi/index.cjs`, `src/koffi/src/static.cjs`) and the staged natives, and drops `node_modules/@koromix/**`, so the VSIX does not depend on which host built it. Per-target VSIXes (`vsce --target`) were not used: the GitHub Release and `agent-doc plugin install vscode` distribute one `agent-doc-<version>.vsix`.
- `scripts/vsixPackage.test.mjs` (`npm run test:package`) unpacks the VSIX outside the checkout and asserts koffi's runtime is present, every published platform's native is there with that platform's executable magic, no other `node_modules` content ships, and `require('koffi')` from the unpacked `out/extension.js` loads the VSIX's native and calls into the host C library. `make dev-harness-test` packages and runs it; `make install-editor-plugins` and the release workflow run it against the VSIX they built (`AGENT_DOC_VSIX=<path>`) before installing or publishing it.

## Verification Requirements

- Unit tests cover the per-document Run/Clear state machine, exact session-status display, `session clear` command wiring, interrupt-clear command wiring, busy/protected clear refusal parsing, restart refusal parsing, popup-menu parity, persistent route-failure presentation, typed save-document signal handling, Project Controller-only CRDT/event delivery, bounded turn-state refreshes, disabled full-content delivery, and absent reconnect-repair hooks. Static guards pin the JetBrains-compatible Run route flags and VS Code command contribution surface.
