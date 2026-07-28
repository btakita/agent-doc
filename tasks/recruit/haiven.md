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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/e715e181b73b2efd56586b6dc037817620083b669835f8f4d448e1adedf65952-20260728-162847.md`*

Compacted content:
- Archived 20 response topic(s): simplify LOADTEST.md + DESIGN.md + README.md; why a friend-frame beats sender-side resolution; opportunistic batching, max-size caps, and whether in-process still wins; 17 more
- Prior summary/context: prior compacted content: Archived 12 response topic(s): writeNext CAS placement + increase test coverage; coverage to...
<!-- agent:boundary:e9bd8d64 -->
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
