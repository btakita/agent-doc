# Plan: lazily-cpp cross-process zero-copy transport + v0.7.0

**Status:** Spec + formal shipped (lazily-spec `3b4de4d`, lazily-formal `0e446a3`,
superproject `008c84947`). This plan = the lazily-cpp adapter implementation,
conforming to that spec/formal, then the v0.7.0 release.

## Task

Implement lazily-cpp's zero-copy transport per
`src/lazily-spec/docs/zero-copy-transport.md`: a `transport.hpp` with a pluggable
`BlobBackend` adapter interface + backends (in_process reusing the existing
`ShmBlobArena::read_view`, shm POSIX, arrow Apache Arrow), plus a spill/resolve
policy over `IpcValue` payloads. Then release v0.7.0 (commit + push + tag +
GitHub release + superproject submodule bump).

## Authority (already shipped — read first, conform to them)

- **SPEC:** `src/lazily-spec/docs/zero-copy-transport.md` — transport model
  (spill → descriptor → resolve), the backend/adapter CONTRACT (write /
  read_view / lifecycle: immutable stable-addressed entries, generation/epoch
  ABA + checksum).
- **SPEC wire:** `src/lazily-spec/schemas/defs.json` `ShmBlobRef` now has an
  OPTIONAL `backend` enum (shm|arrow|in_process, default shm) — backward
  compatible.
- **SPEC fixture:** `src/lazily-spec/conformance/delta_zero_copy_arrow.json` — a
  Delta with a SharedBlob payload backend="arrow" (the conformance target).
- **FORMAL:** `src/lazily-formal/LazilyFormal/ZeroCopyTransport.lean` — the
  backend-agnostic invariants the impl MUST satisfy: `resolve_write` /
  `transport_roundtrip` (spill-then-resolve identity = zero-copy),
  `resolve_wrong_backend` (kind isolation), `resolve_stale_generation` (ABA
  safety), `resolve_corrupt_checksum`.

## lazily-cpp starting point (build on it)

- `include/lazily/ipc.hpp`: `ShmBlobArena` already has `write` + zero-copy
  `read_view(ref) -> const vector<uint8_t>*` (no copy, no rehash; checksum
  cached at write) + the `ShmBlobRef` descriptor. This IS the in_process backend.
- `include/lazily/codec.hpp` + `msgpack.hpp`: the wire codec already encodes
  `IpcValue` Inline vs SharedBlob (extend to carry the optional `backend` field).
- `ROADMAP.md` + `BENCHMARKS.md`: track the arc; the transport is the last
  remaining distributed item.

## Design requirements

- `BlobBackend` abstract interface: `write(bytes)→Descriptor`,
  `read_view(Descriptor)→const bytes*` (nullptr on unknown/stale/corrupt/wrong-
  kind), lifecycle (advance_epoch; never mutate stored entries in place).
  Parameterize so the formal laws hold for every backend.
- Adapters: **in_process** (= wrap `ShmBlobArena`); **shm** (POSIX
  `shm_open`+`mmap`; Linux, `#ifdef __linux__`); **arrow** (Apache Arrow — keep
  the Arrow dependency optional/behind a CMake option so the header-only
  zero-dep default is preserved; the descriptor bytes ARE an Arrow IPC stream the
  receiver imports zero-copy).
- Spill/resolve policy: a threshold (session knob) above which `IpcValue`
  payloads spill to the backend (Inline → SharedBlob{backend=…}); receiver
  resolves SharedBlob via the backend by the `backend` discriminator.
- Backend-agnostic: the formal laws (identity, isolation, ABA, checksum) must
  hold uniformly.

## Discipline established this arc (follow it)

- **Measure-first:** add a transport benchmark (inline-copy vs spill+resolve at a
  few payload sizes; show wire-size reduction + zero-copy throughput) before
  claiming the win.
- **Honest trade-offs, opt-in where risky:** Arrow adapter optional (zero-dep
  default unchanged); document the shm Linux-only limitation.
- **Tests:** round-trip (spill→encode→decode→resolve identity), backend
  isolation, stale-generation + corrupt-checksum rejection (mirror the formal
  theorems). Add to `tests/CMakeLists.txt`. Use a `fork()` cross-process smoke
  test for the shm backend (Linux).

## Repo conventions (MUST follow)

- `src/lazily-cpp/AGENTS.md`: `make check` green, commit + push EVERY turn
  (standing rule). Stage only intended files (never secrets/private names).
- Release pattern (v0.3.0–v0.6.0): bump `CMakeLists.txt` VERSION, refresh
  `BENCHMARKS.md` + `README.md` + `ROADMAP.md`, `git tag -a v0.7.0`, push tag,
  `gh release create v0.7.0 --latest --notes-file …`, then bump the
  `src/lazily-cpp` submodule pointer in the agent-loop superproject (stage ONLY
  `src/lazily-cpp`).
- CI: `gh run list --workflow CI --limit 1` after push (record-only unless
  failing); local `make check` is the gate.

## Versions so far (context)

v0.3.0 measure-baseline+C · v0.4.0 A (opt-in RwThreadSafeContext) · v0.5.0 A2
(ScalableThreadSafeContext, near-linear reads) · v0.6.0 B+E (SmallAny inline
values + alloc-free batch) · post-0.6.0 (unreleased, on main): distributed
measure-first (CRDT unordered_map, ShmBlob checksum cache + read_view, TextCrdt
O(n²)→O(n), msgpack codec). **v0.7.0 = the zero-copy transport**, packaging all
of it.

## Steps

1. Read the SPEC doc + FORMAL model + `ipc.hpp` ShmBlobArena.
2. Add `BlobBackendKind` + optional `backend` to `ShmBlobRef` (default shm);
   extend codec for the field.
3. `transport.hpp`: `BlobBackend` interface, `InProcessBackend` (wraps
   `ShmBlobArena`), `ShmBackend` (Linux POSIX shm), spill/resolve/router.
4. Arrow = consumer-side adapter (documented; no vendored dep).
5. Tests (round-trip identity, isolation, ABA, checksum; fork() shm smoke).
6. Benchmark (inline vs spill+resolve).
7. `make check` green → docs → commit + push + tag v0.7.0 + release + submodule bump.
