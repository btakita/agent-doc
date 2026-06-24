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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260624-162720.md`*

Compacted content:
❯ - Archived 7 response topic(s): lazily-kt GitHub project and submodule; queue guard repair; lazily-js GitHub project and submodule; 4 more
❯ - Prior summary/context: prior compacted content: Archived 6 response topic(s): compaction landed; queue stopped, DAG order intact; #9226 DONE...

### Re: #t0mf eager Signal in lazily-zig — already complete (stale duplicate) — glm-5.2

> **Queue prompt:**
>
> do [#t0mf]


`#t0mf` (Implement eager Signal in lazily-zig) is **already done** — it was split into the two phases the item itself describes, and both landed + shipped. Closing as `--done`.

**Completed work (committed in `src/lazily-zig`):**
- **Phase 1 — engine** (`#9226` / `#lzsigzig1`, DONE): `0c2c81a feat: Signal engine Phase 1 — deferred-recompute queue + snapshot emitChangeUnlocked`. `context.zig` has `Slot.on_invalidate`/`recompute` fn ptrs (default null), `Context.pending_recompute` + `draining_recompute` re-entrancy guard, `drainPendingRecompute()` drained outside the mutex (LIFO), snapshot-then-clear `emitChangeUnlocked` (also fixes the latent iteration-during-mutation bug), and the drain point in `Slot.emitChange()` (`context.zig:324`).
- **Phase 2 — signal.zig** (`#f71y` / `#lzsigzig2`, DONE): `ac8c06f feat: add eager Signal(T) — Phase 2`. `signal.zig` mirrors lazily-rs `SignalHandle<T>`: memoized backing slot + comptime `on_invalidate_hook`/`makeRecomputeFn` puller + `std.meta.eql` memo guard. `get()`/`dispose()`/`is_active()`, exported from `root.zig:37-38`. Tests: eager recompute, memo-guard suppresses equal cascade, dispose→lazy.

**Verification this cycle:**
- `zig build test -Dfilters="lazily/signal"` → **EXIT=0**, all signal tests green.
- SPEC.md (`:127`) + README.md (`:17`) document eager Signal as the backing-slot node (no separate wire type) — committed in `ba7e965`/`083f69b`.
- `src/lazily-zig` working tree clean; superproject gitlink already pinned to `ac8c06f` (no diff) — nothing to commit or push.

**Preflight warnings (observed, not acted on — separate items):**
- `semantic_completion_match`: review `#h6qb` vs DONE `#lzwebrtcwire` — the agent-doable slice is done; `#h6qb` is the operator-only live two-host/NAT run. Left gated.
- `semantic_completion_match`: backlog `#krcm` vs DONE `#t0qn` — **false positive.** `#krcm` is the pre-existing `slot.zig:320 Slot.touch` crash on Zig 0.17.0-dev (installed 0.17.0-dev.892; repo targets 0.16). Unrelated to the Signal primitive and still open. The full `zig build test` hits this on 3/4 binaries on 0.17; signal + state_machine tests pass. Not touched here.

Next queue head is `:pushpin: do [#pwvm]`.

### Re: #pwvm IPC.lean clarification cluster — answered + ShmBlobArena-parity plan scoped — glm-5.2

