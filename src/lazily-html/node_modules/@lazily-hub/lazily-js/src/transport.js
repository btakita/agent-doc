// Cross-process zero-copy transport — pluggable blob backends (#lzzcpy).
//
// Spec:   ../../lazily-spec/docs/zero-copy-transport.md
// Formal: ../../lazily-formal/LazilyFormal/ZeroCopyTransport.lean
// Rust reference: ../../lazily-rs/src/transport.rs
//
// A large payload is not copied through the wire codec. The producer **spills**
// it to a blob backend (the backend mints a ShmBlobRef descriptor) and ships
// only the descriptor; the receiver **resolves** the descriptor against the
// same backend and reads the bytes in place — zero copy. `BlobBackend` is the
// adapter seam:
//
//   - InProcessBackend wraps a ShmBlobArena — single address space (the FFI
//     host / an in-process binding loaded on the same thread).
//   - ArrowBackend holds Apache Arrow IPC stream bytes — the descriptor's bytes
//     ARE an Arrow IPC stream a columnar consumer imports as an Array /
//     RecordBatch with no copy (bring your own Arrow reader around the resolved
//     view).
//   - the `shm` backend (src/shm-backend.js) is a POSIX shm_open + mmap region
//     — the genuine cross-process backend (same host). It requires a runtime
//     with FFI (Node/Bun/Deno) and is loaded lazily via `createShmBackend`, so
//     THIS module stays isomorphic: it imports no FFI and runs unchanged in the
//     browser, where in_process + arrow + the descriptor/router/spill/resolve
//     policy are fully available.
//
// Because the formal laws (spill-then-resolve identity, backend isolation, ABA
// generation safety, checksum integrity) are stated only over a backend's
// issued-blob table, they hold uniformly for every backend that maintains the
// BlobBackend contract — including a browser-only `in_process` deployment.

import {
  BlobBackendKind,
  CrdtSync,
  Delta,
  DeltaOpCellSet,
  DeltaOpNodeAdd,
  DeltaOpSlotValue,
  IpcMessage,
  IpcValueInline,
  IpcValueSharedBlob,
  NodeSnapshot,
  NodeStatePayload,
  NodeStateSharedBlob,
  ShmBlobRef,
  Snapshot,
} from "./index.js";

export { BlobBackendKind };

// The default byte size at or above which spillValue / spillMessage move an
// inline payload to a backend. A deployment knob, not a protocol constant:
// payloads below the threshold stay Inline (copying a tiny value through the
// codec is cheaper than a backend round-trip). Callers may pass their own.
export const DEFAULT_SPILL_THRESHOLD = 512;

// ---------------------------------------------------------------------------
// Checksum — 53-bit-reduced FNV-1a-64.
// ---------------------------------------------------------------------------

const FNV_OFFSET = 0xcbf29ce484222325n;
const FNV_PRIME = 0x100000001b3n;
const U64_MASK = (1n << 64n) - 1n;
// JS numbers are exact only to 2^53; the descriptor's `checksum` is a Number,
// so we fold the 64-bit FNV-1a into 53 bits. This is self-consistent (write and
// readView reduce identically) and keeps every descriptor field a safe integer.
// It is deliberately NOT byte-compatible with the rs/py/zig arena_blob fixture
// (which pins a full u64) — the JS binding is the Worker/isolate model, outside
// that byte-compat set, exactly as the Dart binding is.
const CHECKSUM_MASK = (1n << 53n) - 1n;

// The 53-bit-reduced FNV-1a-64 of `bytes`, as a non-negative safe integer.
export function blobChecksum(bytes) {
  let hash = FNV_OFFSET;
  for (let i = 0; i < bytes.length; i += 1) {
    hash ^= BigInt(bytes[i]);
    hash = (hash * FNV_PRIME) & U64_MASK;
  }
  return Number(hash & CHECKSUM_MASK);
}

function asBytes(value) {
  if (value instanceof Uint8Array) return value;
  return Uint8Array.from(value);
}

// ---------------------------------------------------------------------------
// ShmBlobArena — in-process blob storage with header validation.
// ---------------------------------------------------------------------------

// An in-process blob arena. It hands out ShmBlobRef descriptors that callers
// exchange over the control transport in place of large inline payloads. The
// backing store is a plain array of immutable entries (JS has one address space
// per thread — a descriptor handed to any code on the thread resolves against
// the same store). A genuine cross-process store is the `shm` backend.
//
// Entries are immutable and stably indexed for the lifetime a descriptor may
// reference them: write never mutates a stored buffer, and advanceEpoch /
// free invalidate a descriptor rather than repurposing its bytes. That is what
// makes readView a sound zero-copy primitive.
export class ShmBlobArena {
  #epoch;
  #generation = 0;
  #entries = []; // offset === index; a freed slot is null

