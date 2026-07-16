---
agent_doc_session: 7a3dfe1a-875b-4b93-9e8e-12d156448f63
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
codex_args: -s danger-full-access
queue: start
---

## Status

<!-- agent:status patch=replace -->
**Follow-ups complete (2026-07-06):**
- Tailored resume → `resume/BrianTakita-haiven.pdf` (profile `resume/profiles/haiven.toml`; lazily-go added to `resume/BrianTakita.md` + `resume/data.toml`)
- Application survey answers + AI-screen rehearsal script (6 STAR answers) → `tasks/recruit/haiven-prep.md`
- Craig Fryar contact profile → `mail/contacts/craig-fryar/CLAUDE.md`; Haiven company quick-ref in `haiven-prep.md`

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
<!-- agent:boundary:031ce327 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue go -->
- ⏭️ /goal Implement backlog item(s): #yyzn
- ~~Please go through LOADTEST.md. Simplify based on the existing benchmarks. Show before/after for interesting code events such as no sharding vs sharding. We should be able to replicate no sharding with a sharding of 1. Please simplify LOADTEST.md + DESIGN.md + README.md. They have accumulated cruft as we iterated...some of the content is outdated + inaccurate with the current system.~~ — auto-struck: answered this cycle (#ftstrike)
- Please add a websocket pool to the downstream connections. Load test with 1 websocket connection per downstream service...then load test with the websocket pool. Compare dropped connections + dropped messages/retries.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#friendsnapshot] haiven-websocket-hub: a friend-frame needs a presence snapshot on frame change — presence is at-most-once + conflated, so a newly-framed friend produces no replay and renders 'offline' until their next tick. Add a 'give me current state for these K' query (design §6.1 already specifies snapshot-on-resume for at-most-once classes) + model frame paging (re-subscribe churn on scroll) in the realistic loadtest. This is the real work item behind -friends-frame, not the subscription cap.
- [ ] [#friendfanout] haiven-websocket-hub: implement -friends-fanout=subscribe|resolve in the realistic loadtest — resolve mode drops friend-topic subscriptions and has the presence service resolve i's online friends (reverse adjacency, built once in the harness = the service's store) issuing SendToClient per friend. Then re-run the friends-median sweep to confirm hub per-conn heap stays flat at the friends-off row (~2.31 KiB) at any median, and measure the CPU/throughput cost of the trade (1 SendToTopic -> F SendToClient per presence update). This is the fix for the unbounded friend-presence memory documented in docs/LOADTEST.md.
- [ ] [#yyzn-clarify] Queue head '/goal Implement backlog item(s): #yyzn' references a backlog ID that does not exist in this document (backlog/icebox/done all lack it); the binary marks it skipped_queue_head_ids. Confirm the intended item or remove the head.
- [ ] [#haivenwsopt] haiven-websocket-hub gnet transport O(1)-scale refinements: swap the OnTick liveness sweep for an antlabs/timer per-conn timing wheel; run message dispatch through a panjf2000/ants pool as a poller-goroutine guard; harden the WS handshake for fragmented-across-TCP-segments upgrades (currently single-segment). [recommended]
- [ ] [#tnpa] [recommended] Build the Track C load-test harness for haiven-websocket-hub: simulate unreliable-network clients (drops, slow consumers, reconnect storms), sustained + burst fan-out, kill one downstream mid-test; assert latency SLO holds, memory tracks connection count, and slow consumers get shed without collateral damage (design §9).
- [ ] [#haivenresume] Generate tailored resume for Haiven Backend Platform Engineer via agent-resume (gaming + real-time + Go/reactive profile) [recommended]
- [ ] [#haivenapply] Draft LinkedIn application survey answers ("why you're a perfect fit") for Haiven role
- [ ] [#haivenprofiles] Persist Haiven + Craig Fryar profiles as standalone files (tasks/recruit/, mail/contacts/craig-fryar/)
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-07-06 [#haivenscreen] Write full rehearsal script (6 STAR answers) for the LinkedIn AI screening interview
- 2026-07-15 [#haivenws] Implement the gobwas/ws WebSocket transport migration for haiven-websocket-hub (scale + low latency; chosen over nbio for ecosystem breadth + no single-maintainer risk): event loop via panjf2000/gnet (or cloudwego/netpoll), protocol via gobwas/ws/wsutil ControlHandler, handler pool via panjf2000/ants, deadline/ping timing-wheel via antlabs/timer, keep net/http for /healthz+/metrics; rewrite conn.go pumps into gnet callbacks + a gobwas frame adapter, re-map sendQueue onto AsyncWrite, keep Accept/registry/router/outbox/breaker semantics, update e2e test + DESIGN.md §3-§4, add ADR recording gobwas vs nbio vs gorilla+gin. [recommended]
- 2026-07-16 [#typeconflate] haiven-websocket-hub: typing overflow is DropNewest (hub/backpressure.go:81) — on a full queue sendqueue.go:93 silently discards the ARRIVING frame, including the terminal 'stopped', leaving a stuck 'is typing…' indicator (no TTL exists anywhere in the repo). Switch typing to ConflateByEntity (as presence uses, backpressure.go:80) so the newest always wins and is bounded per entity; ALSO add a receiver-side typing TTL (~5-10s) for the never-sent-stop case (typist crash/disconnect), which no queue policy can fix. Note: conflation will change the realistic loadtest typing delivery counts in docs/LOADTEST.md.
<!-- /agent:done -->


