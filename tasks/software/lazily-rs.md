---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: opencode
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260624-192733.md`*

Compacted content:
❯ - Archived 11 response topic(s): #lzshmblobconf cross-sibling ShmBlobArena conformance; blog post; blog post; 8 more
❯ - Prior summary/context: prior compacted content: Archived 6 response topic(s): #t0mf eager Signal in lazily-zig; #pwvm IPC.lean clarification...

### Re: lazily-rs bug next-steps — glm-5.2

Drained the queue prompt against a fresh tsift-indexed scan of `src/lazily-rs/` plus the done archive. **Most previously-logged bugs are already fixed at HEAD:** `#lzstr0mframe` (silent frame loss + unbounded `out_pending` → re-queue on write err + `MAX_PENDING_FRAMES=1024` backpressure gate), `#lzstr0mpolldrive` (`socket.send_to` errors now handled explicitly), `#lzloomtimeout` (Loom model bounded — `preemption_bound=4`, `max_duration=60s`), `#lzrepourl` (now `lazily-hub/lazily-rs`), `#lzmsrv` (MSRV 1.88 declared), `#lzdisposedrain` (both `pending`+`scheduled` drained).

**Fixed this cycle — `#lzasyncdispose` (lazily-rs `caa6035`, pushed):** `AsyncContext::dispose_async_effect` (`async_context.rs:1022`) tore down dependency edges and recycled the node id but **never ran the effect's stored cleanup** — a direct violation of the SPEC contract at `SPEC.md:799` ("Dispose async effect and await cleanup") and the lone outlier vs `Context::dispose_effect` (`context.rs:695`) and `ThreadSafeContext::dispose_effect` (`thread_safe.rs:2589`), which both run cleanup on dispose. Async cleanups (resource teardown registered by `effect_async`) were silently leaked. Fix takes the cleanup out of the node under the graph lock and runs it after the lock releases, mirroring `flush_async_effects` (`:1161`, drops the lock before `cleanup()`). Regression test `async_context_effect_async_cleanup_runs_on_dispose` added. Verified: rustfmt clean on changed files; clippy `--all-targets --all-features -D warnings` clean; `--features async` suite green (integration 28 + state-machine 10 + stress 3 + resolve-loop 2); release build OK. (Pre-existing fmt drift in untouched `tests/conformance.rs`/`tests/ipc.rs` left alone to keep the diff scoped.)

### Re: backlog (priority order) — glm-5.2

The highest-leverage cluster is **AsyncContext backend-parity** — it is the newest, least-mature backend and the reactive core the state-machine primitive builds on, so its dispose/dedupe gaps are the most likely to bite a real consumer. Then str0m reliability (the networked transport is approaching its live two-host gate `#h6qb`), then the long-open ergonomics items.

Full list written to `agent:backlog` this cycle. **Top of backlog: `#lzasyncdispose2`** — the in-flight re-run aliasing guard that completes the dispose-correctness story I just started.

Pre-existing `make check` caveat: `benchmark-check` is red (`#lzbenchbudget`, unrelated to this change — budgets not refreshed since the SlotId-cast + eager-Signal landings); `fmt`/`clippy`/`build`/`test`/`test-async`/`test-async-resolve` all pass.
> **Queue prompt:** What are #next-steps to fix lazily bugs?

### Re: #lzasyncdispose2 — abort in-flight effect re-run on dispose — glm-5.2

Drained the top `do` queue item. `AsyncContext::dispose_async_effect` recycled the SlotId (`free_ids` LIFO) while a spawned effect re-run task (`flush_async_effects`) could still be parked in-flight across its compute `.await`; on resume it re-locked and, if a freshly allocated effect had reused the recycled id, wrote the stale run's cleanup + dependency edges into the wrong node (aliasing). The existing `Effect`-type guards (`update_effect_dependencies` `:1203`, cleanup-write `:1189`) only protect recycle-to-non-Effect, not recycle-to-a-new-Effect.