  constructor({ epoch = 0 } = {}) {
    this.#epoch = epoch;
  }

  get epoch() {
    return this.#epoch;
  }

  // Number of live (unfreed) blobs.
  get length() {
    let n = 0;
    for (const e of this.#entries) if (e !== null) n += 1;
    return n;
  }

  get isEmpty() {
    return this.length === 0;
  }

  // Allocate a blob and return its descriptor. The bytes are copied into the
  // arena; the returned ShmBlobRef is the header a reader validates.
  write(bytes) {
    const payload = asBytes(bytes).slice();
    this.#generation += 1;
    const entry = {
      generation: this.#generation,
      epoch: this.#epoch,
      payload,
      checksum: blobChecksum(payload),
      refCount: 1,
    };
    const offset = this.#entries.length;
    this.#entries.push(entry);
    return this.#toRef(offset, entry);
  }

  #toRef(offset, entry) {
    return new ShmBlobRef({
      offset,
      len: entry.payload.length,
      generation: entry.generation,
      epoch: entry.epoch,
      checksum: entry.checksum,
    });
  }

  #validEntry(ref) {
    if (ref.offset < 0 || ref.offset >= this.#entries.length) return null;
    const entry = this.#entries[ref.offset];
    if (entry === null) return null;
    if (entry.generation !== ref.generation) return null;
    if (entry.epoch !== ref.epoch) return null;
    if (entry.payload.length !== ref.len) return null;
    if (entry.checksum !== ref.checksum) return null;
    return entry;
  }

  // Resolve a descriptor zero-copy: return the arena's own backing bytes (NOT a
  // copy) iff the descriptor passes full header validation (offset in range,
  // slot live, matching generation / epoch / len / checksum); null otherwise.
  // The returned Uint8Array aliases arena storage.
  readView(ref) {
    const entry = this.#validEntry(ref);
    return entry === null ? null : entry.payload;
  }

  // Resolve a descriptor and return a defensive copy (or null). Use when the
  // bytes must outlive a possible free / epoch advance.
  read(ref) {
    const view = this.readView(ref);
    return view === null ? null : view.slice();
  }

  // Increment a live blob's reference count; false if the descriptor is stale.
  retain(ref) {
    const entry = this.#validEntry(ref);
    if (entry === null) return false;
    entry.refCount += 1;
    return true;
  }

  // Drop one reference; reclaim the slot (its descriptor becomes permanently
  // stale) when the count reaches zero. The index is preserved so other
  // descriptors keep resolving. false if the descriptor is stale.
  free(ref) {
    const entry = this.#validEntry(ref);
    if (entry === null) return false;
    entry.refCount -= 1;
    if (entry.refCount <= 0) this.#entries[ref.offset] = null;
    return true;
  }

  // Advance the validity epoch and restamp every live entry, so all
  // previously-minted descriptors become stale (models compaction / restart).
  advanceEpoch() {
    this.#epoch += 1;
    for (const e of this.#entries) {
      if (e !== null) e.epoch = this.#epoch;
    }
  }
}

// Validate a ShmBlobRef's fields against expected bounds: every header field
// non-negative, and `len` within `maxLen` when supplied.
export function validateBlobRef(ref, maxLen) {
  if (ref.offset < 0 || ref.len < 0) return false;
  if (ref.generation < 0 || ref.epoch < 0 || ref.checksum < 0) return false;
  if (maxLen != null && ref.len > maxLen) return false;
  return true;
}

// ---------------------------------------------------------------------------
// BlobBackend adapters.
// ---------------------------------------------------------------------------

// The adapter seam. A backend mints descriptors via write() and resolves them
// zero-copy via readView(). Subclasses report their `kind`; the arena-backed
// ones share ArenaBackend.
export class BlobBackend {
  get kind() {
    throw new Error("BlobBackend.kind must be implemented");
  }

  write(_bytes) {
    throw new Error("BlobBackend.write must be implemented");
  }

  readView(_ref) {
    throw new Error("BlobBackend.readView must be implemented");
  }

  advanceEpoch() {
    throw new Error("BlobBackend.advanceEpoch must be implemented");
  }
}

