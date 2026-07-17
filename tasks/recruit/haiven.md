---
agent_doc_session: 7a3dfe1a-875b-4b93-9e8e-12d156448f63
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
codex_args: -s danger-full-access
queue: go
---

## Status

<!-- agent:status patch=replace -->
**Follow-ups complete (2026-07-06):**

Deliverable files are in the working tree (resume + mail are submodules — uncommitted; say the word to commit them). Session doc recovered via direct-write + git commit because the editor IPC transport is wedged (stale/dead editor → `already_applied` deadlock).
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
❯ I'm deferring to DESIGN.md and focusing haiven-websocket-hub-takehome-v2.md on salient design aspects + load test-driven design evolution. Please review. I need the highlights of the benchmarks. Please print them here. If we need to redo any benchmarks, please do so. I have until tomorrow morning.
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
<!-- agent:boundary:7c4a87f2 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue go -->
- ⏭️ /goal Implement backlog item(s): #b10a
- ⏭️ /goal Implement backlog item(s): #yyzn
- ⏭️ do [#friendsnapshot]
- ⏭️ do [#yyzn-clarify]
- ⏭️ do [#haivenwsopt]
- ⏭️ do [#tnpa]
- ~~I'm hand-writing /home/brian/work/btakita/agent-loop/tasks/recruit/haiven/haiven-websocket-hub-takehome-v2.md. Please review and help me to complete handwriting it. I will include sections from websocket-hub-design where appropriate. I want it to show how I think of the problem. Please review our sessions and our design changes so I can include a short log of our design decisions. Note the assignment is located at /home/brian/work/btakita/agent-loop/tasks/recruit/haiven/backend-coding-challenge.md~~
- ~~I'm hand-writing /home/brian/work/btakita/agent-loop/tasks/recruit/haiven/haiven-websocket-hub-takehome-v2.md. Please review and help me to complete handwriting it. I will include sections from websocket-hub-design where appropriate. I want it to show how I think of the problem. Please review our sessions and our design changes so I can include a short log of our design decisions. Note the assignment is located at /home/brian/work/btakita/agent-loop/tasks/recruit/haiven/backend-coding-challenge.md~~ — auto-struck: answered this cycle (#ftstrike)
- ~~Implement grpc with opportunistic batching and benchmark.~~
- ⏭️ do [#friendsnapshot]
- ⏭️ do [#yyzn-clarify]
- ~~Implement grpc with opportunistic batching and benchmark.~~
- ⏭️ do [#friendsnapshot]
- ⏭️ do [#yyzn-clarify]
- ⏭️ do [#haivenwsopt]
- 🚧 do [#tnpa]
- ~~Implement grpc with opportunistic batching and benchmark.~~
- ⏭️ do [#friendsnapshot]
- ⏭️ do [#yyzn-clarify]
- ⏭️ do [#haivenwsopt]
- ⏭️ do [#tnpa]
- ~~Implement grpc with opportunistic batching and benchmark.~~
- ⏭️ do [#friendsnapshot]
- ⏭️ do [#yyzn-clarify]
- ⏭️ do [#haivenwsopt]
- ⏭️ do [#tnpa]
- ~~I'm hand-writing /home/brian/work/btakita/agent-loop/tasks/recruit/haiven/haiven-websocket-hub-takehome-v2.md. Please review and help me to complete handwriting it. I will include sections from websocket-hub-design where appropriate. I want it to show how I think of the problem. Please review our sessions and our design changes so I can include a short log of our design decisions. Note the assignment is located at /home/brian/work/btakita/agent-loop/tasks/recruit/haiven/backend-coding-challenge.md~~ — auto-struck: answered this cycle (#ftstrike)
- ~~Implement grpc with opportunistic batching and benchmark.~~
- ⏭️ do [#friendsnapshot]
- ⏭️ do [#yyzn-clarify]
- ~~Implement grpc with opportunistic batching and benchmark.~~
- ⏭️ do [#friendsnapshot]
- ⏭️ do [#yyzn-clarify]
- ⏭️ do [#haivenwsopt]
- 🚧 do [#tnpa]
- ~~Implement grpc with opportunistic batching and benchmark.~~
- ⏭️ do [#friendsnapshot]
- ⏭️ do [#yyzn-clarify]
- ⏭️ do [#haivenwsopt]
- ⏭️ do [#tnpa]
- ~~Implement grpc with opportunistic batching and benchmark.~~
- ⏭️ do [#friendsnapshot]
- ⏭️ do [#yyzn-clarify]
- ⏭️ do [#haivenwsopt]
- ⏭️ do [#tnpa]
- ~~I'm hand-writing /home/brian/work/btakita/agent-loop/tasks/recruit/haiven/haiven-websocket-hub-takehome-v2.md. Please review and help me to complete handwriting it. I will include sections from websocket-hub-design where appropriate. I want it to show how I think of the problem. Please review our sessions and our design changes so I can include a short log of our design decisions. Note the assignment is located at /home/brian/work/btakita/agent-loop/tasks/recruit/haiven/backend-coding-challenge.md~~ — auto-struck: answered this cycle (#ftstrike)
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#friendsnapshot] haiven-websocket-hub: a friend-frame needs a presence snapshot on frame change — presence is at-most-once + conflated, so a newly-framed friend produces no replay and renders 'offline' until their next tick. Add a 'give me current state for these K' query (design §6.1 already specifies snapshot-on-resume for at-most-once classes) + model frame paging (re-subscribe churn on scroll) in the realistic loadtest. This is the real work item behind -friends-frame, not the subscription cap.
- [ ] 🚧 [#yyzn-clarify] Queue head '/goal Implement backlog item(s): #yyzn' references a backlog ID that does not exist in this document (backlog/icebox/done all lack it); the binary marks it skipped_queue_head_ids. Confirm the intended item or remove the head.
- [ ] [#haivenwsopt] haiven-websocket-hub gnet transport O(1)-scale refinements: swap the OnTick liveness sweep for an antlabs/timer per-conn timing wheel; run message dispatch through a panjf2000/ants pool as a poller-goroutine guard; harden the WS handshake for fragmented-across-TCP-segments upgrades (currently single-segment). [recommended]
- [ ] [#tnpa] [recommended] Build the Track C load-test harness for haiven-websocket-hub: simulate unreliable-network clients (drops, slow consumers, reconnect storms), sustained + burst fan-out, kill one downstream mid-test; assert latency SLO holds, memory tracks connection count, and slow consumers get shed without collateral damage (design §9).
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
<!-- /agent:done -->
