---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: claude
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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260626-163523.md`*

Compacted content:
❯ - Archived 9 response topic(s): lazily-py parity + release + publish; IPC proof diagnostic (interrupted-cycle recovery); Blog post; 6 more
❯ - Prior summary/context: prior compacted content: Archived 8 response topic(s): next steps to fix lazily-rs bugs; closeout repair; IPC proof d...

### Re: compressed lazily-js library payload size — opus-4-8

> **Queue prompt:** 🚧 How many bytes is the compressed lazily-js library payload?

`@lazily-hub/js@0.1.0` is pure ESM (no bundler) — the runtime library is two files, `src/index.js` + `src/state-projection.js` (27,110 bytes raw uncompressed). Compressed:

| Payload | gzip -9 | brotli -11 |
|---|---|---|
| `index.js` (17,641 B) | 3,299 B | 2,946 B |
| `state-projection.js` (9,469 B) | 3,296 B | 2,898 B |
| **Runtime library total** | **6,433 B (~6.3 KB)** | **5,779 B (~5.6 KB)** |

So the **compressed library payload is ~6.4 KB gzipped (6,433 bytes)** / ~5.8 KB brotli (5,779 bytes) — that's the JS a consumer actually imports and runs (koffi, the one runtime dependency, is separate and not counted here).

For contrast, the **published npm tarball** (`npm pack`) is **16,355 bytes** gzipped (68,695 unpacked, 15 files) — but that figure is inflated because the package has no `files`/`.npmignore`, so it also packs the test suite (`test/ipc.test.js` + `test/state-projection.test.js` ≈ 21 KB raw), the conformance JSON fixtures, `AGENTS.md`, and the `.d.ts` type stubs. None of that runs in a consumer; adding `"files": ["src/*.js", "src/*.d.ts"]` would drop the tarball to roughly the runtime + types (~3–4 KB packed). Flagging as an observation — not changing packaging unprompted.

### Re: Does lazily-rs benchmark well at 10M-cell recalc vs. other reactive libraries? — opus-4-8

> **Queue prompt:**
>
> ~~Does lazily-rs benchmark well with the 10M cell recalc compared to other reactive libraries?~~ — auto-struck: answered this cycle (#ftstrike)


> **Queue prompt:** Does lazily-rs benchmark well with the 10M cell recalc compared to other reactive libraries?

Short version: lazily-rs's **own** 10M numbers are strong, but there is **no apples-to-apples head-to-head in this repo** — `src/lazily-rs/benches/` only holds lazily's own benchmarks (`scale.rs` is the 10M one), and `BENCHMARKS.md` has no SolidJS / MobX / Preact-Signals / Reactively comparison. So a literal "beats library X at 10M" claim isn't backed by existing data. Here's what the data + architecture do support.

**Measured at Google-Sheets scale (10M cells = 5M inputs + 5M formulas, single criterion run, 186 GB host):**

| case | mean | per cell |
|---|---:|---:|
| `build` (10M cells) | ~706 ms | ~71 ns |
| `cold_full_recalc` (5M) | ~518 ms | ~104 ns |
| `full_recalc_invalidate_all` (5M) | ~329 ms | ~66 ns |
| `viewport_recalc` (1k) | **~11.4 µs** | ~11 ns |

So it backs a full-capacity Google Sheets workbook: build < 1 s, full recompute ~0.5 s.

**Why that's favorable against the broad reactive-library landscape:**
1. **It's lazy / pull-based (demand-driven).** The headline isn't the 0.5 s full recompute — it's that `viewport_recalc` is **~11 µs and independent of sheet size** (also ~11.5 µs at 1M). Editing one input and reading a 1k-cell viewport leaves every off-viewport formula dirty and never recomputes it → cost is O(cells you read), not O(sheet). Eager / push (or hybrid) signal systems — Solid, Preact Signals, MobX — do more work per edit at this scale. The design family lazily actually competes in is the lazy memo-based one (**Reactively / milomg**, **S.js**); lazily matches that demand-driven property in native Rust.
2. **Flat per-node cost + sparse arena.** ~70–100 ns/cell held ~constant from 1M → 10M (linear, no degradation at capacity), and storage is a sparse `Vec<Option<Node>>` arena (~216 B/node), so only populated cells allocate. The standard cross-library reactivity benchmarks (js-reactivity-benchmark, cellx) measure *propagation through small/medium graphs* — diamond/fan-in shapes, thousands of nodes — not a dense 10M-node graph (which is memory-prohibitive in a JS heap and outside what those libs are built/measured for). At 10M, lazily isn't really racing them; it's in Rust-native, spreadsheet-scale territory most reactive libraries are neither designed nor benchmarked to enter.

**Honest caveat:** because no 10M head-to-head exists, the defensible statement is "lazily backs a full-capacity Google Sheets workbook with microsecond viewport edits — something most reactive libraries are not benchmarked to do," **not** a ranked "faster than X." If you want a genuine comparison I'd port the `scale` graph shape (N inputs + N formulas, `formula[i] = input[i] + input[i-1]`) into a harness running **Reactively** and/or a Rust reactive lib (`leptos_reactive` / `futures-signals`) at matched N and add a comparison table to `BENCHMARKS.md`. That's a real piece of work, not a doc tweak — say the word (e.g. queue `#lzscalecompare`) and I'll build it; I'm not auto-filing it into this go-queue so it doesn't silently start.
❯ do #lzscalecompare