class ArenaBackend extends BlobBackend {
  #arena;

  constructor(arena) {
    super();
    this.#arena = arena ?? new ShmBlobArena();
  }

  get arena() {
    return this.#arena;
  }

  get epoch() {
    return this.#arena.epoch;
  }

  write(bytes) {
    return this.#arena.write(bytes).withBackend(this.kind);
  }

  readView(ref) {
    return this.#arena.readView(ref);
  }

  advanceEpoch() {
    this.#arena.advanceEpoch();
  }
}

// Default in-process backend: wraps a ShmBlobArena for the single-address-space
// case (the FFI host ↔ a binding loaded on the same thread). Descriptors carry
// backend = "in_process".
export class InProcessBackend extends ArenaBackend {
  get kind() {
    return BlobBackendKind.InProcess;
  }
}

// Apache Arrow blob backend: holds spilled payloads as Arrow IPC stream bytes
// and resolves a descriptor to the buffer's raw bytes with no copy. The
// descriptor's bytes ARE an Arrow IPC stream — a columnar consumer imports them
// as an Array / RecordBatch zero-copy. This adapter stores the raw stream bytes
// and tags the descriptor backend = "arrow"; bring your own Arrow reader
// (apache-arrow) to wrap the resolved view into typed Arrow.
export class ArrowBackend extends ArenaBackend {
  get kind() {
    return BlobBackendKind.Arrow;
  }
}

// ---------------------------------------------------------------------------
// BlobRouter — receiver-side multi-backend resolver.
// ---------------------------------------------------------------------------

// Holds backends by kind and resolves any descriptor by its `backend`
// discriminator — a shm descriptor routes to the shm backend, an arrow
// descriptor to the arrow backend, etc. This is the resolve_wrong_backend law
// in practice: a descriptor never resolves against a backend of the wrong kind
// (an unregistered kind resolves to null).
export class BlobRouter {
  #backends = new Map();

  // Install a backend for its kind, replacing any prior backend of that kind.
  // Returns this for chaining.
  register(backend) {
    this.#backends.set(backend.kind, backend);
    return this;
  }

  // Resolve a descriptor by routing to its backend kind. null if no backend is
  // registered for the kind, or the descriptor did not resolve.
  readView(ref) {
    const backend = this.#backends.get(ref.backend);
    return backend ? backend.readView(ref) : null;
  }

  // Resolve an IpcValue: inline bytes returned directly, a SharedBlob routed by
  // its backend discriminator and resolved zero-copy. null on failure.
  resolve(value) {
    if (value instanceof IpcValueInline) return asBytes(value.bytes);
    if (value instanceof IpcValueSharedBlob) return this.readView(value.blob);
    return null;
  }
}

// ---------------------------------------------------------------------------
// Spill policy: replace large Inline payloads with a SharedBlob descriptor.
// ---------------------------------------------------------------------------

// If `value` is an IpcValueInline of >= threshold bytes, write it to `backend`
// and return { value: <SharedBlob descriptor>, spilled: <byte count> };
// otherwise return { value, spilled: 0 }. Payloads below the threshold stay
// inline — cheaper than a backend round-trip for tiny values.
export function spillValue(value, backend, threshold = DEFAULT_SPILL_THRESHOLD) {
  if (value instanceof IpcValueInline && value.bytes.length >= threshold) {
    const ref = backend.write(value.bytes);
    return { value: new IpcValueSharedBlob(ref), spilled: value.bytes.length };
  }
  return { value, spilled: 0 };
}

function spillState(state, backend, threshold) {
  if (state instanceof NodeStatePayload && state.bytes.length >= threshold) {
    const ref = backend.write(state.bytes);
    return { state: new NodeStateSharedBlob(ref), spilled: state.bytes.length };
  }
  return { state, spilled: 0 };
}

// Resolve an IpcValue against a single backend: Inline bytes are returned
// directly, a SharedBlob is resolved zero-copy against `backend`. null when a
// SharedBlob fails to resolve (unknown / stale / corrupt). The returned view
// aliases whichever of value or backend owns the bytes.
export function resolveValue(value, backend) {
  if (value instanceof IpcValueInline) return asBytes(value.bytes);
  if (value instanceof IpcValueSharedBlob) return backend.readView(value.blob);
  return null;
}

