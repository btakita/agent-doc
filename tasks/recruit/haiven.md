---
agent_doc_session: 7a3dfe1a-875b-4b93-9e8e-12d156448f63
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
codex_args: -s danger-full-access
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/e715e181b73b2efd56586b6dc037817620083b669835f8f4d448e1adedf65952-20260716-005859.md`*

Compacted content:
- Archived 29 response topic(s): Haiven; Craig Fryar; The job; 26 more
- Prior summary/context: prior compacted content: Archived 28 response topic(s): Haiven; Craig Fryar; The job; 25 more
Why not call `c.writing.CompareAndSwap(false, true)` at the top of `Conn.writeNext` if a flag is set?

Please increase test coverage: `total:									(statements)		42.9%`

### Re: writeNext CAS placement + increase test coverage — opus-4-8

**Why the CAS is in `kick`, not at the top of `writeNext`:** they play different roles. `kick` is the *entry point* — it acquires exclusive drain ownership once via `writing.CompareAndSwap(false, true)`; if it loses (a drain is already in flight), it returns, and the running chain will pick up the frame just enqueued. `writeNext` is the *body* of that already-owned chain: pop a frame → `AsyncWrite` → the completion callback re-enters `writeNext`, all while `writing` stays `true`. If `writeNext` did `CompareAndSwap(false, true)` at its top, it would **fail on every legitimate call** (the flag is already true from `kick`/the callback), so the drain would stall and nothing would ship. The one CAS `writeNext` does need is the empty-queue boundary (conn.go:100): after clearing the flag on an empty pop it re-checks `Len() > 0` and re-arms, covering the lost-wakeup race (a producer enqueued between the pop and the flag clear). So the pattern is acquire-once-at-entry + hold-across-async-callbacks + re-arm-on-empty — a per-call CAS in `writeNext` would break the self-clocking.

**Test coverage: 42.9% → 52.7%.** Added unit tests for the pure-logic seams:
- `metrics` 0% → 100% (all counters + `Snapshot`, `Live = connects-disconnects`)
- `outbox` — `Append`/`Since`/`Ack` ordering, per-(client,service) isolation, frame-clone-not-alias
- `backpressure` — `Guarantee`/`Overflow` `String`, `PolicyTable` `Lookup` fallback + `Set`
- `config` — `Config.ReadDeadline`, `New` option builders + `Hub` accessors
- `downstream/chat` 0% → 100% (`Name`, backlog-full, worker echo + ignore paths)

`hub` 49.7% → 60.2%. `server` and `cmd/hubd` stay at 0% *statement attribution* — `server` is exercised end-to-end by the `test` package (which passes), but `go test` does not cross-attribute that coverage; unit-testing them directly needs a gnet.Conn fake / `main` refactor, which I left out of scope.

Verified `go build`/`go vet`/`go test ./...` all green. Committed `555fb32`, gitlink `2d4d47403`, pushed to GitHub.

Note: `hub/hub.go` has an uncommitted one-line `s.mu.Lock(); defer s.mu.Unlock()` edit (yours, matching your terse style) — it builds/vets but is **not gofmt-clean** (`gofmt` would split it back to two lines). Left it untouched for you to keep or reformat.

### Re: coverage to 90%, simulation testing, and Lean — opus-4-8

**Coverage: 52.7% -> 90.0%** (`go test ./...`; cross-attributed with `-coverpkg` it is 90.6%). Per package: `hub` 92.8%, `downstream/chat` 100%, `auth` 92.3%, `envelope` 91.7%, `server` 85.1%, `cmd/hubd` 60.5% (only `main()` flag parsing is left — I extracted `buildServer`/`run` so the serving path is tested on ephemeral ports). All green, `go vet` clean, `-race` clean on `hub` + `server`. Committed `d44c6c7`, gitlink `504259a36`, pushed.

The enabler was that `Conn` is built to run with a nil `gconn` (delivery-only), plus a small embedded-interface `gnet.Conn` fake for the socket-write paths. New tests cover frame decode (text/fragmented/ping/close/bad/oversized/partial), the send-queue drain chain, Accept/supersede, at-least-once vs at-most-once delivery, control ack/resume, spill-to-outbox, the router (rate-limit / breaker / unknown-service / error paths), the token bucket, and the server HTTP endpoints + the OnTraffic upgrade handshake (accept + reject).

**Simulation testing** (added, `hub/sim_test.go`) — deterministic, fixed-seed so it never flakes and any failure reproduces:
- *at-least-once never loses*: drive 250 messages at a depth-4 slow consumer; assert every seq survives in the queue **or** the outbox, and the queue never exceeds its cap.
- *no head-of-line starvation*: a saturated slow consumer never blocks a fast peer on the same topic (the fast one receives all 50 broadcasts).
- *bookkeeping invariant*: a randomized 500-round accept/send/disconnect schedule keeps `metrics.Live == connects - disconnects == registry.Count`.

**Should we use a Lean model?** My recommendation: no, not for this — it is the wrong cost/benefit, and there is a better formal option if you want one.

- Lean proves properties of a *mathematical model* you write, not of the running Go. To use it you would re-encode the hub (event loop, sockets, timing) as an abstraction and prove lemmas about *that*. You would be proving the model, while the bugs that actually bite live in the code — exactly what the deterministic simulation + `-race` exercise directly. For a v1 service (and a take-home), running the real code under seeded fault schedules is far higher signal per hour.
- Where formal methods *do* pay off here is the lock-free send-queue drain (the `kick`/`writeNext` CAS ownership you asked about) and the at-least-once outbox/cursor protocol under crash + reconnect interleavings — state spaces too large to enumerate by testing. For *those*, the standard tool is a model checker that explores all interleavings, and **TLA+ (or Alloy) fits better than Lean** for concurrency/liveness. A short TLA+ spec of the resume/ack protocol would be a strong, low-cost formal artifact.
- So: keep the simulation suite as the primary correctness net; if you want a formal complement for the interview, write a small TLA+ spec of the at-least-once protocol rather than a Lean model. Lean would be the right call only if you were proving pure algorithmic/data-structure correctness with a machine-checked proof as a hard deliverable.

### Re: Can we get to 100% coverage? — opus-4-8

Short answer: I pushed it from 90.0% to **97.3%**, and every library package is now at or near 100% — but literal 100% is the wrong target, and here is exactly why.

**Where it landed** (`go test ./...`): envelope 100%, server 100%, downstream/chat 100%, `hub` 99.8%, auth 92.3%, `cmd/hubd` 65.1%. All green, `go vet` clean, `-race` clean on `hub` + `server`. Committed `e8def4e`, gitlink `66c403556`, pushed.

I added branch coverage for every reachable path: all send-queue overflow policies, the breaker's half-open probe cycle, control unsubscribe/ack/resume and their error paths, slow-consumer shedding, write-error close, the encode-error branches, registry supersede + topic cleanup, `Validate`/`MustEncode`, auth issuer/audience enforcement, and the server's full upgrade handshake (query-param + `Authorization` header, buffered-bytes drain, post-upgrade read-error).

**The remaining 2.7% is four things I deliberately did not chase:**

1. **`cmd/hubd.main()`** (the 0% that dominates the gap) — flag parsing + signal wiring + `log.Fatal`. A process entrypoint is only coverable by building a `-cover` binary and running it as a subprocess with `GOCOVERDIR`. That is fragile integration machinery, and `main()` is the textbook case that coverage targets *exclude*.
2. **`run` — one `log.Printf`** that only fires if the HTTP listener fails to bind (needs a port-conflict race to hit).
3. **`auth.Verify` — a `non-MapClaims` guard** that `jwt.Parse` cannot actually produce; it is defensive code for an impossible state.
4. **`writeNext`'s lost-wakeup re-arm** — reachable only under a genuine enqueue-vs-drain data race. Forcing it means goroutine-racing tests that flake, which is exactly what the deterministic simulation suite was built to avoid.

**Recommendation:** stop here. 97% with the libraries at ~100% is the honest, high-signal number; the residue is an entrypoint, two defensive branches for states that can't occur, and a concurrency race. Chasing them makes the *tests* worse (flaky, fragile) without making the *code* safer. If a reviewer wants the headline to read 100%, the clean move is to scope the coverage target to the library packages (exclude `cmd/hubd`), which reports ~99.7% — I can wire that into a `make cover` target if you want it.
### Re: Can we simulate the enqueue-vs-drain race? — opus-4-8

Yes — and I did it two ways, which together push `hub` to **100%** (overall now 97.5%; server/envelope/chat also 100%). Committed `fa19836`, gitlink `e86173875`, pushed. `-race` clean.

**1. A faithful concurrency simulation** (`TestConcurrentEnqueueDrainNeverLosesFrames`). The reason a plain fake can't exercise the race is that its `AsyncWrite` completes *synchronously*, so the whole drain runs on the enqueuing goroutine — no window. The fix is an `asyncConn` whose `AsyncWrite` returns immediately and fires the completion callback on its **own goroutine**, exactly like gnet's real poller. Now producer enqueues genuinely race the drain's `writing`-flag clear and re-arm. The test runs 200 rounds × 4 concurrent producers into a queue large enough that nothing is dropped by policy, then asserts **every** frame reached the socket and none is stranded (`QueueLen == 0`). A broken lost-wakeup guard would leave a frame stuck with no drainer — the assertion catches it, and `-race` catches any torn access to the shared flag. This is the honest simulation of the race.

**2. Deterministic coverage of the exact branch.** The catch: that concurrent test hits the lost-wakeup *window* only *probabilistically* (it's a few instructions wide), so it does not reliably cover the re-arm line for coverage purposes — which is the whole reason I'd flagged it as race-only. To close that deterministically I extracted the empty-pop re-arm out of `writeNext` into `Conn.rearmIfRefilled()` — a small refactor that also names the concept — and `TestRearmIfRefilled` drives it directly: set the drain-owner flag (as if a drain just popped empty), enqueue a "late" frame into the window, call `rearmIfRefilled`, and assert it re-acquires and drains it (plus the no-refill case just clears the flag).

So the race is now both **exercised for real** (concurrency + `-race`) and **pinned deterministically** (unit test), rather than left to chance. The only branches still under 100% are `cmd/hubd.main()` (the process entrypoint — subprocess-only) and one HTTP-bind-error log line; the earlier `auth` non-`MapClaims` guard remains a defensive branch `jwt.Parse` can't produce.
<!-- agent:boundary:0f5ec144 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

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
<!-- /agent:done -->