> **Queue prompt:**
>
> :pushpin: do [#pwvm]
>
> do [#9e64]
>
> do [#t1xt]
>
> do [#dcgp]
>
> do [#7gpg]
>
> do [#kjfe]


`#pwvm` and its five duplicates (`#9e64` rs · `#t1xt`/`#dcgp` py · `#7gpg`/`#kjfe` zig) all ask the same question. Investigated the actual sibling + formal-spec state; the answer is definitive, so I'm closing all six and scoping the one real gap.

**Clarification result:**

| Candidate goal | Status |
|---|---|
| **IPC.lean protocol** (Snapshot/Delta/DeltaOp/epoch) | ✅ **DONE** in all 3 siblings + formally proven in `src/lazily-spec/formal/lean/LazilyFormal/IPC.lean` (`applyDelta`/`isSequentialAfter`, memo & signal silence theorems, batch-flush). No work. |
| **ShmBlobArena parity** | ⚠️ **REAL GAP** — plan scoped (below) |
| **Lean → Rust proof extraction** | ❌ **Not indicated** — `IPC.lean` models `Payload := Nat` and intentionally omits the ShmBlob transport. Extraction is a separate research-scale effort with no current consumer. |

**The gap:** lazily-rs owns the only `ShmBlobArena<B>` host (`ipc.rs:150` — `with_capacity`/`from_buffer`/`write_blob:220`/`read_blob:269`). lazily-py (`ipc.py`) and lazily-zig (`ipc.zig`) carry only `ShmBlobRef` + `SharedBlob` wire tagging — they can *deserialize* a descriptor from a Rust host but cannot *host* (allocate/write/read) an arena. They're second-class blob consumers today.

**Action this cycle:**
- Scoped `tasks/software/plan-lazily-shmblob-parity.md` — ports the arena 1:1 to py (`bytearray`+`memoryview`, no native ext — preserves the pure-Python install story #tnbd depends on) and zig (allocator-backed slice), with a lazily-spec cross-sibling conformance test. True cross-process `mmap`/`/dev/shm` is explicitly out of scope (follow-on that swaps the backing buffer).
- Added one consolidated backlog item `#lzshmblobparity` (end of backlog — won't auto-drain into implementation without operator direction on whether parity is the chosen goal vs. dropping it).
- Closed the six duplicate clarification items: `#pwvm`, `#9e64`, `#t1xt`, `#dcgp`, `#7gpg`, `#kjfe` → all resolved by this finding.

> Note: preflight flagged `queue_stall_detected` — expected, since OpenCode has no in-session auto-loop; this is the manual continuation of the drain. `#h6qb`/`#krcm` `semantic_completion_match` warnings carry over unchanged (separate items, not touched).
<!-- agent:boundary:6dba1fba -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#t0mf]~~
- ~~:pushpin: do [#pwvm]~~
- ~~do [#9e64]~~
- ~~do [#t1xt]~~
- ~~do [#dcgp]~~
- ~~do [#7gpg]~~
- ~~do [#kjfe]~~
- do [#tnbd]
- do [#krcm]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#krcm] #lzsigzig3=[recommended] lazily-zig: pre-existing slot.zig:320 Slot.touch test crash on Zig 0.17.0-dev (installed 0.17.0-dev.892; repo targets 0.16 per commit 9b492d8). 3 of 4 test binaries crash in slot.touch, masking the full suite (signal.zig + state_machine tests pass). Investigate the Slot.touch path under the newer std.Io/async semantics, or pin the CI/dev Zig to 0.16.x until 0.17 is stable. Unrelated to #t0qn (Signal primitive is complete).
- [ ] [#tnbd] #lzjsffi=[recommended] Migrate lazily-js FFI loader off ffi-napi (unmaintained; native addon fails to build on Node >=23 — napi_add_finalizer ABI break, confirmed at Node 26.1.0). The loadAgentDocFFI() path in src/lazily-js/src/state-projection.js is mock-tested + pure-JS at import time, but VSCode consumers on modern Node cannot install ffi-napi to reach the live stateProjection/recordStateEvent calls. Evaluate koffi or node-ffi-rs as the maintained replacement.
- [ ] [#lzshmblobparity] #lzshmblobparity=[recommended] ShmBlobArena host parity for lazily-py + lazily-zig: only lazily-rs implements the owning arena (ipc.rs:150 ShmBlobArena<B>: with_capacity/from_buffer/write_blob:220/read_blob:269); py/zig have only ShmBlobRef + SharedBlob wire tagging (ref-only consumers, cannot host). Port arena 1:1 (py: bytearray+memoryview; zig: allocator-backed slice) + lazily-spec conformance test. Plan: tasks/software/plan-lazily-shmblob-parity.md. Resolves #pwvm/#9e64/#t1xt/#dcgp/#7gpg/#kjfe (IPC.lean protocol already DONE+proven; Lean extraction not indicated).
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
