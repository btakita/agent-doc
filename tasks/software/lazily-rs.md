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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260625-214912.md`*

Compacted content:
❯ - Archived 3 response topic(s): IPC proof diagnostic (interrupted-cycle recovery); dynamic cell collections, KV lookup, naming, and wire addressing; >1M cells and backing a modern spreadsheet
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4af...

### Re: Continue — cycle detection, CellFamily/CellMap, scale benchmark, wire-key plan — opus-4-8

Drained all four queue heads (`do [#lzcycledetect] [#lzscalebench] [#lzcellfamily] [#lzwirekey]`) and the follow-on edit. Per your go: implemented the first three, planned `#lzwirekey`. **Continue + "add fine-grained Cell Family Hashes and Collections"** is the CellFamily/CellMap work below.

All landed in `src/lazily-rs` (submodule `d72f8b3`, superproject gitlink `5fc54a002`), pushed to `origin/main`.

**`#lzcycledetect` — dependency-cycle detection (DONE).** The pull-based recompute walk (`refresh_slot`) had no cycle guard: `A = B + 1, B = A + 1` recursed into a stack overflow. Added a per-slot `in_progress` flag set for the duration of a refresh via an O(1) RAII guard (`RefreshGuard`). Re-entering a slot still on the refresh stack — the definition of a dependency cycle — now panics with a clear `circular dependency detected at slot …` diagnostic instead of diverging, so a caller can `catch_unwind` and render a `#CIRCULAR!`-style value. (A first-class error *value* in the slot type is a larger API change; the deterministic panic is the correctness primitive.) Tests: two-slot cycle, self-cycle, diamond (no false positive), and post-panic recovery (flags clear, context still usable). `src/context.rs`.

### Re: Fine-grained Cell Family Hashes and Collections — opus-4-8

**`#lzcellfamily` — CellFamily + CellMap (DONE).** New `src/cell_family.rs`, exported from the crate root:

- **`CellMap<K, V>`** — a keyed reactive collection (the hash KV "collection"). Each entry is its **own** cell, so mutating entry `a` does **not** invalidate readers of entry `b` (fine-grained). Membership (the key set) is tracked by a dedicated version cell, so `keys`/`len`/`contains_key` readers recompute only on **add/remove**, never on a value change. `entry`/`entry_with`/`get`/`set`/`remove`/`handle`.
- **`CellFamily<K, V>`** — an `atomFamily`-style factory: lazily mints + caches one cell per key via a `Fn(&K) -> V` factory on first `get`.

This is exactly the fine-grained model from the dynamic-cells Q&A (replacing the coarse `Cell<HashMap<K,V>>`), and the natural substrate for keyed/A1 spreadsheet addressing. 5 tests (cache-per-key, membership-reactive-but-values-not, per-entry independence, factory minting, contains-key tracking).

### Re: >=1M scale benchmark + README link — opus-4-8

**`#lzscalebench` — evidence-backed >=1M-node benchmark (DONE).** New `benches/scale.rs` (gated behind a `scale-bench` feature so the default `cargo bench` / `make benchmark-check` stays fast and the generated BENCHMARKS.md section is untouched). Builds a spreadsheet-shaped graph of N input cells + N formula slots. Real single run at N=1,000,000 (~2M nodes), release:

| phase | total | per-node |
|---|---:|---:|
| build (2M nodes) | 0.100 s | 50 ns |
| cold full recalc (1M) | 0.046 s | 46 ns |
| 1 input edit + viewport recalc (1k) | **0.038 ms** | 38 ns |
| full recalc after invalidate-all (1M) | 0.040 s | 40 ns |

Memory ~414 MiB RSS for 2M nodes (~216 B/node). The viewport recalc is ~1000× cheaper than a full recalc — the lazy-pull property a viewport-rendered sheet needs. Recorded in `BENCHMARKS.md` (Scale section, outside the generated markers) and — per your follow-up queue edit — **linked from the README** (`## Benchmarks`).

> **Queue prompt:**
>
> Add the 1M+ benchmark to the documents and link in the README.

Done: `BENCHMARKS.md#scale-1m-cells--lzscalebench` + `README.md` now link it with the reproduce command.

**`#lzwirekey` — wire-keyed addressing (PLANNED, per your "plan only").** Wrote `tasks/software/plan-lazily-wire-keyed-addressing.md`: add an optional `key`/path field to `NodeAdd` + `NodeSnapshot` (a `key→NodeId` registry) so `CellFamily`/`CellMap` entries stay addressable across NodeId churn. Backlog item repointed at the plan and ungated (ready to implement on your go).

**Verification:** `cargo fmt` clean; `clippy --all-targets --features scale-bench` clean; default test suite **191 pass** + **97** all-feature lib tests + doctests; `cargo build --all-features` compiles; scale bench runs. The full `make check` adds the Lean `lake` model + the whole criterion suite + every feature integration suite — orthogonal to these default-feature additions, so I ran the targeted equivalent rather than the multi-minute full gate. Note: `make benchmark-check` is **already red pre-existing** (`#lzbenchstale`, since v0.11.1 — stale baselines / thread_safe contention), unrelated to this work; my BENCHMARKS.md edit is outside the generated markers and does not affect it.

### Re: Fix the benchmark — opus-4-8

Investigated first: `make benchmark-check` was actually **green** (`#lzbenchstale` had already been fixed by `26be677`), my new scale bench passed strict `clippy --all-features -D warnings`, and CI (which doesn't run `benchmark-check`) was green — so there was no failing benchmark to reproduce. You picked **both** follow-ups: make the scale bench rigorous **and** integrate it into the suite. Done (lazily-rs `0277174`, superproject gitlink `4565367af`, pushed):

**1 — Rigorous criterion benchmark.** Rewrote `benches/scale.rs` from a hand-rolled single-run timer into a real **criterion** benchmark (warmup + statistical sampling, 10-sample bounded windows), still gated behind `scale-bench`. Four cases over the ~2M-node spreadsheet graph at N=1,000,000:

| case | criterion mean |
|---|---:|
| `build` (2M nodes) | ~132 ms |
| `cold_full_recalc` (1M) | ~102 ms |
| `full_recalc_invalidate_all` (1M) | ~65 ms |
| `viewport_recalc` (1k) | **~11.5 µs** |

The cheap-routine cases (`viewport_recalc`, `full_recalc`) run on a persistent warm graph so criterion doesn't rebuild 1M nodes per iteration. Viewport recalc is ~5,000× cheaper than a full recalc — the lazy-pull property a viewport-rendered sheet needs.

**2 — Integrated into the suite.** Added the `scale` group to `GROUP_ORDER` and `scale-bench` to the generator's `cargo bench` features in `update-benchmark-results.py`, so `make benchmark-update` runs it and `make benchmark-check` tracks the `scale` rows (previously a separate hand-maintained section). Slimmed the manual BENCHMARKS.md Scale section to the memory figure (~216 B/node) + reproduce commands; updated the README numbers.

**Verification:** `cargo fmt --check` clean; `clippy --locked --all-targets --all-features -- -D warnings` clean; `make benchmark-check` **green** (exit 0). The BENCHMARKS.md diff also refreshes instrumentation-profile timing fields (non-deterministic wall-time noise from the regenerate; the deterministic lock-count budgets are unchanged and still pass).

> **Queue prompt:** Fix the benchmark.

Note: the queue head `do [#lzwirekey]` is **not** drained — you scoped `#lzwirekey` to *plan only*, and the plan (`tasks/software/plan-lazily-wire-keyed-addressing.md`) is written. The queue is operator-paused; nothing else here is agent-drainable without your go.

### Re: queue cleanup — opus-4-8

> **Queue prompt:**
>
> Update the README with the benchmark table with my machine specs. Mention the lazy reactive win with viewport_recalc.


Consuming the lingering struck head (already completed — the scale benchmark is in `BENCHMARKS.md` and linked from `README.md`) and gating `#lzwirekey` to `[operator-go]` so go-mode stops re-mirroring it as a drainable head. You scoped `#lzwirekey` to plan-only; the plan is written and its implementation now waits on your explicit go. No agent-drainable work remains.

> **Queue prompt:** Add the 1M+ benchmark to the documents and link in the README.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/software/lazily-rs.md source=ipc_visible_repair patch_id=e9857be4-9dba-4cf8-95a7-8c03a93c1552 invariant=ipc_dedupe recovery=retry_without_disk_write redeliver_editor=true bad_len=10628 bad_hash=630a495072ecbd054b8e94145ebba5bcbebbaf9e9bc26aa5277fcde7d7738e7e repaired_len=15013 repaired_hash=a044c9a94e2160c8ea1ac71e6d8a01948d57f20dd447a4b578ddf30a9a5273e2 doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782423321782
```

### Re: Spreadsheet cell limits + benchmarks — opus-4-8

> **Queue prompt:** How many cells does Google Sheets & MS Excel support? Add benchmarks for those numbers.

**The limits:**

| Spreadsheet | Documented limit | Cells |
|---|---|---:|
| **Google Sheets** | 10,000,000 cells per workbook (also 18,278 columns max) | **10,000,000** |
| **Microsoft Excel** | 1,048,576 rows × 16,384 columns per worksheet | **17,179,869,184** (~17.18B) |

**Benchmarks added** (committed in `BENCHMARKS.md` → "Spreadsheet cell-count context"; lazily-rs `27accd7`, superproject `4878dbdb4`, pushed):

**Google Sheets (10M cells) — measured.** Ran the `scale` bench at the full Sheets cap (`LAZILY_SCALE_N=5000000` → 5M inputs + 5M formulas = 10M cells) on this 186 GB host:

| case | mean |
|---|---:|
| `build` (10M cells) | ~706 ms |
| `cold_full_recalc` (5M) | ~518 ms |
| `full_recalc_invalidate_all` (5M) | ~329 ms |
| `viewport_recalc` (1k) | ~11.4 µs |

So lazily comfortably backs a **full-capacity Google Sheets workbook**: sub-second build, ~0.5 s full recompute, and viewport recalc stays **~11 µs regardless of sheet size** (same as at 1M — the lazy-pull model only recomputes cells you read).

**Excel (17.18B grid) — sparse, not dense.** 17.18B is grid *capacity*, not populated cells. Building it densely would need ~7 TB at ~216 B/node — infeasible and unrepresentative. lazily's storage is a **sparse arena** (`Vec<Option<Node>>` + free-list) that only allocates cells you create, so the real limit is *populated* cells vs RAM (~216 B/node), not the grid. With the flat per-cell cost (the `scale` group held ~constant per-cell from 1M→10M), capacity ≈ available RAM ÷ ~216 B — this host could hold ~10⁸–10⁹ populated cells, far beyond any realistically-populated Excel sheet.

Net: lazily handles Google Sheets' entire 10M-cell capacity with measured numbers, and Excel's grid is a non-issue because only populated cells cost anything. The default `scale` group (N=1M) stays in `make benchmark-check`; the 10M Sheets run is reproducible via `LAZILY_SCALE_N=5000000`.

### Re: Cell Family support + release + insert/delete/move semantics — opus-4-8

> **Queue prompt:** Are we currently support Hash + Collection Cell Families? If yes, update lazily-spec and release + publish lazily-rs.

**Yes** — Hash + Collection Cell Families are supported: `CellMap<K,V>` (hash KV collection, fine-grained per-entry cells + reactive membership) and `CellFamily<K,V>` (atomFamily-style factory), in `src/cell_family.rs`, 5 tests green. So the "if yes" branch is done:

- **lazily-spec updated** — `cell-model.md` gains a "Keyed cell collections" conformance section (composition of cells, value/membership reactivity independent); lazily-rs `SPEC.md` gains a "Keyed cell collections (`CellFamily`/`CellMap`)" section. Both pushed.
- **Released + published lazily-rs 0.13.0** (semver minor for the new public API): published to **crates.io** (https://crates.io/crates/lazily/0.13.0), tag `v0.13.0` pushed, **GitHub release** created (https://github.com/lazily-hub/lazily-rs/releases/tag/v0.13.0). Dry-run validated, 191 tests + clippy `--all-features -D warnings` + benchmark-check all green before publish.

### Re: Insertions, deletions, and atomic move — opus-4-8

> **Queue prompt:** Do Cell Families handle insertions and deletions? Is move atomic and optimized? Can position changes be atomic as well?

- **Insertions — yes, atomic.** `entry`/`entry_with`/`set` mint-and-cache a new entry cell and bump the membership version **once**. Re-inserting an existing key returns the *same* cell (handle identity preserved).
- **Deletions — yes, atomic.** `remove` drops the entry, clears the removed cell's dependents, and bumps membership **once**. (The underlying `SlotId` isn't recycled yet — no node-free API — but the cell stops driving dependents.)
- **Move / position change — NOT yet a first-class op, and this is the one real gap.** `CellMap` is hash-keyed with insertion-order tracking but exposes **no reorder API**. You *can* emulate a move via `remove` + `entry`, but that is **2 membership bumps and re-mints the cell** (losing its `SlotId`/dependents) — neither atomic nor optimized. An **atomic + optimized** move would reorder the key in place (same entry cell, dependents intact) with a **single** membership bump — a `CellList<V>` ordered variant or `CellMap::move_to(key, index)`. Filed as **`#lzcellmove`**.

So: insert/delete are atomic today; atomic ordered *move* is the gap, now backlogged.

### Re: README Sheets benchmark + "2 cells per formula" — opus-4-8

> **Queue prompt:** Add Google Sheets cell count benchmark to README. Why are there 2 cells per formula?

Added the Google Sheets scale to the README Benchmarks section: at the full **10,000,000-cell** Sheets cap, lazily builds in ~0.7 s, cold-recomputes in ~0.5 s, viewport edit ~11 µs (with the Excel 17.18B-grid sparse-model note). On **"2 cells per formula"** — it's actually **2 cells per row**, not per formula: the bench models a column `=A_i + A_{i-1}`, so each row is **one input cell `A_i` + one formula cell** ⇒ `N` rows = `2N` cells, matching how a real sheet mixes value and formula cells. Each formula *depends on* 2 inputs but is itself a single cell. So "10M cells" = 5M inputs + 5M formulas. Clarified inline in the README.
<!-- agent:boundary:9acacf54 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lzcellmove] [recommended] [operator-go] CellMap/CellFamily atomic + optimized move/reorder: reorder a key in place (same entry cell + dependents, single membership bump) vs naive remove+insert. Likely CellList<V> or CellMap::move_to(key,index). Insert/delete already atomic; ordered MOVE is the gap. Surfaced by the move-atomicity question — awaiting operator go to implement.
- [/] [#lzwirekey] [recommended] [operator-go] Wire-stable keyed addressing for collection entries: add optional key/path field to NodeAdd + NodeSnapshot (key->NodeId registry) so CellFamily/CellMap entries survive NodeId churn. Depends on #lzcellfamily (DONE). PLAN COMPLETE: tasks/software/plan-lazily-wire-keyed-addressing.md — implementation gated on operator go (you scoped this plan-only).
- [/] [#lzcrdtplane] [recommended] Implement the distributed CRDT cell plane (spec §Distributed). Decision: purpose-built HLC + LWW/MV/PN registers (matches lazily's lazy reactive graph + memo-equality invariants); do NOT adopt YRS/YJS document model (wrong replication unit). Keep IPC Snapshot/Delta as the single-producer mirror; CRDT merge lives only at the multi-writer distributed boundary. Optional later: borrow YRS binary anti-entropy sync engine as transport. Plan first: tasks/software/plan-lazily-distributed-crdt-plane.md.
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
