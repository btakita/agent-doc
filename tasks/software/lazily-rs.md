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
queue: stop
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
❯ **IPC proof issue dogfood log**

❯ Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

❯ Issue class: `ipc_proof_insufficient`
❯ Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/software/lazily-rs.md source=socket_ack_content patch_id=589ea334-bcfb-4908-a9c9-c60ded09bb9c invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=11660 candidate_hash=ad360de1cc96f9e923355ed3abdcab9a59b8285a4d93f1e56a6b6538ffcff5c0 content_ours_len=9211 content_ours_hash=4b62debfd316ac226f2d255f4c84ecbd5a3e0ce90288406e9d0a81b732919b72 doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782330985974
```
> **Queue prompt:** What are #next-steps to fix lazily bugs?

### Re: #lzasyncdispose2 — abort in-flight effect re-run on dispose — glm-5.2

Drained the top `do` queue item. `AsyncContext::dispose_async_effect` recycled the SlotId (`free_ids` LIFO) while a spawned effect re-run task (`flush_async_effects`) could still be parked in-flight across its compute `.await`; on resume it re-locked and, if a freshly allocated effect had reused the recycled id, wrote the stale run's cleanup + dependency edges into the wrong node (aliasing). The existing `Effect`-type guards (`update_effect_dependencies` `:1203`, cleanup-write `:1189`) only protect recycle-to-non-Effect, not recycle-to-a-new-Effect.

**Fix (lazily-rs `738ff00`, pushed):** store the spawn `JoinHandle` on `AsyncEffectNode.in_flight` and abort it under the graph lock in `dispose_async_effect` (abort released outside the lock) so the in-flight task cannot resume and commit into a recycled-id node. Regression test `async_context_dispose_aborts_in_flight_effect_rerun` parks an effect re-run on a `Notify`, disposes it mid-flight, allocates a new effect at the recycled id, then releases the park — asserting the aborted task does not corrupt the new node (fails without the abort: A overwrites B's cleanup). Verified: clippy `--all-targets --all-features -D warnings` clean; rustfmt clean; `--features async` integration 29 + state-machine 10 + stress 3 green; default tests + release build OK.

The **IPC proof issue dogfood log** that plan flagged as the prompt target is a self-appended agent-doc artifact from the prior interrupted-cycle recovery (`ipc_proof_insufficient`, `live_prompt_drift_after_preflight`) — editor-IPC writeback noise, not actionable lazily-rs work; the write still reached `committed` last cycle. No code action.

Continuing the queue drain (`#lzasynceffdup` next).
❯ **IPC proof issue dogfood log**

❯ Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

❯ Issue class: `ipc_proof_insufficient`
❯ Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/software/lazily-rs.md source=socket_ack_content patch_id=d6722a6c-cf2e-4e08-b1c8-edd6bf0747f9 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=14216 candidate_hash=8aed8b5f12a7e6fa6bdd71f5ec4b69460f232fadcd0c8fcd56243fefd784f6e7 content_ours_len=13348 content_ours_hash=002bba5b1f016c3b315264cdcea7cfd7772f87c5fe948642b5c4540e2d1c58b3 doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782331218740
```

### Re: #lzasynceffdup — dedupe AsyncContext pending effects — glm-5.2

> **Queue prompt:**
>
❯ > do [#lzasynceffdup]


`AsyncContext.pending_async_effects` was a bare `Vec<SlotId>`: every dependency invalidation pushed the effect id again, so the queue grew O(invalidations) with duplicate ids before each flush (harmless at run time only because a `force_run` re-check skipped the extras). `Context` (`context.rs:123`) and `ThreadSafeContext` (`thread_safe.rs:1219`) both dedupe via a scheduled `HashSet`; the async backend was the lone outlier.

**Fix (lazily-rs `090e104`, pushed):** added `scheduled_async_effects: HashSet<SlotId>` to `AsyncContextInner` and extracted `schedule_async_effect(inner, id)` (mirrors `Context::schedule_effect`) — all three scheduling sites now set `force_run` and enqueue only on `scheduled_async_effects.insert(id)`. `flush_async_effects` clears the set on drain; `dispose_async_effect` removes from it (parity with `Context::dispose_effect:684`). White-box unit test `async_schedule_effect_dedupes_pending_queue` schedules the same id three times and asserts exactly one queue entry + flush-clears the set (fails without the HashSet). Verified: clippy `--all-targets --all-features -D warnings` clean, rustfmt clean, `--features async` suite green, default tests + release build OK.

Next queue head: `#lzstr0mnetinbox` (str0m receive-side unbounded inbox).
❯ **IPC proof issue dogfood log**

