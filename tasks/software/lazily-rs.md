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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260616-230815.md`*

Compacted content:
- Archived 3 response topic(s): release + publish; LinkedIn scheduled post; LinkedIn post repair
- Prior summary/context: prior compacted content: Archived 7 response topic(s): Closeout repair; Lean status; Artifact cleanup; 4 more

### Re: Bug audit + fix — ipc dead-code warnings, SPEC drift — opus-4-6

**Bug found and fixed:** `EncodeError` and `DecodeError` enums in `src/ipc.rs` were defined unconditionally whenever the `ipc` feature was enabled, but their variants are gated on `ffi`/`ipc-binary`. Building with `webrtc-str0m` (which enables `ipc` without `ffi`/`ipc-binary`) produced dead-code warnings. Fixed by gating the enum definitions on `#[cfg(any(feature = "ffi", feature = "ipc-binary"))]` to match their existing impl blocks (`1cee080`).

**SPEC.md drift fixed:** `SlotNode`, `CellNode`, and `EffectNode` struct definitions in SPEC.md used `Box<dyn Any>` where the actual implementation uses `Rc<dyn Any>`, and were missing `type_id: TypeId`. Updated to match the code with proper type aliases (`ComputeFn`, `EqualsFn`, `EffectFn`).

**Tests added:** `json_codec` module in `tests/ipc.rs` covering `encode_json`/`decode_json` round-trip, invalid-input rejection, and `Display` impl verification for both error types.

**Audit notes:** Core reactive logic (dependency tracking, memo guard, effect scheduling, invalidation propagation, batch flush) is correct — all 300+ tests pass, clippy clean across all feature combos. No panics on edge cases found. The `ShmBlobArena` wraparound/generation logic is sound.

### Re: #next-steps — follow-up backlog — opus-4-6

Remaining items added to backlog (in priority order). Top of backlog: **#lz57by-close**.

### Re: #jknk — close review #57by (Shuttle async spike NO-GO) — glm-5.1

Review #57by resolved. The Shuttle async spike (#shuttleeval) concluded NO-GO — Shuttle can model async but is randomized-not-exhaustive, needs ~140 cfg-swaps + a custom notifier, and the two #k03k windows are already deterministically covered. The decision is final; no further work needed.

### Re: #tf0k — harden SlotId casts for 32-bit safety — glm-5.1

Replaced all 12 `id.0 as usize` casts in `thread_safe.rs` with `usize::try_from(id.0)` via a `node_index()` helper, matching the single-threaded `Context::node_index` pattern. Getters (`get_node`, `get_node_mut`, `slot_fast_path`, `cell_fast_path`) return `None` on overflow via `?`; setters use `.expect("SlotId does not fit usize")`. This prevents silent truncation on 32-bit targets. Commit `8665e29`.

### Re: #j1vw — From impls for EncodeError/DecodeError — glm-5.1