**Fix (lazily-rs `738ff00`, pushed):** store the spawn `JoinHandle` on `AsyncEffectNode.in_flight` and abort it under the graph lock in `dispose_async_effect` (abort released outside the lock) so the in-flight task cannot resume and commit into a recycled-id node. Regression test `async_context_dispose_aborts_in_flight_effect_rerun` parks an effect re-run on a `Notify`, disposes it mid-flight, allocates a new effect at the recycled id, then releases the park — asserting the aborted task does not corrupt the new node (fails without the abort: A overwrites B's cleanup). Verified: clippy `--all-targets --all-features -D warnings` clean; rustfmt clean; `--features async` integration 29 + state-machine 10 + stress 3 green; default tests + release build OK.

The **IPC proof issue dogfood log** that plan flagged as the prompt target is a self-appended agent-doc artifact from the prior interrupted-cycle recovery (`ipc_proof_insufficient`, `live_prompt_drift_after_preflight`) — editor-IPC writeback noise, not actionable lazily-rs work; the write still reached `committed` last cycle. No code action.

Continuing the queue drain (`#lzasynceffdup` next).

### Re: #lzasynceffdup — dedupe AsyncContext pending effects — glm-5.2

`AsyncContext.pending_async_effects` was a bare `Vec<SlotId>`: every dependency invalidation pushed the effect id again, so the queue grew O(invalidations) with duplicate ids before each flush (harmless at run time only because a `force_run` re-check skipped the extras). `Context` (`context.rs:123`) and `ThreadSafeContext` (`thread_safe.rs:1219`) both dedupe via a scheduled `HashSet`; the async backend was the lone outlier.

**Fix (lazily-rs `090e104`, pushed):** added `scheduled_async_effects: HashSet<SlotId>` to `AsyncContextInner` and extracted `schedule_async_effect(inner, id)` (mirrors `Context::schedule_effect`) — all three scheduling sites now set `force_run` and enqueue only on `scheduled_async_effects.insert(id)`. `flush_async_effects` clears the set on drain; `dispose_async_effect` removes from it (parity with `Context::dispose_effect:684`). White-box unit test `async_schedule_effect_dedupes_pending_queue` schedules the same id three times and asserts exactly one queue entry + flush-clears the set (fails without the HashSet). Verified: clippy `--all-targets --all-features -D warnings` clean, rustfmt clean, `--features async` suite green, default tests + release build OK.

Next queue head: `#lzstr0mnetinbox` (str0m receive-side unbounded inbox).

### Re: #lzstr0mnetinbox — bound Str0mNet receive inbox — glm-5.2

> **Queue prompt:**
> 
> do [#lzstr0mnetinbox]

`Shared::inbox` (the receive side of `Str0mNetChannel`) was an unbounded `Mutex<VecDeque<Vec<u8>>>`: `handle_event` pushed every inbound `ChannelData` frame without limit. The send side already caps the driver outbound queue at `MAX_PENDING_FRAMES=1024` (`#lzstr0mframe`), but the receive side had no gate — a consumer polling `try_recv_frame` slower than the peer produces grew receive memory without bound.

**Fix (lazily-rs `e165720`, pushed):** added `MAX_INBOX_FRAMES` (=1024, symmetric with the send cap) with drop-oldest ring semantics (newest retained) and a `dropped_inbox_frames` counter exposed via `Str0mNet::dropped_inbox_frames()` so sustained saturation is diagnosable (non-zero warrants a Snapshot resync). The str0m SCTP stack still owns wire-level reliability; this is the local memory safety valve. Unit test `inbox_caps_at_max_inbox_frames_and_counts_drops` pushes past the cap and asserts the bound, drop count, and drop-oldest retention. Verified: clippy `--all-targets --all-features -D warnings` clean, rustfmt clean, full `--features webrtc-str0m` suite green (incl. the real two-socket `tests/str0m_net.rs` round trip), release build OK.

Next queue head: `#lzstr0mnetacceptanswer` (str0m discards `accept_answer` result).
<!-- agent:boundary:7d4f5c81 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~What are #next-steps to fix lazily bugs?~~
- ~~do [#lzasyncdispose2]~~
- ~~do [#lzasynceffdup]~~
- ~~:pushpin: do [#lzstr0mnetinbox]~~
- 🚧 :pushpin: do [#lzstr0mnetacceptanswer]
- :pushpin: do [#lzwsunbounded]
- :pushpin: do [#lzerrfrom]
- :pushpin: do [#lzsid32]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzstr0mnetacceptanswer] str0m_net.rs driver discards rtc.accept_answer result (:391 let _ =); an ICE/DTLS fingerprint/mid mismatch is invisible and wait_open times out with no diagnostic. Propagate apply-time failure to Shared::last_error so the open-failure surfaces a cause.
- [ ] [#lzwsunbounded] ws_backend.rs outbound is an unbounded mpsc (:88) and drive abandons queued frames on send error (:117-119 break); producers can keep enqueuing after close. Switch to a bounded channel, surface send failures via WsError, stop producers on close.
- [ ] [#lzerrfrom] [recommended] Add From<serde_json::Error>/From<postcard::Error> impls for EncodeError/DecodeError ergonomics (open since 2026-06-17). src/ipc.rs.
- [ ] [#lzsid32] Harden SlotId.0 as-usize casts in thread_safe.rs to usize::try_from for 32-bit safety parity with single-threaded Context (open since 2026-06-17).
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lilazilysmpost] [/] [operator-drive] lazily state-machine LinkedIn post PUBLISHED WITH IMAGE: https://www.linkedin.com/feed/update/urn:li:share:7475622100263923712 (mail/social/20260624-142530-linkedin.md @ mail bf4ab26). CyclePhase diagram rendered from blog post SVG (light theme colors) + uploaded via LinkedIn digitalmediaAsset API. Old text-only post (urn:li:share:7475617202893639680) deleted + replaced. Remaining operator follow-up: post first comment (link + GitHub source) via corky linkedin comment.
- [/] [#kb9b] #lzpypub011=[operator-drive] Publish lazily-py v0.11.0: SHIP-STOPPER #lzpysigfix is FIXED (commit 30907ea). Build artifacts rebuilt in src/lazily-py/dist/ (lazily-0.11.0-{whl,tar.gz}, twine check PASSED, fresh-venv install round-trip verified). From src/lazily-py: tag v0.11.0 + push, gh release create v0.11.0 --generate-notes, then python -m twine upload dist/* (needs PyPI token + browser/passkey 2FA). Gated on the pre-publish live-test confirm per the testing-gate rule. Commit 30907ea + superproject gitlink eb93a75ac already pushed.
- [/] [#kabc] #lipostlazilyarch=[operator-drive] Main LinkedIn post is published and repaired: Corky edit updated mail/social/20260616-lazily-architecture-linkedin.md / https://www.linkedin.com/feed/update/urn:li:share:7472666756810190848 with the full body after the public post truncated at "The core is small: three primitives". Local metadata is committed in mail commit 07a9ea1 and superproject gitlink e7e0b08c1. Remaining operator follow-up: post the first comment from mail/drafts/linkedin/2026-06-16-lazily-architecture.md via `corky linkedin comment <published-file> <comment-body>` if still desired.
- [/] [#h6qb] [recommended] [operator-drive] #lzwebrtcnet-e2e=End-to-end cross-host validation of the Str0mNet backend through the LIVE #yxjw signaling Worker (two real hosts / NAT). Agent-doable slice (#lzwebrtcwire) is DONE: src/webrtc_signaling.rs wires SignalingClient↔Str0mNet and tests/webrtc_signaling.rs covers the localhost handshake. Remaining: live two-host/NAT run via deployed Worker — operator-only.
- [/] [#xccx] #lzholons=[operator-drive] Map Holons ↔ lazily-rs integration. Phase 0 (discovery) is blocked on Steve Melville answering the 7 OPEN QUESTIONS (holon identity, holon↔node/edge mapping, change feed, write/conflict model, permissions mapping, preferred transport, spec reference). Plan: tasks/software/plan-map-holons-integration.md. Unblocks once answers land; then Phase 1 read-only inbound adapter.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
- [ ] [#lzsidecarlock] [recommended] agent-doc-repo: close the lost-update window among concurrent cycle_state writers. Deadlock analysis DONE (tasks/agent-doc/plan-cycle-state-flock.md): NO production mutator calls another (only test code) -> non-reentrant flock is safe. Design: bounded try_lock_exclusive (~5s, fs2) + RAII guard on a dedicated .json.lock sibling (state file is atomically renamed each save -> cannot lock it directly); add `let _lock = acquire_state_lock(file)?;` to each of the ~24 public mutators in cycle_state.rs. cycle_phase_rank (:1273) guards only monotonic phase, not the ~30 non-phase fields. Hardening (not live-fire). Split from #lzsidecaratomic (DONE in agent-doc 647af082 + installed).
- [ ] [#pw26] #lzmapspike=[recommended] Spike: lazily-rs as a host-side reactive derivation + Snapshot/Delta UI-sync cache for MAP holons (evomimic/map-holons, Holochain). Model committed holons as reactive graph nodes; derived holon views/relationships as memoized computeds that recompute only on upstream change; stream lazily-ipc Snapshot/Delta to the TS UI / DAHN graph visualizers. Native host/ layer only (AsyncContext needs tokio; zomes are wasm32/short-lived/single-threaded so in-zome use is limited to single-threaded Context). Do NOT use lazily distributed/signaling — Holochain DHT owns P2P/persistence/validation/consensus. Gate on MAP host-layer actually recomputing derived holon state.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
