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

### Re: Backpressure, offline peers, and state sync — glm-5.1 (HEAD)

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

### Re: Queue bookkeeping — glm-5.1 (HEAD)

Recovered from queue divergence. Backpressure response committed in prior cycle. Struck resolved items (#ipc1-#ipc3). Queue active — next item: WebRTC P2P broker question.

### Re: WebRTC P2P — broker or brokerless? — glm-5.1 (HEAD)

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
<!-- agent:boundary:383f41b1 -->
<!-- agent:boundary:6aeef3ff -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- How would backpressure be handled? What if the peer is offline or overwhelmed? Can we sync the current state once the peer is available?
- Let's go with WebRTC to start. Do we need a broker server or can this be P2P?
- How would coordination take place? Can a peer-to-peer model work? How would discovery work? Would we need a broker? Are there any internet-friendly ways with no or low cost to coordinate? Would piggybacking on WebRTC be a good approach or another protocol?
- What about security? There should be a fine-grained permission model for remote users. Sometimes I am the remote user and I want access to my own data. If another user is the remote user, I want to be able to control what they can see and do. There should be sensible presets.
- ~do [#ipc1]~
- ~do [#ipc2]~
- ~do [#ipc3]~
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#ipc3] [recommended] Investigate CRDT vs Raft trade-offs for lazily-distributed
- [/] [#ipc2] [recommended] Design lazily-ipc snapshot + incremental update protocol
- [/] [#ipc1] [recommended] Design lazily-serde feature gate (trait bounds vs type-erased closures)
- [/] [#vd5v] Future: leverage typed cache for lock-free cached reads in ThreadSafeContext (#lazilyperf38)
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
