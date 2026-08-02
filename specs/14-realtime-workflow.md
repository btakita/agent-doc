# Real-Time Workflow Authority

## Monotonic response transactions

Lazily owns both operator edits and unsealed agent response checkpoints. A
checkpoint is a cumulative semantic cell, not a whole-document candidate and not
a live-buffer sidecar. A newer checkpoint supersedes only response nodes absent
from the committed exchange anchor; operator prompt nodes and unsaved queue
deletions remain authoritative.

Standalone conclusions may cross the write boundary earlier as a distinct
cycle-scoped salient-response node. The agent declares salience; the binary only
validates structural completeness and upserts through controller/CRDT
authority. The node is explicitly non-final, has no queue/progress answer
authority, and is deterministically replaced or removed by later salient/final
mutations.

The explicit close of the last editor replica first publishes the exact final
buffer cut, then releases the replica. If no replica remains, the binary projects
that retained cut to disk; zero-member delivery alone is never visible-write
proof. A reconnect/replay transient containing duplicate response cells or
standalone protocol boundaries is normalized through Lazily before the generic
integrity gate. Malformed component structure and inline boundary text continue
to fail closed.

This spec is the canonical workflow authority for live session documents. It is
implementation-independent: editor plugins, CRDT transports, IPC payloads,
snapshots, harness hooks, and direct CLI calls may change only if they preserve
these rules or the change is explicitly discussed and accepted.

This is the realtime state machine. For the separate turn/closeout lifecycle
that consumes verified realtime handoffs and owns commits, see
[Turn Lifecycle Authority](15-turn-lifecycle.md).

## Core Invariant

The operator-visible document state is authoritative for every
operator-authored change. An operator change is any text, whitespace, component
body, frontmatter edit, queue/backlog edit, prompt, comment, partial word, or
plugin-defined component content that appears in the editor or visible file and
was not produced by the current binary-owned write.

Captured agent responses are durable `state.db` intents. File snapshots and
`state.db` CRDT checkpoints are cold recovery projections. Lazily receipts are
transition proof. None of these is a
second current-document candidate or authority to delete/reset operator text.

Snapshots are durable backup/audit state, not hot-path authority. Until every
write path carries a structured operation record, a snapshot may temporarily
provide a bounded agent-response candidate, but the hot path must still merge
that candidate into the latest source-of-truth document. A snapshot must never
be load-bearing for deciding what operator-visible state should survive.

## Prompt-to-response acknowledgement

`agent-doc-diff` owns the policy that associates an operator prompt with an
answering response cell (`#prompt-response-adjacency`). Its sources are the
admitted baseline, the ordered unsuppressed diff changes, and the current
operator-visible Exchange projection. The computed result is the aggregate of
still-unanswered prompt changes consumed by realtime steering, admission, and
session-check. Those consumers must not reconstruct acknowledgement from a
filtered change list.

| Transition | Computed acknowledgement |
| --- | --- |
| Exact prompt block followed immediately by a response heading | answered |
| Prompt followed by substantive prose or another prompt, then a later response heading | unresolved |
| Plain prompt-and-answer prose in one contiguous added run | answered when the existing plain-response classifier proves it |
| Explicit prompt followed by binary-owned backlog/done maintenance | unresolved; component maintenance cannot answer steering |
| Existing prompt gains only the canonical `❯` marker | metadata normalization; no new prompt |
| No associated response | unresolved |

The selector fails open: ambiguity keeps the operator prompt actionable. This
can repeat a prompt, but it cannot silently omit operator work. No plugin
receipt, controller generation, or later response heading may substitute for
this document-structure association. Prompt presence also does not suppress a
structural repair whose pure transformation proves that it carries the complete
prompt into the repaired projection. Such narrow repairs run before the broad
live-steering guard and publish through the same authority-fenced write; general
template normalization stays behind the guard.

## Source Of Truth

When a live editor owner owns the document, the CRDT relay/editor buffer is the
source of truth for operator changes. Disk is only a projection of that buffer.
Editor-buffer authority is origin-scoped: only a genuine incremental user edit
may originate editor-to-CP document state. A whole-buffer reload, file-cache
refresh, force-refresh attachment, or mutation performed while applying a CP
projection is non-operator state and must never be adopted into the canonical
document. When such a projection differs, the CP canonical is re-applied to the
editor; convergence is derived only after the visible text hash matches.
If disk and editor state disagree, the binary must converge through the editor,
wait for proven relay delivery, or fail closed. It must not use a direct disk
write as an automatic recovery behind the editor. There is no full live-buffer
side channel: current text lives in the Lazily/CP CRDT model, while open/close,
sync epochs, editor identity, plugin version, and capabilities travel on the
reliable-sync Lazily plane. No filesystem representation of live editor state
is created or consulted.
When a controller recycle removes server-side membership but the editor still
holds a cached client, refresh must retire that cached client and issue a new
registration. Every stale supervisor/controller recycle request emits that
forced-refresh event centrally, no matter which turn stage scheduled it. The
typed `reload_library` intent also refreshes all open document
replicas. Re-registration publishes the exact current editor buffer as the new
replica baseline. It never installs or saves a Lazily-retained whole-document
target first. Retained agent intents replay afterward through the ordered
document-cell/CRDT delivery path over that published operator cut.
At controller startup, the replicated missing-replica set emits one targeted
rebuild request for every stranded registration, including editors that
advertise peer pull. Peer pull remains a complementary startup and detected
transport-recovery path; it cannot replace the controller push because a
controller restart may be transport-transparent to the editor. If the idle
supervisor later observes an explicit attached-missing-replica or sync-pending
state, it requests one targeted current observation and then backs off for 30
seconds rather than polling.
If a hot-path read observes an editor owner but no usable CRDT relay model, the
binary must first attempt a bounded document-model ensure through read-only
editor publish/re-registration before returning an error. A failed ensure is a
named startup/reconciliation failure; it must not be collapsed into the raw
missing-replica observation, and it must not use disk as a successful
editor-authoritative value.
After bounded missing-replica recovery exhausts, an editor that remains open
keeps disk ineligible even for reads: the adapter records a named refusal and
fails closed. Disk becomes read authority only after the editor is proven
detached. Sync-pending follows the same rule after its one model-rebuild attempt.

An explicitly authorized `--force-disk` write is the sole exception to waiting
for editor delivery. It must first retain the complete pre-write disk base and
target as a durable deferred intent, may then write and commit disk without
relay membership, and must reconcile the editor from that intent when the
replica returns. Git HEAD is never the recovery base.

When no live editor owner owns the document, the current visible file
is the source of truth. A stored snapshot may seed a merge candidate, but merge
still applies an agent delta onto the current file and rechecks that the file
still matches that merge input before writing. Snapshot text must not replace
current file content unless the operator explicitly chose a disk-authoritative
recovery.

Out-of-band disk writes are operator-authored only while the document is
detached. With an attached editor, Lazily current remains the single authority:
the disk image is an external candidate that must be causally rebased onto the
current editor generation or rejected. It may never replace the editor cut or
resurrect nodes deleted in a newer editor generation.

Lazily visible-write receipts prove what an editor observed after a patch. They
do not prove that older snapshot text should overwrite newer operator text.

## Editor Frontend Hot Path

The editor text-change callback is a capture boundary, not a convergence worker. JetBrains `DocumentListener` callbacks, VS Code
`onDidChangeTextDocument` callbacks, and equivalent future editor hooks must
capture only the small event fields needed to identify the document, mark the
document dirty/typing-active, and enqueue later work. They must not perform full-buffer reads, CRDT merge, code-point offset conversion, socket I/O, native sidecar writes, patch application, or document saves on the editor UI thread or extension-host text-change callback.

Dirty/typing attribution is reserved for proven operator events. JetBrains
admits incremental events outside its CP-apply guard; VS Code admits dirty
incremental events plus explicit undo/redo. Clean reloads and whole-document
replacements fence stale queued deltas, request a CP drain, and never set typing
or unsynced-user state. Both plugins must implement the same admission contract.

Any work that can scale with document size, block on native code, block on IPC,
or mutate the document must be queued onto cancellable background work. That
background work must re-check the latest operator-visible source before
ACKing, applying, or broadcasting a change, and must be disposed when the document closes or the plugin unloads. A queued editor task is therefore never
authority by itself; if it runs after newer operator input, it must rebase
against that input or fail closed.

## Realtime Queue + Exchange Rules

The `agent:queue` and `agent:exchange` components are realtime document state,
not turn-local state. The operator may edit queue items and exchange text while
the queue is running. Every realtime observation must recompute the in-memory
queue projection and exchange update classification from the latest
source-of-truth document, including editor-buffer changes, pluginless disk
saves, backlog mirror inputs, priority markers, auto-DAG dependencies, and
exchange body additions/edits.

