---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260605-000338.md`*

Compacted content:
- Archived 2 response topic(s): [#4cn0] Build Cloudflare Worker signaling server (#yxjw); do [#mb9b] Lock-free cached reads in ThreadSafeContext (#vd5v)
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: do [#xtwf] Rigorous A/B for #vd5v lock-free reads — opus-4-8

Done — ran the isolated-worktree Criterion A/B and it changed the conclusion. Committed the corrected SPEC + gate to `lazily-rs` `main` (`0d590dc`).

**Method:** `git worktree` at `06fd3c2` (the `parking_lot::RwLock` parent) vs HEAD `463ca71` (arc-swap), one shared `CARGO_TARGET_DIR`, same host/toolchain, Criterion `--save-baseline`/`--baseline`.

**Result — it's a contention tradeoff, not a free win:**

| benchmark | arc-swap vs RwLock |
| --- | --- |
| `cached_reads/thread_safe_context` (1 thread) | **+3.1%** slower |
| `read_mostly_waiters/4` | **+11.9%** slower |
| `read_mostly_waiters/8` | **−6.5%** faster |
| `read_mostly_waiters/16` | **−28.6%** faster |

My earlier "−2.2% improved" was against a stale Criterion baseline (not a clean before/after) — **it was wrong**, and I've corrected the record. arc-swap's wait-free read wins where `RwLock` read-lock cache-line traffic dominates (8+ cores) but costs a few percent uncontended (debt-tracking load + `Arc<Arc<…>>` double-indirection).

**Decision (yours):** keep `463ca71` and treat this as a documented tradeoff, since `ThreadSafeContext` targets highly-parallel reactive graphs. Applied:
- **spec** — rewrote the `cached_reads/thread_safe_context` gate from "must not regress" to a **bounded-regression** gate (≤~3% uncontended, no regression in the 8/16-worker read-contention matrix), and recorded the full A/B table under *Lock strategy evaluation*.
- **benchmark** — A/B table lives in SPEC (authoritative); left the auto-generated `BENCHMARKS.md` untouched (regenerating commits machine-timing noise + it's pre-existingly stale).
- **commit + push** — `0d590dc` on `lazily-rs` `main`.

Note: `463ca71`'s commit message still reads "no regression / improved" (immutable pushed history); the SPEC at HEAD is now the accurate record.

Next queue head: `do [#s0fc]` — make the Cloudflare signaling endpoint consumable as a dependency (for agent-doc's distributed plans).

### Re: do [#s0fc] Make the signaling endpoint consumable as a dependency — opus-4-8

Done (Both: Rust crate + TS package, per your scope choice). Committed + pushed to `lazily-rs` `main` (`cbdb4b8`). Two clients now consume the #yxjw endpoint over one shared, normative wire protocol.

**Rust client** — `signaling-client` feature, `src/signaling_client.rs`:
- `lazily::SignalingClient::connect(base_url, session, peer)` opens a `tokio-tungstenite` WebSocket to `{base_url}/session/{session}`, joins, and exposes `offer`/`answer`/`ice`/`relay`/`leave` + `recv()` for `ServerMessage`s.
- `ClientMessage`/`ServerMessage` are `serde`-tagged (`kebab-case`) and reuse the `#39c5` `PeerId`; rustls/tokio-tungstenite are pulled in **only** when the feature is enabled, default build untouched.
- Conformance tests assert the exact JSON frame shapes. agent-doc can now `lazily = { features = ["signaling-client"] }`.

**TypeScript client** — `@lazily/signaling` package, `signaling/src/client.ts`:
- `SignalingClient.connect(baseUrl, session, peer)` (or `attach(socket, peer)` for a pre-opened socket) over any `WebSocket`-like transport (browser / Node ≥ 22 / injected), with `onMessage` + the same send helpers.
- Package is now publishable (`@lazily/signaling`), exporting `./client` and `./protocol`.

**Wire protocol** is the single source of truth (new SPEC *Consumable clients* section): `PeerId` is a bare JSON number (Rust `u64` ⇄ TS `number`); both client impls conform byte-for-byte.

**Preset `#spec-test-build-install-commit-push`:**
- **spec** — SPEC.md *Consumable clients (#s0fc)* section (normative protocol + both clients).
- **test** — Rust: 3 conformance tests (`cargo test --features signaling-client`); TS: now **31 tests** (added 6 client unit + 1 end-to-end client↔Worker↔DO in `workerd`). `make check` green except the pre-existing `benchmark-check` timing-drift staleness.
- **build/install** — `cargo build --features signaling-client` clean (rustls behind the feature); `npm run check` clean. Both are libraries/deps, no binary install.
- **commit + push** — `cbdb4b8`; CI wired for both (`test-signaling-client` step + the Worker job's `npm run check`).

Queue drained — `#xtwf` and `#s0fc` both consumed.

### Re: release + publish — opus-4-8

Released and published **lazily v0.9.0**.

- **Version** — `0.8.0 → 0.9.0` (minor; all changes since 0.8.0 are additive feature flags). Added `exclude = ["signaling/"]` so the standalone TS Worker/client subtree stays out of the published crate (verified: 0 signaling files in the package, 34 files / 139 KiB compressed).
- **Verify** — `cargo publish --dry-run` compiled the packaged crate clean; the session's full suite (default 120 + tokio + async + loom 12 + distributed 11 + signaling-client 3, and 31 TS tests) is green. `make check`'s only red remains the pre-existing host-sensitive `benchmark-check` staleness (you chose full publish over regenerating BENCHMARKS.md).
- **Commit + tag** — `d17e09c` "release v0.9.0", tag `v0.9.0`, pushed to `main`.
- **crates.io** — `cargo publish` → **lazily v0.9.0 published** (irreversible; yank-only).
- **GitHub release** — https://github.com/btakita/lazily-rs/releases/tag/v0.9.0 (generated notes).

What shipped since 0.8.0:
- `distributed` — `RemoteOp` + per-peer permission allowlist (#39c5)
- `signaling-client` — async Rust client for the signaling endpoint (#s0fc)
- arc-swap lock-free `ThreadSafeContext` cached reads (#vd5v — a documented contention tradeoff after the #xtwf A/B)
- (in-repo, not in the crate) the #yxjw Cloudflare Worker + `@lazily/signaling` TS client under `signaling/`

Queue fully drained. Note: `@lazily/signaling` (the npm package) is **not** published — only the Rust crate went to crates.io. Say the word if you want the TS package published to npm too.
<!-- agent:boundary:61cb466e -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~do [#xtwf]~
- ~do [#s0fc]~
- release + publish
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#ipc1] [recommended] Design lazily-serde feature gate (trait bounds vs type-erased closures)
- [/] [#ipc2] [recommended] Design lazily-ipc snapshot + incremental update protocol
- [/] [#ipc3] [recommended] Investigate CRDT vs Raft trade-offs for lazily-distributed
- [/] [#39c5] #sig1 [recommended] Implement RemoteOp enum + per-peer permission allowlist for lazily-distributed remote access
- [/] [#yxjw] #sig2 [recommended] Build Cloudflare Worker signaling server for internet-scale peer discovery
- [/] [#vd5v] Future: leverage typed cache for lock-free cached reads in ThreadSafeContext (#lazilyperf38)
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
