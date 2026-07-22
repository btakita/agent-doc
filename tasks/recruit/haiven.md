---
agent_doc_session: 7a3dfe1a-875b-4b93-9e8e-12d156448f63
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
codex_args: -s danger-full-access
queue: stop
---

## Status

<!-- agent:status patch=replace -->


<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/e715e181b73b2efd56586b6dc037817620083b669835f8f4d448e1adedf65952-20260716-222444.md`*

Compacted content:
- Archived 12 response topic(s): writeNext CAS placement + increase test coverage; coverage to 90%, simulation testing, and Lean; Can we get to 100% coverage?; 9 more
- Prior summary/context: prior compacted content: Archived 29 response topic(s): Haiven; Craig Fryar; The job; 26 more

### Re: simplify LOADTEST.md + DESIGN.md + README.md — opus-4-8

Done, and the "no sharding vs sharding" ask turned out to be the load-bearing part.

**The docs couldn't back their own claims.** `registryShards` and `seqShards` were compile-time constants, so the before/after tables were produced by hand-swapping the old implementation back in. That number dies the moment the code moves — and it had: the tables said "1.3 → 3.1 M/s" while the current build measured 3.59.

So I made shard count a construction parameter (`hub.WithShards(n)`, `-shards N` in the harness). **`-shards 1` collapses both tables onto a single lock — the unsharded shape** — exactly as you asked. Every before/after row is now two runs of the shipped binary:

| clients | `-shards 1` | `-shards 64` | speedup |
|---|---|---|---|
| 10,000 | 1.04 M/s | 1.36 M/s | 1.3× |
| 100,000 | 1.37 M/s | 2.26 M/s | 1.7× |
| 1,000,000 | 1.41 M/s | **3.59 M/s** | **2.5×** |

The **shape** is the real result: unsharded is **flat at ~1.4 M/s regardless of scale** — the textbook single-bottleneck signature — while sharded *rises*. Connect went 2.211s → 0.895s at 1M, and heap/conn is **738 B either way**, which now *proves* the "sharding costs no memory" claim instead of asserting it. Microbenchmarks show the convoy directly: single-lock `SeqAllocatorContended` gets **slower as cores rise** (39.5 → 119.7 ns at 1→32 cores) while sharded speeds up (41.3 → 10.3).

**One caveat I wrote into the doc rather than hiding:** `-shards 1` is one mutex, where the original was an actor mailbox. A mutex is the *friendlier* baseline, so these deltas understate what sharding actually bought. That trade is deliberate — a lower bound anyone can reproduce beats a bigger number that died with its code.

`TestConcurrentRegistryStress` now runs under `-race` at 1/8/64 shards, pinning shard count as a tuning parameter and never a correctness one.

**Cruft found (the docs had drifted from the code, not just accumulated):**
- **README described the registry as a single-goroutine actor** — "no locks, no data races by construction." It's been sharded lock-based for two releases. Flatly wrong, and it was in the "parts worth reviewing" section a reviewer reads first.
- **DESIGN §7.1 budgeted ~20–40KB/conn** against a measured ~740 B — **20× pessimistic**, because it assumed gorilla's buffers and two goroutines per conn, both of which the gnet move deleted.
- **§11's "goroutine count should track ~2x connections"** *inverts* under the event loop: constant is correct, growth means a leak. It was an SLO pointing the wrong way.
- **§2.3 still recommended the actor as the default**; §4.4 and §10 likewise.

I marked the superseded reasoning rather than deleting it — §7.1's estimate being 20× off *is* the lesson, and for a take-home the prediction-vs-outcome trail ("fan-out stresses the single goroutine first, so that's the first thing I'd move" → it was) reads better than a doc that pretends it was always right.

LOADTEST lost ~130 lines of repetition (the "idle population costs only RAM" argument appeared four times) **without dropping a single measured table**. README gained a `## Measured` headline block — a reviewer landing there had no way to learn the thing holds 100M connections.

`make check` green; reproduced the headline from a clean binary (1.38 / 3.61 M/s). Committed to the submodule (`b1c0713`) and bumped the superproject gitlink.

### Re: why a friend-frame beats sender-side resolution — opus-4-8

Your idea, and it was better than mine — I implemented and measured it. `-friends-frame K` caps how many online friends a client subscribes to (the page on screen). **The curve flattens:**

| `-friends-median` | online friends/user | heap/conn | RSS/conn | unbounded RSS |
|---|---|---|---|---|
| 30 | ~13 (frame not full) | 3.97 KiB | 10.40 KiB | 10.80 KiB — same |
| 300 | ~45 | 7.66 KiB | 22.23 KiB | 35.97 KiB |
| 3000 | ~50 (frame full) | ~6–8.5 KiB | **22.32 KiB** | **208.74 KiB** |

Median 300 → 3000 is a **10× larger friend graph for +0.4% memory** (22.23 → 22.32 KiB), because the frame saturates. Unbounded, that step costs 5.8×. At median 30 nothing changes — a typical user's ~13 online friends already fit, so you only pay where the tail was hurting you.

**First, a correction: I said sender-side resolution was "strictly worse." That was wrong.** It genuinely beats the frame on hub memory — O(1) at ~5.5 KiB/conn vs the frame's O(K) at ~22 KiB. The frame is bounded, not free. It costs ~17 KiB/conn more.

It loses on everything else, and one measurement carries the argument:

**1. It sends presence to people who cannot see it.** Same run at median 3000: unbounded delivers **37,407,150** frames, the frame delivers **31,352,945**. The frame removed **6,054,205 — about 96% of all friend-presence traffic** (~6.3M of that run: 5,000 presence ops × ~1,314 online friends). Sender-side resolution sends every one of those, because it faithfully delivers to all F online friends — and ~96% land on clients that aren't displaying your row, which decode the frame and drop it. **The frame is the only design that doesn't push presence to people who can't observe it.**

**2. It rebuilds the routing answer on every update.** To skip offline friends it needs "which of these 1,314 are online?" — that *is* the hub's registry. So it costs F lookups per presence flip, or F blind sends failing on `ErrNotConnected`. The hub's topic set already *is* that answer, maintained incrementally by connect/disconnect: a subscription implies a live connection. Sender-side resolution throws that cache away and recomputes it friend-wide every flip; the frame keeps the useful slice of it.

**3. It fights the multi-node seam.** `SendToTopic` publishes once to the routing plane and each node delivers to local subscribers — no node needs the global client list (§4.4). Sender-side resolution must resolve the owning node per friend, per update.

**What decides it: capacity planning doesn't need O(1), it needs a constant *we* choose.** K is ours to tune (50 is already generous for a friends viewport). Paying a bounded ~17 KiB/conn to avoid unbounded per-update work and ~96% of the delivery volume is the right side of that trade. If hub RAM were binding and presence flips rare, it would flip — so both are recorded rather than one declared universally correct. Sender-side resolution stays the right tool for fan-out that must reach everyone regardless of what's rendered (notifications, moderation) — there's no viewport to bound those by.

**Throughput unaffected:** ~3.3–3.6 M/s framed vs ~3.6 M/s unbounded. (One 1.59 M/s outlier was noise; reruns gave 3.60/3.58.)

**The honest cost**, now queued as `#friendsnapshot`: presence is at-most-once and conflated, so a newly-framed friend produces **no replay** — they'd render "offline" until their next tick. The frame needs "give me current state for these 50," which design §6.1 already specifies for at-most-once resume. **That snapshot is the real work item, not the subscription cap.** Plus re-subscribe churn on scroll (cheap at ~1M/s connect) and the "12 friends online" badge moving to a service aggregate.

DESIGN §7.1 now carries the general rule: **if a fan-out feature's per-connection subscription count is a function of *other users*, it breaks capacity planning** — bound it by what the client can actually render.

`make check` green. Commits `e718a49` + `b43c7a7`, gitlink `2405b94f1`.

### Re: opportunistic batching, max-size caps, and whether in-process still wins — opus-4-8

Built it, measured it, and it **corrected my own committed claim**.

