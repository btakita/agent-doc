# Plan: lazily-cpp native backend for lazily-py (`#lzpybackend`)

**Goal:** give `lazily-py` an optional **lazily-cpp native backend** — a drop-in
accelerator behind the unchanged Python API — for faster hot-path operations and
**non-GIL multithreading** (GIL released around C++ compute + free-threaded
CPython wheels).

**Owner:** lazily-py (`src/lazily-py`) consuming lazily-cpp (`src/lazily-cpp`).
**Session:** tracked in the lazily-py session.

**Companion plan:**
[`plan-lazily-realtime-distributed-primitives.md`](plan-lazily-realtime-distributed-primitives.md)
owns the lazily-rs primitive families (rateshape / time / membership /
coordination / presence / windowing / resilience / service). That plan's
**backend-portability rules** (payload class + compute-core split + backend-blind
conformance) exist specifically so each new primitive lands in lazily-py with a
C++-eligible compute core this backend can accelerate. Read that section before
adding a primitive; this plan is where the acceleration itself is designed.

## Motivation

lazily-py's reactive graph is pure Python and GIL-bound. The hot paths are
exactly the O(N) inner loops the primitives plan adds — graph propagation, CRDT
merge, membership gossip, windowed aggregation, presence eviction sweeps, breaker
windows. lazily-cpp already ships a header-only reactive core plus
`ThreadSafeContext` / `ScalableRwLock` (true parallel graph). Route lazily-py's
hot paths through it for (a) raw single-thread speed, (b) GIL-released
parallelism on standard CPython, (c) free-threaded CPython scaling.

## Current reality (do not re-discover)

- `lazily-py` is pure Python (`uv_build`, no compiled extension). `lazily/ffi.py`
  is a *contract mirror* (ctypes-shaped status/kind discriminants + canonical
  JSON encode/decode) — it does **not** load any native library today.
- `lazily-cpp` primitives are C++ **templates** on arbitrary `T` (header-only,
  `include/lazily/*.hpp`). Its flat C-ABI (`lazily_ffi`, `src/ffi.cpp`) exposes
  only the **bytes-erased IPC / CRDT-sync frame plane** (`lazily_ffi_channel_*`,
  `lazily_ffi_ipc_message_validate/kind/clone_json`, `lazily_protocol_version`)
  — not the reactive graph.
- `lazily-cpp` already has `ThreadSafeContext` on `ScalableRwLock` — the non-GIL
  parallelism substrate. No new concurrency primitive is needed C++-side.

## Three-layer architecture

1. **Flat C-ABI (extend existing `lazily_ffi`).** Stays bytes-erased. Owns the
   wire / durable / CRDT-sync plane where payloads are already canonical
   JSON/msgpack. Reachable from Python via `ctypes`/`cffi` with **no compile
   toolchain**, so the pure-Python wheel keeps this path even with no native
   build. Low per-call rate (frame granularity) → `ctypes` marshalling overhead
   is negligible here.

2. **Type-erased reactive core (`liblazily_pyabi`, new C++ target).** A
   monomorphization of `ContextImpl<Traits>` whose value slot is a
   `PyObjectHandle` — an opaque owned pointer with incref/decref supplied by a
   **refcount vtable** passed in at init, so lazily-cpp never links libpython
   (stays libpython-agnostic; the binding layer injects the vtable). Exposes
   create-slot/cell/signal/effect, set/get/subscribe, batch, keyed by
   `uintptr_t` handles. Effects invoke a registered C function-pointer
   trampoline.

3. **nanobind binding module (`lazily._native`).** The Python-facing compiled
   extension wrapping layer 2. **nanobind, not `ctypes`, for this layer** — the
   reactive graph does millions of tiny `set`/`get`/`propagate` calls where
   `ctypes` per-call marshalling (~1µs) would erase the C++ speedup, whereas
   nanobind compiles to direct CPython C-API calls. nanobind also carries
   `PyObject*` payloads natively, manages refcounts/GC, invokes Python callables
   as effects, releases the GIL (`nb::gil_scoped_release`) around pure-C++
   propagation, and has first-class free-threading support (`nb::ft`, PEP 703).

   **Rule of thumb:** `BytesPayload` primitives → layer 1 (flat C-ABI). Value-
   generic / `PyObjectPayload` primitives → layers 2–3 (nanobind). Never route a
   per-node reactive hot path through `ctypes`.

## Backend selection (unchanged public API)

- `lazily`'s public API and imports are **unchanged**. A new `lazily/_backend.py`
  shim resolves the active backend: `native` (import `lazily._native`, verify
  ABI/protocol-version match) if available, else `python` (current pure impl).
- Env override `LAZILY_BACKEND=python|native|auto` (default `auto`). `native`
  errors loudly if the extension is missing; `python` forces the reference impl.
- **Every primitive keeps its pure-Python impl as the reference + fallback.** The
  native path is a drop-in that must pass the identical conformance suite —
  mirrors how lazily-py already mirrors the spec. No native-only behavior.

## Non-GIL strategy — two independent wins

1. **GIL released around C++ compute (standard CPython, ships first).** Any op
   whose payload is not a live Python object — bytes-valued CRDT merge, numeric
   windowed aggregate, membership Phi-accrual, timer wheels — runs under
   `gil_scoped_release`, giving real parallelism on today's interpreters.
