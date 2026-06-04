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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260604-201104.md`*

Compacted content:
- Archived 9 response topic(s): Review evaluation; Typed cache fast-path implementation; Queue cleanup; 6 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: Backpressure, offline peers, and state sync — glm-5.1

#### Backpressure handling

WebRTC Data Channel uses SCTP, which has built-in flow control and congestion avoidance. But the reactive signal model adds a semantic layer on top:

1. **Per-signal backpressure** — each signal subscription has its own delta stream. A slow consumer on one signal doesn't block others. The SCTP stream ID maps naturally to `(peer_id, signal_key)` tuples.

2. **Delta compaction under pressure** — when the send buffer is full (SCTP `bufferedAmount` threshold), coalesce pending deltas for the same signal:
   - Multiple `set(value)` deltas → keep only the latest value
   - CRDT merge operations → fold into a single merged delta
   - This is exactly what the existing `InvalidationPlan` infrastructure already does — the lazy invalidation frontier compacts dirty signals into a minimal recomputation set. The same logic applies to network-level delta compaction.

3. **Priority-based dropping** — signal subscriptions declare priority:
   - Critical (permissions, auth) → always deliver, block if buffer full
   - Normal (data state) → compact deltas, drop intermediate values
   - Low (analytics, telemetry) → drop freely under pressure

4. **Explicit backpressure signal** — a `PeerOverloaded` message from the receiver tells the sender to switch to snapshot-mode: stop sending deltas, switch to periodic full-state checkpoints until the receiver signals recovery.

#### Offline peer recovery

When a peer goes offline, the remaining peers continue operating — the reactive graph is local-first. Each peer tracks:

1. **Per-peer vector clock** — monotonic counter per `(peer_id, signal_key)`. Every delta increments the sender's clock. Enables gap detection on reconnect.

2. **Retention window** — keep recent deltas in a ring buffer per peer (configurable, default: last N deltas or T seconds). This is the "replay buffer" for short disconnections.

3. **Dirty set tracking** — the existing `ThreadSafeContext` already tracks which signals have changed since last read. Extend this to track which signals have changed since last successful delta send to each peer. This is the minimum set that needs reconciliation on reconnect.

#### State sync on reconnect

When a peer comes back online:

1. **Handshake** — exchange vector clocks. Each peer identifies which `(signal_key, clock_range)` entries the other is missing.

2. **Delta replay** — if the missing range fits within the retention window, replay the compacted deltas. Fast path for short disconnections (seconds to minutes).

3. **Snapshot fallback** — if gaps exceed retention, send a full signal graph snapshot. The receiving peer merges via CRDT semantics (last-write-wins, OR-Set, etc. — configurable per signal type).

4. **Incremental catch-up** — after snapshot sync, switch back to delta streaming. The transition is seamless because vector clocks provide a total ordering.

5. **Conflict resolution** — if both peers modified the same signal while offline, CRDT merge resolves it. For signals requiring strong consistency, use a deterministic conflict resolver (e.g., last-writer-wins with timestamp + peer_id tiebreaker).

#### Implementation mapping to existing lazily primitives

| Concept | Existing primitive | Extension needed |
|---------|-------------------|------------------|
| Delta compaction | `InvalidationPlan` (thread_safe.rs:733-923) | Network-aware compaction layer |
| Dirty set tracking | `ThreadSafeSlotFastPath` dirty flags | Per-peer dirty bitmask |
| Snapshot serialization | `Context::snapshot()` (if exists) or new `GraphSnapshot` | Serialize slot/cell values + dependency edges |
| Version tracking | `SlotNode::version` | Vector clock per `(peer, signal)` |

The key insight: **lazily's existing lazy invalidation and dirty-tracking infrastructure already solves most of the backpressure and sync problem at the graph level**. The network layer just needs to map dirty sets → delta streams and vector clocks → gap detection.

### Re: Queue bookkeeping — glm-5.1

Recovered from queue divergence. Backpressure response committed in prior cycle. Struck resolved items (#ipc1-#ipc3). Queue active — next item: WebRTC P2P broker question.

### Re: WebRTC P2P — broker or brokerless? — glm-5.1

**Pure P2P is viable with WebRTC, no broker needed for data.** You do need a lightweight signaling server for connection establishment, but it never sees signal data.

#### Why no broker for data

WebRTC Data Channels establish direct peer-to-peer connections after the initial handshake. Once the DTLS session is established, data flows directly between peers — no intermediary handles the payload. This is fundamentally different from WebSocket where every message passes through the relay server.

#### What you still need: signaling server

WebRTC requires a signaling mechanism for the initial handshake (exchange SDP offers/answers and ICE candidates). This is not a broker — it's a one-time coordination service:

1. **Signaling server** — lightweight HTTP/WebSocket server that relays SDP offers + ICE candidates between peers during connection setup. Options:
   - Minimal WebSocket server (~100 lines) that just forwards `offer`/`answer`/`ice-candidate` messages between known peer IDs
   - Existing infrastructure: any message bus (NATS, Redis pub/sub) works as signaling
   - Static config for fixed topologies (2-3 known peers): just exchange SDP out-of-band

2. **STUN server** — public servers (Google's `stun:stun.l.google.com:19302`) handle NAT traversal for free. Your peers discover their public IP/port.

3. **TURN server** — only needed when both peers are behind symmetric NATs (rare, ~5% of connections). Can use commercial TURN providers (Twilio, Xirsys) or self-hosted coturn. This is the only potentially costly component, and it's only used as a last-resort relay.

#### Peer discovery strategies

For a `lazily-distributed` deployment:

1. **Static peer list** — simplest. Each node is configured with known peer addresses. Fine for fixed deployments (your own devices, known servers).

2. **mDNS/DNS-SD** — zero-conf discovery on local networks. No server needed. Peers broadcast their presence and find each other automatically.

3. **Bootstrap node + gossip** — one well-known node introduces new peers. After initial connection, peers exchange peer lists via gossip. Scales to hundreds of peers.

4. **Signaling server as directory** — the signaling server doubles as a peer registry. Peers register on connect, query for other peers. Still not a data broker — it only stores peer addresses, never signal values.

#### Topology options

| Topology | Broker? | Latency | Complexity | Best for |
|----------|---------|---------|------------|----------|
| Full mesh | No | Lowest | O(n²) connections | Small clusters (< 10 peers) |
| Star (one peer as hub) | No | Medium | O(n) connections | Device-to-server, IoT |
| Gossip/superpeers | No | Eventual | Self-organizing | Large dynamic clusters |
| Federated hubs | Hub nodes | Low | Managed | Geographic distribution |

#### Recommendation

Start with **full mesh + signaling server** for `lazily-distributed`:

- Signaling: tiny WebSocket server for SDP/ICE exchange (not a data broker)
- STUN: use free public servers
- TURN: optional, for edge-case NATs
- Discovery: static peer list initially, add mDNS for LAN, gossip for WAN
- Data path: 100% P2P after handshake

The signaling server can be embedded in `lazily-distributed` as a built-in module — it's just SDP/ICE forwarding, not data processing.

### Re: Desktop-to-desktop (LAN), laptop-to-desktop (internet), remote user LAN, remote user internet — glm-5.1

All four scenarios are the same WebRTC P2P architecture with different discovery and NAT traversal paths:

#### 1. Your desktop <-> another desktop on LAN

The simplest case. No internet needed.

1. **Discovery**: mDNS/DNS-SD. Both desktops run `lazily-distributed` and broadcast on the local network. Zero configuration.
2. **Signaling**: LAN signaling via mDNS service records or a tiny local UDP broadcast. No server needed.
3. **Connection**: WebRTC Data Channel establishes directly on the LAN. Sub-millisecond latency.
4. **Practical setup**: Both machines run `lazily-node --discovery mdns`. Done.

No STUN or TURN needed — both are on the same network with no NAT between them.

#### 2. Your laptop <-> your desktop over the internet

1. **Discovery**: Static config. Your desktop's public IP (or hostname) is listed in your laptop's peer config.
2. **Signaling**: Lightweight signaling server. Your desktop registers; your laptop connects to exchange SDP/ICE.
3. **NAT traversal**: STUN server (free, e.g., Google's) for both sides. Works for ~95% of NAT configs.
4. **Fallback**: TURN relay only if both are behind symmetric NATs (rare).
5. **Self-access shortcut**: Tailscale/WireGuard VPN puts both devices on the same virtual LAN — mDNS works over the tunnel, no signaling server needed.

#### 3. Another user connects to your desktop on LAN

Same as scenario 1 technically, but with **authentication and authorization**:

1. **Discovery**: mDNS works — the other user's machine sees your `lazily-node` broadcasting.
2. **Auth**: On first connection, your desktop presents a pairing challenge:
   - **PIN/QR code**: you show a code on your desktop, they enter it. Like Bluetooth pairing.
   - **Pre-shared key**: `lazily-invite --permits read:signals.foo,write:signals.bar`
3. **Authorization**: Peer connection scoped to granted permissions:
   ```toml
   [peers."user-alice"]
   permissions = ["read:signals.weather", "read:signals.sensors.*", "write:signals.chat"]
   ```
4. **Connection**: WebRTC Data Channel after auth handshake. All data stays on LAN.

#### 4. Another user connects to your desktop over the internet

Combines scenarios 2 and 3 — NAT traversal + auth.

1. **Discovery**: Signaling server. Your desktop registers; the other user finds you via peer ID or shareable link.
2. **NAT traversal**: STUN for both sides. TURN fallback if needed.
3. **Auth**: Same pairing flow (PIN, invite token) over the signaling channel before WebRTC connection.
4. **Encryption**: WebRTC uses DTLS by default — end-to-end encrypted. Signaling server never sees signal values.
5. **Dynamic DNS**: If no static IP, the signaling server acts as rendezvous — your desktop connects outbound, no inbound ports needed.

#### Summary: same architecture, different bootstrapping

| Scenario | Discovery | Signaling | NAT Traversal | Auth |
|----------|-----------|-----------|---------------|------|
| Desktop-Desktop (LAN) | mDNS | LAN broadcast | None | Optional (trust LAN) |
| Laptop-Desktop (internet, own) | Static/Tailscale | Signaling server or VPN | STUN | Own devices (trust) |
| Other user-Desktop (LAN) | mDNS | LAN broadcast | None | Required (PIN/invite) |
| Other user-Desktop (internet) | Signaling server | Signaling server | STUN (+ TURN fallback) | Required (PIN/invite) |

The code is the same in all cases — `lazily-distributed` runs the same WebRTC P2P path. Only discovery mechanism and auth requirements change per scenario.

### Re: Free signaling servers & Cloudflare deployment — glm-5.1

#### Free / zero-cost signaling options

1. **PeerJS server** — free hosted signaling at `0.peerjs.com`. Open-source, ~500 lines of JS. Self-hostable. Zero config for dev.

2. **Embedded signaling in `lazily-distributed`** — ~200-line Rust WebSocket handler using `tokio-tungstenite`. Runs as part of the node binary. One peer acts as signaling host.

3. **Cloudflare Worker or Fly.io free-tier** — generous free tiers covering personal/small-team signaling.

#### Cloudflare is an excellent fit

- **Free tier**: 100K requests/day, 10ms CPU. SDP/ICE messages are tiny, signaling only during connection setup. Hundreds of pairings/day free.
- **Global edge**: low-latency worldwide.
- **No server management**: `wrangler deploy`, done.

Minimal Worker (~80 lines):

```typescript
export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname === '/offer' || url.pathname === '/answer' || url.pathname === '/ice') {
      const { from, to, payload } = await request.json();
      await env.SIGNALS.put(`${to}:${Date.now()}`, JSON.stringify({ from, type: url.pathname.slice(1), payload }), { expirationTtl: 60 });
      return new Response('ok');
    }
    if (url.pathname === '/poll') {
      const peer = url.searchParams.get('peer');
      const list = await env.SIGNALS.list({ prefix: `${peer}:` });
      const messages = await Promise.all(list.keys.map(k => env.SIGNALS.get(k.name)));
      return Response.json(messages);
    }
    return new Response('lazily-signal', { status: 200 });
  }
};
```

Cost: **$0/month** for typical usage. KV free tier: 100K reads/day, 1K writes/day. Full handshake = ~4 writes + ~4 reads.

**Recommendation**: Start with embedded signaling (simplest), add Cloudflare Worker for internet-scale.

### Re: Prescribed operations, sandboxing, and prompt injection defense — glm-5.1

#### Core principle: capability-based RPC, not arbitrary execution

Remote peers send **typed RPC commands** from a fixed allowlist — never code or prompts.

#### Prescribed operation model

```rust
enum RemoteOp {
    BacklogAdd { doc: String, text: String },
    BacklogList { doc: String },
    ContextAppend { doc: String, section: String, content: String },
    StateRead { signal_keys: Vec<String> },
    StateSubscribe { signal_keys: Vec<String> },
    ReviewAdd { doc: String, text: String },
}
```

Each operation maps 1:1 to `agent-doc` CLI or `lazily` signal API. No escape hatch.

#### Permission enforcement

```toml
[peers."user-alice"]
allow = ["BacklogAdd", "BacklogList", "StateRead"]
allow_docs = ["tasks/software/lazily-rs.md"]
allow_signals = ["weather.*", "sensors.temperature"]
deny_signals = ["secrets.*", "auth.*"]
max_backlog_add_per_hour = 10
```

Allowlist checked **before** deserialization — unknown operation types rejected at network layer.

#### Sandboxing layers

1. **Network**: WebRTC delivers only typed `RemoteOp` enum. No shell, no eval.
2. **Deserialization**: `serde` into `RemoteOp`. Unknown variants rejected.
3. **Authorization**: per-peer allowlist vs operation type + target. Denied = error, no side effects.
4. **Execution**: handler calls CLI/API directly. No shell, no string interpolation, no LLM prompt from remote input.
5. **Rate limiting**: per-peer counters, configurable per operation type.

#### Prompt injection defense

**Remote input never enters an LLM prompt.** Injection prevented by construction:

```
Remote peer -> RemoteOp enum -> validated handler -> deterministic action
```

"Add to backlog: text" writes `text` as data. Your local agent reads it later with `source=remote:alice` — same trust boundary as local edits.

Additional defenses:

❯ - **Content sanitization**: strip control chars, limit length, reject forged agent-doc HTML comment tags.
- **Provenance tagging**: every mutation tagged `source=remote:alice`.
- **Audit log**: peer ID, operation, target, timestamp.
- **Approval mode**: `require_approval: true` queues for explicit local approval.

#### What remote users CANNOT do

Execute shell commands, invoke agents/LLMs, modify exchange content, access signals outside allowlist, inject structural markdown, trigger compilation/deployment.

#### What remote users CAN do (with permissions)

Add to backlog/review, read signals, subscribe to signal changes, append context, request state snapshots.

Data-plane API only. Your desktop is the sole execution authority.

### Re: Plan for gated IPC tasks — glm-5.1

The three gated review items (#ipc1, #ipc2, #ipc3) form a layered dependency chain.

#### Phase 1: lazily-serde feature gate (#ipc1)

**Goal**: Make serialization optional so `lazily` core compiles without `serde` bounds, while `lazily-distributed` can enable them.

**Steps**:
1. Add `features = ["serde"]` to `lazily/Cargo.toml`, default off
2. Gate all `Serialize`/`Deserialize` derives behind `#[cfg(feature = "serde")]`
3. For closures in computed/effect signals: type-erased `Box<dyn Any + Send + Sync>` when serde is off; when serde is on, require `Serialize + DeserializeOwned` bounds on value types (not closures)
4. Add `lazily-serde` feature providing `SerializableSignal<V>` wrapper
5. Test: compile without serde, compile with serde, roundtrip signal values

