# VS Code Plugin Specification

Extends `editors/SPEC.md` with VS Code-specific behavior.

## Plugin Metadata

- **ID:** `btakita.agent-doc`
- **Name:** Agent Doc
- **Activation:** Markdown documents

## Run Feedback

- `Run Agent Doc` saves the active markdown document, ensures the Project Controller is running, and dispatches a Project Controller `editor_route` request from the document's nearest agent-doc project root. The request carries the dispatch-only plain-trigger route payload, a 120-second ready wait, and `--col` / `--focus` layout arguments for the visible markdown projection.
- Repeating `Run Agent Doc` while an older `editor_route` request is still in flight must not cancel the older request. The first request owns the submit/proof window; later clicks for the same document are visibly deduped.
- `Run Agent Doc` and `Clear Session Context` are serialized per document in the VS Code extension layer before route/clear work is submitted. Repeated Run clicks dedupe behind the first alive `editor_route` request, but `Clear Session Context` preempts a still-dispatching Run by cancelling the in-flight Project Controller request and then running the normal binary-owned `agent-doc session clear <relative-path>` path. If Run is clicked while a normal clear command is already running, the latest Run intent is queued and starts only after the clear command and any chosen clear-refusal recovery action finish.
- A route failure is persisted in the Agent Doc Route Failures output channel with the exact stage-specific output and surfaced as an error notification.

## Session Operator Actions

- `Show Session Status` runs `agent-doc session status <relative-path>` and surfaces the exact output in the Agent Doc Session output channel.
- `Fix Document` runs `agent-doc fix <relative-path>` from the nearest agent-doc project root.
- `Load Tmux Window` runs the explicit autostart layout sync path through Project Controller `sync_tmux_layout` with `no_autostart=false`, equivalent to full `agent-doc sync ...`, for the current visible markdown projection.
- `Clear Session Context` runs `agent-doc session clear <relative-path>` so Codex/Claude clear semantics stay aligned with the binary-owned clear command path while leaving the next `Run Agent Doc` dispatch-only reroute on the same live session. If a non-interrupting clear meets a busy active auto-loop, the binary queues exactly one deferred clear for the next proven idle boundary; repeated clears report the existing queued clear instead of injecting another `/clear`, and VS Code surfaces that output as accepted deferred work.
- `Clear Session Context` must not consult plugin-local response-status or busy flags before invoking the binary. Live pane idleness is binary-owned for status/diagnostics; stale editor-side status must not block clear.
- Protected prompt-input clear refusals show a typed warning with the relevant interrupt action, `Show status`, and `Copy details`. Legacy alive-busy refusals include `Refresh and retry`; protected prompt input does not.
- `Interrupt and clear` requires explicit VS Code confirmation and then runs `agent-doc session interrupt-clear <relative-path>`. It is the only action that intentionally discards protected prompt input.
- `Recycle Supervisor` runs `agent-doc session restart-supervisor <relative-path>` and keeps recycle ownership in the binary/supervisor path. The command ID remains `agentDoc.restartSession`. If the binary refuses because the pane is busy or the authoritative actor is still starting, VS Code shows typed restart recovery actions and the confirmed interrupt path invokes `agent-doc session restart-supervisor --force <relative-path>`.
- `Copy Session Diagnostics` runs `agent-doc session doctor <relative-path>` and copies the exact output for bug reports.

## Tab Sync Compatibility

- VS Code tab changes remain JetBrains-compatible for real tab/layout changes: the extension issues the immediate best-effort Project Controller `focus_document_pane` handoff from the active-editor event before tab-sync planner dedup, then keeps the debounced reconciliation on the passive `sync --no-autostart --exact-visible ...` path with the visible `TabGroups` projection, including empty column placeholders.
- Repeated clicks between already-open split editors must continue to move tmux focus. The immediate focus handoff is deduped only against the last immediate active editor path, not against the last completed automatic sync state, so rapid A -> B -> A switches cannot be skipped by a stale debounced sync snapshot.
- Automatic sync subprocesses use a short timeout with exponential retry backoff. A slow tmux/controller repair must not cause repeated 30s automatic sync attempts while the user is only changing focus or tabs. Manual `Sync Tmux Layout` and `Load Tmux Window` use the Project Controller `sync_tmux_layout` receipt path.

## Patch Application Safety

