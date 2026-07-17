# Editor intent transport

## Purpose

Agent-doc connects the Rust controller to editor plugins without external-file
reloads, cursor displacement, or whole-buffer replacement. The transport carries
typed intent and proof; Lazily carries the live document value.

## Authority model

```text
operator keystrokes
       |
       v
editor buffer <-> Lazily current
       ^                |
       | typed intent   | current value + causal receipts
       |                v
PID-scoped socket <-> project controller <-> state.db
       |
       v
native editor save -> disk projection -> git commit
```

- Lazily current is the only live document authority while an editor replica is
  attached.
- `state.db` owns durable intent and closeout phase.
- Disk is the persistence/commit projection.
- Recovery snapshots are cold audit/projection material, never a live input.

## Intent envelope

```json
{
  "type": "apply_canonical",
  "intent_id": "uuid",
  "cycle_id": "cycle-...",
  "file": "/absolute/path/session.md",
  "expected_generation": 42,
  "expected_current_hash": "sha256",
  "mutation": {
    "node_patches": []
  }
}
```

The accepted intent names are defined once as `EditorIntent` and mirrored
verbatim by Rust, JetBrains, and VS Code. Mutations are node-keyed or
component-keyed operations with expected source proof; replacement content is
not a transport operation.

## Receipt envelope

```json
{
  "intent_id": "uuid",
  "cycle_id": "cycle-...",
  "editor_id": "member-id",
  "phase": "replica_visible",
  "generation": 43,
  "current_hash": "sha256",
  "causal_frontier": "..."
}
```

The controller validates identity, generation, hash, and causal frontier before
advancing its monotonic state machine:

```text
IntentCaptured -> CanonicalApplied -> ReplicaAccepted -> ReplicaVisible
               -> DiskProjected -> Committed
```

Retries resume the same intent from the recorded state. Receipt replay is
idempotent; stale or future-generation receipts are rejected.

## Concurrency and rebase

Immediately before an editor mutation, the plugin rechecks Lazily current and
the native editor generation. A mismatch means the operator changed the buffer.
The plugin performs no mutation and returns the newer proof; the controller
rebases the same narrow agent intent on that current value.

This rule preserves unsaved prompts and queue deletions. It also prevents an old
response delivery from duplicating boundary/component markers after reconnect.

## Focus neutrality

The target document must already be open. Background transport may not open a
file, choose a tab, move focus, scroll, or alter layout. A missing open target is
a typed rejection, not permission to activate the document.

## Failure behavior

Timeouts, disconnects, plugin crashes, and editor restarts leave the durable
intent at its last proven phase. The keyed controller worker retries with
bounded backoff after replica registration. No filesystem inbox is scanned and
no disk fallback is attempted for an attached document.

An ABI or capability mismatch fails closed and asks for the matching plugin and
native library. There is no compatibility transport on the live path.

## Verification

SimWorld exercises all crash points, receipt reorderings, concurrent operator
edits, editor disappearance/reconnect, and duplicate retry delivery. Adapter
tests additionally prove generation rechecks, focus neutrality, exact receipt
shape, and the absence of alternate attached-document transports.
