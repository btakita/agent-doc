# Agent-doc Command Payloads (`command-plane-v1`)

Domain payload contracts for the lazily command/RPC message plane
(`command-plane-v1`). Lazily owns the **envelope** (`CommandSubmit` /
`CommandCancel` / `CommandEvents` / `CommandProjection`, correlation ids,
idempotency, generation guards, causal receipts); agent-doc owns the **payloads**
carried inside `CommandSubmit.payload`. Lazily never interprets a payload body.

See the lazily envelope in `lazily-spec` — `protocol.md` § "Command / RPC
Message Plane" and the schema `schemas/message-passing.json`. Terminal proof for
every command folds through a `CausalReceipt` (`schemas/receipts.json`);
`observed` / `accepted` / `started` events and any transport ACK are progress
only.

## Envelope binding

Each agent-doc command is a `CommandSubmit` with:

- `namespace` = `"agent-doc"`
- `name` = the command name (e.g. `"editor_route"`)
- `payload_type` = `"agent-doc.<name>.v<major>"` (e.g. `"agent-doc.editor_route.v1"`)
- `payload` = an `IpcValue` (lazily `defs.json`) — `Inline` UTF-8 JSON bytes of
  the payload object below, or a `SharedBlob` reference to the same bytes for
  large payloads
- `payload_hash` = `"sha256:<hex>"` over the exact payload bytes
- `authority_generation` = the controller generation the caller last observed
- `idempotency_key` = the dedupe key named per command below

A submit whose `payload_type` the controller does not recognize is rejected
(terminal `rejected` receipt, reason `unknown_payload_type`) — it is never
silently applied.

## Command → payload map

The first-party plugin command surface. Interim path is the current transport;
target path is the lazily command envelope.

| Command | `name` | `payload_type` | Interim path |
|---|---|---|---|
| Run Agent Doc | `editor_route` | `agent-doc.editor_route.v1` | JB `CpRouteClient`; VS Code raw `controller.sock` `editor_route` |
| Sync Tmux Layout / Load Window | `sync_tmux_layout` | `agent-doc.sync_tmux_layout.v1` | JB `CpRouteClient` submits `editor_command_submit_async`; legacy native endpoint remains available for older plugins |
| Focus handoff | `focus_document_pane` | `agent-doc.focus_document_pane.v1` | JB `CpRouteClient` submits `editor_command_submit_async`; legacy native endpoint remains available for older plugins |
| Save document | `save_document` | `agent-doc.save_document.v1` | file signal / socket IPC |
| Session status/clear/restart/doctor | `session_command` | `agent-doc.session_command.v1` | editor-spawned CLI |
| CRDT replica register/update/pull/ack | `crdt_replica` | `agent-doc.crdt_replica.v1` | controller `crdt_replica` custom envelope |

## Payload schemas

All payloads are JSON objects. Absent optional fields default as noted.

### `agent-doc.editor_route.v1`

Idempotency key: `editor_attempt_id` when present, otherwise the stable
`"<project-root>:<relative-path>:run"` route key. Transport retries of one
editor action therefore coalesce, while a later intentional action has a new
attempt identity. The controller separately coalesces any action while the
document generation still has an unconsumed dispatch receipt.

| Field | Type | Notes |
|---|---|---|
| `file` | string | Absolute path to the `.md` document |
| `relative_path` | string | Path relative to the project root |
| `dispatch_only` | boolean | If true, dispatch without waiting for a terminal route result |
| `plain_trigger` | boolean | Send the plain `agent-doc <FILE>` reopen; do not restart the session on a `/clear` head |
| `wait_budget_ms` | integer ≥ 0 | Route wait budget; `0` means no wait |
| `layout_args` | array of string | Optional tmux layout args validated by the controller |
| `route_key` | string | Stable route key for dedupe/diagnostics |
| `editor_attempt_id` | string \| null | Editor's attempt id for route-snapshot correlation |

Maps to the controller's existing `ControllerEditorRoutePayload`; the command
envelope's `command_id` becomes the route's causation id, and the controller
emits `observed` → `accepted` → `started` events plus a terminal `applied` /
`rejected` receipt.

### `agent-doc.sync_tmux_layout.v1`

Idempotency key: `"<project-root>:sync"`.

| Field | Type | Notes |
|---|---|---|
| `project_root` | string | Project root |
| `columns` | array of string | Comma-joined column strings, matching repeated `agent-doc sync --col` values |
| `window` | string \| null | Optional target tmux window |
| `focus` | string \| null | Focused document path after sync |
| `exact_visible` | boolean | Require the exact visible layout |
| `no_autostart` | boolean | Do not autostart a session |
| `caller_kind` | string | `"claim"`, `"run"`, `"automatic"`, or `"manual"` |

### `agent-doc.focus_document_pane.v1`

Idempotency key: `"<project-root>:<document-path>:focus"`.

| Field | Type | Notes |
|---|---|---|
| `document_path` | string | Document to focus |
| `project_root` | string | Project root |
| `no_promotion` | boolean | Do not promote the pane to controller |
| `active_window_guard` | boolean | Only focus if the target window is active |

### `agent-doc.save_document.v1`

Idempotency key: `"<document-path>:save:<patch_id>"`.

| Field | Type | Notes |
|---|---|---|
| `document_path` | string | Document to save |
| `patch_id` | string | Patch id being persisted |
| `expected_generation` | integer ≥ 0 | Generation the caller expects on disk |
| `expected_hash` | string \| null | Expected content hash before write (fail-closed guard) |

### `agent-doc.session_command.v1`

Idempotency key: `"<document-path>:session:<subcommand>"`.

| Field | Type | Notes |
|---|---|---|
| `subcommand` | string | `"status"`, `"clear"`, `"restart"`, or `"doctor"` |
| `force` | boolean | Force through a busy/guarded state |
| `interrupt` | boolean | Interrupt an in-flight turn |
| `document_path` | string | Target document |
| `display_policy` | string | `"inline"`, `"notification"`, or `"silent"` |

Session output copied into `CommandEvents.detail` is diagnostics only; the
terminal outcome is still a causal receipt.

### `agent-doc.crdt_replica.v1`

Idempotency key: the replica op's own id.

Wraps the existing `crdt_replica` op body (register / update / pull / ack); a
delta pull carries `expected_content_hash`, and current clients return the
visible editor buffer's SHA-256 as `content_hash` in the ACK. A generation ACK
whose content hash differs remains pending and requests canonical re-bootstrap;
it cannot open the disk-materialization barrier. The command envelope adds the
command id, generation guard, and
receipt projection so replica traffic shares the same reconnect story. The
existing `Snapshot` / `Delta` / `CrdtSync` state plane is not replaced — this is
the additive command sibling.

## Rollout note

These payloads are additive. Until a plugin advertises `command-plane-v1`, its
commands keep their interim paths; a command that requires the plane fails closed
with a stale-plugin/update warning rather than downgrading silently. Controller
wiring (the shadow endpoint that decodes these payloads and dispatches the
existing code paths) is tracked in the agent-loop plan
`plan-lazily-message-passing-editor-cp.md` (`#lzmsgpcp`), Phase 6.