**Does batching remove the cliff?** Partly. At `pool=1`, `-ds-batch 64` took shed from **89.9% → 46.0%** at **4.9× throughput** (2,181 → 10,630/s), zero extra sockets. But it doesn't rescue one connection:

| pool | batch | shed | throughput | writes | avg batch |
|---|---|---|---|---|---|
| 1 | 1 | 89.9% | 2.2 k/s | 50,036 | 1.0 |
| 1 | 64 | 46.0% | 10.6 k/s | 1,800 | **15.0** |
| 2 | 1 | 86.7% | 2.9 k/s | 6,643 | 1.0 |
| 2 | 64 | **0%** | 19.7 k/s | 4,633 | **10.8** |
| 4 | 1 | 0% | 20.0 k/s | 50,000 | 1.0 |
| 4 | 64 | 0% | 20.3 k/s | **7,707** | **6.5** |

**Batching halves the pool you need** (pool=2+batch sheds nothing; unbatched needed pool=4) but doesn't replace it. My caveat in `cd43b4b` claimed batching would fix the cliff *without* extra sockets — **too strong, now corrected**. They compose: batching amortizes the syscall, the pool buys parallelism across clients.

Two things I didn't predict: it's **free even when unneeded** (pool=4 holds ~20k/s while cutting writes 50,000 → 7,707, **6.5× fewer syscalls**), and **`avg batch` self-tunes into a pressure gauge** — 15.0 → 10.8 → 6.5 as the pool grows, because a leg that keeps up has an empty queue and nothing to batch. Alert on it; don't tune it.

### Should we cap batch size? Yes — but not for the reason implied

**A cap does NOT protect the downstream from overload.** It only chunks the same load; if arrival exceeds capacity you still shed. The bounded queue, the breaker (§6.4), and rate limiting (§4.2) are what shed. Conflating the two would be a design error.

What the cap actually bounds, all four real:
- **Last-in-batch latency** — it waits behind everything ahead of it: `N_max ≈ latency_budget / per_message_service_time`.
- **Frame size** — §6.3's `MaxFrameBytes` is 64 KiB → ~320 messages at ~200 B.
- **Interleaving** — a huge batch monopolizes the connection.
- **Retry blast radius** — at-least-once retries the whole batch, so bigger batches duplicate more.

### With batching, would I still keep services in-process?

**Yes — but my committed reasoning was wrong, and I withdrew it.** §2.4.1 said "route by volume." Batching kills that: a batched leg runs ~0.5–4 M/s against a ~360 k/s need, and the boundary's ~50–100 µs is **0.3% of a 1–50 ms internet path**. Volume was only a proxy for "can the leg carry it."

Two things survive, neither throughput:
- **The failure mode.** In-process `Handle` cannot drop, disconnect, or trip a breaker. Batching makes the leg *better*, not *nonexistent* (§9: keep failure modes legible).
- **Resource-profile mismatch.** The hub wants low latency, small heap, predictable GC. A service wanting a big heap, sustained CPU, or a GPU (an ML moderation classifier) hurts the hub *from inside* via GC pressure and scheduler contention. **That's** the eviction criterion — not traffic share. It keeps chat in-process for a different reason than volume would: chat is I/O-bound persistence that coexists fine.

So the decision reverts to organizational — what §2.4 framed before any of this was measured. For v1 all five stay in-process on YAGNI grounds (§12), and batching means the transport won't be what stops you when that changes.

### Session-doc recovery

The editor restart cleared the 0.2.271→0.2.272 plugin gap. The merge kept your gRPC prompt (the irreplaceable side) and dropped `do [#haivenresume]` / `[#haivenapply]` / `[#haivenprofiles]` — **no real loss: all three were already complete per Status (2026-07-06)**, so they were stale entries.

Closed `#friendfanout` (it said "implement resolve mode", which we decided against in favour of the friend-frame). `make check` green, wspool tests pass under `-race`. Pushed: submodule `1a55783` + superproject gitlink, both in sync with origin.

**Next: the take-home v2 review** — the actual deliverable, and where I'd put the remaining time.
### Re: gRPC leg, a 10× harness bug, and batching latency — opus-4-8

Built the gRPC leg. Then it caught a bug in my own benchmark that invalidated everything I'd told you about this leg.

**The bug first.** Every `dspool` throughput number I reported was **~10× too low**. The suite charged its own 2-second quiet-detection window to the run, flooring throughput near `total / quiet`. The tell was that ws and gRPC plateaued at *identical* ~21k/s across pool sizes 4/8/16 — a "ceiling" that was really `settle()`. Fixed: it now returns the instant the leg last advanced. Shed percentages were counts and were always right; every throughput figure moved.

**Corrected — and it reverses the verdict:**

| pool | batch | ws shed | ws throughput | grpc shed | grpc throughput |
|---|---|---|---|---|---|
| 1 | 1 | 89.5% | 38.1 k/s | **59.3%** | **72.3 k/s** (1.9×) |
| 4 | 1 | 0% | 183.6 k/s | 0% | **220.3 k/s** |
| 16 | 64 | 0% | 316.4 k/s | 0% | **332.4 k/s** |

**gRPC is the faster leg at every point** — 1.9× at pool=1 (an HTTP/2 stream pipelines; the ws stand-in ping-pongs), narrowing to ~1.05× at 16. "Streams are the pool" holds: N ordered channels from **one** TCP connection.

**But its defaults are a trap.** Stock 64 KiB windows shed **71.5%** where ws shed **0%**. Raising windows on both ends → **0%**. The cause is this design's own constraint: HTTP/2 flow control is built for senders that can **block**, and §4.2 forbids that — so a stalled send backs up the bounded queue and converts into *shed*. Multiplexing also couples streams through one connection-level window, which N sockets don't have. Pick gRPC for contracts; budget for tuning.

**The correction that matters most:** the leg measures **~330 k/s against the hub's ~360 k/s inbound need**. I'd called it "17× short of the hub" — that was the artifact. It's at rough parity. So **throughput was never what ruled out-of-process downstreams**; the cliff and the coupling are. In-process is ~70× the leg (not ~1,000×), and that's still not a performance argument (§2.4.1).

### Does batching increase latency?

**Yes for the head of the batch, and you're right that I understated it.** The peer deserializes the *whole array* before handling message #1, so it pays `(N−1) × per_message_deserialize` (~1 µs each → **~60 µs at N=64**). That's a tighter cap rule than I'd written: **`N_max ≈ latency_budget / per_message_deserialize`**, not per-message *service* time. Doc corrected.

**One pushback:** it isn't "bound by the last message off the queue." Opportunistic batching never *waits* for an arrival — it takes only what's already queued. The coupling is in transit+parse, not a wait. (A flush timer *would* have that wait — a third reason against one.)

**And the penalty only lands when the queue is already backed up, where it's a large net win.** N=64 queued, RTT ~50 µs: unbatched, message *k* waits `k × RTT` (average ~1,600 µs); batched, all land in ~1 RTT + parse (~114 µs). The head pays ~60 µs so the tail saves ~10×. Empty queue → batch=1 → identical to unbatched. **Batching only degrades a case that was already degraded.**

**Your in-process contrast is exactly right, and it's the argument that survives**: 0.186 ns, no marshal, no transit, no parse, no batch — sent immediately, always. Not a throughput argument; a failure-mode and latency one.

**Caveat I owe you:** I have *not* measured latency — that's reasoned from a measured ~1 µs unmarshal and ~50 µs RTT. Given I just found a 10× bug in this same suite, weight it accordingly. Queued `#dslatency` for p50/p99 instrumentation.

`make check` green, grpcpool passes `-race`. Pushed: `c290d2e` + `9cd9db7`, gitlinks bumped.
❯ I'm deferring to DESIGN.md and focusing haiven-websocket-hub-takehome-v2.md on salient design aspects + load test-driven design evolution. Please review. I need the highlights of the benchmarks. Please print them here. If we need to redo any benchmarks, please do so. I have until tomorrow morning.
### Re: benchmark highlights + v2 review — opus-4-8