❯ Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

❯ Issue class: `ipc_proof_insufficient`
❯ Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/software/lazily-rs.md source=socket_ack_content patch_id=96ffa092-fdc0-47eb-989c-308951a16a5a invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=15913 candidate_hash=363a04d74c231071f2468d2ed931251e9baad67240c60ae7cb5c892dd53a69a9 content_ours_len=14160 content_ours_hash=0c49ea38118e80efa8900a6f9812c56ba79285a55686989dbcf4a55992602364 doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782332242138
```

### Re: #lzstr0mnetinbox — bound Str0mNet receive inbox — glm-5.2

> **Queue prompt:**
> 
> do [#lzstr0mnetinbox]

`Shared::inbox` (the receive side of `Str0mNetChannel`) was an unbounded `Mutex<VecDeque<Vec<u8>>>`: `handle_event` pushed every inbound `ChannelData` frame without limit. The send side already caps the driver outbound queue at `MAX_PENDING_FRAMES=1024` (`#lzstr0mframe`), but the receive side had no gate — a consumer polling `try_recv_frame` slower than the peer produces grew receive memory without bound.

**Fix (lazily-rs `e165720`, pushed):** added `MAX_INBOX_FRAMES` (=1024, symmetric with the send cap) with drop-oldest ring semantics (newest retained) and a `dropped_inbox_frames` counter exposed via `Str0mNet::dropped_inbox_frames()` so sustained saturation is diagnosable (non-zero warrants a Snapshot resync). The str0m SCTP stack still owns wire-level reliability; this is the local memory safety valve. Unit test `inbox_caps_at_max_inbox_frames_and_counts_drops` pushes past the cap and asserts the bound, drop count, and drop-oldest retention. Verified: clippy `--all-targets --all-features -D warnings` clean, rustfmt clean, full `--features webrtc-str0m` suite green (incl. the real two-socket `tests/str0m_net.rs` round trip), release build OK.

Next queue head: `#lzstr0mnetacceptanswer` (str0m discards `accept_answer` result).
**IPC proof issue dogfood log**

Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/software/lazily-rs.md source=socket_ack_content patch_id=ac5e1a12-a852-4567-a0b7-92241ad5b0ac invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=17902 candidate_hash=08c5a9291b91beef801393ec92666078f17ad9435771b30fa7f2287cc04d2e0d content_ours_len=15218 content_ours_hash=58f4bce236aacf73ef0dcfb386be4b290b87d4877783b3a3cd4fda6d540126f0 doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782332893751
```

### Re: #lzstr0mnetacceptanswer — surface str0m apply failures — glm-5.2

> **Queue prompt:**
> 
> do [#lzstr0mnetacceptanswer]

The str0m driver discarded the result of `rtc.sdp_api().accept_answer(pending, answer)`: an SDP answer that parses (validated eagerly in the public `accept_answer`) can still fail at apply time — ICE/DTLS fingerprint or mid mismatch — and the failure was invisible, so `wait_open` simply timed out with no diagnostic.

**Fix (lazily-rs `7a28283`, pushed):** capture apply-time failures in a new `Shared::last_error` field (let-chained `Err` guard, edition-2024 style) exposed via `Str0mNet::last_error()`, so a `wait_open` timeout has a diagnosable cause. Inline unit test `last_error_round_trips_apply_failure` exercises the plumbing; the real two-socket integration test asserts `last_error` stays clear on a successful handshake. Verified: clippy `--all-targets --all-features -D warnings` clean (after collapsing the nested `if let` into a let-chain per `collapsible_if`), rustfmt clean, `--features webrtc-str0m` suite green (incl. `tests/str0m_net.rs` two-socket round trip), release build OK.

Next queue head: `#lzwsunbounded` (ws_backend unbounded outbound mpsc + drive abandons frames on send error).

