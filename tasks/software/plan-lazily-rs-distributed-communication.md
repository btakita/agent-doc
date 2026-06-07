# Plan: lazily-distributed IPC, Local Network, and Internet Communication

## Overview

Enable `lazily` reactive signals to synchronize across processes, devices, and users via WebRTC P2P with a lightweight signaling server.

## Phases

### Phase 1: Serde Feature Gate (#ipc1)

**Status**: Planned

Make serialization optional so `lazily` core compiles without `serde` bounds.

- Add `features = ["serde"]` to `lazily/Cargo.toml`, default off
- Gate `Serialize`/`Deserialize` derives behind `#[cfg(feature = "serde")]`
- Closures in computed/effect signals: type-erased `Box<dyn Any + Send + Sync>` without serde; `Serialize + DeserializeOwned` bounds on value types with serde enabled (closures are never serialized)
- Add `lazily-serde` feature providing `SerializableSignal<V>` wrapper
- Test: compile without serde, compile with serde, roundtrip signal values

**Key insight**: Feature gate applies to value types, not function types. `ThreadSafeSlot<T>` already requires `T: 'static + Send + Sync`; with serde, add bounds on serialized wrappers only.

### Phase 2: Wire Protocol (#ipc2)

**Status**: Planned
**Depends on**: Phase 1

Define the wire protocol for peer synchronization.

- Define `WireMessage` enum: Handshake, Snapshot, Delta, Subscribe, Unsubscribe, RemoteOp, PeerOverloaded, PeerRecovered
- Define `SignalKey` (newtype over `String` or interned `Arc<str>`)
- Define `SignalValue` (enum covering primitives + common containers, tagged serde)
- Implement `Context::snapshot()` — walks signal graph, collects leaf values + dependency edges
- Implement `DeltaStream` — wraps dirty tracking, emits `WireMessage::Delta` per changed signal
- Test: snapshot roundtrip, delta application, snapshot-then-delta ordering

### Phase 3: Consistency Model (#ipc3)

**Status**: Planned
**Depends on**: Phase 2

Evaluate and implement consistency model.

- **Data signals**: CRDT with last-writer-wins (timestamp + peer_id tiebreaker). Extends `SlotNode::version` to include peer origin. Use `crdts` crate for OR-Set and LWW-Register.
- **Permission/auth signals**: Authority peer (your desktop) as leader. Only authority grants/revokes permissions. Simple since you own the machine.
- Test: concurrent write conflict resolution, authority failover

### Phase 4: WebRTC Transport

**Status**: Planned
**Depends on**: Phase 2

Implement WebRTC Data Channel transport layer.

- Use `webrtc` crate (Rust native) for WebRTC without C dependencies
- Map `WireMessage` -> SCTP stream per `(peer_id, signal_key)` tuple
- Implement backpressure: delta compaction under SCTP buffer pressure, priority-based dropping, `PeerOverloaded`/`PeerRecovered` flow control
- Implement offline recovery: per-peer vector clocks, retention window ring buffer, per-peer dirty set tracking
- Implement reconnect: handshake vector clock exchange, delta replay (fast path), snapshot fallback (slow path)

### Phase 5: Signaling Server

**Status**: Planned
**Depends on**: Phase 4

Provide peer discovery and connection establishment.

- **Embedded mode**: ~200-line Rust WebSocket handler using `tokio-tungstenite`, runs as part of `lazily-distributed` binary
- **Cloudflare Worker mode**: ~80-line TypeScript Worker with KV storage, $0/month for typical usage
- Discovery strategies: static peer list, mDNS/DNS-SD for LAN, signaling server as directory for internet
- STUN: use free public servers (Google's `stun:stun.l.google.com:19302`)
- TURN: optional, for symmetric NAT edge cases

### Phase 6: Remote Operations & Security

**Status**: Planned
**Depends on**: Phase 4

Implement prescribed remote operations with capability-based security.

- Define `RemoteOp` enum: BacklogAdd, BacklogList, ContextAppend, StateRead, StateSubscribe, ReviewAdd
- Per-peer permission config: allowlist of operation types, allowed docs, allowed signal patterns, rate limits
- Allowlist checked before deserialization — unknown types rejected at network layer
- 5 sandboxing layers: network (typed enum only), deserialization (serde validation), authorization (per-peer allowlist), execution (direct CLI/API calls, no shell), rate limiting
- Prompt injection defense: remote input never enters LLM prompt — typed RPC only, no eval, no dynamic dispatch
- Content sanitization: strip control chars, limit length, reject forged agent-doc HTML comment tags
- Provenance tagging: `source=remote:alice` on all mutations
- Audit log: peer ID, operation, target, timestamp
- Optional approval mode: `require_approval: true` queues for explicit local approval

### Phase 7: Connectivity Scenarios

**Status**: Planned
**Depends on**: Phases 4, 5, 6

Support all four connectivity scenarios with same core architecture:

| Scenario | Discovery | Signaling | NAT Traversal | Auth |
|----------|-----------|-----------|---------------|------|
| Desktop-Desktop (LAN) | mDNS | LAN broadcast | None | Optional (trust LAN) |
| Laptop-Desktop (internet, own) | Static/Tailscale | Signaling server or VPN | STUN | Own devices (trust) |
| Other user-Desktop (LAN) | mDNS | LAN broadcast | None | Required (PIN/invite) |
| Other user-Desktop (internet) | Signaling server | Signaling server | STUN (+ TURN fallback) | Required (PIN/invite) |

- Auth methods: PIN/QR code pairing, pre-shared invite tokens
- Optional: Tailscale/WireGuard VPN for own devices (virtual LAN, mDNS over tunnel)

## Dependency Graph

```
Phase 1 (serde) -> Phase 2 (wire protocol) -> Phase 3 (CRDT/Raft)
                                            -> Phase 4 (WebRTC transport) -> Phase 5 (signaling)
                                                                            -> Phase 6 (security)
                                                                            -> Phase 7 (scenarios)
```

## Existing lazily Primitives Mapped

| Concept | Existing Primitive | Extension Needed |
|---------|-------------------|------------------|
| Delta compaction | `InvalidationPlan` | Network-aware compaction layer |
| Dirty set tracking | `ThreadSafeSlotFastPath` dirty flags | Per-peer dirty bitmask |
| Snapshot serialization | New `Context::snapshot()` | Serialize slot/cell values + dependency edges |
| Version tracking | `SlotNode::version` | Vector clock per `(peer, signal)` |
| Lazy invalidation | `ThreadSafeContext` frontier | Map frontier to delta streams |

## File Structure (proposed)

```
lazily/
  src/
    serde.rs          # cfg(feature = "serde") gated serialization
    ipc.rs            # WireMessage, SignalKey, SignalValue
    webrtc.rs         # WebRTC transport, delta stream
    crdt.rs           # CRDT merge for data signals
    auth.rs           # Authority/permission model
    remote_op.rs      # RemoteOp enum + handlers
    signaling.rs      # Embedded signaling server
  Cargo.toml          # features = ["serde", "distributed"]
```
