---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---



# lazily-rs: Reactive Signals Library

<!-- agent:status -->

- [x] Phase 1: Research & design
- [x] Phase 2: Core signal primitives
- [x] Phase 3: Derived/computed signals
- [x] Phase 4: Effect system
- [x] Phase 5: Testing & benchmarks
- [x] Phase 6: Documentation & publish
<!-- /agent:status -->

<!-- agent:exchange -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260604-223953.md`*

Compacted content:
- Archived 11 response topic(s): Backpressure, offline peers, and state sync; Queue bookkeeping; WebRTC P2P; 8 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: [#mvbj] #ipc1 — Design lazily-serde feature gate — opus-4-8

Designed and committed (`lazily-rs` `6c71dff`, pushed to `main`). Deliverable is a new **Serialization (`lazily-serde` feature gate)** section in `SPEC.md`.

##### The problem
`Context`/`ThreadSafeContext` store cached values type-erased (`Rc<dyn Any>` / `Arc<dyn Any + Send + Sync>`). `serde::Serialize` is **not object-safe** (generic `serialize<S>`), and `T` is gone by snapshot time — so a snapshot must recover a monomorphized `Serialize`/`Deserialize` per node.

##### Two approaches weighed
- **A — trait bounds (erased-serde style):** swap `dyn Any` for a sealed serialize-aware `dyn ReactiveValue`, add `T: Serialize + DeserializeOwned` to every signal constructor. Total serialization, but the bound is **viral** (leaks onto `Context` even for never-serialized state), and it **breaks the typed cache fast-path's** unchecked `TypeId`-keyed cast. Deserialize still needs a type-tag registry, so A adds the viral bound *on top of* the registry it doesn't avoid.
- **B — type-erased serde-vtable closures (chosen):** keep `dyn Any`; at creation capture an `Option<&'static SerdeVTable>` (`type_tag` + `serialize`/`deserialize` fn-pointers) next to the `TypeId` the typed cache already records. Thunks monomorphize per `T` at the construction site and run only at snapshot time.

##### Decision: Approach B
Composes with the existing `TypeId`-at-creation typed cache, zero overhead on the read hot path, opt-in per node (`T: !Serialize` → `Opaque` placeholder), and the field compiles out entirely when `serde` is off — preserving the zero-mandatory-dependency goal. The roadmap serializes an allowlisted `RemoteOp` set (#39c5), not arbitrary nodes, so Approach A's *totality* isn't worth its viral-bound + fast-path-rework cost.

##### Feature-gate shape
`serde = ["dep:serde"]` (+ `dep:erased-serde`, both optional); a sealed `MaybeSerialize<T>` helper resolves the vtable so existing constructor signatures are unchanged; `Context::snapshot()` / `ThreadSafeContext::snapshot()` emit `ContextSnapshot { nodes: Vec<NodeSnapshot { slot_id, type_tag, payload | Opaque }> }`. That snapshot type is the direct input to **#ipc2** (snapshot + incremental update protocol) and the value substrate for **#ipc3** (CRDT vs Raft).

Design-only change — `cargo test --test spec_compliance` stays green (101 passed). The uncommitted `Cargo.toml`/`Cargo.lock` `serde`/`serde_json` groundwork in the working tree predates this cycle and was left untouched (a design task shouldn't bundle dep wiring). Next queue item: `[#4rxf] #ipc2`.

### Re: [#4rxf] #ipc2 — Design lazily-ipc snapshot + incremental update protocol — opus-4-8

> **Queue prompt:**
>
> [#4rxf]


Designed and committed (`lazily-rs` `11dc3c3`, pushed to `main`). New **IPC snapshot + incremental update protocol (`lazily-ipc`)** section in `SPEC.md`, built directly on the #ipc1 `ContextSnapshot`.

##### Core idea — reuse the batch boundary, don't invent one
Two message kinds: `Snapshot` (full state, on connect/resync) and `Delta` (incremental). A `Delta` is emitted **once per outermost batch flush** — the single atomic graph-mutation boundary the thread-safe path already guarantees. Coalescing is free: one delta per flush, never one per `set_cell`.

##### Versioning
A context-level `ipc_epoch: u64` advances once per flush (independent of the per-slot `cache_revision` read-path atomics). `Snapshot` carries `epoch`; each `Delta` carries `{ base_epoch, epoch = base_epoch+1 }`, strictly sequential — so a receiver detects any gap/reorder/restart via `base_epoch == last_epoch` and requests a fresh `Snapshot`.

##### DeltaOp set
`CellSet` / `SlotValue` / `Invalidate` / `NodeAdd` / `NodeRemove` / `EdgeAdd` / `EdgeRemove`. The protocol **inherits** existing invariants rather than re-deriving them: PartialEq cell guard → equal writes emit nothing; memo equality suppression → equal recompute emits no `SlotValue`/downstream `Invalidate`; coalesced frontier → a multi-path dependent appears once per delta.

##### Lazy reconciliation
**Value-mirror (default):** sender resolves invalidated allowlisted slots via `ctx.get()` at flush so deltas carry concrete values; receiver is a pure data mirror. **Mirror-lazy** (receiver holds closures, recomputes on read) requires closure replication → deferred to `lazily-distributed` (#ipc3).

##### Boundaries
Allowlisted nodes only (#39c5) — non-allowlisted are *omitted entirely*, not `Opaque`, applied at construction. New `ipc = ["serde"]` feature, transport-agnostic (`IpcSink`/`IpcSource`), length-prefixed tagged messages. The `Delta`/`ipc_epoch` model is a single-writer linear log — whether multi-writer needs CRDT vs Raft on top is exactly **#ipc3**, next in queue (`[#jwwc]`).

Design-only — `cargo test --test spec_compliance` green (101 passed).
<!-- agent:boundary:2d138fcb -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- ~[#4rxf]~
- [#jwwc]
- [#q5r1]
- [#4cn0]
- do [#mb9b]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#jwwc] #ipc3 Investigate CRDT vs Raft trade-offs for lazily-distributed
- [ ] [#q5r1] #39c5 Implement RemoteOp enum + per-peer permission allowlist
- [ ] [#4cn0] #yxjw Build Cloudflare Worker signaling server
- [ ] [#mb9b] #vd5v Leverage typed cache for lock-free cached reads in ThreadSafeContext
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#ipc1] [recommended] Design lazily-serde feature gate (trait bounds vs type-erased closures)
- [/] [#ipc2] [recommended] Design lazily-ipc snapshot + incremental update protocol
- [/] [#ipc3] [recommended] Investigate CRDT vs Raft trade-offs for lazily-distributed
- [/] [#39c5] #sig1 [recommended] Implement RemoteOp enum + per-peer permission allowlist for lazily-distributed remote access
- [/] [#yxjw] #sig2 [recommended] Build Cloudflare Worker signaling server for internet-scale peer discovery
- [/] [#vd5v] Future: leverage typed cache for lock-free cached reads in ThreadSafeContext (#lazilyperf38)
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
