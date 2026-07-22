// Cross-process `shm` blob backend — POSIX shm_open + mmap (#lzzcpy).
//
// This is the genuine cross-process backend: a payload spilled here lives in a
// named POSIX shared-memory region, and a peer process on the same host that
// attaches the same name resolves the descriptor zero-copy — it reads the
// mapped bytes in place, no copy across the process boundary.
//
// It requires a runtime with FFI. Three adapters call libc directly:
//   - Node  → koffi (already a dependency; koffi.view wraps the mapping as an
//             ArrayBuffer with no copy)
//   - Bun   → bun:ffi (toArrayBuffer wraps the mapping zero-copy)
//   - Deno  → Deno.dlopen (UnsafePointerView.getArrayBuffer wraps zero-copy;
//             needs --allow-ffi --unstable-ffi)
//
// This module is loaded ONLY via transport.js `createShmBackend`, which
// dynamically imports it. transport.js and index.js contain no FFI import, so
// the library bundles and runs in the browser unchanged — the browser simply
// never reaches this file and uses in_process / arrow instead.
//
// The region is a bump-allocated arena. All three adapters share the same
// binary layout (little-endian) and the ShmRegionArena logic below; only the
// map/unmap primitive differs per runtime.

import { BlobBackend, blobChecksum } from "./transport.js";
import { BlobBackendKind, ShmBlobRef } from "./index.js";

// ---------------------------------------------------------------------------
// Region layout.
// ---------------------------------------------------------------------------

const MAGIC = 0x4c5a5343; // 'LZSC' big-endian read of the first 4 bytes
const VERSION = 1;
const HEADER_SIZE = 64;
const ENTRY_HEADER = 32; // { generation, epoch, len, checksum } as u64 x4
const DEFAULT_CAPACITY = 1 << 20; // 1 MiB

// Header field offsets.
const H_MAGIC = 0; // u32
const H_VERSION = 4; // u32
const H_CAPACITY = 8; // u64
const H_EPOCH = 16; // u64
const H_GENERATION = 24; // u64
const H_CURSOR = 32; // u64

const align8 = (n) => (n + 7) & ~7;

// A bump-allocated blob arena over a mapped shared-memory region. Concurrency
// across processes is single-writer by construction here (the producer owns the
// cursor); readers only validate + view. Entries are append-only and never
// mutated in place, so a descriptor's bytes are stable for the region's life.
class ShmRegionArena {
  #view;
  #u8;

  constructor(buffer) {
    this.#view = new DataView(buffer);
    this.#u8 = new Uint8Array(buffer);
  }

  initCreate(capacity, epoch) {
    this.#view.setUint32(H_MAGIC, MAGIC, false);
    this.#view.setUint32(H_VERSION, VERSION, true);
    this.#view.setBigUint64(H_CAPACITY, BigInt(capacity), true);
    this.#view.setBigUint64(H_EPOCH, BigInt(epoch), true);
    this.#view.setBigUint64(H_GENERATION, 0n, true);
    this.#view.setBigUint64(H_CURSOR, BigInt(HEADER_SIZE), true);
  }

  validateAttach() {
    if (this.#view.getUint32(H_MAGIC, false) !== MAGIC) {
      throw new Error("shm region: bad magic (not a lazily shm arena)");
    }
    if (this.#view.getUint32(H_VERSION, true) !== VERSION) {
      throw new Error("shm region: unsupported layout version");
    }
  }

