# Plan: `lazily-cpp` wasm build, feature coverage, and benchmarks (`#lzcppwasm`)

**Status:** Design — not started. No wasm target exists today.
**Created:** 2026-07-30
**Session:** `tasks/software/lazily.md`
**Backlog:** `#lzcppwasm`
**Related:** `tasks/software/plan-lazily-gd.md` (`#lzgdbinding`) — the header-closure
analysis below came out of scoping that plan.

## Goal

Give `lazily-cpp` a first-class **Emscripten/wasm32 target**: a build, an honest
per-family capability matrix, and benchmark numbers alongside the existing native
ones. Today CI is `ubuntu-latest` only and nothing is proven on wasm — the closures
below are *unobstructed*, not *tested*, and that distinction is the whole reason
this plan exists.

## Findings that motivate it (measured 2026-07-30)

`lazily-cpp` is a header-only C++17 `INTERFACE` library — only `src/ffi.cpp` and
`src/interop_peer.cpp` compile. Transitive `lazily/*` include closures scanned for
POSIX/threading dependencies:

| Entry header | lazily headers | wasm-hostile deps |
|---|---|---|
| `cell.hpp` | 8 | **none** (pure `<memory>`/`<utility>`) |
| `collections.hpp` | 11 | **none** |
| `keyed_order.hpp` | 1 | **none** |
| `statechart.hpp` | 9 | **none** |
| `work_queue.hpp` | 12 | `<thread>`, `std::thread`, `<mutex>`, `<atomic>` |
| `lazily.hpp` (umbrella) | 46 | above **plus** `shm_open`/`mmap`/`unistd.h` |

Offenders: `transport.hpp` (`ShmBackend`: `::shm_open` + `mmap`, needing the
`_GNU_SOURCE` define set in `CMakeLists.txt` for Linux), `reliable_sync.hpp`
(`unistd.h`), `thread_safe.hpp`, `async_context.hpp`.

So the reactive core cross-compiles unobstructed — **but only if a consumer includes
narrow headers deliberately.** `lazily.hpp` is what a naive consumer reaches for,
and it drags in POSIX shared memory.

## Explicit non-goal: splitting the headers

Header line counts: `context.hpp` 2,117, `ingress.hpp` 1,750, `codec.hpp` 1,104,
`queue.hpp` 781, `reliable_sync.hpp` 761 — 18,630 total. `cell.hpp` is already small
(outside the top 18). **Do not split any of these as part of this work:**

- Header size costs *compile* time, not runtime loading. There is no module loading
  in a compiled binary, so the "efficient loading" concern does not transfer from
  the GDScript side.
- The dependency hazard is already solved at **file** granularity. `cell.hpp`'s
  closure is clean. Splitting `context.hpp` would not improve it; the umbrella is
  the problem and the fix is not including the umbrella.
- It would churn a released library (cpp v0.25.1) for no behavioral gain.

Revisit only if a *measurement* on a real consumer says compile time hurts, and
then split `context.hpp` specifically and nothing else.

## Phases

### Phase 1 — `lazily/core.hpp`: name the narrow contract

Smallest real win, additive, no churn. Add a `lazily/core.hpp` that includes only
the verified wasm-clean subset (`cell` + `collections` + `keyed_order` +
`statechart`), so the narrow-include contract is a named entry point instead of
tribal knowledge. Document in `AGENTS.md` and `README.md` that `lazily.hpp` is the
native-only umbrella.

Exit: `core.hpp` compiles standalone with a POSIX-free include check.

### Phase 2 — Emscripten toolchain + three build tiers

Add an `emcmake`/`emsdk` path to `CMakeLists.txt` and Makefile, producing three
explicitly-labeled tiers rather than one build that silently drops features:

| Tier | Contents | Emscripten flags |
|---|---|---|
| **core** | `core.hpp` subset | none special; single-threaded |
| **threaded** | + `work_queue`, `thread_safe`, `async_context` | `-pthread` (needs SharedArrayBuffer + COOP/COEP hosting headers) |
| **native-only** | `transport`, `reliable_sync` | **excluded** — `shm_open`/`mmap` have no meaningful Emscripten implementation |

A compile-time guard should make the native-only tier fail loudly under
`__EMSCRIPTEN__` rather than fail obscurely at link time.

Exit: `make wasm-core` and `make wasm-threaded` build clean; `make check` unaffected
on native.

### Phase 3 — Feature coverage matrix, replayed not asserted

The point of this phase is a matrix backed by **replayed fixtures**, not a
hand-authored table. Same discipline as every other coverage claim in the family.

- Run the existing conformance suite under Node (or headless browser) against each
  wasm tier, using the same fixtures from `lazily-spec/conformance/`.
- Reuse the **runtime manifest** mechanism (`build/conformance-fixtures-loaded.txt`
  + `scripts/check-conformance-coverage.sh`): a fixture the wasm run did not
  actually read must not be reported as covered. `lazily-cpp`'s own queue tests
  were previously hand-transcribed rather than replayed — precisely the drift the
  runtime manifest exists to catch, and precisely the mistake to avoid repeating on
  a new target.
- Produce a per-family × per-tier table, with every "absent" cell carrying a reason
  (needs threads / needs POSIX shm / not yet replayed). No blank cells.
- **Do not** flip anything in `lazily-spec/coverage.json` from this work.
  `coverage.json` tracks language bindings, not build targets of one binding; the
  wasm matrix lives in `lazily-cpp` (`README.md` or a new `WASM.md`).

Exit: matrix generated from the manifest, CI-enforced, with a positive assertion
that fixtures ran.

### Phase 4 — Benchmarks

`benches/bench_main.cpp` is a custom harness (not Google Benchmark) driven by
`make bench`, and `BENCHMARKS.md` is generated between
`<!-- benchmark-results:start -->` markers. Extend rather than replace:

- Run `bench_main` under wasm and record results in a **separate section** of
  `BENCHMARKS.md`, with its own environment line (emsdk version, single-threaded vs
  `-pthread`, browser/Node runtime). Native `-O3 -DNDEBUG` numbers and wasm numbers
  are not comparable and must not share a table.
- Expect a large gap; the deliverable is a *documented baseline*, not parity.
- Leave `benches/pubsub_load.cpp` out: it is `EXCLUDE_FROM_ALL`, unregistered with
  ctest, and its top rungs allocate tens of GiB — it will not run in a browser and
  should not be forced to.
- Bump the version line in `BENCHMARKS.md` per release convention.

Exit: `BENCHMARKS.md` carries a wasm section generated by the same generator path.

### Phase 5 — CI

Add a wasm job to `.github/workflows/` (currently `ubuntu-latest` native only):
build core + threaded tiers, replay the conformance suite, upload the matrix.
Benchmarks stay on-demand, not a CI gate.

## Downstream consumers this unblocks

- A future `lazily-godot` engine-integration layer wanting a desktop **and** web
  GDExtension fast path. Note this does not remove the console blocker that made
  `lazily-gd` pure GDScript — see `plan-lazily-gd.md`.
- Any browser consumer currently forced through `lazily-js`.

## Open questions

1. Node vs headless-browser for the conformance and bench runs. Node is simpler;
   browser is more honest about the threaded tier's COOP/COEP requirement.
2. Whether the threaded tier is worth shipping at all given the hosting-header
   burden it imposes on consumers.
3. Whether `lazily/core.hpp` (Phase 1) should ship on its own as a patch release
   ahead of the rest of this plan — it is independently useful.