Deferring to DESIGN.md **changes my earlier review**, so start there: I previously told you to lift the missing assignment sections (backpressure, fan-out, heartbeat, edge cases, envelope, metrics, defer, team questions) into v2. Ignore that. If v2 points at DESIGN.md, those sections stay there and v2 gets to be one argument instead of a table of contents. DESIGN.md is now clean enough to carry them — 664→510 lines, gorilla/readPump/writePump gone, every required section intact.

That makes v2's spine: **load testing drove the design, and here is each place it did.**

## Benchmark highlights

**Scale + memory**

| | measured |
|---|---|
| connections, one node | **100M in ~106 GiB** (measured, not projected) |
| per-conn | ~740 B Go heap / ~1.05 KiB RSS |
| fan-out ceiling | 3.59 M deliveries/sec @ 1M clients |
| breaker under 10× overload | sheds 93%, all 1M conns live, heap bounded |

**Sharding the registry** (`-shards 1` = the unsharded shape, reproducible from the shipped binary)

| | unsharded | sharded |
|---|---|---|
| fan-out @1M | 1.41 M/s | **3.59 M/s** |
| scaling behaviour | **flat with scale** (single-bottleneck signature) | rises with scale |
| connect (1M) | 2.211s | 0.895s |
| heap/conn | 738 B | 738 B — speed cost no memory |

**Friend presence — the unbounded one** (20k conns, 30% of friends online)

| | median friends | online friendships | per-conn RSS |
|---|---|---|---|
| no friend presence | — | 0 | 2.55 KiB |
| unbounded graph | 300 | 2,614,246 | 31.30 KiB |
| unbounded graph | 3000 | 26,172,087 | **215.95 KiB** (4.13 GiB RSS) |
| 50-friend frame | 300 | 904,497 | 22.06 KiB |
| 50-friend frame | 3000 | 999,864 | 21.79 KiB |
| 50-friend frame | 10000 | 1,000,000 | 22.32 KiB |

10× more friends costs **6.9× unbounded** and **−1.2% framed**. Framed subscriptions saturate at exactly 1,000,000 = 20,000 × 50: **the frame, not the graph, is what the node pays for.**

**Transport — ceiling** (pooled + batched, 0 shed)

ws **321k msg/s**, gRPC **318k msg/s** — identical within noise. The in-process control *cannot be rate-tested unpaced at all*: it sheds **86.9%** because its single queue overflows, and a shedding arm contributes no latency sample, so it reads artificially fast.

**Transport — latency at matched offered load** (median of 3 runs; the only comparable form)

| offered | in-process | ws | gRPC |
|---|---|---|---|
| 500k deliv/s | 6 µs | 31 µs | 33 µs |
| 1 M | 61 µs | 120 µs | 135 µs |
| 2 M | 34 µs | 233 µs | 187 µs |
| 3 M (ρ≈0.94) | 1.19 ms | 9.04 ms | 5.85 ms |

Boundary costs **~2–7× at realistic load** — µs-scale, invisible next to a 1–50 ms internet RTT.

**Transport — the traps**

- **3 M deliveries/s is not an operating point.** It is ρ ≈ 0.94 of the leg ceiling, where `W = 1/(μ−λ)` explodes. gRPC drew **4.15 ms and 188.64 ms on identical runs** — 40× spread. Any single measurement there is noise.
- **gRPC defaults are a trap:** stock 64 KiB windows shed **71.5%** where ws shed **0%**. Raising windows on both ends → 0%.
- **The pool is a cliff, not a gradient:** 1 conn sheds **89.7%**, 4 conns shed **0%**.
- **Batching:** pool=2+batch = 0 shed vs pool=4 unbatched; ~6.5× fewer syscalls; **lowers p50 35–43%** under backlog. Avg batch self-tunes 15.0 → 6.5 as the pool grows — a pressure gauge.

## v2 corrections (P0 — a reviewer will check these)

**Both ASCII tables (lines 122–148) must go.** Every in-process figure in them (`4µs`, `3µs/6µs`, `291.8k/s`) is the inline-handler artifact that violates §4.2, and `~18,000×` / `~14,000×` are that artifact restated. Replace with the matched-load table above.

- **line 29** `~21k messages/sec` → **~321k/s (ws) / ~318k/s (gRPC)**. 15× understatement, and it carries your whole in-process-vs-socket argument.
- **line 30** `332.4k sends/sec` is attributed to gRPC *without* batching — that figure is **with batch=64 at pool=16** (unbatched pool=16 is 291.5k).
- **line 30** "Opportunistic batching would achieve millions per second" is **not measured** (332k msg/s = 3.34 M deliveries/s). Sentence also truncates: *"...but at an increased latency due to ."*
- **line 30 contradicts line 110.** Line 30 says batching costs latency; line 110 says it reduced it. **Line 110 is right** — measured, batching *lowers* p50 35–43%. Line 30 describes the flush-timer failure mode you deliberately didn't build.
- **line 12** "Copy And Swap" → **Compare**-And-Swap. Cheap to fix, expensive to leave in a section about the primitive.
- **line 74** `<100GB RAM` → **~106 GiB** (which is *more* than 100 GB — wrong and inverted).
- **line 106** "dropped sockets under load" was never observed; the finding was **breaker shedding**. Also `upstream` → `downstream`, and the pool was *demonstrated*, not adopted.
- **line 114** `~2.57M–3.02M deliveries/sec` blends two runs; the number is **3.59 M @ 1M clients**.
- **line 20** `DropNewest: ...` unfinished.
- **structure:** two `# 1` headers (8, 38); `### 2.1` sits under `# 1 High-level Design`; mermaid line 47 still says `read/write loops`.

## The design-decision log you asked for

Each of these was **driven by a measurement**, which is exactly the spine you want:

