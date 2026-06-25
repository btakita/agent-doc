# Plan: Plugin/binary merge-control state machine (`#mergestatemachine`)

## Operator ask

> I'm still seeing JB `File Cache Conflict` prompts. Do we need plugin/binary
> state machine(s) to control the merges to ensure disk writes don't happen while
> the editor/FFI plugin is running?

**Yes.** The `File Cache Conflict` dialogs and the relentless IPC
`no_ack` / `recovery=retry_without_disk_write` / forced `--force-disk` churn
(seen on nearly every closeout this session) are the same failure: a **write race**
between the binary's disk-write path and the editor/FFI plugin's buffer ownership.

## Root structure today

Write ownership is implied by scattered checks, not a single authoritative state:

- `try_ipc` writes a patch file and waits for the plugin to apply+ACK; on timeout
  it `retry_without_disk_write` (fail-closed) — but there is no explicit model of
  "does a live editor currently own this buffer?"
- `commit` logs `skipping working-tree boundary reposition — IPC listener active`
  — an ad-hoc guard, not a state.
- The known `#6b5h` wedge (editor-IPC listener registered but no live editor)
  proves the ambiguity: the binary cannot tell "editor owns buffer" from "stale
  listener," so it either wedges (no_ack) or races (File Cache Conflict).

## Proposal — explicit write-ownership state machine (FFI-owned)

Per the Shared Foundation pattern, the FFI library owns a write-ownership SM that
both the binary and every editor plugin transition through:

```
Detached ─▶ Attached ─▶ EditorOwnsBuffer ─▶ BinaryWriteRequested
   ▲                          │                     │
   │                          │                     ▼
   └────────── Committed ◀── IpcAckProven ◀── (editor applies patch + ACK)
```

- A disk write is **gated on a proven ownership transition**: the binary may
  write to disk only in `Detached` (no editor) or after `IpcAckProven` (editor
  applied the patch). It can never write while `EditorOwnsBuffer` holds → no
  `File Cache Conflict`.
- `#6b5h` (listener registered, no live editor) becomes a detectable state:
  `Attached` without a heartbeat within N → demote to `Detached` → route the
  write to the controller-host disk path instead of wedging on `no_ack`.
- The wait for `IpcAckProven` is bounded by the **`#waitmachine` `AwaitingIpcAck`
  state** (≤10s hard deadline) — the two state machines compose: merge-control
  owns *ownership*, wait-machine owns *deadline*.

## Phases

1. **`#mergestatemachine1`** — define the FFI write-ownership SM + a liveness probe
   (editor heartbeat) that distinguishes `EditorOwnsBuffer` from a stale listener
   (`#6b5h`). Pure transition fn + exhaustive tests.
2. **`#mergestatemachine2`** — gate `try_ipc` / `commit` / disk-write on the SM;
   route `Detached` (incl. demoted-stale) writes to the controller-host disk path
   so a CLI-only / no-editor session never wedges on `no_ack`.
3. **`#mergestatemachine3`** — JB + VSCode plugins report attach/detach/ownership
   through the SM (thin reporters via `#lazilystatesync`); SimWorld + a live
   editor-attached/detached repro for `File Cache Conflict` and the `#6b5h` wedge.

## Relationship to other work

Composes with `#waitmachine` (bounded `AwaitingIpcAck`), `#queuestatemachine`
(queue convergence), and `#lazilystatesync` (plugins as thin reactive reporters).
Shared state-machine backbone on the lazily-rs reactive substrate.