2. **Free-threaded CPython (3.13t / 3.14t).** Build the extension with
   `Py_GIL_DISABLED`; `ThreadSafeContext` (`ScalableRwLock`) becomes the native
   concurrency substrate and native graph mutation scales with cores. Publish
   free-threaded wheels (`cp313t` / `cp314t` ABI tags). Pure-Python graph
   mutation stays serialized by construction; the heavy work lives in C++.

## Build / packaging

- Switch build-backend to **`scikit-build-core` + nanobind + CMake** for the
  native wheel while keeping a pure-Python sdist that installs and works with no
  compiler. CMake pulls lazily-cpp via the **conditional-composite pattern**
  already used for lazily-kt (sibling `../lazily-cpp` include path when present,
  vendored headers otherwise; the lazily-kt `mavenLocal()` fallback analog).
- Ship: pure-Python sdist + `abi3` binary wheels + free-threaded wheels per
  platform. If no wheel matches, pip falls back to the pure-Python sdist and the
  backend shim resolves to `python`.
- Release checklist gains: build the native wheel, then run the **full
  conformance suite under both `LAZILY_BACKEND=python` and `=native`**, plus a
  parallelism benchmark for BENCHMARKS.md.

## Backend phases

Each phase ends with the standard gate (`ruff`/`ty` + pytest under **both**
backends + `make check`) then `#spec-test-build-install-commit-push`.

- **Phase C0 — bindings foundation.** nanobind + scikit-build-core scaffold;
  `lazily._native` importable; `_backend.py` shim (`auto|python|native`); the
  `PyObjectHandle` refcount vtable + effect trampoline; CI builds a native wheel;
  pytest parametrized over backend. Prove the rails with one trivial native
  primitive (a native `Cell[int]` round-trip exercising set/get + GIL-release).
  No production primitive accelerated yet.
- **Phase C1 — bytes-plane acceleration (lowest risk, biggest GIL-free win,
  ships before the distributed primitives).** Route the already-bytes-erased
  planes through layer 1 / native: CRDT merges (`crdt_registers`, `textcrdt`,
  `crdt_tree`, `lossless_tree_crdt`), reliable-sync outbox, IPC frame
  validate/clone (already exported). GIL released. Accelerates what exists today.
- **Phase C2 — PyObject reactive core.** Native `ContextImpl<PyObjectTraits>`
  behind `Context` / `Slot` / `Cell` / `Signal` / `Effect`; refcount vtable in
  production; effects trampoline to Python callables; native batch. General
  reactive-graph acceleration; gated on C0.
- **Phase C3 — new-primitive compute cores (interleaves with the primitives
  plan).** As each primitive lands in lazily-rs and is mirrored to py, push its
  pure compute core (per the compute-core split rule) to the backend: rateshape
  samplers, time wheels (`TimerCell` / `IntervalCell`), membership
  Phi-accrual / SWIM, windowing aggregates, resilience breaker / backoff math.
  Each gated on its primitives-plan phase + C2 (or C1 for bytes-payload
  primitives).
- **Phase C4 — free-threaded build.** `Py_GIL_DISABLED` wheels
  (`cp313t` / `cp314t`); wire `ThreadSafeContext` as the native concurrency
  substrate; add a parallelism benchmark to BENCHMARKS.md showing >1-core scaling
  vs. the pure-Python GIL ceiling.

**Backend DAG:** `C0 → C1` (bytes plane, independent of the primitives plan),
`C0 → C2 → C3` (reactive core → per-primitive cores, C3 also gated per
primitives-plan phase), `C2 → C4` (free-threaded). C1 can ship as soon as C0
lands — it does not wait on any new primitive.

## Constraints / non-goals

- **Backend, not rewrite.** The lazily-cpp backend never changes the lazily-py
  public API or adds native-only behavior. It is a drop-in accelerator gated
  behind `LAZILY_BACKEND` with the pure-Python impl as the always-present
  reference + fallback. Do **not** couple lazily-cpp to libpython — the reactive
  core takes a refcount vtable so it stays libpython-agnostic. No new C++
  concurrency primitive: reuse `ThreadSafeContext` / `ScalableRwLock`.
- **Depends on primitive design, not the reverse.** This plan does not gate the
  primitives plan's critical path. It relies on that plan's backend-portability
  rules being honored per primitive; when they are, C3 acceleration is
  mechanical.

## Suggested sequence (runs alongside the primitives plan)

`#lzpybackend` is **independent of the primitives plan's critical path** and can
start immediately — its early phases accelerate what lazily-py already ships.

1. `C0` (bindings foundation) — start now; no primitives-plan dependency.
2. `C1` (bytes-plane acceleration) — right after C0; accelerates existing CRDT /
   sync / IPC planes before any new distributed primitive lands.
3. `C2` (PyObject reactive core) — after C0; general graph acceleration.
4. `C3` (per-primitive compute cores) — **interleaves**: each primitive's compute
   core is pushed to the backend as it is mirrored to lazily-py (gated on its
   primitives-plan phase + C2, or C1 for bytes-payload primitives).
5. `C4` (free-threaded wheels) — after C2; the non-GIL scaling payoff.

Recommended overall order: land `C0`+`C1` early (fast win on today's code),
proceed with the primitives plan's `#lztime`/`#lzrateshape`/`#lzmemb`, bring up
`C2`, then interleave `C3` per primitive, and finish with `C4` once the native
graph is proven under conformance.
