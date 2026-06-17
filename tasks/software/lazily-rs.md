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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260617-182419.md`*

Compacted content:
- Archived 8 response topic(s): Bug audit + fix; #next-steps; #jknk; 5 more
- Prior summary/context: prior compacted content: Archived 3 response topic(s): release + publish; LinkedIn scheduled post; LinkedIn post repair

### Re: #lzrepourl — Cargo.toml repository URL fixed — glm-5.2

> **Queue prompt:**
>
> [#lzrepourl]

Fixed `Cargo.toml:7` `repository` from the stale `https://github.com/btakita/lazily-rs` to the canonical `https://github.com/lazily-hub/lazily-rs` (matching `book.toml:9-10` and `git remote origin`). The only stale reference in the crate — `rg` across `src/lazily-rs` found no other occurrences.

**Verification:** `cargo build --all-features`, `cargo clippy --all-features --all-targets -- -D warnings`, `cargo fmt --check`, and `cargo test` all pass (metadata-only change).

**Commit + push:** lazily-rs submodule `691fd3b` → `lazily-hub/lazily-rs` main (`1cee080..691fd3b`); superproject gitlink `7e525d275` → `btakita/agent-loop` main (`bfb58e854..7e525d275`).

### Re: #lzloomtimeout — Loom envelope model bounded, now terminates — glm-5.2

> **Queue prompt:**
>
> [#lzloomtimeout]

The `inline_seqlock_envelope_rejects_torn_and_stale_under_concurrent_publish` model (`tests/thread_safe_loom.rs:1451`) never terminated because the full 6-atomic envelope (seqlock + `cache_revision` + `dirty`) across two spawned threads plus the driving body makes the unbounded `loom::model` permutation space non-terminating (>5 min). `loom::model()` defaults `preemption_bound` to unbounded; confirmed via the loom 0.7.2 `Builder` source.

**Fix:** replaced the unbounded `loom::model(|| …)` with a preemption-bounded `loom::model::Builder` (bound 4, `max_duration` 60 s safety cap). The two single-property models (`inline_seqlock_reader_never_observes_torn_value`, `…read_after_completed_invalidation_is_rejected`) stay exhaustive; only the combined envelope model is bounded.

**Bound validation (meaningful, not vacuous):** injected a torn-read bug by disabling the seqlock odd-sequence guard in `seqlock_try_read`; the bounded model still **FAILED** with *"envelope accepted a torn snapshot"* (left: 1, right: 2) — confirming bound 4 exercises the torn-read interleaving. Bug reverted; model passes.

**Verification:** full `thread_safe_loom` suite **15/15 in ~82 s** (was non-terminating); `cargo clippy --features loom --all-targets -- -D warnings` clean; `cargo fmt --check` clean.

**SPEC/docs corrected:** `SPEC.md` Loom-gate bullet and two `src/thread_safe.rs` doc-comments changed from *"modeled exhaustively"* to *"preemption-bounded"* for the combined envelope model (honest about the reduced coverage), with the validation method noted.

**Commit + push:** lazily-rs submodule `641fbf2` → `lazily-hub/lazily-rs` main (`691fd3b..641fbf2`); superproject gitlink `1660e814c` → `btakita/agent-loop` main (`7e525d275..1660e814c`).
<!-- agent:boundary:18a89f95 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- Release + publish
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
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
