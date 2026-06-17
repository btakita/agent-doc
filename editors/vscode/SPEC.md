# VS Code Plugin Specification

Extends `editors/SPEC.md` with VS Code-specific behavior.

## Plugin Metadata

- **ID:** `btakita.agent-doc`
- **Name:** Agent Doc
- **Activation:** Markdown documents

## Run Feedback

- `Run Agent Doc` saves the active markdown document and dispatches through `agent-doc route --dispatch-only <relative-path>` from the document's nearest agent-doc project root.
- Repeating `Run Agent Doc` while an older plugin-spawned route process is still alive must not cancel the older process. The first route process owns the submit/proof window; later clicks for the same document are visibly deduped.
- `Run Agent Doc` and `Clear Session Context` are serialized per document in the VS Code extension layer before either command reaches the binary. Repeated Run clicks dedupe behind the first alive route process, but `Clear Session Context` preempts a still-dispatching Run by cancelling the plugin-spawned route process and then running the normal binary-owned `agent-doc session clear <relative-path>` path. If Run is clicked while a normal clear command is already running, the latest Run intent is queued and starts only after the clear command and any chosen clear-refusal recovery action finish.
- A route failure is persisted in the Agent Doc Route Failures output channel with the exact stage-specific output and surfaced as an error notification.

## Session Operator Actions

- `Show Session Status` runs `agent-doc session status <relative-path>` and surfaces the exact output in the Agent Doc Session output channel.
- `Clear Session Context` runs `agent-doc session clear <relative-path>` so Codex/Claude clear semantics stay aligned with the binary-owned clear command path while leaving the next `Run Agent Doc` dispatch-only reroute on the same live session.
- `Clear Session Context` must not consult plugin-local response-status or busy flags before invoking the binary. Live pane idleness is binary-owned for status/diagnostics; stale editor-side status must not block clear.
- Protected prompt-input clear refusals show a typed warning with the relevant interrupt action, `Show status`, and `Copy details`. Legacy alive-busy refusals include `Refresh and retry`; protected prompt input does not.
- `Interrupt and clear` requires explicit VS Code confirmation and then runs `agent-doc session interrupt-clear <relative-path>`. It is the only action that intentionally discards protected prompt input.
- `Restart Supervisor Process` runs `agent-doc session restart-supervisor <relative-path>` and keeps restart ownership in the binary/supervisor path. If the binary refuses because the pane is busy or the authoritative actor is still starting, VS Code shows typed restart recovery actions and the confirmed interrupt path invokes `agent-doc session restart-supervisor --force <relative-path>`.
- `Copy Session Diagnostics` runs `agent-doc session doctor <relative-path>` and copies the exact output for bug reports.

## Verification Requirements

- Unit tests cover the per-document Run/Clear state machine, exact session-status display, `session clear` command wiring, interrupt-clear command wiring, busy/protected clear refusal parsing, restart refusal parsing, and persistent route-failure presentation.
