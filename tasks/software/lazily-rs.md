---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: opencode
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
queue_active: false
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260604-180738.md`*

Compacted content:
- Archived 1 response topic(s): Update BENCHMARKS.md with Tokio results
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: Review evaluation — glm-5.1

**#lazilyperf40 (Lazy invalidation frontier): COMPLETE** — Implemented via `ThreadSafeInvalidationPlan` (thread_safe.rs:733-923) with sidecar fast frontier (thread_safe.rs:1924-2021). Benchmarks show 34% improvement (eager 5.832ms vs lazy 3.845ms at 16 workers). Covered by Loom model checking, spec compliance tests, and instrumentation tests. All 101 tests pass.

**#lazilyperf39 (Per-slot fine-grained locking): COMPLETE** — Each `ThreadSafeSlotFastPath` (thread_safe.rs:415-627) has independent RwLock, atomics, condvar, and dependency/dependent mutexes. Per-slot recompute uses waiter-counted condvar handoff (thread_safe.rs:534-596) instead of notify_all stampede. Spec compliance tests verify zero graph-lock acquisitions for cached reads and cell-only refreshes.

**#lazilyperf38 (Typed cache fast-path): NOT IMPLEMENTED** — Both `Context` and `ThreadSafeContext` still use `Rc<dyn Any>` / `Arc<dyn Any + Send + Sync>` with `downcast_ref` on every cached read (context.rs:76, thread_safe.rs:46). No typed cache mechanism exists anywhere. No tests, no spec language. This is the remaining gap.

#### Next steps for #lazilyperf38

1. Design typed cache: replace `Rc<dyn Any>` / `Arc<dyn Any + Send + Sync>` with `TypeId`-keyed storage or generic slot wrapper
2. Add SPEC.md language describing the typed cache fast-path contract
3. Implement for both `Context` (single-threaded) and `ThreadSafeContext`
4. Add benchmarks comparing dyn Any downcast vs typed access
5. Update existing tests to validate typed access paths

### Re: Typed cache fast-path implementation — glm-5.1

Completed all three queue items for #lazilyperf38:

**#93dv (SPEC)** — Added "Typed cache fast-path" section to SPEC.md (lines 397-444). Documents inline TypeId storage, unchecked pointer cast read path, benchmark gate, and future implications for lock-free reads.

**#139c (Implementation)** — Added `type_id: TypeId` to `SlotNode`, `CellNode`, `ThreadSafeSlotFastPath`, and `ThreadSafeCellFastPath`. All hot-path reads (`get`, `get_rc`, `get_cell`, `get_cell_rc`, `set_cell`, `read_fresh`, `read_slot_or_dependencies`) now use inline TypeId comparison + unchecked pointer cast instead of `dyn Any::downcast_ref` vtable call. All 101 tests pass. `make check` clean.

**#ek24 (Benchmarks)** — Added `typed_cache_reads` benchmark group (6 cases: context_slot, context_cell, thread_safe_slot, thread_safe_cell, context_rc_slot, context_rc_cell). Updated BENCHMARKS.md cached_reads results:

| Metric | Before | After | Improvement |
|--------|--------|-------|-------------|
| cached_reads/context | 8.22 ns | 7.92 ns | ~3.6% (p<0.05) |
| cached_reads/thread_safe_context | 64.41 ns | 62.49 ns | ~3.0% (p<0.05) |

**Review item #v6r8** — mark done. The typed cache fast-path is now implemented and benchmarked.

### Re: Queue cleanup — glm-5.1

Removed stale queue items. All typed cache work (#93dv, #139c, #ek24) is committed and pushed (30dc6fc). Review item #v6r8 done. #vd5v gated — lock-free cached reads require concurrent workload validation and Loom/Shuttle proof per SPEC.md lock-strategy section before starting.

### Re: Release + publish — glm-5.1

Released **lazily v0.8.0** to crates.io.

**Changes since v0.7.0:**
- Typed cache fast-path (inline TypeId eliminates vtable indirection)
- Synchronous cached-read fast-path for AsyncContext
- ThreadSafeContext-inside-Tokio benchmarks
- AsyncContext benchmark results in BENCHMARKS.md
- BENCHMARKS.md refreshed with updated instrumentation profiles

**Steps completed:**
1. Bumped version 0.7.0 → 0.8.0 in Cargo.toml + BENCHMARKS.md
2. Regenerated Cargo.lock
3. All 206 tests pass (unit + integration + spec + Loom + tokio + async + doc)
4. Committed c5cad84 — release v0.8.0
5. Tagged v0.8.0
6. `cargo publish` — published to crates-io
7. Pushed commit + tag to origin

### Re: Test

Simple inline test.

### Re: Serialization, IPC, and distributed coordination — glm-5.1

**Yes, and it should be a separate crate (or crates).** Here is the full design analysis:

#### Serialization

Signal values are currently type-erased (`Rc<dyn Any>` / `Arc<dyn Any + Send + Sync>`) with no serde dependency. Two approaches:

1. **Trait-bound approach** — optional `serde` feature gating `T: Serialize + DeserializeOwned + 'static`. Cleaner for v1, but forces serde bounds on all signal types.
2. **Type-erased serializer closures** — register `Box<dyn Fn(&dyn Any) -> Vec<u8>>` at slot/cell creation. More flexible, avoids polluting the core API, but more complex.

**Recommendation:** Approach 1 for v1. The type-erased closure approach can layer on later.