### Re: do #lzscalecompare — built the cross-library benchmark — opus-4-8

Built the head-to-head and landed it. New `benches/scale_compare.rs` runs the **identical** spreadsheet graph (`N` inputs + `N` formula memos, `formula[i] = input[i] + input[i-1]`) for both **lazily** and **`leptos_reactive`** (Leptos 0.6 fine-grained reactivity) in the same criterion harness on the same host. `leptos_reactive` is the fair apples-to-apples pick — it's also a **lazy pull-based memo** system, so the comparison isolates per-node overhead + the lazy-pull property rather than racing pull-vs-push. Gated behind a new `scale-compare` feature (optional dep) so normal builds / `make check` never pull it.

Results at **N = 100,000** (200k nodes/lib; leptos is far heavier per node, so this size keeps its wall clock feasible — lazily's own 1M/10M numbers stay in the Scale section):

| case | lazily | leptos_reactive | ratio |
|---|---:|---:|---|
| `build` | **8.58 ms** | 12.89 ms | lazily **1.5×** |
| `cold_full_recalc` | **8.45 ms** | 30.06 ms | lazily **3.6×** |
| `full_recalc_invalidate_all` | **6.26 ms** | 17.29 ms | lazily **2.8×** |
| `viewport_recalc` (edit 1, read 1k) | 11.5 µs | **8.2 µs** | leptos **1.4×** |

**Honest read:** lazily wins the bulk-graph ops — build (1.5×), cold compute (3.6×), full recalc (2.8×) — from its sparse arena + lean single-threaded `Context` vs leptos's runtime slotmap + subscriber bookkeeping. On the cached-read-dominated `viewport_recalc` the two are close and leptos is a touch faster (only ~2 of the 1,000 viewport cells actually recompute; its cache-hit read path is slightly leaner at this size). Both share the lazy-pull headline: a one-input edit + bounded viewport read is **microseconds**, ~1000× cheaper than a full recalc and independent of sheet size. So the defensible claim is "materially higher whole-graph throughput than a comparable native-Rust pull-based reactive system, matched on incremental viewport reads" — **not** "fastest reactive library." (The result that leptos *wins* one case is the proof this isn't cherry-picked: its 30 ms cold recalc also confirms its memos genuinely recompute, so the harness is fair.) JS libs (Solid/MobX/Preact) are excluded as a different runtime.

**Landed:** `lazily-rs` `a1d6bf6` (bench + feature-gated dep + BENCHMARKS.md `### Cross-library comparison`), submodule pointer bumped (`6c963efe2`). clippy clean; all `make check` tests pass. No crates.io release (dev/bench-only, no library code change). Reproduce: `cargo bench --features scale-compare --bench scale_compare`.

**One caveat:** `make check`'s `benchmark-check` step is red, but on a **pre-existing** criterion-data staleness — `--check` just compares the committed machine-specific numbers against whatever is in `target/criterion`, and it fails identically with my changes stashed (verified). My diff is additive-only and touches neither the generated results section nor any benched library code. Refresh it with `make benchmark-update` on an unloaded host when convenient.
❯ Fix make check. Add benchmark Honest read to docs.

### Re: Fix make check + add benchmark Honest read to docs — opus-4-8

Both done; landed in `lazily-rs` `3d124e6` (gitlink `7d45c704c`).

