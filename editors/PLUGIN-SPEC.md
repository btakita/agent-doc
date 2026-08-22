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
- Ordinary editor-origin mutations are the host event's bounded causal splices.
  The adapter preserves splice boundaries through the CRDT/tree ingress and
  never manufactures operator input by diffing a later whole-buffer read
  against an adapter shadow. A complete buffer is admitted only as a typed
  bootstrap, checkpoint, or explicit recovery observation.
- A clean cache reload, whole-text replacement, remote projection, or native
  generation handoff advances the non-operator epoch and fences older queued
  splices even when the host reports the reload as an incremental range edit.
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

Registration, current-value observation, remote CRDT delivery, and visible
state projections use the reliable-sync Lazily plane. Endpoint discovery is
PID-scoped so a stale listener from another editor process cannot receive a
delivery.

Editor adapters must bridge host events that change visible document membership
into the controller-owned editor-surface source. A document-open event may
advance the per-document Lazily/CRDT value before the host finishes restoring
its split layout; the adapter must still publish the completed visible surface.
This bridge reports observed editor state only. It never plans or executes a
tmux mutation locally.

The native library and plugin must advertise the exact required ABI and intent
capabilities. Version skew is an explicit incompatible state; adapters do not
degrade to files or disk writes.

## 3. Shared intent vocabulary

Rust, JetBrains, and VS Code use the same `EditorIntent` names:

| Intent | Meaning |
|---|---|
| `apply_canonical` | Apply a narrow canonical mutation to Lazily current |
| `reposition` | Move the exchange boundary without changing user text |
| `refresh_content` | Republish the already-open editor value to Lazily |
| `observe_lazily_current` | Legacy compatibility input only; current runtimes observe the Lazily projection and emit no request |
| `persist_current` | Save the exact already-visible revision through the editor's native save lifecycle; reject a hash/length mismatch without replacing the buffer |
| `deliver_crdt_remote` | Integrate a remote Lazily change |
| `refresh_vcs` | Refresh VCS decoration for the required absolute `file` after a durable commit |
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

## 6. Persistence, reconnect, and reload

Persistence is a state projection with one narrowly typed editor effect. The
plugin continuously publishes visible CRDT state. When closeout is blocked only
on disk, the controller may issue `persist_current` with the exact visible hash
and byte length. The adapter rechecks its current buffer, performs the host's
native save without changing that buffer, verifies the matching disk projection,
and publishes `disk_persisted`. A raced editor revision rejects the request and
returns to ordinary CRDT delivery; disk, Git, and retained snapshots never become
replacement authority.

On reconnect, the editor republishes its current value and generation. The
controller rebases pending intents from `state.db`; the plugin must not reread
an old delivery or replay a full document. A zero-member state is not proof of
a visible write.

`reload_library` is accepted only by an adapter that can quiesce and join every
old native worker, drain calls, terminate any generation-owned call thread,
close the old library handle, and prove the old mapping is absent before loading
the announced ABI. Such an adapter re-registers capabilities, preserves the
same Lazily replica, and does not change the active document or editor focus.
An adapter that cannot prove that boundary must load no replacement and report
that a process restart is required.
Unknown adapter identities fail closed to restart-required.

Editor adapters must keep lifecycle and refresh work bounded:

- `refresh_vcs` is file-scoped. JetBrains dirties that file only; VS Code asks
  only the containing repository to refresh status. A workspace-wide VFS or
  VCS refresh is forbidden.
- Native reload stops inbound callbacks before disposing the CRDT managers
  they can call. A failed listener quiesce must not rebuild every replica from
  a half-disposed generation.
- Native callback threads never wait indefinitely for an editor read permit.
  Capture editor-owned objects on the editor thread without blocking the
  caller, then continue native, socket, and CRDT work on a pooled executor.
- Layout observation uses one project/process-scoped listener filtered to the
  active editor split tree; it does not recursively install a listener on every
  Swing container.
- Endpoint discovery is demand-driven by the project root and open Markdown
  files. Dormant nested `.agent-doc` roots do not each receive a listener at
  project startup.

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
- Keep failures visible in the editor; never silently accept them.
- Never treat timeout, socket disappearance, file save, or process exit as an
  inferred visible-state projection.
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
11. Refresh, reload, layout, and endpoint discovery stay within the bounded
    scope above and never block the editor event thread on native work.
