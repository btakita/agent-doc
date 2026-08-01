# Plan — lazy document delivery projection

Status: accepted for implementation

## Incident

After a compact write had been accepted and ACKed, a later JetBrains local
delta reported an obsolete baseline. The relay quarantined that delta correctly,
but then requested an authoritative full state from the editor. JetBrains
answered by re-registering from its stale buffer. Repeated bootstrap/replay
merged obsolete component bytes into the controller canonical document and
eventually produced:

```text
mismatched component: opened 'status' but closed 'backlog'
```

The tmux pane projection was not the writer. It converged, then its route command
waited behind the delivery-recovery barrier and parsed the corrupted canonical
document.

## Architecture contract

### Invariant

For an attached document, the controller's canonical document projection is the
only whole-document authority. An editor observation may acknowledge or reject a
specific projected revision, but it may never replace canonical state merely
because its local baseline is stale. A quarantined stale delta cannot mutate
canonical text.

### Owner

The project controller `ProcessScope` owns one keyed Lazily graph per document.
That graph owns the canonical target, editor delivery receipt, retained
settlement verdict, and any closeout continuation. Editor plugins are thin
projection consumers. Disk is a settlement sink, not a competing live authority.

### Transition table

| Input transition | Derived projection | Effect |
| --- | --- | --- |
| Agent/compact mutation accepted | Canonical revision becomes desired; delivery is pending | Queue exact canonical replace/delta for attached editors |
| Editor applies exact revision | Content-qualified delivery receipt advances | Persist the exact canonical target and resume the captured closeout |
| Editor reports a stale baseline | Delivery remains pending; stale delta is quarantined | Re-project current canonical revision to that editor |
| Editor has unsaved divergent text | Conflict remains explicit and unsettled | Do not adopt, merge, save, snapshot, or commit |
| Editor detaches and disk authority is proven | Detached-disk settlement becomes eligible | Persist through the normal detached authority path |
| Route/tmux consumer observes pending delivery | Route projection remains pending/current | Do not request ACK replay, refresh, or full-state adoption |

### Reactive topology

```text
canonical mutation SourceMap
  + editor membership/endpoint SourceMap
  + visible receipt SourceMap
  + disk observation SourceMap
        |
        v
per-document delivery/settlement ComputedMap
        |
        +--> editor delivery Effect (exact canonical revision)
        +--> durable settlement Effect (only after derived authority proof)
        +--> captured closeout continuation Effect
        +--> route/tmux read-only projection
```

The graph is lazy and keyed by document hash. A transport call may wake or
observe it, but no transport callback is allowed to decide authority.

JetBrains, VS Code, and Zed implement the same boundary: registration consumes
the controller bootstrap, controller revisions flow downstream, and only
subsequent user-attributable incremental edit deltas flow upstream.

### Imperative extraction

Remove these active recovery decisions:

- `AckRecoveryState` requests for ACK replay and force refresh;
- `RequestFullState` after registration or quarantined stale updates;
- JetBrains stale-baseline adoption/re-registration from the editor buffer;
- foreground compact polling that waits eight seconds and then asks recovery to
  mutate delivery state;
- CRDT recovery-sidecar reads/writes used as a second whole-document authority.

Legacy sidecar rows may remain decodable for rolling data compatibility, but
runtime document authority must neither hydrate from nor checkpoint them.

### Allowed surfaces

- Controller canonical text plus revision/lineage.
- Content-qualified editor delivery receipts.
- Lazily-derived retained settlement and closeout continuation.
- Replace-capable controller-to-editor delivery that preserves unsaved operator
  conflicts by refusing to clobber them.
- Typed pending outcomes for callers that observe a retained projection.

No editor-to-controller whole-document adoption is allowed as stale-baseline
recovery.

## Verification contract

- A stale editor delta after compact is quarantined and cannot change canonical
  text.
- Registration and stale-delta handling do not emit `RequestFullState`.
- JetBrains stale-baseline handling schedules a pull of controller canonical
  state and never adopts/re-registers from the stale editor buffer.
- VS Code exposes no text-adopt/full-state recovery binding and reprojects the
  controller bootstrap on reconnect.
- Zed opening buffers never replace the controller bootstrap; full-sync
  `didChange` notifications are reduced to the smallest causal text delta before
  publication.
- An unacknowledged compact target returns as retained/pending without the
  eight-second recovery request barrier.
- Compact does not checkpoint a CRDT recovery sidecar.
- A malformed component projection is rejected before publication; route/tmux
  reads fail promptly and never trigger editor recovery.
- Existing tmux layout convergence remains green.

## Out of scope

- Changing component grammar.
- Replacing the Yrs delta format.
- Treating unsaved editor divergence as automatically resolvable.
- Schema-destructive deletion of legacy sidecar rows during the rolling
  compatibility window.