**1. Fixed `make check` (`benchmark-check`).** My earlier "pre-existing" read was wrong — the real cause was my own `cargo bench --features scale-compare` run: it wrote `scale_compare/*` estimates into `target/criterion`, and the results generator (`scripts/update-benchmark-results.py`) globbed **every** `**/new/estimates.json` group with no allowlist, so the comparison bench's numbers leaked into the auto-generated results table → it no longer matched the committed section → `--check` flagged stale. (Stashing the *source* didn't clear it because that doesn't remove the *criterion data* — which is what misled me.) Fix: the generator now skips `EXCLUDED_GROUPS = {"scale_compare"}` in `discover_results` — the cross-library head-to-head is documented in BENCHMARKS.md prose, not a tracked lazily benchmark, so running the comparison bench can never make `benchmark-check` stale again. Verified `python3 scripts/update-benchmark-results.py --check` exits 0 *with* the scale_compare criterion data present, and a full **`make check` is green (EXIT=0)**.

**2. Added the "Honest read" to docs.** BENCHMARKS.md's Cross-library section now has a clearly-labeled **`Honest read:`** callout (was "Reading the result honestly"), with the fairness point made explicit: leptos *winning* the viewport case — and its 30 ms cold recalc proving its memos genuinely recompute — is the evidence the comparison isn't cherry-picked. The balanced claim stands: lazily has materially higher whole-graph throughput than a comparable native-Rust pull-based reactive system and matches it on incremental viewport reads — not "fastest reactive library."