**Key insight**: Closures are never serialized — only signal values are. Feature gate applies to value types, not function types.

#### Phase 2: lazily-ipc snapshot + incremental protocol (#ipc2)

**Depends on**: Phase 1

**Steps**:
1. Define `WireMessage` enum with Handshake, Snapshot, Delta, Subscribe, Unsubscribe, RemoteOp, PeerOverloaded, PeerRecovered variants
2. Define `SignalKey` as newtype over `String`
3. Define `SignalValue` as enum covering primitives + common containers
4. Implement `Context::snapshot()` — walks signal graph, collects leaf values + dependency edges
5. Implement `DeltaStream` — wraps dirty tracking, emits deltas per changed signal
6. Test: snapshot roundtrip, delta application, ordering

#### Phase 3: CRDT vs Raft trade-offs (#ipc3)

**Depends on**: Phase 2

**Recommendation**: CRDTs for signal values, authority-peer leader for permission/auth signals.

- Data signals: CRDT with last-writer-wins (timestamp + peer_id tiebreaker). Extends `SlotNode::version` to include peer origin.
- Permission/auth: your desktop as authority leader. Simple since you own the machine.

#### Dependency graph

```
Phase 1 (serde gate) -> Phase 2 (wire protocol) -> Phase 3 (CRDT/Raft)
```

