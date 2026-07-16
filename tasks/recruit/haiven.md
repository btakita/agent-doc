---
agent_doc_session: 7a3dfe1a-875b-4b93-9e8e-12d156448f63
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
codex_args: -s danger-full-access
queue: stop
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
<!-- agent:boundary:f86a414b -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue go -->
- Please go through LOADTEST.md. Simplify based on the existing benchmarks. Show before/after for interesting code events such as no sharding vs sharding. We should be able to replicate no sharding with a sharding of 1. Please simplify LOADTEST.md + DESIGN.md + README.md. They have accumulated cruft as we iterated...some of the content is outdated + inaccurate with the current system.
- Please add a websocket pool to the downstream connections. Load test with 1 websocket connection per downstream service...then load test with the websocket pool. Compare dropped connections + dropped messages/retries.
- Please note how friend presence grows memory. Can we optimize friend presence memory? Is there a way to bound friend presence memory...so a user with a large number of friends does not change the memory characteristics of the hub...causing difficulty in predicting capacity??- Please note how friend presence grows memory. Can we optimize friend presence memory? Is there a way to bound friend presence memory...so a user with a large number of friends does not change the memory characteristics of the hub...causing difficulty in predicting capacity?- Please note how friend presence grows memory. Can we optimize friend presence memory? Is there a way to bound friend presence memory...so a user with a large number of friends does not change the memory characteristics of the hub...causing difficulty in predicting capacity??<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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


