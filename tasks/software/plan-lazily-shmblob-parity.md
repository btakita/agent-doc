# Plan: `ShmBlobArena` parity for lazily-py / lazily-zig (`#lzshmblobparity`)

**Status:** Scoped — gated on operator confirming "ShmBlobArena parity" (not Lean proof extraction) is the desired goal. Resolves the `#lzffileanrs` / `#lzffileanpy` / `#lzffileanzig` clarification cluster (#pwvm / #9e64 / #t1xt / #dcgp / #7gpg / #kjfe).
**Created:** 2026-06-24
**Session:** `tasks/software/lazily-rs.md`
**Reference impl:** `src/lazily-rs/src/ipc.rs` (`ShmBlobArena<B>` L150, `ShmBlobRef` L32)

## Clarification result (the answer to the #pwvm cluster)

Two of the three candidate goals are already satisfied; one is a real gap:

| Goal | Status | Evidence |
|---|---|---|
| **IPC.lean protocol** (Snapshot / Delta / DeltaOp / epoch sequencing) | ✅ DONE in all siblings + formally proven | `IPC.lean` proves `applyDelta`, `isSequentialAfter`, memo/signal silence, batch flush; rs/py/zig `ipc.*` all implement the wire types. |
| **ShmBlobArena parity** | ⚠️ REAL GAP (this plan) | Only lazily-rs hosts an arena. py/zig are ref-only consumers. |
| **Lean → Rust proof extraction** | ❌ Not indicated | `IPC.lean` is spec + theorems over `Payload := Nat`; it deliberately omits the ShmBlob transport. Extraction is a separate research-scale effort with no current consumer. |

### The gap (concrete)

lazily-rs `ShmBlobArena<B>` (`ipc.rs:150`) is a owning shared-memory arena:
- `with_capacity(capacity)` / `from_buffer(bytes)` — allocate or wrap a region
- `write_blob(...)` (L220) — append a blob, return a `ShmBlobRef` descriptor
- `read_blob(descriptor)` (L269) — read a blob back by descriptor
- `capacity()` / `max_blob_len()` / `write_offset()` — introspection

lazily-py (`ipc.py`) and lazily-zig (`ipc.zig`) have **only** `ShmBlobRef` + `SharedBlob` wire tagging — they can *deserialize* a descriptor produced by a Rust host and carry it in `IpcValue`/`NodeState`, but they **cannot host** (allocate/write/read) an arena themselves. A py/zig process is a second-class blob consumer today: it must round-trip through Rust (or some other arena host) to materialize blob payloads.

## Goal

Port `ShmBlobArena` host capability to lazily-py and lazily-zig so any sibling can both produce and consume shared-memory blob payloads — eliminating the Rust-only asymmetry.

## Design

The arena is a flat byte buffer + an append-only write cursor + fixed-size `ShmBlobRef { offset, len }` descriptors. No allocator pool, no free-list (append-only, matching the rs semantics). Each sibling ports the rs API 1:1.

### lazily-py (`src/lazily-py/src/lazily/ipc.py`)

- `class ShmBlobArena:` wrapping a `bytearray` buffer.
  - `__init__(self, capacity_or_buffer)` — `with_capacity(n)` / `from_buffer(buf)` overload.
  - `write_blob(self, data: bytes) -> ShmBlobRef` — append, return descriptor.
  - `read_blob(self, ref: ShmBlobRef) -> memoryview` — zero-copy view into the buffer.
  - `capacity`, `max_blob_len`, `write_offset` properties.
- Reuse the existing `ShmBlobRef` dataclass (L91); add `offset`/`len` fields if not present.
- Backing: `bytearray` + `memoryview` (no native extension — keeps the pure-Python install story that #tnbd is trying to preserve by moving off ffi-napi).
- Tests: write/read round-trip, over-capacity error, `from_buffer` aliasing, integration with `IpcValue.SharedBlob` / `NodeState_SharedBlob` wire round-trip.

### lazily-zig (`src/lazily-zig/src/lazily/ipc.zig`)

- `pub const ShmBlobArena = struct { buffer: []u8, write_offset: usize, ... }`.
  - `init(allocator, capacity)` / `fromBuffer(buffer)`.
  - `writeBlob(self, data) !ShmBlobRef` — append, return descriptor.
  - `readBlob(self, ref) []const u8` — slice into the buffer.
  - `capacity()` / `maxBlobLen()` / `writeOffset()`.
- Reuse the existing `ShmBlobRef` struct (L38).
- Backing: `ctx.allocator.alloc(u8, capacity)` (or accept an externally-owned buffer for true shm).
- Tests: write/read round-trip, over-capacity `error.OutOfMemory`, `fromBuffer` aliasing, `IpcValue.sharedBlob` / `NodeState.sharedBlob` wire round-trip.

### Wiring

- py: export `ShmBlobArena` from `ipc.py` `__all__` + `lazily/__init__.py`.
- zig: export `ShmBlobArena` from `root.zig`.
- rs: unchanged (reference impl).

## Phases

1. **py arena** — `ShmBlobArena` + tests + exports.
2. **zig arena** — `ShmBlobArena` + tests + exports.
3. **Cross-sibling conformance** — add a shared `lazily-spec` IPC conformance test for the arena API (write/read round-trip, descriptor stability) so all three siblings stay in lockstep. Mirror the existing `lazily-spec` IPC conformance harness.

## Out of scope

- Lean proof extraction (separate effort; IPC.lean intentionally abstracts Payload).
- True OS shared memory (`/dev/shm`, mmap) — this plan ports the in-process arena; real cross-process shm is a follow-on that swaps the backing buffer for an mmap region in each sibling.
- Changing `ShmBlobRef` wire format (stable; siblings already agree).