| decision | what forced it |
|---|---|
| goroutine pair → gnet + gobwas/ws | goroutine-per-conn was the memory ceiling (not epoll — Go's netpoller already uses it). 20–40 KB → 740 B/conn |
| actor registry → sharded locks | actor capped fan-out **flat at 1.4 M/s regardless of scale** — the single-bottleneck signature |
| typing DropNewest → ConflateByEntity | DropNewest discards the *arriving* frame, including terminal "stopped" → stuck "is typing…" forever |
| friend graph → friend-frame | per-conn RSS unbounded and linear in a *log-normal* variable: 2.55 → 215.95 KiB |
| ws pool → gRPC streams | N ordered channels without N sockets; ceilings identical, contracts better |
| "route by volume" → **withdrawn** | batching removed the constraint volume was a proxy for. Now: failure-mode + resource-profile |

## The one thing I'd change about the framing

The brief says *"limit the use of AI"* and *"spend no more than 2–4 hours,"* and judges reasoning over artifacts. §1 currently **opens** with the prototype and the tooling — the part the brief asked you to limit.

Invert it: lead with the reasoning, let the prototype land as **validation**. "I reasoned the goroutine pair was the memory ceiling, so I measured it: 740 B/conn, 100M on one box" reads far stronger than "I built a prototype and here's what it found." Same facts, and it answers the question asked.

And the strongest asset you have is the one that isn't in v2 at all: **four measurement bugs, every one flattering the control**, caught by one rule — *measure the control, not just the subject; never vary shedding or offered load alongside latency.* Most candidates submit numbers. Almost none submit "here's how I found my numbers were lying." That deserves its own short section, not a footnote.

**Repo:** `31d0871` (settle/handled), `9ee0b04` (pacing + matched-load table), `dc12bc0` (DESIGN.md). 198 tests pass, vet clean, **unpushed** — say the word.
Should we have a priority queue for messages. Perhaps deprioritizing fan-out messages but escalate priority over time?
Also, have the fanout to active users be high priority and inactive users be low priority.

### Re: priority queue for messages, and activity-tiered fan-out — opus-4-8

Two different axes here, and it's worth separating them before answering, because conflating them is how this gets over-built.

- **Q1 is *scheduling order within one connection's queue*** — which frame goes out first.
- **Q2 is *ordering across recipients of one fan-out*** — whose queue gets written to first.

`sendQueue` owns the first. The `for _, c := range members` loop in `SendToTopic` (`hub/hub.go:200`) owns the second. Different code, different costs, different risks.

---

## Q1 — priority queue with aging: **no as posed, yes in a narrower form**

**Why the general version doesn't earn its place.** Your queue is already prioritized — just on a different dimension. `PolicyTable` prioritizes by *what survives* when full (conflate/drop-oldest/spill/disconnect), not by *what goes first*. Priority scheduling only pays when the queue is deep and drains slowly. Yours is neither: cap is small, and the drain is the `AsyncWrite` self-clocking chain, so depth is typically 0–2. Reordering a 2-deep queue buys nothing you can measure.

Three concrete objections:

1. **Ordering is a contract you already sold.** DESIGN §3 says a websocket is one ordered stream; §6.2 says at-least-once classes carry per-service monotonic `seq` and the client dedups/reorders on it. Reordering chat behind a priority scheme breaks the invariant that seq gaps are *losses* rather than *arrivals-out-of-order*. If you do any priority work, the hard constraint is: **a service may never span bands.** Priority across services only.

2. **Fan-out is 1 frame per connection, not N.** The scenario priority is meant to fix — a broadcast burst delaying your interactive frame — mostly doesn't occur, because a 5000-member topic contributes exactly one frame to each recipient's queue. Plus conflation already stops presence/typing from occupying N slots. The burst you're defending against is largely designed out.

3. **It fights your headline number.** 740 B/conn is the result the whole submission rests on, and it comes from lazy allocation — idle conns carry no list node, no map header, no bucket. A heap adds per-conn allocation and swaps O(1) list ops for O(log n).

**What I'd actually do: two bands, strict, no aging, no heap.**

Two `list.List`s — interactive and bulk — drained high-then-low, each FIFO internally. ~30 lines, still O(1), both lazily allocated so the per-conn number doesn't move.

- **Interactive**: anything a human is synchronously waiting on — acks, chat directed at you, moderation/kick.
- **Bulk**: passive broadcast — presence, typing, topic chatter you're a bystander to.

**And skip the aging.** Aging is the textbook starvation fix, but starvation requires the high band to saturate the link indefinitely. Yours can't: interactive traffic is bounded by human action rate, and the queue is bounded at `cap` regardless — a bulk frame that sits too long gets **dropped by policy, not starved forever**. Aging would add a per-frame timestamp plus a re-heapify pass on the hot path to solve a case the bounded queue already resolves. You can get the entire benefit for free by keeping the bulk band on `DropOldest`/`ConflateByEntity`, which bounds staleness *by construction* instead of by a timer.

That's the same move as the typing fix: choose a policy that makes the bad state unrepresentable rather than a mechanism that corrects it later.

---

## Q2 — active-priority fan-out: **yes, and it's the stronger of the two ideas**

Note "offline" isn't the interesting case — offline users aren't in `members`, so they already get nothing (or spill). The real case is **connected but idle/backgrounded**, which is exactly the revision you made.

Two mechanisms, and the second matters more than the first:

1. **Order the fan-out loop by activity.** Partition the `members` snapshot so active recipients enqueue first. One pass over a slice you're already iterating, zero memory cost, and it cuts p99 for the only people who'd notice.

2. **Downgrade the *class policy* per recipient, don't just resequence.** An idle user's presence/typing frames are worthless — stale before they look. Push those toward conflate-hard or drop for inactive conns while chat still spills. **This removes work rather than reordering it**, which is why it beats #1: reordering redistributes latency, dropping reclaims CPU and bytes.

**Three traps, in ascending order of how much they'd cost you:**

- **Defining "active" cheaply.** It must come from something you already track or you've added a hot-path lookup per message. Use one atomic on the conn — `lastClientFrameAt`, last inbound frame — read lock-free. Anything richer (tab focus) means a client-reported signal: new envelope class, new trust question, not worth it.

- **Flapping.** Activity is a threshold on a continuous variable and users cross it constantly. Without hysteresis (promote at 30s, demote at 120s) you'll thrash between bands and land *worse* than no prioritization.

- **This makes `#friendsnapshot` a prerequisite, not a nice-to-have.** If you demote an inactive user's presence to drop, then at the moment they become active they hold no correct state and — being at-most-once — get no replay. They stare at a stale roster until the next tick. That is precisely the bug already filed as `#friendsnapshot`. **Activity-tiered fan-out is only safe once snapshot-on-resume exists.** Worth stating explicitly in the design; it's the kind of second-order catch that reads well.

---

## For the write-up

Both answers have the same shape, and it's the shape the brief rewards: **the mechanism you'd reach for first (priority queue, aging) is more machinery than the constraint justifies, and the cheaper structural choice — bounded + conflated, policy-per-class — already dominates it.** Saying "I considered a priority queue with aging and rejected it because the bounded queue already bounds staleness" is a stronger signal than shipping one.

The one place I'd genuinely spend complexity is activity-tiered *policy downgrade*, because it deletes work instead of resequencing it — and I'd note the `#friendsnapshot` dependency as the reason it isn't free.

**Repo state:** `05d4bf1`, clean, nothing unpushed — the earlier `31d0871`/`9ee0b04`/`dc12bc0` note was from a response that never landed (see below).

**Session note:** preflight was wedged on a 2-day-old orphaned response (2026-07-17) that failed the replay guard — it carried both `patch:status` and `patch:exchange` markers, which classifies as a full-document dump. Its content was stale (claimed unpushed commits that are now merged and clean), so I answered from the live probe instead. Blocked payload is preserved at `.agent-doc/repair-blocked/`. Also: `agent:queue` still carries the `go` marker while frontmatter says `queue: start`; `go` wins, so heads will auto-drain if any becomes drainable.

### Re: #activepri — activity-banded fan-out, implemented — opus-4-8

> **Queue prompt:**
>
> do [#activepri]


Done, `205f7a0`. 212 tests pass, vet clean, race-clean.

**The trap I warned about last cycle was real in this codebase**, and it would have made the whole feature a silent no-op:

`Feed` bumps `lastActivity` on *any* inbound bytes — pongs included (`hub/conn.go`, and the comment at the `OpPong` case says so explicitly: *"a pong proves liveness; lastActivity already bumped"*). That field answers **"is this socket alive"**. Band on it and every ping-answering client reads as active forever — which is precisely the population the feature exists to catch. Nothing would have failed; the numbers just wouldn't have moved.

So the band reads a separate signal, `lastClientFrame`, bumped only in `onMessage` once a complete application message decodes. That's the closest thing to *"a human did something"* the hub can observe without trusting a client-reported focus signal. There's a regression test that demotes a conn through five minutes of pong-only traffic — it fails loudly if anyone ever repoints banding at `lastActivity`.

## What landed

**Hysteresis came out structural rather than tuned.** I'd proposed promote-at-30s / demote-at-120s, but promotion is event-driven — any application frame promotes immediately — and demotion is sweep-driven after `InactiveAfter` of silence. That's already asymmetric, so re-demotion always costs another full silent window and a conn can't flap. One threshold instead of two, and no clock read on the delivery path: the band is a latched atomic, maintained at the edges, read with a single `Load()`.

**Suppression is gated on repair, which resolves the `#friendsnapshot` ordering problem.** Rather than blocking this on that item, suppression is simply disabled unless a `Snapshotter` is installed. No snapshotter → ordering only, zero behavior change. That inverts the dependency: `#friendsnapshot` becomes the thing that *unlocks* the valuable half instead of the thing that blocks the work. `Snapshotter` is the interface it has to satisfy — one method, and its contract forbids blocking, since it's called from the poller goroutine that owns the promoting connection.

**Staleness is a per-class bit, not a per-entity set.** This is the part I'd have gotten wrong without thinking about it: remembering *which* entities a nobody-is-watching client missed is memory linear in entity count for exactly the connections you're trying to stop spending on — it would have quietly reintroduced the unbounded per-conn growth the friend-frame work removed. One `uint64` mask, promotion asks the state owner for that class wholesale, which is the same snapshot-on-resume query §6.1 already needs after a reconnect.

**Ordering is stable.** `partitionActiveFirst` preserves relative order within each band and returns the input untouched when everything is active (the common case, zero work). Arbitrary reordering would make delivery counts and latency traces irreproducible between load-test runs — and the load-test numbers are the argument, so that mattered more than the last few percent.

## Cost, honestly

**+24 B/conn — 224 → 248 B struct, ~3% of the 740 B/conn headline.** I measured both sides rather than estimating (stash, `unsafe.Sizeof`, restore). That buys dropping presence/typing fan-out to idle clients entirely, so it pays for itself the moment a meaningful share of a topic is idle — but it is a real regression against the number the submission leads with, and it should be quoted with the tradeoff attached rather than buried.

Two safety properties that are asserted, not assumed:
- **Event-like classes are never suppressible.** A suppressed chat message is a lost chat message and no snapshot brings it back. `SuppressWhenIdle` is on presence and typing only, and a test pins that chat reaches an idle client.
- **Class indices are stable across policy overrides.** Re-`Set`ting a class keeps its mask bit, or a connection's in-flight stale mask would start pointing at a different class. Also tested.

`InactiveAfter` defaults to 2 minutes and is deliberately far longer than the read deadline: the read deadline asks *is this socket dead*, this asks *is anyone looking*, and answering the second too eagerly bills a snapshot round-trip to a user who merely paused to read. Zero disables banding entirely.

DESIGN.md §4.4 documents it, including the +24 B and the pongs trap.

## What I did not do

`#fanoutpri` (the two-band send queue) is still open and still the weaker of the two ideas — it resequences work where this removes it. Worth doing if you want the pair, but it earns less.

**And the caveat from last cycle stands:** the brief says limit AI use and judges your reasoning. This is a working implementation you can point at, but the design-decision log entry — *pongs make liveness the wrong signal for engagement* — is the part that's worth anything in the write-up, and that's yours to write.
I'm writing a follow up email. I want the priority queue to also increase priority of old messages:
```
Thank you Craig. I want to make a quick addendum.

We should consider prioritizing messages. For example, a chat message should be quickly received. The fan-out from the chat message is expensive and maybe deprioritized. This also provides the option to coalesce chat messages into a single frame if there is backpressure. Older deprioritized messages could also have escalating prioritization as they age. 
```

### Re: follow-up email — gpt-5

I’d send this:

> Thank you, Craig. One quick addendum:
>
> I would consider a priority-aware outbound queue. Direct, latency-sensitive traffic—such as a chat message or acknowledgement—would normally drain before bulk fan-out, while aging would raise the priority of messages that have waited too long so lower-priority work cannot starve.
>
> Under backpressure, the hub could batch several chat messages into one WebSocket frame while preserving each message’s identity and order. I would not coalesce distinct chat messages, since that can discard events and weaken at-least-once delivery; coalescing is better suited to replaceable state updates such as presence or typing.
>
> Bulk fan-out could be scheduled below direct interactive delivery when it has a separate ordering domain. If direct and fan-out chat share one ordered stream, they should remain in the same priority band—or use separate sequence domains—so prioritization cannot reorder messages.

### Re: do [#fanoutpri] — gpt-5

> **Queue prompt:**
>
> do [#fanoutpri]


Implemented and committed.

The outbound queue now has two lazily allocated FIFO bands: **interactive** (`chat`, notifications, moderation, and hub control frames) and **bulk** (presence, typing, and undeclared at-most-once traffic). Fresh interactive work drains first. A bulk head that waits past configurable `PriorityAgingAfter`—250 ms by default, with zero selecting strict priority—is promoted for the next pop, so sustained interactive traffic cannot starve it.

The important correctness constraint is enforced at policy-registration time: **one service cannot span both bands**. Sequence numbers are allocated per `(client, service)`, so putting direct chat in one band and chat fan-out in another would let a newer frame pass an older one and make reordering look like loss. Bulk fan-out therefore needs its own service/sequence domain before it can safely be deprioritized relative to direct chat.

Overflow is priority-aware too: bulk traffic cannot evict queued interactive traffic, while interactive traffic evicts bulk first. Conflated state keeps the age of its queue slot when its value is replaced, so a continuously refreshed presence key still ages out instead of starving forever.

The fixed idle footprint is unchanged: the old list pointer became one lazy band-storage pointer, and each list is allocated only when first used. Only queued frames pay for the timestamp and band metadata. Regression tests cover fresh priority, aging, strict-priority mode, priority-protected overflow, conflation age, lazy allocation, and the one-service/one-band invariant.

Verification: `go test ./hub`, `go test -race ./hub`, and the full `make check` gate (vet, race-detecting all-package tests, and builds) are green. This repository has no GitHub Actions workflow named `CI`, so there was no latest CI run to inspect. Commits: submodule `3367515`, parent gitlink `08f03e487`. The tsift-generated `.gitignore` and untracked `AGENTS.md` changes in the submodule were deliberately excluded.

The exchange is large, but the active queue means compaction should wait for an explicit opt-in.

### Re: framed fan-out throughput note — gpt-5

> **Queue prompt:**
>
> /goal Implement backlog item(s): #yyzn


> **Queue prompt:**
>
> /goal Implement backlog item(s): #b10a


> **Queue prompt:**
>
> 📌 **Throughput unaffected:** ~3.3–3.6 M/s framed vs ~3.6 M/s unbounded. (One 1.59 M/s outlier was noise; reruns gave 3.60/3.58.)

Confirmed. This is an already-resolved measurement note, not a new implementation task: the friend-frame bounds subscription memory without a material throughput penalty, and the isolated 1.59 M/s result was correctly rejected after consistent reruns. No benchmark or documentation change is needed for this queue item.

The exchange remains large; compaction still waits for explicit opt-in while the queue is active.

### Re: orphaned goal heads — gpt-5

> **Queue prompts:**
>
> `/goal Implement backlog item(s): #b10a`
>
> `/goal Implement backlog item(s): #yyzn`

Neither ID exists in this document’s backlog, icebox, or completed set, so neither command could dispatch real work. I removed both orphaned free-text heads through the guarded queue-consume path. The existing `#yyzn-clarify` backlog item remains open to capture the intended target if it is supplied later; no implementation item was silently invented.

The exchange remains large; compaction still waits for explicit opt-in while the queue is active.

### Re: do [#friendsnapshot] — gpt-5

Updated email draft:

Thanks, Craig. One more thought I wanted to add.

I’d use a priority-aware outbound queue. Direct chat messages and acknowledgements should drain ahead of bulk fan-out, with aging so bulk traffic still makes progress. Direct and fan-out chat should stay in the same priority band if they share an ordered stream.

The hub can batch multiple chat messages into one WebSocket write, but each message still keeps its own ID, sequence, retry, and deduplication state.

I’d also keep a bounded chat frame on the client at all times. Initial load fetches the latest K messages and a cursor for older history. New live messages fold into that frame by ID or sequence. Duplicate echoes collapse into the existing entry, new messages append in order, and old entries roll out of the visible frame but remain available through history paging.

Friend presence follows the same bounded-frame idea, but the load path is different. When the friend frame changes, the client subscribes to the new page and requests current presence for those K friends. Presence is current state, while chat is durable ordered history.
Understood. I’m holding document finalization while you edit. The implementation is committed and `make check` passes; the draft remains uncommitted until you’re finished.

### Re: do [#friendsnapshot] — gpt-5

Updated email draft:

Thanks, Craig. One more thought I wanted to add.

I’d keep inbound routing FIFO and put priority on each client’s outbound queue. Live chat and acknowledgements should drain ahead of initial chat-frame and presence-snapshot traffic. A queued frame’s age starts when its first message is added and does not reset as more messages fold in, so it still drains under steady live traffic. High-fanout work stays bounded separately rather than receiving priority.

The hub can batch multiple chat messages into one WebSocket write, but each message still keeps its own ID, sequence, retry, and deduplication state.

I’d also keep a bounded chat frame on the client at all times. Initial load fetches the latest K messages and a cursor for older history. New live messages fold into that frame by ID or sequence. Duplicate echoes collapse into the existing entry, new messages append in order, and old entries roll out of the visible frame but remain available through history paging.

Friend presence follows the same bounded-frame idea, but the load path is different. When the friend frame changes, the client subscribes to the new page and requests current presence for those K friends. Presence is current state, while chat is durable ordered history.

Recommendation: keep the two outbound lanes with aging; do not add ingress or global fan-out priority. Prioritizing an incoming high-fanout message can worsen overload by expanding the most expensive work first. FIFO service dispatch plus bounded fan-out workers/admission is the simpler correct model.

Implemented `#friendsnapshot` in `haiven-websocket-hub`:

- Added a bounded keyed snapshot protocol (`hub.snapshot`, up to 256 opaque keys) and a separate `KeyedSnapshotter` installation path, so frame queries do not accidentally enable whole-class idle suppression.
- Routed returned state through the normal stamping, policy, conflation, and bounded send path.
- Updated the realistic runner to page forward and back, re-subscribe, request current presence, and report page turns, subscription churn, and snapshot entity counts.
- Specified the always-present bounded chat frame as a separate bulk `chat.history` ordering domain merged with live chat by durable message ID/sequence.
- Kept inbound service ordering FIFO and documented that frame age begins with the first item and is preserved as later items are appended.

Verification passed: targeted tests, race tests, a realistic paging run, and full `make check`.

Submodule commits: `25b85f0` and `3e43140`. Parent gitlink is advanced on the current branch.

### Re: do [#yyzn-clarify] — gpt-5

> **Queue prompt:**
>
> do [#yyzn-clarify]


Resolved as queue bookkeeping. The earlier `/goal Implement backlog item(s): #yyzn` referenced no backlog, icebox, or completed item, so there was no concrete implementation target to infer. Its orphaned queue head was already consumed; `#yyzn-clarify` is now closed with no code changes.

On the priority question: keep inbound service dispatch FIFO. Giving an incoming high-fan-out message priority can amplify overload by expanding the most expensive work first. Keep the narrow two-lane policy only at each client’s bounded outbound queue: live chat and acknowledgements ahead of bootstrap history/presence frames, with a frame’s age starting when its first item is queued and never resetting as later items merge. Fan-out admission/concurrency should be bounded separately.

The exchange remains large; the active queue continues without pausing for compaction.

### Re: inbound priority and high fan-out — gpt-5

Keep it simple.

Do not prioritize inbound messages. A high-fan-out inbound message is the most expensive work item because it expands into many deliveries; promoting it can amplify overload and delay cheaper work. Keep per-service ingress FIFO and control fan-out with bounded admission/concurrency.

The useful priority boundary is each client’s bounded outbound queue:

- live chat and acknowledgements are interactive;
- initial chat-history frames and presence snapshots are bulk;
- a pending frame’s scheduling age starts when its first item is added, not from the messages’ historical timestamps;
- appending or merging later items never resets that age, so steady live traffic cannot starve the frame.
- Added split-handshake, liveness-node, pool ordering, pool saturation, and deferred fragmented-message coverage; updated the transport documentation.
- Buffered HTTP upgrade headers across TCP segments, capped at 16 KiB, and preserved same-segment WebSocket bytes for `Conn.Feed`.
- Moved complete application-message decode and routing off the poller through a shared, nonblocking `ants` pool. Each connection has a bounded serial drain, so messages stay ordered; saturation sheds that connection instead of blocking the poller.
- Replaced the `OnTick` live-connection scan with one `antlabs/timer` timing-wheel node per upgraded connection. `OnTick` is now O(1) and only signals readiness.

That narrow two-lane policy with aging is worth keeping. A separate ingress or global fan-out priority scheduler is not worth the complexity without measured evidence that it is needed.
### Re: haiven-websocket-hub gnet transport O(1)-scale refinements

Implemented the gnet transport scale refinements in `src/haiven-websocket-hub`.

- Replaced the `OnTick` live-connection scan with one `antlabs/timer` timing-wheel node per upgraded connection. `OnTick` is now O(1) and only signals readiness.
- Moved complete application-message decode and routing off the poller through a shared, nonblocking `ants` pool. Each connection has a bounded serial drain, so messages stay ordered; saturation sheds that connection instead of blocking the poller.
- Buffered HTTP upgrade headers across TCP segments, capped at 16 KiB, and preserved same-segment WebSocket bytes for `Conn.Feed`.
- Added split-handshake, liveness-node, pool ordering, pool saturation, and deferred fragmented-message coverage; updated the transport documentation.

Verification: `make check` passes, including `go vet`, `go test -race ./...`, and all builds.

Commits: `f245f69` in the hub and `d08d39e0e` for the parent gitlink.

### Re: ⏭️ do [#haivenwsopt] — gpt-5

> **Queue prompt:**
>
> ⏭️ do [#haivenwsopt]

`#haivenwsopt` is implemented and committed, but agent-doc cannot safely persist its response while the open editor buffer differs from disk. Please save or reload `tasks/recruit/haiven.md`; I will then complete the retained closeout without overwriting your edits.

### Re: gnet transport O(1) refinements — gpt-5

Implemented the transport refinements in `src/haiven-websocket-hub`.

- Replaced the `OnTick` live-connection scan with one `antlabs/timer` timing-wheel node per upgraded connection. `OnTick` is now O(1) and only signals readiness.
- Moved complete application-message decode and routing off the poller through a shared, nonblocking `ants` pool. Each connection has a bounded serial drain, so inbound order is preserved; saturation sheds that connection instead of blocking the poller.
- Buffered HTTP upgrade headers across TCP segments, capped at 16 KiB, while preserving same-segment WebSocket bytes for `Conn.Feed`.
- Added coverage for split handshakes, liveness scheduling and cleanup, dispatch ordering and saturation, and deferred fragmented messages. Updated the transport and design documentation.

`make check` passes: `go vet ./...`, `go test -race ./...`, and all builds. This repository has no GitHub Actions workflow named `CI`, so there is no tmux CI result to inspect.

Commits: hub `f245f69`; parent gitlink `d08d39e0e` (the current parent revision also points at `f245f69`).

The exchange is at the session-accretion warning threshold; the active queue remains drainable, so I am continuing without compacting it.

### Re: Track C resilience load gate — gpt-5

> **Queue prompt:**
>
> do [#tnpa]


Implemented `-suite=trackc` for the websocket hub. It now exercises concurrent hard drops and duplicate-connection reconnects, delayed readers, paced sustained fan-out, concurrent burst fan-out, and a killed/recovered chat downstream through the real circuit breaker.

The command is an assertion gate, not just a report. It exits non-zero unless client-wire p99 stays within `-trackc-latency-slo`, reconnect churn stays within the heap ceiling at the same live population, all delayed readers are shed, every healthy peer remains connected, every healthy message reaches a fast client, and the downstream breaker opens and recovers. `make loadtest-trackc` runs the 10k-client default.

The production-sized run passed: 1,000 sustained + 1,000 burst messages, 1,000 reconnects, 200 delayed readers shed, 9,800 healthy peers still live, heap 18.69 MiB before / 18.70 MiB after churn, and client-wire p99 6.91 ms against a 250 ms SLO. `make check` passed, including the race suite and both builds. This repository has no GitHub Actions workflow named `CI`, as confirmed during the preceding queue cycle.

Commits: `a21dda3` in `haiven-websocket-hub`; parent gitlink `41c13912a`.
❯ 🚧 Please help me prepare for my haiven interview. They will be going over my solution. They have not seen the code.

### Re: Haiven interview preparation — gpt-5

Your strongest framing is: **this is a design review backed by executable experiments, not a claim that the prototype is production-ready.** They said they care about reasoning and communication. Lead with the reasoning; use the code and benchmarks as evidence.

#### Your 90-second opening

> The problem is to give each client one persistent connection while multiplexing chat, presence, notifications, moderation, and future services behind it. I separated that into four concerns: a small versioned envelope, authenticated connection lifecycle, routing through downstream interfaces, and per-message-class delivery and backpressure policy.
>
> The central invariant is that one slow client or failed downstream must never block shared work. Every client has a bounded, nonblocking outbound queue. Replaceable state such as presence and typing is conflated; durable events such as chat spill to an outbox; a persistently slow consumer can be disconnected and resume.
>
> I began with a simpler goroutine-per-connection transport and actor-style registry. I then used load tests to find where those assumptions stopped scaling. That led to an event-loop transport, sharded registry and sequence allocation, bounded rendered friend frames, and explicit failure isolation. I would not prescribe all of those choices before learning the product's actual scale and delivery requirements—the prototype exists to make those tradeoffs measurable.

Stop there. Do not open with “100 million connections” or with the library list. First establish that you understand the product problem and the correctness boundary.

#### A clean 12–15 minute walkthrough

1. **Problem and assumptions (1–2 minutes).** One connection per client, unreliable networks, low-latency small messages, multiple downstream services, and unknown target scale. Say that the two decisions you need from the team are target scale and which message classes are genuinely durable.

2. **One inbound message (2 minutes).** `server.OnTraffic` completes and authenticates the WebSocket upgrade, then `Conn.Feed` parses frames. A bounded worker pool moves decode/routing off the poller while a serial drain per connection preserves that client's order. `Conn.onMessage` validates the envelope, ignores client-supplied identity fields, stamps authenticated identity, and calls `Router.dispatch`. The router applies unknown-service handling, rate limiting, the downstream circuit breaker, then `Downstream.Handle`.

3. **One outbound message (2 minutes).** A downstream uses `Sink.SendToClient` or `SendToTopic`. The hub stamps ID/time and, for at-least-once classes, a per-client/per-service sequence. It looks up the class policy and enqueues without blocking. `AsyncWrite` drains one frame at a time through completion callbacks, preserving connection-local order without a write goroutine.

4. **Backpressure—the heart of the design (3 minutes).** Walk through `backpressure.go` and `sendqueue.go`:

   - chat, notifications, and moderation are interactive, at-least-once, and spill when memory is full;
   - presence and typing are bulk, at-most-once, and conflate by entity because only the latest state matters;
   - interactive traffic drains before fresh bulk traffic, with age promotion to prevent starvation;
   - no overflow policy blocks the fan-out path.

   The best concrete example is typing: `DropNewest` can discard the terminal “stopped” state and strand the UI. `ConflateByEntity` replaces the pending state in place, while a client-side TTL handles a crashed typist whose final state never arrives.

5. **Reconnect and fan-out correctness (2 minutes).** The registry is sharded behind an interface. Paths touching both tables lock **connection shard, then topic shard**, preventing lock cycles. A reconnect is last-writer-wins; unregister removes a connection only if it is still the registered instance, so cleanup from an old socket cannot evict the replacement. Topic membership is copied under lock and delivery happens after releasing it.

6. **What testing changed (2 minutes).** Use only two evolution stories:

   - the single actor/lock shape stayed flat around 1.5M deliveries/s; 64 shards reached about 3.7M/s at 1M clients without changing per-connection memory;
   - an unbounded friend graph made memory depend on other users' graph size, so a 50-friend rendered frame plus snapshot-on-frame-change made capacity depend on a product-controlled constant.

7. **Close with deferrals (1 minute).** Production multi-node ownership/routing, a truly durable outbox, exact product delivery semantics, and operational rollout remain team decisions. The prototype establishes seams and invariants; it does not pretend those decisions are finished.

#### Code tour order

Keep the repository tree closed and pre-open these files in this order:

1. `envelope/envelope.go` — the wire contract and server-owned fields.
2. `server/server.go` — `OnTraffic`, authenticated upgrade, bounded dispatch, and timing-wheel liveness.
3. `hub/conn.go` — `Feed`, `onMessage`, nonblocking enqueue, and the self-clocking `AsyncWrite` chain.
4. `hub/router.go` — the short, legible inbound policy pipeline.
5. `hub/backpressure.go` and `hub/sendqueue.go` — the main design invariant.
6. `hub/registry.go` — sharding, lock order, reconnect race prevention.
7. `hub/hub.go` — `SendToClient`, `SendToTopic`, delivery stamping, snapshots, and resume.
8. Tests only after the design: `sendqueue_test.go`, `registry_test.go`, `race_test.go`, `sim_test.go`, and `cmd/loadtest/trackc.go`.

Do not tour every downstream adapter. The interfaces are the point; the sample services merely prove the seam.

#### The questions they are most likely to ask

**Why gnet instead of ordinary goroutines?**

> Goroutine-per-connection is the simpler default and may be entirely correct at Haiven's real scale. I moved to gnet only after treating per-connection memory as a measured constraint. The benefit is no fixed read/write goroutine pair per idle connection. The cost is more explicit lifecycle and dispatch machinery. I would choose after learning the actual connection target, not from benchmark fashion.

**Why sharded locks instead of an actor?**

> The actor was easier to reason about, but it created one serialization point. Keeping `Registry` behind an interface let me replace the implementation after measurement. Shards preserve local reasoning: each map has an owner lock, cross-table paths have one lock order, and reconnect cleanup checks connection identity.

**What ordering do you guarantee?**

> Per client, per service—not a global order across unrelated services. Inbound work is serialized per connection. Outbound writes are one-at-a-time per connection, and durable classes receive a per-client/per-service sequence. A service may not span priority bands because that could let a newer sequence overtake an older one.

**Does at-least-once actually work?**

> The protocol shape is present: IDs, per-service sequences, acknowledgements, resume, client deduplication, and spill. The prototype outbox is in memory, so it demonstrates semantics but is not durable across process loss. In production I would back it with a durable log/service or make the owning downstream the replay authority.

**What happens to a slow client?**

> It never blocks shared fan-out. Replaceable state is conflated or dropped according to class; durable events spill; a class configured to disconnect closes the client so it can resume. Queue depth, drops, spills, disconnects, and breaker state are observable.

**What happens when a downstream dies?**

> Each downstream has a circuit breaker. Dispatch fails fast rather than allowing a dead dependency to consume the hub's bounded queues indefinitely. The Track C harness kills and recovers a downstream while also exercising reconnects, slow readers, sustained fan-out, and bursts.

**How do you scale across nodes?**

> Connections and subscriptions are node-local. An external routing plane publishes once; each node fans out only to its local subscribers. The prototype deliberately does not claim that multi-node ownership, presence aggregation, or resume storage is solved.

**Why JSON?**

> The hub treats payloads as opaque and only owns a small envelope. JSON keeps the contract inspectable and avoids forcing every service schema into the hub. Protobuf or MsgPack is a measured optimization later; zero-copy claims are limited because the event loop and asynchronous dispatch still require clear buffer ownership.

**Why keep services in process?**

> Not simply because of message volume. The deciding questions are failure isolation, ownership, and resource-profile compatibility. A service with a large heap, heavy CPU, or separate deployment lifecycle may belong out of process even if the socket can carry its traffic.

**What would you build first with two other engineers?**

> Engineer one owns transport/auth/lifecycle and the client SDK contract. Engineer two owns registry, subscriptions, and fan-out. Engineer three owns downstream adapters, class policies, and observability. Agree first on the envelope, `Downstream`/`Sink` interfaces, class-policy table, and test fixtures so the three tracks can proceed independently. Integrate with an end-to-end reconnect/slow-consumer test before optimization.

#### Benchmark numbers worth remembering

Use a small set and always attach scope:

- **Memory shape:** about **700 B Go heap / 1.08 KiB RSS per materialized hub connection**, flat from 5M to 100M in the in-process scale harness.
- **Fan-out:** roughly **3.5–3.9M deliveries/s**, depending on the exact suite and room width. Distinguish **sends** from **deliveries**; one send can fan out to many recipients.
- **Sharding:** about **1.57M → 3.70M deliveries/s** at 1M clients, a **2.4×** improvement, with unchanged marginal memory.
- **Overload:** at 10× offered load the breaker shed about **93%** while keeping the 1M connection population live and memory bounded.
- **Track C:** at the production-sized 10k-client resilience run, client-wire p99 was **6.91 ms against a 250 ms SLO**, with reconnect churn, delayed-reader shedding, burst+sustained traffic, and downstream recovery assertions.

Do **not** imply that the 100M run was 100M kernel TCP/TLS sockets. It materialized 100M real `*hub.Conn` objects and exercised the in-process subscription path; it validates marginal application-state memory and data-structure scaling. It does not validate file-descriptor limits, kernel socket memory, TLS, NIC bandwidth, load-balancer behavior, or internet clients. Saying this unprompted will make the benchmark more credible.

Also say that the out-of-process socket benchmark is withheld because the harness produced impossible results. That is good engineering judgment: you found a measurement you could not trust and refused to use it.

#### Four corrections to own before they catch them

1. **CAS means compare-and-swap, not copy-and-swap.** Do not repeat “lock-free Copy And Swap.” The code uses atomic compare-and-swap for state flags; the registry's reconnect-safe unregister is an identity check under a shard lock. `slices.Clone` is copying, not CAS.
2. **gnet does not provide your heartbeat policy.** Your server implements ping/read-deadline behavior using an `antlabs/timer` timing wheel; gnet supplies the event loop and asynchronous write primitive.
3. **The in-memory outbox is not durable storage.** Present it as a replaceable prototype implementation of the interface and protocol.
4. **The 100M headline is not an end-to-end network claim.** Use the precise scope above.

#### How to answer the AI question

The prompt explicitly limited AI, and your write-up mentions Claude Code, so assume they will ask. Do not minimize or imply that you manually authored every line.

> I used AI to accelerate prototype implementation and test generation. I retained responsibility for the design and reviewed the output against explicit invariants. I corrected decisions that were wrong—for example typing used `DropNewest`, which could lose the terminal stopped state; I changed it to keyed conflation. I also rejected benchmark results when the harness contradicted basic expectations. I can explain the code path, locking, delivery semantics, failure modes, and what I would remove for a production first version.

The proof of ownership is not a percentage estimate; it is your ability to explain why each policy exists, identify the prototype's limits, and change your mind when evidence contradicts a claim.

#### Final rehearsal rule

Answer each question in this order: **decision → reason → tradeoff → evidence → what would change your mind**. Keep the first answer under 90 seconds, then let them pull you deeper. The biggest risk is sounding attached to the sophistication. The strongest version of you in this interview is the engineer who can explain the advanced prototype and still say, “Given a modest real target, I would ship the simpler design first.”
<!-- agent:boundary:69aeb532 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#haivenmock] [recommended] Run a timed Haiven solution mock interview: 90-second opening, 10-minute code tour, 15-minute adversarial design Q&A, then tighten weak answers.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-07-06 [#haivenscreen] Write full rehearsal script (6 STAR answers) for the LinkedIn AI screening interview
- 2026-07-15 [#haivenws] Implement the gobwas/ws WebSocket transport migration for haiven-websocket-hub (scale + low latency; chosen over nbio for ecosystem breadth + no single-maintainer risk): event loop via panjf2000/gnet (or cloudwego/netpoll), protocol via gobwas/ws/wsutil ControlHandler, handler pool via panjf2000/ants, deadline/ping timing-wheel via antlabs/timer, keep net/http for /healthz+/metrics; rewrite conn.go pumps into gnet callbacks + a gobwas frame adapter, re-map sendQueue onto AsyncWrite, keep Accept/registry/router/outbox/breaker semantics, update e2e test + DESIGN.md §3-§4, add ADR recording gobwas vs nbio vs gorilla+gin. [recommended]
- 2026-07-16 [#typeconflate] haiven-websocket-hub: typing overflow is DropNewest (hub/backpressure.go:81) — on a full queue sendqueue.go:93 silently discards the ARRIVING frame, including the terminal 'stopped', leaving a stuck 'is typing…' indicator (no TTL exists anywhere in the repo). Switch typing to ConflateByEntity (as presence uses, backpressure.go:80) so the newest always wins and is bounded per entity; ALSO add a receiver-side typing TTL (~5-10s) for the never-sent-stop case (typist crash/disconnect), which no queue policy can fix. Note: conflation will change the realistic loadtest typing delivery counts in docs/LOADTEST.md.
- 2026-07-16 [#friendfanout] haiven-websocket-hub: implement -friends-fanout=subscribe|resolve in the realistic loadtest — resolve mode drops friend-topic subscriptions and has the presence service resolve i's online friends (reverse adjacency, built once in the harness = the service's store) issuing SendToClient per friend. Then re-run the friends-median sweep to confirm hub per-conn heap stays flat at the friends-off row (~2.31 KiB) at any median, and measure the CPU/throughput cost of the trade (1 SendToTopic -> F SendToClient per presence update). This is the fix for the unbounded friend-presence memory documented in docs/LOADTEST.md.
- 2026-07-19 [#activepri] haiven-websocket-hub: activity-tiered fan-out in SendToTopic (hub/hub.go:200) — (a) partition the members snapshot so active recipients enqueue first, (b) more importantly downgrade per-recipient class policy for inactive conns (presence/typing -> conflate-hard/drop, chat still spills) since that removes work rather than resequencing it. Define active from one lock-free atomic lastClientFrameAt on the conn, NOT a client-reported focus signal. Needs hysteresis (promote 30s / demote 120s) or conns thrash between bands. BLOCKED ON #friendsnapshot: demoting inactive presence to drop means a user who becomes active holds no correct state and gets no replay (at-most-once) — snapshot-on-resume must land first.
- 2026-07-20 [#fanoutpri] haiven-websocket-hub: two-band send queue (interactive vs bulk) in hub/sendqueue.go — two lazily-allocated list.Lists drained high-then-low, each FIFO internally, O(1), no heap, so per-conn idle footprint (740 B) is unchanged. Interactive = acks, directed chat, moderation; bulk = presence/typing/passive topic traffic. HARD CONSTRAINT: a service may never span bands, or per-service monotonic seq (DESIGN §6.2) stops meaning 'loss' and client dedup/reorder breaks. Explicitly NO aging/escalation — bulk band stays DropOldest/ConflateByEntity, which bounds staleness by construction instead of by a timer.
- 2026-07-20 [#friendsnapshot] haiven-websocket-hub: a friend-frame needs a presence snapshot on frame change — presence is at-most-once + conflated, so a newly-framed friend produces no replay and renders 'offline' until their next tick. Add a 'give me current state for these K' query (design §6.1 already specifies snapshot-on-resume for at-most-once classes) + model frame paging (re-subscribe churn on scroll) in the realistic loadtest. This is the real work item behind -friends-frame, not the subscription cap.
- 2026-07-20 [#yyzn-clarify] Queue head '/goal Implement backlog item(s): #yyzn' references a backlog ID that does not exist in this document (backlog/icebox/done all lack it); the binary marks it skipped_queue_head_ids. Confirm the intended item or remove the head.
- 2026-07-20 [#haivenwsopt] haiven-websocket-hub gnet transport O(1)-scale refinements: swap the OnTick liveness sweep for an antlabs/timer per-conn timing wheel; run message dispatch through a panjf2000/ants pool as a poller-goroutine guard; harden the WS handshake for fragmented-across-TCP-segments upgrades (currently single-segment). [recommended]
- 2026-07-20 [#tnpa] [recommended] Build the Track C load-test harness for haiven-websocket-hub: simulate unreliable-network clients (drops, slow consumers, reconnect storms), sustained + burst fan-out, kill one downstream mid-test; assert latency SLO holds, memory tracks connection count, and slow consumers get shed without collateral damage (design §9).
<!-- /agent:done -->
