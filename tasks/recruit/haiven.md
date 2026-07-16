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
<!-- agent:boundary:4cb87d25 -->
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


