# Plan: Reactive plugin state-sync via lazily-kt / lazily-js (`#lazilystatesync`)

## Operator ask

> What are #next-steps to use lazily-ky [lazily-kt] and lazily-js to sync the
> plugin state machines and state graphs from the FFI plugin?

## Context (what exists today)

- **`lazily`** is a reactive-primitives family: `Context`, `Slots`, `Cells` with
  automatic dependency tracking + cache invalidation. `lazily-rs` (Rust),
  `lazily-kt` (Kotlin), `lazily-js` (JS), and a language-agnostic wire protocol
  in `lazily-spec`.
- The FFI already exposes `agent_doc_state_projection(document_hash) -> JSON`
  (`ffi.rs:1726`). The durable state machines + stale-generation checks live in
  the binary (Shared Foundation pattern).
- The JB plugin's `StateProjectionBridge.kt` is explicitly "a thin editor-side
  bridge for the Rust-owned state backbone" — it reports observed facts and
  renders the projection JSON; `EditorCommandStateMachine.kt` is the editor-side
  state machine. VSCode mirrors this in TypeScript.
- **Gap:** the plugins consume the projection as a *pull/render* JSON snapshot.
  There is no reactive subscription, so the editor state machine/graph does not
  auto-update from FFI state changes — it re-renders on observed events. That is
  exactly the "sync the plugin state machines and state graphs from the FFI"
  the operator wants to close with the lazily family.

## Target architecture

FFI (lazily-rs) owns the authoritative reactive state graph; the plugins hold a
*mirror* lazily graph that applies FFI-emitted deltas, so editor UI derives from
tracked cells instead of manual JSON re-render.

```
lazily-rs Cells (FFI, authoritative)
   │  state-graph deltas (lazily-spec wire format)
   ▼
agent_doc_state_subscribe / delta export (FFI C ABI)
   ├──▶ lazily-kt mirror graph ──▶ EditorCommandStateMachine (JB)
   └──▶ lazily-js mirror graph ──▶ VSCode state projection
```

## #next-steps (backlog, priority order)

1. **`#lazilystatesync1`** — Define the `lazily-spec` wire shape for the agent-doc
   state projection (state machine nodes + state-graph edges + generation) so the
   FFI can emit *deltas*, not just full snapshots. Pin it as a versioned schema in
   `lazily-spec` so kt/js/rs stay in lockstep.
2. **`#lazilystatesync2`** — Back `agent_doc_state_projection` with a `lazily-rs`
   `Context`/`Cells` graph and add a subscription/delta export
   (`agent_doc_state_subscribe` + changed-cell delta serialization in the
   lazily-spec format). Keep the existing full-snapshot export for cold reads.