### Re: #lzwsunbounded — bound WsDataChannel outbound — glm-5.2

> **Queue prompt:**
> 
> do [#lzwsunbounded]

`WsDataChannel` outbound was an `mpsc::unbounded_channel`: `send_frame` pushed without limit and the `drive` task abandoned every queued frame on a write error (`break`) while producers kept enqueuing — a fast producer grew outbound memory without bound and silently lost frames on send failure.

**Fix (lazily-rs `a41aaec`, pushed):** switch to a bounded `mpsc::channel(WS_OUTBOUND_CAPACITY = 1024)` and make the sync `send_frame` use `try_send`, mapping `Full` → `WsError::Backpressure` (new variant) and `Closed` → `WsError::Closed` — so a producer faster than the socket drains is surfaced flow control instead of unbounded growth, and the frames abandoned on a write-error break are now bounded. Test `ws_send_frame_surfaces_backpressure_when_buffer_fills` exploits the single-threaded `#[tokio::test]` runtime (a tight sync loop never yields to the driver) to fill the bounded channel deterministically and asserts `Backpressure`. Verified: clippy `--all-targets --all-features -D warnings` clean, rustfmt clean, `--features websocket` suite green, release build OK.

Remaining queue: `#lzerrfrom` (error From impls), `#lzsid32` (SlotId 32-bit casts).

### Re: #lzerrfrom + #lzsid32 — already satisfied (stale) — glm-5.2

> **Queue prompt:**
>
> do [#lzerrfrom]
> do [#lzsid32]

Both final queue items are **already implemented** — they were stale backlog entries from 2026-06-17, resolved by intervening work. Verified against HEAD, no code change needed:

- **#lzerrfrom (done):** all four `From` impls are present in `src/ipc.rs` — `From<serde_json::Error>` for `EncodeError` (`:691`) and `DecodeError` (`:738`), and `From<postcard::Error>` for `EncodeError` (`:698`) and `DecodeError` (`:745`), each correctly feature-gated (`ffi` / `ipc-binary`) to mirror its variant and mapping to `Self::Json` / `Self::Binary`.
- **#lzsid32 (done):** every `SlotId -> usize` conversion in the crate already routes through a `node_index` helper using `usize::try_from(id.0)` — `context.rs:186`, `thread_safe.rs:1230`, `async_context.rs:251`. A repo-wide grep for bare `.0 as usize` SlotId casts returns **none** (the only `as usize` in `thread_safe.rs:1542` is an `Arc::as_ptr` HashMap key, unrelated). 32-bit parity across all three backends is already achieved.

**Queue is now empty — drain complete.** The session fixed six real bugs this run: `#lzasyncdispose` (caa6035), `#lzasyncdispose2` (738ff00), `#lzasynceffdup` (090e104), `#lzstr0mnetinbox` (e165720), `#lzstr0mnetacceptanswer` (7a28283), `#lzwsunbounded` (a41aaec) — all pushed to `lazily-hub/lazily-rs`, each with clippy `--all-features -D warnings` + rustfmt clean + feature-suite green + release build OK.

Note: the editor-IPC CRDT for this doc stayed diverged through the run (`ack_mismatch` at queue consume on every cycle, recovered via `write --commit` + `content_ours`); the responses all committed, but a proven clean closeout needs the editor buffer reloaded to disk. Worth filing the `#exchange-prompt-dropped-on-merge` agent-doc behavior (its own auto-appended dogfood logs classified as fail-closed user prompts) against `tasks/agent-doc/agent-doc-bugs2.md`.
<!-- agent:boundary:7a97d88d -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
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