  get capacity() {
    return Number(this.#view.getBigUint64(H_CAPACITY, true));
  }

  get epoch() {
    return Number(this.#view.getBigUint64(H_EPOCH, true));
  }

  #cursor() {
    return Number(this.#view.getBigUint64(H_CURSOR, true));
  }

  write(bytes) {
    const payload = bytes instanceof Uint8Array ? bytes : Uint8Array.from(bytes);
    const entryStart = align8(this.#cursor());
    const payloadStart = entryStart + ENTRY_HEADER;
    const end = payloadStart + payload.length;
    if (end > this.capacity) {
      throw new RangeError("shm region full: grow capacity or advanceEpoch");
    }
    const generation =
      Number(this.#view.getBigUint64(H_GENERATION, true)) + 1;
    const epoch = this.epoch;
    const checksum = blobChecksum(payload);
    // Entry header.
    this.#view.setBigUint64(entryStart + 0, BigInt(generation), true);
    this.#view.setBigUint64(entryStart + 8, BigInt(epoch), true);
    this.#view.setBigUint64(entryStart + 16, BigInt(payload.length), true);
    this.#view.setBigUint64(entryStart + 24, BigInt(checksum), true);
    // Payload.
    this.#u8.set(payload, payloadStart);
    // Commit header cursor + generation.
    this.#view.setBigUint64(H_GENERATION, BigInt(generation), true);
    this.#view.setBigUint64(H_CURSOR, BigInt(end), true);
    return new ShmBlobRef({
      offset: payloadStart,
      len: payload.length,
      generation,
      epoch,
      checksum,
      backend: BlobBackendKind.Shm,
    });
  }

  // Zero-copy resolve: a Uint8Array aliasing the mapped region, or null if the
  // descriptor fails header validation (bounds / generation / epoch / len /
  // checksum). No copy, no checksum recompute — the stored checksum is compared.
  readView(ref) {
    const entryStart = ref.offset - ENTRY_HEADER;
    if (entryStart < HEADER_SIZE) return null;
    if (ref.offset + ref.len > this.capacity) return null;
    if (Number(this.#view.getBigUint64(entryStart + 0, true)) !== ref.generation)
      return null;
    if (Number(this.#view.getBigUint64(entryStart + 8, true)) !== ref.epoch)
      return null;
    if (Number(this.#view.getBigUint64(entryStart + 16, true)) !== ref.len)
      return null;
    if (Number(this.#view.getBigUint64(entryStart + 24, true)) !== ref.checksum)
      return null;
    return this.#u8.subarray(ref.offset, ref.offset + ref.len);
  }

  // Advance the region epoch and restamp every stored entry, invalidating all
  // previously-minted descriptors (their epoch no longer matches). Walks the
  // contiguous entry table from HEADER_SIZE to the write cursor.
  advanceEpoch() {
    const newEpoch = this.epoch + 1;
    let pos = HEADER_SIZE;
    const cursor = this.#cursor();
    while (pos < cursor) {
      const start = align8(pos);
      if (start + ENTRY_HEADER > cursor) break;
      const len = Number(this.#view.getBigUint64(start + 16, true));
      this.#view.setBigUint64(start + 8, BigInt(newEpoch), true);
      pos = start + ENTRY_HEADER + len;
    }
    this.#view.setBigUint64(H_EPOCH, BigInt(newEpoch), true);
  }
}

// ---------------------------------------------------------------------------
// Per-platform / per-runtime libc mapping.
// ---------------------------------------------------------------------------

function platformOf() {
  if (typeof globalThis.Deno !== "undefined") return globalThis.Deno.build.os;
  const p = globalThis.process?.platform;
  return p === "darwin" ? "darwin" : p === "win32" ? "windows" : "linux";
}

// libc name + the open-flag constants that differ across POSIX platforms.
function platformAbi() {
  const os = platformOf();
  if (os === "windows") {
    throw new Error(
      "shm backend: POSIX shm_open is unavailable on Windows; " +
        "use InProcessBackend / ArrowBackend",
    );
  }
  if (os === "darwin") {
    return { lib: "libSystem.dylib", O_CREAT: 0x0200, O_EXCL: 0x0800 };
  }
  return { lib: "libc.so.6", O_CREAT: 0o100, O_EXCL: 0o200 };
}

const O_RDWR = 2;
const PROT_READ = 1;
const PROT_WRITE = 2;
const MAP_SHARED = 1;

function normalizeName(name) {
  const n = name.startsWith("/") ? name : `/${name}`;
  // POSIX caps shm names (macOS at 31 incl. the slash); keep it defensive.
  if (n.length > 250) throw new Error("shm name too long");
  return n;
}

function cbytes(str) {
  return new TextEncoder().encode(`${str}\0`);
}

// Each mapper returns { buffer, capacity, close, unlink } where `buffer` is an
// ArrayBuffer aliasing the mapping (zero-copy).
async function mapNode(name, capacity, create) {
  const abi = platformAbi();
  const { createRequire } = await import("node:module");
  const require = createRequire(import.meta.url);
  const koffi = require("koffi");
  const libc = koffi.load(abi.lib);
  const shm_open = libc.func(
    "int shm_open(const char* name, int oflag, unsigned int mode)",
  );
  const ftruncate = libc.func("int ftruncate(int fd, long length)");
  const mmap = libc.func(
    "void* mmap(void* addr, size_t length, int prot, int flags, int fd, long offset)",
  );
  const munmap = libc.func("int munmap(void* addr, size_t length)");
  const close = libc.func("int close(int fd)");
  const shm_unlink = libc.func("int shm_unlink(const char* name)");

  const oflag = create ? O_RDWR | abi.O_CREAT | abi.O_EXCL : O_RDWR;
  const fd = shm_open(name, oflag, 0o600);
  if (fd < 0) {
    throw new Error(
      `shm_open(${name}) failed (fd=${fd})` +
        (create ? " — already exists? pass create:false to attach" : ""),
    );
  }
  if (create && ftruncate(fd, capacity) !== 0) {
    close(fd);
    shm_unlink(name);
    throw new Error("ftruncate failed sizing shm region");
  }
  const ptr = mmap(null, capacity, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  const addr = koffi.address(ptr);
  if (addr === 0n || addr === 0xffffffffffffffffn) {
    close(fd);
    if (create) shm_unlink(name);
    throw new Error("mmap failed for shm region");
  }
  const buffer = koffi.view(ptr, capacity); // zero-copy ArrayBuffer
  return {
    buffer,
    capacity,
    close: () => {
      munmap(ptr, capacity);
      close(fd);
    },
    unlink: () => shm_unlink(name),
  };
}

async function mapBun(name, capacity, create) {
  const abi = platformAbi();
  const { dlopen, FFIType, toArrayBuffer } = await import("bun:ffi");
  const libc = dlopen(abi.lib, {
    shm_open: {
      args: [FFIType.cstring, FFIType.i32, FFIType.u32],
      returns: FFIType.i32,
    },
    ftruncate: { args: [FFIType.i32, FFIType.i64], returns: FFIType.i32 },
    mmap: {
      args: [
        FFIType.ptr,
        FFIType.usize,
        FFIType.i32,
        FFIType.i32,
        FFIType.i32,
        FFIType.i64,
      ],
      returns: FFIType.ptr,
    },
    munmap: { args: [FFIType.ptr, FFIType.usize], returns: FFIType.i32 },
    close: { args: [FFIType.i32], returns: FFIType.i32 },
    shm_unlink: { args: [FFIType.cstring], returns: FFIType.i32 },
  });
  const c = libc.symbols;
  const nameC = cbytes(name);
  const oflag = create ? O_RDWR | abi.O_CREAT | abi.O_EXCL : O_RDWR;
  const fd = c.shm_open(nameC, oflag, 0o600);
  if (fd < 0) {
    throw new Error(`shm_open(${name}) failed (fd=${fd})`);
  }
  if (create && c.ftruncate(fd, capacity) !== 0) {
    c.close(fd);
    c.shm_unlink(nameC);
    throw new Error("ftruncate failed sizing shm region");
  }
  const ptr = c.mmap(null, capacity, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  if (!ptr || ptr === -1) {
    c.close(fd);
    if (create) c.shm_unlink(nameC);
    throw new Error("mmap failed for shm region");
  }
  const buffer = toArrayBuffer(ptr, 0, capacity); // zero-copy
  return {
    buffer,
    capacity,
    close: () => {
      c.munmap(ptr, capacity);
      c.close(fd);
    },
    unlink: () => c.shm_unlink(nameC),
  };
}

async function mapDeno(name, capacity, create) {
  const abi = platformAbi();
  const Deno = globalThis.Deno;
  const libc = Deno.dlopen(abi.lib, {
    shm_open: { parameters: ["buffer", "i32", "u32"], result: "i32" },
    ftruncate: { parameters: ["i32", "i64"], result: "i32" },
    mmap: {
      parameters: ["pointer", "usize", "i32", "i32", "i32", "i64"],
      result: "pointer",
    },
    munmap: { parameters: ["pointer", "usize"], result: "i32" },
    close: { parameters: ["i32"], result: "i32" },
    shm_unlink: { parameters: ["buffer"], result: "i32" },
  });
  const c = libc.symbols;
  const nameC = cbytes(name);
  const oflag = create ? O_RDWR | abi.O_CREAT | abi.O_EXCL : O_RDWR;
  const fd = c.shm_open(nameC, oflag, 0o600);
  if (fd < 0) throw new Error(`shm_open(${name}) failed (fd=${fd})`);
  if (create && c.ftruncate(fd, BigInt(capacity)) !== 0) {
    c.close(fd);
    c.shm_unlink(nameC);
    throw new Error("ftruncate failed sizing shm region");
  }
  const ptr = c.mmap(
    null,
    BigInt(capacity),
    PROT_READ | PROT_WRITE,
    MAP_SHARED,
    fd,
    0n,
  );
  if (ptr === null) {
    c.close(fd);
    if (create) c.shm_unlink(nameC);
    throw new Error("mmap failed for shm region");
  }
  const buffer = new Deno.UnsafePointerView(ptr).getArrayBuffer(capacity); // zero-copy
  return {
    buffer,
    capacity,
    close: () => {
      c.munmap(ptr, BigInt(capacity));
      c.close(fd);
      libc.close();
    },
    unlink: () => c.shm_unlink(nameC),
  };
}

async function mapRegion(name, capacity, create) {
  if (typeof globalThis.Deno !== "undefined") {
    return mapDeno(name, capacity, create);
  }
  if (typeof globalThis.Bun !== "undefined") {
    return mapBun(name, capacity, create);
  }
  return mapNode(name, capacity, create);
}

// ---------------------------------------------------------------------------
// ShmBackend — the BlobBackend adapter over a mapped region.
// ---------------------------------------------------------------------------

// The `shm` BlobBackend. Descriptors it mints carry backend = "shm" (the wire
// default, omitted on serialization). Close it to release the mapping; the
// creator additionally unlinks the name.
export class ShmBackend extends BlobBackend {
  #arena;
  #region;
  #owner;

  constructor(arena, region, owner) {
    super();
    this.#arena = arena;
    this.#region = region;
    this.#owner = owner;
  }

  get kind() {
    return BlobBackendKind.Shm;
  }

  get name() {
    return this.#region.name;
  }

  get capacity() {
    return this.#arena.capacity;
  }

  get epoch() {
    return this.#arena.epoch;
  }

  write(bytes) {
    return this.#arena.write(bytes);
  }

  readView(ref) {
    return this.#arena.readView(ref);
  }

  advanceEpoch() {
    this.#arena.advanceEpoch();
  }

  // Release the mapping. The creating process also unlinks the shared name so
  // the region is reclaimed once every attacher has unmapped.
  close() {
    this.#region.close();
    if (this.#owner) this.#region.unlink();
  }
}

// Open (create or attach) a cross-process `shm` backend. Called only through
// transport.js `createShmBackend`. Options: { capacity, create, epoch }.
export async function openShmBackend(name, options = {}) {
  const create = options.create ?? true;
  const capacity = options.capacity ?? DEFAULT_CAPACITY;
  const epoch = options.epoch ?? 0;
  const normalized = normalizeName(name);
  const region = await mapRegion(normalized, capacity, create);
  region.name = normalized;
  const arena = new ShmRegionArena(region.buffer);
  if (create) {
    arena.initCreate(region.capacity, epoch);
  } else {
    arena.validateAttach();
  }
  return new ShmBackend(arena, region, create);
}
