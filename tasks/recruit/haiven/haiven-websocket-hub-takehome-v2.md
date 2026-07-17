# WebSocket Hub: Prototype, Load Testing, and Design Evolution

**Author:** Brian Takita
**Audience:** Haiven backend + frontend + product
**Status:** Prototype built and load-tested; design in [DESIGN.md](https://github.com/btakita/haiven-websocket-hub/blob/main/DESIGN.md)
**Scope:** A single WebSocket Hub in Go: one persistent connection per client, multiplexing many downstream services (chat, presence, notifications, moderation, UI state, etc) over that one socket.

# 1 Prototype + Learnings + Claude Code Review

In my first version, I researched + did a draft design of a websocket hub supporting multiple stream services. The original case used 2 goroutines per connection, both inbound and outbound, but I soon learned about high-performance + scaling solutions in the Go ecosystem. I reasoned that the goroutine pair was the memory ceiling — two goroutine stacks dominate everything else a connection holds — so I removed it, moving to a gnet event loop. That got the connection down to 698 B/conn, and 100M on my dev machine. I will describe improvements and validation with a prototype that includes load, performance, and realistic scenario benchmarks. Developing the prototype helped me to:
- Explore high-scalability + performance libraries such as gnet & gobwas/ws...And that Goroutines have more overhead than evented solutions such as netpoller using epoll/kqueue.
- Implement lock-free Copy And Swap (CAS) via atomics where possible.
- Validate backpressure + overflow policy under load such as:
  - `ConflateByEntity`: Replace the existing message value with the latest value. Effectively LWW policy covered all cases in this class...which was only Presence in the case of the designed system. Future data types may include a true merge or aggregation. Supports **at-most-once** delivery...it discards by design, and a lost presence tick is corrected by the next one.
  - `DropOldest`: Drop the oldest message and replace with the new message at the back of the queue. Collapses to LWW in all use cases in the system. Can generalize to a circular buffer if needed. Supports **at-most-once** delivery.
  - `DropNewest`: Drop the arriving message when the queue is full, leaving the existing backlog intact. Supports **at-most-once** delivery. This is the fallback for any class that doesn't declare a policy...a class opts into durability, it doesn't get it by default. No shipped service uses it: chat/notification/moderation take Spill, presence/typing take ConflateByEntity.
  - `Spill` onto durable storage to preserve messages while keeping memory usage in budget. Supports **at-least-once** delivery...nothing is dropped, so it's the only policy that can make that promise, and it pays for it in storage plus client-side dedup by `id`. Services with this policy include:
    - Chat
    - Notification
    - Moderation
- Identify memory + performance improvements by load testing.
  - Identify the performance effects of sharding the connection registry Map and the seqAllocator (chat, notifications, moderation)
- Drive realistic scenarios backed by fake downstream services. Identified use cases that included:
  - Large rooms with many users (fan-out)
  - A user being in multiple rooms (fan-out)
    - Typing indicator + in-room presence messages being sent only to users currently viewing the room...a la Discord.
  - Presence of friends (fan-out) and memory growth from a naive friends graph leading to unpredictable capacity. Key point is to design the hub to have bounded messages and fan-out.
  - Downstream websocket reliability under load via pooling
- Same process benchmarks have gaudy numbers (>3M deliveries/sec). Moving a service out of process over a socket costs throughput, but I'm not quoting a figure: I iterated on that harness several rounds without getting it right. It has produced a socket beating a function call, which cannot be true, and each fix moved the number a lot. It also can't settle ws vs gRPC — a harness bottlenecked on something both transports share would report them as identical, which is what it reports.
  - I started with "route by volume" — keep the loud services in process, push the quiet ones out. Load testing overturned it: a socket carries a service's *send* rate, and the hub fans out locally, so fan-out width never crosses the boundary. Wide fan-out is the hub's expensive case and the socket's cheap case. What actually decides it is the failure mode and the resource profile, not the traffic (see Benchmark Highlights). For v1 everything stays in process anyway...one team, one deploy, and YAGNI.
- Review Claude Code generated source code which mostly was satisfactory to me with minor corrections + tweaks. Changes include:
  - Claude set Typing message to overflow policy to DropNewest...it should have been ConflateByEntity.
  - Use `a := slices.Clone(b)` instead of `a := make([]byte, len(b)) + copy(a, b)` to snapshot slice `b`.
  - Use gnet `Engine.Stop` instead of `gnet.Stop`...driven by deprecation warning and validated by race condition in load tests.
  - Rename ambiguous names such as `gc` to `gconn` for the gnet connection.

# 2 Design

Please see the DESIGN.md attached to the email...or [DESIGN.md](https://github.com/btakita/haiven-websocket-hub/blob/main/DESIGN.md)...which is in a private repo. Please contact me if you need permission to view.

I'm going to focus this document on salient design aspects + load test-driven design evolution.

# 3 Iteration to Arrive at using Go's High Scalability & Performance Ecosystem

Being new to the Go ecosystem, I researched the highest performance solutions and arrived at gnet + gobwas/ws. I considered scalability, performance, memory usage, and community ecosystem + maintainer bus factor in choosing these libraries. This required a redesign from the simple inbound/outbound goroutines to the gnet evented api.

Gnet provides heartbeats configured by `Server.pingInterval` and read deadlines configured by `Server.readDeadline`. These features are crucial for maintaining the reliability and stability of the WebSocket connections.

Lock-free CAS via atomics replaced Mutex locks where possible. Then mutex Locks replaced Goroutine actors where possible. Finally, Goroutine actors were sharded as load testing stressed the bottlenecks.

Also adopted connection pooling of websockets to upstream services due to dropped sockets under load.

JSON is the codec being used. More efficient codecs such as MsgPack + Protobuf over grpc were explored but not adopted due to messages being opaque. Zero copy codecs such as CapnProto is another possibility for downstream services sharing the same address space.

In-process vs socket communication was benchmarked, with opportunistic batching on the socket path to amortize syscalls. Where the socket's ceiling actually sits is still open...that harness isn't correct yet (see Benchmark Highlights).

# 4 Load Testing

I was able to achieve benchmarks up to 100M connections, holding ~3.1M deliveries/sec with 0 shedding under no backpressure.

For users with 0 friends, the heap/connection was 698-717 B/connection...flat from 5M to 100M, which is what makes capacity planning arithmetic rather than extrapolation.

The initial implementation of a friends graph caused linear unbounded memory growth. A solution was to create a friend-frame, which caps memory growth to 1 page of friends and reduces topic fan-out message count. A full implementation is beyond the scope of this take-home, but worth considering for capacity planning.

## Benchmark Highlights

See attached LOADTEST.md or [LOADTEST.md](https://github.com/btakita/haiven-websocket-hub/blob/main/LOADTEST.md).

Every figure below is measured on one box (Ryzen 9 9950X3D, 16c/32t, 186 GiB, Go 1.26) from the shipped binary. Nothing is projected.

**Scale + memory**

| | measured |
|---|---|
| connections, one node | **100,000,000 in 102.93 GiB**, connected in 70 s |
| cost per connection | **698 B heap / 1.08 KiB RSS** — flat from 5M to 100M |
| fan-out ceiling, in-process | **~670k sends/sec** and **~3.9 M deliveries/sec** — two ceilings, whichever binds first |
| out-of-process service, over a socket | harness not yet correct — no figure I trust |
| breaker under 10× overload | sheds **93%**, all 1M conns live, heap bounded at 704 MiB |

A node's memory is `fixed + N × marginal`, and only the marginal figure multiplies. Per-connection cost is **flat across a 20× range** (5M → 100M), which is what makes capacity planning arithmetic rather than extrapolation: a 64 GiB node holds ~60M connections.

**Throughput is bounded by active senders, never by held connections.** With 1M senders held constant, delivery rate stays at 3.04–3.36 M/s whether the node holds 5M or 100M connections — idle connections cost RAM, not throughput.

**Sharding the registry** (`-shards 1` = the unsharded shape, reproducible from the shipped binary)

| clients | unsharded | sharded (64) | speedup |
|---|---|---|---|
| 10,000 | 1.49 M/s | 1.78 M/s | 1.2× |
| 100,000 | 1.45 M/s | 2.34 M/s | 1.6× |
| 1,000,000 | **1.57 M/s** | **3.70 M/s** | **2.4×** |

Unsharded is flat at ~1.5 M/s regardless of scale — the textbook single-bottleneck signature. Sharded rises. Connect at 1M goes 2.70 s → 872 ms, and heap per connection is identical either way, so sharding costs near nothing in memory. The contended microbenchmarks show the convoy directly: `RegistryConnect` 1642 → 419 ns/op, `SeqAllocatorContended` 106.8 → 11.1 ns/op.

**Friend presence** (100k conns, Discord shape, 30% of friends online)

*Methodology:* each user's friend count is sampled from a **log-normal** (σ 0.9, deterministic in the user index via splitmix64), so the graph is heavy-tailed the way a real one is — most users have a handful, a few have many — rather than a uniform constant that would hide the tail this section is about. A log-normal's mean sits above its median by e^(σ²/2) ≈ 1.5×, so **median 30 means mean ~43**, and the totals below run on the mean: `100,000 × 43.46 × 0.30 online = 1,303,758` predicted vs 1,303,044 measured. *Online friendships* counts directed **subscriptions** (i subscribes to online friend j's presence topic), not undirected pairs — it is what the node holds, not a social statistic.

| | median friends | mean friends | online friendships | heap/conn | RSS/conn |
|---|---|---|---|---|---|
| no friend presence | — | — | 0 | 1.73 KiB | 5.42 KiB |
| unbounded graph | 30 | 43.5 | 1,303,044 | 3.34 KiB | 10.76 KiB |
| unbounded graph | 300 | 437.8 | 13,136,552 | 12.59 KiB | 32.85 KiB |
| unbounded graph | 3000 | 4381.0 | 131,443,329 | **98.90 KiB** | **202.35 KiB** |
| 50-friend frame | 300 | 437.8 | 4,520,151 | 6.60 KiB | 21.19 KiB |
| 50-friend frame | 3000 | 4381.0 | 4,999,272 | 6.94 KiB | 22.04 KiB |

~76 B heap per online friendship, linear, no ceiling: friends-off to median 3000 is **37× per-conn RSS** for the same connection count (0.54 → 19.3 GiB). A 10× larger friend graph (300 → 3000) costs **6.2× unbounded and 4% framed**, and framed subscriptions saturate at 4,999,272 ≈ 100,000 × 50 — **the frame, not the graph, is what the node pays for.** At median 30 nothing changes, because a typical user's ~13 online friends already fit; you only pay where the tail was hurting.

**In-process vs. a socket**

A downstream service either runs **in-process** — the hub calls `Handle`, a function call — or **out-of-process**, over a socket. They are bounded by different things.

In-process has two ceilings, and both must hold. Sweeping room size at 1M clients:

| room size | sends/sec | deliveries/sec |
|---|---|---|
| 1 | **669,306** | 669,306 |
| 10 | 351,134 | 3,511,345 |
| 50 | 78,617 | **3,930,839** |

Sends cap near **~670k/sec**, deliveries near **~3.9 M/sec**. A send costs 4–6× a delivery — it carries the parse, route, lookup, and encode, and that encode is shared across recipients. So a wider room amortizes the expensive half over more cheap halves: deliveries/sec rises, sends/sec collapses.

A socket has one ceiling — **sends** — and fan-out width is free to it, since the hub fans out locally either way. So wide fan-out is the hub's expensive case and the socket's cheap case. The Discord run is the extreme: 124,080,000 deliveries at 3.78 M/sec, from **3,049 sends/sec**.

**Where that socket ceiling sits, I don't have a number I trust.** The harness has produced a socket beating a function call on both latency and throughput, which is not a thing that happens, and each fix moved the figure substantially. At saturation it leaves ~24 of 32 threads idle, so it is measuring something other than the socket. It can't settle ws vs gRPC either: a harness bottlenecked on something both transports share reports them as identical, which is what it reports — the null result is evidence of the defect, not a finding despite it. What does survive is the pool sweep, because it runs against a service with a deliberate 1 ms per-message cost, so the service is the bottleneck by construction: one connection sheds 65.7%, four shed 0%. And gRPC's stock 64 KiB flow-control windows shed 71.5% where ws sheds 0% — a difference in whether the queue overflows, not a rate.

**Room size is an assumption, not a finding.** The suites run at 10 by default and 250 for the Discord shape, and nothing in the design argues for either. §1 lists the scale target as needing input; fan-out width belongs on that list.

**Two costs no benchmark covers, and they don't depend on any number above.** Both shapes enqueue and both shed — §4.2 forbids `Handle` from working inline either way — but in-process there is no connection to drop, no reconnect, and no breaker. And a service wanting a big heap or a GPU affects the hub from inside its process through GC and scheduler contention; over a socket it doesn't. These are the durable reasons to move a service out.

# 5 Questions for the backend team

1. **Delivery guarantees and persistence:** do we need at-least-once plus resume for chat in v1, or is at-most-once acceptable to start?
   *Options:* (a) **at-most-once** everywhere first, add durability later; (b) at-least-once for chat/notifications/moderation from day 1.
   *My lean:* (a), for the fastest correct core, with the class-policy table (§4.3) already in place so upgrading a class is a config change plus an outbox, not a rewrite.
   *What would change my mind:* if product treats a dropped chat message as a data-integrity bug rather than a UX blip, I'd build the outbox in v1.
2. **Target scale:** how many concurrent connections are we sizing for?

# 6 Questions for the frontend team

1. **Reconnect and resume semantics:** will the client hold a `resumeToken` and dedup by `id`, or should the server guarantee no duplicates?
   *Trade-off:* client-side dedup lets the server stay simple (**at-least-once** plus resend); server-side no-dup pushes complexity and state onto the hub.
   *My lean:* the client dedups by `id` — cheap, robust across reconnects. I'd want to hear how much per-service state frontend is comfortable tracking.
2. **Multi-device and duplicate connections:** is one user allowed on multiple devices at once, and what happens to the older connection on reconnect?
   *Options:* **last-writer-wins** — the newer connection wins via the `connID` generation counter (§6.1) — or true multi-device fan-out keyed by `(clientID, deviceID)`.
   *Impact:* this changes the registry key, the presence model, and the reconnect UX. As much a product decision as an engineering one, so I'd want frontend and product in the room.
   *Related:* I'd want frontend to co-own the `type` catalog and confirm additive-only evolution works for their release cadence.
