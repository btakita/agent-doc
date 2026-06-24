# Plan: lazily state projection via FFI (no lazily-kt / lazily-js port)

**Status:** Decision + phased roadmap. Implementation gated on `#lzffistate` phase 1.
**Created:** 2026-06-24
**Session:** `tasks/software/lazily-rs.md`
**Queue item:** `#lzffistate`

## Question (resolved)

> Should we have a `lazily-kt` and `lazily-js` package to support the JB and VSCode plugins? Would the plugins benefit from the lazily state machine? Would it make sense for these plugins to use lazily FFI to communicate with the FFI plugin (bi-directional)?

## Decision

**No full reactive-core port. Yes to binary-owned FSMs consumed as serializable projections over the existing FFI/IPC channel.**

### Why not `lazily-kt` / `lazily-js` (port of the reactive core)

1. **The reactive core does not port.** `StateMachine<S,E>` (`lazily-rs/src/state_machine.rs:60`) holds state in a `CellHandle<S>` and a transition `Rc<dyn Fn(&S,&E)->Option<S>>` (thread-safe variant: `Arc<dyn Fn + Send + Sync>`). Its value is the auto-propagation through `Context`/`ThreadSafeContext`'s dependency graph (RefCell / `parking_lot::Mutex` interior mutability). That closure + borrow-graph model has no faithful Kotlin/TS analogue — a port would be a re-implementation, not a translation.
2. **It contradicts the project's own law.** agent-doc `AGENTS.md`: *"FFI-first for editor integration (Shared Foundation pattern) … Editor plugins should be thin event reporters … Business logic belongs in the shared FFI library, not duplicated across IntelliJ/VS Code plugins."* And *"All deterministic behavior in the binary."* Three reactive cores (Rust + Kotlin + TS) is exactly the duplication that law exists to prevent.
3. **The formal model only covers Rust.** `lazily-spec` Lean model + `make test-lean-formal` certify one implementation. A port re-derives an uncertified surface.
4. **lazily-rs FFI already refuses to export it.** `lazily-rs/src/ffi.rs` deliberately owns only opaque `lazily-ipc` Snapshot/Delta channel bytes — zero `StateMachine`/`Context` handles cross the ABI. That boundary is intentional; this plan keeps it.

### What the plugins DO get from the lazily state machine

They consume **authoritative projections** of the binary's machines instead of re-deriving state from file-watching heuristics. agent-doc already runs the durable FSMs — `CyclePhaseMachine` (`cycle_state_machine.rs:44`) and the four `ThreadSafeStateMachine` machines in `state_backbone.rs`: `TransportPatchMachine` (:1009), `ActorLifecycleMachine` (:1112), `RouteReadinessMachine` (:1246), `ProofGateMachine` (:1353). They fold `StateFact`/`StateEvent` into a `DocumentStateProjection` (:313, `#[derive(Serialize, Deserialize)]`: document/queue/closeout/transport/supervisor/route/proof slices). That struct is the seam: JSON-friendly, plain data, no Rust closures.

## Architecture

```
                 ┌─────────────────────────── agent-doc binary (Rust) ───────────────────────────┐
                 │  ThreadSafeStateMachine<Phase,Event> machines (state_backbone.rs)             │
                 │      ▲ apply StateFact                                                                       │
                 │      │                                                                                       │
                 │  DocumentStateProjection (Serialize/Deserialize)                              │
                 └──────┬──────────────────────────────────────┬─────────────────────────────────┘
            read JSON   │                              write JSON │  (event intake)
   agent_doc_state_<…>  │                              agent_doc_record_state_event
                        ▼                                       ▲
   ┌──────── JNA / C ABI (ffi.rs) + socket IPC ─────────────────┴──────────────────┐
   │ agent_doc_admin_*_json · agent_doc_start_ipc_listener_v2 · document_changed*  │
   └──────┬─────────────────────────────────────────────────────────┬─────────────┘
          ▼ JB plugin (Kotlin)                                  ▼ VSCode ext (TypeScript)
   render projection + report editor ops                render projection + report editor ops
   EditorCommandStateMachine.kt = LOCAL UI coalescing only (ephemeral, not a durable FSM)
```

The "lazily FFI for bi-directional communication" the operator asked about **is this channel** — it carries serialized state events/projections, not typed lazily handles. Existing reporter calls (`agent_doc_report_editor_state`, `agent_doc_record_editor_op`, `agent_doc_document_changed_digest_content_for_editor`) already cover editor→binary; this plan adds the state-projection read + state-event write pair.

## Phases

### Phase 1 — Projection read + event intake (the `#lzffistate` spike)

- `agent_doc_state_projection(doc_hash) -> FfiJsonResult` — returns `DocumentStateProjection` slices (or a filtered subset via a `slice` arg: queue/closeout/route/proof/transport/supervisor).
- `agent_doc_record_state_event(doc_hash, fact_json) -> i32` — feeds a `StateFact` into the backbone; deduped by `seen_event_ids` (`state_backbone.rs:301`).
- Both behind the existing JNA bridge: add to `ffi.rs` and mirror in `NativeLib.kt` (`AgentDocLib` interface) + the VSCode native-addon binding.
- Prototype behind a compile flag; unit tests for round-trip (record fact → read projection).
- Plugins become **event-reporter + projection-renderer** per the FFI-first rule.

### Phase 2 — Streaming deltas (optional, only if full-JSON round-trip is a perf concern)

- Reuse `lazily-rs` `IpcMessage::{Snapshot, Delta}` (`lazily-rs/src/ffi.rs`) as the transport for incremental projection updates instead of full JSON snapshots. Binary emits a Delta when a single slice changes; plugin applies to its cached projection.
- This is an optimization over Phase 1, not a prerequisite. Gate on a measured cost from full-snapshot polling.

### Phase 3 — Plugin UI wiring

- JB: `PromptPanel` / status bar reads `agent_doc_state_projection` to show route/closeout/proof phase; replaces heuristic phase guesses. `EditorCommandStateMachine.kt` stays local UI coalescing (keystroke batching, focus timers) — explicitly NOT a durable FSM.
- VSCode: equivalent status-item rendering.

## What stays local in the plugins (no lazily needed)

- Keystroke/scroll/focus coalescing (`EditorCommandStateMachine.kt`).
- Per-editor owner-election state already mediated by `agent_doc_plugin_owner_try_acquire`.
- Color/settings UI (`AgentDocColorSettings`), visual highlighter token mapping.

## Non-goals

- No Kotlin/TS port of `Context`/`Cell`/`Signal`/`Effect`/`StateMachine`.
- No new reactive graph in the editors.
- No exporting typed lazily handles across FFI (the `lazily-rs/src/ffi.rs` opaque-bytes boundary stays).

## Open / deferred

- Exact `slice` filter vocabulary for `agent_doc_state_projection` — decide at Phase 1 implementation from the first real consumer (JB status bar).
- Whether Phase 2 delta transport is worth it — measure Phase 1 first.