#### IPC: separate crate (`lazily-ipc`)

Would handle:
- **Snapshot transport** — serialize graph state (slot/cell values + dependency edges) for process transfer
- **Incremental updates** — leverage existing dirty-tracking / invalidation plan infrastructure to send only changed values
- **Transport abstraction** — channels, shared memory, unix sockets, TCP; do not bake in a specific transport

Core `lazily` stays dependency-free. `lazily-ipc` depends on `lazily` + `serde` + transport abstraction.

#### Distributed coordination: separate crate (`lazily-distributed`)

**Peer-to-peer model works well here.** Reactive signals are inherently local-state-centric — each node owns its graph and reconciles via message passing. A P2P model maps naturally:

- Each peer runs its own `ThreadSafeContext`
- Signal changes propagate as CRDT-mergeable operations (or Raft log entries for strong consistency)
- No central broker needed for eventual consistency
- Optional broker/leader for strong consistency (Raft)

**Discovery options (pick one):**
1. **Static config** — simplest, fine for fixed deployments
2. **mDNS/DNS-SD** — zero-conf local network discovery, no broker
3. **DHT (e.g., Kademlia)** — scalable peer discovery for large clusters, but overkill for most signal-graph use cases
4. **Gossip protocol** — members propagate peer lists organically; works well with CRDT merge
5. **Broker/registry** — central discovery service; simplest for cloud deployments

**Recommendation:** Start with gossip + static fallback. Gossip pairs naturally with CRDT-based signal merging. Add DHT or broker as optional backends.

**Consensus trade-offs:**
- **CRDT** — no coordination needed for merge; best for eventually-consistent signals (UI state, caches, analytics). Cells use CRDT merge semantics (last-write-wins, OR-Set, etc.)
- **Raft** — strong consistency; needed when signal values must agree across all nodes (financial data, permissions). Higher latency, requires leader election.
- **Hybrid** — per-signal consistency model; some signals use CRDT, others use Raft. Most flexible, most complex.

#### Recommended crate layering

```
lazily              — core reactive primitives (current, zero-dep)
lazily-serde        — optional serde integration (feature flag or thin crate)
lazily-ipc          — cross-process signal transport (serde + transport)
lazily-distributed  — multi-node coordination (consensus + discovery + gossip)
```

Each layer depends only on the layer below. Core `lazily` stays zero-dependency.

#### Queue items

`do #ipc1`, `do #ipc2`, `do #ipc3` are `[recommended]` design-phase items. They need design docs before spec/test/build. Removing from queue; keeping in backlog for design work. Ready to start design when you want.

### Re: Internet-friendly coordination — glm-5.1

The above response covered serialization, IPC, distributed coordination, P2P models, discovery, and brokers. For **internet-friendly** coordination specifically:

#### Internet-friendly approaches

1. **WebRTC** — peer-to-peer with NAT traversal (STUN/TURN). Signal state changes propagate directly between browsers/nodes. Best for real-time collaborative apps.

2. **WebSocket + relay** — lightweight server relays CRDT-merged signal deltas. Simple, works through firewalls. The relay is not a broker — it just forwards messages.

3. **libp2p** — modular networking stack (used by IPFS). Built-in Kademlia DHT for discovery, gossipsub for broadcast, noise for encryption. Ideal for decentralized signal graphs across the open internet.

4. **gRPC + tonic** — for controlled deployments where nodes know each other. Stream-based signal updates over HTTP/2.

5. **MQTT/NATS** — pub/sub messaging. Signal changes publish to topics; subscribers receive deltas. NATS specifically has CRDT-friendly features (JetStream persist + replay).

**Recommendation for internet use:** Start with `lazily-ipc` using a WebSocket relay (simplest path through firewalls). Layer `libp2p` in `lazily-distributed` for true P2P across NATs.

The queue items `do #ipc1`, `do #ipc2`, `do #ipc3` are `[recommended]` design-phase items — not ready for spec/test/build preset. They stay in backlog.
❯ Can lazily-rs support serialization, transferring content between processes, and network communication? Would we need a DHT or another distributed consensus protocol for reliable coordination? Should the IPC and network coordination be a separate crate?
Queue items `do #ipc1`, `do #ipc2`, `do #ipc3` are [recommended] design-phase items — no spec, no test plan, no implementation exists. The `#spec-test-build-install-commit-push` preset requires concrete spec + test changes. BLOCKER: design documents must be created before these items can enter a build queue. Response to the serialization/IPC/distributed question is above. Keeping items in backlog for future design work.
Restored backlog items #ipc1-#ipc3. Design-phase items, ready for future work.
<!-- agent:boundary:e024ce05 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
- do [#ipc1]
- do [#ipc2]
- do [#ipc3]
- How would coordination take place? Can a peer-to-peer model work? How would discovery work? Would we need a broker? Are there any internet-friendly ways with no or low cost to coordinate? Would piggybacking on WebRTC be a good approach or another protocol?
- What about security? Remote users 
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#ipc1] [recommended] Design lazily-serde feature gate (trait bounds vs type-erased closures)
- [ ] [#ipc2] [recommended] Design lazily-ipc snapshot + incremental update protocol
- [ ] [#ipc3] [recommended] Investigate CRDT vs Raft trade-offs for lazily-distributed
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#vd5v] Future: leverage typed cache for lock-free cached reads in ThreadSafeContext (#lazilyperf38)
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