- VS Code must preserve the same no-replay safety boundary as JetBrains for stale visible editor state. Active-typing debounce timeouts may leave a file-watch patch queued for another idle attempt, but once an apply-proof check observes that the editor generation or text changed after patch planning, the extension must fail the payload back to binary retry accounting without scheduling a delayed replay of that same patch file.
- Lazily current is observed from open/change/save/heartbeat events. No filesystem signal participates in current-document authority or recovery.
- VS Code consumes typed `save_document` messages from its PID-scoped editor socket. The handler must require an already-open markdown document, wait for typing idle, call `TextDocument.save()`, and publish the saved text through `agent_doc_editor_content_applied_for_editor_v1` with `lazily_transport_receipts_v1` for the supplied `patch_id`. It records `missing_file`, `missing_document`, `saved`, and `failed` through the shared `agent_doc_record_editor_surface_event` FFI schema. It must not write `.agent-doc/ack-content`, open a closed document as proof of a live buffer, or use this path for full-document replacement or reconnect reread repair. Missing lazily receipt support is an incompatible plugin/native-library version error.

## Project Controller Event Compatibility

- VS Code CRDT replica IPC must use `.agent-doc/controller.sock` with the controller `crdt_replica` envelope. It must not connect to `.agent-doc/supervisor/*.sock`.
- VS Code drains named-document CRDT deliveries from targeted `EditorIntent` events and the Lazily-backed controller subscription. It must not watch a filesystem event directory or use a fixed interval remote-update pull loop.
- VS Code reads turn-state refreshes from the Project Controller `state_subscribe` Lazily projection and mirrors the returned snapshot/delta locally. It does not read filesystem state for ordinary turn-state UI. If the Project Controller request fails, the status bar shows `agent-doc: Project Controller disconnected`; there is no compatibility fallback. Native-library reload arrives as the targeted `reload_library` editor intent. Turn-state refreshes are coalesced and use a minimum refresh interval; active-editor changes may force one immediate Project Controller refresh.
- The mirrored closeout payload's Rust-owned `realtime_steering` aggregate drives
  the active-turn status label/count and hover text; TypeScript must not derive
  steering from the visible buffer or disk.
- VS Code activation must not run automatic `agent-doc resync`, `resync --fix`, or a reconnect-reread scan over open buffers. Session repair/audit remains an explicit `Resync / Fix Sessions` operator action only.
- Prompt steering is Project Controller-owned. VS Code must not treat stale supervisor freshness as an editor-IPC apply/receipt/repair veto; supervisor recycle is only an explicit session action.

## External Disk Pending Parity

- A whole-buffer editor notification is reduced to its causal text delta against the retained shadow; reconnect and recovery never publish the full visible buffer.
- Replica registration opens the controller bootstrap and projects it downstream. Dirty operator changes originate only from subsequent editor events; disk candidates are never component-merged or promoted while a controller-owned document remains attached.
- This is the same FFI and authority lifecycle as JetBrains. VS Code must not implement a private disk reread, Git fallback, or extension-local pending-response slot.

## Editor Performance Parity

VS Code must match the JetBrains plugin's editor-hot-path discipline so neither plugin can slow the host editor:

- **Markdown-first filtering.** The visual highlighter (`scheduleRefresh`), the typing listener, and the CRDT local-change listener must short-circuit on `document.languageId !== 'markdown'` before any timer, map, or native work. Non-markdown documents must never pay per-keystroke churn for refresh scheduling, Lazily epoch reporting, or replica forwarding. (JetBrains parity: `VisualHighlighterManager` global listeners gate on markdown before scheduling.)
- **No project-wide cache invalidation on document change.** VS Code has no PSI layer, so it must not introduce an equivalent whole-workspace re-parse/re-tokenize on a per-document change. Visual tokens come from the document text via `native.visualTokens`, never from a cached workspace index that is dropped and rebuilt on each change.
- **No fixed-interval polling.** Turn state, CRDT remote delivery, tmux pane focus, and cdylib reload are event-driven Project Controller/Lazily intents. VS Code must not add a `setInterval`/`pollRemoteUpdates` loop for any of these, including tmux pane focus mirroring.

Static guards in `uiThreadBudget.test.ts` enforce the markdown-first filter, the deferred heavy work, and the absence of `setInterval`.

## Verification Requirements

- Unit tests cover the per-document Run/Clear state machine, exact session-status display, `session clear` command wiring, interrupt-clear command wiring, busy/protected clear refusal parsing, restart refusal parsing, popup-menu parity, persistent route-failure presentation, typed save-document signal handling, Project Controller-only CRDT/event delivery, bounded turn-state refreshes, disabled full-content delivery, and absent reconnect-repair hooks. Static guards pin the JetBrains-compatible Run route flags and VS Code command contribution surface.