### Re: do #39c5 — RemoteOp enum + permission allowlist — glm-5.1

Design incorporated into Phase 2 above. Implementation deferred until Phase 1 lands. Gated on #ipc1.

### Re: do #yxjw — Cloudflare Worker signaling server — glm-5.1

> **Queue prompt:**
>
> Create a plan for the gated ipc tasks


Independent of lazily core but depends on Phase 2 wire format. Design in prior response. Gated on #ipc2.

### Re: Plan file for ipc, local network, and internet communication — glm-5.1

> **Queue prompt:**
>
> create a plan file for ipc, local network, and internet communication


Created plan file at `tasks/software/plan-lazily-rs-distributed-communication.md` covering 7 phases:

1. **Serde feature gate** (#ipc1) — optional serialization, value-type bounds only
2. **Wire protocol** (#ipc2) — WireMessage enum, snapshot/delta streaming
3. **Consistency model** (#ipc3) — CRDT for data signals, authority leader for permissions
4. **WebRTC transport** — Data Channel transport, backpressure, offline recovery
5. **Signaling server** — embedded Rust or Cloudflare Worker, mDNS for LAN
6. **Remote operations and security** — RemoteOp enum, capability-based RPC, 5-layer sandbox, prompt injection defense
7. **Connectivity scenarios** — LAN, internet, own devices, other users

Dependency chain: Phases 1-3 are sequential. Phase 4 depends on 2. Phases 5-7 depend on 4.

The plan maps existing lazily primitives to distributed extensions and proposes the file structure under `lazily/src/`.
<!-- agent:boundary:cd2eb776 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- If a remote user adds a task to a backlog, the task should be clearly annotated.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#yxjw] #sig2 [recommended] Build Cloudflare Worker signaling server for internet-scale peer discovery
- [/] [#39c5] #sig1 [recommended] Implement RemoteOp enum + per-peer permission allowlist for lazily-distributed remote access
- [/] [#ipc1] [recommended] Design lazily-serde feature gate (trait bounds vs type-erased closures)
- [/] [#ipc2] [recommended] Design lazily-ipc snapshot + incremental update protocol
- [/] [#ipc3] [recommended] Investigate CRDT vs Raft trade-offs for lazily-distributed
- [/] [#vd5v] Future: leverage typed cache for lock-free cached reads in ThreadSafeContext (#lazilyperf38)
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