3. **`#lazilystatesync3`** — JB plugin: replace `StateProjectionBridge`'s manual
   projection-JSON render with a `lazily-kt` mirror graph that applies FFI deltas;
   make `EditorCommandStateMachine` a derived `lazily-kt` cell so dispatch-ready /
   busy / queue projections update reactively. Keep the plugin a thin consumer.

   **Status (0.2.190):** mirror substrate shipped. `agent_doc_state_subscribe`
   wired into `AgentDocLib`; plugin-local `StateGraphMirror.kt` (Gson, pinned to
   lazily-kt's `StateGraphMirrorTest` via `#lzpkgwire`) applies snapshot/delta
   messages; `StateProjectionBridge.subscribeMirrorForFile` advances a
   per-document mirror and `mirrorSummaryForFile` derives
   `MirrorProjectionSummary` (routeReadiness / routePaneId / transportPhase /
   proofMarkers) from tracked cells reactively. lazily-kt conformance pin
   (`StateGraphMirror` + `StateProjectionClient.refreshMirror`) + plugin tests
   green. **Follow-up (`#n529b`):** wire the reactive summary into the
   `EditorCommandStateMachine` / dispatch-ready / busy consumers
   (`PromptPoller`, `RunAgentDocAttemptLedger`) so they read the mirror instead
   of the cold-read pull, and dogfood live.
4. **`#lazilystatesync4`** — VSCode plugin: same mirror via `lazily-js`.
5. **`#lazilystatesync5`** — Parity + tests: cross-editor `lazily-spec`
   conformance test (the existing `EditorCommandStateMachineTest` extended to the
   reactive path) + an FFI round-trip test proving a delta applied to the kt/js
   mirror converges to the rs authoritative graph.

## Wire-shape definition (`#lazilystatesync1` design — pinned this cycle)

Grounding (verified in code as of this cycle):
- The binary already owns a typed `EventLedger` + `DocumentStateProjection`
  (`agent-doc-orchestration/src/state_backbone.rs`) across 7 domains — Document,
  Queue, Closeout, Transport, Supervisor, Route, Proof. Each domain's lifecycle is
  already a `lazily::ThreadSafeStateMachine` (QueueHead, TransportPatch,
  ActorLifecycle, RouteReadiness, ProofGate, CyclePhase). Replay is idempotent via
  `seen_event_ids`; per-`StateOwner` `generation` already rejects stale events.
- FFI surface today: `agent_doc_state_projection(hash) -> DocumentStateProjection`
  JSON (full snapshot, recomputed per call) + `agent_doc_record_state_event`
  (append). **No delta path exists yet** — the gap `#lazilystatesync1` closes.
- lazily-spec already defines the wire envelope: `schemas/snapshot.json`
  (`epoch` + `nodes[]` {slot_id, type_tag, state, payload} + `edges[]` + `roots`)
  and `schemas/delta.json` (`base_epoch`→`epoch` + `ops[]`: cell_set / slot_value /
  invalidate / node_add / node_remove / edge_add / edge_remove).

### type_tag vocabulary (the pin)

Each agent-doc state node maps to one lazily-spec node with a stable `type_tag`.
`slot_id = fnv1a(document_hash, type_tag, entity_key)` so kt/js/rs address the same
node without a central allocator.

| type_tag | source struct | state/payload |
|---|---|---|
| `agent_doc.document.baseline` | `BaselineProjection` | cycle_id + baseline_hash |
| `agent_doc.queue` | `QueueProjection` | active_head key |
| `agent_doc.queue.head` | `QueueHeadProjection` | `QueueHeadPhase` (pending/selected/deferred/completed) |
| `agent_doc.closeout.cycle` | `CloseoutProjection` | `CyclePhase` (+ cycle_id, commit, capture_id) |
| `agent_doc.transport.patch` | `TransportPatchProjection` | `TransportPatchPhase` (+ actor_generation) |
| `agent_doc.supervisor.owner` | `OwnerProjection` | `ActorLifecyclePhase` (+ generation, capabilities) |
| `agent_doc.route` | `RouteProjection` | `RouteReadinessPhase` (+ pane_id) |
| `agent_doc.proof.marker` | `ProofMarkerProjection` | `ProofGatePhase` (+ sources) |

`payload = base64(serde_json(struct))`. `state = resolved` once ≥1 event applied
for that entity, else `unset`/`dirty`.

### Edges (derivation graph)

- `closeout.cycle → document.baseline` (cycle scoped to its baseline)
- `queue.head → closeout.cycle` (head selected within a cycle)
- `transport.patch → closeout.cycle` (patch within a cycle)
- `route → supervisor.owner[RouteDispatch]` (readiness derives from actor lifecycle)
- `supervisor.owner[EditorIpcBridge] → transport.patch` (patches owned by editor gen)

These edges let a plugin lazily mirror invalidate/recompute only the derived
subtree on a delta instead of re-rendering the whole projection.

### Epoch / generation

- lazily-spec `epoch` = per-document monotonic counter bumped once per **accepted**
  (deduped) `StateEvent` — equivalent to `seen_event_ids.len()` after each `apply`.
  A re-emit/replay does not bump epoch (idempotent ⇒ no-op delta).
- Per-owner `generation` (already on `StateOwner`) is a **sub-lane frontier**: a
  `cell_set` on `transport.patch`/`route` carries `actor_generation`; the mirror
  rejects an op whose generation < the node's current generation (mirrors the
  binary's `current_generation_matches` stale rejection).

### Delta derivation (`agent_doc_state_subscribe`)

Cold read = full `snapshot` (existing `agent_doc_state_projection`, unchanged).
Warm read = `delta` since the caller's last epoch:
1. Caller passes `last_epoch` (or the FFI tracks the highest delivered epoch per
   subscriber).
2. FFI replays ledger events with `epoch > last_epoch` in order; for each
   `DocumentStateProjection::apply` that mutates a node, emit the matching op:
   - first appearance of an entity → `node_add` + `edge_add`(s)
   - phase/field change → `cell_set`/`slot_value` (new payload)
   - a derived node whose dependency changed but hasn't recomputed → `invalidate`
   - completion/reap → `node_remove` + `edge_remove`
3. Delta `epoch` = current accepted-event count; `base_epoch` = last_epoch. `ops[]`
   are ordered by emission so a kt/js mirror applies them verbatim.

Because the projection is a pure fold of deduped events, **delta application is
deterministic and idempotent** — the property `#queuestatemachine`/`#qdedupsync`
need, which is why this backbone is the right substrate for the queue + merge SMs.

### What lands where (split for scope-discipline)

- **This repo / agent-doc (`#lazilystatesync2`):** add `agent_doc_state_subscribe`
  + delta serializer in `ffi.rs` backed by the existing ledger; keep the full
  snapshot export as the cold path. Add the `type_tag` registry + `slot_id` fn.
- **lazily-spec (sibling, own session):** pin the `type_tag` table above as
  `schemas/agent-doc-state.json` (versioned) + a conformance snapshot/delta pair so
  kt/js/rs import one canonical vocabulary. This schema-pin half of `#56ta` is the
  only genuinely out-of-repo step; it is queued for the lazily-spec session.

## Scope / sequencing

- Steps 1–2 are FFI + lazily-rs/spec (this repo + sibling `lazily-*`); 3–4 are the
  editor plugins. Per scope-discipline, sibling `lazily-*` repo changes are
  confirmed/queued for their own sessions, not committed by inference from here.
- This is a multi-repo architecture effort — each step is its own focused cycle.

## Out of scope

- Moving durable state machines out of the binary (they stay FFI-authoritative;
  the plugins only mirror).
- Replacing the existing snapshot projection (kept as the cold-read path).
