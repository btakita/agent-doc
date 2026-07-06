# VS Code Plugin Specification

Extends `editors/SPEC.md` with VS Code-specific behavior.

## Plugin Metadata

- **ID:** `btakita.agent-doc`
- **Name:** Agent Doc
- **Activation:** Markdown documents

## Run Feedback

- `Run Agent Doc` saves the active markdown document and dispatches through `agent-doc route --dispatch-only --plain-trigger --debounce 0 --wait-for-ready 120 <relative-path>` from the document's nearest agent-doc project root, including `--col` and `--focus` layout arguments for the visible markdown projection.
- Repeating `Run Agent Doc` while an older plugin-spawned route process is still alive must not cancel the older process. The first route process owns the submit/proof window; later clicks for the same document are visibly deduped.
- `Run Agent Doc` and `Clear Session Context` are serialized per document in the VS Code extension layer before either command reaches the binary. Repeated Run clicks dedupe behind the first alive route process, but `Clear Session Context` preempts a still-dispatching Run by cancelling the plugin-spawned route process and then running the normal binary-owned `agent-doc session clear <relative-path>` path. If Run is clicked while a normal clear command is already running, the latest Run intent is queued and starts only after the clear command and any chosen clear-refusal recovery action finish.
- A route failure is persisted in the Agent Doc Route Failures output channel with the exact stage-specific output and surfaced as an error notification.

## Session Operator Actions

- `Show Session Status` runs `agent-doc session status <relative-path>` and surfaces the exact output in the Agent Doc Session output channel.
- `Fix Document` runs `agent-doc fix <relative-path>` from the nearest agent-doc project root.
- `Load Tmux Window` runs the explicit autostart layout sync path, equivalent to `agent-doc sync ...` without `--no-autostart`, for the current visible markdown projection.
- `Clear Session Context` runs `agent-doc session clear <relative-path>` so Codex/Claude clear semantics stay aligned with the binary-owned clear command path while leaving the next `Run Agent Doc` dispatch-only reroute on the same live session. If a non-interrupting clear meets a busy active auto-loop, the binary queues exactly one deferred clear for the next proven idle boundary; repeated clears report the existing queued clear instead of injecting another `/clear`, and VS Code surfaces that output as accepted deferred work.
- `Clear Session Context` must not consult plugin-local response-status or busy flags before invoking the binary. Live pane idleness is binary-owned for status/diagnostics; stale editor-side status must not block clear.
- Protected prompt-input clear refusals show a typed warning with the relevant interrupt action, `Show status`, and `Copy details`. Legacy alive-busy refusals include `Refresh and retry`; protected prompt input does not.
- `Interrupt and clear` requires explicit VS Code confirmation and then runs `agent-doc session interrupt-clear <relative-path>`. It is the only action that intentionally discards protected prompt input.
- `Recycle Supervisor` runs `agent-doc session restart-supervisor <relative-path>` and keeps recycle ownership in the binary/supervisor path. The command ID remains `agentDoc.restartSession`. If the binary refuses because the pane is busy or the authoritative actor is still starting, VS Code shows typed restart recovery actions and the confirmed interrupt path invokes `agent-doc session restart-supervisor --force <relative-path>`.
- `Copy Session Diagnostics` runs `agent-doc session doctor <relative-path>` and copies the exact output for bug reports.

## Tab Sync Compatibility

- VS Code tab changes remain JetBrains-compatible for real tab/layout changes: the extension issues the immediate best-effort `agent-doc focus <file>` handoff and then keeps the debounced reconciliation on `agent-doc sync --no-autostart --exact-visible ...` with the visible `TabGroups` projection, including empty column placeholders.
- Automatic sync subprocesses use a short timeout with exponential retry backoff. A slow tmux/controller repair must not cause repeated 30s automatic sync attempts while the user is only changing focus or tabs. Manual `Sync Tmux Layout` and `Load Tmux Window` keep the longer manual timeout.

## Patch Application Safety

- VS Code must preserve the same no-replay safety boundary as JetBrains for stale visible editor state. Active-typing debounce timeouts may leave a file-watch patch queued for another idle attempt, but once an apply-proof check observes that the editor generation or text changed after patch planning, the extension must fail the payload back to binary retry accounting without scheduling a delayed replay of that same patch file.
- VS Code consumes `.agent-doc/patches/save-document.signal` as the file-IPC equivalent of JetBrains `save_document` socket messages. The handler must require an already-open markdown document, wait for typing idle, call `TextDocument.save()`, and publish the saved text through `agent_doc_editor_content_applied_for_editor_v1` with `lazily_transport_receipts_v1` for the supplied `patch_id`. It must not write `.agent-doc/ack-content`, open a closed document as proof of a live buffer, or use this path for full-document replacement or reconnect reread repair. Missing lazily receipt support is an incompatible plugin/native-library version error.

## CPC Event Compatibility

- VS Code CRDT replica IPC must use `.agent-doc/controller.sock` with the controller `crdt_replica` envelope. It must not connect to `.agent-doc/supervisor/*.sock`.
- VS Code watches `.agent-doc/crdt-replica-events/*.json` and drains the named document's pending CRDT deliveries from the controller. It must not use a fixed interval remote-update pull loop.
- VS Code watches `.agent-doc/turn-scope/*.json` for turn-state refreshes and the global cdylib reload-broadcast file for native reloads. These paths are event-driven; no fallback status/reload polling interval is allowed. Turn-state refreshes are coalesced, use a minimum refresh interval, and apply slow-projection backoff; active-editor changes may force one immediate refresh.
- VS Code activation must not run automatic `agent-doc resync`, `resync --fix`, or a reconnect-reread scan over open buffers. Session repair/audit remains an explicit `Resync / Fix Sessions` operator action only.
- Prompt steering is CPC-owned. VS Code must not treat stale supervisor freshness as an editor-IPC apply/receipt/repair veto; supervisor recycle is only an explicit session action.

## Verification Requirements

- Unit tests cover the per-document Run/Clear state machine, exact session-status display, `session clear` command wiring, interrupt-clear command wiring, busy/protected clear refusal parsing, restart refusal parsing, popup-menu parity, persistent route-failure presentation, typed save-document signal handling, CPC-only CRDT/event delivery, bounded turn-state refreshes, disabled full-content delivery, and absent reconnect-repair hooks. Static guards pin the JetBrains-compatible Run route flags and VS Code command contribution surface.
