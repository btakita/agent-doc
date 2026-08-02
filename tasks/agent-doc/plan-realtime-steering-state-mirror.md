# Realtime steering state-mirror contract

## Architecture contract

- **Invariant:** During an open turn, every settled CRDT replica update is compared with the turn baseline and the aggregate operator steering is projected through the durable state backbone. JetBrains, VS Code, the direct FFI projection, and session-check must expose the same primary kind, directive count, preview, and full verbatim aggregate. A new or closed cycle exposes no steering.
- **Policy owner:** `agent-doc-document-realtime` owns baseline comparison and aggregate conversion; `agent-doc-state-backbone::CloseoutProjection` owns the active-cycle state transition. Editor code only decodes the Rust-owned closeout payload.
- **Evidence inputs:** active closeout `cycle_id` and `phase`, the saved turn snapshot, the controller CRDT relay's canonical text after a `replica_update`, and the resulting content hash.
- **Allowed edit surfaces:** `agent-doc-turn`, `agent-doc-document-realtime`, `agent-doc-state-backbone`, the Project Controller CRDT ingestion adapter, the existing state-mirror projections in JetBrains and VS Code, their tests/specs, and release metadata.
- **Verification:** pure aggregate conversion tests; state-backbone cycle transition tests; state-wire snapshot/delta tests; controller CRDT observation tests; JetBrains and VS Code mirror/presentation tests; `make check`; installed-plugin build; latest CI review.
- **Out of scope:** replacing the text CRDT itself, dispatching text into a busy terminal pane, or treating every keystroke as a committed prompt.

### Identity-keyed observable-set completion (`#steeringobservableset`)

The durable closeout projection now carries each steering directive under a
stable SHA-256 identity derived from its kind and complete normalized body, with
an independent document-order ordinal. Controller replica updates and visible
projection receipts replace the current identity set in the state backbone;
reconnect replay deduplicates stable identities and retraction removes only the
missing member. The projection includes the controller-observed canonical
content hash. During an open cycle session-check treats that controller set,
including emptiness, as authoritative only while the receipt matches the
canonical content currently being checked; missing/stale receipts fall back to
baseline comparison. This makes the CRDT generation receipt the acknowledgement
boundary without a second plugin handshake. Aggregate
state/count/preview/verbatim fields remain as a rolling-upgrade and
editor-presentation view.

## Transition table

| Current cycle | Evidence | Backbone transition | Editor projection |
| --- | --- | --- | --- |
| idle / closed | any replica update | no steering fact | absent |
| open | baseline equals canonical CRDT text | `RealtimeSteeringObserved(none)` + canonical content-hash receipt | receipt only; no steering label |
| open | one prompt-bearing change | observed aggregate for current cycle | kind + count 1 + preview + verbatim |
| open | multiple prompt-bearing changes | observed aggregate for current cycle | primary kind + total count + full ordered aggregate |
| open | prompt deleted or reduced | observed deletion/reduction aggregate | deletion/reduction state + evidence |
| open | newer CRDT revision | idempotent content-hash event replaces current aggregate | latest aggregate |
| any | next `PreflightStarted` | reset steering before new evidence | absent |
| open | commit or abandonment | clear steering | absent |
| open | editor sync pending / missing snapshot | retain prior proven projection and retry on later update | no half-typed dispatch |

## Operator settlement

Editing surfaces steering immediately, but it does not type into a busy agent pane. The normal stop/session-check barrier delivers the aggregate verbatim to the active turn. Invoking `agent-doc <file>` after editing remains the explicit settled handoff: when an owner is active it queues behind that owner, avoiding submission of a partially typed document; otherwise it starts the next turn from the current committed CRDT projection.