Exchange response-turn identity is content-derived from the response heading
**and** body, never the heading alone (`#qcellmerge-response-body-id`). Two
`### Re:` turns that share a heading — a same-topic or same-preset follow-up
drained from the queue — but carry different bodies are **distinct** turns and
must never collide: the cell merge must append the fresh turn, not treat it as a
duplicate of the prior committed turn and drop it ("a cell-merge chose the
existing content"). Byte-identical mirror-ordered duplicates left by a failed
3-way merge still share an identity (same heading and body, modulo the transient
` (HEAD)` annotation, a `~~…~~` strike wrapper, per-cycle boundary markers, and
trailing whitespace) and still collapse to one occurrence. This identity is owned
by `agent_doc_markdown_ast::exchange_tree::response_identity_digest` and is used
consistently by the append filter and the convergence dedup in
`agent-doc-merge`.

A whole-document replay is transport corruption, not a prompt the agent must
repair. Preflight and route preparation may coalesce two structurally complete
copies only when they have byte-identical frontmatter and either are identical
or one copy's lines are an order-preserving subset of the other. The superset is
the canonical projection so concurrent live prompt additions are retained. Two
divergent or reordered copies remain ambiguous and must fail closed. The
coalesced projection is written through CRDT authority before prompt-residue
classification or dispatch. The shared merge entry point enforces the same
gate before lossless-tree, component, or whole-document reconciliation: it may
canonicalize an exact/monotonic replay, but it must reject two divergent
complete projections rather than pass them to the whole-document fallback.
Monotonic equivalence may ignore only shared transient materialization rules:
code-block-aware ` (HEAD)` heading annotations and exchange-scoped `❯ ` prompt
prefixes. The current superset is retained byte-verbatim; ordinary reordered or
changed lines remain ambiguous.

An accepted write that reaches canonical CRDT authority while the durable editor
owner has zero registered replicas remains a content-bearing deferred delivery,
not a partial response and not a repair request. It records the exact editor/disk
base and complete target, requests stale-supervisor recovery, and leaves disk
unchanged. Once a replica reattaches, terminal verification may automatically
resume that same target through the ordinary CRDT delivery-projection barrier only when
canonical authority equals committed `HEAD`, disk still equals the retained base,
and the retained hashes and bytes match exactly. Any newer editor or disk content
invalidates automatic resumption and fails closed.

The same retained-delivery rule applies when an attached editor has not yet
published the matching visible-state projection. The write remains a successful
retained continuation. No foreground recovery signal, replay request, refresh,
or re-registration command is emitted; a later projection invalidates the
derived settlement Computed and resumes the eligible Effect.

The delivery receipt is a proof of one frontier, not a lock on canonical
authority. A live editor delta may advance canonical text between that receipt
and disk projection. The document actor must retain the original projection base
and complete target and re-enter the same CRDT replace/projection operation against the
advanced frontier. This is an internal rebase of one semantic intent: response
and backlog mutations apply at most once, operator text survives, and no new
capture/finalize/force-disk operation is admitted. A bounded foreground retry may
hand the unchanged intent to asynchronous supervisor recovery, but must never
misreport the retained delivery as an instruction for the agent to retry it.

Every Run Agent Doc preflight invokes retained-write recovery before it admits a
new cycle. The state-backbone policy classifies the retained verdict: satisfied
or unobserved intent continues, authority/disk divergence waits for the existing
delivery to converge, and only an intent whose payload is absent from an
authority/disk-agreed cut is replayed as stranded. Replay uses the existing
content-bearing journal base and target to apply the retained agent delta over
the current operator cut. It must preserve newer operator text, validate the
rebased document, and clear the intent only after canonical authority and disk
both equal the replayed target.

Queue recomputation is allowed to update future queue state without retargeting
the current turn. The active HEAD set is the runnable prompt or prompts the
realtime scheduler has proven are currently executing after queue normalization,
backlog sync, priority sorting, auto-DAG topological ordering, and done/review
catch-up strikes. In the common single-owner drain this set has one head. If the
scheduler explicitly owns multiple concurrent heads, the active HEAD set may
have multiple heads.

The `🚧` marker is a projection of the active HEAD set into the visible document;
it is not operator intent, not queue identity, and not an independent scheduling
input. Realtime must update the document so every actively running HEAD carries
the `🚧` marker, and no inactive/drained head carries it. Cosmetic markers do not
change selected head identity. The canonical marker vocabulary is:

- `🚧` — active/in-progress HEAD projection (the head being dispatched this cycle).
- `⏭️` — SKIPPED head projection (`#queueskip`): a head that was dispatched, came
  back unconsumed, and was passed over so the queue could advance to a
  non-dependent drainable head. It is a re-derived cosmetic projection like `🚧`,
  never changes identity, and clears automatically once the head is consumed or is
  no longer a live head. A skipped head is left in the queue for the operator (it
  is never auto-marked-done or dropped — halt-safety is preserved); the binary
  decides the skip deterministically so the agent is never asked to reconcile it.
- `📌` (operator pin) / `📍` (agent pin) — priority pins. The binary INJECTS the
  direct emoji; the `:pushpin:` / `:round_pushpin:` shortcodes (and the
  `**pin**` / `*prioritized*` emphasis spellings) remain accepted on parse, so
  existing documents keep working. All are stripped for identity comparison.

The skip decision is cycle-state driven: a head skipped after coming back
unconsumed once is recorded in `skipped_queue_head_ids` and carried forward until
resolved. When every remaining head is skipped or depends (via the `after=` DAG)
on a skipped head, selection is empty and the queue falls back to stall-stop.

If the operator moves `🚧` in the current realtime source epoch, realtime treats
that as a retarget request, validates it through the same auto-DAG dependency
projection, and projects `🚧` onto the selected head plus required prerequisites.
Stale markers left from an older projection are not retarget intent unless the
current diff/source epoch shows the marker move.

The pure queue marker and active-head projection rules live in
`agent-doc-document`. Realtime scheduling consumes those pure functions; turn
lifecycle code consumes the selected-head projection. `agent-doc-orchestration`
may temporarily adapt existing parser and IO surfaces, but it must not be the
long-term owner of `🚧` semantics.

| Operator edit | Realtime queue effect | Current turn effect |
|---|---|---|
| Edit, insert, delete, or reorder a non-selected queue head | Update the in-memory queue projection and backup/audit state. | Does not change the active turn when the selected head identity is unchanged. |
| Insert a new queue prompt before the selected head | Preserve the inserted prompt as future queue state unless the same source epoch explicitly retargets the active head. | Does not consume or replace the current turn target; closeout consumes the selected/snapshot head if it still exists, otherwise it no-ops/fails closed. |
| Edit the selected queue head | Update the queue projection. | Affects the active turn. If the buffer is still being edited, wait/pause; once settled, adopt the edited head as active input. |
| Edit a non-selected head so auto-DAG/priority recomputation changes the selected head | Recompute and persist the new queue projection. | Affects the active turn because the selected head changed. |
| Backlog-to-queue sync runs | Recompute id-backed mirror entries and preserve no-id manual queue entries. | Affects the active turn only if the selected head identity changes. Sync must not delete free-text operator queue lines. |
| Edit backlog/icebox/pending dependency metadata used by auto-DAG | Recompute dependency order from the latest source. | Affects the active turn when the selected head changes; otherwise it updates future queue state only. |
| Active HEAD set changes for any reason | Move the `🚧` projection to the active HEAD set in the visible document and backup/audit projection. | Affects a turn only when that turn's active HEAD identity changed or other active-turn input changed. |
| Edit `agent:exchange` | Preserve and merge the exchange update. | Always affects the active turn. Exchange edits are never hidden as future queue-only state, even when the same source epoch also changes non-selected queue heads. |

Queue crash recovery is part of the Lazily/CP current-document lineage. No
queue-specific journal or second head set exists: absence from newer Lazily
current may be an operator deletion and must remain absent. A genuinely unsaved queue add survives through
the editor's durable CRDT/reliable-sync stream, using the same ordered causality
as every other editor operation rather than a second queue-specific head set.

The combined realtime diff state is therefore not simply "file changed" or
"queue changed". It must classify at least these cases:

- `FutureQueueStateOnly`: queue projection changed, selected head identity is
  unchanged, and no exchange update or other active-turn input changed;
- `SelectedQueueHeadChanged`: selected head identity changed because the head
  text changed or queue normalization/auto-DAG selected a different head;
- `ExchangeUpdated`: exchange body or prompt-bearing exchange text changed;
- `MixedExchangeAndQueueUpdate`: exchange updated and queue projection changed
  in the same source epoch.

`FutureQueueStateOnly` may update realtime/backup queue state, including moving
or correcting the `🚧` projection when the active HEAD set identities relevant
to the current turn are unchanged, without replacing the active turn checkpoint.
`SelectedQueueHeadChanged`, `ExchangeUpdated`, and
`MixedExchangeAndQueueUpdate` are active-turn-affecting and must be surfaced to
the turn lifecycle. This classification belongs to realtime authority;
preflight and other lifecycle consumers should consume the classified state
rather than re-deriving it from raw unified diff text.

Each accepted editor `replica_update` also projects the current aggregate onto
the open closeout cycle as a `RealtimeSteeringObserved` backbone fact. The
JetBrains and VS Code state mirrors render that same Rust-owned payload in their
active-turn banner/status surface; they must not derive steering from raw editor
text. Projection is immediate, but dispatch remains barriered: editing does not
type into a busy pane. The normal stop/session-check barrier supplies the full
aggregate to the active turn. An explicit `agent-doc <file>` / **Run Agent Doc**
after editing is the settlement handoff; when a turn owner is active it queues
behind that owner, so partially typed text is never submitted as a new turn.

Auto-DAG is part of realtime queue projection. Dependency edges such as
`after=#id`, queue/backlog priority, and operator/agent priority pins are
recomputed before turn admission decides which queue head is active. A
non-selected queue edit is future-only only after this recomputation proves the
selected head identity did not change. If recomputation selects a different
head, the edit is active-turn input.

This rule separates queue-state convergence from turn lifecycle. Realtime may
apply and verify queue projection updates, but it must not commit them. The
turn lifecycle decides whether the verified state is committed; see
[Turn Lifecycle Authority](15-turn-lifecycle.md).

## Element Models

The realtime document model is composed from per-element realtime models. Each
supported `agent:*` element gets a small pure crate under the
`agent-doc-element-*` family:

| Element crate | Element | Local realtime model | Composition role |
|---|---|---|---|
| `agent-doc-element-exchange` | `agent:exchange` | prompt/response exchange state | local shared surface |
| `agent-doc-element-boundary` | `agent:boundary:*` | inline response boundary marker | projection consumed by exchange/turn closeout |
| `agent-doc-element-queue` | `agent:queue` | active heads, pins, priorities, auto-DAG ordering, `🚧` projection | consumer/projection of runnable work |
| `agent-doc-element-backlog` | `agent:backlog` (`agent:pending` alias) | tracked runnable work items | producer for queue projection |
| `agent-doc-element-review` | `agent:review` | tracked gated work items | local gate state |
| `agent-doc-element-icebox` | `agent:icebox` | tracked parked work items | local parked state, not queue producer |
| `agent-doc-element-done` | `agent:done` | completed-work archive state | archive target |
| `agent-doc-element-status` | `agent:status` | status projection | observer/projection |
| `agent-doc-element-signals` | `agent:signals` | signal definitions and readings | observer across document/runtime models |
| `agent-doc-element-unknown` | unregistered `agent:*` components | generic operator-authoritative component text | local safe fallback |

The base `agent-doc-element` crate defines descriptor vocabulary: marker
names, aliases, source, local realtime model, composition role, write policy,
and authority. `agent-doc-element-registry` composes the built-in
descriptors. These crates are pure: they must not read/write files, open IPC,
run tmux, mutate snapshots, dispatch agents, or commit.

`agent-doc-document` composes element models and owns cross-element invariants:
backlog-to-queue sync, active-head projection, auto-DAG recomputation,
tracked-work archive routing, and signal observations that depend on queue or
turn state. Element crates own local rules only. For example, backlog and
icebox can share a tracked-item model, but only backlog is a runnable work
producer; icebox is parked and must not mirror into `agent:queue`.

`agent-doc-element-unknown` is not a reserved `agent:unknown` document marker.
It is an internal fallback classification for an `agent:*` component whose name
is not known to built-in code or registered plugins. The fallback is
operator-authoritative and merge-only: realtime may preserve and carry the
content, but it must not perform semantic mutations against it.

Plugin-defined custom elements should register additional element descriptors
against the same vocabulary. Plugin loading, version checks, permissions, and
any effectful plugin execution are scheduled for a later runtime/plugin crate.
A plugin element still has to declare its authority and write policy up front.
Until that descriptor is registered, the unknown fallback preserves
operator-visible text.

## Disk Visibility And Durability

Realtime disk authority is based on bytes that are visible through a fresh
read of the document path after the write/save event, not on proof that the
storage device has durably flushed those bytes with `fsync`.

On a local OS, a completed editor save, `rename`, or atomic write is visible to
other processes through the kernel page cache before it is necessarily durable
on physical storage. That read-after-write visibility is the hot-path proof the
realtime state machine needs. Durability barriers (`fsync`, git object writes,
backup snapshots, and commit recovery sidecars) belong to backup, audit,
crash-recovery, or turn-commit boundaries; they must not make snapshots
hot-path authority.

File watcher events are hints, not proof. After a watcher event or pluginless
save, realtime must reopen/read the current file, compute the digest/epoch it
will use, and either observe a stable parseable document or enter
`ParseRecoverable`, `ParseBlocked`, `DiskDriftObserved`, or `ConflictBlocked`.
If a writer exposes a partially written file, the realtime loop waits for a
stable read/epoch or fails closed. It must not merge against stale buffered
content merely because a save notification fired.

## Disk Change Propagation To Live Editors

When the document file changes on disk out of band — a `git` operation
(`checkout`/`reset`/rebase), an external editor, or another process — the change
must reach the canonical CP replica and, from there, the live editor buffers.
The controller watch daemon and the owning supervisor run in separate processes;
the canonical `RelayHub` lives in the supervisor. Propagation crosses that
boundary through a **file marker polled by the supervisor idle loop** (the same
robust cross-process signal as recycle-request), never a socket the change
depends on.

Legacy socket/file-signal component patches are admitted only when two
independent facts agree: the reliable document-open set reports this file live,
and a captured targeted legacy endpoint is live for the same file. When both are
absent the document is detached and disk authority handles the write. A one-sided
result is an authority mismatch and must fail closed before a patch payload is
materialized or sent; recovery content remains retryable for the owning turn.

Path (`plan-crdt-scramble-and-disk-propagation.md`, Phase C/D):

1. **Detect + gate (daemon).** On a settled watch `Change`, the daemon runs
   `decide_watch_action(delivery, authority, edit_in_flight)`. Editor-attached
   documents (`ReconcileIntoCanonical` / `DeferForEditSettle`) drop a marker at
   `.agent-doc/disk-change-requests/<hash>.json`; headless
   (`ApplyAsDiskAuthority`, owned by the disk-authority load path) and non-change
   deliveries drop none. Self-write echoes are suppressed upstream by the watch
   gate, so the daemon never re-signals agent-doc's own write.
2. **Reconcile (supervisor idle loop).** The idle loop reads the current disk
   text and routes it through `RelayHub::apply_disk_change`, yielding a
   `DiskChangeOutcome`:
   - `AlreadyReconciled` — the canonical already holds the change (the editor
     authored it, or a peer already pulled it): a no-op. This is the
     **"editor buffer already has the changes → reconcile"** case, and it is
     idempotent.
   - `RebuiltFromDisk { live_members }` — an out-of-band **deletion** the additive
     CRDT delta cannot express. The canonical is rebuilt from disk and hub-side
     mirrors reseeded; `live_members` live editors still need a replace-capable
     re-bootstrap (below). The count is reported, never silently dropped.
   - `BaselineDeferred` — no commit baseline yet; the disk text is adopted as the
     baseline and the change defers to the normal editor-delta / commit path.
   The marker is cleared once observed (even on a headless no-op) so the idle loop
   never spins on it. The reconcile runs behind the bounded, fail-open editor sync
   barrier — never a held lock.
3. **Deliver to editors.** Additive changes reach live editors through the
   existing `ReplicaPull` delta channel. A `RebuiltFromDisk` deletion requires a
   **replace-capable bootstrap delivery**: the editor applies it by *replacing*
   its buffer, not CRDT-merging (an additive merge cannot drop the stale text).

   **Wire form (D2).** The supervisor owns the choice (FFI-first): `replica_pull`
   returns a tagged response — `{ "kind": "delta", "updates": [...] }` for the normal
   additive stream, or `{ "kind": "replace", "replace": "<canonical text>" }` when the
   pulling editor is flagged for re-bootstrap (`handle_replica_pull` drains
   `pull_rebootstrap_for_file`, which clears the flag). A replace pull is checked
   before the delta pull so a pending re-bootstrap always wins. On receipt the plugin
   (JetBrains `CrdtReplicaManager.applyReplaceDelivery`, VS Code
   `applyReplaceDelivery` — identical logic, thin consumers): (a) no-ops if the buffer
   already equals the canonical; (b) never clobbers unsaved operator edits
   (fail-open); (c) otherwise installs the canonical wholesale via a minimal edit
   (preserving cursor/undo) and re-bootstraps the native replica so later deltas are
relative to the corrected state. The `ReplicaPullDelivery` type (`Deltas` |
`Replace`) is mirrored across both frontends per the Editor Parity Requirement.

**CRDT lineage fence.** Additive updates are commutative and idempotent only
inside the canonical history from which they were produced. Every replica
registration therefore returns an opaque `lineage`; JetBrains and VS Code attach
it to each durable document-op frame. Whole-document rebuild/adopt operations
rotate the lineage. A receiver applies only a matching-lineage frame. A stale
lineage, or an unscoped legacy frame after rotation, is terminally quarantined
and quarantined so retry cannot duplicate the exchange, resurrect a queue tombstone,
or wedge the reliable-sync cursor. The lineage is checkpointed beside the `.yrs`
projection with that projection's SHA-256; recovery preserves it only when the
hash matches and otherwise mints a new fail-closed lineage.

   **Full-state delivery projection.** Every delta item also carries the SHA-256
   `expected_content_hash` of the canonical visible target. After applying the
   delta to its native replica *and* installing the converged text in the editor,
   the plugin publishes the visible buffer's `content_hash` with
   `replica_projection`.
   Generation equality alone is not convergence: a hash mismatch retains the
   delivery, flags the member for the replace-capable bootstrap above, and keeps
   disk materialization closed. The observation is cumulative: when an editor
   coalesces generations and proves the final target for generation N, the relay
   derives and advances every older pending generation through N.

JetBrains, VS Code, and Zed publish their current full-buffer projection after
visible apply and again after native save, with `disk_persisted` distinguishing
those two observed states. There is no retained per-update ACK sidecar or ACK
replay request. A missing or mismatched projection leaves that exact canonical
revision pending and re-projects it to the editor;
it never requests an editor full state, force-refreshes, or re-registers from the
editor buffer. In particular, a stale native baseline quarantines the local
delta and schedules a pull of the current controller canonical projection.

The deferred candidate remains retained as an ordered semantic intent, and the
per-document settlement graph resumes persistence and closeout only when its
derived projected state is current. Process exit, controller recycle,
or a same-target finalize retry therefore cannot lose or duplicate the response.
No foreground caller polls that graph or waits through a recovery barrier:
pending is a successful retained state, while unsaved divergent editor text is
an explicit conflict. The controller's canonical text remains the only
whole-document authority throughout recovery.

Deferred agent mutations are an ordered, content-bearing intent journal rather
than one replaceable whole-document target. Each entry retains its expected cut
and target bytes. Reconnect replays every entry in order over the current
operator cut, and convergence of entry N settles only the journal prefix through
N; an older projection cannot clear a newer mutation. This is required
for same-component changes such as backlog add followed by backlog mark: even if
the later target was composed from a stale base, replay preserves both changes.
When a response awaiting projection is already visible with later operator text, that
response intent counts as applied before later entries are replayed, so an
ours-wins exchange conflict cannot erase the operator text.

The Lazily intent generation is the authority that permits each reconnect
effect. RPC and in-process FFI are transport/wakeup mechanisms only: returning
target bytes does not itself authorize a later editor mutation. Immediately at
the editor-effect boundary, the worker must still observe the same desired
intent id, expected editor revision, target revision, and endpoint generation;
retired, superseded, or model-less observations cancel that effect without
touching the document. Effect completion publishes the observed editor revision
and receipt back into the same projection. A registered endpoint with no backing
model therefore derives an idempotent re-registration effect; it never grants
disk read authority and never requires an agent-managed library reload.

Exact target bytes are not sufficient settlement proof for a semantic rebase
intent. The non-capture byte-equality fast path is limited to delivery-only
reasons (missing replica, stale delivery worker, or pending delivery projection). An
intent that merged an unsaved editor cut, extended reconnect lineage, or awaits
an external-source decision remains retained until its operator-cut causality is
proved; reaching canonical authority and disk cannot bless a stale resurrection.

All generic realtime merges on this path—settled-operator rebase, deferred-target
composition, and reconnect—canonicalize boundary control state from the newest
target branch after the CRDT merge. A valid target boundary is the singleton
boundary in the result and is placed at exchange end; a non-template, malformed,
or boundary-free target cannot cause normalization to mint or strip a boundary.
Every effective agent target is then structurally validated: it must contain no
duplicate exchange component and at most one boundary marker. Invalid targets
remain retained and are never delivered.

The live editor document is normally the current text authority, but authority
for concurrent operator intent is the durable editor-op stream—not agent-written
projection bytes that happen to be present in that buffer. If a prior agent
projection structurally poisons the editor document (for example, duplicated
exchange content or boundary markers), reconnect rebuilds the operator cut by
replaying captured editor ops over the intent's expected base, validates that
cut, and then replays the deferred agent-intent journal. Thus an operator-only
`queue: stop` survives while duplicated agent content is discarded. This repair
is automatic and does not require or authorize a force-disk reset.

Editor-op capture is epoch-scoped. Immediately before JetBrains applies any
remote or agent-authored text mutation, it closes the native op-capture epoch.
Later operator operations therefore cannot be concatenated onto an old snapshot
base across an intervening agent projection. The newest CP/CRDT current text
remains the authority/fallback when an op epoch is unavailable.

Document-scoped editor actions must not create cross-document authority edges.
In particular, JetBrains Compact Exchange saves only its selected document
before routing; `saveAllDocuments()` is forbidden because it can synchronously
wake an unrelated document's retained delivery and surface that projection failure as
the selected document's compact result.

When the deferred reconnect result differs from the open JetBrains document, a
forced refresh must install those exact bytes into the visible `Document` before
registering the replacement replica. Installation is compare-and-swap against the
editor bytes used to compute the reconnect result, refuses pending local edits or
later buffer drift, and runs under the non-operator mutation guard. It refreshes
the clean target `VirtualFile` stamp before mutation and must save the installed
canonical bytes through `FileDocumentManager` before registration can advertise
that frontier. Only after the visible buffer, disk projection, and reconnect
target agree may registration seed and atomically swap the replacement member.
After every editor state projection, the controller compares disk bytes and
skips its legacy atomic projection when the editor already saved the same
canonical bytes; an identical rewrite is forbidden because changing only the file stamp can
raise JetBrains File Cache Conflict. This ordering prevents queue-consume or other
post-response targets from being accepted from a target-only replica while stale IDEA
text later replays an older boundary or queue state.

### Turn-State Projection To The Plugin

The CP owns the authoritative turn phase (`CyclePhase`). The plugin observes a
coarse projection — `TurnProjection` (Idle / AwaitingResponse / Persisting) plus
`turn_in_flight` and a `would_collide_with_in_flight_response()` guard so a
forwarded operator prompt queues for the next turn instead of double-appending
into an in-flight response. The plugin never drives a turn-state transition; the
CP is authoritative for every transition (`transition_authority`).

`TurnProjection` also carries optional `realtime_steering` while a turn is in
flight. The CP computes this from the current realtime document model compared
with the immutable turn baseline: prompt-target additions, document edits,
prompt deletion, and prompt reduction are projected as named steering states
with a short preview. The durable projection includes an `elements` observable
set keyed by SHA-256 identity over each directive kind and complete normalized
body; its ordinal retains document order. Replica update and visible projection
edges both refresh this set, so reconnects deduplicate stable directives and
one directive can be retracted without removing its concurrent siblings.
Each observation also carries the controller-stamped canonical CRDT content
hash. Session-check consumes the controller set directly for an open turn,
including authoritative emptiness, only when that receipt matches the canonical
content currently being checked. A missing or stale receipt falls back to
baseline comparison; it must never turn an unobserved default-empty projection
into evidence. This generation receipt is the acknowledgement boundary, so the
plugin does not need a second bespoke steering acknowledgement.
These states do not make disk authoritative and they do not turn the baseline
into a document source; they are operator steering signals that the editor must
surface on the active turn banner/status label so a deleted or changed prompt is
visible before the old response is persisted blindly.

### Editor Parity Requirement

All editor-facing behavior in this section — `ReplicaPull` application, the
replace-capable re-bootstrap delivery, and `TurnProjection` consumption — is part
of the Shared Foundation contract and **must have parity across the JetBrains and
VS Code plugins**. The reconcile, decision, marker, and projection logic lives in
the shared Rust/FFI layer; each plugin is a thin consumer of the same FFI
surface. A change to the editor delivery or turn-state projection is not complete
until both the IntelliJ and VS Code frontends consume it identically. Divergence
between the two frontends on any of these paths is a forbidden shape.

The **turn-state projection is the shared contract; the visible surface is
per-editor** because the two IDE platforms do not paint the same widgets
reliably. Both frontends map `TurnProjection` through the identical
`buildTurnStatePresentation` / `TurnStateBridge.presentation` logic (show
`⟳ agent-doc: persisting` / `⟳ agent-doc: awaiting response` while the CP turn is
in flight, append realtime steering such as `prompt deleted` when present, hide
when idle) and poll the `agent_doc_turn_projection` FFI on the same cadence. They
differ only in the native surface that renders it:

- **VS Code** renders it in a **status-bar item** (`turnStatusBarItem`), which
  paints reliably, with a tooltip and attention background while in flight.
- **JetBrains** renders it in an **editor banner**
  (`TurnStateBannerProvider`, an `EditorNotificationProvider`) driven by
  `TurnStateBannerRefresher`, because the IntelliJ 2026.1 status-bar widget API
  instantiates the widget but silently never paints it. The banner is a real
editor component that fails loudly instead of silently, so it is both reliable
and diagnosable. Local asynchronous document actions may install a token-guarded
transient presentation over the CP projection; `Compact Exchange` uses this to
show `⟳ agent-doc: Compacting Exchange` from action start through completion,
without allowing a stale callback to clear a newer operation. The status-bar
widget (`TurnStateStatusBarWidget`) is retained only for its `idea.log`
coordination probes.

Parity is satisfied by identical projection→presentation logic and equivalent
in-flight/idle behavior, not by forcing the same non-native widget onto both
platforms.

## Realtime States

These states describe document authority, not the agent turn/cycle. Agent cycle
states such as idle, `preflight_started`, `response_captured`, `write_applied`,
post-preflight, and committed may schedule work, but they must not change the
realtime authority rules.

| State | Authority | Meaning | Allowed next action |
|---|---|---|---|
| `DiskAuthoritative` | Current visible file | No editor listener currently owns the document. | Build a merge against the file, or wait for an editor attach. |
| `EditorDirty` | Live editor buffer | An editor owns the document and has unquiesced, unsaved, or in-flight edits. | Wait, observe CRDT/editor epochs, or fail closed. |
| `EditorQuiescent` | Live editor buffer | The editor owns the document and the debounce/epoch barrier is quiet. Disk may still be only a projection. | Build a merge against the editor-visible state and apply through editor/CRDT transport. |
| `DiskDriftObserved` | Live editor buffer plus current file as operator inputs | Disk changed outside the owning editor, such as a pluginless editor save, while an editor listener owns the document. | Reconcile disk and editor operator edits through the owner, or fail closed. |
| `AgentDeltaReady` | Prior source-of-truth plus structured agent intent | The binary has an agent-owned operation candidate, such as appending a response or compacting `agent:exchange`. | Call `agent-doc-merge` with the latest realtime document. |
| `MergePlanned` | Latest source-of-truth plus merge output | The pure merge core produced a patch plan or merged document that preserves operator text. | Deliver the plan through the owning transport. |
| `ApplyInFlight` | Pre-apply source-of-truth remains authoritative | A patch/write has been sent but delivery is not proven. | Wait for a matching visible-state projection, or fail closed. |
| `AppliedVerified` | Post-apply source-of-truth | The owner-visible document contains the agent operation and preserves observed operator text. | Save backup state and commit. |
| `ConflictBlocked` | Current source-of-truth | Merge or delivery could not prove preservation of operator text. | Leave the document untouched and report/retry later. |

Each live editor registration on the reliable-sync plane must carry a stable
frontend capability proof before the controller treats that editor as safe for
operator-preserving mutation. The canonical capability is
`operator_text_authority_v1`; current visible text itself remains in the CRDT,
not in registration metadata. A live but capability-unknown frontend remains an
operator-authority fence, but it is not safe delivery proof. The controller must
enter `ConflictBlocked` before sending a mutation through such a frontend. This
applies to normal writeback, compact exchange, repair redelivery, socket delivery,
and file-IPC fallback. Reloading/updating the frontend publishes a newer monotone
registration; direct disk overwrite remains an explicit operator escape hatch.

Snapshots never create a realtime state. A snapshot can contribute a candidate
delta to `AgentDeltaReady`; it cannot move a document to `MergePlanned`,
`ApplyInFlight`, or `AppliedVerified` by itself.

Controller replacement reconstructs live reactive inputs from durable facts,
not from a disk fallback or a retrying status RPC. Editor-authority and
disk-authority observations remain separate projection planes and carry the
write-fact ordinal at which each was observed. A replacement generation may
seed an exact-hash settlement input only when that ordinal is at or after the
retained intent; an older matching hash is not convergence proof. Installing a
new controller therefore recreates the graph and schedules its settlement
Effect without requiring an editor restart or `session-check` polling.

Closed-set workflow flags and transition provenance are typed enums in the
development harness. Free-text strings are permitted only at presentation or
serialization boundaries (`as_str`, logs, SQLite, or IPC). A matching frontend
enum is required only when the value crosses the editor wire protocol; internal
controller provenance such as `RetainedWriteCycleBoundary::{Preflight,
SessionCheck, FinalizePreCapture}` remains binary-only. Regression tests must
pin both the enum variants and their serialized tokens so a new consumer cannot
invent a near-duplicate free-text flag.

A deferred response target also cannot overrule a newer `ApplyInFlight` CP
frontier that already contains the latest complete response. That newer live
target supersedes the deferred assistant tail and rebases reconnect proof on the
exact failed-projection target. An unchanged editor cut receives the live target
directly; if the operator appended prompt nodes afterward, semantic response-tail
reconciliation preserves those prompts while removing the superseded response
and canonicalizing one terminal boundary.

The diagrams pin their Mermaid palette instead of inheriting page colors. Nodes
and edge labels use opaque fills, dark text, and mid-contrast links so they stay
readable in both light and dark renderers.

```mermaid
%%{init: {"theme": "base", "themeVariables": {"background": "transparent", "primaryTextColor": "#0f172a", "fontFamily": "ui-sans-serif, system-ui, sans-serif", "lineColor": "#64748b", "edgeLabelBackground": "#f8fafc", "clusterBkg": "#ffffff", "clusterBorder": "#94a3b8"}}}%%
flowchart LR
    DiskAuthoritative["DiskAuthoritative<br/>authority: current visible file"]
    EditorDirty["EditorDirty<br/>authority: live editor buffer"]
    EditorQuiescent["EditorQuiescent<br/>authority: live editor buffer"]
    DiskDriftObserved["DiskDriftObserved<br/>pluginless disk edit while editor owns"]
    AgentDeltaReady["AgentDeltaReady<br/>structured agent intent"]
    MergePlanned["MergePlanned<br/>operator-preserving plan"]
    ApplyInFlight["ApplyInFlight<br/>delivery not yet proven"]
    AppliedVerified["AppliedVerified<br/>post-apply source verified"]
    ConflictBlocked["ConflictBlocked<br/>leave document untouched"]
    Snapshot["Snapshot<br/>backup/audit only"]

    DiskAuthoritative -- editor attaches --> EditorDirty
    EditorDirty -- debounce/epoch settle --> EditorQuiescent
    EditorQuiescent -- operator edits --> EditorDirty
    EditorQuiescent -- clean detach --> DiskAuthoritative
    EditorDirty -- clean detach after projection proof --> DiskAuthoritative
    DiskAuthoritative -- pluginless editor saves --> DiskAuthoritative
    EditorDirty -- pluginless editor saves --> DiskDriftObserved
    EditorQuiescent -- pluginless editor saves --> DiskDriftObserved
    DiskDriftObserved -- reconcile operator sources --> EditorDirty
    DiskDriftObserved -- conflict or unproven merge --> ConflictBlocked

    DiskAuthoritative -- build operation --> AgentDeltaReady
    EditorQuiescent -- build operation --> AgentDeltaReady
    EditorDirty -- capture current authority epoch and rebase --> AgentDeltaReady
    DiskDriftObserved -- agent delta waits --> ConflictBlocked
    Snapshot -. bounded candidate only .-> AgentDeltaReady
    Snapshot -. forbidden .-> AppliedVerified

    AgentDeltaReady -- agent-doc-merge succeeds --> MergePlanned
    AgentDeltaReady -- typed conflict --> ConflictBlocked
    MergePlanned -- deliver through owner --> ApplyInFlight
    ApplyInFlight -- owner-visible projection equals target --> AppliedVerified
    ApplyInFlight -- newer operator or authority projection --> EditorDirty
    ApplyInFlight -- no editor and file changed --> DiskAuthoritative
    ApplyInFlight -- ambiguous/fails proof --> ConflictBlocked
    AppliedVerified -- realtime handoff complete --> EditorQuiescent
    AppliedVerified -- realtime handoff complete --> DiskAuthoritative

    classDef source fill:#e0f2fe,stroke:#0369a1,color:#0f172a;
    classDef work fill:#ede9fe,stroke:#6d28d9,color:#0f172a;
    classDef verify fill:#dcfce7,stroke:#15803d,color:#0f172a;
    classDef blocked fill:#fee2e2,stroke:#b91c1c,color:#0f172a;
    classDef backup fill:#fef3c7,stroke:#b45309,color:#0f172a,stroke-dasharray: 5 3;
    classDef split fill:#ffedd5,stroke:#c2410c,color:#0f172a;

    class DiskAuthoritative,EditorDirty,EditorQuiescent source;
    class DiskDriftObserved split;
    class AgentDeltaReady,MergePlanned,ApplyInFlight work;
    class AppliedVerified verify;
    class ConflictBlocked blocked;
    class Snapshot backup;
```

## State Transitions

Realtime transitions are continuous and must work regardless of agent state:

| Event | From | To | Required proof |
|---|---|---|---|
| Editor listener attaches | `DiskAuthoritative` | `EditorDirty` or `EditorQuiescent` | The editor publishes a live buffer identity/epoch for the document. |
| Editor listener detaches cleanly | `EditorDirty` or `EditorQuiescent` | `DiskAuthoritative` | Disk projection is current, or the detach path fails closed until current state is known. |
| Operator types, deletes, or plugin-mutates text | Any editor-owned state | `EditorDirty` | The edit is observed in the live buffer or CRDT/editor epoch. |
| Pluginless editor or external process saves the file with no editor owner | `DiskAuthoritative` | `DiskAuthoritative` | The next read observes the saved file as the source of truth; snapshots and `content_ours` do not overwrite it. |
| Pluginless editor or external process saves the file while an editor owns the document | `EditorDirty`, `EditorQuiescent`, or `ApplyInFlight` | `DiskDriftObserved` | Current file digest differs from the editor-owned projection or apply baseline. |
| Disk/editor operator sources reconcile | `DiskDriftObserved` | `EditorDirty` or `EditorQuiescent` | The editor-visible buffer includes all preserved disk and editor operator edits. |
| Disk/editor operator sources conflict | `DiskDriftObserved` | `ConflictBlocked` | Typed conflict proves the operator sources cannot be safely combined automatically. |
| Debounce and editor epoch settle | `EditorDirty` | `EditorQuiescent` | Latest editor-visible text is known and no in-flight local edit remains. |
| Agent response, queue operation, pending mutation, or compact operation is captured | Any source state | `AgentDeltaReady` | The operation is narrowed to the binary-owned node/intent and records the current Lazily/CP authority epoch as its merge base; snapshots are only a fallback source for this candidate. |
| Merge succeeds | `AgentDeltaReady` plus latest realtime source | `MergePlanned` | `agent-doc-merge` proves operator text is preserved or explicitly owned by the operation. |
| Merge conflicts | `AgentDeltaReady` plus latest realtime source | `ConflictBlocked` | Typed conflict describing the same-node or ambiguous placement failure. |
| Editor/CRDT delivery starts | `MergePlanned` with editor owner | `ApplyInFlight` | Patch plan targets the current editor-visible baseline or node proof. For CRDT remote text delivery, the handoff carries the expected editor text observed before convergence. |
| Disk delivery starts | `MergePlanned` with no editor owner | `ApplyInFlight` | Current file still matches the merge input, or the merge is recomputed first. |
| Owner-visible projection converges | `ApplyInFlight` | `AppliedVerified` | The Lazily projection of current authority equals the intended target and contains the agent delta plus every observed operator edit. A transport acknowledgement is evidence only and cannot drive settlement by itself. |
| Delivery fails, expected editor text mismatches, or a newer authority projection appears | `ApplyInFlight` | `EditorDirty`, `DiskAuthoritative`, or `ConflictBlocked` | Reactive inputs invalidate the computed settlement projection. The same content-bearing intent remains retained and rebases on the newly authoritative source; only a typed semantic conflict may enter `ConflictBlocked`. |
| Realtime handoff completes | `AppliedVerified` | `EditorQuiescent` or `DiskAuthoritative` | Realtime publishes the verified apply proof and latest source-of-truth text without committing. |

Forbidden transitions:

- `Snapshot -> AppliedVerified`;
- `VisibleWriteReceipt -> AppliedVerified` without comparing the owner-visible document;
- `AgentDeltaReady -> ApplyInFlight` without `agent-doc-merge`;
- `DiskDriftObserved -> AgentDeltaReady` before reconciling disk and editor
  operator sources;
- `MergePlanned` or `ApplyInFlight` to any document commit;
- `agent-doc-merge` or `agent-doc-document-realtime` running `git commit`;
- any transition that drops visible operator text to match `content_ours`, a
  snapshot, a stale replica projection, or a lazily visible-write receipt.

Additional convergence invariants:

- Both the requested agent target and the actual canonical text produced by a
reconnect or delivery must pass the shared document structural validator
before becoming an editor projection. This includes singleton components,
balanced component markers, and at most one live exchange boundary marker.
- Retained delivery is one exhaustive Lazily state projection over durable Base
  → Target facts, current authority text, live replica membership, convergence,
  and controller generation. One optional-effect projection covers activation
  observation, guarded target application, and closeout resume; waiting and
  conflict states project no Effect.
- Replica registration and its generation prove the canonical bootstrap opened
  by the replica, not the text currently visible in an attached editor buffer.
  When the post-registration buffer exactly equals that canonical text, the
  editor publishes the existing visible-state projection receipt for the new
  replica; a failed projection remains on the remote-drain retry path.
- Authority and disk equality do not settle a retained write while a known live
  editor still owes that receipt. The settlement Effect remains subscribed to
  the delivery projection, preserves the pending continuation, and emits the
  durable converged/wake edge only after delivery converges. Detached or
  unobserved delivery retains the ordinary authority-plus-disk settlement path.
- A replacement controller observes the already-retained registered editor/CRDT
  projection as the `AwaitingDelivery` transition's activation Effect. The
  controller listener is bound before restored liveness publishes
  missing-replica targets, so the editor's resulting projection becomes a
  delivery Source edge instead of racing an unopened socket. This does not ask
  an editor to acknowledge a request.
- If current authority consists of a structurally valid document through the
  terminal `agent:done` close followed only by text provably duplicated from
  managed content, the settlement projection may remove that trailing debris
  through the same editor-authority Effect. Any unique trailing text remains a
  typed structural conflict.
- A whole-document editor REPLACE delivery uses the same structural guard as an
incremental delivery. If the remote CRDT result is invalid, the plugin does
not install or ACK it; it re-adopts the exact coherent editor baseline,
atomically re-registers the replica, and retries the retained intent.
- Forced reconnect publishes the exact live editor cut and returns without
  reconstructing or installing retained response content through the editor
  bridge. Retained replay remains controller-owned and arrives through ordinary
  CRDT delivery, where the editor, replacement replica, pending-local counter,
  and disk fences are revalidated before mutation and acknowledgement. Replica
  registration itself must stay bounded so one document's recovery cannot wedge
  the shared editor-native generation.
- A retained captured response whose editor authority has advanced is replayed
over that authority whether the operator cut is still unsaved or has already
reached disk. The binary then requests native editor save itself. Operator
Ctrl+S, preflight repair, recapture, and force-disk are not recovery steps.
- A retained `post_commit_reposition` target may be superseded by a newer
structurally valid converged editor projection that already contains the
captured response. The deferred transition pins its owning cycle/capture
continuation when the event ledger is projected, so later closeout cycles and
supervisor recycling cannot replace the evidence used by the transition.
Commit-time boundary normalization preserves that transition's existing
boundary identity. Re-observing the same snapshot or working projection is a
fixed point: it must not mint a new boundary, publish a replacement target, or
restart editor delivery after the retained target has converged.
Replica ingress publishes the full visible projection as a `Source`; a
controller `Computed` derives a typed materialized-capture reconcile action,
and one controller `Effect` publishes it to the shared supervisor state plane.
The effect revalidates the pinned transition continuation, builds the
supervisor wake from that continuation rather than the current closeout cycle,
and advances its published frontier only after publication succeeds.
The supervisor subscriber derives both its keyed single-flight identity and
replay body from the same pending-transition continuation; a later closeout
cycle cannot make the received wake appear operationless.
Repeated observations and multiple supervisors are deduplicated by the exact
controller-generation/delivery-version action identity. Ordinary divergent
writes and malformed projections derive no action.
- Repeating that retained replay with the original content-bearing merge base is
idempotent per component. An unsplittable exchange race is isolated to the
exchange leaf; it cannot re-merge an already-composed queue as flat text.
  Operator-owned list multiplicity remains exact: intentional duplicate prompts
  are preserved when present in the operator cut, while retained-target-only
  replay copies are removed on the next merge over a cleaned operator cut.
- IDE document selection is operator-owned. Replica refresh, reconnect,
  controller activity, tmux focus changes, and background queue work must not
  open, select, or focus a different JetBrains document.
- Automatic editor-surface observation crosses the editor-native boundary as a
  bounded validate-and-enqueue operation. Controller probes and tmux
  consequences run outside the native-call generation, serialize per project
  root, and are latest-wins before execution; a blocked route cannot disable
  unrelated editor/CRDT calls or replay a superseded intermediate layout.

## Realtime Parse State

Document parsing is a separate realtime state machine. It runs over the same
source-of-truth document chosen by the authority machine above, but it answers a
different question: can the binary safely identify document components, spans,
frontmatter, response blocks, queue/backlog nodes, and operation targets without
guessing?

The parse state is a lazily-backed projection in
`agent-doc-document-realtime`. It is recomputed after every editor or disk epoch
and is mirrored to editor plugins through the lazily-spec snapshot/delta graph.
The parser should be pure document logic, with no access to turns, git, sockets,
clocks, or repair policy. A future `agent-doc-parse` or document-model crate may
own this pure parser, but the document realtime scheduler owns when the latest
parse projection is observed and published.

| State | Meaning | Allowed next action |
|---|---|---|
| `ParseValid` | The latest source-of-truth document has a valid component/frontmatter/span model. | Merge/apply may target parsed nodes after the authority state is also safe. |
| `ParseRecoverable` | The document has a local marker, boundary, or syntax issue with a deterministic diagnostic and optional structured repair proposal. | Surface feedback and wait for operator/editor action, or apply an explicitly owned realtime quick-fix with visible-state proof. |
| `ParseBlocked` | The parser cannot safely identify the nodes needed for dispatch, merge, compact, or commit closeout. | Block turn dispatch or realtime apply; keep the document visible and report diagnostics. |

```mermaid
%%{init: {"theme": "base", "themeVariables": {"background": "transparent", "primaryTextColor": "#0f172a", "fontFamily": "ui-sans-serif, system-ui, sans-serif", "lineColor": "#64748b", "edgeLabelBackground": "#f8fafc", "clusterBkg": "#ffffff", "clusterBorder": "#94a3b8"}}}%%
flowchart LR
    SourceEpoch["Source epoch<br/>editor buffer or visible file"]
    ParseValid["ParseValid<br/>component spans known"]
    ParseRecoverable["ParseRecoverable<br/>diagnostic + proposal"]
    ParseBlocked["ParseBlocked<br/>unsafe to target nodes"]
    PluginFeedback["Editor feedback<br/>inline diagnostics / status"]
    QuickFix["Explicit quick-fix<br/>through realtime apply"]
    TurnGate["Turn lifecycle gate<br/>consume parse projection"]

    SourceEpoch -- parse ok --> ParseValid
    SourceEpoch -- local marker issue --> ParseRecoverable
    SourceEpoch -- ambiguous structure --> ParseBlocked
    ParseRecoverable --> PluginFeedback
    ParseBlocked --> PluginFeedback
    PluginFeedback -- operator edits --> SourceEpoch
    ParseRecoverable -- operator accepts owned fix --> QuickFix
    QuickFix -- visible proof --> SourceEpoch
    ParseValid --> TurnGate
    ParseRecoverable -- no mutation without proof --> TurnGate
    ParseBlocked -- block dispatch/apply --> TurnGate

    classDef source fill:#e0f2fe,stroke:#0369a1,color:#0f172a;
    classDef parse fill:#dcfce7,stroke:#15803d,color:#0f172a;
    classDef recover fill:#fef3c7,stroke:#b45309,color:#0f172a;
    classDef blocked fill:#fee2e2,stroke:#b91c1c,color:#0f172a;
    classDef work fill:#ede9fe,stroke:#6d28d9,color:#0f172a;

    class SourceEpoch source;
    class ParseValid parse;
    class ParseRecoverable recover;
    class ParseBlocked blocked;
    class PluginFeedback,QuickFix,TurnGate work;
```

The editor plugins must surface parse issues as realtime feedback: inline
diagnostics, gutter/status feedback, and quick-fix proposals when a structured
fix exists. This feedback is advisory until the operator edits the document or
accepts a quick-fix that then crosses the same realtime authority, merge, apply,
and visible-verification path as any other document mutation.

Preflight consumes the latest parse projection; it does not own parse recovery:
preflight repair must not be the normal parse recovery path. Repair is a narrow
crash/retry backstop for already-captured lifecycle state, not a substitute for
continuous parse diagnostics over the live document. A `ParseBlocked` document
must remain operator-visible and block unsafe dispatch/apply instead of being
silently rewritten into a guessed valid shape.

```mermaid
%%{init: {"theme": "base", "themeVariables": {"background": "transparent", "primaryTextColor": "#0f172a", "fontFamily": "ui-sans-serif, system-ui, sans-serif", "lineColor": "#64748b", "edgeLabelBackground": "#f8fafc", "clusterBkg": "#ffffff", "clusterBorder": "#94a3b8"}}}%%
flowchart LR
    Source["Read latest source-of-truth<br/>editor buffer or current visible file"]
    Oob{"Out-of-band disk write<br/>observed?"}
    Split["DiskDriftObserved<br/>disk and editor are operator sources"]
    Dirty{"Editor dirty<br/>or epoch in flight?"}
    Wait["Wait for debounce/epoch<br/>or fail closed"]
    Delta["Build bounded agent delta<br/>from structured operation"]
    SnapshotCandidate["Snapshot-derived response candidate<br/>legacy fallback only"]
    Merge["Call agent-doc-merge<br/>latest source + agent intent"]
    Conflict["Typed conflict<br/>same-node or ambiguous placement"]
    Plan["Patch plan / merged document<br/>operator text preserved"]
    Owner{"Editor owns<br/>document?"}
    EditorApply["Apply through editor/CRDT<br/>node proof or current baseline"]
    DiskApply["Apply to disk only after<br/>current-file proof"]
    InFlight["ApplyInFlight<br/>source remains authoritative"]
    Verify{"Post-apply visible state<br/>contains agent delta<br/>and operator edits?"}
    Handoff["Hand off verified proof<br/>to document turn lifecycle"]
    TurnLifecycle["Document turn lifecycle<br/>may save backup/commit"]
    Block["ConflictBlocked<br/>leave document untouched"]

    Source --> Oob
    Oob -- no --> Dirty
    Oob -- "yes; no editor owner" --> Dirty
    Oob -- "yes; editor owner" --> Split
    Split -- "reconcile disk + editor edits" --> Source
    Split -- "conflict/unproven" --> Block
    Dirty -- yes --> Wait
    Wait -- settled --> Source
    Wait -- cannot prove --> Block
    Dirty -- no --> Delta
    SnapshotCandidate -. narrowed candidate .-> Delta
    Delta --> Merge
    Merge -- conflict --> Conflict --> Block
    Merge -- success --> Plan
    Plan --> Owner
    Owner -- yes --> EditorApply --> InFlight
    Owner -- no --> DiskApply --> InFlight
    InFlight --> Verify
    Verify -- yes --> Handoff
    Handoff -. "optional commit outside realtime" .-> TurnLifecycle
    Verify -- "no: stale ACK/new edit/mismatch" --> Block
    InFlight -- newer operator edit observed --> Source

    classDef source fill:#e0f2fe,stroke:#0369a1,color:#0f172a;
    classDef decision fill:#f8fafc,stroke:#475569,color:#0f172a;
    classDef work fill:#ede9fe,stroke:#6d28d9,color:#0f172a;
    classDef verify fill:#dcfce7,stroke:#15803d,color:#0f172a;
    classDef blocked fill:#fee2e2,stroke:#b91c1c,color:#0f172a;
    classDef backup fill:#fef3c7,stroke:#b45309,color:#0f172a,stroke-dasharray: 5 3;
    classDef split fill:#ffedd5,stroke:#c2410c,color:#0f172a;

    class Source,Wait source;
    class Dirty,Oob,Owner,Verify decision;
    class Split split;
    class Delta,Merge,Plan,EditorApply,DiskApply,InFlight work;
    class Handoff verify;
    class TurnLifecycle backup;
    class Conflict,Block blocked;
    class SnapshotCandidate backup;
```

## Mutation Protocol

Every document mutation that can affect a session document follows this order:

1. Read the latest source-of-truth document after the quiescence gate.
2. If disk changed outside the owning editor, reconcile the disk and editor
   operator sources first. With no editor owner, the current file is the source;
   with an editor owner, enter `DiskDriftObserved` and preserve both operator
   sources or fail closed.
3. Build an agent delta for the nodes the binary owns in this operation.
   Prefer structured operation/capture state. A snapshot-derived delta is a
   legacy fallback and must be narrowed before merge.
4. Rebase that delta against the latest source-of-truth document immediately
   before apply.
5. If the operator changed the same node, preserve the operator change and
   either merge the agent response around it with explicit proof or fail closed.
6. If the operator changed a disjoint node, keep both changes.
7. Apply through the editor/CRDT transport when reliable-sync liveness reports
an editor owner. Use the guarded `DetachedDisk` path only when the Lazily plane
proves no live editor owns the document, or when
   the operator explicitly chose a disk-authoritative recovery. `DetachedDisk`
   is the current-file realtime replica, not a snapshot fallback. CRDT remote
   delivery must compare the live editor text with the expected editor text
   captured before convergence; if the editor text advanced, the delivery is
   stale and must not ACK or mutate. Socket IPC, file-IPC fallback, and
reposition payloads target the newest live reliable-sync editor registration
carrying `operator_text_authority_v1`. Untargeted file-IPC
   fallback is not delivery proof for an editor-owned document.
8. Verify the post-apply source-of-truth document equals the intended target,
   contains the agent response, and still contains every operator-authored line
   observed before the apply. Editor API success alone is not proof.
9. Return the verified apply proof and latest source-of-truth text to the
   document turn lifecycle. Do not commit from merge or realtime code.

## Commit Boundary

CRDT merge does not commit. `agent-doc-merge` returns only a merged
document/patch plan or a typed conflict. `agent-doc-document-realtime` may
schedule, deliver, and verify that plan against the current editor or disk
source of truth, but merge/document-realtime paths must not run git commit,
advance the document turn lifecycle, or decide closeout success.

The document turn lifecycle owns commits. It may commit after it has the
captured response, pending-operation decisions, verified apply proof, current
source-of-truth text, and the selected write policy. It may also intentionally
leave a verified realtime merge uncommitted, such as a compact preview, a retry
handoff, or an operator-selected no-commit flow. Saving backup/audit state can
be part of lifecycle closeout, but backup writes are not realtime authority.
Invariant: document turn lifecycle owns commits; merge/realtime paths must not run git commit.

## Document Projection Crate Boundary

`agent-doc-document` owns pure document projections that can be computed from
document text and typed facts without disk, git, editors, tmux, clocks, or turn
state. Current responsibilities include:

- queue in-progress marker identity and projection;
- active HEAD projection from queue rows, marker-retarget requests, and
  auto-DAG prerequisite expansion;
- future pure parse/document views used by realtime diagnostics.

`agent-doc-document` does not commit, dispatch turns, run repair, call tmux,
open IPC, own editor/disk epochs, or decide lifecycle closeout. It returns
document-state facts for `agent-doc-document-realtime` and `agent-doc-turn` to
consume.

## Merge Crate Boundary

The pure boundary is the `agent-doc-merge` crate.
Use `agent-doc-merge` for pure merge semantics: document merge, conflict
resolution, and operation semantics as pure functions. It has no access to
disk, git, sockets, editor APIs, cycle state, ops logs, snapshots, or clocks.

Inputs include:

- the latest operator-visible document;
- the proposed agent delta or replacement node;
- the operation intent, such as `append_response`, `replace_exchange_for_compact`,
  `queue_consume`, or `pending_mutation`;
- optional base/CRDT state when available.

Outputs are a merged document/patch plan or a typed conflict. The merge core
must preserve disjoint operator edits, keep same-node operator changes unless
the operation explicitly owns the node with proof, and distinguish normal
response append from explicit exchange replacement.

`agent-doc-document-realtime` is the document-specific realtime boundary:
editor ownership, disk visibility epochs, debounce, CRDT transport,
reliable-sync registrations, parse diagnostics, and retry timing.
Other realtime loops, such as tmux, supervisor, editor-plugin, and controller
loops, use their own crate names. Document realtime orchestration may decide
when to call the merge core and how to deliver its patch plan, but it must not
redefine document authority.

## Lazily-RS State Backbone

`agent-doc-document-realtime` must use `lazily-rs` for document realtime state.
The authority state machine above, editor/disk epochs, owner leases, in-flight
apply facts, disk-drift facts, parse projections, and retry/fail-closed
decisions are lazily-backed state projections, not ad hoc globals, snapshots,
or turn-local sidecars.

The Rust implementation uses `lazily::ThreadSafeStateMachine` for closed
transition domains and `lazily::ThreadSafeContext` for the per-document
projection context. The projection is exposed through the existing lazily-spec
snapshot/delta graph so JetBrains (`lazily-kt`) and VS Code (`@lazily/js`) can
mirror the same realtime state without reparsing raw logs or session files.

Transient collaborator awareness is an ephemeral plane, not document state:
the relay stores it in lazily-rs `EphemeralMapCore`, excludes it from durable
CRDT serialization, and evicts it when a replica deregisters. Readiness that
admits an install-handoff auto-trigger is likewise an explicit lazily-rs
`ReadinessCore` composition of current-child output, actor readiness, and exact
owned-pane dispatch readiness; no single stale projection is sufficient.

Durable storage may keep append-only facts, checkpoints, and backup snapshots,
but the hot path reads the lazily-backed projection when deciding:

- which source currently owns document authority;
- whether disk drift is `DiskAuthoritative` or `DiskDriftObserved`;
- whether an apply is still `ApplyInFlight` or has reached `AppliedVerified`;
- whether a stale ACK, stale generation, missing owner lease, or out-of-band
  disk write forces a retry or `ConflictBlocked`;
- which editor/plugin mirror has the latest observed epoch.

Cycle state remains separate. It may request work or retain a captured response,
but it must not carry realtime document authority. Any future
`agent-doc-document-realtime` crate should therefore depend on
`agent-doc-document` for pure document projections, `agent-doc-merge` for pure
merge semantics, editor adapter crates for live buffer facts,
`agent-doc-turn-executor`/executor-specific model crates for dispatch-readiness
facts, `agent-doc-tmux` for shared tmux observations where tmux is the active
executor family, `agent-doc-supervisor` for supervisor realtime observations and
pure lifecycle decisions, and lazily-rs for its state machine/projection
substrate.
Tmux command construction and subprocess effects remain outside the realtime
authority core in `agent-doc-tmux-commands` and `agent-doc-tmux-io`.
Supervisor spawn/kill/reexec/socket effects belong in
`agent-doc-supervisor-process`, while durable project control-plane RPC/CAS
belongs in `agent-doc-controller`.

## Forbidden Shapes

The following are correctness violations:

- replacing a template session document with full-content IPC;
- adopting `content_ours` or a snapshot as a whole document when a narrower
  agent delta can be rebased onto the current visible document;
- dropping any visible operator-authored text, including non-prompt text;
- using a lazily visible-write receipt or a socket `already_applied` result to authorize a
  whole-document replacement;
- reading stale disk after a socket `already_applied` result and materializing a
  missing response into that disk image; without a matching visible-write receipt,
  the response operation remains pending for CP/CRDT replay;
- treating a pluginless editor save or other out-of-band disk write as stale
  drift to reset from `content_ours`, a snapshot, a lazily visible-write receipt, or HEAD;
- storing realtime authority only in a turn-local cycle sidecar, backup
  snapshot, ops log, or harness state instead of a lazily-backed projection;
- sending live-prompt-drift recovery patches for non-response components or
  frontmatter;
- treating operator-origin authority as future work or as gated by a later CRDT
  hookup;
- direct harness patchback of an assistant response into the session document.

## Harness Contract

Claude Code, Codex, OpenCode, Cursor, editor actions, Stop hooks, and direct
terminal runs all share the same boundary:

- turn resolution goes through `agent-doc respond` (`finalize` alias) or
  `agent-doc write --commit`;
- hooks may recover an interrupted turn only by replaying through the binary
  write/commit path;
- harnesses may insert a missing user prompt before a response exists, but they
  must not patch the assistant response directly into the document;
- a stale MCP server or other long-lived tool host whose running binary,
  executable path, generated instructions, or mutation contract is no longer
  launchable/current must refuse mutating tools such as `agent_doc_admit`,
  `agent_doc_preflight`, and `agent_doc_finalize` until restarted or recycled;
- untrusted, stale, or missing delivery proof is a fail-closed result, not
  permission to write the session document directly.

Queue and backlog items may request proof, tests, or cleanup for this invariant.
They must not describe operator authority as optional, deferred, or dependent on
a future implementation phase.

## Minimum Regression Coverage

Implementations must keep tests for these cases:

- ordinary non-prompt operator text typed during closeout is preserved;
- queue and backlog edits typed during closeout are preserved;
- live-prompt-drift recovery rebases only the missing response block onto the
  realtime document;
- compact exchange remains explicit `agent:exchange` replacement and preserves
  concurrent non-exchange edits;
- closeout/preflight maintenance must not send queue/backlog/status/reap
  convergence patches while an unsaved operator-visible editor buffer is ahead
  of disk;
- session-check must accept a latest committed response that is visible in a
  capable live editor buffer even when the stale disk file has not caught up;
- out-of-band disk writes with no editor owner are preserved as the current
  visible file state;
- out-of-band disk writes while an editor owns the document reconcile with the
  live editor buffer or fail closed before any agent response lands;
- lazily visible-write drift cannot reset operator-visible file content;
- an editor state projection persists its full visible content in Lazily, and a legacy
  hash-only `already_applied` receipt cannot authorize current-buffer
  publication; the controller reprojects its retained canonical revision;
- an editor-owned write with zero registered replicas retains its full target
  as a keyed Lazily deferred-write intent, returns promptly, and does not
  project to disk; registration and controller delivery events wake the same
  canonical projection without a recovery request;
- a peer-pull-capable editor still receives the controller's targeted
missing-replica rebuild after restart, and an idle explicit missing/sync-pending
observation schedules one bounded repair before the zero-replica backoff;
- stale-replica handling quarantines the obsolete delta and retains the
  controller revision. JetBrains, VS Code, and Zed project that revision
  downstream and expose no whole-editor recovery publication;
- after controller restart, an incremental update arriving before replica
  registration may restore membership but must not union-apply the retained
  editor lineage to a disk-seeded canonical. Every update from that member stays
  fenced while the controller bootstrap is lazily reprojected downstream;
- every `DocumentWriteDeferred` target passes the shared structural validator
  before its event is appended. A legacy exact-2× whole-document projection may
  retire retained intents before the integrity gate only when disk is the exact
  structurally valid single-copy target and every retained payload is that
  target (or its exact repetition). Retirement appends convergence events,
  never edits `state.db`, and refuses any payload containing text absent from
  the trusted target. The subsequent authority replacement is compare-and-swap
  against the observed doubled cut;
- explicit `--force-disk` is a true recovery override: it reads the disk cut,
  retires superseded ordinary retained-write lineage for that document, and
  writes through the audited raw disk path instead of re-entering live CRDT
  convergence. `escaped_template_patch` diagnosis applies only when HEAD itself
  drifted from the durable baseline; escaped-marker prose in an unchanged HEAD
  must not mask a doubled or otherwise corrupt visible projection;
- response-replay semantic normalization emits only a candidate-prepared event.
  Recovery success is emitted exactly once after re-reading live authority and
  observing the exact target content/hash. The JetBrains EditorSurface
  focus/layout lane remains independent of document-authority recovery and its
  first pane sync must complete in less than two seconds;
- `#crdtpushdrain`: the editor's no-op drain backoff gates only speculative
  polling. A controller-published canonical-projection event is positive
  evidence of retained work and drains urgently. The per-document Lazily key
  coalesces repeated projection demand; no ACK-replay or force-refresh
  escalation exists;
- response-cell retry materialization normalizes transient ` (HEAD)` and
  boundary annotations, and a latest complete response supersedes only
  uncommitted assistant-response nodes after the last unchanged committed
  response while preserving intervening operator prompts and one boundary;
- an applied relay mutation with an empty target set is not delivery
  convergence;
- harness Stop-hook recovery cannot commit transcript-shaped or direct-patched
  responses.
