# Agent-Doc Editor Plugin Specification

This contract is shared by JetBrains, VS Code, and future editor adapters.
Plugins are thin adapters over the editor API and Lazily. Markdown semantics,
turn state, queue state, recovery decisions, and commit ownership remain in the
Rust binary.

## 1. Core invariants

- The open editor buffer is the operator-visible current document. Lazily owns
  that live value and its causal history.
- Disk is a projection and commit surface. It is not a fallback live authority
  while an editor replica is attached.
- `state.db` is the exclusive durable authority for intent identity, expected
  generation, delivery phase, leases, captures, and exact-once closeout state.
- The binary rebases a narrow agent intent onto Lazily current. It never sends a
  stale whole-document replacement.
- A background delivery must not open, select, focus, or scroll an editor. If
  the target is not already open, the adapter rejects the intent.
- Operator edits are monotonic. An agent intent may not resurrect text that the
  operator deleted from a newer editor generation.
- Missing capabilities, an incompatible ABI, an unknown intent, or ambiguous
  structure fails closed without mutating the buffer or disk.

There is no filesystem delivery queue, live-value projection, receipt file,
plugin-owner file, queue journal, or file-signal compatibility transport.

## 2. Registration and transport

An editor replica registers over the project-controller's PID-scoped local
socket with:

```json
{
  "editor_id": "stable-process-member-id",
  "pid": 12345,
  "plugin_version": "0.2.275",
  "document": "/absolute/path/session.md",
  "generation": 42,
  "capabilities": [
    "lazily_current_v1",
    "lazily_transport_receipts_v1",
    "typed_editor_intents_v1"
  ]
}
```

Registration, current-value observation, remote CRDT delivery, ACKs, and
visibility receipts use the reliable-sync Lazily plane. Endpoint discovery is
PID-scoped so a stale listener from another editor process cannot receive a
delivery.

The native library and plugin must advertise the exact required ABI and intent
capabilities. Version skew is an explicit incompatible state; adapters do not
degrade to files or disk writes.

## 3. Shared intent vocabulary

Rust, JetBrains, and VS Code use the same `EditorIntent` names:

| Intent | Meaning |
|---|---|
| `apply_canonical` | Apply a narrow canonical mutation to Lazily current |
| `reposition` | Move the exchange boundary without changing user text |
| `save_document` | Save the already-open current buffer through the editor API |
| `refresh_content` | Republish the already-open editor value to Lazily |
| `observe_lazily_current` | Return current value, generation, and causal proof |
| `deliver_crdt_remote` | Integrate a remote Lazily change |
| `refresh_vcs` | Refresh editor VCS decoration after a durable commit |
| `reload_library` | Reload a compatible native library only when the adapter can prove a safe boundary; otherwise require process restart |

Every mutating intent carries `intent_id`, `cycle_id`, `expected_generation`,
and expected current-value proof. Unknown fields may be observed for forward
diagnostics, but an unknown intent or missing required proof is rejected.

## 4. Monotonic delivery state machine

The binary owns one durable state machine per intent:

```text
IntentCaptured
  -> CanonicalApplied
  -> ReplicaAccepted
  -> ReplicaVisible
  -> DiskProjected
  -> Committed
```

Transitions are monotonic and idempotent. A retry resumes the same `intent_id`
from its recorded phase. It does not recapture the response or create a second
delivery. `ReplicaAccepted` alone is not visible-write proof, and
`DiskProjected` alone is not commit proof.

The plugin publishes typed receipts containing the intent, editor member,
generation, current-value hash, and causal frontier. The binary advances only
when the receipt matches the expected intent and current lineage.

## 5. Apply algorithm

For `apply_canonical`, the adapter must:

1. Confirm the target markdown document is already open without changing focus.
2. Observe Lazily current plus the editor generation.
3. Wait for the bounded typing-idle condition.
4. Recheck current value and generation immediately before mutation.
5. Ask the shared native document model to apply the node-keyed/component-keyed
   intent against that exact current value.
6. Reject an expected-node or generation mismatch. The controller then rebases
   the same intent on the newer current value.
7. Apply the accepted edit as one native undoable editor command.
8. Publish accepted and visible receipts after the editor exposes the exact
   resulting current value.

The plugin does not parse markdown, merge components, normalize duplicated
scaffolds, or decide which side wins. Those decisions are shared native-model
operations so every editor implements the same semantics.

## 6. Save, reconnect, and reload

`save_document` saves the already-open buffer and publishes the resulting
Lazily current hash and disk-projection receipt. It never replaces the buffer.

On reconnect, the editor republishes its current value and generation. The
controller rebases pending intents from `state.db`; the plugin must not reread
an old delivery or replay a full document. A zero-member state is not proof of
a visible write.

`reload_library` is accepted only by an adapter that can quiesce and unload all
old native calls before loading the announced ABI. Such an adapter re-registers
capabilities, preserves the same Lazily replica, and does not change the active
document or editor focus. An adapter that cannot prove that boundary must retain
its one loaded native generation and report that a process restart is required.
Unknown adapter identities fail closed to restart-required.

## 7. User actions and routing

Submit, claim, compact, and sync actions invoke the resolved `agent-doc` binary
and route through the controller. Rapid duplicate actions are coalesced by
document/cycle identity. A busy harness pane is never injected into until the
harness-specific idle-prompt proof is present; modal UI and online artifacts
are busy states.

Plugins report layout and document membership without changing layout. Hidden
or stashed panes remain registered, while background document delivery remains
focus-neutral.

## 8. Error handling

- Log every rejection with intent id, document, expected/current generation,
  editor member, and reason.
- Keep failures visible in the editor; never silently acknowledge them.
- Never treat timeout, socket disappearance, file save, or process exit as an
  inferred delivery ACK.
- Recovery retries are bounded and resume the durable state-machine phase.
- Structural ambiguity and two-sided concurrent edits are rebase inputs, not a
  reason to elect disk or replace the buffer.

## 9. Required tests

Each adapter must cover:

1. Current-generation apply and exact accepted/visible receipts.
2. Typing between observation and apply rejects with no mutation.
3. Operator deletion followed by retry does not resurrect deleted queue text.
4. Reconnect resumes one intent exactly once.
5. Duplicate/out-of-order receipts cannot regress state.
6. No attached-editor path reads disk as current authority.
7. Background delivery never opens, focuses, selects, or scrolls a document.
8. Missing ABI/capability fails closed without fallback.
9. Save projects the existing buffer and does not replace it.
10. Crash points at every state-machine transition converge under simulation.