**⚠️ One thing I could NOT fix — a separate, unrelated blocker.** When I re-ran `make check` I hit a second failure that is **not mine**: an untracked `benches/ipc_transport.rs` (it was already `??` at session start, from concurrent/unmerged IPC work) fails `fmt` then `clippy`/`build` because it imports `lazily::{IpcTransport, InProcessIpcTransport, IpcControlFrame, IPC_DEFAULT_QUEUE_CAPACITY}` — symbols that **don't exist in this tree's source** (it's written against an unmerged IPC transport API). Since `clippy`/`build` use `--all-features --all-targets`, that `#![cfg(feature="ipc")]` bench is always built and can't compile here. I verified `make check` is green with it isolated, then **restored it untouched** (I won't complete or commit another session's WIP, and I won't silently hide it). To get `make check` fully green its owner needs to land the IPC transport API, or gate/remove the bench. No crates.io release (dev/docs/bench-tooling only).

### Re: Update lazily-py README with the latest lazily-spec + family docs — opus-4-8

Done — rewrote `src/lazily-py/README.md` (was 73 lines documenting only the basic lazy example; now covers the full current surface) and pushed it.

**What was stale:** the README only showed `CellSlot`/`slot`/`cell` lazy evaluation. It never mentioned anything added through 0.11.0+ — the eager `Signal` primitive, `lazily.ipc`, `ShmBlobArena`, or the cross-language family.

**Brought in, mirroring the `lazily-rs` README structure with Python-accurate API:**
- **`Slot → Cell → Signal` family** — overview + a Core Concepts section per primitive (tables of the actual exported types/decorators), plus a runnable eager-`Signal` example (`@signal`, eager materialization, `v1 → v2` no-unset-window, memo guard, `dispose()` → lazy).
- **`StateMachine`** — brief section (Cell-backed FSM, `send`/`state`, PartialEq self-transition guard).
- **`lazily.ipc` — the `lazily-spec` wire protocol** — Snapshot/Delta, the 7 `DeltaOp` variants, epoch sequencing, byte-compatibility with rs/zig/TS bindings, conformance fixtures, a working `IpcMessage.of_snapshot/of_delta` + `encode_json`/`decode_json` example, and the default-deny `PeerPermissions` boundary.
- **`ShmBlobArena`** — Python-hosted shared-memory blob arena (40-byte `LZSH`/FNV-1a-64 header, append-only + wraparound, rs/zig byte-compatible).
- **The lazily family** — table linking `lazily-rs` / `lazily-py` / `lazily-zig` / `@lazily/signaling` / `lazily-spec`, pointing at the spec's canonical schemas + Lean formal model.
- Tagline, PyPI badge, and a Development section using `uv run poe precommit`.

**Verification:**
- Both new code blocks (Signal + IPC) executed against the installed package — pass.
- Preserved the `tests/run_readme.py`-validated quick-start example verbatim → `run_readme` passes.
- Full test suite: **77 passed**.
- Pushed: `lazily-hub/lazily-py` `c255482..ed786cc` (`docs: refresh README …`).

**Note (out of scope, pre-existing):** `poe precommit`'s `ty` step reports 2 type errors in `src/lazily/state_machine.py` (`subscriber` callback shape vs `CellSubscriber[T]`). They reproduce with my change stashed, so they predate this README task — not touched here. Flag if you want them fixed in a follow-up.

Queue stays paused (the `release + publish` heads remain operator-gated), so I'm not auto-looping.
<!-- agent:boundary:4ad9044b -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- 🚧 Update lazily-py README with the latest docs using the latest changes using lazily-spec + the lazily family.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#z0hq] #lzjsnpmpub=[operator-drive] npm publish @lazily-hub/js v0.1.0 (FIRST publish; scoped). The @lazily scope was taken on npmjs, so the package was renamed @lazily/js -> @lazily-hub/js (lazily-js ac97122 pushed; tag v0.1.0 + GitHub release moved to it; npm view @lazily-hub/js = 404/free; tests 28/28). npm is E401 (not logged in); btakita npm 2FA is browser/passkey. Steps: in a real terminal `! npm login` (approve 2FA), then `cd src/lazily-js && npm publish --access public`.
- [/] [#lilazilysmpost] [/] [operator-drive] lazily state-machine LinkedIn post PUBLISHED WITH IMAGE: https://www.linkedin.com/feed/update/urn:li:share:7475622100263923712 (mail/social/20260624-142530-linkedin.md @ mail bf4ab26). CyclePhase diagram rendered from blog post SVG (light theme colors) + uploaded via LinkedIn digitalmediaAsset API. Old text-only post (urn:li:share:7475617202893639680) deleted + replaced. Remaining operator follow-up: post first comment (link + GitHub source) via corky linkedin comment.
- [/] [#kabc] #lipostlazilyarch=[operator-drive] Main LinkedIn post is published and repaired: Corky edit updated mail/social/20260616-lazily-architecture-linkedin.md / https://www.linkedin.com/feed/update/urn:li:share:7472666756810190848 with the full body after the public post truncated at "The core is small: three primitives". Local metadata is committed in mail commit 07a9ea1 and superproject gitlink e7e0b08c1. Remaining operator follow-up: post the first comment from mail/drafts/linkedin/2026-06-16-lazily-architecture.md via `corky linkedin comment <published-file> <comment-body>` if still desired.
- [/] [#h6qb] [recommended] [operator-drive] #lzwebrtcnet-e2e=End-to-end cross-host validation of the Str0mNet backend through the LIVE #yxjw signaling Worker (two real hosts / NAT). Agent-doable slice (#lzwebrtcwire) is DONE: src/webrtc_signaling.rs wires SignalingClient↔Str0mNet and tests/webrtc_signaling.rs covers the localhost handshake. Remaining: live two-host/NAT run via deployed Worker — operator-only.
- [/] [#xccx] #lzholons=[operator-drive] Map Holons ↔ lazily-rs integration. Phase 0 (discovery) is blocked on Steve Melville answering the 7 OPEN QUESTIONS (holon identity, holon↔node/edge mapping, change feed, write/conflict model, permissions mapping, preferred transport, spec reference). Plan: tasks/software/plan-map-holons-integration.md. Unblocks once answers land; then Phase 1 read-only inbound adapter.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
- [ ] [#ppce] Write a blog post + linkedin teaser about the 1M+ Cell Family performance.
- [ ] [#lzsidecarlock] [recommended] agent-doc-repo: close the lost-update window among concurrent cycle_state writers. Deadlock analysis DONE (tasks/agent-doc/plan-cycle-state-flock.md): NO production mutator calls another (only test code) -> non-reentrant flock is safe. Design: bounded try_lock_exclusive (~5s, fs2) + RAII guard on a dedicated .json.lock sibling (state file is atomically renamed each save -> cannot lock it directly); add `let _lock = acquire_state_lock(file)?;` to each of the ~24 public mutators in cycle_state.rs. cycle_phase_rank (:1273) guards only monotonic phase, not the ~30 non-phase fields. Hardening (not live-fire). Split from #lzsidecaratomic (DONE in agent-doc 647af082 + installed).
- [ ] [#pw26] #lzmapspike=[recommended] Spike: lazily-rs as a host-side reactive derivation + Snapshot/Delta UI-sync cache for MAP holons (evomimic/map-holons, Holochain). Model committed holons as reactive graph nodes; derived holon views/relationships as memoized computeds that recompute only on upstream change; stream lazily-ipc Snapshot/Delta to the TS UI / DAHN graph visualizers. Native host/ layer only (AsyncContext needs tokio; zomes are wasm32/short-lived/single-threaded so in-zome use is limited to single-threaded Context). Do NOT use lazily distributed/signaling — Holochain DHT owns P2P/persistence/validation/consensus. Gate on MAP host-layer actually recomputing derived holon state.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