> **Queue prompt:**
>
> [#jknk]
>
> [#tf0k]
>
> [#j1vw]


Added `From<serde_json::Error>` (cfg `ffi`) and `From<postcard::Error>` (cfg `ipc-binary`) for both `EncodeError` and `DecodeError` in `src/ipc.rs`, enabling ergonomic `?` propagation from serde/postcard call sites. Commit `8665e29`.

**Verification:** fmt, clippy (`-D warnings`), build (all features), and 300+ tests pass across default/tokio/async feature combos. Loom exhaustive model tests: 14/15 passed; the last (`inline_seqlock_envelope_rejects_torn_and_stale`) timed out at 5min — a known resource-intensive exhaustive check, not a regression from these changes.

### Re: #advance-review — review audit + actionable backlog — glm-5.1

> **Queue prompt:**
>
> Go through the review items. Add actionable backlog items to complete the review items.


Reviewed all 4 `[operator-drive]` review items. One ship-stopper found, one review item updated, two confirmed purely operator-gated with no agent-doable slice.

**#kb9b (#lzpypub011) — SHIP-STOPPER:** Validated the v0.11.0 dist artifacts (`twine check` PASSED for both whl + tar.gz). But install-in-venv round-trip revealed `Signal` reading a `Slot` dependency hits `RecursionError`. Root cause: `Slot.__call__` fires `self.touch(resolved)` immediately after computing (slot.py:98), which notifies subscribers → `_subscriber` calls `parent_slot.reset(ctx)` → `Slot.reset` calls `self.touch()` again before `_subscribers.clear()` runs (slot.py:115) → re-entrant cascade through the `_SignalSlot` ↔ source-`Slot` subscription cycle. `Cell` doesn't trigger this because it overrides `__call__` to skip `touch` on reads. The existing 8 signal tests only cover `Signal`→`Cell` dependencies, so this escaped. The fix + regression test is queued as **#lzpysigfix** — the publish MUST wait for this.

**#kabc (#lipostlazilyarch):** Pure operator action — post a LinkedIn comment via `corky`. Comment body already drafted in `mail/drafts/linkedin/`. No agent-doable slice; stays operator-gated.

**#h6qb (#lzwebrtcnet-e2e):** The agent-doable slice (**#lzwebrtcwire**) is already DONE — `src/webrtc_signaling.rs` wires `SignalingClient`↔`Str0mNet::{offer,answer,accept_answer,add_remote_candidate,local_candidate}` and `tests/webrtc_signaling.rs` covers the localhost handshake. Review text updated to reflect completion; only the live two-host/NAT run remains (operator-only).

**#xccx (#lzholons):** Blocked on Steve Melville answering 7 OPEN QUESTIONS. The plan (`tasks/software/plan-map-holons-integration.md`) explicitly says "use [the] direct line to resolve the OPEN QUESTIONS rather than guessing." No agent-doable slice until answers land; stays operator-gated.

### Re: #syej — Signal→Slot RecursionError fixed — glm-5.1

> **Queue prompt:**
>
> :pushpin: What are #next-steps to fix lazily-rs bugs?


> **Queue prompt:**
>
> [#syej]


**Ship-stopper resolved.** The `RecursionError` when a `Signal` reads a `Slot` dependency is fixed (commit `30907ea` in lazily-py, gitlink `eb93a75ac` in superproject).

**Root cause:** `Slot.__call__` fired `self.touch(resolved)` immediately after computing — still inside the `try` block with the slot on `slot_stack`. When a `_SignalSlot` read a `source` Slot during Signal construction, `source` computed, then `touch()` notified its subscriber (`reset _SignalSlot`), which cascaded through `Slot.reset` → `touch()` (before `_subscribers.clear()`) → mutual recursion. `Cell` avoided this because its `value` getter never touches on read.

**Two-part fix** (`src/lazily/slot.py`):
1. **Removed `touch()` from `Slot.__call__`** — computation must not cascade invalidation. Only explicit `reset` propagates (consistent with the SPEC, which never documented `__call__` touching).
2. **`Slot.reset` clears subscribers before notification** — a subscriber that triggers a re-entrant reset finds an empty subscriber set, breaking the mutual recursion.
2. **#lzloomtimeout (verification gap):** the Loom exhaustive model `inline_seqlock_envelope_rejects_torn_and_stale_under_concurrent_publish` (`tests/thread_safe_loom.rs:1451`) never completes — confirmed it runs >5min without finishing. The 2 writer/reader threads + main body blow up Loom's state space, so the torn/stale-read invariant for the inline seqlock is *not* exhaustively proven today. Non-exhaustive stress tests pass, so this is a proof-coverage gap, not a known runtime race. Fix by bounding preemptions (`loom::model_g`) or factoring the model to shrink interleavings.
1. **#lzrepourl (metadata bug):** `Cargo.toml:7` `repository = "https://github.com/btakita/lazily-rs"` is stale — the actual remote is `lazily-hub/lazily-rs` (confirmed via `git remote -v`). Publishes a wrong repo link to crates.io/docs.rs. Trivial one-line fix; land before any crates.io publish.

**Test added:** `test_signal_depends_on_slot` — constructs a `Signal` with a `@slot` dependency (the exact scenario that crashed), asserts the value, and verifies caching + slot reset behavior.

**Verification:** 58/58 tests pass, `ruff check` clean, `ty check` clean. `twine check` PASSED on both rebuilt dist artifacts. Fresh-venv install round-trip confirmed `Signal→Slot` works without recursion.

SPEC + CHANGELOG updated to document the re-entrancy-safe invalidation semantics. Review #kb9b updated: ship-stopper cleared, dist artifacts rebuilt and verified.
<!-- agent:boundary:d3707bbf -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzrepourl] #lzrepourl=Fix stale Cargo.toml repository URL (line 7: points to https://github.com/btakita/lazily-rs; actual git remote origin is lazily-hub/lazily-rs). Publishes a wrong repo link to crates.io/docs.rs. One-line fix; land before any crates.io publish.
- [ ] [#lzloomtimeout] #lzloomtimeout=Bound the Loom exhaustive model for inline_seqlock_envelope_rejects_torn_and_stale_under_concurrent_publish (tests/thread_safe_loom.rs:1451) — 2 writer/reader threads + main body blow up the state space and never complete (>5min confirmed), leaving the torn/stale-read invariant for the inline seqlock without an exhaustive proof. Use loom::model_g preemption bounds or factor the model to shrink interleavings. Verification-coverage gap, not a known runtime race (non-exhaustive stress tests pass).
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
- [ ] [#pw26] #lzmapspike=[recommended] Spike: lazily-rs as a host-side reactive derivation + Snapshot/Delta UI-sync cache for MAP holons (evomimic/map-holons, Holochain). Model committed holons as reactive graph nodes; derived holon views/relationships as memoized computeds that recompute only on upstream change; stream lazily-ipc Snapshot/Delta to the TS UI / DAHN graph visualizers. Native host/ layer only (AsyncContext needs tokio; zomes are wasm32/short-lived/single-threaded so in-zome use is limited to single-threaded Context). Do NOT use lazily distributed/signaling — Holochain DHT owns P2P/persistence/validation/consensus. Gate on MAP host-layer actually recomputing derived holon state.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