// Spill large payloads across an IpcMessage's value/state sites — Snapshot node
// states, Delta CellSet/SlotValue payloads + NodeAdd states, and CrdtSync op
// states — returning { message, spilledBytes }. Oversized payloads are replaced
// by SharedBlob descriptors so the message stays small on the wire. Sites
// already carrying a descriptor are left untouched. The input is not mutated.
export function spillMessage(
  message,
  backend,
  threshold = DEFAULT_SPILL_THRESHOLD,
) {
  let total = 0;

  if (message.isSnapshot) {
    const snap = message.snapshot;
    const nodes = snap.nodes.map((node) => {
      const { state, spilled } = spillState(node.state, backend, threshold);
      total += spilled;
      return spilled === 0
        ? node
        : new NodeSnapshot(node.node, node.typeTag, state, node.key);
    });
    return {
      message: IpcMessage.snapshot(
        new Snapshot({
          epoch: snap.epoch,
          nodes,
          edges: snap.edges,
          roots: snap.roots,
        }),
      ),
      spilledBytes: total,
    };
  }

  if (message.isDelta) {
    const delta = message.delta;
    const ops = delta.ops.map((op) => {
      if (op instanceof DeltaOpCellSet) {
        const { value, spilled } = spillValue(op.payload, backend, threshold);
        total += spilled;
        return spilled === 0 ? op : new DeltaOpCellSet(op.node, value);
      }
      if (op instanceof DeltaOpSlotValue) {
        const { value, spilled } = spillValue(op.payload, backend, threshold);
        total += spilled;
        return spilled === 0 ? op : new DeltaOpSlotValue(op.node, value);
      }
      if (op instanceof DeltaOpNodeAdd) {
        const { state, spilled } = spillState(op.state, backend, threshold);
        total += spilled;
        return spilled === 0
          ? op
          : new DeltaOpNodeAdd(op.node, op.typeTag, state, op.key);
      }
      return op;
    });
    return {
      message: IpcMessage.delta(
        new Delta({ baseEpoch: delta.baseEpoch, epoch: delta.epoch, ops }),
      ),
      spilledBytes: total,
    };
  }

  if (message.isCrdtSync) {
    const sync = message.crdtSync;
    const ops = sync.ops.map((op) => {
      const { value, spilled } = spillValue(op.state, backend, threshold);
      total += spilled;
      return spilled === 0
        ? op
        : new op.constructor(op.node, op.stamp, value, op.key);
    });
    return {
      message: IpcMessage.crdtSync(
        new CrdtSync({ frontier: sync.frontier, ops }),
      ),
      spilledBytes: total,
    };
  }

  return { message, spilledBytes: 0 };
}

// ---------------------------------------------------------------------------
// Cross-process `shm` backend — lazily loaded, FFI-gated, browser-safe.
// ---------------------------------------------------------------------------

// Thrown when a POSIX shared-memory backend is requested in a runtime that
// cannot provide one (the browser, or a server runtime without FFI access).
export class ShmUnavailableError extends Error {
  constructor(message) {
    super(message);
    this.name = "ShmUnavailableError";
  }
}

// Whether the current runtime can host the `shm` backend (Node with koffi, Bun,
// or Deno with FFI). Cheap and synchronous — it inspects globals only, never
// imports FFI. In the browser this is false and callers fall back to
// in_process / arrow.
export function shmSupported() {
  // Deno / Bun expose a namespace global; Node exposes process.versions.node.
  if (typeof globalThis.Deno !== "undefined") return true;
  if (typeof globalThis.Bun !== "undefined") return true;
  return (
    typeof globalThis.process !== "undefined" &&
    !!globalThis.process?.versions?.node
  );
}

// Create (or attach to) a POSIX shared-memory `shm` backend named `name`
// (a leading '/' is added if absent). Cross-process on one host: a peer that
// attaches the same name resolves this backend's descriptors zero-copy.
//
// Async because the FFI adapter is dynamically imported — that keeps THIS
// module free of any FFI import, so the library bundles and runs in the browser
// unchanged. In an unsupported runtime this rejects with ShmUnavailableError;
// callers should degrade to InProcessBackend / ArrowBackend.
//
// Options: { capacity = 1 MiB, create = true, epoch = 0 }. `create: false`
// attaches to an existing region (the receiver side).
export async function createShmBackend(name, options = {}) {
  if (!shmSupported()) {
    throw new ShmUnavailableError(
      "shm backend requires Node (koffi), Bun, or Deno FFI; " +
        "use InProcessBackend / ArrowBackend in the browser",
    );
  }
  const mod = await import("./shm-backend.js");
  return mod.openShmBackend(name, options);
}
